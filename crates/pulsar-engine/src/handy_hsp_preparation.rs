//! Public HSP preparation owns one synchronization and original native clocks.
//! History never recreates readiness; source/presentation admission remains separate.
use super::*;
#[path = "handy_hsp_clock_owner.rs"]
mod clock_owner;
use crate::{
    device_effect_journal::{
        ClaimedHspPreparationReceipt, ClockSyncCompletion, HspPreparationMembership,
        HspResolvedReceipt, PreparedHspPreparationReceipt,
    },
    handy_credentials::{Credentials, HandyRouteIdentity},
    handy_hsp_dispatch::{
        HandyHspTransport, HspClaimGate, HspClaimedHandoff, HspDispatchFailure, HspNativeOwner,
        HspPreclaimProgress, HspSetupPreviousState, HspTransportBinding,
    },
    handy_hsp_execution::{
        HspAdmission, HspAuthorityEvaluation, HspAuthoritySnapshot, HspNativeAuthority,
        HspSessionHandle, HspSessionRetirement,
    },
    handy_hsp_preparation_records::HspPreparationRecord,
    handy_hsp_records::{HspBindingRecord, HspQualificationLinkRecord, HspStreamRecord},
    handy_hsp_runtime::{HspBinding, HspDispatchFacts},
    handy_online::{DeviceTimingRead, PlayingTimingRead},
    handy_playback_clock::{HspClockPin, NativeDeviceClock, NativePlaybackClock},
    handy_server_clock::{NativeServerClock, Sample},
    handy_wire::DeviceIdentity,
};
pub(super) use clock_owner::{HspClockAdmission, HspContinuityClockOwner};
use std::{
    sync::{RwLock, TryLockError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const CAPACITY: usize = 64;
const POLL: Duration = Duration::from_millis(2);

pub(super) trait HspPreparationIo: Send + Sync {
    fn now(&self) -> PResult<u64>;
    fn inspect(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<DeviceTimingRead>;
    fn synchronize(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> ClockSyncCompletion;
    fn server_sample(
        &self,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<Sample>;
    fn inspect_hsp_stream(
        &self,
        _credentials: Arc<Credentials>,
        _deadline: Instant,
        _clock: &mut dyn FnMut() -> PResult<u64>,
        _stream_id: u32,
    ) -> PResult<PlayingTimingRead> {
        Err(ProtocolError::unsupported(
            "HSP stream observation is not provided",
        ))
    }
    fn before_claim(&self) -> PResult<()> {
        Ok(())
    }
    fn after_claim(&self) -> PResult<()> {
        Ok(())
    }
    #[cfg(test)]
    fn metadata_contended(&self) -> PResult<()> {
        Ok(())
    }
    #[cfg(test)]
    fn hsp_origin(&self) -> Option<String> {
        None
    }
}
struct NativeIo;
impl HspPreparationIo for NativeIo {
    fn inspect_hsp_stream(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
        stream_id: u32,
    ) -> PResult<PlayingTimingRead> {
        crate::handy_online::inspect_hsp_clock_context(credentials, deadline, clock, stream_id)
    }
    fn now(&self) -> PResult<u64> {
        crate::handy_clock::boottime_ns()
    }
    fn inspect(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<DeviceTimingRead> {
        crate::handy_online::inspect_device_timing(credentials, deadline, clock)
    }
    fn synchronize(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> ClockSyncCompletion {
        crate::handy_clock_sync_wire::send(credentials, deadline, clock)
    }
    fn server_sample(
        &self,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<Sample> {
        crate::handy_server_clock::sample_until(deadline, clock)
    }
}
fn unavailable(message: &'static str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "HSP preparation dependencies changed",
    )
}
fn cancelled() -> ProtocolError {
    ProtocolError::new(ErrorCode::Cancelled, "original HSP preparation retired")
}
fn native_identity(value: &HandyIdentity) -> DeviceIdentity {
    DeviceIdentity {
        firmware: value.firmware.clone(),
        model_number: value.model_number,
        model_name: value.model_name.clone(),
        model_variant: value.model_variant,
        session_id: value.session_id.clone(),
        firmware_status: value.firmware_status,
    }
}

struct Dependencies {
    db: Arc<Mutex<Connection>>,
    sessions: Arc<Mutex<handy_sessions::Registry>>,
    admission: Arc<RwLock<()>>,
    journal: Arc<Mutex<crate::device_effect_journal::DeviceEffectJournal>>,
    shutdown: Arc<AtomicBool>,
    epoch: DeviceSessionId,
    request: Request,
    actor: SessionId,
    fingerprint: String,
    session: HandyConnectionSession,
    credentials: Arc<Credentials>,
    qualification: HandyQualificationBinding,
    route: HandyRouteIdentity,
    started_ns: u64,
    deadline_ns: u64,
    deadline: Instant,
}
#[derive(Clone)]
enum PendingMetadata {
    Complete(ClaimedHspPreparationReceipt, ClockSyncCompletion),
    Cancel(PreparedHspPreparationReceipt),
    ClaimUncertain(String),
}
struct Readiness {
    record: HspPreparationRecord,
    parent: PreparedHspPreparationReceipt,
    clocks: Arc<NativePlaybackClock>,
    qualified: handy_qualification::QualifiedHspBundle,
    observation: HandySetupDeviceObservation,
    valid_until_ns: u64,
}
struct State {
    phase: HandyHspPreparationPhase,
    error: Option<ProtocolError>,
    record: Option<HspPreparationRecord>,
    prepared: Option<PreparedHspPreparationReceipt>,
    claimed: Option<ClaimedHspPreparationReceipt>,
    completion: Option<ClockSyncCompletion>,
    pending: Option<PendingMetadata>,
    candidate: Option<Readiness>,
    ready: Option<Readiness>,
    child: Option<HspSessionRetirement>,
    consumed: bool,
    consumed_source: Option<clock_owner::ConsumedSource>,
    resolved: bool,
}
pub(super) struct Entry {
    deps: Dependencies,
    io: Arc<dyn HspPreparationIo>,
    cancel: AtomicBool,
    claim_entered: AtomicBool,
    active: AtomicBool,
    state: Mutex<State>,
    worker: Mutex<Option<JoinHandle<()>>>,
    continuity: std::sync::OnceLock<Arc<HspContinuityClockOwner>>,
    #[cfg(test)]
    preclaim_test: Mutex<Option<Arc<HspPreclaimTestProbe>>>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) enum HspPreclaimTestSite {
    Db,
    Sessions,
    Source,
}
#[cfg(test)]
impl HspPreclaimTestSite {
    fn index(self) -> usize {
        match self {
            Self::Db => 0,
            Self::Sessions => 1,
            Self::Source => 2,
        }
    }
}
#[cfg(test)]
#[derive(Default)]
struct HspPreclaimTestState {
    entered: bool,
    released: bool,
    timed_out: bool,
    counts: [u64; 3],
    claimed: u64,
}
#[cfg(test)]
#[derive(Default)]
struct HspPreclaimTestProbe {
    state: Mutex<HspPreclaimTestState>,
    changed: std::sync::Condvar,
}
#[cfg(test)]
pub(super) struct HspPreclaimTestHold(Arc<HspPreclaimTestProbe>, std::sync::Weak<Entry>);
#[cfg(test)]
impl HspPreclaimTestHold {
    fn wait(&self, timeout: Duration, predicate: impl Fn(&HspPreclaimTestState) -> bool) -> bool {
        let state = self.0.state.lock().unwrap();
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |s| !predicate(s))
            .unwrap();
        predicate(&state) && !state.timed_out
    }
    pub(super) fn wait_entered(&self, timeout: Duration) -> bool {
        self.wait(timeout, |state| state.entered)
    }
    pub(super) fn wait_contended(&self, site: HspPreclaimTestSite, timeout: Duration) -> bool {
        self.wait(timeout, |state| state.counts[site.index()] != 0)
    }
    pub(super) fn release(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.released = true;
        self.0.changed.notify_all();
    }
    pub(super) fn counts(&self) -> [u64; 3] {
        self.0.state.lock().unwrap().counts
    }
    pub(super) fn timed_out(&self) -> bool {
        self.0.state.lock().unwrap().timed_out
    }
    pub(super) fn claimed(&self) -> u64 {
        self.0.state.lock().unwrap().claimed
    }
    /// Exercise existing original native retirement without needing the DB
    /// deliberately held by the fixture. This is not a public Stop latency test.
    pub(super) fn cancel_original_for_test(&self) -> PResult<bool> {
        let entry = self
            .1
            .upgrade()
            .ok_or_else(|| unavailable("original native preparation is absent"))?;
        let was_cancelled = entry.cancel.load(Ordering::Acquire);
        entry.retire();
        Ok(!was_cancelled)
    }
}
#[cfg(test)]
impl Drop for HspPreclaimTestHold {
    fn drop(&mut self) {
        self.release();
    }
}
#[cfg(test)]
impl HspPreclaimTestProbe {
    fn enter(&self) -> std::result::Result<(), HspDispatchFailure> {
        let mut state = self.state.lock().unwrap();
        state.entered = true;
        self.changed.notify_all();
        let (mut state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(2), |s| !s.released)
            .unwrap();
        if !state.released {
            state.timed_out = true;
            return Err(HspDispatchFailure::Cancelled);
        }
        Ok(())
    }
    fn contended(&self, site: HspPreclaimTestSite) {
        let mut state = self.state.lock().unwrap();
        state.counts[site.index()] += 1;
        self.changed.notify_all();
    }
    fn claimed(&self) {
        let mut state = self.state.lock().unwrap();
        state.claimed += 1;
        self.changed.notify_all();
    }
}
#[cfg(test)]
struct PublicTestIo {
    actor: SessionId,
    engine_epoch: DeviceSessionId,
    device_session: DeviceSessionId,
    io: Arc<dyn HspPreparationIo>,
}
/// The installed fixture origin is immutable even if its delegated observer
/// later changes internal state. All clocks and observations remain delegated.
#[cfg(test)]
struct LoopbackTestIo {
    inner: Arc<dyn HspPreparationIo>,
    origin: String,
}
#[cfg(test)]
impl HspPreparationIo for LoopbackTestIo {
    fn now(&self) -> PResult<u64> {
        self.inner.now()
    }
    fn inspect(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<DeviceTimingRead> {
        self.inner.inspect(credentials, deadline, clock)
    }
    fn synchronize(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> ClockSyncCompletion {
        self.inner.synchronize(credentials, deadline, clock)
    }
    fn server_sample(
        &self,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
    ) -> PResult<Sample> {
        self.inner.server_sample(deadline, clock)
    }
    fn inspect_hsp_stream(
        &self,
        credentials: Arc<Credentials>,
        deadline: Instant,
        clock: &mut dyn FnMut() -> PResult<u64>,
        stream: u32,
    ) -> PResult<PlayingTimingRead> {
        self.inner
            .inspect_hsp_stream(credentials, deadline, clock, stream)
    }
    fn before_claim(&self) -> PResult<()> {
        self.inner.before_claim()
    }
    fn after_claim(&self) -> PResult<()> {
        self.inner.after_claim()
    }
    fn hsp_origin(&self) -> Option<String> {
        Some(self.origin.clone())
    }
}
#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<(SessionId, RequestId), Arc<Entry>>,
    #[cfg(test)]
    public_test_io: Option<PublicTestIo>,
    #[cfg(test)]
    public_admission_started: bool,
}
impl Registry {
    pub(super) fn cancel_all(&mut self) {
        for entry in self.entries.values() {
            entry.retire();
        }
    }
    pub(super) fn invalidate(&mut self, reason: handy_playback_preparation::Retirement<'_>) {
        for entry in self.entries.values() {
            let retire = match reason {
                handy_playback_preparation::Retirement::Device(id) => {
                    entry.deps.session.registration.device_id == *id
                }
                handy_playback_preparation::Retirement::Connection(id) => {
                    entry.deps.session.device_session == *id
                }
                handy_playback_preparation::Retirement::Qualification(id) => {
                    entry.deps.qualification.dossier_id == *id
                }
                _ => false,
            };
            if retire {
                entry.retire();
            }
        }
    }
    pub(super) fn invalidate_changed(&mut self, db: &Connection) {
        for entry in self.entries.values() {
            if entry.deps.validate_db(db).is_err() {
                entry.retire();
            }
        }
    }
}
impl Dependencies {
    fn validate_db(&self, db: &Connection) -> PResult<HandyQualificationReport> {
        authenticate_session(db, &self.actor, self.request.auth_token.as_deref())?;
        require_grant(db, &self.actor, project(&self.request)?, Scope::Read)?;
        require_grant(db, &self.actor, project(&self.request)?, Scope::Export)?;
        check_revision(db, &self.request)?;
        check_request_identity(db, self.actor.as_str(), &self.request, &self.fingerprint)?;
        let registration = handy_registrations::current(
            db,
            &self.session.registration.device_id,
            self.session.registration.revision,
        )?;
        if registration != self.session.registration || registration.disabled {
            return Err(mismatch());
        }
        handy_qualification::accepted_hsp_record(db, &self.qualification, &registration)
    }
    fn validate_session(&self, sessions: &handy_sessions::Registry) -> PResult<()> {
        let (session, credentials) = sessions.capture(&self.actor, &self.session.device_session)?;
        if session.engine_epoch != self.session.engine_epoch
            || session.device_session != self.session.device_session
            || session.open_request_id != self.session.open_request_id
            || session.registration != self.session.registration
            || !Arc::ptr_eq(&credentials, &self.credentials)
        {
            return Err(mismatch());
        }
        Ok(())
    }
}
impl Entry {
    fn budget(&self) -> PResult<u64> {
        if self.cancel.load(Ordering::Acquire) || self.deps.shutdown.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let now = self.io.now()?;
        if now < self.deps.started_ns
            || now >= self.deps.deadline_ns
            || Instant::now() >= self.deps.deadline
        {
            return Err(unavailable("original HSP preparation deadline expired"));
        }
        Ok(now)
    }
    fn retire(&self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(owner) = self.continuity.get() {
            owner.request_stop();
        }
        if let Ok(mut state) = self.state.try_lock() {
            state.phase = HandyHspPreparationPhase::Retired;
            state.ready = None;
            state.candidate = None;
            if let Some(child) = &state.child {
                child.cancel();
            }
        }
    }
    fn set_phase(&self, phase: HandyHspPreparationPhase) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = phase;
        }
    }
    fn lock<'a, T>(&self, mutex: &'a Mutex<T>) -> PResult<std::sync::MutexGuard<'a, T>> {
        loop {
            self.budget()?;
            match mutex.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(unavailable("HSP preparation dependency unavailable"))
                }
                Err(TryLockError::WouldBlock) => thread::park_timeout(POLL),
            }
        }
    }
    fn current(
        &self,
        observation: Option<&HandySetupDeviceObservation>,
    ) -> PResult<Option<handy_qualification::QualifiedHspBundle>> {
        self.budget()?;
        let _admission = self
            .deps
            .admission
            .try_read()
            .map_err(|_| unavailable("HSP recovery admission is fenced"))?;
        let db = self.lock(&self.deps.db)?;
        self.deps.validate_db(&db)?;
        let sessions = self.lock(&self.deps.sessions)?;
        self.deps.validate_session(&sessions)?;
        let bundle = observation
            .map(|observation| {
                handy_qualification::accepted_hsp_bundle(
                    &db,
                    &self.deps.qualification,
                    &self.deps.session.registration,
                    observation,
                )
            })
            .transpose()?;
        self.budget()?;
        Ok(bundle)
    }
    fn verify_read(&self, read: &DeviceTimingRead) -> PResult<()> {
        read.before.validate()?;
        read.after.validate()?;
        read.first.validate()?;
        read.second.validate()?;
        if !read.before.same_state(&read.after)
            || read.first.read_started_boottime_ns < self.deps.started_ns
            || read.first.read_finished_boottime_ns > read.second.read_started_boottime_ns
            || read.second.read_finished_boottime_ns >= self.deps.deadline_ns
            || read.second.info.device_uptime_ms < read.first.info.device_uptime_ms
            || read.first.user_offset != read.second.user_offset
            || read.after.mode.mode != 4
            || read.after.remote_hsp.as_ref().is_none_or(|state| {
                !matches!(
                    state.play_state,
                    HandyRemotePlayState::NotInitialized | HandyRemotePlayState::Stopped
                )
            })
        {
            return Err(unavailable(
                "fresh HSP idle device timing context is invalid or changed",
            ));
        }
        self.budget()?;
        Ok(())
    }
    fn save_witness(&self, history: HandyHspPreparationHistory) -> PResult<()> {
        let report = HandyHspPreparationStatus {
            project_id: project(&self.deps.request)?.clone(),
            preparation_request_id: self.deps.request.request_id.clone(),
            history: Some(history),
            native: None,
            armed: false,
            playback_enabled: false,
        };
        report.validate()?;
        let mut db = self.lock(&self.deps.db)?;
        check_request_identity(
            &db,
            self.deps.actor.as_str(),
            &self.deps.request,
            &self.deps.fingerprint,
        )?;
        let tx = db.transaction().map_err(internal)?;
        let changed = tx.execute("UPDATE requests SET response=?1 WHERE session=?2 AND request=?3 AND fingerprint=?4", params![encode(&ResponseBody::HandyHspPreparation(Box::new(report)))?, self.deps.actor.as_str(), self.deps.request.request_id.as_str(), self.deps.fingerprint]).map_err(internal)?;
        if changed != 1 {
            return Err(mismatch());
        }
        tx.commit().map_err(internal)?;
        self.budget()?;
        Ok(())
    }
    fn persist(&self) -> PResult<()> {
        self.persist_with_journal_wait(false)
    }
    fn persist_until_deadline(&self) -> PResult<()> {
        self.persist_with_journal_wait(true)
    }
    fn persist_with_journal_wait(&self, wait: bool) -> PResult<()> {
        let pending = self.state.lock().map_err(internal)?.pending.clone();
        let Some(pending) = pending else {
            return Ok(());
        };
        // Only this owned worker performs retry I/O. It cannot resend sync or
        // refresh an original deadline; cancellation does not discard history.
        let mut journal = loop {
            // Only the still-active preparation may wait, under its original
            // clocks, cancellation and wall deadline. Retirement/metadata-only
            // retries stay nonblocking and cannot revive readiness or resync.
            if wait {
                self.budget()?;
            }
            match self.deps.journal.try_lock() {
                Ok(journal) => break journal,
                Err(TryLockError::WouldBlock) => {
                    #[cfg(test)]
                    self.io.metadata_contended()?;
                    if !wait {
                        return Err(unavailable("HSP preparation metadata is pending"));
                    }
                    thread::park_timeout(POLL);
                }
                Err(TryLockError::Poisoned(_)) => {
                    return Err(unavailable("HSP preparation metadata is pending"));
                }
            }
        };
        match &pending {
            PendingMetadata::Complete(claim, outcome) => {
                journal
                    .complete_hsp_preparation(claim, outcome.clone())
                    .map_err(internal)?;
            }
            PendingMetadata::Cancel(prepared) => {
                if journal.cancel_hsp_preparation(prepared).is_err() {
                    // Cancellation cannot label a possibly committed claim as
                    // unsent. Preserve exact original custody as Unknown; an
                    // existing Ack/Unknown remains unchanged. No receipt or
                    // synchronization capability is reconstructed here.
                    journal
                        .mark_hsp_preparation_unknown_original(
                            prepared.id(),
                            &self.deps.actor,
                            &self.deps.request.request_id,
                        )
                        .map_err(internal)?;
                }
            }
            PendingMetadata::ClaimUncertain(id) => {
                if journal
                    .mark_hsp_preparation_unknown_original(
                        id,
                        &self.deps.actor,
                        &self.deps.request.request_id,
                    )
                    .is_err()
                {
                    journal
                        .cancel_hsp_preparation_original(
                            id,
                            &self.deps.actor,
                            &self.deps.request.request_id,
                        )
                        .map_err(internal)?;
                }
            }
        }
        drop(journal);
        self.state.lock().map_err(internal)?.pending = None;
        Ok(())
    }
    fn run_native(&self) -> PResult<()> {
        self.current(None)?;
        let mut clock = || self.io.now();
        let before = self.io.inspect(
            self.deps.credentials.clone(),
            self.deps.deadline,
            &mut clock,
        )?;
        self.verify_read(&before)?;
        let qualified = self.current(Some(&before.after))?.ok_or_else(mismatch)?;
        let clocks = handy_qualification::hsp_clock_contracts(&qualified)?;
        if before.second.user_offset.offset_ms != clocks.1.contract.user_offset_ms {
            return Err(mismatch());
        }
        let record = HspPreparationRecord {
            boot: self.deps.epoch.clone(),
            actor_session: self.deps.actor.clone(),
            request_id: self.deps.request.request_id.clone(),
            request_fingerprint: self.deps.fingerprint.clone(),
            project_id: project(&self.deps.request)?.clone(),
            revision: self.deps.request.expected_revision.ok_or_else(mismatch)?,
            device_session: self.deps.session.device_session.clone(),
            registration: self.deps.session.registration.clone(),
            device: before.after.identity.clone(),
            mode: before.after.mode.clone(),
            operative_settings_json: serde_json::to_string(&before.after.operative_settings)
                .map_err(internal)?,
            qualification: self.deps.qualification.clone(),
            subject_sha256: qualified.subject_sha256.clone(),
            profile_sha256: qualified.profile_sha256.clone(),
            route_sha256: self.deps.route.route_sha256().into(),
            route_namespace_sha256: self.deps.route.namespace_sha256().into(),
            user_offset_before: before.second.user_offset.clone(),
            started_boottime_ns: self.deps.started_ns,
            deadline_boottime_ns: self.deps.deadline_ns,
        };
        record.validate().map_err(internal)?;
        let prepared = self
            .lock(&self.deps.journal)?
            .prepare_hsp_preparation(&record)
            .map_err(internal)?;
        {
            let mut state = self.state.lock().map_err(internal)?;
            state.record = Some(record.clone());
            state.prepared = Some(prepared.clone());
        }
        let history = self
            .lock(&self.deps.journal)?
            .hsp_preparation_history(&self.deps.actor, &self.deps.request.request_id)
            .map_err(internal)?
            .ok_or_else(mismatch)?
            .history()?;
        self.save_witness(history)?;
        self.io.before_claim()?;
        self.budget()?;
        let claim = {
            let _admission = self
                .deps
                .admission
                .try_read()
                .map_err(|_| unavailable("HSP recovery admission is fenced"))?;
            let db = self.lock(&self.deps.db)?;
            self.deps.validate_db(&db)?;
            let sessions = self.lock(&self.deps.sessions)?;
            self.deps.validate_session(&sessions)?;
            let mut journal = self.lock(&self.deps.journal)?;
            self.budget()?;
            self.claim_entered.store(true, Ordering::Release);
            let claim = match journal.claim_hsp_preparation(&prepared) {
                Ok(claim) => claim,
                Err(error) => {
                    self.claim_entered.store(false, Ordering::Release);
                    return Err(internal(error));
                }
            };
            self.state.lock().map_err(internal)?.claimed = Some(claim.clone());
            claim
        };
        self.set_phase(HandyHspPreparationPhase::Synchronizing);
        self.io.after_claim()?;
        self.budget()?;
        self.current(Some(&before.after))?;
        let completion = self.io.synchronize(
            self.deps.credentials.clone(),
            self.deps.deadline,
            &mut clock,
        );
        let completion = match completion {
            ClockSyncCompletion::Acknowledged(ref ack)
                if ack.sent_boottime_ns < self.deps.started_ns
                    || ack.received_boottime_ns >= self.deps.deadline_ns =>
            {
                ClockSyncCompletion::Unknown
            }
            completion => completion,
        };
        {
            let mut state = self.state.lock().map_err(internal)?;
            state.completion = Some(completion.clone());
            state.pending = Some(PendingMetadata::Complete(claim, completion.clone()));
        }
        let persisted = self.persist_until_deadline();
        let ClockSyncCompletion::Acknowledged(ack) = completion else {
            return Err(unavailable(
                "original HSP synchronization outcome is unknown",
            ));
        };
        self.budget()?;
        self.set_phase(HandyHspPreparationPhase::Checking);
        let device = NativeDeviceClock::new(ack, clocks.1)?;
        let after = self.io.inspect(
            self.deps.credentials.clone(),
            self.deps.deadline,
            &mut clock,
        )?;
        self.verify_read(&after)?;
        if !before.after.same_state(&after.before) || !before.after.same_state(&after.after) {
            return Err(mismatch());
        }
        device.check_observation(&after.first)?;
        device.check_observation(&after.second)?;
        let current = self.current(Some(&after.after))?.ok_or_else(mismatch)?;
        if current.subject_sha256 != qualified.subject_sha256
            || current.profile_sha256 != qualified.profile_sha256
        {
            return Err(mismatch());
        }
        let sample = self.io.server_sample(self.deps.deadline, &mut clock)?;
        if sample.sent_ns < after.second.read_finished_boottime_ns
            || sample.received_ns >= self.deps.deadline_ns
        {
            return Err(mismatch());
        }
        let domain = ArtifactId::new(format!("hsp-clock-{}", record.id().map_err(internal)?))
            .map_err(internal)?;
        let mut server = NativeServerClock::new(domain, sample, clocks.0)?;
        server.clock.valid_until_ns = server.clock.valid_until_ns.min(self.deps.deadline_ns).min(
            after.first.read_started_boottime_ns.saturating_add(
                qualified
                    .report
                    .manifest
                    .subject
                    .profile
                    .maximum_command_age_ns,
            ),
        );
        let native = Arc::new(NativePlaybackClock { server, device });
        let now = self.budget()?;
        let clock = native.at(now)?;
        let valid_until_ns = clock.valid_until_ns.min(self.deps.deadline_ns).min(
            after.first.read_started_boottime_ns.saturating_add(
                qualified
                    .report
                    .manifest
                    .subject
                    .profile
                    .maximum_command_age_ns,
            ),
        );
        if now >= valid_until_ns {
            return Err(unavailable("original HSP clocks or settings expired"));
        }
        self.current(Some(&after.after))?;
        self.state.lock().map_err(internal)?.candidate = Some(Readiness {
            record,
            parent: prepared,
            clocks: native,
            qualified,
            observation: after.after,
            valid_until_ns,
        });
        persisted?;
        self.install_candidate()
    }
    fn install_candidate(&self) -> PResult<()> {
        let now = self.budget()?;
        self.current(None)?;
        let mut state = self.state.lock().map_err(internal)?;
        if state.pending.is_some() || state.consumed {
            return Err(mismatch());
        }
        let candidate = state.candidate.take().ok_or_else(mismatch)?;
        candidate.clocks.at(now)?;
        if now >= candidate.valid_until_ns {
            return Err(unavailable("original HSP readiness expired"));
        }
        state.ready = Some(candidate);
        state.phase = HandyHspPreparationPhase::Ready;
        state.error = None;
        Ok(())
    }
    fn finish_failure(&self, error: ProtocolError) {
        if let Ok(mut state) = self.state.lock() {
            state.phase = HandyHspPreparationPhase::Retired;
            state.error = Some(error);
            state.ready = None;
            if state.pending.is_none() && state.candidate.is_none() && state.completion.is_none() {
                state.pending = if let Some(claim) = &state.claimed {
                    Some(PendingMetadata::Complete(
                        claim.clone(),
                        ClockSyncCompletion::Unknown,
                    ))
                } else if self.claim_entered.load(Ordering::Acquire) {
                    state
                        .record
                        .as_ref()
                        .and_then(|record| record.id().ok())
                        .map(PendingMetadata::ClaimUncertain)
                } else {
                    state.prepared.clone().map(PendingMetadata::Cancel)
                };
            }
        }
        let _ = self.persist();
    }
    fn spawn(self: &Arc<Self>, metadata_only: bool) -> PResult<()> {
        self.join_finished()?;
        let mut slot = self.worker.lock().map_err(internal)?;
        if slot.is_some() || self.active.swap(true, Ordering::AcqRel) {
            return Err(unavailable(
                "original HSP preparation worker is still owned",
            ));
        }
        let captured = self.clone();
        match thread::Builder::new()
            .name("pulsar-hsp-preparation".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if metadata_only {
                        captured.persist()?;
                        if captured.state.lock().map_err(internal)?.candidate.is_some() {
                            captured.install_candidate()?;
                        }
                        Ok(())
                    } else {
                        captured.run_native()
                    }
                }))
                .unwrap_or_else(|_| Err(unavailable("original HSP preparation worker failed")));
                if let Err(error) = result {
                    captured.finish_failure(error);
                }
                captured.active.store(false, Ordering::Release);
            }) {
            Ok(worker) => {
                *slot = Some(worker);
                Ok(())
            }
            Err(_) => {
                self.active.store(false, Ordering::Release);
                self.finish_failure(unavailable("HSP preparation worker unavailable"));
                Err(unavailable("HSP preparation worker unavailable"))
            }
        }
    }
    fn join_finished(&self) -> PResult<bool> {
        let worker = {
            let mut slot = self.worker.lock().map_err(internal)?;
            if slot.as_ref().is_some_and(|worker| !worker.is_finished()) {
                return Ok(false);
            }
            slot.take()
        };
        if let Some(worker) = worker {
            worker
                .join()
                .map_err(|_| unavailable("HSP preparation worker failed during join"))?;
        }
        Ok(!self.active.load(Ordering::Acquire))
    }
    fn join_all(&self) -> PResult<()> {
        self.retire();
        let worker = self.worker.lock().map_err(internal)?.take();
        if let Some(worker) = worker {
            worker
                .join()
                .map_err(|_| unavailable("HSP preparation worker failed during shutdown"))?;
        }
        if let Some(owner) = self.continuity.get() {
            owner.join()?;
        }
        if self.state.lock().map_err(internal)?.pending.is_some() {
            Err(unavailable("HSP preparation metadata remains unresolved"))
        } else {
            Ok(())
        }
    }
    fn native_status(&self) -> PResult<HandyHspPreparationNativeStatus> {
        self.join_finished()?;
        let state = self.state.lock().map_err(internal)?;
        let cancelled =
            self.cancel.load(Ordering::Acquire) || self.deps.shutdown.load(Ordering::Acquire);
        let valid_until = state.ready.as_ref().and_then(|ready| {
            let now = self.io.now().ok()?;
            (!cancelled
                && !state.consumed
                && now < ready.valid_until_ns
                && ready.clocks.at(now).is_ok())
            .then_some(ready.valid_until_ns)
        });
        Ok(HandyHspPreparationNativeStatus {
            phase: if cancelled
                || state.consumed
                || (state.phase == HandyHspPreparationPhase::Ready && valid_until.is_none())
            {
                HandyHspPreparationPhase::Retired
            } else {
                state.phase
            },
            worker_active: self.active.load(Ordering::Acquire),
            cancel_requested: cancelled,
            metadata_pending: state.pending.is_some(),
            clock_valid_until_boottime_ns: valid_until,
            error: state.error.clone(),
        })
    }
}

pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::PrepareHandyHsp { .. }
            | Command::HandyHspPreparationStatus { .. }
            | Command::CancelHandyHspPreparation { .. }
    )
}
fn empty_status(project_id: ProjectId, request: RequestId) -> HandyHspPreparationStatus {
    HandyHspPreparationStatus {
        project_id,
        preparation_request_id: request,
        history: None,
        native: None,
        armed: false,
        playback_enabled: false,
    }
}
fn stored_original(
    db: &Connection,
    actor: &SessionId,
    id: &RequestId,
    project: &ProjectId,
) -> PResult<HandyHspPreparationStatus> {
    let raw: Option<String> = db
        .query_row(
            "SELECT response FROM requests WHERE session=?1 AND request=?2",
            params![actor.as_str(), id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let Some(raw) = raw else {
        return Err(ProtocolError::new(
            ErrorCode::NotFound,
            "original HSP preparation request not found",
        ));
    };
    let ResponseBody::HandyHspPreparation(status) = decode(&raw)? else {
        return Err(mismatch());
    };
    if status.project_id != *project || status.preparation_request_id != *id {
        return Err(mismatch());
    }
    status.validate()?;
    Ok(*status)
}
impl Engine {
    pub(in crate::authority) fn handy_hsp_preparation_control(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        #[cfg(test)]
        let io = self.public_hsp_preparation_io_for_test(request, actor)?;
        #[cfg(not(test))]
        let io: Arc<dyn HspPreparationIo> = Arc::new(NativeIo);
        self.handy_hsp_preparation_impl(request, actor, fingerprint, io)
    }
    #[cfg(test)]
    pub(in crate::authority) fn install_handy_hsp_preparation_io_for_test(
        &self,
        actor: &SessionId,
        engine_epoch: &DeviceSessionId,
        device_session: &DeviceSessionId,
        io: Arc<dyn HspPreparationIo>,
    ) -> PResult<()> {
        if self.is_shutting_down()
            || engine_epoch.as_str() != self.engine_epoch
            || engine_epoch == device_session
        {
            return Err(mismatch());
        }
        let origin = io
            .hsp_origin()
            .ok_or_else(|| unavailable("test preparation requires loopback HSP origin"))?;
        let _loopback_address = origin
            .strip_prefix("http://")
            .and_then(|host| host.parse::<std::net::SocketAddr>().ok())
            .filter(|address| address.ip().is_loopback() && address.port() != 0)
            .ok_or_else(|| unavailable("test preparation requires numeric loopback HTTP origin"))?;
        // Validation only: never resolve or contact the origin.
        let _admission = self
            .handy_reconciliation_admission
            .try_write()
            .map_err(|_| unavailable("HSP preparation admission already in progress"))?;
        let sessions = self.handy_sessions.lock().map_err(internal)?;
        let (connection, _) = sessions.capture(actor, device_session)?;
        if connection.engine_epoch != *engine_epoch || connection.device_session != *device_session
        {
            return Err(mismatch());
        }
        let mut registry = self.handy_hsp_preparations.lock().map_err(internal)?;
        if registry.public_test_io.is_some()
            || registry.public_admission_started
            || !registry.entries.is_empty()
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "test preparation IO must be installed once before original admission",
            ));
        }
        registry.public_test_io = Some(PublicTestIo {
            actor: actor.clone(),
            engine_epoch: engine_epoch.clone(),
            device_session: device_session.clone(),
            io: Arc::new(LoopbackTestIo { inner: io, origin }),
        });
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::authority) fn public_hsp_preparation_io_for_test(
        &self,
        request: &Request,
        actor: &SessionId,
    ) -> PResult<Arc<dyn HspPreparationIo>> {
        let Command::PrepareHandyHsp {
            engine_epoch,
            device_session,
            ..
        } = &request.command
        else {
            // Status/cancel use the unchanged metadata path and each retained
            // entry's original IO. No new IO is invoked by these commands.
            return Ok(Arc::new(NativeIo));
        };
        let mut registry = self.handy_hsp_preparations.try_lock().map_err(internal)?;
        registry.public_admission_started = true;
        if let Some(selected) = &registry.public_test_io {
            if request.session.as_ref() != Some(actor)
                || actor != &selected.actor
                || engine_epoch != &selected.engine_epoch
                || device_session != &selected.device_session
                || engine_epoch.as_str() != self.engine_epoch
            {
                return Err(mismatch());
            }
            return Ok(selected.io.clone());
        }
        Ok(Arc::new(NativeIo))
    }
    #[cfg(test)]
    pub(in crate::authority) fn handy_hsp_preparation_with_io(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        io: Arc<dyn HspPreparationIo>,
    ) -> PResult<ResponseBody> {
        self.handy_hsp_preparation_impl(request, actor, fingerprint, io)
    }
    fn hsp_preparation_report(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        id: &RequestId,
    ) -> PResult<HandyHspPreparationStatus> {
        let mut status = {
            let db = self.db.lock().map_err(internal)?;
            stored_original(&db, actor, id, project_id)?
        };
        let observation = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .hsp_preparation_history(actor, id)
            .map_err(internal)?;
        if let Some(observation) = observation {
            if observation.record.project_id != *project_id {
                return Err(mismatch());
            }
            status.history = Some(observation.history()?);
        }
        let entry = self
            .handy_hsp_preparations
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), id.clone()))
            .cloned();
        status.native = entry.map(|entry| entry.native_status()).transpose()?;
        if status
            .native
            .as_ref()
            .is_some_and(|native| native.phase == HandyHspPreparationPhase::Ready)
        {
            // The worker may have installed readiness between the first history
            // read and native snapshot. Re-read only metadata; never refresh
            // clocks. A concurrent consumption/resolution is reported retired.
            status.history = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .hsp_preparation_history(actor, id)
                .map_err(internal)?
                .map(|observation| observation.history())
                .transpose()?;
            if status.history.as_ref().is_none_or(|history| {
                history.state != HandyClockSyncState::Acknowledged
                    || history.linked_stream_id.is_some()
                    || history.custody_transferred
                    || history.resolved
            }) {
                if let Some(native) = &mut status.native {
                    native.phase = HandyHspPreparationPhase::Retired;
                    native.clock_valid_until_boottime_ns = None;
                }
            }
        }
        status.validate()?;
        Ok(status)
    }
    fn handy_hsp_preparation_impl(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        io: Arc<dyn HspPreparationIo>,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        // Fresh admission must consume the budget captured before any lock or
        // storage work. A historical replay does not require this clock result;
        // status and cancellation do not sample a starting clock at all.
        let starting = matches!(request.command, Command::PrepareHandyHsp { .. }).then(|| {
            let instant = Instant::now();
            io.now().map(|now| (now, instant))
        });
        let _admission = if matches!(
            request.command,
            Command::PrepareHandyHsp { .. } | Command::CancelHandyHspPreparation { .. }
        ) {
            Some(
                self.handy_reconciliation_admission
                    .try_read()
                    .map_err(|_| unavailable("HSP recovery admission is fenced"))?,
            )
        } else {
            None
        };
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        let project_id = project(request)?.clone();
        if let Command::HandyHspPreparationStatus {
            preparation_request_id,
            ..
        } = &request.command
        {
            stored_original(&db, actor, preparation_request_id, &project_id)?;
            drop(db);
            return self
                .hsp_preparation_report(actor, &project_id, preparation_request_id)
                .map(|value| ResponseBody::HandyHspPreparation(Box::new(value)));
        }
        if let Command::CancelHandyHspPreparation {
            preparation_request_id,
            ..
        } = &request.command
        {
            let saved = stored_original(&db, actor, preparation_request_id, &project_id)?;
            if replay(&db, actor.as_str(), request, fingerprint)?.is_none() {
                let tx = db.transaction().map_err(internal)?;
                let mut historical = saved;
                historical.native = None;
                remember(
                    &tx,
                    actor.as_str(),
                    request,
                    fingerprint,
                    &ResponseBody::HandyHspPreparation(Box::new(historical)),
                )?;
                tx.commit().map_err(internal)?;
            }
            let entry = self
                .handy_hsp_preparations
                .lock()
                .map_err(internal)?
                .entries
                .get(&(actor.clone(), preparation_request_id.clone()))
                .cloned();
            drop(db);
            if let Some(entry) = &entry {
                entry.retire();
                if entry.join_finished()? {
                    let mut state = entry.state.lock().map_err(internal)?;
                    if state.pending.is_none() && state.claimed.is_none() {
                        state.pending = state.prepared.clone().map(PendingMetadata::Cancel);
                    }
                    let pending = state.pending.is_some();
                    drop(state);
                    if pending {
                        entry.spawn(true)?;
                    }
                }
            } else {
                let mut journal = self._device_effect_journal.lock().map_err(internal)?;
                if let Some(history) = journal
                    .hsp_preparation_history(actor, preparation_request_id)
                    .map_err(internal)?
                {
                    if history.state == HandyClockSyncState::Prepared {
                        journal
                            .cancel_hsp_preparation_original(
                                &history.id,
                                actor,
                                preparation_request_id,
                            )
                            .map_err(internal)?;
                    }
                }
            }
            return self
                .hsp_preparation_report(actor, &project_id, preparation_request_id)
                .map(|value| ResponseBody::HandyHspPreparation(Box::new(value)));
        }
        let Command::PrepareHandyHsp {
            engine_epoch,
            device_session,
            qualification,
            timeout_ms,
            allow_online_synchronization,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::invalid("expected HSP preparation command"));
        };
        if let Some(body) = replay(&db, actor.as_str(), request, fingerprint)? {
            let ResponseBody::HandyHspPreparation(saved) = body else {
                return Err(mismatch());
            };
            if saved.preparation_request_id != request.request_id || saved.project_id != project_id
            {
                return Err(mismatch());
            }
            let entry = self
                .handy_hsp_preparations
                .lock()
                .map_err(internal)?
                .entries
                .get(&(actor.clone(), request.request_id.clone()))
                .cloned();
            drop(db);
            if let Some(entry) = entry {
                if entry.join_finished()? && entry.state.lock().map_err(internal)?.pending.is_some()
                {
                    entry.spawn(true)?;
                }
            }
            return self
                .hsp_preparation_report(actor, &project_id, &request.request_id)
                .map(|value| ResponseBody::HandyHspPreparation(Box::new(value)));
        }
        if !allow_online_synchronization {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "HSP preparation requires explicit online synchronization consent",
            ));
        }
        let epoch = DeviceSessionId::new(self.engine_epoch.clone()).map_err(internal)?;
        if *engine_epoch != epoch {
            return Err(mismatch());
        }
        check_revision(&db, request)?;
        let (session, credentials) = self
            .handy_sessions
            .lock()
            .map_err(internal)?
            .capture(actor, device_session)?;
        let registration = handy_registrations::current(
            &db,
            &session.registration.device_id,
            session.registration.revision,
        )?;
        if registration != session.registration {
            return Err(mismatch());
        }
        handy_qualification::accepted_hsp_record(&db, qualification, &registration)?;
        let (started_ns, started_instant) = starting.ok_or_else(mismatch)??;
        let duration = Duration::from_millis(*timeout_ms);
        let deadline = started_instant.checked_add(duration).ok_or_else(mismatch)?;
        let deadline_ns = started_ns
            .checked_add(timeout_ms.checked_mul(1_000_000).ok_or_else(mismatch)?)
            .ok_or_else(mismatch)?;
        let route =
            HandyRouteIdentity::from_snapshot(&self.handy_owner_token, &credentials.connection)?;
        let entry = Arc::new(Entry {
            deps: Dependencies {
                db: self.db.clone(),
                sessions: self.handy_sessions.clone(),
                admission: self.handy_reconciliation_admission.clone(),
                journal: self._device_effect_journal.clone(),
                shutdown: self.shutdown_requested.clone(),
                epoch,
                request: request.clone(),
                actor: actor.clone(),
                fingerprint: fingerprint.into(),
                session,
                credentials,
                qualification: qualification.clone(),
                route,
                started_ns,
                deadline_ns,
                deadline,
            },
            io,
            cancel: AtomicBool::new(false),
            claim_entered: AtomicBool::new(false),
            active: AtomicBool::new(false),
            state: Mutex::new(State {
                phase: HandyHspPreparationPhase::Inspecting,
                error: None,
                record: None,
                prepared: None,
                claimed: None,
                completion: None,
                pending: None,
                candidate: None,
                ready: None,
                child: None,
                consumed: false,
                consumed_source: None,
                resolved: false,
            }),
            worker: Mutex::new(None),
            continuity: std::sync::OnceLock::new(),
            #[cfg(test)]
            preclaim_test: Mutex::new(None),
        });
        let mut registry = self.handy_hsp_preparations.lock().map_err(internal)?;
        if registry.entries.len() >= CAPACITY {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "native HSP preparation capacity reached",
            ));
        }
        entry.budget()?;
        let status = empty_status(project_id.clone(), request.request_id.clone());
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            request,
            fingerprint,
            &ResponseBody::HandyHspPreparation(Box::new(status)),
        )?;
        tx.commit().map_err(internal)?;
        registry
            .entries
            .insert((actor.clone(), request.request_id.clone()), entry.clone());
        drop(registry);
        drop(db);
        if let Err(error) = entry.budget().and_then(|_| entry.spawn(false)) {
            entry.finish_failure(error);
        }
        self.hsp_preparation_report(actor, &project_id, &request.request_id)
            .map(|value| ResponseBody::HandyHspPreparation(Box::new(value)))
    }
    pub(in crate::authority) fn shutdown_handy_hsp_preparations(&self) -> PResult<()> {
        let entries = self
            .handy_hsp_preparations
            .lock()
            .map_err(internal)?
            .entries
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for entry in &entries {
            entry.retire();
        }
        let mut failure = None;
        for entry in entries {
            if let Err(error) = entry.join_all() {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

pub(in crate::authority) fn validate_journal_links(
    db: &Connection,
    state_dir: &std::path::Path,
) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut query = db.prepare("SELECT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,response FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_hsp_preparation' ELSE response LIKE '%handy_hsp_preparation%' END LIMIT 2049")?;
    let mut witnesses = Vec::new();
    let mut count = 0;
    for row in query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (actor, raw) = row?;
        count += 1;
        anyhow::ensure!(raw.len() <= 32768, "HSP preparation witness exceeds bound");
        let ResponseBody::HandyHspPreparation(status) = serde_json::from_str(&raw)? else {
            continue;
        };
        status.validate()?;
        anyhow::ensure!(
            status.native.is_none(),
            "native HSP readiness cannot be restored from request history"
        );
        if let Some(history) = status.history {
            witnesses.push((
                SessionId::new(actor)?,
                status.preparation_request_id,
                history.record_id.as_str().to_owned(),
            ));
        }
    }
    anyhow::ensure!(count <= 2048, "HSP preparation witnesses exceed capacity");
    crate::device_effect_journal::DeviceEffectJournal::verify_hsp_preparation_records(
        state_dir, &witnesses,
    )
}

/// Native, source-independent preparation context. Cloning the retained clock
/// does not consume readiness or restore it from history; the handoff rechecks
/// the exact original readiness after the source factory has finished.
pub(super) struct HspPreparedSourceContext {
    pub record: HspPreparationRecord,
    pub clocks: Arc<NativePlaybackClock>,
    pub policy: HspPolicy,
    pub profile: HandyHsspQualificationProfile,
    pub valid_until_ns: u64,
}

impl HspPreparedSourceContext {
    pub(super) fn stream_record(
        &self,
        actor: &SessionId,
        request: &RequestId,
        binding: &HspBinding,
    ) -> HspStreamRecord {
        stream_record(&self.record, actor, request, binding)
    }
}
fn stream_record(
    prepared: &HspPreparationRecord,
    actor: &SessionId,
    request: &RequestId,
    binding: &HspBinding,
) -> HspStreamRecord {
    HspStreamRecord {
        boot: prepared.boot.clone(),
        actor_session: actor.clone(),
        request_id: request.clone(),
        registration: prepared.registration.clone(),
        device: prepared.device.clone(),
        route_sha256: prepared.route_sha256.clone(),
        route_namespace_sha256: prepared.route_namespace_sha256.clone(),
        binding: HspBindingRecord::capture(binding),
        qualification_sha256: prepared.qualification.dossier_id.as_str().into(),
        profile_sha256: prepared.profile_sha256.clone(),
    }
}

pub(super) struct HspSourceHandoff {
    pub child_request: RequestId,
    pub allow_online_control: bool,
    pub binding: HspBinding,
    pub authority: Arc<dyn HspNativeAuthority>,
}

struct PreparedClockGuard {
    entry: Arc<Entry>,
    binding: HspBinding,
    clocks: Arc<NativePlaybackClock>,
    valid_until_ns: u64,
}
impl PreparedClockGuard {
    fn validate(
        &self,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> std::result::Result<(), HspDispatchFailure> {
        if self.entry.cancel.load(Ordering::Acquire)
            || self.entry.deps.shutdown.load(Ordering::Acquire)
        {
            return Err(HspDispatchFailure::Cancelled);
        }
        if let Some(pin) = facts.clock_pin() {
            if facts.binding() != &self.binding {
                return Err(HspDispatchFailure::BindingMismatch);
            }
            return self
                .entry
                .continuity
                .get()
                .ok_or(HspDispatchFailure::BindingMismatch)?
                .validate_pin(pin, now_ns, facts)
                .map_err(|_| HspDispatchFailure::InvalidClock);
        }
        let reference = facts.reference();
        if facts.binding() != &self.binding
            || self.binding.clock_domain() != &self.clocks.server.clock.domain
            || reference.at_ns() > now_ns
            || now_ns >= self.valid_until_ns
            || facts.window().clock_until_ns() > self.valid_until_ns
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        let observed = self
            .clocks
            .at(reference.at_ns())
            .map_err(|_| HspDispatchFailure::InvalidClock)?;
        self.clocks
            .at(now_ns)
            .map_err(|_| HspDispatchFailure::InvalidClock)?;
        let center = i128::from(reference.start_time_ms());
        let margin = |distance: Option<i128>| -> std::result::Result<u128, HspDispatchFailure> {
            distance
                .and_then(|value| u128::try_from(value).ok())
                .and_then(|value| value.checked_mul(1_000_000))
                .ok_or(HspDispatchFailure::InvalidClock)
        };
        let before = margin(center.checked_sub(i128::from(reference.phase().earliest_ms)))?;
        let after = margin(i128::from(reference.phase().latest_ms).checked_sub(center))?;
        if reference.server_time_ms() != observed.estimated_server_ms
            || facts.window().clock_until_ns() > observed.valid_until_ns
            || before < u128::from(observed.uncertainty_ns)
            || after < u128::from(observed.uncertainty_ns)
        {
            return Err(HspDispatchFailure::InvalidClock);
        }
        Ok(())
    }
}
struct PreparedSourceAuthority {
    clocks: Arc<PreparedClockGuard>,
    source: Arc<dyn HspNativeAuthority>,
}
impl PreparedSourceAuthority {
    fn validate_snapshot(
        &self,
        now_ns: u64,
        snapshot: &HspAuthoritySnapshot,
    ) -> std::result::Result<(), HspDispatchFailure> {
        if let Some(current) = &snapshot.facts {
            self.clocks.validate(now_ns, current)?;
        }
        if let Some(projected) = &snapshot.projected {
            let current = snapshot
                .facts
                .as_ref()
                .ok_or(HspDispatchFailure::InvalidClock)?;
            if current.reference().at_ns() != now_ns
                || projected.reference().at_ns() <= now_ns
                || projected.binding() != current.binding()
                || projected.window() != current.window()
            {
                return Err(HspDispatchFailure::InvalidClock);
            }
            // Prediction keeps the same original clock capability and ceiling.
            self.clocks
                .validate(projected.reference().at_ns(), projected)?;
        }
        Ok(())
    }
    fn validate_observation(
        &self,
        evaluation: HspAuthorityEvaluation,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        if evaluation
            .snapshot
            .facts
            .as_ref()
            .is_some_and(|facts| facts.reference().at_ns() != evaluation.sampled_at_ns)
        {
            return Err(HspDispatchFailure::InvalidClock);
        }
        self.validate_snapshot(evaluation.sampled_at_ns, &evaluation.snapshot)?;
        Ok(evaluation)
    }
    fn preclaim_checkpoint(
        &self,
        gate: &mut HspClaimGate<'_>,
        owner: &HspNativeOwner,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> std::result::Result<(), HspDispatchFailure> {
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(HspDispatchFailure::Cancelled);
        }
        gate.check_preclaim_wait(owner)?;
        if let Some(owner) = self.clocks.entry.continuity.get() {
            owner
                .checkpoint()
                .map_err(|_| HspDispatchFailure::Cancelled)?;
        } else {
            self.clocks
                .entry
                .budget()
                .map_err(|_| HspDispatchFailure::Cancelled)?;
        }
        Ok(())
    }
    fn wait_preclaim(
        &self,
        gate: &mut HspClaimGate<'_>,
        owner: &HspNativeOwner,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> std::result::Result<(), HspDispatchFailure> {
        // The gate retains one first-contention cap across every lock site.
        // No dependency guard is held here, and a wake cannot renew its expiry.
        let delay = gate.preclaim_retry_delay(owner)?;
        self.preclaim_checkpoint(gate, owner, deadline, cancel)?;
        thread::park_timeout(delay);
        self.preclaim_checkpoint(gate, owner, deadline, cancel)
    }
}
impl HspNativeAuthority for PreparedSourceAuthority {
    fn initial_source_cursor(&self) -> Option<HspSourceCursor> {
        self.source.initial_source_cursor()
    }
    fn poll_owned_work(&self) -> std::result::Result<(), HspDispatchFailure> {
        let entry = &self.clocks.entry;
        if let Some(owner) = entry.continuity.get() {
            owner.poll().map_err(|_| HspDispatchFailure::InvalidClock)?;
        }
        self.source.poll_owned_work()?;
        if !self.source.has_pending_source_transfer() {
            return Ok(());
        }
        let db = match entry.deps.db.try_lock() {
            Ok(db) => db,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::BindingMismatch),
        };
        let sessions = match entry.deps.sessions.try_lock() {
            Ok(sessions) => sessions,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::BindingMismatch),
        };
        entry
            .deps
            .validate_db(&db)
            .map_err(|_| HspDispatchFailure::BindingMismatch)?;
        entry
            .deps
            .validate_session(&sessions)
            .map_err(|_| HspDispatchFailure::BindingMismatch)?;
        if let Some(owner) = entry.continuity.get() {
            owner
                .checkpoint()
                .map_err(|_| HspDispatchFailure::Cancelled)?;
        } else {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.source.advance_source_with_db(&db)
    }
    fn request_owned_stop(&self) {
        if let Some(owner) = self.clocks.entry.continuity.get() {
            owner.request_stop();
        }
        self.source.request_owned_stop();
    }
    fn owned_work_joined(&self) -> bool {
        self.clocks
            .entry
            .continuity
            .get()
            .is_none_or(|owner| owner.is_joined())
            && self.source.owned_work_joined()
    }
    fn join_owned_work(&self) -> std::result::Result<(), HspDispatchFailure> {
        let clock = self
            .clocks
            .entry
            .continuity
            .get()
            .map(|owner| owner.join())
            .transpose();
        let source = self.source.join_owned_work();
        clock.map_err(|_| HspDispatchFailure::WorkerUnavailable)?;
        source
    }
    fn current_snapshot(
        &self,
        now_ns: u64,
    ) -> std::result::Result<HspAuthoritySnapshot, HspDispatchFailure> {
        let snapshot = self.source.current_snapshot(now_ns)?;
        self.validate_snapshot(now_ns, &snapshot)?;
        Ok(snapshot)
    }
    fn observe_current(
        &self,
        clock: &mut dyn FnMut() -> std::result::Result<u64, HspDispatchFailure>,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        self.validate_observation(self.source.observe_current(clock)?)
    }
    fn observe_snapshot(
        &self,
        clock: &mut dyn FnMut() -> std::result::Result<u64, HspDispatchFailure>,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        self.validate_observation(self.source.observe_snapshot(clock)?)
    }
    fn admission_failed(&self) {
        self.source.admission_failed();
    }
    fn attach(&self, handle: &HspSessionHandle) -> std::result::Result<(), HspDispatchFailure> {
        self.source.attach(handle)
    }
    fn current_facts(
        &self,
        now_ns: u64,
    ) -> std::result::Result<Option<HspDispatchFacts>, HspDispatchFailure> {
        let Some(facts) = self.source.current_facts(now_ns)? else {
            return Ok(None);
        };
        self.clocks.validate(now_ns, &facts)?;
        Ok(Some(facts))
    }
    fn authorize(
        &self,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'_>,
        owner: &HspNativeOwner,
    ) -> std::result::Result<HspClaimedHandoff, HspDispatchFailure> {
        let entry = &self.clocks.entry;
        let clocks = self.clocks.clone();
        let mut gate =
            gate.with_facts_validator(Arc::new(move |now, facts| clocks.validate(now, facts)))?;
        #[cfg(test)]
        let test_probe = entry.preclaim_test.lock().unwrap().take();
        #[cfg(test)]
        if let Some(probe) = &test_probe {
            probe.enter()?;
        }
        loop {
            self.preclaim_checkpoint(&mut gate, owner, deadline, cancel)?;
            let db = match entry.deps.db.try_lock() {
                Ok(db) => db,
                Err(TryLockError::WouldBlock) => {
                    #[cfg(test)]
                    if let Some(probe) = &test_probe {
                        probe.contended(HspPreclaimTestSite::Db);
                    }
                    self.wait_preclaim(&mut gate, owner, deadline, cancel)?;
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::Busy),
            };
            self.preclaim_checkpoint(&mut gate, owner, deadline, cancel)?;
            let sessions = match entry.deps.sessions.try_lock() {
                Ok(sessions) => sessions,
                Err(TryLockError::WouldBlock) => {
                    drop(db);
                    #[cfg(test)]
                    if let Some(probe) = &test_probe {
                        probe.contended(HspPreclaimTestSite::Sessions);
                    }
                    self.wait_preclaim(&mut gate, owner, deadline, cancel)?;
                    continue;
                }
                Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::Busy),
            };
            self.preclaim_checkpoint(&mut gate, owner, deadline, cancel)?;
            // Every reacquisition revalidates the actor, exact original request,
            // registration, qualification and captured connection under guards.
            entry
                .deps
                .validate_db(&db)
                .map_err(|_| HspDispatchFailure::BindingMismatch)?;
            entry
                .deps
                .validate_session(&sessions)
                .map_err(|_| HspDispatchFailure::BindingMismatch)?;
            self.preclaim_checkpoint(&mut gate, owner, deadline, cancel)?;
            let progress = self
                .source
                .try_authorize_with_db(&db, deadline, cancel, gate, owner);
            drop(sessions);
            drop(db);
            match progress? {
                HspPreclaimProgress::Claimed(handoff) => {
                    #[cfg(test)]
                    if let Some(probe) = &test_probe {
                        probe.claimed();
                    }
                    return Ok(handoff);
                }
                HspPreclaimProgress::Contended(original) => {
                    gate = original;
                    #[cfg(test)]
                    if let Some(probe) = &test_probe {
                        probe.contended(HspPreclaimTestSite::Source);
                    }
                    self.wait_preclaim(&mut gate, owner, deadline, cancel)?;
                }
            }
        }
    }
}
impl Engine {
    #[cfg(test)]
    pub(super) fn hold_next_handy_hsp_preclaim_for_test(
        &self,
        actor: &SessionId,
        preparation: &RequestId,
    ) -> PResult<HspPreclaimTestHold> {
        let entry = self
            .handy_hsp_preparations
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), preparation.clone()))
            .cloned()
            .ok_or_else(mismatch)?;
        let mut slot = entry.preclaim_test.lock().map_err(internal)?;
        if slot.is_some() {
            return Err(mismatch());
        }
        let probe = Arc::new(HspPreclaimTestProbe::default());
        *slot = Some(probe.clone());
        Ok(HspPreclaimTestHold(probe, Arc::downgrade(&entry)))
    }
    fn hsp_ready_entry(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        request: &RequestId,
    ) -> PResult<Arc<Entry>> {
        let entry = self
            .handy_hsp_preparations
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), request.clone()))
            .cloned()
            .ok_or_else(|| unavailable("no current native HSP readiness"))?;
        if project(&entry.deps.request)? != project_id || !entry.join_finished()? {
            return Err(unavailable(
                "original HSP preparation worker has not settled",
            ));
        }
        entry.budget()?;
        let observation = {
            let state = entry.state.lock().map_err(internal)?;
            if state.pending.is_some() || state.consumed {
                return Err(unavailable(
                    "original HSP readiness is unavailable or consumed",
                ));
            }
            state
                .ready
                .as_ref()
                .ok_or_else(|| unavailable("original HSP readiness is absent"))?
                .observation
                .clone()
        };
        entry.current(Some(&observation))?;
        let history = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .hsp_preparation_history(actor, request)
            .map_err(internal)?
            .ok_or_else(mismatch)?;
        if history.state != HandyClockSyncState::Acknowledged
            || history.linked_stream_id.is_some()
            || history.custody_transferred
            || history.resolved
        {
            return Err(mismatch());
        }
        Ok(entry)
    }
    #[cfg(test)]
    pub(in crate::authority) fn with_handy_hsp_readiness_for_test<T>(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        preparation: &RequestId,
        read: impl FnOnce(&NativePlaybackClock, &HspPreparationRecord) -> T,
    ) -> PResult<T> {
        let entry = self.hsp_ready_entry(actor, project_id, preparation)?;
        let state = entry.state.lock().map_err(internal)?;
        let ready = state.ready.as_ref().ok_or_else(mismatch)?;
        if entry.io.now()? >= ready.valid_until_ns {
            return Err(unavailable("original HSP readiness expired"));
        }
        Ok(read(&ready.clocks, &ready.record))
    }
    pub(in crate::authority) fn handoff_handy_hsp_preparation_factory<T>(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        preparation: &RequestId,
        factory: impl FnOnce(HspPreparedSourceContext) -> PResult<(HspSourceHandoff, T)>,
    ) -> PResult<(HspSessionHandle, T)> {
        let entry = self.hsp_ready_entry(actor, project_id, preparation)?;
        let context = {
            let state = entry.state.lock().map_err(internal)?;
            let ready = state.ready.as_ref().ok_or_else(mismatch)?;
            if state.consumed || state.pending.is_some() || entry.io.now()? >= ready.valid_until_ns
            {
                return Err(mismatch());
            }
            HspPreparedSourceContext {
                record: ready.record.clone(),
                clocks: ready.clocks.clone(),
                policy: ready.qualified.policy,
                profile: ready.qualified.report.manifest.subject.profile.clone(),
                valid_until_ns: ready.valid_until_ns,
            }
        };
        // No readiness/state mutex is held while the factory pins live source
        // membership or persists its original descriptive witness.
        let (handoff, result) = factory(context)?;
        self.handoff_handy_hsp_preparation(actor, project_id, preparation, handoff)
            .map(|handle| (handle, result))
    }

    /// Source-independent synchronization is not a source/presentation grant.
    /// The caller supplies a separately admitted native source authority and an
    /// explicit online-control intent; neither can come from a history DTO.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::authority) fn handoff_handy_hsp_preparation(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        preparation: &RequestId,
        handoff: HspSourceHandoff,
    ) -> PResult<HspSessionHandle> {
        let HspSourceHandoff {
            child_request,
            allow_online_control,
            binding,
            authority: source,
        } = handoff;
        if !allow_online_control {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "HSP stream handoff requires separate online control consent",
            ));
        }
        let _admission = self
            .handy_reconciliation_admission
            .try_read()
            .map_err(|_| unavailable("HSP recovery admission is fenced"))?;
        let entry = self.hsp_ready_entry(actor, project_id, preparation)?;
        let now = entry.io.now()?;
        let mut state = entry.state.lock().map_err(internal)?;
        let ready = state.ready.as_ref().ok_or_else(mismatch)?;
        if state.consumed
            || state.pending.is_some()
            || now >= ready.valid_until_ns
            || binding.owner().as_str() != actor.as_str()
            || binding.session() != &ready.record.device_session
            || binding.clock_domain() != &ready.clocks.server.clock.domain
            || binding.identity().live.project != *project_id
            || binding.identity().live.revision != ready.record.revision
            || child_request == ready.record.request_id
        {
            return Err(mismatch());
        }
        let clock_guard = Arc::new(PreparedClockGuard {
            entry: entry.clone(),
            binding: binding.clone(),
            clocks: ready.clocks.clone(),
            valid_until_ns: ready.valid_until_ns,
        });
        let observed = clock_owner::observe_source_current(
            source.as_ref(),
            &mut || entry.io.now(),
            "native HSP source authority is unavailable",
        )?;
        let now = observed.sampled_at_ns;
        if now >= ready.valid_until_ns {
            return Err(mismatch());
        }
        let initial = observed
            .snapshot
            .facts
            .ok_or_else(|| unavailable("native HSP source facts are absent"))?;
        clock_guard.validate(now, &initial).map_err(|_| {
            unavailable("native HSP source facts differ from original preparation clocks")
        })?;
        let consumed_source = clock_owner::ConsumedSource {
            request: child_request.clone(),
            binding: binding.clone(),
            clocks: ready.clocks.clone(),
            qualified: ready.qualified.clone(),
            valid_until_ns: ready.valid_until_ns,
            source: Arc::downgrade(&source),
            record: stream_record(&ready.record, actor, &child_request, &binding),
        };
        let ready = state.ready.take().ok_or_else(mismatch)?;
        state.consumed = true;
        state.phase = HandyHspPreparationPhase::Retired;
        state.candidate = None;
        drop(state);
        let remote = ready.observation.remote_hsp.as_ref().ok_or_else(mismatch)?;
        let previous = match remote.play_state {
            HandyRemotePlayState::NotInitialized => {
                crate::handy_hsp_wire::HspPlayState::NotInitialized
            }
            HandyRemotePlayState::Stopped => crate::handy_hsp_wire::HspPlayState::Stopped,
            _ => return Err(mismatch()),
        };
        let transport = HandyHspTransport::from_snapshot(
            HspTransportBinding {
                binding: binding.clone(),
                registration: ready.record.registration.clone(),
                device: native_identity(&ready.record.device),
                mode: ready.record.mode.clone(),
                setup_previous: HspSetupPreviousState::new(remote.stream_id, previous)
                    .map_err(|_| mismatch())?,
            },
            entry.deps.credentials.clone(),
            true,
            &self.handy_owner_token,
        )
        .map_err(|_| unavailable("native HSP transport binding failed"))?;
        #[cfg(test)]
        let transport = {
            let mut transport = transport;
            if let Some(origin) = entry.io.hsp_origin() {
                transport.set_test_origin(origin);
            }
            transport
        };
        let record = stream_record(&ready.record, actor, &child_request, &binding);
        let link = HspQualificationLinkRecord {
            stream_id: record.id().map_err(internal)?,
            qualification: ready.record.qualification.clone(),
            subject_sha256: ready.record.subject_sha256.clone(),
            profile_sha256: ready.record.profile_sha256.clone(),
            admission_sha256: format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&(
                        ready.record.id().map_err(internal)?,
                        &record.binding,
                        &record.request_id
                    ))
                    .map_err(internal)?
                )
            ),
        };
        let authority: Arc<dyn HspNativeAuthority> = Arc::new(PreparedSourceAuthority {
            clocks: clock_guard,
            source,
        });
        let admission = HspAdmission::new(
            binding,
            ready.qualified.policy,
            transport,
            record,
            authority,
        )
        .map_err(|_| unavailable("native HSP admission rejected"))?
        .with_qualification_link(link)
        .with_preparation_parent(ready.parent);
        let child = self
            .ensure_handy_hsp_execution()?
            .try_admit(admission)
            .map_err(|_| unavailable("native HSP owner refused the original handoff"))?;
        let mut state = entry.state.lock().map_err(internal)?;
        state.child = Some(child.retirement_handle());
        state.consumed_source = Some(consumed_source);
        if entry.cancel.load(Ordering::Acquire) || entry.deps.shutdown.load(Ordering::Acquire) {
            child.cancel();
        }
        Ok(child)
    }
}

/// Captured native entries remain retired on every failed/cancelled drain.
/// The memberships are sealed only after the child HSP owner has also drained.
pub(super) struct PreparationDrain {
    device: DeviceId,
    route: String,
    namespace: String,
    entries: Vec<Arc<Entry>>,
    memberships: Vec<HspPreparationMembership>,
    sealed: bool,
}
impl PreparationDrain {
    pub(super) fn memberships(&self) -> &[HspPreparationMembership] {
        &self.memberships
    }
}
fn recovery_wait(engine: &Engine, deadline: Instant, cancel: &AtomicBool) -> PResult<()> {
    if cancel.load(Ordering::Acquire) || engine.is_shutting_down() {
        return Err(cancelled());
    }
    if Instant::now() >= deadline {
        return Err(unavailable("native HSP preparation drain expired"));
    }
    Ok(())
}
impl Engine {
    pub(in crate::authority) fn drain_handy_hsp_preparations(
        &self,
        basis: &crate::device_effect_journal::CustodySnapshot,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<PreparationDrain> {
        recovery_wait(self, deadline, cancel)?;
        let entries = {
            let registry = self.handy_hsp_preparations.lock().map_err(internal)?;
            let mut entries = Vec::new();
            for entry in registry.entries.values() {
                let device = &entry.deps.session.registration.device_id;
                if entry.state.lock().map_err(internal)?.resolved {
                    continue;
                }
                if device == &basis.device || entry.deps.route.route_sha256() == basis.route_sha256
                {
                    if entry.deps.route.route_sha256() != basis.route_sha256
                        || entry.deps.route.namespace_sha256() != basis.route_namespace_sha256
                    {
                        return Err(mismatch());
                    }
                    entries.push(entry.clone());
                }
            }
            entries
        };
        for entry in &entries {
            entry.retire();
        }
        for entry in &entries {
            loop {
                recovery_wait(self, deadline, cancel)?;
                if entry.join_finished()? {
                    let pending = entry.state.lock().map_err(internal)?.pending.is_some();
                    if !pending {
                        break;
                    }
                    // Metadata-only retry uses original receipts/outcomes. It
                    // cannot acquire clocks or repeat synchronization.
                    entry.spawn(true)?;
                }
                thread::park_timeout(POLL);
            }
        }
        Ok(PreparationDrain {
            device: basis.device.clone(),
            route: basis.route_sha256.clone(),
            namespace: basis.route_namespace_sha256.clone(),
            entries,
            memberships: Vec::new(),
            sealed: false,
        })
    }
    pub(in crate::authority) fn seal_handy_hsp_preparation_drain(
        &self,
        drain: &mut PreparationDrain,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        recovery_wait(self, deadline, cancel)?;
        let journal = loop {
            recovery_wait(self, deadline, cancel)?;
            match self._device_effect_journal.try_lock() {
                Ok(journal) => break journal,
                Err(TryLockError::WouldBlock) => thread::park_timeout(POLL),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(unavailable("HSP preparation journal unavailable"))
                }
            }
        };
        let mut memberships = Vec::new();
        for entry in &drain.entries {
            if !entry.cancel.load(Ordering::Acquire)
                || !entry.join_finished()?
                || entry.state.lock().map_err(internal)?.pending.is_some()
            {
                return Err(unavailable("original HSP preparation is not drained"));
            }
            if let Some(history) = journal
                .hsp_preparation_history(&entry.deps.actor, &entry.deps.request.request_id)
                .map_err(internal)?
            {
                if !history.resolved && history.state != HandyClockSyncState::Cancelled {
                    memberships.push(
                        journal
                            .hsp_preparation_membership(&history.id)
                            .map_err(internal)?,
                    );
                }
            }
        }
        memberships.sort_by(|a, b| a.id.cmp(&b.id));
        recovery_wait(self, deadline, cancel)?;
        drain.memberships = memberships;
        drain.sealed = true;
        Ok(())
    }
    pub(in crate::authority) fn finish_handy_hsp_preparation_recovery(
        &self,
        drain: PreparationDrain,
        receipt: &HspResolvedReceipt,
    ) -> PResult<()> {
        if !drain.sealed
            || !receipt.matches_target(&drain.device, &drain.route, &drain.namespace)
            || !receipt.contains_exact_preparations(&drain.memberships)
        {
            return Err(mismatch());
        }
        let registry = self.handy_hsp_preparations.lock().map_err(internal)?;
        for entry in &drain.entries {
            let key = (
                entry.deps.actor.clone(),
                entry.deps.request.request_id.clone(),
            );
            if !registry
                .entries
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, entry))
                || !entry.join_finished()?
                || !entry.cancel.load(Ordering::Acquire)
            {
                return Err(mismatch());
            }
            let mut state = entry.state.lock().map_err(internal)?;
            if state.pending.is_some() {
                return Err(unavailable("HSP preparation metadata remains unresolved"));
            }
            state.ready = None;
            state.candidate = None;
            state.resolved = true;
            state.phase = HandyHspPreparationPhase::Retired;
        }
        Ok(())
    }
}
