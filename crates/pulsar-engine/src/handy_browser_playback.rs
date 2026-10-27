//! Browser-bound original play custody and independently renewed native ownership.
use super::*;
use crate::authority::{
    browser_timing::BrowserTimingPin, handy_browser_start_qualification::AcceptedBrowserStart,
};
use crate::handy_server_clock::NativeServerClock;
use std::sync::{atomic::AtomicU64, Weak};
const MAX_OWNERS: usize = 256;

struct Selection {
    preparation: ArtifactId,
    controller: ClientId,
    acquisition: RequestId,
    clock_request: RequestId,
    runtime: Weak<Mutex<HandyPlaybackRuntime>>,
}
pub(in crate::authority::handy_playback_preparation) struct Owner {
    actor: SessionId,
    project: ProjectId,
    request_id: RequestId,
    auth_hash: String,
    timing: BrowserTimingPin,
    fingerprint: Option<String>,
    selection: Option<Selection>,
    pub(super) cancelled: AtomicBool,
    generation: AtomicU64,
}
pub(in crate::authority::handy_playback_preparation) struct BrowserPlay {
    pub(super) owner: Arc<Owner>,
    pub(super) timing: BrowserTimingPin,
    pub(super) qualification: AcceptedBrowserStart,
    pub(super) decision: HandyBrowserStartDecision,
    pub(super) original_server: NativeServerClock,
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "original browser playback owner is unavailable",
    )
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "browser playback native lineage differs",
    )
}
fn owner_proof(engine: &Engine, proof: &str) -> PResult<()> {
    let difference = engine
        .handy_owner_token
        .bytes()
        .zip(proof.bytes())
        .fold(0u8, |n, (a, b)| n | (a ^ b));
    if proof.len() != engine.handy_owner_token.len() || difference != 0 {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "Handy owner capability required",
        ));
    }
    Ok(())
}
fn auth_hash(r: &Request) -> PResult<String> {
    r.auth_token
        .as_ref()
        .map(|t| format!("{:x}", Sha256::digest(t.as_bytes())))
        .ok_or_else(unavailable)
}
fn owner_key(r: &Request, request: &RequestId) -> PResult<(SessionId, RequestId)> {
    Ok((
        r.session.as_ref().ok_or_else(unavailable)?.clone(),
        request.clone(),
    ))
}
fn owns(owner: &Owner, r: &Request, timing: &ArtifactId, hash: &str) -> bool {
    r.session.as_ref() == Some(&owner.actor)
        && r.project.as_ref() == Some(&owner.project)
        && &owner.timing.binding().binding_id == timing
        && owner.auth_hash == hash
}
fn same_selection(owner: &Owner, entry: &Entry) -> bool {
    let Some(selection) = &owner.selection else {
        return false;
    };
    let Some(lease) = &entry.controller else {
        return false;
    };
    entry.owner == owner.actor
        && lease.report.preparation_id == selection.preparation
        && lease.report.controller_id == selection.controller
        && lease.report.acquisition_request_id == selection.acquisition
        && lease.report.generation == owner.generation.load(Ordering::Acquire)
        && selection
            .runtime
            .upgrade()
            .is_some_and(|runtime| Arc::ptr_eq(&entry.runtime, &runtime))
}
pub(super) fn check_phase_coverage(
    alignment: &BrowserStartAlignment,
    binding: &BrowserTimingBinding,
    shape: &HandyScriptSampleShape,
) -> PResult<()> {
    for ns in [alignment.phase_earliest_ns, alignment.phase_latest_ns] {
        let ns = i64::try_from(ns).map_err(|_| mismatch())?;
        binding
            .browser_media
            .selection
            .placement
            .spec
            .source_at_project(ProjectTime::from_nanos(ns))
            .map_err(internal)?;
    }
    let first = shape
        .first_time_ms
        .checked_mul(1_000_000)
        .ok_or_else(mismatch)?;
    let last = shape
        .last_time_ms
        .checked_mul(1_000_000)
        .ok_or_else(mismatch)?;
    if alignment.phase_earliest_ns < first
        || alignment.phase_latest_ns > last
        || alignment.start_time_ms < shape.first_time_ms
        || alignment.start_time_ms > shape.last_time_ms
    {
        return Err(mismatch());
    }
    Ok(())
}
impl BrowserPlay {
    pub(super) fn current(&self, now: u64) -> bool {
        !self.owner.cancelled.load(Ordering::Acquire)
            && self.timing.current(now)
            && self.qualification.current()
    }
    pub(super) fn alignment(
        &self,
        pins: &play::PlayPins,
        native: Option<&crate::handy_continuous_timing::NativeContinuousTiming>,
        now: u64,
        controller_deadline: u64,
    ) -> PResult<(BrowserTimingObservation, BrowserStartAlignment)> {
        if !self.current(now) {
            return Err(unavailable());
        }
        let observation = self.timing.latest(now)?;
        let (phase_ms, device_error, valid_until) = if let Some(native) = native {
            let estimate = native.at(now)?;
            (
                estimate.observed_phase_ms,
                estimate.phase_uncertainty_ns,
                estimate.valid_until_boottime_ns,
            )
        } else {
            // Before a separately admitted ongoing device observation, the
            // original command's expiry stays authoritative despite heartbeats.
            if now >= pins.record.clock.valid_until_ns {
                return Err(unavailable());
            }
            let server = self.original_server.at(now)?;
            let delta = server
                .estimated_server_ms
                .checked_sub(pins.record.clock.estimated_server_ms)
                .ok_or_else(mismatch)?;
            let error = server
                .uncertainty_ns
                .checked_add(pins.device_clock.error_at(now)?)
                .ok_or_else(mismatch)?;
            (
                pins.record
                    .start_time_ms
                    .checked_add(delta)
                    .ok_or_else(mismatch)?,
                error,
                pins.record.clock.valid_until_ns,
            )
        };
        let mut alignment = self
            .qualification
            .subject()
            .timing
            .phase_request(
                &observation,
                now,
                self.qualification.script_quantization().max_time_error_ns,
                device_error,
                valid_until.min(controller_deadline),
            )?
            .calculate()
            .map_err(internal)?;
        check_phase_coverage(
            &alignment,
            self.timing.binding(),
            &pins.captured.hosted.sample_shape(),
        )?;
        let phase_ns = phase_ms.checked_mul(1_000_000).ok_or_else(mismatch)?;
        let actual_error = alignment
            .error_against(phase_ns, device_error)
            .map_err(internal)?;
        if actual_error
            > self
                .qualification
                .subject()
                .timing
                .maximum_alignment_error_ns
            || actual_error > pins.device_clock.qualified.maximum_total_error_ns
        {
            return Err(mismatch());
        }
        alignment.start_time_ms = phase_ms;
        alignment.phase_error_ns = alignment.maximum_distance_ns(phase_ns);
        alignment.total_error_ns = actual_error;
        alignment.validate().map_err(internal)?;
        Ok((observation, alignment))
    }
    pub(super) fn original_claim_current(
        &self,
        pins: &play::PlayPins,
        now: u64,
        deadline: u64,
    ) -> PResult<()> {
        if !self.current(now) || now >= self.decision.alignment.valid_until_boottime_ns {
            return Err(unavailable());
        }
        let server = self.original_server.at(now)?;
        let error = server
            .uncertainty_ns
            .checked_add(pins.device_clock.error_at(now)?)
            .ok_or_else(mismatch)?;
        let alignment = self
            .qualification
            .subject()
            .timing
            .phase_request(
                &self.decision.observation,
                now,
                self.qualification.script_quantization().max_time_error_ns,
                error,
                self.decision
                    .alignment
                    .valid_until_boottime_ns
                    .min(deadline),
            )?
            .calculate()
            .map_err(internal)?;
        let phase = pins
            .record
            .start_time_ms
            .checked_add(
                server
                    .estimated_server_ms
                    .checked_sub(pins.record.clock.estimated_server_ms)
                    .ok_or_else(mismatch)?,
            )
            .and_then(|ms| ms.checked_mul(1_000_000))
            .ok_or_else(mismatch)?;
        let error = alignment.error_against(phase, error).map_err(internal)?;
        if error
            > self
                .qualification
                .subject()
                .timing
                .maximum_alignment_error_ns
            || error > pins.device_clock.qualified.maximum_total_error_ns
        {
            return Err(mismatch());
        }
        check_phase_coverage(
            &alignment,
            self.timing.binding(),
            &pins.captured.hosted.sample_shape(),
        )
    }
}
impl Registry {
    fn prune_browser_owners(&mut self, now: u64) {
        let retained = self
            .entries
            .values()
            .filter_map(|entry| entry.browser_owner.as_ref())
            .cloned()
            .collect::<Vec<_>>();
        self.browser_owners.retain(|_, owner| {
            owner.timing.current(now) || retained.iter().any(|kept| Arc::ptr_eq(kept, owner))
        });
    }
}
impl Engine {
    /// Root calls this before DB/live work for StartFromBrowser. A missing old
    /// native scope still reaches ordinary history replay; fresh execution then
    /// refuses without a reservation. This function never grants physical play.
    pub(in crate::authority) fn reserve_handy_browser_start_owner(
        &self,
        r: &Request,
    ) -> PResult<()> {
        self.reserve_handy_browser_start_owner_with(r, boottime_ns)
    }
    pub(in crate::authority) fn reserve_handy_browser_start_owner_with<
        F: FnOnce() -> PResult<u64>,
    >(
        &self,
        r: &Request,
        clock: F,
    ) -> PResult<()> {
        let Command::StartHandyPlaybackFromBrowser {
            acquisition_request_id,
            controller_id,
            expected_generation,
            clock_request_id,
            browser_timing_binding_id,
            owner_proof: proof,
            ..
        } = &r.command
        else {
            return Ok(());
        };
        r.validate()?;
        owner_proof(self, proof)?;
        let hash = auth_hash(r)?;
        let key = owner_key(r, &r.request_id)?;
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(r).map_err(internal)?)
        );
        {
            let registry = self.handy_preparations.lock().map_err(internal)?;
            if let Some(owner) = registry.browser_owners.get(&key) {
                if !owns(owner, r, browser_timing_binding_id, &hash)
                    || owner
                        .fingerprint
                        .as_ref()
                        .is_some_and(|old| old != &fingerprint)
                {
                    return Err(mismatch());
                }
                return Ok(());
            }
        }
        let Ok(timing) = self.browser_timing_scope(r, browser_timing_binding_id) else {
            return Ok(());
        };
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let now = registry.sweep_controllers(clock)?;
        registry.prune_browser_owners(now);
        if let Some(owner) = registry.browser_owners.get(&key) {
            if !owns(owner, r, browser_timing_binding_id, &hash)
                || owner
                    .fingerprint
                    .as_ref()
                    .is_some_and(|old| old != &fingerprint)
            {
                return Err(mismatch());
            }
            return Ok(());
        }
        if registry.browser_owners.len() >= MAX_OWNERS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "browser playback owner registry is full",
            ));
        }
        if !timing.current(now) {
            return Ok(());
        }
        let selected = registry.entries.values().find(|entry| {
            entry.owner == key.0
                && entry.controller.as_ref().is_some_and(|lease| {
                    lease.report.state == HandyControllerState::Active
                        && lease.report.acquisition_request_id == *acquisition_request_id
                        && lease.report.controller_id == *controller_id
                        && lease.report.generation == *expected_generation
                })
                && entry.playback_clock.as_ref().is_some_and(|value| {
                    value.report.request_id == *clock_request_id && value.native.at(now).is_ok()
                })
        });
        let Some(entry) = selected else {
            return Ok(());
        };
        if entry.browser_owner.is_some() {
            return Err(mismatch());
        }
        let lease = &entry.controller.as_ref().expect("selected").report;
        let preparation_id = lease.preparation_id.clone();
        let owner = Arc::new(Owner {
            actor: key.0.clone(),
            project: project(r)?.clone(),
            request_id: r.request_id.clone(),
            auth_hash: hash,
            timing,
            fingerprint: Some(fingerprint),
            selection: Some(Selection {
                preparation: lease.preparation_id.clone(),
                controller: controller_id.clone(),
                acquisition: acquisition_request_id.clone(),
                clock_request: clock_request_id.clone(),
                runtime: Arc::downgrade(&entry.runtime),
            }),
            cancelled: AtomicBool::new(false),
            generation: AtomicU64::new(*expected_generation),
        });
        registry
            .entries
            .get_mut(&preparation_id)
            .expect("selected")
            .browser_owner = Some(owner.clone());
        registry.browser_owners.insert(key, owner);
        Ok(())
    }
    pub(super) fn browser_start_owner(
        &self,
        r: &Request,
        fingerprint: &str,
    ) -> PResult<Arc<Owner>> {
        let Command::StartHandyPlaybackFromBrowser {
            browser_timing_binding_id,
            ..
        } = &r.command
        else {
            return Err(mismatch());
        };
        let registry = self.handy_preparations.lock().map_err(internal)?;
        let owner = registry
            .browser_owners
            .get(&owner_key(r, &r.request_id)?)
            .cloned()
            .ok_or_else(unavailable)?;
        if !owns(&owner, r, browser_timing_binding_id, &auth_hash(r)?)
            || owner
                .fingerprint
                .as_ref()
                .is_some_and(|old| old != fingerprint)
        {
            return Err(mismatch());
        }
        if owner.cancelled.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "original browser play was released",
            ));
        }
        if owner.fingerprint.is_none() {
            return Err(mismatch());
        }
        Ok(owner)
    }
}
impl Owner {
    pub(super) fn current(&self, entry: &Entry, now: u64) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && self.timing.current(now)
            && same_selection(self, entry)
    }
    pub(super) fn selected_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}
fn report(
    registry: &Registry,
    owner: &Arc<Owner>,
    now: u64,
) -> PResult<HandyBrowserPlaybackReport> {
    let mut value = HandyBrowserPlaybackReport {
        project_id: owner.project.clone(),
        play_request_id: owner.request_id.clone(),
        play_record_id: None,
        browser_timing_binding_id: owner.timing.binding().binding_id.clone(),
        controller: None,
        current: false,
        physical_stop_pending: false,
        browser_observation_id: None,
        alignment: None,
    };
    if let Some(selection) = &owner.selection {
        if let Some(entry) = registry.entries.get(&selection.preparation).filter(|e| {
            e.browser_owner
                .as_ref()
                .is_some_and(|o| Arc::ptr_eq(o, owner))
        }) {
            value.controller = entry.controller.as_ref().map(|lease| lease.report.clone());
            value.physical_stop_pending = entry
                .controller
                .as_ref()
                .is_none_or(|lease| lease.report.physical_stop_pending);
            if let Some(pins) = &entry.play {
                if let Some(browser) = &pins.browser {
                    if Arc::ptr_eq(&browser.owner, owner) {
                        value.play_record_id =
                            Some(ArtifactId::new(pins.id.clone()).map_err(internal)?);
                        // Retirement cannot erase the immutable admitted decision.
                        // These historical fields do not imply current authority.
                        value.browser_observation_id =
                            Some(browser.decision.observation.observation_id.clone());
                        value.alignment = Some(browser.decision.alignment.clone());
                        if let Ok((observation, alignment)) = browser.alignment(
                            pins,
                            entry
                                .continuous_timing
                                .as_ref()
                                .map(|timing| &timing.native),
                            now,
                            entry
                                .controller
                                .as_ref()
                                .ok_or_else(mismatch)?
                                .report
                                .deadline_boottime_ns,
                        ) {
                            value.current = owner.current(entry, now);
                            value.browser_observation_id = Some(observation.observation_id);
                            value.alignment = Some(alignment);
                        }
                    }
                }
            } else if entry
                .controller
                .as_ref()
                .is_some_and(|lease| lease.report.state == HandyControllerState::Active)
                && entry.playback_clock.as_ref().is_some_and(|clock| {
                    clock.report.request_id == selection.clock_request
                        && clock.native.at(now).is_ok()
                })
            {
                value.current = owner.current(entry, now);
            }
            if value.current {
                value.physical_stop_pending = true;
            }
        }
    }
    value.validate()?;
    Ok(value)
}
impl Engine {
    pub(in crate::authority) fn handy_browser_owner_fast(
        &self,
        r: &Request,
    ) -> Option<PResult<ResponseBody>> {
        if !matches!(
            r.command,
            Command::RenewHandyBrowserPlayback { .. }
                | Command::ReleaseHandyBrowserPlayback { .. }
                | Command::HandyBrowserPlaybackStatus { .. }
        ) {
            return None;
        }
        if !matches!(r.command, Command::ReleaseHandyBrowserPlayback { .. }) {
            if let Err(error) = self.check_handy_controller_monitor().and_then(|healthy| {
                if healthy {
                    Ok(())
                } else {
                    Err(unavailable())
                }
            }) {
                return Some(Err(error));
            }
        }
        Some(self.handy_browser_owner_with(r, boottime_ns))
    }
    pub(in crate::authority) fn handy_browser_owner_with<F: FnOnce() -> PResult<u64>>(
        &self,
        r: &Request,
        clock: F,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let (play_request, record_id, timing_id, proof) = match &r.command {
            Command::RenewHandyBrowserPlayback {
                play_request_id,
                play_record_id,
                browser_timing_binding_id,
                owner_proof,
            }
            | Command::ReleaseHandyBrowserPlayback {
                play_request_id,
                play_record_id,
                browser_timing_binding_id,
                owner_proof,
            }
            | Command::HandyBrowserPlaybackStatus {
                play_request_id,
                play_record_id,
                browser_timing_binding_id,
                owner_proof,
            } => (
                play_request_id,
                play_record_id,
                browser_timing_binding_id,
                owner_proof,
            ),
            _ => return Err(mismatch()),
        };
        owner_proof(self, proof)?;
        let hash = auth_hash(r)?;
        let key = owner_key(r, play_request)?;
        // Exact cached timing identity is sufficient to cancel one's own future
        // request. It grants no controller or play authority and performs no DB I/O.
        let existing = self
            .handy_preparations
            .lock()
            .map_err(internal)?
            .browser_owners
            .get(&key)
            .cloned();
        let timing = if let Some(owner) = &existing {
            if !owns(owner, r, timing_id, &hash) {
                return Err(mismatch());
            }
            owner.timing.clone()
        } else {
            self.browser_timing_scope(r, timing_id)?
        };
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        if self.is_shutting_down() {
            return Err(unavailable());
        }
        let now = registry.sweep_controllers(clock)?;
        registry.prune_browser_owners(now);
        let owner = if let Some(owner) = registry.browser_owners.get(&key).cloned() {
            owner
        } else if matches!(r.command, Command::ReleaseHandyBrowserPlayback { .. })
            && record_id.is_none()
        {
            if registry.browser_owners.len() >= MAX_OWNERS {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "browser playback cancellation registry is full",
                ));
            }
            let owner = Arc::new(Owner {
                actor: key.0.clone(),
                project: project(r)?.clone(),
                request_id: play_request.clone(),
                auth_hash: hash.clone(),
                timing,
                fingerprint: None,
                selection: None,
                cancelled: AtomicBool::new(true),
                generation: AtomicU64::new(0),
            });
            registry.browser_owners.insert(key, owner.clone());
            owner
        } else {
            return Err(unavailable());
        };
        if !owns(&owner, r, timing_id, &hash) {
            return Err(mismatch());
        }
        let before = report(&registry, &owner, now)?;
        if record_id
            .as_ref()
            .is_some_and(|id| before.play_record_id.as_ref() != Some(id))
        {
            return Err(mismatch());
        }
        match &r.command {
            Command::ReleaseHandyBrowserPlayback { .. } => {
                owner.cancelled.store(true, Ordering::Release);
                if let Some(selection) = &owner.selection {
                    if let Some(entry) =
                        registry
                            .entries
                            .get_mut(&selection.preparation)
                            .filter(|e| {
                                e.browser_owner
                                    .as_ref()
                                    .is_some_and(|o| Arc::ptr_eq(o, &owner))
                            })
                    {
                        retire_entry(entry);
                    }
                }
            }
            Command::RenewHandyBrowserPlayback { .. } => {
                if !before.current {
                    return Err(unavailable());
                }
                let selection = owner.selection.as_ref().ok_or_else(unavailable)?;
                let entry = registry
                    .entries
                    .get_mut(&selection.preparation)
                    .ok_or_else(unavailable)?;
                if !owner.current(entry, now) {
                    return Err(unavailable());
                }
                let lease = &mut entry.controller.as_mut().ok_or_else(unavailable)?.report;
                lease.generation = lease
                    .generation
                    .checked_add(1)
                    .filter(|n| *n <= i64::MAX as u64)
                    .ok_or_else(unavailable)?;
                lease.deadline_boottime_ns = deadline(now, lease.duration_ms)?;
                owner.generation.store(lease.generation, Ordering::Release);
                // Browser same-lineage renewal preserves the captured native
                // clock and its original age, including a queued referenced play.
            }
            _ => {}
        }
        Ok(ResponseBody::HandyBrowserPlayback(Box::new(report(
            &registry, &owner, now,
        )?)))
    }
}

impl Engine {
    /// The public request keeps its own immutable fingerprint. Only native
    /// selection is translated to the existing independently qualified reader.
    pub(in crate::authority) fn handy_browser_timing_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        if !self.check_handy_controller_monitor()? {
            return Err(unavailable());
        }
        self.handy_browser_timing_with(
            r,
            actor,
            fingerprint,
            (
                |hosted, deadline| hosted.recheck(deadline).map_err(|_| mismatch()),
                crate::handy_online::inspect_playing_timing,
                crate::handy_server_clock::sample_until,
            ),
            boottime_ns,
        )
    }
    pub(in crate::authority) fn handy_browser_timing_with<R, D, S, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        io: (R, D, S),
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::PlayingTimingRead>,
        S: FnMut(
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        let Command::RefreshHandyBrowserPlaybackTiming {
            play_request_id,
            play_record_id,
            browser_timing_binding_id,
            owner_proof: proof,
            allow_online_inspection,
        } = &r.command
        else {
            return Err(mismatch());
        };
        let (selection, revision) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyContinuousTiming(saved) = body else {
                    return Err(mismatch());
                };
                if saved.request_id != r.request_id
                    || saved.play_request_id != *play_request_id
                    || saved.controller.project_id != *project(r)?
                    || play_record_id
                        .as_ref()
                        .is_some_and(|id| id != &saved.play_record_id)
                {
                    return Err(mismatch());
                }
                (saved.selection(), saved.controller.revision)
            } else {
                let hash = auth_hash(r)?;
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                let owner = registry
                    .browser_owners
                    .get(&owner_key(r, play_request_id)?)
                    .ok_or_else(unavailable)?;
                if !owns(owner, r, browser_timing_binding_id, &hash) {
                    return Err(mismatch());
                }
                let selected = owner.selection.as_ref().ok_or_else(unavailable)?;
                let entry = registry
                    .entries
                    .get(&selected.preparation)
                    .ok_or_else(unavailable)?;
                let pins = entry.play.as_ref().ok_or_else(unavailable)?;
                let lease = &entry.controller.as_ref().ok_or_else(unavailable)?.report;
                if !owner.current(entry, now)
                    || pins
                        .browser
                        .as_ref()
                        .is_none_or(|b| !Arc::ptr_eq(&b.owner, owner))
                    || play_record_id
                        .as_ref()
                        .is_some_and(|id| id.as_str() != pins.id)
                {
                    return Err(mismatch());
                }
                (
                    HandyContinuousTimingSelection {
                        acquisition_request_id: lease.acquisition_request_id.clone(),
                        controller_id: lease.controller_id.clone(),
                        expected_generation: lease.generation,
                        play_request_id: play_request_id.clone(),
                        play_record_id: ArtifactId::new(pins.id.clone()).map_err(internal)?,
                    },
                    lease.revision,
                )
            }
        };
        let mut internal = r.clone();
        internal.expected_revision = Some(revision);
        internal.command = Command::RefreshHandyPlaybackTiming {
            selection,
            allow_online_inspection: *allow_online_inspection,
            owner_proof: proof.clone(),
        };
        self.handy_continuous_timing_with(&internal, actor, fingerprint, io, clock)
    }
}
