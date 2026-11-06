//! Engine-owned HSP lifetime. No worker retains Engine and no second sample queue exists.
//! Admission stays internal until real HSP qualification and a native inference producer exist.
use crate::{
    device_effect_journal::{
        ClaimedHspReceipt, DeviceEffectJournal, HspResolvedReceipt, HspStreamMembership,
        HspStreamReceipt, PreparedHspPreparationReceipt, PreparedHspReceipt,
    },
    handy_hsp_dispatch::{
        HandyHspDispatcher, HandyHspTransport, HspClaimGate, HspClaimedHandoff, HspDispatchAttempt,
        HspDispatchCompletion, HspDispatchFailure, HspDispatchRequest, HspEmergencyStopCompletion,
        HspEmergencyStopObservation, HspJoinedCompletion, HspNativeOwner, HspPreclaimProgress,
        HspStopSendTracker,
    },
    handy_hsp_records::{HspBindingRecord, HspQualificationLinkRecord, HspStreamRecord},
    handy_hsp_runtime::{
        HandyHspRuntime, HspBinding, HspCompletionRecord, HspDispatchFacts, HspEffect,
        HspEffectKind, HspFiniteDrainConfig, HspObservedOutcome, HspRuntimeState, HspRuntimeTick,
    },
};
use pulsar_core::{
    ClientId, DeviceId, HspFiniteDrainPolicy, HspFiniteDrainWindow, HspInputSeal, HspInputState,
    HspOfferOutcome, HspPolicy, HspSample, HspSourceCursor, HspSourceTransition,
    HspSourceTransitionSpec, LiveIdentity, ProjectId, RequestId, SessionId,
};
use pulsar_protocol::{
    HandyHspEffectKind, HandyHspEffectStatus, HandyHspFiniteDrainStatus, HandyHspInputState,
    HandyHspJoinedObservation, HandyHspJoinedOutcome, HandyHspNativeStatus, HandyHspRuntimeState,
    HandyHspStatus,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex, OnceLock, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_SESSIONS: usize = 16;
const MAILBOX_CAPACITY: usize = 4;
const TICK: Duration = Duration::from_millis(20);
const DISPATCH_BUDGET: Duration = Duration::from_secs(3);
pub(crate) const DRAIN_BUDGET: Duration = Duration::from_secs(12);
// Transient producer contention does not revoke a valid stream. The interval is
// measured from the first missed watchdog lock in suspend-aware boottime and is
// reset only by an actual successful runtime tick, never by fresh supplied facts.
const WATCHDOG_CONTENTION_NS: u64 = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HspExecutionError {
    Busy,
    Unavailable,
    Capacity,
    InvalidAdmission,
    NotFound,
    Unresolved,
    Dispatch(HspDispatchFailure),
}
impl From<HspDispatchFailure> for HspExecutionError {
    fn from(error: HspDispatchFailure) -> Self {
        Self::Dispatch(error)
    }
}
/// Current and future phase derived from the same retained observations. The
/// future value is a prediction within original expiry, never a fresh sample.
#[derive(Clone)]
pub(crate) struct HspAuthoritySnapshot {
    pub(crate) facts: Option<HspDispatchFacts>,
    pub(crate) projected: Option<HspDispatchFacts>,
}
/// The native time sampled after selecting all observations used by `snapshot`.
/// Consumers pass this same time to the corresponding runtime operation.
pub(crate) struct HspAuthorityEvaluation {
    pub(crate) sampled_at_ns: u64,
    pub(crate) snapshot: HspAuthoritySnapshot,
}
impl HspAuthoritySnapshot {
    fn drain_window(&self) -> Option<HspFiniteDrainWindow> {
        let current = self.facts.as_ref()?;
        let projected = self.projected.as_ref()?;
        (current.binding() == projected.binding()
            && current.window() == projected.window()
            && projected.reference().at_ns() > current.reference().at_ns()
            && projected.valid_at(projected.reference().at_ns()).is_ok())
        .then_some(HspFiniteDrainWindow {
            at_ns: current.reference().at_ns(),
            phase: current.reference().phase(),
            stop_at_ns: projected.reference().at_ns(),
            stop_phase: projected.reference().phase(),
        })
    }
}

/// Implementations use bounded try-locks. `authorize` must retain native source,
/// clock, qualification and actor guards while consuming `gate`; sampled facts
/// and journal records alone never authorize a command. `current_facts` may run
/// while the runtime guard is held: it must not reacquire that runtime or block
/// on dependency locks. The inverse final-claim path also uses try-locks.
pub(crate) trait HspNativeAuthority: Send + Sync {
    fn initial_source_cursor(&self) -> Option<HspSourceCursor> {
        None
    }
    /// Called by the owner outside runtime and dependency guards. Implementations
    /// may launch bounded reads or advance an already-retained source transfer;
    /// they must not block on I/O or turn observation into renewed authority.
    fn poll_owned_work(&self) -> Result<(), HspDispatchFailure> {
        Ok(())
    }
    fn has_pending_source_transfer(&self) -> bool {
        false
    }
    /// Preparation supplies freshly checked DB/session ownership. A source must
    /// not reacquire either guard while advancing its exact retained transfer.
    fn advance_source_with_db(&self, _db: &rusqlite::Connection) -> Result<(), HspDispatchFailure> {
        Ok(())
    }
    fn request_owned_stop(&self) {}
    fn owned_work_joined(&self) -> bool {
        true
    }
    /// Final cleanup only, with no runtime or dependency guard retained.
    fn join_owned_work(&self) -> Result<(), HspDispatchFailure> {
        Ok(())
    }
    /// Select native observations before invoking `clock` exactly once, including
    /// when selection fails. Evaluate only the selected observations at that time.
    /// Implementations with independently published observations must override
    /// this default; no failed evaluation renews or replaces retained authority.
    fn observe_current(
        &self,
        clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
    ) -> Result<HspAuthorityEvaluation, HspDispatchFailure> {
        let sampled_at_ns = clock()?;
        Ok(HspAuthorityEvaluation {
            sampled_at_ns,
            snapshot: HspAuthoritySnapshot {
                facts: self.current_facts(sampled_at_ns)?,
                projected: None,
            },
        })
    }
    /// Fresh finite-tail evaluation. Current and projected facts must derive
    /// from the same selected observations; ordinary callers use observe_current.
    /// As above, the callback is invoked exactly once even on selection failure.
    fn observe_snapshot(
        &self,
        clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
    ) -> Result<HspAuthorityEvaluation, HspDispatchFailure> {
        let sampled_at_ns = clock()?;
        Ok(HspAuthorityEvaluation {
            sampled_at_ns,
            snapshot: self.current_snapshot(sampled_at_ns)?,
        })
    }
    fn current_snapshot(&self, now_ns: u64) -> Result<HspAuthoritySnapshot, HspDispatchFailure> {
        Ok(HspAuthoritySnapshot {
            facts: self.current_facts(now_ns)?,
            projected: None,
        })
    }
    fn admission_failed(&self) {}
    /// Invoked before the owner's Work enters its mailbox. The source may install
    /// its exact delivery handle here; failure cancels this never-enqueued owner.
    fn attach(&self, _handle: &HspSessionHandle) -> Result<(), HspDispatchFailure> {
        Ok(())
    }
    /// Preparation retains this project DB guard through the final claim. A
    /// source must use the borrowed connection, never recursively lock Engine DB.
    fn authorize_with_db(
        &self,
        _db: &rusqlite::Connection,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'_>,
        owner: &HspNativeOwner,
    ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
        self.authorize(deadline, cancel, gate, owner)
    }
    /// Only a native preclaim acquisition can return the original gate. Legacy
    /// failures, including Busy, remain terminal and cannot acquire retry custody.
    fn try_authorize_with_db<'a>(
        &self,
        db: &rusqlite::Connection,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'a>,
        owner: &HspNativeOwner,
    ) -> Result<HspPreclaimProgress<'a>, HspDispatchFailure> {
        self.authorize_with_db(db, deadline, cancel, gate, owner)
            .map(HspPreclaimProgress::Claimed)
    }
    fn current_facts(&self, now_ns: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure>;
    fn authorize(
        &self,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'_>,
        owner: &HspNativeOwner,
    ) -> Result<HspClaimedHandoff, HspDispatchFailure>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HspJoinedOutcome {
    Acknowledged,
    Unknown,
    Rejected,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HspJoinedObservation {
    pub sequence: u64,
    pub kind: HspEffectKind,
    pub outcome: HspJoinedOutcome,
}
#[derive(Clone, Debug)]
pub(crate) struct HspSessionSnapshot {
    pub stream_record_id: String,
    pub project: ProjectId,
    pub actor: ClientId,
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub generation: u64,
    pub stream_id: u32,
    pub live: LiveIdentity,
    pub runtime_state: HspRuntimeState,
    pub current_effect: Option<(u64, HspEffectKind)>,
    pub accepted_samples: u64,
    pub pending_points: usize,
    pub retained_points: usize,
    pub buffered_points: usize,
    pub total_added: u64,
    pub last_joined: Option<HspJoinedObservation>,
    pub metadata_pending: bool,
    pub stop_pending: bool,
    pub stop_blocked: bool,
    pub emergency_observed: bool,
    pub post_setup_bound: bool,
    pub native_active: bool,
    pub retired: bool,
    pub qualified: bool,
    pub emergency_history_durable: bool,
    pub finite_drain: Option<HandyHspFiniteDrainStatus>,
}
impl HspSessionSnapshot {
    pub(crate) fn into_status(self) -> Result<HandyHspStatus, HspExecutionError> {
        let state = match self.runtime_state {
            HspRuntimeState::AwaitingSetup => HandyHspRuntimeState::AwaitingSetup,
            HspRuntimeState::Prefilling => HandyHspRuntimeState::Prefilling,
            HspRuntimeState::Playing => HandyHspRuntimeState::Playing,
            HspRuntimeState::StopRequired => HandyHspRuntimeState::StopRequired,
            HspRuntimeState::StopInFlight => HandyHspRuntimeState::StopInFlight,
            HspRuntimeState::StopAcknowledged => HandyHspRuntimeState::StopAcknowledged,
            HspRuntimeState::StopUnknown => HandyHspRuntimeState::StopUnknown,
            HspRuntimeState::SetupCancelled => HandyHspRuntimeState::SetupCancelled,
            HspRuntimeState::SetupUnsent => HandyHspRuntimeState::SetupUnsent,
            HspRuntimeState::StopUnclaimed => HandyHspRuntimeState::StopUnclaimed,
        };
        let effect = |sequence, kind| HandyHspEffectStatus {
            sequence,
            kind: match kind {
                HspEffectKind::Setup => HandyHspEffectKind::Setup,
                HspEffectKind::Append => HandyHspEffectKind::Append,
                HspEffectKind::Play => HandyHspEffectKind::Play,
                HspEffectKind::Stop => HandyHspEffectKind::Stop,
            },
        };
        let count = |n| u32::try_from(n).map_err(|_| HspExecutionError::InvalidAdmission);
        let status = HandyHspStatus {
            project_id: self.project,
            stream_request_id: self.request_id,
            stream_record_id: pulsar_core::ArtifactId::new(self.stream_record_id)
                .map_err(|_| HspExecutionError::InvalidAdmission)?,
            generation: self.generation,
            stream_id: self.stream_id,
            live: self.live,
            native: Some(HandyHspNativeStatus {
                runtime_state: state,
                native_active: self.native_active,
                current_effect: self
                    .current_effect
                    .map(|(sequence, kind)| effect(sequence, kind)),
                accepted_samples: self.accepted_samples,
                pending_points: count(self.pending_points)?,
                retained_points: count(self.retained_points)?,
                buffered_points: count(self.buffered_points)?,
                total_added: self.total_added,
                last_joined: self.last_joined.map(|joined| HandyHspJoinedObservation {
                    effect: effect(joined.sequence, joined.kind),
                    outcome: match joined.outcome {
                        HspJoinedOutcome::Acknowledged => HandyHspJoinedOutcome::Acknowledged,
                        HspJoinedOutcome::Unknown => HandyHspJoinedOutcome::Unknown,
                        HspJoinedOutcome::Rejected => HandyHspJoinedOutcome::Rejected,
                    },
                }),
                metadata_pending: self.metadata_pending,
                stop_pending: self.stop_pending,
                stop_blocked: self.stop_blocked,
                emergency_observed: self.emergency_observed,
                post_setup_bound: self.post_setup_bound,
                retired: self.retired,
                finite_drain: self.finite_drain,
            }),
            qualified: self.qualified,
            emergency_history_durable: self.emergency_history_durable,
        };
        status
            .validate()
            .map_err(|_| HspExecutionError::InvalidAdmission)?;
        Ok(status)
    }
}
/// First branch observed by the native owner, not an attribution of human intent.
/// Later cancellation, shutdown and metadata failures cannot replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HspFirstStopReason {
    OtherNativeLatch,
    WatchdogCurrentObservation,
    WatchdogMissingFacts,
    WatchdogInvalidFacts,
    WatchdogReferenceMismatch,
    WatchdogBindingMismatch,
    WatchdogSnapshotObservation,
    WatchdogRuntimeTick,
    WatchdogTailBudget,
    FiniteTailReached,
    WatchdogRuntimePoisoned,
    WatchdogBlockedClock,
    WatchdogBlockedFacts,
    WatchdogContentionLimit,
    MetadataDeadline,
    ShutdownObserved,
    CancellationObserved,
    OwnedWorkPoll,
    PostSetupProbeStart,
    NextEffectObservation,
    NextEffectRuntime,
    JoinedDispatchRejected,
    JoinedObservationFailed,
    JoinedOutcomeUnknown,
    PostSetupProbeInstall,
    OwnerFinish,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct HspStopFactsObservation {
    pub reference_at_ns: u64,
    pub lease_until_ns: u64,
    pub source_until_ns: u64,
    pub clock_until_ns: u64,
    pub phase_earliest_ms: i64,
    pub phase_latest_ms: i64,
}
impl HspStopFactsObservation {
    fn capture(facts: &HspDispatchFacts) -> Self {
        Self {
            reference_at_ns: facts.reference().at_ns(),
            lease_until_ns: facts.window().lease_until_ns(),
            source_until_ns: facts.window().source_until_ns(),
            clock_until_ns: facts.window().clock_until_ns(),
            phase_earliest_ms: facts.reference().phase().earliest_ms,
            phase_latest_ms: facts.reference().phase().latest_ms,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct HspStopEffectObservation {
    pub sequence: u64,
    pub kind: HspEffectKind,
    pub original_facts: Option<HspStopFactsObservation>,
}
impl HspStopEffectObservation {
    fn capture(effect: &HspEffect) -> Self {
        Self {
            sequence: effect.sequence(),
            kind: effect.kind(),
            original_facts: effect
                .original_facts()
                .map(HspStopFactsObservation::capture),
        }
    }
}
/// Bounded scalar/error metadata only. No clock pins, source capabilities,
/// transport requests, credentials, peer strings or growing event log survive.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct HspFirstStopObservation {
    pub reason: HspFirstStopReason,
    pub failure: Option<HspDispatchFailure>,
    /// Existing sample associated with this branch; None means unavailable.
    /// This is never a newly sampled or asserted stop-effect timestamp.
    pub sampled_at_ns: Option<u64>,
    pub facts: Option<HspStopFactsObservation>,
    pub runtime_state: Option<HspRuntimeState>,
    pub effect: Option<HspStopEffectObservation>,
    pub active_native_worker: bool,
    pub active_ordinary: bool,
    pub post_setup_bound: bool,
    pub last_joined: Option<HspJoinedObservation>,
    pub source_file: &'static str,
    pub source_line: u32,
}
impl HspFirstStopObservation {
    #[cfg(test)]
    pub(crate) fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"reason":self.reason,"failure":self.failure.as_ref().map(|e|format!("{e:?}")),
            "sampled_at_ns":self.sampled_at_ns,"facts":self.facts,
            "runtime_state":self.runtime_state.map(|s|format!("{s:?}")),
            "effect":self.effect.as_ref().map(|e|serde_json::json!({"sequence":e.sequence,"kind":format!("{:?}",e.kind),"original_facts":e.original_facts})),
            "active_native_worker":self.active_native_worker,"active_ordinary":self.active_ordinary,"post_setup_bound":self.post_setup_bound,
            "last_joined":self.last_joined.as_ref().map(|j|serde_json::json!({"sequence":j.sequence,"kind":format!("{:?}",j.kind),"outcome":format!("{:?}",j.outcome)})),
            "source_file":self.source_file,"source_line":self.source_line,
            "scope":"first native Work branch; cancellation is not proof of a public UI action"})
    }
}

struct Session {
    first_stop: OnceLock<HspFirstStopObservation>,
    record: HspStreamRecord,
    runtime: Arc<Mutex<HandyHspRuntime>>,
    authority: Arc<dyn HspNativeAuthority>,
    owner: HspNativeOwner,
    cancelled: AtomicBool,
    accepted: AtomicU64,
    stop_send: Arc<HspStopSendTracker>,
    #[cfg(test)]
    watchdog_busy_ticks: AtomicU64,
    #[cfg(test)]
    watchdog_successful_ticks: AtomicU64,
    #[cfg(test)]
    watchdog_contention_retired: AtomicBool,
    snapshot: Mutex<HspSessionSnapshot>,
    recovery_token: AtomicU64,
    released: AtomicBool,
    #[cfg(test)]
    metadata_pause: Mutex<Option<Arc<MetadataPause>>>,
    #[cfg(test)]
    metadata_busy: Mutex<Option<HspMetadataBusyObservation>>,
}
#[derive(Default)]
struct Registry {
    sessions: BTreeMap<String, Arc<Session>>,
    fences: Vec<RecoveryFence>,
    next_recovery: u64,
}

/// No public command can construct this. The future native admission path must
/// provide accepted qualification, source, clock and actor capabilities.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct HspAdmission {
    record: HspStreamRecord,
    session: Arc<Session>,
    transport: HandyHspTransport,
    setup: HspEffect,
    qualification: Option<HspQualificationLinkRecord>,
    preparation: Option<PreparedHspPreparationReceipt>,
}
#[cfg_attr(not(test), allow(dead_code))]
impl HspAdmission {
    pub(crate) fn with_preparation_parent(mut self, parent: PreparedHspPreparationReceipt) -> Self {
        self.preparation = Some(parent);
        self
    }
    pub(crate) fn with_qualification_link(mut self, link: HspQualificationLinkRecord) -> Self {
        self.qualification = Some(link);
        self
    }
    pub(crate) fn new(
        binding: HspBinding,
        policy: HspPolicy,
        transport: HandyHspTransport,
        record: HspStreamRecord,
        authority: Arc<dyn HspNativeAuthority>,
    ) -> Result<Self, HspExecutionError> {
        let stop_send = Arc::new(HspStopSendTracker::default());
        let transport = transport.with_stop_send_tracker(stop_send.clone());
        let id = record
            .id()
            .map_err(|_| HspExecutionError::InvalidAdmission)?;
        if record.binding != HspBindingRecord::capture(&binding) {
            return Err(HspExecutionError::InvalidAdmission);
        }
        let observation = authority.observe_current(&mut native_now)?;
        let now = observation.sampled_at_ns;
        let facts = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::InvalidAdmission)?;
        let mut runtime = match authority.initial_source_cursor() {
            Some(source) => HandyHspRuntime::new_with_source(binding.clone(), policy, source),
            None => HandyHspRuntime::new(binding.clone(), policy),
        }
        .map_err(HspDispatchFailure::Authority)?;
        let setup = runtime
            .next_effect(now, Some(&facts))
            .map_err(HspDispatchFailure::Authority)?
            .ok_or(HspExecutionError::InvalidAdmission)?;
        let snapshot = HspSessionSnapshot {
            stream_record_id: id,
            project: binding.identity().live.project.clone(),
            actor: binding.owner().clone(),
            actor_session: record.actor_session.clone(),
            request_id: record.request_id.clone(),
            generation: binding.identity().generation,
            stream_id: binding.identity().stream_id,
            live: binding.identity().live.clone(),
            runtime_state: runtime.state(),
            current_effect: Some((setup.sequence(), setup.kind())),
            accepted_samples: 0,
            pending_points: 0,
            retained_points: 0,
            buffered_points: 0,
            total_added: 0,
            last_joined: None,
            metadata_pending: false,
            stop_pending: false,
            stop_blocked: false,
            emergency_observed: false,
            post_setup_bound: false,
            native_active: false,
            retired: false,
            qualified: false,
            emergency_history_durable: false,
            finite_drain: None,
        };
        Ok(Self {
            record: record.clone(),
            qualification: None,
            preparation: None,
            session: Arc::new(Session {
                first_stop: OnceLock::new(),
                record,
                runtime: Arc::new(Mutex::new(runtime)),
                authority,
                owner: transport.owner(),
                cancelled: AtomicBool::new(false),
                accepted: AtomicU64::new(0),
                stop_send,
                #[cfg(test)]
                watchdog_busy_ticks: AtomicU64::new(0),
                #[cfg(test)]
                watchdog_successful_ticks: AtomicU64::new(0),
                #[cfg(test)]
                watchdog_contention_retired: AtomicBool::new(false),
                snapshot: Mutex::new(snapshot),
                recovery_token: AtomicU64::new(0),
                released: AtomicBool::new(false),
                #[cfg(test)]
                metadata_pause: Mutex::new(None),
                #[cfg(test)]
                metadata_busy: Mutex::new(None),
            }),
            transport,
            setup,
        })
    }
}
#[derive(Clone)]
struct RecoverySelector {
    device: DeviceId,
    route: String,
    namespace: String,
}
impl RecoverySelector {
    fn matches(&self, record: &HspStreamRecord) -> bool {
        record.route_namespace_sha256 == self.namespace
            && (record.registration.device_id == self.device || record.route_sha256 == self.route)
    }
    fn conflicts(&self, record: &HspStreamRecord) -> bool {
        (record.registration.device_id == self.device
            && (record.route_sha256 != self.route
                || record.route_namespace_sha256 != self.namespace))
            || (record.route_sha256 == self.route
                && record.route_namespace_sha256 != self.namespace)
    }
    fn overlaps(&self, other: &Self) -> bool {
        self.device == other.device || self.route == other.route
    }
}
struct RecoveryFence {
    selector: RecoverySelector,
    token: u64,
    ids: Vec<String>,
}
/// Exact captured native objects, not a reconstructed list of historical IDs.
pub(crate) struct HspRecoveryTargets {
    registry: Arc<Mutex<Registry>>,
    selector: RecoverySelector,
    token: u64,
    sessions: Vec<Arc<Session>>,
    ids: Vec<String>,
}
impl HspRecoveryTargets {
    pub(crate) fn stream_ids(&self) -> &[String] {
        &self.ids
    }
}
/// Native jobs and metadata were joined before these exact memberships were
/// read. No Deserialize/Clone operation can mint another release capability.
pub(crate) struct HspDrainedProof {
    targets: HspRecoveryTargets,
    memberships: Vec<HspStreamMembership>,
}
impl HspDrainedProof {
    pub(crate) fn memberships(&self) -> &[HspStreamMembership] {
        &self.memberships
    }
    #[cfg(test)]
    pub(crate) fn stream_ids(&self) -> &[String] {
        &self.targets.ids
    }
}
enum RecoveryAction {
    Drain(HspRecoveryTargets),
    Finish(HspDrainedProof),
}
enum RecoveryReply {
    Drained(Box<HspDrainedProof>),
    Finished,
}
struct RecoveryRequest {
    action: RecoveryAction,
    deadline: Instant,
    abandoned: Arc<AtomicBool>,
    reply: SyncSender<Result<RecoveryReply, HspExecutionError>>,
}
fn recovery_valid(registry: &Registry, targets: &HspRecoveryTargets) -> bool {
    let Some(fence) = registry
        .fences
        .iter()
        .find(|fence| fence.token == targets.token)
    else {
        return false;
    };
    if fence.ids != targets.ids {
        return false;
    }
    let current: Vec<_> = registry
        .sessions
        .iter()
        .filter(|(_, session)| {
            !session.released.load(Ordering::Acquire) && targets.selector.matches(&session.record)
        })
        .collect();
    current.len() == targets.ids.len()
        && current.iter().zip(&targets.sessions).zip(&targets.ids).all(
            |(((id, current), selected), expected)| {
                *id == expected
                    && Arc::ptr_eq(current, selected)
                    && selected.recovery_token.load(Ordering::Acquire) == targets.token
            },
        )
}

#[derive(Clone)]
pub(crate) struct HspExecutionHandle {
    registry: Arc<Mutex<Registry>>,
    incoming: SyncSender<Work>,
    recovery: SyncSender<RecoveryRequest>,
    stopped: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    wake: thread::Thread,
}
#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct HspSessionHandle {
    session: Arc<Session>,
    stopped: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    wake: thread::Thread,
}
impl HspExecutionHandle {
    /// Capture also fences new matching admission. Existing selected streams
    /// keep running until drain; abandoning a capture conservatively retains its
    /// fence until a fresh capture/drain/committed finish for the same scope.
    pub(crate) fn capture_recovery_targets(
        &self,
        device: &DeviceId,
        route: &str,
        namespace: &str,
    ) -> Result<HspRecoveryTargets, HspExecutionError> {
        if self.stopped.load(Ordering::Acquire) || self.shutdown_requested.load(Ordering::Acquire) {
            return Err(HspExecutionError::Unavailable);
        }
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !digest(route) || !digest(namespace) {
            return Err(HspExecutionError::InvalidAdmission);
        }
        let selector = RecoverySelector {
            device: device.clone(),
            route: route.into(),
            namespace: namespace.into(),
        };
        let mut registry = self
            .registry
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        if registry.sessions.values().any(|session| {
            !session.released.load(Ordering::Acquire) && selector.conflicts(&session.record)
        }) || registry.fences.iter().any(|fence| {
            selector.overlaps(&fence.selector)
                && (selector.route != fence.selector.route
                    || selector.namespace != fence.selector.namespace)
        }) {
            return Err(HspExecutionError::InvalidAdmission);
        }
        if registry.fences.len() >= MAX_SESSIONS
            && !registry
                .fences
                .iter()
                .any(|fence| selector.overlaps(&fence.selector))
        {
            return Err(HspExecutionError::Capacity);
        }
        let token = registry
            .next_recovery
            .checked_add(1)
            .ok_or(HspExecutionError::Capacity)?;
        let mut ids = Vec::new();
        let mut sessions = Vec::new();
        for (id, session) in &registry.sessions {
            if !session.released.load(Ordering::Acquire) && selector.matches(&session.record) {
                ids.push(id.clone());
                sessions.push(session.clone());
            }
        }
        registry.next_recovery = token;
        registry
            .fences
            .retain(|fence| !selector.overlaps(&fence.selector));
        registry.fences.push(RecoveryFence {
            selector: selector.clone(),
            token,
            ids: ids.clone(),
        });
        for session in &sessions {
            session.recovery_token.store(token, Ordering::Release);
        }
        Ok(HspRecoveryTargets {
            registry: self.registry.clone(),
            selector,
            token,
            sessions,
            ids,
        })
    }
    pub(crate) fn drain_recovery(
        &self,
        targets: HspRecoveryTargets,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<HspDrainedProof, HspExecutionError> {
        if !Arc::ptr_eq(&self.registry, &targets.registry) {
            return Err(HspExecutionError::InvalidAdmission);
        }
        {
            let registry = self
                .registry
                .try_lock()
                .map_err(|_| HspExecutionError::Busy)?;
            if !recovery_valid(&registry, &targets) {
                return Err(HspExecutionError::InvalidAdmission);
            }
            // Retirement wins even if the following wait is already cancelled,
            // expired, or the bounded mailbox is full. No failed drain reopens.
            for session in &targets.sessions {
                session.cancelled.store(true, Ordering::Release);
                session.owner.retire_ordinary();
            }
        }
        self.wake.unpark();
        match self.recovery_request(RecoveryAction::Drain(targets), deadline, cancel)? {
            RecoveryReply::Drained(proof) => Ok(*proof),
            RecoveryReply::Finished => Err(HspExecutionError::InvalidAdmission),
        }
    }
    pub(crate) fn finish_recovery(
        &self,
        proof: HspDrainedProof,
        receipt: &HspResolvedReceipt,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(), HspExecutionError> {
        if proof.memberships.len() != proof.targets.ids.len()
            || proof
                .memberships
                .iter()
                .zip(&proof.targets.ids)
                .any(|(member, id)| member.stream_id() != id)
            || !Arc::ptr_eq(&self.registry, &proof.targets.registry)
            || !receipt.matches_target(
                &proof.targets.selector.device,
                &proof.targets.selector.route,
                &proof.targets.selector.namespace,
            )
            || !receipt.contains_exact_memberships(&proof.memberships)
        {
            return Err(HspExecutionError::InvalidAdmission);
        }
        // Only this path can enqueue Finish; a journal/history DTO cannot
        // construct the native proof or satisfy the typed committed receipt.
        match self.recovery_request(RecoveryAction::Finish(proof), deadline, cancel)? {
            RecoveryReply::Finished => Ok(()),
            RecoveryReply::Drained(_) => Err(HspExecutionError::InvalidAdmission),
        }
    }
    fn recovery_request(
        &self,
        action: RecoveryAction,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<RecoveryReply, HspExecutionError> {
        let abandoned = Arc::new(AtomicBool::new(false));
        let (reply, receiver) = mpsc::sync_channel(1);
        self.recovery
            .try_send(RecoveryRequest {
                action,
                deadline,
                abandoned: abandoned.clone(),
                reply,
            })
            .map_err(|_| HspExecutionError::Busy)?;
        self.wake.unpark();
        loop {
            if cancel.load(Ordering::Acquire)
                || Instant::now() >= deadline
                || self.stopped.load(Ordering::Acquire)
                || self.shutdown_requested.load(Ordering::Acquire)
            {
                abandoned.store(true, Ordering::Release);
                self.wake.unpark();
                return Err(HspExecutionError::Unavailable);
            }
            let wait = deadline.saturating_duration_since(Instant::now()).min(TICK);
            match receiver.recv_timeout(wait) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(HspExecutionError::Unavailable)
                }
            }
        }
    }
    /// Exact original authenticated session, project and request; this read does
    /// not touch a clock, database, watchdog or transport.
    pub(crate) fn snapshot_for_session(
        &self,
        actor: &SessionId,
        project: &ProjectId,
        request: &RequestId,
    ) -> Result<HspSessionSnapshot, HspExecutionError> {
        let registry = self
            .registry
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        for session in registry.sessions.values() {
            let snapshot = session
                .snapshot
                .try_lock()
                .map_err(|_| HspExecutionError::Busy)?;
            if &snapshot.actor_session == actor
                && &snapshot.project == project
                && &snapshot.request_id == request
            {
                return Ok(snapshot.clone());
            }
        }
        Err(HspExecutionError::NotFound)
    }
    /// Read only the exact native actor/project/request's first observation.
    /// No clocks, runtime locks, database or transport are touched.
    #[cfg(test)]
    pub(crate) fn first_stop_for_session(
        &self,
        actor: &SessionId,
        project: &ProjectId,
        request: &RequestId,
    ) -> Result<Option<HspFirstStopObservation>, HspExecutionError> {
        let registry = self
            .registry
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let session = registry
            .sessions
            .values()
            .find(|session| {
                &session.record.actor_session == actor
                    && &session.record.binding.identity.live.project == project
                    && &session.record.request_id == request
            })
            .ok_or(HspExecutionError::NotFound)?;
        Ok(session.first_stop.get().cloned())
    }
    /// Additional internal identity check for the future producer admission.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn snapshot(
        &self,
        actor: &ClientId,
        project: &ProjectId,
        request: &RequestId,
    ) -> Result<HspSessionSnapshot, HspExecutionError> {
        let registry = self
            .registry
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        for session in registry.sessions.values() {
            let snapshot = session
                .snapshot
                .try_lock()
                .map_err(|_| HspExecutionError::Busy)?;
            if &snapshot.actor == actor
                && &snapshot.project == project
                && &snapshot.request_id == request
            {
                return Ok(snapshot.clone());
            }
        }
        Err(HspExecutionError::NotFound)
    }
    /// No native work is newly admitted after shutdown starts.
    pub(crate) fn retire_all(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(registry) = self.registry.try_lock() {
            for session in registry.sessions.values() {
                session.cancelled.store(true, Ordering::Release);
                session.owner.retire_ordinary();
            }
        }
        self.wake.unpark();
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn try_admit(
        &self,
        admission: HspAdmission,
    ) -> Result<HspSessionHandle, HspExecutionError> {
        if self.stopped.load(Ordering::Acquire) || self.shutdown_requested.load(Ordering::Acquire) {
            return Err(HspExecutionError::Unavailable);
        }
        let mut registry = self
            .registry
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        if registry.sessions.len() >= MAX_SESSIONS {
            return Err(HspExecutionError::Capacity);
        }
        for entry in registry.sessions.values() {
            let previous = entry
                .snapshot
                .try_lock()
                .map_err(|_| HspExecutionError::Busy)?;
            if (previous.actor_session == admission.record.actor_session
                && previous.request_id == admission.record.request_id)
                || (previous.live == admission.record.binding.identity.live
                    && previous.generation == admission.record.binding.identity.generation)
            {
                return Err(HspExecutionError::InvalidAdmission);
            }
        }
        if registry.fences.iter().any(|fence| {
            fence.selector.matches(&admission.record) || fence.selector.conflicts(&admission.record)
        }) {
            return Err(HspExecutionError::Unavailable);
        }
        let id = admission
            .record
            .id()
            .map_err(|_| HspExecutionError::InvalidAdmission)?;
        let session = admission.session.clone();
        if self.stopped.load(Ordering::Acquire) || self.shutdown_requested.load(Ordering::Acquire) {
            return Err(HspExecutionError::Unavailable);
        }
        let handle = HspSessionHandle {
            session: session.clone(),
            stopped: self.stopped.clone(),
            shutdown_requested: self.shutdown_requested.clone(),
            wake: self.wake.clone(),
        };
        if let Err(error) = session.authority.attach(&handle) {
            handle.cancel();
            session.authority.admission_failed();
            return Err(error.into());
        }
        if self
            .incoming
            .try_send(Work::new(
                admission,
                self.stopped.clone(),
                self.shutdown_requested.clone(),
            ))
            .is_err()
        {
            handle.cancel();
            session.authority.admission_failed();
            return Err(HspExecutionError::Busy);
        }
        registry.sessions.insert(id, session);
        self.wake.unpark();
        Ok(handle)
    }
}
/// One non-deserializable native command-admission reference, consumed once.
pub(crate) struct HspPlayIntent {
    session: std::sync::Weak<Session>,
    original: HspDispatchFacts,
}
pub(crate) struct HspWeakSessionHandle {
    session: std::sync::Weak<Session>,
    stopped: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    wake: thread::Thread,
}
impl HspWeakSessionHandle {
    pub(crate) fn is_retired(&self) -> bool {
        self.upgrade()
            .is_none_or(|handle| handle.ensure_open().is_err())
    }
    pub(crate) fn upgrade(&self) -> Option<HspSessionHandle> {
        self.session.upgrade().map(|session| HspSessionHandle {
            session,
            stopped: self.stopped.clone(),
            shutdown_requested: self.shutdown_requested.clone(),
            wake: self.wake.clone(),
        })
    }
    pub(crate) fn cancel(&self) {
        if let Some(handle) = self.upgrade() {
            handle.cancel();
        }
    }
}
pub(crate) struct HspSessionRetirement {
    session: std::sync::Weak<Session>,
    wake: thread::Thread,
}
impl HspSessionRetirement {
    pub(crate) fn cancel(&self) {
        if let Some(session) = self.session.upgrade() {
            session.cancelled.store(true, Ordering::Release);
            session.owner.retire_ordinary();
            self.wake.unpark();
        }
    }
}
#[cfg_attr(not(test), allow(dead_code))]
impl HspSessionHandle {
    pub(crate) fn downgrade(&self) -> HspWeakSessionHandle {
        HspWeakSessionHandle {
            session: Arc::downgrade(&self.session),
            stopped: self.stopped.clone(),
            shutdown_requested: self.shutdown_requested.clone(),
            wake: self.wake.clone(),
        }
    }
    pub(crate) fn retirement_handle(&self) -> HspSessionRetirement {
        HspSessionRetirement {
            session: Arc::downgrade(&self.session),
            wake: self.wake.clone(),
        }
    }

    fn ensure_open(&self) -> Result<(), HspExecutionError> {
        if self.stopped.load(Ordering::Acquire)
            || self.shutdown_requested.load(Ordering::Acquire)
            || self.session.cancelled.load(Ordering::Acquire)
        {
            return Err(HspExecutionError::Unavailable);
        }
        Ok(())
    }
    /// Timestamps must already use suspend-aware native boottime, not an Instant
    /// origin or browser clock. There is no intermediate unbounded point queue.
    pub(crate) fn offer(
        &self,
        identity: &LiveIdentity,
        sample: HspSample,
    ) -> Result<HspOfferOutcome, HspExecutionError> {
        self.ensure_open()?;
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        // The evaluation clock belongs to this serialized runtime mutation.
        // A concurrent producer may have advanced core time before this lock.
        // The sample's own capture and acceptance timestamps remain unchanged.
        let observation = self.session.authority.observe_current(&mut native_now)?;
        let now = observation.sampled_at_ns;
        let facts = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::Unavailable)?;
        self.ensure_open()?;
        let result = runtime
            .offer(identity, sample, now, &facts)
            .map_err(HspDispatchFailure::Authority)?;
        if result == HspOfferOutcome::Accepted {
            // Every producer updates this counter under the same runtime lock.
            // Readers only load it; no atomic update loop is needed.
            let Some(accepted) = self.session.accepted.load(Ordering::Acquire).checked_add(1)
            else {
                runtime.stop();
                self.session.owner.retire_ordinary();
                self.session.cancelled.store(true, Ordering::Release);
                return Err(HspExecutionError::Capacity);
            };
            self.session.accepted.store(accepted, Ordering::Release);
        }
        drop(runtime);
        self.wake.unpark();
        Ok(result)
    }
    pub(crate) fn prepare_source_transition(
        &self,
        spec: HspSourceTransitionSpec,
        facts_at: impl FnOnce(u64) -> Result<HspDispatchFacts, HspDispatchFailure>,
    ) -> Result<HspSourceTransition, HspExecutionError> {
        self.ensure_open()?;
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let at = now()?;
        // The caller retains the actual capture/timing transfer guards. This
        // closure reads those guards directly instead of recursively taking them.
        let facts = facts_at(at)?;
        self.ensure_open()?;
        runtime
            .prepare_source_transition(spec, at, &facts)
            .map_err(HspDispatchFailure::Authority)
            .map_err(Into::into)
    }
    pub(crate) fn commit_source_transition(
        &self,
        transition: &mut Option<HspSourceTransition>,
        facts_at: impl FnOnce(u64) -> Result<HspDispatchFacts, HspDispatchFailure>,
    ) -> Result<(), HspExecutionError> {
        self.ensure_open()?;
        if transition.is_none() {
            return Err(HspExecutionError::InvalidAdmission);
        }
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let at = now()?;
        let facts = facts_at(at)?;
        self.ensure_open()?;
        let accepted = self
            .session
            .accepted
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or(HspExecutionError::Capacity)?;
        // Local contention above leaves the sole original token with its caller.
        // Core consumes it exactly once and enqueues the real successor point.
        runtime
            .commit_source_transition(
                transition.take().expect("checked original token"),
                at,
                &facts,
            )
            .map_err(HspDispatchFailure::Authority)?;
        self.session.accepted.store(accepted, Ordering::Release);
        drop(runtime);
        self.wake.unpark();
        Ok(())
    }
    pub(crate) fn seal_input(
        &self,
        identity: &LiveIdentity,
        terminal: HspSample,
        closed_at_ns: u64,
        policy: HspFiniteDrainPolicy,
        maximum_stop_dispatch_delay_ns: u64,
    ) -> Result<HspInputSeal, HspExecutionError> {
        self.ensure_open()?;
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let observation = self.session.authority.observe_current(&mut native_now)?;
        let at = observation.sampled_at_ns;
        let facts = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::Unavailable)?;
        self.ensure_open()?;
        let seal = runtime
            .seal_input(
                identity,
                terminal,
                closed_at_ns,
                HspFiniteDrainConfig {
                    policy,
                    maximum_stop_dispatch_delay_ns,
                },
                at,
                &facts,
            )
            .map_err(HspDispatchFailure::Authority)?;
        drop(runtime);
        self.wake.unpark();
        Ok(seal)
    }
    pub(crate) fn capture_play_intent(&self) -> Result<HspPlayIntent, HspExecutionError> {
        self.ensure_open()?;
        let _runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let observation = self.session.authority.observe_current(&mut native_now)?;
        let at = observation.sampled_at_ns;
        let original = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::Unavailable)?;
        if original.reference().at_ns() != at {
            return Err(HspExecutionError::InvalidAdmission);
        }
        original
            .valid_at(at)
            .map_err(HspDispatchFailure::Authority)?;
        self.ensure_open()?;
        Ok(HspPlayIntent {
            session: Arc::downgrade(&self.session),
            original,
        })
    }
    pub(crate) fn request_original_play(
        &self,
        intent: HspPlayIntent,
    ) -> Result<(), HspExecutionError> {
        self.ensure_open()?;
        if !std::sync::Weak::ptr_eq(&intent.session, &Arc::downgrade(&self.session)) {
            return Err(HspExecutionError::InvalidAdmission);
        }
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        let observation = self.session.authority.observe_current(&mut native_now)?;
        let at = observation.sampled_at_ns;
        let current = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::Unavailable)?;
        self.ensure_open()?;
        runtime
            .request_play_original(at, &current, &intent.original)
            .map_err(HspDispatchFailure::Authority)?;
        drop(runtime);
        self.wake.unpark();
        Ok(())
    }
    /// Explicit user intent retains this exact original reference and expiry.
    pub(crate) fn request_play(&self) -> Result<(), HspExecutionError> {
        self.ensure_open()?;
        let mut runtime = self
            .session
            .runtime
            .try_lock()
            .map_err(|_| HspExecutionError::Busy)?;
        // Capture the explicit reference only while accepting it into this
        // runtime. An already-pending Play keeps its original reference/expiry.
        let observation = self.session.authority.observe_current(&mut native_now)?;
        let now = observation.sampled_at_ns;
        let facts = observation
            .snapshot
            .facts
            .ok_or(HspExecutionError::Unavailable)?;
        self.ensure_open()?;
        runtime
            .request_play(now, &facts)
            .map_err(HspDispatchFailure::Authority)?;
        drop(runtime);
        self.wake.unpark();
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn metadata_busy_for_test(&self) -> Option<HspMetadataBusyObservation> {
        self.session.metadata_busy.try_lock().ok()?.clone()
    }
    #[cfg(test)]
    pub(crate) fn hold_next_metadata_for_test(
        &self,
        kind: HspMetadataTaskKind,
    ) -> HspMetadataTestHold {
        let pause = Arc::new(MetadataPause {
            kind,
            state: Mutex::new(MetadataPauseState {
                identity: None,
                released: false,
            }),
            changed: std::sync::Condvar::new(),
            joined: AtomicBool::new(false),
        });
        *self.session.metadata_pause.lock().unwrap() = Some(pause.clone());
        HspMetadataTestHold(pause)
    }
    #[cfg(test)]
    pub(crate) fn watchdog_busy_ticks_for_test(&self) -> u64 {
        self.session.watchdog_busy_ticks.load(Ordering::Acquire)
    }
    #[cfg(test)]
    pub(crate) fn watchdog_progress_for_test(&self) -> (u64, bool) {
        (
            self.session
                .watchdog_successful_ticks
                .load(Ordering::Acquire),
            self.session
                .watchdog_contention_retired
                .load(Ordering::Acquire),
        )
    }
    #[cfg(test)]
    pub(crate) fn first_stop_for_test(&self) -> Option<HspFirstStopObservation> {
        self.session.first_stop.get().cloned()
    }
    #[cfg(test)]
    pub(crate) fn runtime_for_test(&self) -> Arc<Mutex<HandyHspRuntime>> {
        self.session.runtime.clone()
    }
    pub(crate) fn cancel(&self) {
        self.session.cancelled.store(true, Ordering::Release);
        self.session.owner.retire_ordinary();
        self.wake.unpark();
    }
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HspMetadataTaskKind {
    Prepare,
    Complete,
    Cancel,
    Emergency,
    Membership,
}
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HspMetadataTaskIdentity {
    pub kind: HspMetadataTaskKind,
    pub sequence: Option<u64>,
    pub token: u64,
}
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct HspMetadataBusyObservation {
    pub identity: HspMetadataTaskIdentity,
    pub first_attempt: Instant,
    pub busy_count: u64,
}

#[cfg(test)]
struct MetadataPauseState {
    identity: Option<HspMetadataTaskIdentity>,
    released: bool,
}
#[cfg(test)]
struct MetadataPause {
    kind: HspMetadataTaskKind,
    state: Mutex<MetadataPauseState>,
    changed: std::sync::Condvar,
    joined: AtomicBool,
}
/// The caller owns release; unwinding releases the test worker as well.
#[cfg(test)]
pub(crate) struct HspMetadataTestHold(Arc<MetadataPause>);
#[cfg(test)]
impl HspMetadataTestHold {
    pub(crate) fn wait_entered(
        &self,
        deadline: Instant,
    ) -> Result<HspMetadataTaskIdentity, HspExecutionError> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| HspExecutionError::Unavailable)?;
        loop {
            if let Some(identity) = &state.identity {
                return Ok(identity.clone());
            }
            let wait = deadline
                .checked_duration_since(Instant::now())
                .ok_or(HspExecutionError::Unavailable)?;
            state = self
                .0
                .changed
                .wait_timeout(state, wait)
                .map_err(|_| HspExecutionError::Unavailable)?
                .0;
        }
    }
    pub(crate) fn release(&self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.released = true;
            self.0.changed.notify_all();
        }
    }
    pub(crate) fn joined(&self) -> bool {
        self.0.joined.load(Ordering::Acquire)
    }
}
#[cfg(test)]
impl Drop for HspMetadataTestHold {
    fn drop(&mut self) {
        self.release();
    }
}
#[cfg(test)]
impl MetadataPause {
    fn wait(&self, identity: HspMetadataTaskIdentity) -> Result<(), HspExecutionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| HspExecutionError::Unavailable)?;
        state.identity = Some(identity);
        self.changed.notify_all();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.released)
            .map_err(|_| HspExecutionError::Unavailable)?;
        if state.released {
            Ok(())
        } else {
            Err(HspExecutionError::Unavailable)
        }
    }
}

#[derive(Clone)]
struct PrepareMetadata {
    record: HspStreamRecord,
    stream: Option<HspStreamReceipt>,
    effect: HspEffect,
    qualification: Option<HspQualificationLinkRecord>,
    preparation: Option<PreparedHspPreparationReceipt>,
}
#[derive(Clone)]
struct CompleteMetadata {
    claim: ClaimedHspReceipt,
    record: HspCompletionRecord,
}
#[derive(Clone)]
struct CancelMetadata {
    record: HspStreamRecord,
    stream: Option<HspStreamReceipt>,
    effect: HspEffect,
    receipt: Option<PreparedHspReceipt>,
    qualification: Option<HspQualificationLinkRecord>,
    preparation: Option<PreparedHspPreparationReceipt>,
}
#[derive(Clone)]
struct EmergencyMetadata {
    record: HspStreamRecord,
    observation: HspEmergencyStopObservation,
}
#[derive(Clone)]
enum MetadataTask {
    Prepare(Box<PrepareMetadata>),
    Complete(Box<CompleteMetadata>),
    Cancel(Box<CancelMetadata>),
    Emergency(Box<EmergencyMetadata>),
    Membership { stream: String, token: u64 },
}
impl MetadataTask {
    #[cfg(test)]
    fn identity(&self, token: u64) -> HspMetadataTaskIdentity {
        let (kind, sequence) = match self {
            Self::Prepare(task) => (HspMetadataTaskKind::Prepare, Some(task.effect.sequence())),
            Self::Complete(task) => (
                HspMetadataTaskKind::Complete,
                Some(task.record.effect.sequence()),
            ),
            Self::Cancel(task) => (HspMetadataTaskKind::Cancel, Some(task.effect.sequence())),
            Self::Emergency(task) => (
                HspMetadataTaskKind::Emergency,
                Some(task.observation.record().effect.sequence()),
            ),
            Self::Membership { .. } => (HspMetadataTaskKind::Membership, None),
        };
        HspMetadataTaskIdentity {
            kind,
            sequence,
            token,
        }
    }
    fn execute(
        &self,
        shared: &Mutex<DeviceEffectJournal>,
    ) -> Result<MetadataOutput, HspExecutionError> {
        // The pump never executes this function. A successful try-lock does not
        // bound storage latency; this entire transaction belongs to its worker.
        let mut journal = match shared.try_lock() {
            Ok(journal) => journal,
            Err(TryLockError::WouldBlock) => return Err(HspExecutionError::Busy),
            Err(TryLockError::Poisoned(_)) => {
                return Err(HspExecutionError::Dispatch(HspDispatchFailure::Journal))
            }
        };
        let failed = |_| HspExecutionError::Dispatch(HspDispatchFailure::Journal);
        match self {
            Self::Prepare(task) => {
                let (stream, receipt) = prepare_metadata(
                    &mut journal,
                    &task.record,
                    task.stream.as_ref(),
                    &task.effect,
                    task.qualification.as_ref(),
                    task.preparation.as_ref(),
                )
                .map_err(failed)?;
                Ok(MetadataOutput::Prepared { stream, receipt })
            }
            Self::Complete(task) => {
                journal
                    .try_complete_hsp(&task.claim, &task.record)
                    .map_err(failed)?;
                Ok(MetadataOutput::Done)
            }
            Self::Cancel(task) => {
                let (stream, receipt) =
                    if let (Some(stream), Some(receipt)) = (&task.stream, &task.receipt) {
                        (stream.clone(), receipt.clone())
                    } else {
                        prepare_metadata(
                            &mut journal,
                            &task.record,
                            task.stream.as_ref(),
                            &task.effect,
                            task.qualification.as_ref(),
                            task.preparation.as_ref(),
                        )
                        .map_err(failed)?
                    };
                journal
                    .try_cancel_hsp_effect(&receipt, &task.effect)
                    .map_err(failed)?;
                if task.effect.kind() == HspEffectKind::Setup {
                    journal.cancel_hsp_stream(&stream).map_err(failed)?;
                }
                Ok(MetadataOutput::Cancelled { stream })
            }
            Self::Emergency(task) => {
                journal
                    .persist_hsp_emergency(&task.record, &task.observation)
                    .map_err(failed)?;
                Ok(MetadataOutput::Done)
            }
            Self::Membership { stream, .. } => Ok(MetadataOutput::Membership(Box::new(
                journal.hsp_stream_membership(stream).map_err(failed)?,
            ))),
        }
    }
}
fn prepare_metadata(
    journal: &mut DeviceEffectJournal,
    record: &HspStreamRecord,
    stream: Option<&HspStreamReceipt>,
    effect: &HspEffect,
    qualification: Option<&HspQualificationLinkRecord>,
    preparation: Option<&PreparedHspPreparationReceipt>,
) -> anyhow::Result<(HspStreamReceipt, PreparedHspReceipt)> {
    if let Some(stream) = stream {
        return journal
            .try_prepare_hsp_effect(stream, effect)
            .map(|receipt| (stream.clone(), receipt));
    }
    if let Some(parent) = preparation {
        let link = qualification
            .ok_or_else(|| anyhow::anyhow!("HSP child requires its original qualification link"))?;
        return journal.prepare_hsp_stream_from_preparation(parent, record, effect, link);
    }
    if let Some(link) = qualification {
        journal.prepare_hsp_stream_effect_linked(record, effect, link)
    } else {
        journal.try_prepare_hsp_stream_effect(record, effect)
    }
}
enum MetadataOutput {
    Prepared {
        stream: HspStreamReceipt,
        receipt: PreparedHspReceipt,
    },
    Cancelled {
        stream: HspStreamReceipt,
    },
    Done,
    Membership(Box<HspStreamMembership>),
}
struct MetadataRetry {
    task: MetadataTask,
    token: u64,
    first_attempt: Instant,
}
struct MetadataActive {
    task: MetadataTask,
    token: u64,
    first_attempt: Instant,
    worker: JoinHandle<Result<MetadataOutput, HspExecutionError>>,
    #[cfg(test)]
    pause: Option<Arc<MetadataPause>>,
}

struct DurableMetadata {
    completion: Box<HspDispatchCompletion>,
    record: HspCompletionRecord,
}
struct UnsentMetadata {
    effect: HspEffect,
    receipt: Option<PreparedHspReceipt>,
}
struct Work {
    record: HspStreamRecord,
    qualification: Option<HspQualificationLinkRecord>,
    session: Arc<Session>,
    stopped: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    transport: Option<HandyHspTransport>,
    dispatcher: Option<HandyHspDispatcher>,
    stream: Option<HspStreamReceipt>,
    pending: Option<HspEffect>,
    prepared: Option<PreparedHspReceipt>,
    active: bool,
    active_ordinary: bool,
    joined: Option<HspJoinedCompletion>,
    ordinary_metadata: Option<DurableMetadata>,
    stop_metadata: Option<DurableMetadata>,
    unsent: Option<UnsentMetadata>,
    unused_stop: Option<UnsentMetadata>,
    metadata_worker: Option<MetadataActive>,
    metadata_retry: Option<MetadataRetry>,
    next_metadata_token: u64,
    metadata_retry_at: Instant,
    metadata_broken: bool,
    owned_join_failed: bool,
    emergency_recorded: bool,
    emergency_rejection_recorded: bool,
    recovery_membership: Option<(u64, HspStreamMembership)>,
    released: bool,
    frozen_recovery: Option<u64>,
    rejected_stop: Option<Box<HspDispatchCompletion>>,
    emergency: Option<Box<HspEmergencyStopCompletion>>,
    post_setup: Option<HspEffect>,
    post_setup_bound: bool,
    probe_failed: bool,
    retired: bool,
    stop_blocked: bool,
    force_emergency: bool,
    watchdog_blocked_since_ns: Option<u64>,
    last_joined: Option<HspJoinedObservation>,
    preparation: Option<PreparedHspPreparationReceipt>,
}
impl Work {
    #[cfg_attr(not(test), allow(dead_code))]
    fn new(
        admission: HspAdmission,
        stopped: Arc<AtomicBool>,
        shutdown_requested: Arc<AtomicBool>,
    ) -> Self {
        Self {
            record: admission.record,
            qualification: admission.qualification,
            preparation: admission.preparation,
            session: admission.session,
            stopped,
            shutdown_requested,
            transport: Some(admission.transport),
            dispatcher: None,
            stream: None,
            pending: Some(admission.setup),
            prepared: None,
            active: false,
            active_ordinary: false,
            joined: None,
            ordinary_metadata: None,
            stop_metadata: None,
            unsent: None,
            unused_stop: None,
            metadata_worker: None,
            metadata_retry: None,
            next_metadata_token: 0,
            metadata_retry_at: Instant::now(),
            metadata_broken: false,
            owned_join_failed: false,
            emergency_recorded: false,
            emergency_rejection_recorded: false,
            recovery_membership: None,
            released: false,
            frozen_recovery: None,
            rejected_stop: None,
            emergency: None,
            post_setup: None,
            post_setup_bound: false,
            probe_failed: false,
            retired: false,
            stop_blocked: false,
            force_emergency: false,
            watchdog_blocked_since_ns: None,
            last_joined: None,
        }
    }
    #[track_caller]
    fn record_first_stop(
        &self,
        reason: HspFirstStopReason,
        failure: Option<HspDispatchFailure>,
        sampled_at_ns: Option<u64>,
        facts: Option<&HspDispatchFacts>,
        runtime: Option<&HandyHspRuntime>,
    ) {
        if self.session.first_stop.get().is_some() {
            return;
        }
        let location = std::panic::Location::caller();
        let effect = runtime
            .and_then(HandyHspRuntime::in_flight)
            .or(self.pending.as_ref())
            .or(self.post_setup.as_ref())
            .map(HspStopEffectObservation::capture);
        // Work has a single owning pump thread. Readers can inspect this once
        // initialized cell without acquiring a runtime or dependency lock.
        let _ = self.session.first_stop.set(HspFirstStopObservation {
            reason,
            failure,
            sampled_at_ns,
            facts: facts.map(HspStopFactsObservation::capture),
            runtime_state: runtime.map(HandyHspRuntime::state),
            effect,
            active_native_worker: self.active,
            active_ordinary: self.active_ordinary,
            post_setup_bound: self.post_setup_bound,
            last_joined: self.last_joined.clone(),
            source_file: location.file(),
            source_line: location.line(),
        });
    }
    #[track_caller]
    fn latch(&mut self) {
        // Covers every unclassified native latch site without guessing its
        // initiating cause. Precise branch observations below win when present.
        self.record_first_stop(HspFirstStopReason::OtherNativeLatch, None, None, None, None);
        self.retired = true;
        self.session.cancelled.store(true, Ordering::Release);
        self.session.owner.retire_ordinary();
        self.session.authority.request_owned_stop();
        if let Ok(mut runtime) = self.session.runtime.try_lock() {
            runtime.stop();
        }
        if self.active && self.active_ordinary {
            if let Some(dispatcher) = &self.dispatcher {
                dispatcher.cancel();
            }
        }
    }
    fn metadata_pending(&self) -> bool {
        self.ordinary_metadata.is_some()
            || self.stop_metadata.is_some()
            || self.unsent.is_some()
            || self.unused_stop.is_some()
            || self.rejected_stop.is_some()
            || self.metadata_worker.is_some()
            || self.metadata_retry.is_some()
            || self.metadata_broken
            || self.emergency.as_ref().is_some_and(|completion| {
                if completion.observation().is_some() {
                    !self.emergency_recorded
                } else {
                    !self.emergency_rejection_recorded
                }
            })
    }
    fn record_join(&mut self, effect: &HspEffect, outcome: HspJoinedOutcome) {
        self.last_joined = Some(HspJoinedObservation {
            sequence: effect.sequence(),
            kind: effect.kind(),
            outcome,
        });
    }
    fn observe_joined(&mut self) {
        let Some(mut joined) = self.joined.take() else {
            return;
        };
        match &mut joined {
            HspJoinedCompletion::Durable(completion) => {
                let effect = completion.request.effect.clone();
                if let HspDispatchAttempt::Rejected(error) = &completion.attempt {
                    self.record_join(&effect, HspJoinedOutcome::Rejected);
                    self.record_first_stop(
                        HspFirstStopReason::JoinedDispatchRejected,
                        Some(error.clone()),
                        None,
                        effect.original_facts(),
                        None,
                    );
                    self.latch();
                    if effect.kind() == HspEffectKind::Stop {
                        // A joined rejection with no claim is not a sent Stop.
                        // If the journal alone refused it, attempt the same exact
                        // native Stop once through the metadata-independent path.
                        let fallback = completion.claim.is_none()
                            && matches!(
                                completion.attempt,
                                HspDispatchAttempt::Rejected(HspDispatchFailure::Journal)
                            );
                        self.stop_blocked = !fallback;
                        if fallback {
                            self.pending = Some(effect);
                            self.force_emergency = true;
                        }
                        let HspJoinedCompletion::Durable(completion) = joined else {
                            unreachable!()
                        };
                        self.rejected_stop = Some(completion);
                    } else {
                        self.pending = Some(effect);
                        self.prepared = Some(completion.request.receipt.clone());
                    }
                    return;
                }
                let observed = match self.session.runtime.try_lock() {
                    Ok(mut runtime) => observe_for_settlement(self.session.authority.as_ref())
                        .and_then(|observation| {
                            completion.observe(
                                &mut runtime,
                                observation.sampled_at_ns,
                                observation.snapshot.facts.as_ref(),
                            )
                        }),
                    Err(_) => {
                        self.joined = Some(joined);
                        return;
                    }
                };
                let record = match observed {
                    Ok(record) => record,
                    Err(error) => {
                        self.record_first_stop(
                            HspFirstStopReason::JoinedObservationFailed,
                            Some(error),
                            None,
                            effect.original_facts(),
                            None,
                        );
                        self.latch();
                        self.joined = Some(joined);
                        return;
                    }
                };
                let outcome = match record.outcome {
                    HspObservedOutcome::Acknowledged(_) => HspJoinedOutcome::Acknowledged,
                    HspObservedOutcome::Unknown => HspJoinedOutcome::Unknown,
                };
                self.record_join(&effect, outcome);
                if effect.kind() == HspEffectKind::Setup {
                    self.post_setup = Some(effect.clone());
                }
                if outcome == HspJoinedOutcome::Unknown {
                    self.record_first_stop(
                        HspFirstStopReason::JoinedOutcomeUnknown,
                        None,
                        None,
                        effect.original_facts(),
                        None,
                    );
                    self.latch();
                }
                let HspJoinedCompletion::Durable(completion) = joined else {
                    unreachable!()
                };
                let metadata = DurableMetadata { completion, record };
                if effect.kind() == HspEffectKind::Stop {
                    self.stop_metadata = Some(metadata);
                } else {
                    self.ordinary_metadata = Some(metadata);
                }
            }
            HspJoinedCompletion::Emergency(completion) => {
                let effect = completion.effect().clone();
                if matches!(completion.attempt(), HspDispatchAttempt::Rejected(_)) {
                    self.record_join(&effect, HspJoinedOutcome::Rejected);
                    self.stop_blocked = true;
                } else {
                    let observed = match self.session.runtime.try_lock() {
                        Ok(mut runtime) => native_now().and_then(|now_ns| {
                            completion
                                .observe(&mut runtime, now_ns)
                                .map(|r| r.record().outcome)
                        }),
                        Err(_) => {
                            self.joined = Some(joined);
                            return;
                        }
                    };
                    match observed {
                        Ok(HspObservedOutcome::Acknowledged(_)) => {
                            self.record_join(&effect, HspJoinedOutcome::Acknowledged)
                        }
                        Ok(HspObservedOutcome::Unknown) => {
                            self.record_join(&effect, HspJoinedOutcome::Unknown)
                        }
                        Err(_) => {
                            self.stop_blocked = true;
                            self.joined = Some(joined);
                            return;
                        }
                    }
                }
                let HspJoinedCompletion::Emergency(completion) = joined else {
                    unreachable!()
                };
                self.emergency = Some(completion);
            }
            HspJoinedCompletion::PostSetupProbe(completion) => {
                match self
                    .dispatcher
                    .as_mut()
                    .expect("initialized dispatcher")
                    .install_post_setup_probe(completion)
                {
                    Ok(()) => {
                        self.post_setup_bound = true;
                        self.post_setup = None;
                    }
                    Err(error) => {
                        self.record_first_stop(
                            HspFirstStopReason::PostSetupProbeInstall,
                            Some(error),
                            None,
                            None,
                            None,
                        );
                        self.probe_failed = true;
                        self.stop_blocked = true;
                        self.latch();
                    }
                }
            }
        }
    }
    fn spawn_metadata(
        &mut self,
        task: MetadataTask,
        journal: &Arc<Mutex<DeviceEffectJournal>>,
    ) -> bool {
        if self.metadata_worker.is_some()
            || self.metadata_broken
            || Instant::now() < self.metadata_retry_at
        {
            return false;
        }
        let MetadataRetry {
            task,
            token,
            first_attempt,
        } = if let Some(retry) = self.metadata_retry.take() {
            retry
        } else {
            let Some(token) = self.next_metadata_token.checked_add(1) else {
                self.metadata_broken = true;
                self.latch();
                return false;
            };
            self.next_metadata_token = token;
            MetadataRetry {
                task,
                token,
                first_attempt: Instant::now(),
            }
        };
        #[cfg(test)]
        let pause = {
            let mut slot = self.session.metadata_pause.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|pause| pause.kind == task.identity(token).kind)
            {
                slot.take()
            } else {
                None
            }
        };
        let captured = task.clone();
        let journal = journal.clone();
        #[cfg(test)]
        let held = pause.clone();
        let worker = thread::Builder::new()
            .name("pulsar-hsp-metadata".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(pause) = held {
                    pause.wait(captured.identity(token))?;
                }
                captured.execute(&journal)
            });
        match worker {
            Ok(worker) => {
                self.metadata_worker = Some(MetadataActive {
                    task,
                    token,
                    first_attempt,
                    worker,
                    #[cfg(test)]
                    pause,
                });
                true
            }
            Err(_) => {
                self.metadata_retry = Some(MetadataRetry {
                    task,
                    token,
                    first_attempt,
                });
                self.metadata_retry_at = Instant::now() + Duration::from_millis(100);
                self.latch();
                false
            }
        }
    }
    /// `join` is used only by final owner shutdown. Normal ticks only join an
    /// already-finished worker and never hold a runtime/journal/registry guard.
    fn collect_metadata(&mut self, join: bool) -> Option<bool> {
        if !join
            && self
                .metadata_worker
                .as_ref()
                .is_none_or(|active| !active.worker.is_finished())
        {
            return None;
        }
        let active = self.metadata_worker.take()?;
        let result = active
            .worker
            .join()
            .unwrap_or(Err(HspExecutionError::Unavailable));
        #[cfg(test)]
        if let Some(pause) = active.pause {
            pause.joined.store(true, Ordering::Release);
        }
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                let busy = error == HspExecutionError::Busy;
                #[cfg(test)]
                if busy {
                    if let Ok(mut retained) = self.session.metadata_busy.try_lock() {
                        let identity = active.task.identity(active.token);
                        let count = retained
                            .as_ref()
                            .filter(|prior| prior.identity == identity)
                            .map_or(1, |prior| prior.busy_count.saturating_add(1));
                        *retained = Some(HspMetadataBusyObservation {
                            identity,
                            first_attempt: active.first_attempt,
                            busy_count: count,
                        });
                    }
                }
                // Mutex WouldBlock made no journal call. Retry exactly this
                // metadata task/token without resetting its first-attempt
                // budget. Poison/actual DB failure still retires immediately.
                if !busy || active.first_attempt.elapsed() >= DISPATCH_BUDGET {
                    self.latch();
                }
                if let MetadataTask::Prepare(task) = &active.task {
                    if task.effect.kind() == HspEffectKind::Stop {
                        self.force_emergency = true;
                    }
                }
                self.metadata_retry = Some(MetadataRetry {
                    task: active.task,
                    token: active.token,
                    first_attempt: active.first_attempt,
                });
                self.metadata_retry_at = Instant::now()
                    + if busy {
                        TICK
                    } else {
                        Duration::from_millis(100)
                    };
                return Some(false);
            }
        };
        let applied = match (active.task, output) {
            (MetadataTask::Prepare(task), MetadataOutput::Prepared { stream, receipt }) => {
                self.stream = Some(stream);
                if self.pending.as_ref() == Some(&task.effect) && !self.force_emergency {
                    self.prepared = Some(receipt);
                    true
                } else if let Some(unsent) = self
                    .unsent
                    .as_mut()
                    .filter(|unsent| unsent.effect == task.effect)
                {
                    unsent.receipt = Some(receipt);
                    true
                } else if task.effect.kind() == HspEffectKind::Stop {
                    // Native fallback may have run while storage was stalled.
                    // Cancel only this unused durable preparation; the separate
                    // emergency observation keeps its no-prewrite provenance.
                    self.unused_stop = Some(UnsentMetadata {
                        effect: task.effect,
                        receipt: Some(receipt),
                    });
                    true
                } else {
                    false
                }
            }
            (MetadataTask::Complete(task), MetadataOutput::Done) => {
                let slot = if task.record.effect.kind() == HspEffectKind::Stop {
                    &mut self.stop_metadata
                } else {
                    &mut self.ordinary_metadata
                };
                if slot.as_ref().is_some_and(|current| {
                    current.record == task.record
                        && current.completion.claim.as_ref() == Some(&task.claim)
                }) {
                    *slot = None;
                    true
                } else {
                    false
                }
            }
            (MetadataTask::Cancel(task), MetadataOutput::Cancelled { stream }) => {
                self.stream = Some(stream);
                let mut found = false;
                for slot in [&mut self.unsent, &mut self.unused_stop] {
                    if slot
                        .as_ref()
                        .is_some_and(|current| current.effect == task.effect)
                    {
                        *slot = None;
                        found = true;
                    }
                }
                if self.rejected_stop.as_ref().is_some_and(|current| {
                    current.request.effect == task.effect && current.claim.is_none()
                }) {
                    self.rejected_stop = None;
                    found = true;
                }
                if self.emergency.as_ref().is_some_and(|current| {
                    current.effect() == &task.effect
                        && matches!(current.attempt(), HspDispatchAttempt::Rejected(_))
                }) {
                    self.emergency_rejection_recorded = true;
                    found = true;
                }
                if self.stop_blocked
                    && self.pending.as_ref() == Some(&task.effect)
                    && task.effect.kind() == HspEffectKind::Stop
                {
                    self.pending = None;
                    self.prepared = None;
                    found = true;
                }
                found
            }
            (MetadataTask::Emergency(task), MetadataOutput::Done) => {
                if self
                    .emergency
                    .as_ref()
                    .and_then(|current| current.observation())
                    .is_some_and(|current| current == &task.observation)
                {
                    self.emergency_recorded = true;
                    true
                } else {
                    false
                }
            }
            (MetadataTask::Membership { token, .. }, MetadataOutput::Membership(membership)) => {
                if token == self.session.recovery_token.load(Ordering::Acquire) {
                    self.recovery_membership = Some((token, *membership));
                }
                // A superseded read remains only historical data. Its result
                // cannot certify the newer target token or release any owner.
                true
            }
            _ => false,
        };
        if !applied {
            self.metadata_broken = true;
            self.stop_blocked = true;
            self.latch();
        }
        Some(applied)
    }
    fn metadata_task(&self) -> Option<MetadataTask> {
        if let Some(retry) = &self.metadata_retry {
            return Some(retry.task.clone());
        }
        if let Some(metadata) = [&self.ordinary_metadata, &self.stop_metadata]
            .into_iter()
            .flatten()
            .next()
        {
            if let Some(claim) = &metadata.completion.claim {
                return Some(MetadataTask::Complete(Box::new(CompleteMetadata {
                    claim: claim.clone(),
                    record: metadata.record.clone(),
                })));
            }
            return None;
        }
        if let Some(unsent) = [&self.unsent, &self.unused_stop]
            .into_iter()
            .flatten()
            .next()
        {
            return Some(self.cancel_task(&unsent.effect, unsent.receipt.as_ref()));
        }
        if let Some(rejected) = &self.rejected_stop {
            if rejected.claim.is_none() {
                return Some(
                    self.cancel_task(&rejected.request.effect, Some(&rejected.request.receipt)),
                );
            }
            return None;
        }
        if let Some(emergency) = &self.emergency {
            if let Some(observation) = emergency.observation() {
                if !self.emergency_recorded {
                    return Some(MetadataTask::Emergency(Box::new(EmergencyMetadata {
                        record: self.record.clone(),
                        observation: observation.clone(),
                    })));
                }
            } else if !self.emergency_rejection_recorded
                && matches!(emergency.attempt(), HspDispatchAttempt::Rejected(_))
            {
                return Some(self.cancel_task(emergency.effect(), None));
            }
        }
        if self.stop_blocked {
            if let Some(effect) = self
                .pending
                .as_ref()
                .filter(|effect| effect.kind() == HspEffectKind::Stop)
            {
                return Some(self.cancel_task(effect, self.prepared.as_ref()));
            }
        }
        None
    }
    fn cancel_task(
        &self,
        effect: &HspEffect,
        receipt: Option<&PreparedHspReceipt>,
    ) -> MetadataTask {
        MetadataTask::Cancel(Box::new(CancelMetadata {
            record: self.record.clone(),
            stream: self.stream.clone(),
            effect: effect.clone(),
            receipt: receipt.cloned(),
            qualification: self.qualification.clone(),
            preparation: self.preparation.clone(),
        }))
    }
    fn persist_metadata(&mut self, journal: &Arc<Mutex<DeviceEffectJournal>>) {
        if self.metadata_worker.is_some() {
            return;
        }
        if let Some(task) = self.metadata_task() {
            self.spawn_metadata(task, journal);
        }
    }
    fn retire_pending(&mut self) {
        let Some(effect) = self.pending.as_ref() else {
            return;
        };
        if effect.kind() == HspEffectKind::Stop {
            return;
        }
        if self
            .dispatcher
            .as_ref()
            .expect("initialized dispatcher")
            .retire_native_unsent(effect, &self.session.owner)
            .is_ok()
        {
            self.unsent = Some(UnsentMetadata {
                effect: self.pending.take().expect("checked pending"),
                receipt: self.prepared.take(),
            });
        }
    }
    fn prepare(&mut self, journal: &Arc<Mutex<DeviceEffectJournal>>) -> bool {
        if self.prepared.is_some() {
            return true;
        }
        let Some(effect) = self.pending.clone() else {
            return false;
        };
        if self.metadata_worker.is_some() {
            if effect.kind() == HspEffectKind::Stop {
                // Give an already-finished preparation one poll; a stalled
                // metadata lane never prevents the bound native Stop fallback.
                self.force_emergency = true;
            }
            return false;
        }
        if self.metadata_task().is_some() {
            if effect.kind() == HspEffectKind::Stop {
                self.force_emergency = true;
            }
            return false;
        }
        self.spawn_metadata(
            MetadataTask::Prepare(Box::new(PrepareMetadata {
                record: self.record.clone(),
                stream: self.stream.clone(),
                effect,
                qualification: self.qualification.clone(),
                preparation: self.preparation.clone(),
            })),
            journal,
        );
        false
    }
    fn start_pending(&mut self, journal: &Arc<Mutex<DeviceEffectJournal>>) {
        let Some(effect) = self.pending.clone() else {
            return;
        };
        if self.active || self.joined.is_some() || self.stop_blocked {
            return;
        }
        if effect.kind() != HspEffectKind::Setup && !self.post_setup_bound {
            self.stop_blocked = true;
            self.latch();
            return;
        }
        let prepared = !self.force_emergency && self.prepare(journal);
        if !prepared {
            if effect.kind() != HspEffectKind::Stop || !self.force_emergency {
                return;
            }
            let dispatcher = self.dispatcher.as_mut().expect("initialized dispatcher");
            match dispatcher.prepare_emergency_stop(effect, &self.session.owner) {
                Ok(request) => match dispatcher
                    .start_emergency_stop(request, Instant::now() + DISPATCH_BUDGET)
                {
                    Ok(()) => {
                        self.pending = None;
                        self.active = true;
                        self.active_ordinary = false;
                    }
                    Err(_) => {
                        self.stop_blocked = true;
                    }
                },
                Err(_) => {
                    self.stop_blocked = true;
                }
            }
            return;
        }
        let request = HspDispatchRequest {
            effect,
            receipt: self.prepared.take().expect("prepared"),
        };
        let authority = self.session.authority.clone();
        let owner = self.session.owner.clone();
        let stopped = self.stopped.clone();
        let shutdown = self.shutdown_requested.clone();
        let ordinary = request.effect.kind() != HspEffectKind::Stop;
        match self
            .dispatcher
            .as_mut()
            .expect("initialized dispatcher")
            .start(
                request,
                Instant::now() + DISPATCH_BUDGET,
                move |deadline, cancel, gate| {
                    if ordinary
                        && (stopped.load(Ordering::Acquire) || shutdown.load(Ordering::Acquire))
                    {
                        owner.retire_ordinary();
                        return Err(HspDispatchFailure::Cancelled);
                    }
                    let gate = gate.with_retirement_flags(stopped, shutdown);
                    if ordinary {
                        authority.authorize(deadline, cancel, gate, &owner)
                    } else {
                        gate.apply(&owner, |_| Ok(None))
                    }
                },
            ) {
            Ok(()) => {
                self.pending = None;
                self.active = true;
                self.active_ordinary = ordinary;
            }
            Err(failure) => {
                self.pending = Some(failure.request.effect);
                self.prepared = Some(failure.request.receipt);
                self.latch();
                if ordinary {
                    self.retire_pending();
                } else {
                    self.stop_blocked = true;
                }
            }
        }
    }
    /// Return true only after the runtime itself ran its watchdog. During a
    /// short lock conflict we admit no new command in this pump tick. Native
    /// dispatch still checks exact original facts at claim and every I/O boundary.
    fn watchdog(&mut self) -> bool {
        // Preserve the callback's existing sample even when the provider returns
        // an error. This wrapper performs no additional clock read.
        let mut sampled_at = None;
        let observation = match self.session.authority.observe_current(&mut || {
            let sampled = native_now();
            sampled_at = sampled.as_ref().ok().copied();
            sampled
        }) {
            Ok(observation) => observation,
            Err(error) => {
                self.record_first_stop(
                    HspFirstStopReason::WatchdogCurrentObservation,
                    Some(error),
                    sampled_at,
                    None,
                    None,
                );
                self.latch();
                return false;
            }
        };
        let now_ns = observation.sampled_at_ns;
        let Some(facts) = observation.snapshot.facts else {
            self.record_first_stop(
                HspFirstStopReason::WatchdogMissingFacts,
                None,
                Some(now_ns),
                None,
                None,
            );
            self.latch();
            return false;
        };
        if let Err(error) = facts.valid_at(now_ns) {
            self.record_first_stop(
                HspFirstStopReason::WatchdogInvalidFacts,
                Some(HspDispatchFailure::Authority(error)),
                Some(now_ns),
                Some(&facts),
                None,
            );
            self.latch();
            return false;
        }
        if facts.reference().at_ns() != now_ns {
            self.record_first_stop(
                HspFirstStopReason::WatchdogReferenceMismatch,
                None,
                Some(now_ns),
                Some(&facts),
                None,
            );
            self.latch();
            return false;
        }
        if HspBindingRecord::capture(facts.binding()) != self.record.binding {
            self.record_first_stop(
                HspFirstStopReason::WatchdogBindingMismatch,
                None,
                Some(now_ns),
                Some(&facts),
                None,
            );
            self.latch();
            return false;
        }
        let (checked, poisoned) = match self.session.runtime.try_lock() {
            Ok(mut runtime) => {
                let mut snapshot_sample = None;
                let checked = self
                    .session
                    .authority
                    .observe_snapshot(&mut || {
                        let sampled = native_now();
                        snapshot_sample = sampled.as_ref().ok().copied();
                        sampled
                    })
                    .inspect_err(|error| {
                        self.record_first_stop(
                            HspFirstStopReason::WatchdogSnapshotObservation,
                            Some(error.clone()),
                            snapshot_sample,
                            None,
                            Some(&runtime),
                        );
                    })
                    .and_then(|observation| {
                        let outcome = runtime
                            .tick_snapshot(
                                observation.sampled_at_ns,
                                observation.snapshot.facts.as_ref(),
                                observation.snapshot.drain_window(),
                                !self.active && self.joined.is_none() && self.post_setup.is_none(),
                            )
                            .map_err(HspDispatchFailure::Authority)
                            .inspect_err(|error| {
                                self.record_first_stop(
                                    HspFirstStopReason::WatchdogRuntimeTick,
                                    Some(error.clone()),
                                    Some(observation.sampled_at_ns),
                                    observation.snapshot.facts.as_ref(),
                                    Some(&runtime),
                                );
                            })?;
                        if let HspRuntimeTick::TailReached(ref tail) = outcome {
                            let armed = (|| {
                                let delay = runtime
                                    .finite_dispatch_delay_ns()
                                    .ok_or(HspDispatchFailure::InvalidBudget)?;
                                self.session.stop_send.arm(
                                    tail.reached_at_ns()
                                        .checked_add(delay)
                                        .ok_or(HspDispatchFailure::InvalidClock)?,
                                )
                            })();
                            if let Err(error) = armed {
                                self.record_first_stop(
                                    HspFirstStopReason::WatchdogTailBudget,
                                    Some(error.clone()),
                                    Some(observation.sampled_at_ns),
                                    observation.snapshot.facts.as_ref(),
                                    Some(&runtime),
                                );
                                return Err(error);
                            }
                            self.record_first_stop(
                                HspFirstStopReason::FiniteTailReached,
                                None,
                                Some(observation.sampled_at_ns),
                                observation.snapshot.facts.as_ref(),
                                Some(&runtime),
                            );
                        }
                        Ok(outcome)
                    });
                (Some(checked), false)
            }
            Err(TryLockError::WouldBlock) => (None, false),
            Err(TryLockError::Poisoned(_)) => (Some(Err(HspDispatchFailure::Busy)), true),
        };
        if poisoned {
            self.stop_blocked = true;
            self.record_first_stop(
                HspFirstStopReason::WatchdogRuntimePoisoned,
                Some(HspDispatchFailure::Busy),
                Some(now_ns),
                Some(&facts),
                None,
            );
            self.latch();
            return false;
        }
        match checked {
            Some(outcome) => {
                self.watchdog_blocked_since_ns = None;
                let valid = outcome.is_ok();
                if !matches!(outcome, Ok(HspRuntimeTick::Continue)) {
                    self.latch();
                }
                #[cfg(test)]
                if valid {
                    self.session.watchdog_successful_ticks.store(
                        self.session
                            .watchdog_successful_ticks
                            .load(Ordering::Acquire)
                            .saturating_add(1),
                        Ordering::Release,
                    );
                }
                valid
            }
            None => {
                // Same existing blocked observation; record its actual error
                // without renewing the previous facts or contention interval.
                let blocked_ns = match native_now() {
                    Ok(blocked_ns) => {
                        if let Err(error) = facts.valid_at(blocked_ns) {
                            self.record_first_stop(
                                HspFirstStopReason::WatchdogBlockedFacts,
                                Some(HspDispatchFailure::Authority(error)),
                                Some(blocked_ns),
                                Some(&facts),
                                None,
                            );
                            self.latch();
                            return false;
                        }
                        blocked_ns
                    }
                    Err(error) => {
                        self.record_first_stop(
                            HspFirstStopReason::WatchdogBlockedClock,
                            Some(error),
                            None,
                            Some(&facts),
                            None,
                        );
                        self.latch();
                        return false;
                    }
                };
                #[cfg(test)]
                self.session.watchdog_busy_ticks.store(
                    self.session
                        .watchdog_busy_ticks
                        .load(Ordering::Acquire)
                        .saturating_add(1),
                    Ordering::Release,
                );
                let since = *self.watchdog_blocked_since_ns.get_or_insert(blocked_ns);
                if blocked_ns
                    .checked_sub(since)
                    .is_none_or(|elapsed| elapsed >= WATCHDOG_CONTENTION_NS)
                {
                    #[cfg(test)]
                    self.session
                        .watchdog_contention_retired
                        .store(true, Ordering::Release);
                    self.record_first_stop(
                        HspFirstStopReason::WatchdogContentionLimit,
                        None,
                        Some(blocked_ns),
                        Some(&facts),
                        None,
                    );
                    self.latch();
                }
                false
            }
        }
    }
    fn publish(&self) {
        let runtime = self.session.runtime.try_lock().ok();
        let Ok(mut snapshot) = self.session.snapshot.try_lock() else {
            return;
        };
        if let Some(runtime) = runtime {
            snapshot.runtime_state = runtime.state();
            snapshot.current_effect = runtime
                .in_flight()
                .map(|effect| (effect.sequence(), effect.kind()));
            snapshot.pending_points = runtime.stream().pending_points();
            snapshot.retained_points = runtime.stream().retained_points();
            snapshot.buffered_points = runtime.stream().buffered_points();
            snapshot.total_added = runtime.stream().total_added();
            snapshot.finite_drain = runtime.finite_seal().map(|seal| {
                let terminal = seal.terminal();
                let (deadline, sent, missed) = self.session.stop_send.status();
                HandyHspFiniteDrainStatus {
                    input_state: match runtime.stream().input_state() {
                        HspInputState::Draining => HandyHspInputState::Draining,
                        HspInputState::TailReached => HandyHspInputState::TailReached,
                        _ => HandyHspInputState::Sealed,
                    },
                    terminal_time_ms: terminal.point.time_ms,
                    terminal_position_percent: terminal.point.position_percent,
                    terminal_source_sequence: terminal.source_sequence,
                    captured_at_ns: terminal.captured_at_ns,
                    accepted_at_ns: terminal.accepted_at_ns,
                    closed_at_boottime_ns: seal.closed_at_ns(),
                    expires_at_boottime_ns: seal.expires_at_ns(),
                    tail_reached_at_boottime_ns: runtime
                        .finite_tail()
                        .map(|tail| tail.reached_at_ns()),
                    stop_dispatch_deadline_boottime_ns: deadline,
                    stop_dispatched_at_boottime_ns: sent,
                    stop_deadline_missed: missed,
                    aborted: runtime.finite_aborted(),
                }
            });
            snapshot.stop_pending = matches!(
                runtime.state(),
                HspRuntimeState::StopRequired | HspRuntimeState::StopInFlight
            );
        }
        // These owner observations remain publishable during runtime contention;
        // counters/state may lag, but retirement must not disappear behind it.
        snapshot.accepted_samples = self.session.accepted.load(Ordering::Acquire);
        snapshot.last_joined = self.last_joined.clone();
        snapshot.metadata_pending = self.metadata_pending();
        snapshot.stop_blocked = self.stop_blocked;
        snapshot.emergency_observed = self
            .emergency
            .as_ref()
            .is_some_and(|c| c.observation().is_some());
        snapshot.post_setup_bound = self.post_setup_bound;
        snapshot.emergency_history_durable = self.emergency_recorded;
        // Active means an owned command/probe worker, never physical motion or
        // the liveness of the entire Engine-owned pump.
        snapshot.native_active = self.active;
        snapshot.retired = self.retired;
    }
    fn step(&mut self, journal: &Arc<Mutex<DeviceEffectJournal>>, shutdown: bool) {
        if self.released
            || self
                .frozen_recovery
                .is_some_and(|token| token == self.session.recovery_token.load(Ordering::Acquire))
        {
            self.publish();
            return;
        }
        self.frozen_recovery = None;
        let step_sample = match native_now() {
            Ok(at) => {
                self.session.stop_send.observe(at);
                Some(at)
            }
            Err(_) => None,
        };
        self.collect_metadata(false);
        if self
            .metadata_worker
            .as_ref()
            .is_some_and(|active| active.first_attempt.elapsed() >= DISPATCH_BUDGET)
            || self
                .metadata_retry
                .as_ref()
                .is_some_and(|retry| retry.first_attempt.elapsed() >= DISPATCH_BUDGET)
        {
            self.record_first_stop(
                HspFirstStopReason::MetadataDeadline,
                None,
                step_sample,
                None,
                None,
            );
            self.latch();
        }
        if self.dispatcher.is_none() {
            self.dispatcher = Some(HandyHspDispatcher::new(
                self.session.runtime.clone(),
                journal.clone(),
                self.transport.take().expect("fresh transport"),
            ));
        }
        if self.joined.is_none() {
            if let Some(joined) = self
                .dispatcher
                .as_mut()
                .expect("initialized dispatcher")
                .poll_any()
            {
                self.active = false;
                self.joined = Some(joined);
            }
        }
        self.observe_joined();
        if shutdown {
            self.record_first_stop(
                HspFirstStopReason::ShutdownObserved,
                None,
                step_sample,
                None,
                None,
            );
            self.latch();
        } else if self.session.cancelled.load(Ordering::Acquire) {
            self.record_first_stop(
                HspFirstStopReason::CancellationObserved,
                None,
                step_sample,
                None,
                None,
            );
            self.latch();
        }
        // A clock read or source transfer must never execute while the runtime
        // mutex is held. Retired owners may only collect already-finished work.
        if let Err(error) = self.session.authority.poll_owned_work() {
            self.record_first_stop(
                HspFirstStopReason::OwnedWorkPoll,
                Some(error),
                step_sample,
                None,
                None,
            );
            self.latch();
        }
        // Run the independent watchdog even while a worker or metadata is blocked.
        let watchdog_checked = if !self.retired {
            self.watchdog()
        } else {
            if let Ok(mut runtime) = self.session.runtime.try_lock() {
                runtime.stop();
            }
            false
        };
        if self.retired && !self.active && self.joined.is_none() {
            self.retire_pending();
        }
        self.persist_metadata(journal);
        if (!self.retired && !watchdog_checked) || self.active || self.joined.is_some() {
            self.publish();
            return;
        }
        if let Some(setup) = &self.post_setup {
            if !self.probe_failed {
                match self
                    .dispatcher
                    .as_mut()
                    .expect("initialized dispatcher")
                    .start_post_setup_probe(
                        setup,
                        &self.session.owner,
                        Instant::now() + DISPATCH_BUDGET,
                    ) {
                    Ok(()) => {
                        self.active = true;
                        self.active_ordinary = false;
                    }
                    Err(error) => {
                        self.record_first_stop(
                            HspFirstStopReason::PostSetupProbeStart,
                            Some(error),
                            step_sample,
                            None,
                            None,
                        );
                        self.probe_failed = true;
                        self.stop_blocked = true;
                        self.latch();
                    }
                }
            }
            self.publish();
            return;
        }
        if self.pending.is_none() && !self.stop_blocked {
            let next = self.session.runtime.try_lock().ok().map(|mut runtime| {
                if runtime.is_finite_draining() {
                    return Ok(None);
                }
                observe_for_settlement(self.session.authority.as_ref())
                    .inspect_err(|error| {
                        self.record_first_stop(
                            HspFirstStopReason::NextEffectObservation,
                            Some(error.clone()),
                            None,
                            None,
                            Some(&runtime),
                        );
                    })
                    .and_then(|observation| {
                        runtime
                            .next_effect(
                                observation.sampled_at_ns,
                                observation.snapshot.facts.as_ref(),
                            )
                            .map_err(HspDispatchFailure::Authority)
                            .inspect_err(|error| {
                                self.record_first_stop(
                                    HspFirstStopReason::NextEffectRuntime,
                                    Some(error.clone()),
                                    Some(observation.sampled_at_ns),
                                    observation.snapshot.facts.as_ref(),
                                    Some(&runtime),
                                )
                            })
                    })
            });
            match next {
                Some(Ok(effect)) => self.pending = effect,
                Some(Err(_)) => self.latch(),
                None => {}
            }
        }
        // Ordinary continuation waits for exact metadata. Stop never does.
        if self
            .pending
            .as_ref()
            .is_some_and(|e| e.kind() == HspEffectKind::Stop)
            || (!self.retired && self.metadata_task().is_none())
        {
            self.start_pending(journal);
        }
        self.publish();
    }
    fn quiescent(&self) -> bool {
        !self.active
            && self.joined.is_none()
            && self.session.authority.owned_work_joined()
            && (self.stop_blocked
                || (self.pending.is_none()
                    && self.session.runtime.try_lock().ok().is_some_and(|r| {
                        matches!(
                            r.state(),
                            HspRuntimeState::StopAcknowledged
                                | HspRuntimeState::StopUnknown
                                | HspRuntimeState::StopUnclaimed
                                | HspRuntimeState::SetupCancelled
                                | HspRuntimeState::SetupUnsent
                        )
                    })))
    }
    fn recovery_ready(&self) -> bool {
        self.quiescent()
            && self.pending.is_none()
            && !self.metadata_pending()
            && self.metadata_task().is_none()
            && (self.post_setup.is_none() || self.probe_failed)
    }
    fn unresolved(&self) -> bool {
        if self.released {
            return false;
        }
        self.owned_join_failed
            || !self.session.authority.owned_work_joined()
            || self.metadata_pending()
            || self.stop_blocked
            || self.active
            || self.joined.is_some()
            || self.session.runtime.try_lock().ok().is_none_or(|r| {
                !matches!(
                    r.state(),
                    HspRuntimeState::StopAcknowledged
                        | HspRuntimeState::SetupCancelled
                        | HspRuntimeState::SetupUnsent
                )
            })
    }
    fn finish(&mut self, journal: &Arc<Mutex<DeviceEffectJournal>>) {
        self.record_first_stop(HspFirstStopReason::OwnerFinish, None, None, None, None);
        self.latch();
        if let Some(dispatcher) = &mut self.dispatcher {
            if let Some(joined) = dispatcher.shutdown() {
                self.active = false;
                self.joined = Some(joined);
            }
        }
        self.step(journal, true);
        // Final join can exceed the drain budget. Never detach an owned worker or
        // imply that a software drain budget proves a physical stopping bound.
        if let Some(dispatcher) = &mut self.dispatcher {
            if let Some(joined) = dispatcher.shutdown() {
                self.active = false;
                self.joined = Some(joined);
            }
        }
        self.observe_joined();
        if self.session.authority.join_owned_work().is_err() {
            self.owned_join_failed = true;
        }
        if self.collect_metadata(true) != Some(false) {
            for _ in 0..8 {
                self.metadata_retry_at = Instant::now();
                self.persist_metadata(journal);
                if self.metadata_worker.is_none() || self.collect_metadata(true) == Some(false) {
                    break;
                }
            }
        }
        self.publish();
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        self.session.authority.request_owned_stop();
        let _ = self.session.authority.join_owned_work();
        // Even unwinding cannot detach a storage worker. It has no Engine and
        // no transport authority; joining never requires its held DB mutex.
        if let Some(active) = self.metadata_worker.take() {
            let _ = active.worker.join();
            #[cfg(test)]
            if let Some(pause) = active.pause {
                pause.joined.store(true, Ordering::Release);
            }
        }
    }
}
fn service_recovery(
    active: &mut Option<RecoveryRequest>,
    work: &mut [Work],
    registry: &Arc<Mutex<Registry>>,
    journal: &Arc<Mutex<DeviceEffectJournal>>,
    shutdown: bool,
) {
    let Some(request) = active.as_ref() else {
        return;
    };
    let finish = |active: &mut Option<RecoveryRequest>, result| {
        if let Some(request) = active.take() {
            let _ = request.reply.try_send(result);
        }
    };
    if shutdown || request.abandoned.load(Ordering::Acquire) || Instant::now() >= request.deadline {
        finish(active, Err(HspExecutionError::Unavailable));
        return;
    }
    let targets = match &request.action {
        RecoveryAction::Drain(targets) => targets,
        RecoveryAction::Finish(proof) => &proof.targets,
    };
    let valid = match registry.try_lock() {
        Ok(registry) => recovery_valid(&registry, targets),
        Err(_) => return,
    };
    if !valid {
        finish(active, Err(HspExecutionError::InvalidAdmission));
        return;
    }
    match &request.action {
        RecoveryAction::Drain(targets) => {
            let mut memberships = Vec::new();
            for session in &targets.sessions {
                let Some(current) = work
                    .iter_mut()
                    .find(|current| Arc::ptr_eq(&current.session, session))
                else {
                    return;
                };
                current.latch();
                if !current.recovery_ready() {
                    return;
                }
                if let Some((token, membership)) = &current.recovery_membership {
                    if *token == targets.token {
                        memberships.push(membership.clone());
                        continue;
                    }
                }
                if current.metadata_worker.is_none() {
                    let stream = match current.record.id() {
                        Ok(stream) => stream,
                        Err(_) => {
                            finish(active, Err(HspExecutionError::InvalidAdmission));
                            return;
                        }
                    };
                    current.spawn_metadata(
                        MetadataTask::Membership {
                            stream,
                            token: targets.token,
                        },
                        journal,
                    );
                }
                return;
            }
            for session in &targets.sessions {
                if let Some(current) = work
                    .iter_mut()
                    .find(|current| Arc::ptr_eq(&current.session, session))
                {
                    current.frozen_recovery = Some(targets.token);
                }
            }
            let request = active.take().expect("checked request");
            let RecoveryAction::Drain(targets) = request.action else {
                unreachable!()
            };
            let _ = request
                .reply
                .try_send(Ok(RecoveryReply::Drained(Box::new(HspDrainedProof {
                    targets,
                    memberships,
                }))));
        }
        RecoveryAction::Finish(proof) => {
            for (session, member) in proof.targets.sessions.iter().zip(&proof.memberships) {
                let Some(current) = work
                    .iter()
                    .find(|current| Arc::ptr_eq(&current.session, session))
                else {
                    finish(active, Err(HspExecutionError::InvalidAdmission));
                    return;
                };
                if !current.recovery_ready()
                    || current.frozen_recovery != Some(proof.targets.token)
                    || current
                        .recovery_membership
                        .as_ref()
                        .is_none_or(|(token, current)| {
                            *token != proof.targets.token || current != member
                        })
                {
                    finish(active, Err(HspExecutionError::InvalidAdmission));
                    return;
                }
            }
            let Ok(mut registry) = registry.try_lock() else {
                return;
            };
            if !recovery_valid(&registry, &proof.targets) {
                drop(registry);
                finish(active, Err(HspExecutionError::InvalidAdmission));
                return;
            }
            if request.abandoned.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                drop(registry);
                finish(active, Err(HspExecutionError::Unavailable));
                return;
            }
            for session in &proof.targets.sessions {
                if let Some(current) = work
                    .iter_mut()
                    .find(|current| Arc::ptr_eq(&current.session, session))
                {
                    current.session.owner.retire_owner();
                    current.session.released.store(true, Ordering::Release);
                    current.released = true;
                    current.publish();
                }
            }
            registry
                .fences
                .retain(|fence| fence.token != proof.targets.token);
            drop(registry);
            finish(active, Ok(RecoveryReply::Finished));
        }
    }
}

/// Preserve settlement and Stop progress after source refusal using the one
/// successful clock callback from that attempted observation. `None` authorizes
/// no ordinary command. A failed clock is never retried or replaced here.
fn observe_for_settlement(
    authority: &dyn HspNativeAuthority,
) -> Result<HspAuthorityEvaluation, HspDispatchFailure> {
    observe_for_settlement_with_clock(authority, &mut native_now)
}

fn observe_for_settlement_with_clock(
    authority: &dyn HspNativeAuthority,
    clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
) -> Result<HspAuthorityEvaluation, HspDispatchFailure> {
    let mut clock_called = false;
    let mut clock_fault = None;
    let mut sampled_at_ns = None;
    let observation = authority.observe_current(&mut || {
        // Reject the second invocation before it can take another native sample.
        if clock_called {
            clock_fault = Some(HspDispatchFailure::InvalidClock);
            return Err(HspDispatchFailure::InvalidClock);
        }
        clock_called = true;
        match clock() {
            Ok(at) => {
                sampled_at_ns = Some(at);
                Ok(at)
            }
            Err(error) => {
                clock_fault = Some(error.clone());
                Err(error)
            }
        }
    });
    // A source cannot hide a failed/repeated clock or manufacture its own tick.
    // Only a source refusal after one successful sample may settle with no facts.
    if let Some(error) = clock_fault {
        return Err(error);
    }
    let sampled_at_ns = sampled_at_ns.ok_or(HspDispatchFailure::InvalidClock)?;
    match observation {
        Ok(observation) if observation.sampled_at_ns == sampled_at_ns => Ok(observation),
        Ok(_) => Err(HspDispatchFailure::InvalidClock),
        Err(_) => Ok(HspAuthorityEvaluation {
            sampled_at_ns,
            snapshot: HspAuthoritySnapshot {
                facts: None,
                projected: None,
            },
        }),
    }
}

fn native_now() -> Result<u64, HspDispatchFailure> {
    crate::handy_clock::boottime_ns().map_err(|_| HspDispatchFailure::InvalidClock)
}
fn now() -> Result<u64, HspExecutionError> {
    native_now().map_err(Into::into)
}

pub(crate) struct HspExecutionPump {
    handle: HspExecutionHandle,
    worker: Option<JoinHandle<Vec<Work>>>,
    retained: Vec<Work>,
    failed: bool,
}
impl HspExecutionPump {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn start(
        journal: Arc<Mutex<DeviceEffectJournal>>,
        shutdown_requested: Arc<AtomicBool>,
    ) -> Result<Self, HspExecutionError> {
        let (incoming, commands) = mpsc::sync_channel::<Work>(MAILBOX_CAPACITY);
        let (recovery, recovery_requests) = mpsc::sync_channel::<RecoveryRequest>(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let registry = Arc::new(Mutex::new(Registry::default()));
        let stop = stopped.clone();
        let engine_stop = shutdown_requested.clone();
        let worker_registry = registry.clone();
        let worker = thread::Builder::new()
            .name("pulsar-hsp-owner".into())
            .spawn(move || {
                let mut work = Vec::with_capacity(MAX_SESSIONS);
                let mut deadline = None;
                let mut recovery_request = None;
                loop {
                    while let Ok(next) = commands.try_recv() {
                        work.push(next);
                    }
                    let shutdown =
                        stop.load(Ordering::Acquire) || engine_stop.load(Ordering::Acquire);
                    if shutdown && deadline.is_none() {
                        deadline = Some(Instant::now() + DRAIN_BUDGET);
                    }
                    if shutdown {
                        stop.store(true, Ordering::Release);
                        // Close admission while draining the mailbox. A caller
                        // that passed its first flag check cannot enqueue work
                        // after the owner decides all sessions are quiescent.
                        let Ok(_admission_guard) = worker_registry.try_lock() else {
                            thread::park_timeout(TICK);
                            continue;
                        };
                        while let Ok(next) = commands.try_recv() {
                            work.push(next);
                        }
                    }
                    for session in &mut work {
                        session.step(&journal, shutdown);
                    }
                    if recovery_request.is_none() {
                        recovery_request = recovery_requests.try_recv().ok();
                    }
                    service_recovery(
                        &mut recovery_request,
                        &mut work,
                        &worker_registry,
                        &journal,
                        shutdown,
                    );
                    if deadline
                        .is_some_and(|d| work.iter().all(Work::quiescent) || Instant::now() >= d)
                    {
                        break;
                    }
                    thread::park_timeout(TICK);
                }
                if let Some(request) = recovery_request.take() {
                    let _ = request.reply.try_send(Err(HspExecutionError::Unavailable));
                }
                while let Ok(request) = recovery_requests.try_recv() {
                    let _ = request.reply.try_send(Err(HspExecutionError::Unavailable));
                }
                for session in &mut work {
                    session.finish(&journal);
                }
                work
            })
            .map_err(|_| HspExecutionError::Unavailable)?;
        let handle = HspExecutionHandle {
            registry,
            incoming,
            recovery,
            stopped,
            shutdown_requested,
            wake: worker.thread().clone(),
        };
        Ok(Self {
            handle,
            worker: Some(worker),
            retained: Vec::new(),
            failed: false,
        })
    }
    pub(crate) fn is_running(&self) -> bool {
        !self.handle.stopped.load(Ordering::Acquire)
            && !self.handle.shutdown_requested.load(Ordering::Acquire)
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
    }
    pub(crate) fn handle(&self) -> HspExecutionHandle {
        self.handle.clone()
    }
    pub(crate) fn request_shutdown(&self) {
        self.handle.retire_all();
    }
    pub(crate) fn shutdown(&mut self) -> Result<(), HspExecutionError> {
        self.request_shutdown();
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(work) => {
                    self.failed = work.iter().any(Work::unresolved);
                    self.retained = work;
                }
                Err(_) => {
                    self.failed = true;
                }
            }
        }
        if self.failed {
            Err(HspExecutionError::Unresolved)
        } else {
            Ok(())
        }
    }
}
impl Drop for HspExecutionPump {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod fresh_settlement_tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Behavior {
        Accept,
        Refuse,
        SkipClock,
        RepeatClock,
        MismatchTick,
        SwallowClockFailure,
    }
    struct SettlementAuthority(Behavior);
    impl HspNativeAuthority for SettlementAuthority {
        fn observe_current(
            &self,
            clock: &mut dyn FnMut() -> Result<u64, HspDispatchFailure>,
        ) -> Result<HspAuthorityEvaluation, HspDispatchFailure> {
            if matches!(self.0, Behavior::SkipClock) {
                return Err(HspDispatchFailure::BindingMismatch);
            }
            let sampled_at_ns = match clock() {
                Ok(at) => at,
                Err(_) if matches!(self.0, Behavior::SwallowClockFailure) => 99,
                Err(_) if matches!(self.0, Behavior::Refuse) => {
                    return Err(HspDispatchFailure::BindingMismatch);
                }
                Err(error) => return Err(error),
            };
            match self.0 {
                Behavior::Refuse => return Err(HspDispatchFailure::BindingMismatch),
                Behavior::RepeatClock => {
                    assert!(matches!(clock(), Err(HspDispatchFailure::InvalidClock)));
                }
                _ => {}
            }
            Ok(HspAuthorityEvaluation {
                sampled_at_ns: if matches!(self.0, Behavior::MismatchTick) {
                    sampled_at_ns + 1
                } else {
                    sampled_at_ns
                },
                snapshot: HspAuthoritySnapshot {
                    facts: None,
                    projected: None,
                },
            })
        }
        fn current_facts(&self, _: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            panic!("settlement must use the selected observation API")
        }
        fn authorize(
            &self,
            _: Instant,
            _: &AtomicBool,
            _: HspClaimGate<'_>,
            _: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            panic!("this fixture performs no final claim")
        }
    }

    #[test]
    fn settlement_retains_one_original_sample_for_success_or_source_refusal() {
        for behavior in [Behavior::Accept, Behavior::Refuse] {
            let mut samples = 0;
            let evaluation =
                observe_for_settlement_with_clock(&SettlementAuthority(behavior), &mut || {
                    samples += 1;
                    Ok(123)
                })
                .unwrap();
            assert_eq!(samples, 1);
            assert_eq!(evaluation.sampled_at_ns, 123);
            assert!(evaluation.snapshot.facts.is_none());
            assert!(evaluation.snapshot.projected.is_none());
        }
    }

    #[test]
    fn settlement_rejects_missing_repeated_or_mismatched_sample() {
        for (behavior, expected_samples) in [
            (Behavior::SkipClock, 0),
            (Behavior::RepeatClock, 1),
            (Behavior::MismatchTick, 1),
        ] {
            let mut samples = 0;
            let evaluation =
                observe_for_settlement_with_clock(&SettlementAuthority(behavior), &mut || {
                    samples += 1;
                    Ok(123)
                });
            assert_eq!(samples, expected_samples);
            assert!(matches!(evaluation, Err(HspDispatchFailure::InvalidClock)));
        }
    }

    #[test]
    fn settlement_cannot_conceal_a_failed_clock_as_source_refusal_or_success() {
        for behavior in [Behavior::Refuse, Behavior::SwallowClockFailure] {
            let mut samples = 0;
            let evaluation =
                observe_for_settlement_with_clock(&SettlementAuthority(behavior), &mut || {
                    samples += 1;
                    Err(HspDispatchFailure::Cancelled)
                });
            assert_eq!(samples, 1);
            assert!(matches!(evaluation, Err(HspDispatchFailure::Cancelled)));
        }
    }
}
