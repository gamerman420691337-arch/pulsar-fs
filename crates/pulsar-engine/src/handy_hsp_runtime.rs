//! Native, supplied-fact HSP coordinator. No authenticated transport, device
//! qualification, physical observation, or public playback admission is provided.
//! The Engine must verify every supplied authority and clock fact and run the
//! watchdog even while inference or transport is blocked. Facts are deliberately
//! not deserializable client authority.
use crate::device_effect_journal::CancelledHspReceipt;
use crate::handy_hsp_wire::{
    decode_state, HspEncodedRequest, HspPlayState, HspState, HspWireError, HspWirePoint,
};
use crate::handy_playback_clock::HspClockPin;
use pulsar_core::{
    ArtifactId, ClientId, DeviceSessionId, HspAppendOutcome, HspChunk, HspDrainProgress,
    HspFiniteDrainPolicy, HspFiniteDrainWindow, HspInputSeal, HspInputState, HspOfferOutcome,
    HspPhaseWindow, HspPolicy, HspSample, HspSourceCursor, HspSourceTransition,
    HspSourceTransitionSpec, HspStream, HspStreamError, HspStreamIdentity, HspStreamState,
    HspTailObservation, LiveIdentity,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HspRuntimeError {
    Core(HspStreamError),
    Wire(HspWireError),
    ForeignAuthority,
    InvalidClock,
    ExpiredAuthority,
    Retired,
    PlayAlreadyRequested,
    ForeignEffect,
    AlreadyObserved,
    NativeDispatchBusy,
    SequenceExhausted,
}
impl From<HspStreamError> for HspRuntimeError {
    fn from(value: HspStreamError) -> Self {
        Self::Core(value)
    }
}
impl From<HspWireError> for HspRuntimeError {
    fn from(value: HspWireError) -> Self {
        Self::Wire(value)
    }
}

/// Engine-verified owner/session/source binding. The owner must allocate a fresh
/// generation for every runtime, including one that reuses a remote stream ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspBinding {
    identity: HspStreamIdentity,
    owner: ClientId,
    session: DeviceSessionId,
    clock_domain: ArtifactId,
}
impl HspBinding {
    pub fn new(
        identity: HspStreamIdentity,
        owner: ClientId,
        session: DeviceSessionId,
        clock_domain: ArtifactId,
    ) -> Self {
        Self {
            identity,
            owner,
            session,
            clock_domain,
        }
    }
    pub fn identity(&self) -> &HspStreamIdentity {
        &self.identity
    }
    pub fn owner(&self) -> &ClientId {
        &self.owner
    }
    pub fn session(&self) -> &DeviceSessionId {
        &self.session
    }
    pub fn clock_domain(&self) -> &ArtifactId {
        &self.clock_domain
    }
}

/// Original Engine-verified correlated native/server/script clock reference.
/// `phase` must independently bound script phase at that instant; never derive it
/// from response arrival, remote `current_point`, or inference completion time.
/// `server_time_ms` and `start_time_ms` are the original correlated play reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspPlayReference {
    at_ns: u64,
    phase: HspPhaseWindow,
    start_time_ms: i32,
    server_time_ms: u64,
}
impl HspPlayReference {
    pub fn at_ns(&self) -> u64 {
        self.at_ns
    }
    pub fn phase(&self) -> HspPhaseWindow {
        self.phase
    }
    pub fn start_time_ms(&self) -> i32 {
        self.start_time_ms
    }
    pub fn server_time_ms(&self) -> u64 {
        self.server_time_ms
    }
    pub fn new(
        at_ns: u64,
        phase: HspPhaseWindow,
        start_time_ms: i32,
        server_time_ms: u64,
    ) -> Result<Self, HspRuntimeError> {
        if phase.earliest_ms > phase.latest_ms
            || phase.earliest_ms < i64::from(i32::MIN)
            || phase.latest_ms > i64::from(i32::MAX)
            || i64::from(start_time_ms) < phase.earliest_ms
            || i64::from(start_time_ms) > phase.latest_ms
        {
            return Err(HspRuntimeError::InvalidClock);
        }
        HspEncodedRequest::play(start_time_ms, server_time_ms)?;
        Ok(Self {
            at_ns,
            phase,
            start_time_ms,
            server_time_ms,
        })
    }
}

/// Engine-verified validity deadlines, including clock uncertainty and maximum
/// command age. Reanchoring cannot refresh the original source deadlines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspAuthorityWindow {
    lease_until_ns: u64,
    source_until_ns: u64,
    clock_until_ns: u64,
}
impl HspAuthorityWindow {
    pub fn lease_until_ns(&self) -> u64 {
        self.lease_until_ns
    }
    pub fn source_until_ns(&self) -> u64 {
        self.source_until_ns
    }
    pub fn clock_until_ns(&self) -> u64 {
        self.clock_until_ns
    }
    pub fn new(lease_until_ns: u64, source_until_ns: u64, clock_until_ns: u64) -> Self {
        Self {
            lease_until_ns,
            source_until_ns,
            clock_until_ns,
        }
    }
}

/// Facts validated by the Engine for the reference's exact native instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspDispatchFacts {
    binding: HspBinding,
    reference: HspPlayReference,
    window: HspAuthorityWindow,
    clock_pin: Option<HspClockPin>,
}
impl HspDispatchFacts {
    pub fn binding(&self) -> &HspBinding {
        &self.binding
    }
    pub fn reference(&self) -> &HspPlayReference {
        &self.reference
    }
    pub fn window(&self) -> &HspAuthorityWindow {
        &self.window
    }
    pub(crate) fn clock_pin(&self) -> Option<&HspClockPin> {
        self.clock_pin.as_ref()
    }
    /// Attach one immutable native observation to newly constructed facts.
    /// Cloned facts retain that exact pin; it cannot be replaced in place.
    pub(crate) fn with_clock_pin(mut self, pin: HspClockPin) -> Result<Self, HspRuntimeError> {
        if self
            .clock_pin
            .as_ref()
            .is_some_and(|original| original != &pin)
        {
            return Err(HspRuntimeError::ForeignAuthority);
        }
        pin.validate_facts(self.reference.at_ns, &self)
            .map_err(|_| HspRuntimeError::InvalidClock)?;
        self.clock_pin = Some(pin);
        self.valid_at(self.reference.at_ns)?;
        Ok(self)
    }
    pub fn new(
        binding: HspBinding,
        reference: HspPlayReference,
        window: HspAuthorityWindow,
    ) -> Result<Self, HspRuntimeError> {
        let value = Self {
            binding,
            reference,
            window,
            clock_pin: None,
        };
        value.valid_at(value.reference.at_ns)?;
        Ok(value)
    }
    pub(crate) fn valid_at(&self, now_ns: u64) -> Result<(), HspRuntimeError> {
        if now_ns < self.reference.at_ns {
            return Err(HspRuntimeError::InvalidClock);
        }
        if now_ns >= self.window.lease_until_ns || now_ns >= self.window.source_until_ns {
            return Err(HspRuntimeError::ExpiredAuthority);
        }
        if now_ns >= self.window.clock_until_ns {
            return Err(HspRuntimeError::InvalidClock);
        }
        if let Some(pin) = &self.clock_pin {
            pin.validate_facts(now_ns, self)
                .map_err(|_| HspRuntimeError::InvalidClock)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspEffectKind {
    Setup,
    Append,
    Play,
    Stop,
}

/// Once-issued, sealed operation identity with immutable original request bytes.
/// A dispatcher may send this intent once. Cloning it is for custody/completion,
/// never retry permission. `/hsp/stop` has no stream ID on the wire: the transport
/// owner MUST prove the exact original session/stream before sending this bound
/// intent, including when ordinary source/clock authority has expired.
#[derive(Clone, Debug)]
struct NativeRuntimeIdentity(Arc<()>);
impl PartialEq for NativeRuntimeIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for NativeRuntimeIdentity {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspEffect {
    runtime_instance: NativeRuntimeIdentity,
    sequence: u64,
    kind: HspEffectKind,
    binding: HspBinding,
    request: HspEncodedRequest,
    original_facts: Option<HspDispatchFacts>,
    chunk: Option<Arc<HspChunk>>,
    expected_running: bool,
}
impl HspEffect {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn kind(&self) -> HspEffectKind {
        self.kind
    }
    pub fn binding(&self) -> &HspBinding {
        &self.binding
    }
    pub fn request(&self) -> &HspEncodedRequest {
        &self.request
    }
    pub fn chunk(&self) -> Option<&HspChunk> {
        self.chunk.as_deref()
    }
    pub fn original_facts(&self) -> Option<&HspDispatchFacts> {
        self.original_facts.as_ref()
    }
    pub fn expected_running(&self) -> bool {
        self.expected_running
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspRuntimeState {
    AwaitingSetup,
    Prefilling,
    Playing,
    StopRequired,
    StopInFlight,
    /// Transport acknowledgment only; never evidence of physical stopping.
    StopAcknowledged,
    /// One stop was issued and its outcome is unknown. No automatic retry.
    StopUnknown,
    /// The first Setup was durably cancelled before claim. This generation sent
    /// nothing, is terminal, and must not issue a follow-up Stop.
    SetupCancelled,
    /// Native custody proves the exact Setup never reached a claim. No durable
    /// cancellation is asserted, and this generation must not issue Stop.
    SetupUnsent,
    /// The journal proved this Stop was cancelled before claim. No automatic
    /// replacement Stop is issued. The physical stop obligation remains open;
    /// this state provides no physical stopping observation.
    StopUnclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HspObservedOutcome {
    /// Correlated approximate transport snapshot; no point-content/physical proof.
    Acknowledged(HspState),
    Unknown,
}

/// The first unknown judgment stays unknown even when a late reply is recorded.
/// The external owner can persist returned records; internal custody is bounded
/// to the last completion and at most one uncertain ordinary operation plus stop.
#[derive(Clone, Debug, PartialEq)]
pub struct HspCompletionRecord {
    pub effect: HspEffect,
    pub outcome: HspObservedOutcome,
    pub late_observation: Option<HspObservedOutcome>,
}

/// Supply ONLY once the transport task has stopped writing and has been joined
/// or otherwise proven settled. A timeout or cancellation request alone does not
/// satisfy this contract and must not free the single transport slot.
pub enum HspSettledTransport<'a> {
    Response { status: u16, body: &'a [u8] },
    Unknown,
}

// The runtime retains the exact native reservation across dispatcher instances.
// Only the worker holding this opaque token can settle it after join. A rejected
// duplicate, a timeout, and a cancellation request cannot release another worker.
const NATIVE_RESERVED: u8 = 0;
const NATIVE_ATTEMPTING: u8 = 1;
const NATIVE_CLAIMED: u8 = 2;
const NATIVE_SETTLED: u8 = 3;
const NATIVE_RELEASED: u8 = 4;
#[derive(Clone)]
pub(crate) struct NativeHspTransportIdentity(Arc<()>);
impl NativeHspTransportIdentity {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
struct NativeHspCustody {
    transport: NativeHspTransportIdentity,
    effect: HspEffect,
    state: AtomicU8,
}
#[derive(Clone)]
pub(crate) struct NativeHspReservation(Arc<NativeHspCustody>);
impl NativeHspReservation {
    pub(crate) fn begin_claim(&self) -> Result<(), HspRuntimeError> {
        self.0
            .state
            .compare_exchange(
                NATIVE_RESERVED,
                NATIVE_ATTEMPTING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| HspRuntimeError::NativeDispatchBusy)
    }
    pub(crate) fn reject_claim(&self) {
        // A normal journal error proves its transaction did not claim. Panic
        // leaves ATTEMPTING and therefore conservatively consumes this attempt.
        let _ = self.0.state.compare_exchange(
            NATIVE_ATTEMPTING,
            NATIVE_RESERVED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub(crate) fn claimed(&self) {
        let _ = self.0.state.compare_exchange(
            NATIVE_ATTEMPTING,
            NATIVE_CLAIMED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub(crate) fn join(&self) -> bool {
        // Called only after the exact worker joined (or failed to spawn). Atomic
        // settlement deliberately does not take the caller-owned runtime lock.
        let state = self.0.state.load(Ordering::Acquire);
        let attempted = state != NATIVE_RESERVED && state != NATIVE_RELEASED;
        self.0.state.store(
            if attempted {
                NATIVE_SETTLED
            } else {
                NATIVE_RELEASED
            },
            Ordering::Release,
        );
        attempted
    }
}

#[derive(Clone)]
pub(crate) struct NativeHspProbeReservation {
    instance: NativeRuntimeIdentity,
    setup: HspEffect,
    sequence: u64,
    in_flight: Option<HspEffect>,
    joined: Arc<AtomicBool>,
}
impl NativeHspProbeReservation {
    pub(crate) fn join(&self) {
        self.joined.store(true, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HspRuntimeTick {
    Continue,
    TailReached(Box<HspTailObservation>),
}

#[derive(Clone, Copy)]
pub(crate) struct HspFiniteDrainConfig {
    pub(crate) policy: HspFiniteDrainPolicy,
    pub(crate) maximum_stop_dispatch_delay_ns: u64,
}

pub struct HandyHspRuntime {
    instance: NativeRuntimeIdentity,
    attempted_setup: Option<(HspEffect, NativeHspTransportIdentity)>,
    probe: Option<NativeHspProbeReservation>,
    native_unsent_effect: Option<HspEffect>,
    binding: HspBinding,
    stream: HspStream,
    policy: HspPolicy,
    remote_capacity: Option<u32>,
    state: HspRuntimeState,
    next_sequence: u64,
    in_flight: Option<HspEffect>,
    native_custody: Option<NativeHspReservation>,
    pending_play: Option<HspDispatchFacts>,
    play_requested: bool,
    last_completion: Option<HspCompletionRecord>,
    uncertain_effect: Option<HspCompletionRecord>,
    uncertain_stop: Option<HspCompletionRecord>,
    unclaimed_effect: Option<HspEffect>,
    unclaimed_stop: Option<HspEffect>,
    finite_seal: Option<HspInputSeal>,
    finite_tail: Option<Box<HspTailObservation>>,
    finite_dispatch_delay_ns: Option<u64>,
    finite_aborted: bool,
    source_transition_facts: Option<HspDispatchFacts>,
}
impl HandyHspRuntime {
    pub fn new(binding: HspBinding, policy: HspPolicy) -> Result<Self, HspRuntimeError> {
        let stream = HspStream::new(binding.identity.clone(), policy)?;
        Ok(Self::from_stream(binding, policy, stream))
    }
    pub fn new_with_source(
        binding: HspBinding,
        policy: HspPolicy,
        source: HspSourceCursor,
    ) -> Result<Self, HspRuntimeError> {
        let stream = HspStream::new_with_source(binding.identity.clone(), policy, source)?;
        Ok(Self::from_stream(binding, policy, stream))
    }
    fn from_stream(binding: HspBinding, policy: HspPolicy, stream: HspStream) -> Self {
        Self {
            instance: NativeRuntimeIdentity(Arc::new(())),
            attempted_setup: None,
            probe: None,
            native_unsent_effect: None,
            binding,
            stream,
            policy,
            remote_capacity: None,
            state: HspRuntimeState::AwaitingSetup,
            next_sequence: 0,
            in_flight: None,
            native_custody: None,
            pending_play: None,
            play_requested: false,
            last_completion: None,
            uncertain_effect: None,
            uncertain_stop: None,
            unclaimed_effect: None,
            unclaimed_stop: None,
            finite_seal: None,
            finite_tail: None,
            finite_dispatch_delay_ns: None,
            finite_aborted: false,
            source_transition_facts: None,
        }
    }
    pub fn state(&self) -> HspRuntimeState {
        self.state
    }
    pub fn stream(&self) -> &HspStream {
        &self.stream
    }
    pub fn active_source(&self) -> Option<&HspSourceCursor> {
        self.stream.active_source()
    }
    pub fn source_transition_pending(&self) -> bool {
        self.stream.source_transition_pending()
    }
    fn active_identity(&self) -> &LiveIdentity {
        self.active_source()
            .map_or(&self.binding.identity.live, |source| &source.identity)
    }
    pub fn policy(&self) -> HspPolicy {
        self.policy
    }
    pub fn in_flight(&self) -> Option<&HspEffect> {
        self.in_flight.as_ref()
    }
    pub(crate) fn reserve_native(
        &mut self,
        effect: &HspEffect,
        transport: &NativeHspTransportIdentity,
    ) -> Result<NativeHspReservation, HspRuntimeError> {
        if self.in_flight.as_ref() != Some(effect) {
            return Err(HspRuntimeError::ForeignEffect);
        }
        self.require_probe_joined()?;
        self.require_native_released()?;
        let reservation = NativeHspReservation(Arc::new(NativeHspCustody {
            transport: transport.clone(),
            effect: effect.clone(),
            state: AtomicU8::new(NATIVE_RESERVED),
        }));
        self.native_custody = Some(reservation.clone());
        Ok(reservation)
    }
    pub(crate) fn validate_native_completion(
        &self,
        effect: &HspEffect,
        reservation: &NativeHspReservation,
    ) -> Result<(), HspRuntimeError> {
        if self.in_flight.as_ref() != Some(effect)
            || !self
                .native_custody
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(&current.0, &reservation.0))
        {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if reservation.0.state.load(Ordering::Acquire) != NATIVE_SETTLED {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        Ok(())
    }
    pub(crate) fn require_native_released(&self) -> Result<(), HspRuntimeError> {
        if self
            .native_custody
            .as_ref()
            .is_some_and(|r| r.0.state.load(Ordering::Acquire) != NATIVE_RELEASED)
        {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        Ok(())
    }
    fn require_probe_joined(&self) -> Result<(), HspRuntimeError> {
        if self
            .probe
            .as_ref()
            .is_some_and(|p| !p.joined.load(Ordering::Acquire))
        {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        Ok(())
    }
    pub(crate) fn reserve_post_setup_probe(
        &mut self,
        setup: &HspEffect,
        transport: &NativeHspTransportIdentity,
    ) -> Result<NativeHspProbeReservation, HspRuntimeError> {
        self.require_probe_joined()?;
        if self
            .attempted_setup
            .as_ref()
            .is_none_or(|(effect, original)| effect != setup || !original.same(transport))
            || self
                .last_completion
                .as_ref()
                .is_none_or(|r| r.effect != *setup)
            || self
                .in_flight
                .as_ref()
                .is_some_and(|e| e.kind != HspEffectKind::Stop)
        {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if self.native_custody.as_ref().is_some_and(|r| {
            !matches!(
                r.0.state.load(Ordering::Acquire),
                NATIVE_SETTLED | NATIVE_RELEASED
            )
        }) {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        let reservation = NativeHspProbeReservation {
            instance: self.instance.clone(),
            setup: setup.clone(),
            sequence: self.next_sequence,
            in_flight: self.in_flight.clone(),
            joined: Arc::new(AtomicBool::new(false)),
        };
        self.probe = Some(reservation.clone());
        Ok(reservation)
    }
    pub(crate) fn validate_post_setup_probe(
        &self,
        probe: &NativeHspProbeReservation,
    ) -> Result<(), HspRuntimeError> {
        if self.instance != probe.instance
            || self
                .attempted_setup
                .as_ref()
                .is_none_or(|(effect, _)| effect != &probe.setup)
            || self.next_sequence != probe.sequence
            || self.in_flight != probe.in_flight
            || self
                .probe
                .as_ref()
                .is_none_or(|p| !Arc::ptr_eq(&p.joined, &probe.joined))
        {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if !probe.joined.load(Ordering::Acquire) {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        Ok(())
    }
    /// A native-only observation. It does not assert a journal cancellation,
    /// release historical route custody, or establish any remote state.
    pub fn native_unsent_effect(&self) -> Option<&HspEffect> {
        self.native_unsent_effect.as_ref()
    }
    pub(crate) fn retire_native_unsent(
        &mut self,
        effect: &HspEffect,
        transport: &NativeHspTransportIdentity,
    ) -> Result<(), HspRuntimeError> {
        self.require_probe_joined()?;
        self.require_native_released()?;
        if effect.kind == HspEffectKind::Stop
            || effect.runtime_instance != self.instance
            || self.in_flight.as_ref() != Some(effect)
            || self
                .native_custody
                .as_ref()
                .is_some_and(|r| r.0.effect != *effect || !r.0.transport.same(transport))
        {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if let Some(chunk) = &effect.chunk {
            self.stream
                .complete(chunk.token(), HspAppendOutcome::Rejected)?;
        }
        self.pending_play = None;
        self.stream.cancel();
        self.in_flight = None;
        self.state = if effect.kind == HspEffectKind::Setup {
            HspRuntimeState::SetupUnsent
        } else {
            HspRuntimeState::StopRequired
        };
        self.native_unsent_effect = Some(effect.clone());
        Ok(())
    }
    pub fn last_completion(&self) -> Option<&HspCompletionRecord> {
        self.last_completion.as_ref()
    }
    pub fn uncertain_effect(&self) -> Option<&HspCompletionRecord> {
        self.uncertain_effect.as_ref()
    }
    pub fn uncertain_stop(&self) -> Option<&HspCompletionRecord> {
        self.uncertain_stop.as_ref()
    }
    /// Exact ordinary intent retired after durable proof it was never claimed.
    /// This is neither a network observation nor permission to retry it.
    pub fn unclaimed_effect(&self) -> Option<&HspEffect> {
        self.unclaimed_effect.as_ref()
    }
    pub fn unclaimed_stop(&self) -> Option<&HspEffect> {
        self.unclaimed_stop.as_ref()
    }

    fn open(&self) -> bool {
        matches!(
            self.state,
            HspRuntimeState::AwaitingSetup | HspRuntimeState::Prefilling | HspRuntimeState::Playing
        )
    }
    fn check_facts(&self, facts: &HspDispatchFacts, now_ns: u64) -> Result<(), HspRuntimeError> {
        if facts.binding != self.binding {
            return Err(HspRuntimeError::ForeignAuthority);
        }
        if facts.reference.at_ns != now_ns {
            return Err(HspRuntimeError::InvalidClock);
        }
        facts.valid_at(now_ns)
    }
    fn validate_context(
        &mut self,
        facts: &HspDispatchFacts,
        now_ns: u64,
    ) -> Result<(), HspRuntimeError> {
        if !self.open() {
            return Err(HspRuntimeError::Retired);
        }
        if let Err(error) = self.check_facts(facts, now_ns) {
            // A foreign producer cannot cancel another owner's valid generation.
            if error != HspRuntimeError::ForeignAuthority {
                self.stop();
            }
            return Err(error);
        }
        // A watchdog with newer facts cannot renew an outstanding command or a
        // queued explicit play reference while its original task is blocked.
        let original = self
            .in_flight
            .as_ref()
            .and_then(|effect| effect.original_facts.as_ref())
            .into_iter()
            .chain(self.pending_play.as_ref())
            .chain(self.source_transition_facts.as_ref())
            .find_map(|original| original.valid_at(now_ns).err());
        if let Some(error) = original {
            self.stop();
            return Err(error);
        }
        Ok(())
    }
    fn guard(&mut self, facts: &HspDispatchFacts, now_ns: u64) -> Result<(), HspRuntimeError> {
        self.validate_context(facts, now_ns)?;
        if let Err(error) = self.stream.tick(now_ns, facts.reference.phase) {
            self.stop();
            return Err(error.into());
        }
        Ok(())
    }

    pub(crate) fn finite_seal(&self) -> Option<&HspInputSeal> {
        self.finite_seal.as_ref()
    }
    pub(crate) fn finite_tail(&self) -> Option<&HspTailObservation> {
        self.finite_tail.as_deref()
    }
    pub(crate) fn finite_aborted(&self) -> bool {
        self.finite_aborted
    }
    pub(crate) fn finite_dispatch_delay_ns(&self) -> Option<u64> {
        self.finite_dispatch_delay_ns
    }
    pub(crate) fn is_finite_draining(&self) -> bool {
        // Core retains the input state after cancellation for honest history.
        // It must not hide the runtime's newly required Stop from the owner.
        self.open() && self.stream.input_state() == HspInputState::Draining
    }
    pub(crate) fn seal_input(
        &mut self,
        identity: &LiveIdentity,
        terminal: HspSample,
        closed_at_ns: u64,
        config: HspFiniteDrainConfig,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> Result<HspInputSeal, HspRuntimeError> {
        let HspFiniteDrainConfig {
            policy,
            maximum_stop_dispatch_delay_ns,
        } = config;
        if identity != self.active_identity() {
            return Err(HspStreamError::ForeignSource.into());
        }
        if maximum_stop_dispatch_delay_ns == 0
            || maximum_stop_dispatch_delay_ns >= policy.stop_budget_ns
        {
            return Err(HspStreamError::InvalidDrainWindow.into());
        }
        if let Some(seal) = &self.finite_seal {
            if seal.terminal() == terminal
                && seal.closed_at_ns() == closed_at_ns
                && seal.policy() == policy
                && self.finite_dispatch_delay_ns == Some(maximum_stop_dispatch_delay_ns)
            {
                return Ok(seal.clone());
            }
            return Err(HspStreamError::ForeignSeal.into());
        }
        self.guard(facts, now_ns)?;
        let seal = match self.stream.seal_input(
            identity,
            terminal,
            closed_at_ns,
            policy,
            now_ns,
            facts.reference.phase,
        ) {
            Ok(seal) => seal,
            Err(error) => {
                self.stop();
                return Err(error.into());
            }
        };
        self.finite_dispatch_delay_ns = Some(maximum_stop_dispatch_delay_ns);
        self.finite_seal = Some(seal.clone());
        Ok(seal)
    }
    /// The owner has joined its active native command/probe before it passes
    /// `native_jobs_joined`. Actual Play acknowledgment is checked separately.
    pub(crate) fn tick_snapshot(
        &mut self,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
        window: Option<HspFiniteDrainWindow>,
        native_jobs_joined: bool,
    ) -> Result<HspRuntimeTick, HspRuntimeError> {
        let Some(facts) = facts else {
            self.stop();
            return Err(HspRuntimeError::ExpiredAuthority);
        };
        self.validate_context(facts, now_ns)?;
        let Some(seal) = self.finite_seal.clone() else {
            self.guard(facts, now_ns)?;
            return Ok(HspRuntimeTick::Continue);
        };
        let checked_window = || -> Result<HspFiniteDrainWindow, HspRuntimeError> {
            let window = window.ok_or(HspStreamError::DrainWindowRequired)?;
            if window.at_ns != now_ns
                || window.phase != facts.reference.phase
                || window.stop_at_ns >= facts.window.lease_until_ns
                || window.stop_at_ns >= facts.window.source_until_ns
                || window.stop_at_ns >= facts.window.clock_until_ns
            {
                return Err(HspRuntimeError::InvalidClock);
            }
            Ok(window)
        };
        let result = (|| -> Result<Option<HspDrainProgress>, HspRuntimeError> {
            match self.stream.input_state() {
                HspInputState::Sealed => {
                    self.stream.tick(now_ns, facts.reference.phase)?;
                    let settled = self.native_custody.as_ref().is_none_or(|r| {
                        matches!(
                            r.0.state.load(Ordering::Acquire),
                            NATIVE_SETTLED | NATIVE_RELEASED
                        )
                    }) && self
                        .probe
                        .as_ref()
                        .is_none_or(|probe| probe.joined.load(Ordering::Acquire));
                    if self.state != HspRuntimeState::Playing
                        || self.in_flight.is_some()
                        || self.uncertain_effect.is_some()
                        || !native_jobs_joined
                        || !settled
                        || self.stream.pending_points() != 0
                        || self.stream.in_flight().is_some()
                    {
                        return Ok(None);
                    }
                    Ok(Some(self.stream.begin_finite_drain(
                        &seal,
                        now_ns,
                        checked_window()?,
                    )?))
                }
                HspInputState::Draining => Ok(Some(self.stream.tick_finite_drain(
                    &seal,
                    now_ns,
                    checked_window()?,
                )?)),
                _ => Err(HspRuntimeError::Retired),
            }
        })();
        match result {
            Ok(Some(HspDrainProgress::TailReached(observation))) => {
                self.finite_tail = Some(observation.clone());
                self.stop();
                Ok(HspRuntimeTick::TailReached(observation))
            }
            Ok(_) => Ok(HspRuntimeTick::Continue),
            Err(error) => {
                self.stop();
                Err(error)
            }
        }
    }

    /// Latch immediately, including while an append task is blocked. No further
    /// append/play can be issued; the active transport must settle before Stop.
    pub fn stop(&mut self) {
        if self.finite_seal.is_some() && self.finite_tail.is_none() {
            self.finite_aborted = true;
        }
        if self.open() {
            self.state = HspRuntimeState::StopRequired;
            self.pending_play = None;
            self.stream.cancel();
        }
    }

    /// Release only the exact local transport slot after the journal has
    /// atomically cancelled the exact sealed intent while it was unclaimed.
    /// The caller must retain the same dependency guards used by dispatch; the
    /// sealed proof cannot be deserialized or minted from a historical record.
    /// A rejected append stays frozen in core custody, without inventing an
    /// ambiguous network attempt or accepting a later completion as progress.
    pub(crate) fn retire_unclaimed(
        &mut self,
        effect: &HspEffect,
        proof: &CancelledHspReceipt,
    ) -> Result<(), HspRuntimeError> {
        self.require_native_released()?;
        if !proof.matches_effect(effect) {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if self.in_flight.is_none()
            && (self.unclaimed_effect.as_ref() == Some(effect)
                || self.unclaimed_stop.as_ref() == Some(effect))
        {
            return Ok(());
        }
        if self.in_flight.as_ref() != Some(effect) {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if let Some(chunk) = &effect.chunk {
            self.stream
                .complete(chunk.token(), HspAppendOutcome::Rejected)?;
        }
        self.pending_play = None;
        self.stream.cancel();
        self.in_flight = None;
        if effect.kind == HspEffectKind::Stop {
            self.state = HspRuntimeState::StopUnclaimed;
            self.unclaimed_stop = Some(effect.clone());
        } else {
            self.state = if effect.kind == HspEffectKind::Setup {
                HspRuntimeState::SetupCancelled
            } else {
                HspRuntimeState::StopRequired
            };
            self.unclaimed_effect = Some(effect.clone());
        }
        Ok(())
    }

    /// Required independent watchdog. Missing authority also latches Stop.
    pub fn tick(
        &mut self,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
    ) -> Result<(), HspRuntimeError> {
        let Some(facts) = facts else {
            self.stop();
            return Err(HspRuntimeError::ExpiredAuthority);
        };
        self.guard(facts, now_ns)
    }

    pub fn offer(
        &mut self,
        identity: &LiveIdentity,
        sample: HspSample,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> Result<HspOfferOutcome, HspRuntimeError> {
        if identity != self.active_identity() {
            return Err(HspStreamError::ForeignSource.into());
        }
        if self.finite_seal.is_some() {
            return Err(HspStreamError::InputSealed.into());
        }
        self.guard(facts, now_ns)?;
        let result = self
            .stream
            .offer(identity, sample, now_ns, facts.reference.phase);
        if matches!(self.stream.state(), HspStreamState::StopRequired(_)) {
            self.stop();
        }
        result.map_err(Into::into)
    }

    fn require_source_transfer_settled(&self) -> Result<(), HspRuntimeError> {
        if self.state != HspRuntimeState::Playing {
            return Err(HspStreamError::NotReady.into());
        }
        self.require_probe_joined()?;
        if self.in_flight.is_some()
            || self.pending_play.is_some()
            || self.uncertain_effect.is_some()
            || self.uncertain_stop.is_some()
            || self.unclaimed_effect.is_some()
            || self.unclaimed_stop.is_some()
            || self.native_unsent_effect.is_some()
            || self.native_custody.as_ref().is_some_and(|reservation| {
                !matches!(
                    reservation.0.state.load(Ordering::Acquire),
                    NATIVE_SETTLED | NATIVE_RELEASED
                )
            })
        {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        Ok(())
    }

    /// The Controller must first validate the native accepted pair and joined
    /// predecessor producer work. This method does not mint that authority.
    pub(crate) fn prepare_source_transition(
        &mut self,
        spec: HspSourceTransitionSpec,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> Result<HspSourceTransition, HspRuntimeError> {
        if self.active_source() != Some(&spec.predecessor) {
            return Err(HspStreamError::ForeignSource.into());
        }
        if self.finite_seal.is_some() {
            return Err(HspStreamError::InputSealed.into());
        }
        if self.source_transition_pending() {
            return Err(HspStreamError::SourceTransitionPending.into());
        }
        self.guard(facts, now_ns)?;
        self.require_source_transfer_settled()?;
        let result = self
            .stream
            .prepare_source_transition(spec, now_ns, facts.reference.phase);
        if matches!(self.stream.state(), HspStreamState::StopRequired(_)) {
            self.stop();
        }
        let transition = result?;
        // Later source/clock facts cannot renew this barrier's original
        // authority while the caller completes its atomic native handoff.
        self.source_transition_facts = Some(facts.clone());
        Ok(transition)
    }

    pub(crate) fn commit_source_transition(
        &mut self,
        transition: HspSourceTransition,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> Result<(), HspRuntimeError> {
        if !self.stream.owns_source_transition(&transition) {
            return Err(HspStreamError::ForeignSourceTransition.into());
        }
        self.guard(facts, now_ns)?;
        if let Err(error) = self.require_source_transfer_settled() {
            self.stop();
            return Err(error);
        }
        if let Err(error) =
            self.stream
                .commit_source_transition(transition, now_ns, facts.reference.phase)
        {
            self.stop();
            return Err(error.into());
        }
        self.source_transition_facts = None;
        Ok(())
    }

    /// Explicit Engine-verified user play intent. Merely supplying samples never
    /// calls this. The original reference/deadlines are retained until issuance.
    pub fn request_play(
        &mut self,
        now_ns: u64,
        facts: &HspDispatchFacts,
    ) -> Result<(), HspRuntimeError> {
        self.request_play_original(now_ns, facts, facts)
    }

    /// Retain the one original native admission reference across request
    /// persistence. Fresh facts validate current authority but cannot retime it.
    pub(crate) fn request_play_original(
        &mut self,
        now_ns: u64,
        current: &HspDispatchFacts,
        original: &HspDispatchFacts,
    ) -> Result<(), HspRuntimeError> {
        if self.source_transition_pending() {
            return Err(HspStreamError::SourceTransitionPending.into());
        }
        self.guard(current, now_ns)?;
        self.check_facts(original, original.reference.at_ns)?;
        if let Err(error) = original.valid_at(now_ns) {
            self.stop();
            return Err(error);
        }
        if self.play_requested {
            return Err(HspRuntimeError::PlayAlreadyRequested);
        }
        self.pending_play = Some(original.clone());
        self.play_requested = true;
        Ok(())
    }

    /// Claim once at dispatch time. Repeated polling while an effect is in flight
    /// never produces another command. Stop intentionally needs no ordinary data
    /// or clock fact; its binding still requires the transport guard documented
    /// on `HspEffect`. No network guarantee is made by this coordinator.
    pub fn next_effect(
        &mut self,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
    ) -> Result<Option<HspEffect>, HspRuntimeError> {
        if self.open() {
            self.tick(now_ns, facts)?;
        }
        if self.in_flight.is_some() {
            return Ok(None);
        }
        if self.state == HspRuntimeState::StopRequired {
            return self
                .issue(HspEffectKind::Stop, HspEncodedRequest::stop(), None, None)
                .map(Some);
        }
        if !self.open() {
            return Ok(None);
        }
        if self.source_transition_pending() {
            return Ok(None);
        }
        let facts = facts.ok_or(HspRuntimeError::ExpiredAuthority)?;
        if self.state == HspRuntimeState::AwaitingSetup {
            let request = HspEncodedRequest::setup(self.binding.identity.stream_id)?;
            return self
                .issue(HspEffectKind::Setup, request, Some(facts.clone()), None)
                .map(Some);
        }
        if let Some(original) = self
            .pending_play
            .as_ref()
            .filter(|_| self.stream.total_added() >= 2)
        {
            if let Err(error) = original.valid_at(now_ns) {
                self.stop();
                return Err(error);
            }
            let original = original.clone();
            if let Err(error) = self.stream.mark_running(now_ns, facts.reference.phase) {
                self.stop();
                return Err(error.into());
            }
            let request = HspEncodedRequest::play(
                original.reference.start_time_ms,
                original.reference.server_time_ms,
            )?;
            self.pending_play = None;
            return self
                .issue(HspEffectKind::Play, request, Some(original), None)
                .map(Some);
        }
        let chunk = match self.stream.claim(now_ns, facts.reference.phase) {
            Ok(chunk) => chunk,
            Err(HspStreamError::Capacity) => return Ok(None),
            Err(error) => {
                self.stop();
                return Err(error.into());
            }
        };
        let Some(chunk) = chunk else {
            return Ok(None);
        };
        let points = chunk
            .samples()
            .iter()
            .map(|p| HspWirePoint::new(p.point.time_ms, p.point.position_percent))
            .collect::<Result<Vec<_>, _>>()?;
        let request = HspEncodedRequest::add(&points, chunk.tail_point_stream_index())?;
        self.issue(
            HspEffectKind::Append,
            request,
            Some(facts.clone()),
            Some(chunk),
        )
        .map(Some)
    }

    fn issue(
        &mut self,
        kind: HspEffectKind,
        request: HspEncodedRequest,
        original_facts: Option<HspDispatchFacts>,
        chunk: Option<Arc<HspChunk>>,
    ) -> Result<HspEffect, HspRuntimeError> {
        // Preserve one final sequence for Stop if ordinary sequence space ends.
        if kind != HspEffectKind::Stop && self.next_sequence >= u64::MAX - 1 {
            self.stop();
            return Err(HspRuntimeError::SequenceExhausted);
        }
        let Some(sequence) = self.next_sequence.checked_add(1) else {
            self.stop();
            return Err(HspRuntimeError::SequenceExhausted);
        };
        let effect = HspEffect {
            runtime_instance: self.instance.clone(),
            sequence,
            kind,
            binding: self.binding.clone(),
            request,
            original_facts,
            chunk,
            expected_running: self.stream.state() == HspStreamState::Running,
        };
        self.next_sequence = sequence;
        self.native_custody = None;
        self.in_flight = Some(effect.clone());
        if kind == HspEffectKind::Stop {
            self.state = HspRuntimeState::StopInFlight;
        }
        Ok(effect)
    }

    /// Recheck immediately before handing an already claimed intent to transport.
    /// Fresh facts cannot refresh its original reference or validity deadline.
    pub fn validate_dispatch(
        &mut self,
        effect: &HspEffect,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
    ) -> Result<(), HspRuntimeError> {
        if self.in_flight.as_ref() != Some(effect) {
            return Err(HspRuntimeError::ForeignEffect);
        }
        if effect.kind == HspEffectKind::Stop {
            return Ok(());
        }
        if !self.open() {
            return Err(HspRuntimeError::Retired);
        }
        let Some(facts) = facts else {
            self.stop();
            return Err(HspRuntimeError::ExpiredAuthority);
        };
        self.guard(facts, now_ns)?;
        if let Err(error) = effect
            .original_facts
            .as_ref()
            .ok_or(HspRuntimeError::InvalidClock)?
            .valid_at(now_ns)
        {
            self.stop();
            return Err(error);
        }
        if let Some(chunk) = &effect.chunk {
            for sample in chunk.samples() {
                let error = if now_ns.saturating_sub(sample.captured_at_ns)
                    > self.policy.max_sample_age_ns
                {
                    Some(HspStreamError::StaleSample)
                } else if i64::from(sample.point.time_ms) - facts.reference.phase.latest_ms
                    < i64::from(self.policy.min_lead_ms)
                {
                    Some(HspStreamError::InsufficientLead)
                } else if i64::from(sample.point.time_ms) - facts.reference.phase.earliest_ms
                    > i64::from(self.policy.max_future_ms)
                {
                    Some(HspStreamError::FutureHorizon)
                } else {
                    None
                };
                if let Some(error) = error {
                    self.stop();
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    fn correlate(
        &self,
        effect: &HspEffect,
        response: HspSettledTransport<'_>,
    ) -> HspObservedOutcome {
        let HspSettledTransport::Response { status, body } = response else {
            return HspObservedOutcome::Unknown;
        };
        let Ok(state) = decode_state(status, body) else {
            return HspObservedOutcome::Unknown;
        };
        if state.stream_id != effect.binding.identity.stream_id {
            return HspObservedOutcome::Unknown;
        }
        if effect.kind == HspEffectKind::Stop {
            return if state.play_state == HspPlayState::Stopped {
                HspObservedOutcome::Acknowledged(state)
            } else {
                HspObservedOutcome::Unknown
            };
        }
        if state.max_points < self.policy.device_capacity_points as u32
            || state.points > state.max_points
            || self
                .remote_capacity
                .is_some_and(|capacity| capacity != state.max_points)
        {
            return HspObservedOutcome::Unknown;
        }
        // Setup does not establish playback options. Explicit Play sets them;
        // only its response and subsequent running appends must echo that fixed
        // profile. Decoded defaults on empty setup/stopped prefill are usable.
        let requires_play_options = effect.kind == HspEffectKind::Play
            || (effect.kind == HspEffectKind::Append && effect.expected_running);
        if requires_play_options
            && (state.looping || state.playback_rate != 1.0 || state.pause_on_starving)
        {
            return HspObservedOutcome::Unknown;
        }
        let matched = match effect.kind {
            // Empty count establishes the local initial append index; the
            // approximate initial remote tail is not a point-content proof.
            HspEffectKind::Setup => {
                matches!(
                    state.play_state,
                    HspPlayState::NotInitialized | HspPlayState::Stopped
                ) && state.points == 0
            }
            HspEffectKind::Append => effect.chunk.as_ref().is_some_and(|chunk| {
                state.tail_point_stream_index == chunk.tail_point_stream_index() as i32
                    && matches!(
                        state.play_state,
                        HspPlayState::Stopped | HspPlayState::Playing
                    )
                    && (state.play_state == HspPlayState::Playing) == effect.expected_running
            }),
            HspEffectKind::Play => {
                state.play_state == HspPlayState::Playing
                    && u64::try_from(state.tail_point_stream_index)
                        .ok()
                        .and_then(|tail| tail.checked_add(1))
                        == Some(self.stream.total_added())
            }
            HspEffectKind::Stop => unreachable!(),
        };
        if matched {
            HspObservedOutcome::Acknowledged(state)
        } else {
            HspObservedOutcome::Unknown
        }
    }

    /// Return the exact original observation for persistence. A late response to
    /// an unknown operation can annotate its custody once, but cannot replace the
    /// unknown judgment, alter the queue, rearm playback, or issue another effect.
    /// The supplied transport result asserts the task is settled; see its type.
    pub fn complete_settled(
        &mut self,
        effect: &HspEffect,
        response: HspSettledTransport<'_>,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
    ) -> Result<HspCompletionRecord, HspRuntimeError> {
        if self.in_flight.as_ref() == Some(effect)
            && self.native_custody.as_ref().is_some_and(|r| {
                r.0.effect != *effect || r.0.state.load(Ordering::Acquire) != NATIVE_SETTLED
            })
        {
            return Err(HspRuntimeError::NativeDispatchBusy);
        }
        let outcome = self.correlate(effect, response);
        if self.in_flight.as_ref() != Some(effect) {
            let retained = if self
                .uncertain_effect
                .as_ref()
                .is_some_and(|r| r.effect == *effect)
            {
                &mut self.uncertain_effect
            } else if self
                .uncertain_stop
                .as_ref()
                .is_some_and(|r| r.effect == *effect)
            {
                &mut self.uncertain_stop
            } else {
                return Err(HspRuntimeError::ForeignEffect);
            };
            let record = retained.as_mut().expect("matched retained record");
            if record.late_observation.is_some() {
                return Err(HspRuntimeError::AlreadyObserved);
            }
            record.late_observation = Some(outcome);
            return Ok(record.clone());
        }
        if effect.kind != HspEffectKind::Stop && self.open() {
            // A genuine observation is retained even if authority expired while
            // the transport was blocked; expiry forbids advancing the generation.
            if effect
                .original_facts
                .as_ref()
                .is_some_and(|original| original.valid_at(now_ns).is_err())
                || facts.is_none_or(|facts| self.check_facts(facts, now_ns).is_err())
            {
                self.stop();
            } else if let Some(facts) = facts {
                if self.stream.tick(now_ns, facts.reference.phase).is_err() {
                    self.stop();
                }
            }
        }
        if let Some(chunk) = &effect.chunk {
            let append = match outcome {
                HspObservedOutcome::Acknowledged(state) => HspAppendOutcome::Acknowledged {
                    stream_id: state.stream_id,
                    tail_point_stream_index: state.tail_point_stream_index as u32,
                },
                HspObservedOutcome::Unknown => HspAppendOutcome::Unknown,
            };
            if self.finite_seal.is_some() && self.open() {
                let phase = facts
                    .ok_or(HspRuntimeError::ExpiredAuthority)?
                    .reference
                    .phase;
                if let Err(error) = self
                    .stream
                    .complete_at(chunk.token(), append, now_ns, phase)
                {
                    if self.stream.in_flight_outcome().is_some() {
                        self.stop();
                    } else {
                        return Err(error.into());
                    }
                }
            } else {
                self.stream.complete(chunk.token(), append)?;
            }
        }
        if effect.kind == HspEffectKind::Setup
            && self.native_custody.as_ref().is_some_and(|r| {
                r.0.effect == *effect && r.0.state.load(Ordering::Acquire) == NATIVE_SETTLED
            })
        {
            self.attempted_setup = Some((
                effect.clone(),
                self.native_custody
                    .as_ref()
                    .expect("checked settled custody")
                    .0
                    .transport
                    .clone(),
            ));
        }
        self.in_flight = None;
        match outcome {
            HspObservedOutcome::Unknown => {
                if effect.kind == HspEffectKind::Stop {
                    self.state = HspRuntimeState::StopUnknown;
                } else {
                    self.stop();
                }
            }
            HspObservedOutcome::Acknowledged(state) => match effect.kind {
                HspEffectKind::Setup if self.open() => {
                    self.remote_capacity = Some(state.max_points);
                    self.state = HspRuntimeState::Prefilling;
                }
                HspEffectKind::Play if self.open() => {
                    self.state = HspRuntimeState::Playing;
                }
                HspEffectKind::Stop => {
                    self.state = HspRuntimeState::StopAcknowledged;
                }
                _ => {}
            },
        }
        let record = HspCompletionRecord {
            effect: effect.clone(),
            outcome,
            late_observation: None,
        };
        if outcome == HspObservedOutcome::Unknown {
            if effect.kind == HspEffectKind::Stop {
                self.uncertain_stop = Some(record.clone());
            } else {
                self.uncertain_effect = Some(record.clone());
            }
        }
        self.last_completion = Some(record.clone());
        Ok(record)
    }
}

#[cfg(test)]
#[path = "handy_hsp_runtime_tests.rs"]
mod tests;

#[cfg(test)]
mod original_play_tests {
    use super::*;
    use pulsar_core::*;
    fn binding(stream_id: u32) -> HspBinding {
        HspBinding::new(
            HspStreamIdentity {
                generation: 1,
                stream_id,
                live: LiveIdentity {
                    epoch: AttemptId::new("capture").unwrap(),
                    project: ProjectId::new("project").unwrap(),
                    revision: RevisionId::new(0),
                    source: SourceVersionId::new("source").unwrap(),
                },
            },
            ClientId::new("owner").unwrap(),
            DeviceSessionId::new("session").unwrap(),
            ArtifactId::new("clock").unwrap(),
        )
    }
    fn facts(binding: &HspBinding, at: u64, until: u64) -> HspDispatchFacts {
        HspDispatchFacts::new(
            binding.clone(),
            HspPlayReference::new(
                at,
                HspPhaseWindow {
                    earliest_ms: -100,
                    latest_ms: -98,
                },
                -99,
                1000 + at,
            )
            .unwrap(),
            HspAuthorityWindow::new(until, until, until),
        )
        .unwrap()
    }
    fn runtime(binding: HspBinding) -> HandyHspRuntime {
        HandyHspRuntime::new(
            binding,
            HspPolicy {
                max_buffered_points: 10,
                device_capacity_points: 10,
                max_chunk_points: 2,
                min_lead_ms: 1,
                max_future_ms: 1000,
                max_sample_age_ns: 1000,
                max_gap_ms: 1000,
            },
        )
        .unwrap()
    }
    #[test]
    fn native_play_keeps_pre_persistence_reference_and_refuses_replacement() {
        let binding = binding(1);
        let mut runtime = runtime(binding.clone());
        let original = facts(&binding, 10, 100);
        runtime
            .request_play_original(20, &facts(&binding, 20, 1000), &original)
            .unwrap();
        assert_eq!(runtime.pending_play.as_ref(), Some(&original));
        assert_eq!(
            runtime.request_play_original(
                21,
                &facts(&binding, 21, 1000),
                &facts(&binding, 21, 1000)
            ),
            Err(HspRuntimeError::PlayAlreadyRequested)
        );
        assert_eq!(runtime.pending_play.as_ref(), Some(&original));
    }
    #[test]
    fn fresh_current_facts_do_not_revive_expired_or_foreign_original_play() {
        let binding = binding(1);
        let mut expired = runtime(binding.clone());
        assert_eq!(
            expired.request_play_original(
                100,
                &facts(&binding, 100, 1000),
                &facts(&binding, 10, 100)
            ),
            Err(HspRuntimeError::ExpiredAuthority)
        );
        assert!(expired.pending_play.is_none());
        let mut foreign = runtime(binding.clone());
        assert_eq!(
            foreign.request_play_original(
                20,
                &facts(&binding, 20, 1000),
                &facts(&super::original_play_tests::binding(2), 10, 100)
            ),
            Err(HspRuntimeError::ForeignAuthority)
        );
        assert!(foreign.pending_play.is_none());
    }
}
