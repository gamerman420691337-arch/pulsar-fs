//! Explicit session observation ownership. Preparation deadlines and effects are
//! immutable; only separately admitted fresh native observations may continue.
use super::*;
use arc_swap::ArcSwap;
use std::sync::Weak;

pub(super) struct ConsumedSource {
    pub request: RequestId,
    pub binding: HspBinding,
    pub clocks: Arc<NativePlaybackClock>,
    pub qualified: handy_qualification::QualifiedHspBundle,
    pub valid_until_ns: u64,
    pub source: Weak<dyn HspNativeAuthority>,
    pub record: HspStreamRecord,
}
pub(in crate::authority) struct HspClockAdmission<'a> {
    pub request: &'a Request,
    pub preparation_request_id: &'a RequestId,
    pub binding: &'a HspBinding,
    pub admitted_at_ns: u64,
    pub admitted_at: Instant,
}
struct ReadResult {
    predecessor: HspClockPin,
    clocks: Arc<NativePlaybackClock>,
    valid_until_ns: u64,
}
#[derive(Default)]
struct WorkerState {
    worker: Option<JoinHandle<PResult<ReadResult>>>,
    pending: Option<ReadResult>,
    joining: bool,
    error: Option<ProtocolError>,
}
pub(in crate::authority) struct HspContinuityClockOwner {
    entry: Weak<Entry>,
    source: Weak<dyn HspNativeAuthority>,
    binding: HspBinding,
    qualified: handy_qualification::QualifiedHspBundle,
    enable: RequestId,
    fingerprint: String,
    history: HandyHspContinuityHistory,
    deadline: Instant,
    retired: Arc<AtomicBool>,
    latest: ArcSwap<HspClockPin>,
    worker: Mutex<WorkerState>,
}
/// A fresh reader selects this exact native observation before sampling its
/// evaluation time. A later publication cannot replace or extend the pin.
pub(in crate::authority) struct HspClockRead<'a> {
    owner: &'a HspContinuityClockOwner,
    pin: HspClockPin,
}
impl HspClockRead<'_> {
    pub(in crate::authority) fn evaluate_at(&self, now: u64) -> PResult<HspClockPin> {
        self.owner.check_at(now)?;
        self.pin.at(now)?;
        Ok(self.pin.clone())
    }
}
fn identity(request: &Request) -> PResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(request_fingerprint(request).map_err(internal)?)
    ))
}
impl HspContinuityClockOwner {
    pub(in crate::authority) fn history(&self) -> &HandyHspContinuityHistory {
        &self.history
    }
    pub(in crate::authority) fn snapshot(&self, now_ns: u64) -> PResult<HspClockPin> {
        self.check_at(now_ns)?;
        let pin = self.latest.load_full();
        pin.at(now_ns)?;
        Ok((*pin).clone())
    }
    pub(in crate::authority) fn select_read(&self) -> HspClockRead<'_> {
        HspClockRead {
            owner: self,
            pin: (*self.latest.load_full()).clone(),
        }
    }
    pub(in crate::authority) fn checkpoint(&self) -> PResult<u64> {
        let entry = self.entry.upgrade().ok_or_else(cancelled)?;
        let now = entry.io.now()?;
        self.check_at(now)?;
        Ok(now)
    }
    fn check_at(&self, now: u64) -> PResult<()> {
        let entry = self.entry.upgrade().ok_or_else(cancelled)?;
        if self.retired.load(Ordering::Acquire)
            || entry.cancel.load(Ordering::Acquire)
            || entry.deps.shutdown.load(Ordering::Acquire)
            || now < self.history.admitted_at_boottime_ns
            || now >= self.history.deadline_boottime_ns
            || Instant::now() >= self.deadline
        {
            return Err(cancelled());
        }
        Ok(())
    }
    pub(super) fn validate_pin(
        &self,
        pin: &HspClockPin,
        now: u64,
        facts: &HspDispatchFacts,
    ) -> PResult<()> {
        self.check_at(now)?;
        if facts.binding() != &self.binding
            || !pin.belongs_to(&self.history.clock_owner_id, &self.retired)
        {
            return Err(mismatch());
        }
        pin.validate_facts(now, facts)
    }
    pub(in crate::authority) fn request_stop(&self) {
        self.retired.store(true, Ordering::Release);
    }
    fn fail(&self, error: ProtocolError) -> ProtocolError {
        self.request_stop();
        if let Ok(mut state) = self.worker.try_lock() {
            if state.error.is_none() {
                state.error = Some(error.clone());
            }
        }
        error
    }
    /// One short owner-pump step. Native HTTP work is performed solely by the
    /// bounded worker; this method joins only workers already known finished.
    pub(in crate::authority) fn poll(&self) -> PResult<()> {
        let now = match self.checkpoint() {
            Ok(now) => now,
            Err(refusal) => {
                self.request_stop();
                // Retirement cannot strand completed read custody until final
                // Engine shutdown. Cleanup never validates/publishes a result,
                // starts another read, or waits for an unfinished worker.
                let mut state = match self.worker.try_lock() {
                    Ok(state) => state,
                    Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                    Err(TryLockError::WouldBlock) => return Err(refusal),
                };
                state.pending = None;
                if state.error.is_none() {
                    state.error = Some(refusal.clone());
                }
                if state.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                    let worker = state.worker.take().expect("finished read custody");
                    let _ = worker.join();
                }
                return Err(refusal);
            }
        };
        let mut state = match self.worker.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => {
                return Err(self.fail(unavailable("continuity worker state poisoned")))
            }
        };
        if state.joining {
            return Ok(());
        }
        if state.worker.as_ref().is_some_and(|w| !w.is_finished()) {
            if self.latest.load().at(now).is_err() {
                self.request_stop();
                return Err(unavailable("original observation expired during refresh"));
            }
            return Ok(());
        }
        if let Some(worker) = state.worker.take() {
            let result = worker
                .join()
                .unwrap_or_else(|_| Err(unavailable("continuity observation worker panicked")));
            match result {
                Ok(result) => state.pending = Some(result),
                Err(error) => {
                    self.request_stop();
                    state.error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        if state.pending.is_some() {
            let at = self.checkpoint()?;
            self.snapshot(at)?;
            if !self.validate_current()? {
                return Ok(());
            }
            let result = state.pending.take().expect("checked original refresh");
            let published = (|| {
                let at = self.checkpoint()?;
                if self.latest.load().as_ref() != &result.predecessor {
                    return Err(mismatch());
                }
                result.predecessor.at(at)?;
                result
                    .clocks
                    .server
                    .continues(&result.predecessor.native().server, at)?;
                let pin = HspClockPin::new(
                    self.history.clock_owner_id.clone(),
                    result
                        .predecessor
                        .generation()
                        .checked_add(1)
                        .ok_or_else(mismatch)?,
                    result.clocks,
                    result.valid_until_ns,
                    self.history.deadline_boottime_ns,
                    self.retired.clone(),
                )?;
                pin.at(at)?;
                self.latest.store(Arc::new(pin));
                Ok(())
            })();
            if let Err(error) = published {
                self.request_stop();
                state.error = Some(error.clone());
                return Err(error);
            }
        }
        let at = self.checkpoint()?;
        let previous = self.snapshot(at)?;
        let observed = previous.native().server.clock.monotonic_ns;
        let refresh_at = observed
            .checked_add(
                previous
                    .valid_until_ns()
                    .checked_sub(observed)
                    .ok_or_else(mismatch)?
                    / 2,
            )
            .ok_or_else(mismatch)?;
        if at < refresh_at {
            return Ok(());
        }
        let entry = self.entry.upgrade().ok_or_else(cancelled)?;
        let source = self.source.clone();
        let binding = self.binding.clone();
        let qualified = self.qualified.clone();
        let retired = self.retired.clone();
        let until = previous
            .valid_until_ns()
            .min(self.history.deadline_boottime_ns);
        let deadline = Instant::now()
            .checked_add(Duration::from_nanos(
                until.checked_sub(at).ok_or_else(mismatch)?,
            ))
            .ok_or_else(mismatch)?
            .min(self.deadline)
            .min(Instant::now() + Duration::from_secs(3));
        let task = ReadTask {
            entry,
            source,
            binding,
            qualified,
            previous,
            retired,
            deadline,
            until,
        };
        state.worker = Some(
            thread::Builder::new()
                .name("pulsar-hsp-clock-observation".into())
                .spawn(move || task.run())
                .map_err(|_| unavailable("continuity observation worker unavailable"))?,
        );
        Ok(())
    }
    fn validate_current(&self) -> PResult<bool> {
        let entry = self.entry.upgrade().ok_or_else(cancelled)?;
        self.checkpoint()?;
        validate_dependencies(&entry, &self.source, &self.binding, &mut || {
            self.checkpoint()
        })
    }
    /// Final cleanup may wait for a bounded native read; no authority mutex is
    /// retained during join. Cancellation never waits for this method.
    pub(in crate::authority) fn join(&self) -> PResult<()> {
        self.request_stop();
        let worker = {
            let mut state = self.worker.lock().map_err(internal)?;
            if state.joining {
                return Err(unavailable("continuity worker already joining"));
            }
            let worker = state.worker.take();
            state.pending = None;
            state.joining = worker.is_some();
            worker
        };
        if let Some(worker) = worker {
            let result = worker.join();
            self.worker.lock().map_err(internal)?.joining = false;
            let _ = result
                .map_err(|_| unavailable("continuity observation worker panicked during join"))?;
        }
        Ok(())
    }
    pub(in crate::authority) fn is_joined(&self) -> bool {
        self.worker
            .try_lock()
            .is_ok_and(|state| state.worker.is_none() && !state.joining)
    }
    pub(in crate::authority) fn status(&self, now: u64) -> HandyHspContinuityNativeStatus {
        let pin = self.latest.load_full();
        let state = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        HandyHspContinuityNativeStatus {
            observed_at_boottime_ns: now,
            generation: pin.generation(),
            clock_valid_until_boottime_ns: pin.valid_until_ns(),
            current: self.check_at(now).is_ok() && pin.at(now).is_ok(),
            retired: self.retired.load(Ordering::Acquire),
            worker_active: state.worker.is_some() || state.joining,
            worker_joined: state.worker.is_none() && !state.joining,
            error: state.error.clone(),
        }
    }
}
impl Drop for HspContinuityClockOwner {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .worker
            .take()
        {
            let _ = worker.join();
        }
    }
}
fn validate_dependencies(
    entry: &Entry,
    source: &Weak<dyn HspNativeAuthority>,
    binding: &HspBinding,
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<bool> {
    if entry.cancel.load(Ordering::Acquire) || entry.deps.shutdown.load(Ordering::Acquire) {
        return Err(cancelled());
    }
    let _admission = match entry.deps.admission.try_read() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return Ok(false),
        Err(TryLockError::Poisoned(_)) => {
            return Err(unavailable("continuity recovery admission poisoned"))
        }
    };
    let db = match entry.deps.db.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return Ok(false),
        Err(TryLockError::Poisoned(_)) => {
            return Err(unavailable("continuity project authority poisoned"))
        }
    };
    entry.deps.validate_db(&db)?;
    let sessions = match entry.deps.sessions.try_lock() {
        Ok(guard) => guard,
        Err(TryLockError::WouldBlock) => return Ok(false),
        Err(TryLockError::Poisoned(_)) => {
            return Err(unavailable("continuity device session poisoned"))
        }
    };
    entry.deps.validate_session(&sessions)?;
    let source = source.upgrade().ok_or_else(cancelled)?;
    let observed = observe_source_current(
        source.as_ref(),
        clock,
        "continuity source authority unavailable",
    )?;
    let facts = observed.snapshot.facts.ok_or_else(mismatch)?;
    if facts.binding() != binding
        || facts.reference().at_ns() != observed.sampled_at_ns
        || facts.valid_at(observed.sampled_at_ns).is_err()
    {
        return Err(mismatch());
    }
    Ok(true)
}
/// Preserve the caller's original bounded clock callback and its refusal. This
/// bridge never substitutes a sampled tick for a fresh native clock call.
pub(super) fn observe_source_current(
    source: &dyn HspNativeAuthority,
    clock: &mut dyn FnMut() -> PResult<u64>,
    source_refusal: &'static str,
) -> PResult<HspAuthorityEvaluation> {
    let mut calls = 0u8;
    let mut sampled = None;
    let mut refusal = None;
    let result = source.observe_current(&mut || {
        if calls != 0 {
            refusal = Some(unavailable(
                "source observation sampled its clock more than once",
            ));
            return Err(HspDispatchFailure::InvalidClock);
        }
        calls = 1;
        match clock() {
            Ok(now) => {
                sampled = Some(now);
                Ok(now)
            }
            Err(error) => {
                refusal = Some(error);
                Err(HspDispatchFailure::InvalidClock)
            }
        }
    });
    if let Some(error) = refusal {
        return Err(error);
    }
    let observed = result.map_err(|_| unavailable(source_refusal))?;
    if calls != 1 || sampled != Some(observed.sampled_at_ns) {
        return Err(mismatch());
    }
    Ok(observed)
}
fn await_dependencies(
    entry: &Entry,
    source: &Weak<dyn HspNativeAuthority>,
    binding: &HspBinding,
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<()> {
    loop {
        // Check the original read budget even when dependency locks are busy.
        // The authority's separate evaluation clock is sampled after selection.
        clock()?;
        if validate_dependencies(entry, source, binding, clock)? {
            clock()?;
            return Ok(());
        }
        // Every guard from the unsuccessful read has dropped. The clock checks
        // the original read deadline, observation expiry and cancellation.
        thread::park_timeout(Duration::from_millis(1));
    }
}
struct ReadTask {
    entry: Arc<Entry>,
    source: Weak<dyn HspNativeAuthority>,
    binding: HspBinding,
    qualified: handy_qualification::QualifiedHspBundle,
    previous: HspClockPin,
    retired: Arc<AtomicBool>,
    deadline: Instant,
    until: u64,
}
impl ReadTask {
    fn run(self) -> PResult<ReadResult> {
        let Self {
            entry,
            source,
            binding,
            qualified,
            previous,
            retired,
            deadline,
            until,
        } = self;
        let mut last = 0;
        let mut clock = || {
            let now = entry.io.now()?;
            if retired.load(Ordering::Acquire)
                || entry.cancel.load(Ordering::Acquire)
                || entry.deps.shutdown.load(Ordering::Acquire)
                || now < last
                || now >= until
                || Instant::now() >= deadline
            {
                return Err(cancelled());
            }
            last = now;
            Ok(now)
        };
        await_dependencies(&entry, &source, &binding, &mut clock)?;
        let read = entry.io.inspect_hsp_stream(
            entry.deps.credentials.clone(),
            deadline,
            &mut clock,
            binding.identity().stream_id,
        )?;
        crate::handy_online::validate_hsp_clock_context(&read, binding.identity().stream_id)?;
        if read.started_boottime_ns < previous.native().server.clock.monotonic_ns
            || read.device.before.identity != qualified.report.manifest.subject.identity
            || Some(&read.device.before.operative_settings)
                != qualified
                    .report
                    .manifest
                    .subject
                    .operative_settings
                    .as_ref()
            || read.device.before.remote_hsp.as_ref().is_none_or(|state| {
                usize::try_from(state.max_points).map_or(true, |capacity| {
                    capacity < qualified.policy.device_capacity_points
                })
            })
        {
            return Err(mismatch());
        }
        previous
            .native()
            .device
            .check_observation(&read.device.first)?;
        previous
            .native()
            .device
            .check_observation(&read.device.second)?;
        let sample = entry.io.server_sample(deadline, &mut clock)?;
        if sample.sent_ns < read.finished_boottime_ns {
            return Err(mismatch());
        }
        let at = clock()?;
        previous.at(at)?;
        let (server_contract, _) = handy_qualification::hsp_clock_contracts(&qualified)?;
        let mut server = NativeServerClock::new(
            previous.native().server.clock.domain.clone(),
            sample,
            server_contract,
        )?;
        server.clock.valid_until_ns = server
            .clock
            .valid_until_ns
            .min(previous.session_until_ns())
            .min(
                read.started_boottime_ns
                    .checked_add(
                        qualified
                            .report
                            .manifest
                            .subject
                            .profile
                            .maximum_command_age_ns,
                    )
                    .ok_or_else(mismatch)?,
            );
        server.continues(&previous.native().server, at)?;
        let clocks = Arc::new(NativePlaybackClock {
            server,
            device: previous.native().device.clone(),
        });
        let observed = clocks.at(at)?;
        await_dependencies(&entry, &source, &binding, &mut clock)?;
        Ok(ReadResult {
            predecessor: previous,
            valid_until_ns: observed.valid_until_ns,
            clocks,
        })
    }
}
impl Engine {
    pub(in crate::authority) fn admit_handy_hsp_continuity_clock_locked(
        &self,
        db: &Connection,
        admission: HspClockAdmission<'_>,
    ) -> PResult<Arc<HspContinuityClockOwner>> {
        let request = admission.request;
        request.validate()?;
        let Command::EnableHandyHspContinuity {
            stream_request_id,
            duration_ms,
            allow_online_observations,
            ..
        } = &request.command
        else {
            return Err(mismatch());
        };
        validate_handy_hsp_continuity_selection(
            stream_request_id,
            &request.request_id,
            *duration_ms,
            *allow_online_observations,
        )?;
        let actor = request.session.as_ref().ok_or_else(mismatch)?;
        let fingerprint = identity(request)?;
        let entry = self
            .handy_hsp_preparations
            .try_lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), admission.preparation_request_id.clone()))
            .cloned()
            .ok_or_else(mismatch)?;
        if let Some(owner) = entry.continuity.get() {
            if owner.enable != request.request_id || owner.fingerprint != fingerprint {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "original continuity admission already exists",
                ));
            }
            return Ok(owner.clone());
        }
        let now = entry.budget()?;
        entry.deps.validate_db(db)?;
        let sessions = entry.deps.sessions.try_lock().map_err(internal)?;
        entry.deps.validate_session(&sessions)?;
        if request.project != entry.deps.request.project
            || request.expected_revision != entry.deps.request.expected_revision
            || admission.admitted_at_ns > now
            || admission.admitted_at > Instant::now()
        {
            return Err(mismatch());
        }
        let state = entry.state.try_lock().map_err(internal)?;
        let consumed = state.consumed_source.as_ref().ok_or_else(mismatch)?;
        if !state.consumed
            || state.child.is_none()
            || state.resolved
            || consumed.request != *stream_request_id
            || consumed.binding != *admission.binding
            || now >= consumed.valid_until_ns
        {
            return Err(mismatch());
        }
        let execution = self
            .handy_hsp_execution
            .try_lock()
            .map_err(internal)?
            .as_ref()
            .ok_or_else(mismatch)?
            .handle();
        let child = execution
            .snapshot_for_session(actor, project(request)?, stream_request_id)
            .map_err(|_| unavailable("original HSP stream is not observable"))?;
        if !child.post_setup_bound
            || child.retired
            || child.stream_record_id != consumed.record.id().map_err(internal)?
            || !matches!(
                child.runtime_state,
                crate::handy_hsp_runtime::HspRuntimeState::Prefilling
                    | crate::handy_hsp_runtime::HspRuntimeState::Playing
            )
        {
            return Err(unavailable(
                "continuity requires the exact live post-Setup stream",
            ));
        }
        let source = consumed.source.upgrade().ok_or_else(cancelled)?;
        let observed = observe_source_current(
            source.as_ref(),
            &mut || entry.budget(),
            "original source is unavailable",
        )?;
        let now = observed.sampled_at_ns;
        if now >= consumed.valid_until_ns || now < admission.admitted_at_ns {
            return Err(mismatch());
        }
        let initial = consumed.clocks.at(now)?;
        let facts = observed.snapshot.facts.ok_or_else(mismatch)?;
        if facts.binding() != admission.binding || facts.reference().at_ns() != now {
            return Err(mismatch());
        }
        facts.valid_at(now).map_err(|_| mismatch())?;
        let duration = Duration::from_millis(u64::from(*duration_ms));
        let end = admission
            .admitted_at_ns
            .checked_add(u64::from(*duration_ms) * 1_000_000)
            .ok_or_else(mismatch)?;
        let deadline = admission
            .admitted_at
            .checked_add(duration)
            .ok_or_else(mismatch)?;
        let sync_until = consumed.clocks.synchronization_until_ns()?;
        let subject = &consumed.qualified.report.manifest.subject;
        let floor = consumed
            .clocks
            .device
            .error_at(end)?
            .checked_add(
                subject
                    .server_clock
                    .as_ref()
                    .ok_or_else(mismatch)?
                    .timestamp_error_ns,
            )
            .and_then(|n| n.checked_add(1_000_000))
            .ok_or_else(mismatch)?;
        if now >= end
            || Instant::now() >= deadline
            || end > sync_until
            || floor > subject.profile.maximum_clock_uncertainty_ns
        {
            return Err(unavailable(
                "requested continuity duration exceeds original synchronization or error ceiling",
            ));
        }
        let retired = Arc::new(AtomicBool::new(false));
        let owner_id = ArtifactId::new(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &request.request_id,
                    &fingerprint,
                    consumed.record.id().map_err(internal)?,
                    end
                ))
                .map_err(internal)?
            )
        ))
        .map_err(internal)?;
        let valid_until = initial.valid_until_ns.min(consumed.valid_until_ns).min(end);
        let pin = HspClockPin::new(
            owner_id.clone(),
            1,
            consumed.clocks.clone(),
            valid_until,
            end,
            retired.clone(),
        )?;
        let owner = Arc::new(HspContinuityClockOwner {
            entry: Arc::downgrade(&entry),
            source: consumed.source.clone(),
            binding: consumed.binding.clone(),
            qualified: consumed.qualified.clone(),
            enable: request.request_id.clone(),
            fingerprint,
            history: HandyHspContinuityHistory {
                preparation_request_id: admission.preparation_request_id.clone(),
                stream_record_id: ArtifactId::new(consumed.record.id().map_err(internal)?)
                    .map_err(internal)?,
                clock_owner_id: owner_id,
                admitted_at_boottime_ns: admission.admitted_at_ns,
                deadline_boottime_ns: end,
                synchronization_sent_boottime_ns: consumed
                    .clocks
                    .device
                    .acknowledgment
                    .sent_boottime_ns,
                synchronization_ceiling_boottime_ns: sync_until,
                initial_clock_valid_until_boottime_ns: valid_until,
            },
            deadline,
            retired,
            latest: ArcSwap::from_pointee(pin),
            worker: Mutex::new(WorkerState::default()),
        });
        entry
            .continuity
            .set(owner.clone())
            .map_err(|_| mismatch())?;
        Ok(owner)
    }
}
