//! Bounded generated HSP append custody. The Engine supplies native sample ages,
//! an independent playback-phase interval, and qualified policy. This module
//! neither derives device time from capture receipts nor establishes authority,
//! transport delivery, point-content identity on the device, or physical motion.
use crate::{AttemptId, LiveIdentity};
use std::{collections::VecDeque, sync::Arc};

/// Canonical HSP percent coordinates; no conversion to a 50 or 255 scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspPoint {
    pub time_ms: u32,
    pub position_percent: u8,
}
impl HspPoint {
    pub fn new(time_ms: u32, position_percent: u8) -> Result<Self, HspStreamError> {
        let value = Self {
            time_ms,
            position_percent,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(self) -> Result<(), HspStreamError> {
        if self.position_percent > 100 {
            return Err(HspStreamError::InvalidPoint);
        }
        Ok(())
    }

    /// An exact checked boundary for numeric producers, never a quantizer.
    pub fn try_from_f64(time_ms: f64, position_percent: f64) -> Result<Self, HspStreamError> {
        if !time_ms.is_finite() || !position_percent.is_finite() {
            return Err(HspStreamError::NonFinite);
        }
        if !(0.0..=f64::from(u32::MAX)).contains(&time_ms)
            || !(0.0..=100.0).contains(&position_percent)
            || time_ms.fract() != 0.0
            || position_percent.fract() != 0.0
        {
            return Err(HspStreamError::InvalidPoint);
        }
        Self::new(time_ms as u32, position_percent as u8)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspStreamIdentity {
    /// The owner must never reuse this generation for another stream kernel.
    pub generation: u64,
    pub stream_id: u32,
    pub live: LiveIdentity,
}

/// Original capture and inference attempt behind one epoch's local sequences.
/// This descriptor alone conveys no native source or playback authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspSourceCursor {
    pub identity: LiveIdentity,
    pub attempt_id: AttemptId,
}

/// The Engine must supply this only after validating a native transfer witness
/// for the exact real boundary pair and joined predecessor work/delivery. Core
/// checks bounded stream continuity; it cannot authenticate pixels or callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspSourceTransitionSpec {
    pub predecessor: HspSourceCursor,
    pub successor: HspSourceCursor,
    pub predecessor_tail: HspSample,
    pub first_successor: HspSample,
    pub expires_at_ns: u64,
}

/// Single-use custody of this kernel's one prepared source barrier. Dropping
/// the token does not reopen input; the watchdog still enforces its deadline.
#[must_use = "commit the exact source barrier or stop its original stream"]
pub struct HspSourceTransition {
    data: Arc<HspSourceTransitionSpec>,
}
impl HspSourceTransition {
    pub fn predecessor(&self) -> &HspSourceCursor {
        &self.data.predecessor
    }
    pub fn successor(&self) -> &HspSourceCursor {
        &self.data.successor
    }
    pub fn predecessor_tail(&self) -> HspSample {
        self.data.predecessor_tail
    }
    pub fn first_successor(&self) -> HspSample {
        self.data.first_successor
    }
    pub fn expires_at_ns(&self) -> u64 {
        self.data.expires_at_ns
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspPolicy {
    /// Includes pending, in-flight, and conservatively retained device points.
    pub max_buffered_points: usize,
    pub device_capacity_points: usize,
    pub max_chunk_points: usize,
    pub min_lead_ms: u32,
    pub max_future_ms: u32,
    pub max_sample_age_ns: u64,
    pub max_gap_ms: u32,
}
impl HspPolicy {
    pub fn validate(self) -> Result<(), HspStreamError> {
        if !(2..=4096).contains(&self.max_buffered_points)
            || !(2..=4096).contains(&self.device_capacity_points)
            || self.device_capacity_points > self.max_buffered_points
            || !(2..=100).contains(&self.max_chunk_points)
            || self.max_chunk_points > self.device_capacity_points
            || self.min_lead_ms == 0
            || self.max_future_ms < self.min_lead_ms
            || self.max_sample_age_ns == 0
            || self.max_gap_ms == 0
        {
            return Err(HspStreamError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Bounds on device script phase at the SAME native `now_ns` instant. Negative
/// phase supports a scheduled start before script time zero. Neither field is
/// the remote ring's approximate `current_point`, a receipt tick, or an RTT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspPhaseWindow {
    pub earliest_ms: i64,
    pub latest_ms: i64,
}
impl HspPhaseWindow {
    fn validate(self) -> Result<(), HspStreamError> {
        if self.earliest_ms > self.latest_ms
            || self.earliest_ms < i64::from(i32::MIN)
            || self.latest_ms > i64::from(i32::MAX)
        {
            return Err(HspStreamError::InvalidPhase);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspSample {
    pub point: HspPoint,
    /// Nonzero, strictly increasing original analysis sequence. Analyses may
    /// have been skipped; this number alone never establishes source coverage.
    pub source_sequence: u64,
    /// Original sample capture and native acceptance instants, in one monotonic
    /// domain with `now_ns`. Neither is a device script timestamp.
    pub captured_at_ns: u64,
    pub accepted_at_ns: u64,
    /// Engine-checked original source-pair coverage for this edge, never inferred
    /// from small spacing or analysis-sequence adjacency.
    /// Ignored on the first offered point; required on every subsequent point.
    pub continuous_from_previous: bool,
}

/// Qualified limits for draining only the original acknowledged finite input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspFiniteDrainPolicy {
    pub maximum_duration_ns: u64,
    pub maximum_terminal_overrun_ns: u64,
    /// Includes both dispatch delay and the qualified physical stopping bound.
    pub stop_budget_ns: u64,
}
impl HspFiniteDrainPolicy {
    pub fn validate(self) -> Result<(), HspStreamError> {
        if self.maximum_duration_ns == 0
            || self.maximum_terminal_overrun_ns == 0
            || self.stop_budget_ns == 0
            || self.stop_budget_ns > self.maximum_terminal_overrun_ns
        {
            return Err(HspStreamError::InvalidPolicy);
        }
        Ok(())
    }
}

/// Both intervals come from the same retained native observations. The owner
/// projects them outward, with qualified drift, through the full Stop budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HspFiniteDrainWindow {
    pub at_ns: u64,
    pub phase: HspPhaseWindow,
    pub stop_at_ns: u64,
    pub stop_phase: HspPhaseWindow,
}

/// Input lifetime is independent of whether the owner has started playback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspInputState {
    Open,
    Sealed,
    Draining,
    TailReached,
}

/// An immutable capability for this particular kernel's exact original tail.
/// It has no public or serialized constructor, and cannot reopen the input.
#[derive(Clone, Debug)]
pub struct HspInputSeal {
    owner: Arc<()>,
    identity: HspStreamIdentity,
    source: Option<Arc<HspSourceCursor>>,
    terminal: HspSample,
    closed_at_ns: u64,
    expires_at_ns: u64,
    policy: HspFiniteDrainPolicy,
}
impl PartialEq for HspInputSeal {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
            && self.identity == other.identity
            && self.source == other.source
            && self.terminal == other.terminal
            && self.closed_at_ns == other.closed_at_ns
            && self.expires_at_ns == other.expires_at_ns
            && self.policy == other.policy
    }
}
impl Eq for HspInputSeal {}
impl HspInputSeal {
    pub fn identity(&self) -> &HspStreamIdentity {
        &self.identity
    }
    pub fn source_cursor(&self) -> Option<&HspSourceCursor> {
        self.source.as_deref()
    }
    pub fn terminal(&self) -> HspSample {
        self.terminal
    }
    pub fn closed_at_ns(&self) -> u64 {
        self.closed_at_ns
    }
    pub fn expires_at_ns(&self) -> u64 {
        self.expires_at_ns
    }
    pub fn policy(&self) -> HspFiniteDrainPolicy {
        self.policy
    }
}

/// A phase-passage observation requiring immediate owned Stop. This is neither
/// a recovery drained proof nor evidence of physical execution or stopping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspTailObservation {
    seal: HspInputSeal,
    reached_at_ns: u64,
    window: HspFiniteDrainWindow,
}
impl HspTailObservation {
    pub fn seal(&self) -> &HspInputSeal {
        &self.seal
    }
    pub fn terminal(&self) -> HspSample {
        self.seal.terminal()
    }
    pub fn reached_at_ns(&self) -> u64 {
        self.reached_at_ns
    }
    pub fn window(&self) -> HspFiniteDrainWindow {
        self.window
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HspDrainProgress {
    Draining,
    TailReached(Box<HspTailObservation>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HspStreamError {
    #[error("invalid bounded HSP policy")]
    InvalidPolicy,
    #[error("HSP generation and device stream identity must be nonzero")]
    InvalidIdentity,
    #[error("HSP points require exact integer milliseconds and percent in 0..=100")]
    InvalidPoint,
    #[error("HSP point contains a nonfinite number")]
    NonFinite,
    #[error("invalid signed HSP phase uncertainty window")]
    InvalidPhase,
    #[error("native time or independent playback phase regressed")]
    ClockRegression,
    #[error("sample belongs to another live source identity")]
    ForeignSource,
    #[error("one bounded source transition is already pending")]
    SourceTransitionPending,
    #[error("source transition does not name this exact pending kernel barrier")]
    ForeignSourceTransition,
    #[error("original source transition deadline expired")]
    SourceTransitionExpired,
    #[error("sample capture/acceptance native instants are invalid or regress")]
    InvalidSampleTime,
    #[error("point time or source sequence is not strictly increasing")]
    SourceOrder,
    #[error("missing source coverage or interpolation gap exceeds policy")]
    CoverageGap,
    #[error("original sample exceeds maximum native age")]
    StaleSample,
    #[error("point has insufficient lead at latest possible playback phase")]
    InsufficientLead,
    #[error("point exceeds horizon at earliest possible playback phase")]
    FutureHorizon,
    #[error("bounded local queue or conservative device capacity is full")]
    Capacity,
    #[error("one immutable append is already in flight")]
    InFlight,
    #[error("acknowledged continuous initial coverage is not ready")]
    NotReady,
    #[error("running acknowledged coverage is exhausted or cannot meet lead")]
    Starved,
    #[error("cumulative zero-based HSP tail exceeds the signed RPC index bound")]
    IndexOverflow,
    #[error("local chunk sequence is exhausted")]
    ChunkSequenceOverflow,
    #[error("completion token does not name the exact current append")]
    ForeignCompletion,
    #[error("this append already has a completion observation")]
    AlreadyCompleted,
    #[error("append delivery is unknown; stop is required and retry is forbidden")]
    UnknownAppend,
    #[error("append was rejected; stop is required")]
    RejectedAppend,
    #[error("append observation has a different stream or cumulative tail")]
    AcknowledgmentMismatch,
    #[error("finite HSP input is sealed and cannot accept another point")]
    InputSealed,
    #[error("finite input seal does not name the exact last original sample")]
    InvalidTerminal,
    #[error("finite input seal does not belong to this exact stream closure")]
    ForeignSeal,
    #[error("finite drain requires a coherent current and projected Stop window")]
    InvalidDrainWindow,
    #[error("finite drain requires its guarded time and phase operation")]
    DrainWindowRequired,
    #[error("original finite input close deadline has expired")]
    FiniteDrainExpired,
    #[error("projected Stop exceeds the qualified terminal overrun")]
    TerminalOverrun,
    #[error("finite drain time or bound arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("HSP queue is frozen and cannot reopen")]
    Frozen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspStreamState {
    Priming,
    Running,
    StopRequired(HspStreamError),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspOfferOutcome {
    Accepted,
    /// No queued point was overwritten or retimed. A later coverage gap is
    /// still refused; a running stream instead freezes on a stale sample.
    DroppedStale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspChunkToken {
    identity: HspStreamIdentity,
    sequence: u64,
    tail_point_stream_index: u32,
}
impl HspChunkToken {
    pub fn identity(&self) -> &HspStreamIdentity {
        &self.identity
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn tail_point_stream_index(&self) -> u32 {
        self.tail_point_stream_index
    }
}

/// Immutable fields, shared with the caller without copying the bounded points.
#[derive(Debug, PartialEq, Eq)]
pub struct HspChunk {
    token: HspChunkToken,
    samples: Vec<HspSample>,
    source: Option<Arc<HspSourceCursor>>,
}
impl HspChunk {
    pub fn token(&self) -> &HspChunkToken {
        &self.token
    }
    pub fn samples(&self) -> &[HspSample] {
        &self.samples
    }
    pub fn source_cursor(&self) -> Option<&HspSourceCursor> {
        self.source.as_deref()
    }
    pub fn stream_id(&self) -> u32 {
        self.token.identity.stream_id
    }
    pub fn tail_point_stream_index(&self) -> u32 {
        self.token.tail_point_stream_index
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspAppendOutcome {
    /// A transport observation only. Matching counters do not prove exact
    /// point content, physical execution, stopping, or human acceptance.
    Acknowledged {
        stream_id: u32,
        tail_point_stream_index: u32,
    },
    Unknown,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspCompletionDisposition {
    Advanced,
    /// The exact claimed append and observation remain in terminal custody.
    Retained,
}

struct AppendCustody {
    chunk: Arc<HspChunk>,
    outcome: Option<HspAppendOutcome>,
}

struct RetainedSample {
    sample: HspSample,
    source: Option<Arc<HspSourceCursor>>,
}

/// One continuous generation; no pause/resume, automatic holds, seek, sorting,
/// rescaling, retries, or recovery of an uncertain append is provided.
pub struct HspStream {
    identity: HspStreamIdentity,
    policy: HspPolicy,
    state: HspStreamState,
    pending: VecDeque<HspSample>,
    retained: VecDeque<RetainedSample>,
    in_flight: Option<AppendCustody>,
    last_offered: Option<HspSample>,
    last_context: Option<(u64, HspPhaseWindow)>,
    // Past passage remains known when a fresh valid interval is less precise.
    // Keep it separate so no original phase or command reference is rewritten.
    certified_passage_ms: Option<i64>,
    total_added: u64,
    chunk_sequence: u64,
    input_owner: Arc<()>,
    input_state: HspInputState,
    input_seal: Option<HspInputSeal>,
    last_drain_window: Option<HspFiniteDrainWindow>,
    // Knowledge at the last drain window's future Stop target, never at now.
    certified_stop_passage_ms: Option<i64>,
    tail_observation: Option<Box<HspTailObservation>>,
    active_source: Option<Arc<HspSourceCursor>>,
    source_transition: Option<Arc<HspSourceTransitionSpec>>,
}
impl HspStream {
    pub fn new(identity: HspStreamIdentity, policy: HspPolicy) -> Result<Self, HspStreamError> {
        policy.validate()?;
        if identity.generation == 0 || identity.stream_id == 0 {
            return Err(HspStreamError::InvalidIdentity);
        }
        Ok(Self {
            identity,
            policy,
            state: HspStreamState::Priming,
            pending: VecDeque::new(),
            retained: VecDeque::new(),
            in_flight: None,
            last_offered: None,
            last_context: None,
            certified_passage_ms: None,
            total_added: 0,
            chunk_sequence: 0,
            input_owner: Arc::new(()),
            input_state: HspInputState::Open,
            input_seal: None,
            last_drain_window: None,
            certified_stop_passage_ms: None,
            tail_observation: None,
            active_source: None,
            source_transition: None,
        })
    }

    pub fn new_with_source(
        identity: HspStreamIdentity,
        policy: HspPolicy,
        source: HspSourceCursor,
    ) -> Result<Self, HspStreamError> {
        if source.identity != identity.live {
            return Err(HspStreamError::ForeignSource);
        }
        let mut stream = Self::new(identity, policy)?;
        stream.active_source = Some(Arc::new(source));
        Ok(stream)
    }

    pub fn active_source(&self) -> Option<&HspSourceCursor> {
        self.active_source.as_deref()
    }
    pub fn source_transition_pending(&self) -> bool {
        self.source_transition.is_some()
    }
    pub fn owns_source_transition(&self, transition: &HspSourceTransition) -> bool {
        self.source_transition
            .as_ref()
            .is_some_and(|pending| Arc::ptr_eq(pending, &transition.data))
    }
    fn active_identity(&self) -> &LiveIdentity {
        self.active_source()
            .map_or(&self.identity.live, |source| &source.identity)
    }

    pub fn identity(&self) -> &HspStreamIdentity {
        &self.identity
    }
    pub fn state(&self) -> HspStreamState {
        self.state
    }
    pub fn input_state(&self) -> HspInputState {
        self.input_state
    }
    pub fn pending_points(&self) -> usize {
        self.pending.len()
    }
    pub fn retained_points(&self) -> usize {
        self.retained.len()
    }
    pub fn buffered_points(&self) -> usize {
        self.pending.len()
            + self.retained.len()
            + self.in_flight.as_ref().map_or(0, |c| c.chunk.samples.len())
            + usize::from(self.source_transition.is_some())
    }
    pub fn total_added(&self) -> u64 {
        self.total_added
    }
    pub fn in_flight(&self) -> Option<&HspChunk> {
        self.in_flight.as_ref().map(|c| c.chunk.as_ref())
    }
    pub fn in_flight_outcome(&self) -> Option<HspAppendOutcome> {
        self.in_flight.as_ref().and_then(|c| c.outcome)
    }

    fn ensure_open(&self) -> Result<(), HspStreamError> {
        match self.state {
            HspStreamState::Priming | HspStreamState::Running => Ok(()),
            _ => Err(HspStreamError::Frozen),
        }
    }

    fn stop(&mut self, error: HspStreamError) -> HspStreamError {
        if matches!(
            self.state,
            HspStreamState::Priming | HspStreamState::Running
        ) {
            self.state = HspStreamState::StopRequired(error);
        }
        error
    }

    fn running_failure(&mut self, error: HspStreamError) -> HspStreamError {
        if self.state == HspStreamState::Running {
            self.stop(error)
        } else {
            error
        }
    }

    fn sample_timing(
        &self,
        sample: HspSample,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<(), HspStreamError> {
        sample.point.validate()?;
        if sample.source_sequence == 0 {
            return Err(HspStreamError::SourceOrder);
        }
        if sample.captured_at_ns > sample.accepted_at_ns || sample.accepted_at_ns > now_ns {
            return Err(HspStreamError::InvalidSampleTime);
        }
        if now_ns - sample.captured_at_ns > self.policy.max_sample_age_ns {
            return Err(HspStreamError::StaleSample);
        }
        let time = i64::from(sample.point.time_ms);
        if time - phase.latest_ms < i64::from(self.policy.min_lead_ms) {
            return Err(HspStreamError::InsufficientLead);
        }
        if time - phase.earliest_ms > i64::from(self.policy.max_future_ms) {
            return Err(HspStreamError::FutureHorizon);
        }
        Ok(())
    }

    /// Advance only from Engine-provided bounds. Contradictory passage or a
    /// regressing native clock requires stop while running: deleted ring slots
    /// cannot be recovered. A wider interval does not undo certified passage.
    fn advance_context(
        &mut self,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<(), HspStreamError> {
        self.ensure_open()?;
        if matches!(
            self.input_state,
            HspInputState::Draining | HspInputState::TailReached
        ) {
            return Err(self.stop(HspStreamError::DrainWindowRequired));
        }
        self.check_context(now_ns, phase).map_err(|e| {
            if self.input_seal.is_some() {
                self.stop(e)
            } else {
                self.running_failure(e)
            }
        })?;
        self.retain_context(now_ns, phase);
        if self.state == HspStreamState::Running {
            let Some(tail_time) = self.retained.back().map(|p| p.sample.point.time_ms) else {
                return Err(self.stop(HspStreamError::Starved));
            };
            if i64::from(tail_time) <= phase.latest_ms + i64::from(self.policy.min_lead_ms) {
                return Err(self.stop(HspStreamError::Starved));
            }
            self.release_passed_points();
        }
        Ok(())
    }

    fn check_context(&self, now_ns: u64, phase: HspPhaseWindow) -> Result<(), HspStreamError> {
        phase.validate()?;
        // A fresh interval may widen or tighten. It must remain compatible
        // with passage already certified for this same forward-only stream.
        if self
            .last_context
            .is_some_and(|(old_now, _)| now_ns < old_now)
            || self
                .certified_passage_ms
                .is_some_and(|passed| phase.latest_ms < passed)
        {
            return Err(HspStreamError::ClockRegression);
        }
        if self
            .input_seal
            .as_ref()
            .is_some_and(|seal| now_ns >= seal.expires_at_ns)
        {
            return Err(HspStreamError::FiniteDrainExpired);
        }
        if let Some(transition) = &self.source_transition {
            if now_ns >= transition.expires_at_ns {
                return Err(HspStreamError::SourceTransitionExpired);
            }
            if now_ns
                .checked_sub(transition.first_successor.captured_at_ns)
                .is_none_or(|age| age > self.policy.max_sample_age_ns)
            {
                return Err(HspStreamError::StaleSample);
            }
        }
        Ok(())
    }

    fn retain_context(&mut self, now_ns: u64, phase: HspPhaseWindow) {
        self.last_context = Some((now_ns, phase));
        self.certified_passage_ms = Some(
            self.certified_passage_ms
                .map_or(phase.earliest_ms, |passed| passed.max(phase.earliest_ms)),
        );
    }

    fn release_passed_points(&mut self) {
        let Some(passed) = self.certified_passage_ms else {
            return;
        };
        while self.retained.len() > 1 && i64::from(self.retained[1].sample.point.time_ms) < passed {
            self.retained.pop_front();
        }
    }

    fn source_boundary_ready(&self, spec: &HspSourceTransitionSpec) -> bool {
        self.state == HspStreamState::Running
            && self.total_added >= 2
            && self.pending.is_empty()
            && self.in_flight.is_none()
            && self.last_offered == Some(spec.predecessor_tail)
            && self.retained.back().is_some_and(|retained| {
                retained.sample == spec.predecessor_tail && retained.source == self.active_source
            })
    }

    fn validate_source_boundary(
        &self,
        spec: &HspSourceTransitionSpec,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<(), HspStreamError> {
        if now_ns >= spec.expires_at_ns {
            return Err(HspStreamError::SourceTransitionExpired);
        }
        self.sample_timing(spec.first_successor, now_ns, phase)?;
        if spec.first_successor.point.time_ms <= spec.predecessor_tail.point.time_ms {
            return Err(HspStreamError::SourceOrder);
        }
        if spec.first_successor.captured_at_ns < spec.predecessor_tail.captured_at_ns
            || spec.first_successor.accepted_at_ns < spec.predecessor_tail.accepted_at_ns
        {
            return Err(HspStreamError::InvalidSampleTime);
        }
        if spec.first_successor.point.time_ms - spec.predecessor_tail.point.time_ms
            > self.policy.max_gap_ms
        {
            return Err(HspStreamError::CoverageGap);
        }
        // The accepted real cross-epoch pair is checked by the Engine. Neither
        // its original local sequence nor its local continuity flag is changed.
        Ok(())
    }

    pub fn prepare_source_transition(
        &mut self,
        spec: HspSourceTransitionSpec,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<HspSourceTransition, HspStreamError> {
        if self.active_source() != Some(&spec.predecessor)
            || spec.successor.identity.project != spec.predecessor.identity.project
            || spec.successor.identity.revision != spec.predecessor.identity.revision
            || spec.successor.identity.source != spec.predecessor.identity.source
            || spec.successor.identity.epoch == spec.predecessor.identity.epoch
            || spec.successor.identity.epoch == self.identity.live.epoch
            || spec.successor.attempt_id == spec.predecessor.attempt_id
        {
            return Err(HspStreamError::ForeignSource);
        }
        self.ensure_open()?;
        if self.input_state != HspInputState::Open {
            return Err(HspStreamError::InputSealed);
        }
        if self.source_transition.is_some() {
            return Err(HspStreamError::SourceTransitionPending);
        }
        self.advance_context(now_ns, phase)?;
        if !self.source_boundary_ready(&spec) {
            return Err(HspStreamError::NotReady);
        }
        self.validate_source_boundary(&spec, now_ns, phase)
            .map_err(|error| self.stop(error))?;
        if self.buffered_points() >= self.policy.max_buffered_points {
            return Err(HspStreamError::Capacity);
        }
        let data = Arc::new(spec);
        self.source_transition = Some(data.clone());
        Ok(HspSourceTransition { data })
    }

    pub fn commit_source_transition(
        &mut self,
        transition: HspSourceTransition,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<(), HspStreamError> {
        // An old or foreign token cannot advance the current stream's context.
        if !self.owns_source_transition(&transition) {
            return Err(HspStreamError::ForeignSourceTransition);
        }
        self.ensure_open()?;
        self.advance_context(now_ns, phase)?;
        if self.input_state != HspInputState::Open
            || self.active_source() != Some(&transition.data.predecessor)
            || !self.source_boundary_ready(&transition.data)
        {
            return Err(self.stop(HspStreamError::NotReady));
        }
        self.validate_source_boundary(&transition.data, now_ns, phase)
            .map_err(|error| self.stop(error))?;
        // Preparation reserved this one point's local capacity. Everything
        // fallible precedes the cursor switch and unchanged first-point offer.
        self.active_source = Some(Arc::new(transition.data.successor.clone()));
        self.pending.push_back(transition.data.first_successor);
        self.last_offered = Some(transition.data.first_successor);
        self.source_transition = None;
        Ok(())
    }

    pub fn offer(
        &mut self,
        identity: &LiveIdentity,
        sample: HspSample,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<HspOfferOutcome, HspStreamError> {
        // A foreign producer cannot mutate a newer stream's clock or stop it.
        if identity != self.active_identity() {
            return Err(HspStreamError::ForeignSource);
        }
        self.ensure_open()?;
        if self.input_state != HspInputState::Open {
            return Err(HspStreamError::InputSealed);
        }
        if self.source_transition.is_some() {
            return Err(HspStreamError::SourceTransitionPending);
        }
        self.advance_context(now_ns, phase)?;
        if let Err(error) = self.sample_timing(sample, now_ns, phase) {
            if error == HspStreamError::StaleSample && self.state == HspStreamState::Priming {
                return Ok(HspOfferOutcome::DroppedStale);
            }
            return Err(self.running_failure(error));
        }
        if let Some(previous) = self.last_offered {
            if sample.point.time_ms <= previous.point.time_ms
                || sample.source_sequence <= previous.source_sequence
            {
                return Err(self.running_failure(HspStreamError::SourceOrder));
            }
            if sample.captured_at_ns < previous.captured_at_ns
                || sample.accepted_at_ns < previous.accepted_at_ns
            {
                return Err(self.running_failure(HspStreamError::InvalidSampleTime));
            }
            if !sample.continuous_from_previous
                || sample.point.time_ms - previous.point.time_ms > self.policy.max_gap_ms
            {
                return Err(self.stop(HspStreamError::CoverageGap));
            }
        }
        if self.buffered_points() >= self.policy.max_buffered_points {
            return Err(HspStreamError::Capacity);
        }
        self.pending.push_back(sample);
        self.last_offered = Some(sample);
        Ok(HspOfferOutcome::Accepted)
    }

    /// Claim once, at actual dispatch time. Every point is rechecked against
    /// source age and BOTH uncertainty bounds; pending offers cannot replace it.
    pub fn claim(
        &mut self,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<Option<Arc<HspChunk>>, HspStreamError> {
        self.advance_context(now_ns, phase)?;
        if self.source_transition.is_some() {
            return Ok(None);
        }
        if self.in_flight.is_some() {
            return Err(HspStreamError::InFlight);
        }
        let needed = if self.total_added == 0 { 2 } else { 1 };
        if self.pending.len() < needed {
            return Ok(None);
        }
        let available = self
            .policy
            .device_capacity_points
            .saturating_sub(self.retained.len());
        let count = self
            .pending
            .len()
            .min(self.policy.max_chunk_points)
            .min(available);
        if count < needed {
            return Err(HspStreamError::Capacity);
        }
        for index in 0..count {
            if let Err(error) = self.sample_timing(self.pending[index], now_ns, phase) {
                return Err(self.stop(error));
            }
        }
        let Some(total) = self.total_added.checked_add(count as u64) else {
            return Err(self.stop(HspStreamError::IndexOverflow));
        };
        if total - 1 > i32::MAX as u64 {
            return Err(self.stop(HspStreamError::IndexOverflow));
        }
        let Some(sequence) = self.chunk_sequence.checked_add(1) else {
            return Err(self.stop(HspStreamError::ChunkSequenceOverflow));
        };
        let chunk = Arc::new(HspChunk {
            token: HspChunkToken {
                identity: self.identity.clone(),
                sequence,
                tail_point_stream_index: (total - 1) as u32,
            },
            samples: self.pending.drain(..count).collect(),
            source: self.active_source.clone(),
        });
        self.chunk_sequence = sequence;
        self.in_flight = Some(AppendCustody {
            chunk: Arc::clone(&chunk),
            outcome: None,
        });
        Ok(Some(chunk))
    }

    pub fn complete(
        &mut self,
        token: &HspChunkToken,
        outcome: HspAppendOutcome,
    ) -> Result<HspCompletionDisposition, HspStreamError> {
        self.complete_observed(token, outcome, false)
    }

    /// Observe an append at its native completion instant. Check the OLD
    /// acknowledged coverage first: a late final append cannot rescue a stream
    /// that already starved. Even a failed check retains the exact observation.
    pub fn complete_at(
        &mut self,
        token: &HspChunkToken,
        outcome: HspAppendOutcome,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<HspCompletionDisposition, HspStreamError> {
        self.check_completion(token)?;
        let context = self.advance_context(now_ns, phase);
        if let Err(error) = context {
            self.stop(error);
        }
        let completion = self.complete_observed(token, outcome, true);
        context?;
        completion
    }

    fn check_completion(&self, token: &HspChunkToken) -> Result<(), HspStreamError> {
        let Some(custody) = self.in_flight.as_ref() else {
            return Err(HspStreamError::ForeignCompletion);
        };
        if token != custody.chunk.token() {
            return Err(HspStreamError::ForeignCompletion);
        }
        if custody.outcome.is_some() {
            return Err(HspStreamError::AlreadyCompleted);
        }
        Ok(())
    }

    fn complete_observed(
        &mut self,
        token: &HspChunkToken,
        outcome: HspAppendOutcome,
        context_checked: bool,
    ) -> Result<HspCompletionDisposition, HspStreamError> {
        self.check_completion(token)?;
        self.in_flight.as_mut().expect("checked custody").outcome = Some(outcome);
        match outcome {
            HspAppendOutcome::Unknown => {
                self.stop(HspStreamError::UnknownAppend);
            }
            HspAppendOutcome::Rejected => {
                self.stop(HspStreamError::RejectedAppend);
            }
            HspAppendOutcome::Acknowledged {
                stream_id,
                tail_point_stream_index,
            } => {
                if stream_id != self.identity.stream_id
                    || tail_point_stream_index != token.tail_point_stream_index
                {
                    self.stop(HspStreamError::AcknowledgmentMismatch);
                    return Err(HspStreamError::AcknowledgmentMismatch);
                }
                if matches!(
                    self.state,
                    HspStreamState::Priming | HspStreamState::Running
                ) {
                    if self.input_seal.is_some() && !context_checked {
                        return Err(self.stop(HspStreamError::DrainWindowRequired));
                    }
                    let custody = self.in_flight.take().expect("checked custody");
                    self.total_added = u64::from(tail_point_stream_index) + 1;
                    self.retained
                        .extend(custody.chunk.samples.iter().map(|sample| RetainedSample {
                            sample: *sample,
                            source: custody.chunk.source.clone(),
                        }));
                    return Ok(HspCompletionDisposition::Advanced);
                }
            }
        }
        Ok(HspCompletionDisposition::Retained)
    }

    /// Close the original input exactly once. This creates no point, append,
    /// start authority, clock, or renewed source lifetime. An identical retry
    /// returns the same seal and keeps its original absolute deadline.
    pub fn seal_input(
        &mut self,
        identity: &LiveIdentity,
        terminal: HspSample,
        closed_at_ns: u64,
        policy: HspFiniteDrainPolicy,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<HspInputSeal, HspStreamError> {
        if identity != self.active_identity() {
            return Err(HspStreamError::ForeignSource);
        }
        self.ensure_open()?;
        if self.source_transition.is_some() {
            return Err(HspStreamError::SourceTransitionPending);
        }
        if let Some(seal) = &self.input_seal {
            if terminal != seal.terminal
                || closed_at_ns != seal.closed_at_ns
                || policy != seal.policy
            {
                return Err(HspStreamError::ForeignSeal);
            }
            let seal = seal.clone();
            self.check_context(now_ns, phase)
                .map_err(|e| self.stop(e))?;
            // A retry is not a watchdog observation, and cannot move either the
            // phase context or a previously latched TailReached instant.
            return Ok(seal);
        }
        policy.validate()?;
        if self.last_offered != Some(terminal) {
            return Err(HspStreamError::InvalidTerminal);
        }
        // The last CPU result may be accepted after ingress closes, while the
        // original final work joins. That must not move the input-close clock.
        if terminal.captured_at_ns > closed_at_ns
            || terminal.accepted_at_ns > now_ns
            || closed_at_ns > now_ns
        {
            return Err(self.running_failure(HspStreamError::InvalidSampleTime));
        }
        let expires_at_ns = closed_at_ns
            .checked_add(policy.maximum_duration_ns)
            .ok_or_else(|| self.running_failure(HspStreamError::ArithmeticOverflow))?;
        if now_ns >= expires_at_ns {
            return Err(self.running_failure(HspStreamError::FiniteDrainExpired));
        }
        self.advance_context(now_ns, phase)?;
        let seal = HspInputSeal {
            owner: Arc::clone(&self.input_owner),
            identity: self.identity.clone(),
            source: self.active_source.clone(),
            terminal,
            closed_at_ns,
            expires_at_ns,
            policy,
        };
        self.input_state = HspInputState::Sealed;
        self.input_seal = Some(seal.clone());
        Ok(seal)
    }

    fn check_seal(&self, seal: &HspInputSeal) -> Result<(), HspStreamError> {
        if self.input_seal.as_ref() != Some(seal) {
            return Err(HspStreamError::ForeignSeal);
        }
        Ok(())
    }

    fn terminal_acknowledged(&self, seal: &HspInputSeal) -> bool {
        self.total_added >= 2
            && self.pending.is_empty()
            && self.in_flight.is_none()
            && self.last_offered == Some(seal.terminal)
            && self.retained.back().is_some_and(|retained| {
                retained.sample == seal.terminal && retained.source == seal.source
            })
    }

    fn check_drain_window(
        &self,
        seal: &HspInputSeal,
        now_ns: u64,
        window: HspFiniteDrainWindow,
    ) -> Result<(), HspStreamError> {
        self.check_context(now_ns, window.phase)?;
        window.stop_phase.validate()?;
        let stop_at_ns = now_ns
            .checked_add(seal.policy.stop_budget_ns)
            .ok_or(HspStreamError::ArithmeticOverflow)?;
        if window.at_ns != now_ns
            || window.stop_at_ns != stop_at_ns
            || window.stop_phase.earliest_ms < window.phase.earliest_ms
            || window.stop_phase.latest_ms < window.phase.latest_ms
        {
            return Err(HspStreamError::InvalidDrainWindow);
        }
        if self
            .last_drain_window
            .is_some_and(|old| window.stop_at_ns < old.stop_at_ns)
            || self
                .certified_stop_passage_ms
                .is_some_and(|passed| window.stop_phase.latest_ms < passed)
        {
            return Err(HspStreamError::ClockRegression);
        }
        let overrun_ms = window
            .stop_phase
            .latest_ms
            .checked_sub(i64::from(seal.terminal.point.time_ms))
            .ok_or(HspStreamError::ArithmeticOverflow)?
            .max(0) as u64;
        let overrun_ns = overrun_ms
            .checked_mul(1_000_000)
            .ok_or(HspStreamError::ArithmeticOverflow)?;
        if overrun_ns > seal.policy.maximum_terminal_overrun_ns {
            return Err(HspStreamError::TerminalOverrun);
        }
        Ok(())
    }

    fn retain_drain_window(&mut self, window: HspFiniteDrainWindow) {
        self.certified_stop_passage_ms = Some(
            self.certified_stop_passage_ms
                .map_or(window.stop_phase.earliest_ms, |passed| {
                    passed.max(window.stop_phase.earliest_ms)
                }),
        );
        self.last_drain_window = Some(window);
    }

    /// The owner may call only AFTER the actual Play acknowledgment and after
    /// its original native writer/probe work has joined. Core Running is not
    /// that authority. The exact terminal append must already be acknowledged,
    /// and the previous acknowledged coverage must still satisfy ordinary lead.
    pub fn begin_finite_drain(
        &mut self,
        seal: &HspInputSeal,
        now_ns: u64,
        window: HspFiniteDrainWindow,
    ) -> Result<HspDrainProgress, HspStreamError> {
        self.check_seal(seal)?;
        self.ensure_open()?;
        if self.input_state != HspInputState::Sealed || self.state != HspStreamState::Running {
            return Err(HspStreamError::NotReady);
        }
        self.check_drain_window(seal, now_ns, window)
            .map_err(|e| self.stop(e))?;
        self.advance_context(now_ns, window.phase)?;
        if !self.terminal_acknowledged(seal) {
            return Err(HspStreamError::NotReady);
        }
        self.input_state = HspInputState::Draining;
        self.retain_drain_window(window);
        Ok(HspDrainProgress::Draining)
    }

    /// Guard every bounded watchdog tick using the original seal and native
    /// phase projection. Expiry, regression, and Stop overrun take precedence
    /// over terminal passage. TailReached must immediately latch owned Stop.
    pub fn tick_finite_drain(
        &mut self,
        seal: &HspInputSeal,
        now_ns: u64,
        window: HspFiniteDrainWindow,
    ) -> Result<HspDrainProgress, HspStreamError> {
        self.check_seal(seal)?;
        self.ensure_open()?;
        if !matches!(
            self.input_state,
            HspInputState::Draining | HspInputState::TailReached
        ) || self.state != HspStreamState::Running
        {
            return Err(HspStreamError::NotReady);
        }
        self.check_drain_window(seal, now_ns, window)
            .map_err(|e| self.stop(e))?;
        if !self.terminal_acknowledged(seal) {
            return Err(self.stop(HspStreamError::NotReady));
        }
        self.retain_context(now_ns, window.phase);
        self.retain_drain_window(window);
        self.release_passed_points();
        if let Some(observation) = &self.tail_observation {
            return Ok(HspDrainProgress::TailReached(observation.clone()));
        }
        if self
            .certified_passage_ms
            .is_some_and(|passed| passed >= i64::from(seal.terminal.point.time_ms))
        {
            let observation = Box::new(HspTailObservation {
                seal: seal.clone(),
                reached_at_ns: now_ns,
                window,
            });
            self.tail_observation = Some(observation.clone());
            self.input_state = HspInputState::TailReached;
            Ok(HspDrainProgress::TailReached(observation))
        } else {
            Ok(HspDrainProgress::Draining)
        }
    }

    /// Engine calls after its own start authority checks. Requires acknowledged
    /// coverage across the current phase, or a pre-zero scheduled start with
    /// an exact first point at zero. This is not an observation of physical play.
    pub fn mark_running(
        &mut self,
        now_ns: u64,
        phase: HspPhaseWindow,
    ) -> Result<(), HspStreamError> {
        self.ensure_open()?;
        if self.state != HspStreamState::Priming {
            return Err(HspStreamError::NotReady);
        }
        self.advance_context(now_ns, phase)?;
        if self.retained.len() < 2 {
            return Err(HspStreamError::NotReady);
        }
        let first = i64::from(
            self.retained
                .front()
                .expect("two retained points")
                .sample
                .point
                .time_ms,
        );
        if first > phase.earliest_ms && !(first == 0 && phase.latest_ms < 0) {
            return Err(HspStreamError::NotReady);
        }
        self.state = HspStreamState::Running;
        self.advance_context(now_ns, phase)
    }

    /// The Engine must call this on its bounded watchdog cadence, including
    /// while generation or an append is pending. Pending points are not coverage.
    pub fn tick(&mut self, now_ns: u64, phase: HspPhaseWindow) -> Result<(), HspStreamError> {
        self.advance_context(now_ns, phase)
    }

    /// Retains any claimed append for exactly one late completion observation.
    /// No operation can reopen this generation, even after an acknowledgment.
    pub fn cancel(&mut self) {
        self.state = HspStreamState::Cancelled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttemptId, ProjectId, RevisionId, SourceVersionId};

    fn identity() -> HspStreamIdentity {
        HspStreamIdentity {
            generation: 17,
            stream_id: 23,
            live: LiveIdentity {
                epoch: AttemptId::new("live-1").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(4),
                source: SourceVersionId::new("capture").unwrap(),
            },
        }
    }
    fn policy() -> HspPolicy {
        HspPolicy {
            max_buffered_points: 16,
            device_capacity_points: 8,
            max_chunk_points: 3,
            min_lead_ms: 10,
            max_future_ms: 1000,
            max_sample_age_ns: 1_000_000,
            max_gap_ms: 100,
        }
    }
    fn phase(earliest_ms: i64, latest_ms: i64) -> HspPhaseWindow {
        HspPhaseWindow {
            earliest_ms,
            latest_ms,
        }
    }
    fn prefill() -> HspPhaseWindow {
        phase(-100, -50)
    }
    fn sample(sequence: u64, time_ms: u32) -> HspSample {
        HspSample {
            point: HspPoint {
                time_ms,
                position_percent: if sequence.is_multiple_of(2) { 0 } else { 100 },
            },
            source_sequence: sequence + 1,
            captured_at_ns: 1000 + sequence,
            accepted_at_ns: 2000 + sequence,
            continuous_from_previous: true,
        }
    }
    fn stream() -> HspStream {
        HspStream::new(identity(), policy()).unwrap()
    }
    fn offer(stream: &mut HspStream, sequence: u64, time_ms: u32, phase: HspPhaseWindow) {
        assert_eq!(
            stream.offer(&identity().live, sample(sequence, time_ms), 10_000, phase),
            Ok(HspOfferOutcome::Accepted)
        );
    }
    fn acknowledge(stream: &mut HspStream, chunk: &HspChunk) {
        assert_eq!(
            stream.complete(
                chunk.token(),
                HspAppendOutcome::Acknowledged {
                    stream_id: chunk.stream_id(),
                    tail_point_stream_index: chunk.tail_point_stream_index(),
                }
            ),
            Ok(HspCompletionDisposition::Advanced)
        );
    }
    fn running() -> HspStream {
        let mut stream = stream();
        for index in 0..3 {
            offer(&mut stream, index, index as u32 * 100, prefill());
        }
        let chunk = stream.claim(10_000, prefill()).unwrap().unwrap();
        acknowledge(&mut stream, &chunk);
        stream.mark_running(10_000, phase(-20, -10)).unwrap();
        stream
    }

    #[test]
    fn continuous_chunks_keep_original_samples_and_cumulative_zero_based_tail() {
        let mut stream = stream();
        offer(&mut stream, 0, 0, prefill());
        assert_eq!(stream.claim(10_000, prefill()).unwrap(), None);
        offer(&mut stream, 1, 100, prefill());
        offer(&mut stream, 2, 200, prefill());
        let first = stream.claim(10_000, prefill()).unwrap().unwrap();
        assert_eq!(first.token().sequence(), 1);
        assert_eq!(first.tail_point_stream_index(), 2);
        assert_eq!(
            first.samples(),
            &[sample(0, 0), sample(1, 100), sample(2, 200)]
        );
        assert_eq!(stream.total_added(), 0);
        offer(&mut stream, 3, 300, prefill());
        assert_eq!(
            stream.claim(10_000, prefill()),
            Err(HspStreamError::InFlight)
        );
        assert!(std::ptr::eq(first.as_ref(), stream.in_flight().unwrap()));
        acknowledge(&mut stream, &first);
        let second = stream.claim(10_000, prefill()).unwrap().unwrap();
        assert_eq!(second.token().sequence(), 2);
        assert_eq!(second.samples(), &[sample(3, 300)]);
        assert_eq!(second.tail_point_stream_index(), 3);
        acknowledge(&mut stream, &second);
        assert_eq!(stream.total_added(), 4);
        assert_eq!(stream.retained_points(), 4);
        stream.mark_running(10_000, phase(-20, -10)).unwrap();
        offer(&mut stream, 4, 400, phase(0, 10));
        let third = stream.claim(10_000, phase(0, 10)).unwrap().unwrap();
        assert_eq!(third.tail_point_stream_index(), 4);
        acknowledge(&mut stream, &third);
        assert_eq!(stream.total_added(), 5);
    }

    #[test]
    fn actual_claim_rechecks_latest_lead_earliest_horizon_and_native_age() {
        let mut late = stream();
        offer(&mut late, 0, 0, prefill());
        offer(&mut late, 1, 100, prefill());
        assert_eq!(
            late.claim(10_001, phase(-20, -9)),
            Err(HspStreamError::InsufficientLead)
        );
        assert_eq!(
            late.state(),
            HspStreamState::StopRequired(HspStreamError::InsufficientLead)
        );
        assert_eq!(late.pending_points(), 2);
        assert!(late.in_flight().is_none());

        let mut stale = stream();
        offer(&mut stale, 0, 0, prefill());
        offer(&mut stale, 1, 100, prefill());
        assert_eq!(
            stale.claim(1_001_001, prefill()),
            Err(HspStreamError::StaleSample)
        );
        assert_eq!(stale.total_added(), 0);

        let mut uncertain = stream();
        assert_eq!(
            uncertain.offer(&identity().live, sample(0, 901), 10_000, prefill()),
            Err(HspStreamError::FutureHorizon)
        );
        assert_eq!(
            uncertain.offer(&identity().live, sample(0, 900), 10_000, prefill()),
            Ok(HspOfferOutcome::Accepted)
        );
        assert_eq!(
            uncertain.offer(&identity().live, sample(1, 950), 10_000, phase(-50, -20)),
            Ok(HspOfferOutcome::Accepted)
        );
        let chunk = uncertain.claim(10_000, phase(-50, -20)).unwrap().unwrap();
        assert_eq!(chunk.samples()[1].point.time_ms, 950);
    }

    #[test]
    fn pressure_preserves_claim_and_only_independent_earliest_releases_ring_slots() {
        let mut rules = policy();
        rules.max_buffered_points = 4;
        rules.device_capacity_points = 4;
        rules.max_chunk_points = 2;
        let mut stream = HspStream::new(identity(), rules).unwrap();
        for index in 0..4 {
            offer(&mut stream, index, index as u32 * 100, prefill());
        }
        let first = stream.claim(10_000, prefill()).unwrap().unwrap();
        assert_eq!(stream.buffered_points(), 4);
        assert_eq!(
            stream.offer(&identity().live, sample(4, 400), 10_000, prefill()),
            Err(HspStreamError::Capacity)
        );
        assert_eq!(first.samples(), &[sample(0, 0), sample(1, 100)]);
        acknowledge(&mut stream, &first);
        let second = stream.claim(10_000, prefill()).unwrap().unwrap();
        acknowledge(&mut stream, &second);
        stream.mark_running(10_000, phase(-20, -10)).unwrap();
        stream.tick(10_001, phase(100, 200)).unwrap();
        assert_eq!(stream.retained_points(), 4); // exact lower bound retains predecessor
        stream.tick(10_002, phase(101, 200)).unwrap();
        assert_eq!(stream.retained_points(), 3); // upper bound does not release 100ms
        assert_eq!(stream.retained.front().unwrap().sample.point.time_ms, 100);
        assert_eq!(
            stream.offer(&identity().live, sample(4, 400), 10_002, phase(101, 200)),
            Ok(HspOfferOutcome::Accepted)
        );
        let third = stream.claim(10_002, phase(101, 200)).unwrap().unwrap();
        assert_eq!(third.samples(), &[sample(4, 400)]);
        assert_eq!(third.tail_point_stream_index(), 4);
        assert_eq!(stream.buffered_points(), 4);
    }

    #[test]
    fn capacity_cannot_count_queued_points_as_device_ring_space() {
        let mut rules = policy();
        rules.device_capacity_points = 3;
        let mut stream = HspStream::new(identity(), rules).unwrap();
        for index in 0..5 {
            offer(&mut stream, index, index as u32 * 100, prefill());
        }
        let first = stream.claim(10_000, prefill()).unwrap().unwrap();
        acknowledge(&mut stream, &first);
        assert_eq!(
            stream.claim(10_000, prefill()),
            Err(HspStreamError::Capacity)
        );
        assert_eq!(stream.pending_points(), 2);
        assert_eq!(stream.total_added(), 3);
    }

    #[test]
    fn explicit_gap_and_excess_spacing_require_stop() {
        for candidate in [
            HspSample {
                continuous_from_previous: false,
                ..sample(3, 300)
            },
            sample(3, 301),
        ] {
            let mut stream = running();
            assert_eq!(
                stream.offer(&identity().live, candidate, 10_000, phase(0, 10)),
                Err(HspStreamError::CoverageGap)
            );
            assert_eq!(
                stream.state(),
                HspStreamState::StopRequired(HspStreamError::CoverageGap)
            );
            assert_eq!(stream.pending_points(), 0);
            assert_eq!(
                stream.claim(10_000, phase(0, 10)),
                Err(HspStreamError::Frozen)
            );
        }
    }

    #[test]
    fn sequence_is_nonzero_and_increasing_but_continuity_is_independently_supplied() {
        let mut empty = stream();
        let zero_sequence = HspSample {
            source_sequence: 0,
            ..sample(0, 0)
        };
        assert_eq!(
            empty.offer(&identity().live, zero_sequence, 10_000, prefill()),
            Err(HspStreamError::SourceOrder)
        );
        assert_eq!(empty.buffered_points(), 0);

        let mut stream = running();
        let skipped_analyses = sample(8, 300);
        assert_eq!(
            stream.offer(&identity().live, skipped_analyses, 10_000, phase(0, 10)),
            Ok(HspOfferOutcome::Accepted)
        );
        assert_eq!(stream.pending.front(), Some(&skipped_analyses));
        let chunk = stream.claim(10_000, phase(0, 10)).unwrap().unwrap();
        assert_eq!(chunk.samples()[0].source_sequence, 9);
        assert_eq!(chunk.tail_point_stream_index(), 3);
        acknowledge(&mut stream, &chunk);
        assert_eq!(
            stream.offer(&identity().live, sample(8, 400), 10_000, phase(0, 10)),
            Err(HspStreamError::SourceOrder)
        );
        assert_eq!(
            stream.state(),
            HspStreamState::StopRequired(HspStreamError::SourceOrder)
        );
    }

    #[test]
    fn running_starvation_never_counts_pending_or_unknown_points_as_coverage() {
        let mut stream = running();
        offer(&mut stream, 3, 300, phase(0, 10));
        let chunk = stream.claim(10_000, phase(0, 10)).unwrap().unwrap();
        assert_eq!(
            stream.tick(10_001, phase(180, 190)),
            Err(HspStreamError::Starved)
        );
        assert_eq!(
            stream.state(),
            HspStreamState::StopRequired(HspStreamError::Starved)
        );
        assert_eq!(
            stream.complete(
                chunk.token(),
                HspAppendOutcome::Acknowledged {
                    stream_id: 23,
                    tail_point_stream_index: 3,
                }
            ),
            Ok(HspCompletionDisposition::Retained)
        );
        assert_eq!(stream.total_added(), 3);
        assert_eq!(stream.in_flight().unwrap(), chunk.as_ref());
        assert_eq!(
            stream.mark_running(10_001, phase(180, 190)),
            Err(HspStreamError::Frozen)
        );
    }

    #[test]
    fn unknown_append_retains_exact_custody_and_never_retries_or_advances_tail() {
        let mut stream = running();
        offer(&mut stream, 3, 300, phase(0, 10));
        let chunk = stream.claim(10_000, phase(0, 10)).unwrap().unwrap();
        assert_eq!(
            stream.complete(chunk.token(), HspAppendOutcome::Unknown),
            Ok(HspCompletionDisposition::Retained)
        );
        assert_eq!(
            stream.state(),
            HspStreamState::StopRequired(HspStreamError::UnknownAppend)
        );
        assert_eq!(stream.total_added(), 3);
        assert_eq!(stream.in_flight_outcome(), Some(HspAppendOutcome::Unknown));
        assert_eq!(stream.in_flight().unwrap().samples(), &[sample(3, 300)]);
        assert_eq!(
            stream.complete(
                chunk.token(),
                HspAppendOutcome::Acknowledged {
                    stream_id: 23,
                    tail_point_stream_index: 3,
                }
            ),
            Err(HspStreamError::AlreadyCompleted)
        );
        assert_eq!(
            stream.claim(10_001, phase(0, 10)),
            Err(HspStreamError::Frozen)
        );
    }

    #[test]
    fn cancellation_keeps_in_flight_outcome_custody_without_reopening() {
        for outcome in [
            HspAppendOutcome::Unknown,
            HspAppendOutcome::Rejected,
            HspAppendOutcome::Acknowledged {
                stream_id: 23,
                tail_point_stream_index: 1,
            },
        ] {
            let mut stream = stream();
            offer(&mut stream, 0, 0, prefill());
            offer(&mut stream, 1, 100, prefill());
            let chunk = stream.claim(10_000, prefill()).unwrap().unwrap();
            stream.cancel();
            assert_eq!(
                stream.complete(chunk.token(), outcome),
                Ok(HspCompletionDisposition::Retained)
            );
            assert_eq!(stream.state(), HspStreamState::Cancelled);
            assert_eq!(stream.total_added(), 0);
            assert_eq!(stream.in_flight_outcome(), Some(outcome));
            assert_eq!(stream.in_flight().unwrap().samples(), chunk.samples());
            assert_eq!(
                stream.complete(chunk.token(), outcome),
                Err(HspStreamError::AlreadyCompleted)
            );
            assert_eq!(stream.claim(10_000, prefill()), Err(HspStreamError::Frozen));
        }
    }

    #[test]
    fn completion_tokens_fence_old_chunks_foreign_generations_and_modified_fields() {
        let mut stream = stream();
        offer(&mut stream, 0, 0, prefill());
        offer(&mut stream, 1, 100, prefill());
        let first = stream.claim(10_000, prefill()).unwrap().unwrap();
        acknowledge(&mut stream, &first);
        offer(&mut stream, 2, 200, prefill());
        let second = stream.claim(10_000, prefill()).unwrap().unwrap();
        let mut foreign = second.token().clone();
        foreign.identity.generation += 1;
        let mut altered = second.token().clone();
        altered.tail_point_stream_index += 1;
        for token in [first.token(), &foreign, &altered] {
            assert_eq!(
                stream.complete(token, HspAppendOutcome::Unknown),
                Err(HspStreamError::ForeignCompletion)
            );
            assert_eq!(stream.state(), HspStreamState::Priming);
            assert_eq!(stream.in_flight_outcome(), None);
            assert_eq!(stream.total_added(), 2);
        }
        acknowledge(&mut stream, &second);
        assert_eq!(
            stream.complete(second.token(), HspAppendOutcome::Unknown),
            Err(HspStreamError::ForeignCompletion)
        );
        assert_eq!(stream.total_added(), 3);
    }

    #[test]
    fn mismatched_remote_tail_or_stream_is_terminal_and_retains_observation() {
        for outcome in [
            HspAppendOutcome::Acknowledged {
                stream_id: 24,
                tail_point_stream_index: 1,
            },
            HspAppendOutcome::Acknowledged {
                stream_id: 23,
                tail_point_stream_index: 2,
            },
        ] {
            let mut stream = stream();
            offer(&mut stream, 0, 0, prefill());
            offer(&mut stream, 1, 100, prefill());
            let chunk = stream.claim(10_000, prefill()).unwrap().unwrap();
            assert_eq!(
                stream.complete(chunk.token(), outcome),
                Err(HspStreamError::AcknowledgmentMismatch)
            );
            assert_eq!(stream.in_flight_outcome(), Some(outcome));
            assert_eq!(
                stream.state(),
                HspStreamState::StopRequired(HspStreamError::AcknowledgmentMismatch)
            );
            assert_eq!(stream.total_added(), 0);
        }
    }

    #[test]
    fn point_boundaries_are_exact_and_invalid_values_cannot_enter_queue() {
        assert_eq!(
            HspPoint::try_from_f64(0.0, 100.0).unwrap(),
            HspPoint {
                time_ms: 0,
                position_percent: 100
            }
        );
        assert_eq!(
            HspPoint::try_from_f64(f64::from(u32::MAX), 0.0)
                .unwrap()
                .time_ms,
            u32::MAX
        );
        for (t, x) in [(f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            assert_eq!(HspPoint::try_from_f64(t, x), Err(HspStreamError::NonFinite));
        }
        for (t, x) in [
            (-1.0, 0.0),
            (0.0, -1.0),
            (0.0, 101.0),
            (0.5, 50.0),
            (0.0, 50.5),
            (f64::from(u32::MAX) + 1.0, 0.0),
        ] {
            assert_eq!(
                HspPoint::try_from_f64(t, x),
                Err(HspStreamError::InvalidPoint)
            );
        }
        let mut stream = stream();
        let mut bad = sample(0, 0);
        bad.point.position_percent = 255;
        assert_eq!(
            stream.offer(&identity().live, bad, 10_000, prefill()),
            Err(HspStreamError::InvalidPoint)
        );
        assert_eq!(stream.buffered_points(), 0);
    }

    #[test]
    fn native_age_and_original_order_are_separate_from_device_script_time() {
        let mut stream = stream();
        let mut stale = sample(0, 0);
        stale.captured_at_ns = 0;
        assert_eq!(
            stream.offer(&identity().live, stale, 1_000_001, prefill()),
            Ok(HspOfferOutcome::DroppedStale)
        );
        assert_eq!(stream.pending_points(), 0);
        let future = HspSample {
            accepted_at_ns: 1_000_003,
            ..sample(0, 0)
        };
        assert_eq!(
            stream.offer(&identity().live, future, 1_000_002, prefill()),
            Err(HspStreamError::InvalidSampleTime)
        );
        let reversed = HspSample {
            captured_at_ns: 3000,
            accepted_at_ns: 2000,
            ..sample(0, 0)
        };
        assert_eq!(
            stream.offer(&identity().live, reversed, 1_000_002, prefill()),
            Err(HspStreamError::InvalidSampleTime)
        );
        let fresh = HspSample {
            captured_at_ns: 999_000,
            accepted_at_ns: 1_000_000,
            ..sample(0, 0)
        };
        assert_eq!(
            stream.offer(&identity().live, fresh, 1_000_002, prefill()),
            Ok(HspOfferOutcome::Accepted)
        );
        let repeated_time = HspSample {
            source_sequence: 1,
            ..fresh
        };
        assert_eq!(
            stream.offer(&identity().live, repeated_time, 1_000_002, prefill()),
            Err(HspStreamError::SourceOrder)
        );
        assert_eq!(stream.pending.front().unwrap().point.time_ms, 0);
    }

    #[test]
    fn stale_running_sample_requires_stop_and_foreign_source_has_no_effect() {
        let mut stream = running();
        let mut foreign = identity().live;
        foreign.revision = RevisionId::new(99);
        assert_eq!(
            stream.offer(&foreign, sample(3, 300), 9_000, phase(-50, -30)),
            Err(HspStreamError::ForeignSource)
        );
        assert_eq!(stream.state(), HspStreamState::Running);
        assert_eq!(
            stream.offer(&identity().live, sample(3, 300), 1_001_004, phase(0, 10)),
            Err(HspStreamError::StaleSample)
        );
        assert_eq!(
            stream.state(),
            HspStreamState::StopRequired(HspStreamError::StaleSample)
        );
    }

    #[test]
    fn cumulative_index_and_local_sequence_exhaustion_fail_before_claim() {
        let mut index = stream();
        offer(&mut index, 0, 0, prefill());
        index.total_added = i32::MAX as u64;
        let last = index.claim(10_000, prefill()).unwrap().unwrap();
        assert_eq!(last.tail_point_stream_index(), i32::MAX as u32);
        acknowledge(&mut index, &last);
        offer(&mut index, 1, 100, prefill());
        assert_eq!(
            index.claim(10_000, prefill()),
            Err(HspStreamError::IndexOverflow)
        );
        assert_eq!(index.pending_points(), 1);
        assert!(index.in_flight().is_none());
        assert_eq!(
            index.state(),
            HspStreamState::StopRequired(HspStreamError::IndexOverflow)
        );

        let mut serial = stream();
        offer(&mut serial, 0, 0, prefill());
        offer(&mut serial, 1, 100, prefill());
        serial.chunk_sequence = u64::MAX;
        assert_eq!(
            serial.claim(10_000, prefill()),
            Err(HspStreamError::ChunkSequenceOverflow)
        );
        assert_eq!(serial.pending_points(), 2);
        assert!(serial.in_flight().is_none());
    }

    #[test]
    fn malformed_policy_identity_and_regressing_phase_are_refused() {
        for invalid in [
            HspPolicy {
                max_buffered_points: 4097,
                ..policy()
            },
            HspPolicy {
                device_capacity_points: 1,
                ..policy()
            },
            HspPolicy {
                max_chunk_points: 101,
                ..policy()
            },
            HspPolicy {
                max_chunk_points: 1,
                ..policy()
            },
            HspPolicy {
                min_lead_ms: 0,
                ..policy()
            },
            HspPolicy {
                max_future_ms: 9,
                ..policy()
            },
            HspPolicy {
                max_sample_age_ns: 0,
                ..policy()
            },
            HspPolicy {
                max_gap_ms: 0,
                ..policy()
            },
        ] {
            assert!(matches!(
                HspStream::new(identity(), invalid),
                Err(HspStreamError::InvalidPolicy)
            ));
        }
        assert!(matches!(
            HspStream::new(
                HspStreamIdentity {
                    generation: 0,
                    ..identity()
                },
                policy()
            ),
            Err(HspStreamError::InvalidIdentity)
        ));
        for bad in [
            phase(1, 0),
            phase(i64::from(i32::MIN) - 1, 0),
            phase(0, i64::from(i32::MAX) + 1),
        ] {
            assert_eq!(
                stream().tick(10_000, bad),
                Err(HspStreamError::InvalidPhase)
            );
        }
        let mut clock = running();
        clock.tick(10_001, phase(0, 10)).unwrap();
        assert_eq!(
            clock.tick(10_002, phase(-2, -1)),
            Err(HspStreamError::ClockRegression)
        );
        assert_eq!(
            clock.state(),
            HspStreamState::StopRequired(HspStreamError::ClockRegression)
        );
        let mut native = running();
        assert_eq!(
            native.tick(9_999, phase(0, 10)),
            Err(HspStreamError::ClockRegression)
        );
    }

    include!("handy_stream_finite_tests.rs");
    include!("handy_stream_transition_tests.rs");
    include!("handy_stream_passage_tests.rs");
}
