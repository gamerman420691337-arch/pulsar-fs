//! Owned, one-job Handy HTTP dispatch. Public Engine playback remains gated.
//! Callers supply fresh Engine authority; receipt bindings alone are not authority.
use super::*;
use crate::device_effect_journal::{EffectReceiptBinding, PreparedEffectReceipt};
use crate::handy_wire::{self, HsspObservation, RemotePlayState};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
const MAX_IO_BUDGET: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchFailure {
    Busy,
    InvalidBudget,
    Credentials,
    BindingMismatch,
    Cancelled,
    WorkerUnavailable,
    Authority(PlaybackFailure),
}

/// Engine-only refreshed facts. Acquisition must recheck grants and qualification.
/// All ticks are in the same suspend-aware host clock domain as the controller.
/// The original lease deadline is never extended by a queue or transport budget.
pub struct DispatchAuthority {
    pub admission: PlaybackAdmission,
    pub controller: Option<ClientId>,
    pub clock_domain: ArtifactId,
    pub observed_tick_ns: u64,
    pub lease_deadline_ns: u64,
}

type HostClock = Arc<dyn Fn() -> Result<u64, DispatchFailure> + Send + Sync>;

/// Absolute source/lease/command limit. Instant only bounds a local I/O wait;
/// it must not interpolate the suspend-aware clock or renew this window.
struct DispatchWindow {
    last_tick_ns: u64,
    expires_ns: u64,
    source: PlaybackClock,
    timing: QualifiedCommandTiming,
}
impl DispatchWindow {
    fn remaining(&mut self, clock: &HostClock) -> Result<Duration, DispatchFailure> {
        let now = clock()?;
        if now < self.last_tick_ns {
            return Err(DispatchFailure::Authority(PlaybackFailure::InvalidClock));
        }
        self.last_tick_ns = now;
        self.timing
            .checked_clock_age(&self.source, &self.source.domain, now)
            .map_err(DispatchFailure::Authority)?;
        let remaining = self.expires_ns.checked_sub(now).filter(|n| *n > 0);
        remaining
            .map(Duration::from_nanos)
            .ok_or(DispatchFailure::Authority(PlaybackFailure::StaleCommand))
    }
}

/// Exact loaded-program and host-session binding, not a qualification constructor.
pub struct TransportBinding {
    pub admission: PlaybackAdmission,
    pub session: DeviceSessionId,
    pub stream_id: u64,
}

/// Credential values stay in native memory; no URL, redirect or proxy is supplied
/// by callers. Construction does not contact a device or authorize a command.
pub struct HandyTransport {
    binding: TransportBinding,
    origin: String,
    credentials: Arc<crate::handy_credentials::Credentials>,
}
impl HandyTransport {
    #[cfg(test)]
    pub(crate) fn set_test_origin(&mut self, origin: String) {
        assert!(origin.starts_with("http://127.0.0.1:"));
        self.origin = origin;
    }
    pub fn from_environment(
        binding: TransportBinding,
        api_key_env: &str,
        connection_key_env: &str,
    ) -> Result<Self, DispatchFailure> {
        let credentials =
            crate::handy_credentials::Credentials::from_environment(&HandyInspectionRequest {
                api_key_env: api_key_env.into(),
                connection_key_env: connection_key_env.into(),
                allow_online_inspection: true,
            })
            .map_err(|_| DispatchFailure::Credentials)?;
        Self::from_snapshot(binding, Arc::new(credentials))
    }
    pub(crate) fn from_snapshot(
        binding: TransportBinding,
        credentials: Arc<crate::handy_credentials::Credentials>,
    ) -> Result<Self, DispatchFailure> {
        let transport = Self {
            binding,
            origin: ORIGIN.into(),
            credentials,
        };
        transport.validate()?;
        Ok(transport)
    }
    pub(crate) fn preparation_matches(
        &self,
        admission: &PlaybackAdmission,
        session: &DeviceSessionId,
        stream_id: u64,
        credentials: &Arc<crate::handy_credentials::Credentials>,
    ) -> bool {
        self.binding.admission == *admission
            && self.binding.session == *session
            && self.binding.stream_id == stream_id
            && Arc::ptr_eq(&self.credentials, credentials)
    }
    fn validate(&self) -> Result<(), DispatchFailure> {
        if self.binding.stream_id > (1u64 << 53) - 1 {
            return Err(DispatchFailure::BindingMismatch);
        }
        self.credentials
            .validate()
            .map_err(|_| DispatchFailure::Credentials)?;
        Ok(())
    }
    fn matches(&self, effect: &PlaybackEffect) -> bool {
        self.binding.admission == effect.admission && self.binding.session == effect.session
    }
    fn execute(
        &self,
        effect: &PlaybackEffect,
        mut deadline: Instant,
        mut window: Option<DispatchWindow>,
        clock: &HostClock,
        cancelled: &AtomicBool,
    ) -> Option<HsspObservation> {
        let request = effect.request();
        let expected_path = if effect.is_stop() {
            "/hssp/stop"
        } else {
            "/hssp/play"
        };
        if request.method() != "PUT" || request.path() != expected_path {
            return None;
        }
        if let Some(window) = &mut window {
            // Sample Instant before the suspend-aware clock so conversion cannot
            // add the intervening elapsed time to the absolute lifetime.
            let now = Instant::now();
            let remaining = window.remaining(clock).ok()?;
            deadline = deadline.min(now.checked_add(remaining)?);
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() {
            return None;
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .proxy(None)
            .build()
            .into();
        let mut builder = agent
            .put(format!("{}{}", self.origin, expected_path))
            .header("X-Api-Key", &self.credentials.api)
            .header("X-Connection-Key", &self.credentials.connection)
            .header("Accept", "application/json");
        // Redact custom authentication headers in dependency diagnostics too.
        for name in ["X-Api-Key", "X-Connection-Key"] {
            builder.headers_mut()?.get_mut(name)?.set_sensitive(true);
        }
        // Recheck after request construction too. A suspend after the claim
        // cannot turn a still-live Instant budget into fresh play authority.
        if let Some(window) = &mut window {
            let now = Instant::now();
            let remaining = window.remaining(clock).ok()?;
            deadline = deadline.min(now.checked_add(remaining)?);
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() || cancelled.load(Ordering::Acquire) {
            return None;
        }
        let builder = builder.config().timeout_global(Some(remaining)).build();
        let mut response = match request.body() {
            Some(bytes) => builder
                .header("Content-Type", "application/json")
                .send(bytes),
            None => builder.send_empty(),
        }
        .ok()?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(handy_wire::RESPONSE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if Instant::now() >= deadline {
            return None;
        }
        if let Some(window) = &mut window {
            window.remaining(clock).ok()?;
        }
        let observation = handy_wire::decode_hssp(status, &bytes).ok()?;
        if self
            .credentials
            .echoed_text(&serde_json::to_string(&observation).ok()?)
        {
            return None;
        }
        let expected_state = if effect.is_stop() {
            RemotePlayState::Stopped
        } else {
            RemotePlayState::Playing
        };
        (observation.stream_id == self.binding.stream_id
            && observation.play_state == expected_state
            && observation.playback_rate == 1.0
            && !observation.looping)
            .then_some(observation)
    }
}

/// Not cloneable. A rejected submission returns ownership to its caller.
pub enum DispatchRequest {
    Play {
        effect: PlaybackEffect,
        receipt: PreparedEffectReceipt,
    },
    Stop(PreparedStopEffect),
    EmergencyStop(Box<EmergencyStopEffect>),
}
impl DispatchRequest {
    fn parts(&self) -> Option<(&PlaybackEffect, &PreparedEffectReceipt)> {
        match self {
            Self::Play { effect, receipt } => Some((effect, receipt)),
            Self::Stop(stop) => Some((&stop.effect, &stop.receipt)),
            Self::EmergencyStop(_) => None,
        }
    }
    pub(crate) fn effect(&self) -> &PlaybackEffect {
        match self {
            Self::Play { effect, .. } => effect,
            Self::Stop(stop) => &stop.effect,
            Self::EmergencyStop(stop) => &stop.effect,
        }
    }
}
pub struct StartFailure {
    pub reason: DispatchFailure,
    pub request: DispatchRequest,
}

#[derive(Debug)]
pub enum DispatchAttempt {
    /// No claim or network operation was made. Authority is not changed here.
    Rejected(DispatchFailure),
    /// The claim is consumed, including uncertain I/O or post-claim cancellation.
    Observed {
        outcome: DispatchOutcome,
        response: Option<HsspObservation>,
    },
}
#[must_use = "retain the completion and handle its observation or rejection"]
pub struct DispatchCompletion {
    pub request: DispatchRequest,
    pub attempt: DispatchAttempt,
}
pub enum CompletionObservation {
    Play(Box<PlayCompletion>),
    Stop(Box<EffectCompletionRecord>),
    EmergencyStop(Box<EmergencyStopObservation>),
}
impl DispatchCompletion {
    /// Preserve an attempted transport result as history even after revocation
    /// or a newer stop made its runtime callback stale. This never changes live
    /// state, constructs a stop, or turns a pre-claim refusal into an outcome.
    pub fn historical_observation(
        &self,
        binding: &EffectReceiptBinding,
    ) -> Result<EffectCompletionRecord, DispatchFailure> {
        let outcome = match &self.attempt {
            DispatchAttempt::Rejected(reason) => return Err(reason.clone()),
            DispatchAttempt::Observed { outcome, .. } => *outcome,
        };
        let (effect, receipt) = self
            .request
            .parts()
            .ok_or(DispatchFailure::BindingMismatch)?;
        binding
            .validate_effect_receipt(effect, receipt)
            .map_err(|_| DispatchFailure::Authority(PlaybackFailure::JournalUnavailable))?;
        Ok(EffectCompletionRecord {
            effect: effect.clone(),
            receipt: receipt.clone(),
            outcome,
            recorded: false,
        })
    }
    pub(crate) fn historical_emergency_observation(
        &self,
        binding: &EffectReceiptBinding,
    ) -> Result<EmergencyStopObservation, DispatchFailure> {
        let outcome = match &self.attempt {
            DispatchAttempt::Rejected(reason) => return Err(reason.clone()),
            DispatchAttempt::Observed { outcome, .. } => *outcome,
        };
        let DispatchRequest::EmergencyStop(stop) = &self.request else {
            return Err(DispatchFailure::BindingMismatch);
        };
        stop.observation(outcome, binding)
            .map_err(DispatchFailure::Authority)
    }
    /// Observe under control ownership, then release it before metadata writes.
    /// A rejected attempt is not a callback and cannot retire another worker's claim.
    pub fn observe(
        &self,
        runtime: &mut HandyPlaybackRuntime,
        binding: &EffectReceiptBinding,
    ) -> Result<CompletionObservation, DispatchFailure> {
        let outcome = match self.attempt {
            DispatchAttempt::Rejected(ref reason) => return Err(reason.clone()),
            DispatchAttempt::Observed { outcome, .. } => outcome,
        };
        match &self.request {
            DispatchRequest::Play { effect, receipt } => runtime
                .observe_play_completion(effect, receipt, outcome, binding)
                .map(|value| CompletionObservation::Play(Box::new(value))),
            DispatchRequest::Stop(stop) => runtime
                .observe_stop_completion(stop, outcome, binding)
                .map(|value| CompletionObservation::Stop(Box::new(value))),
            DispatchRequest::EmergencyStop(stop) => runtime
                .observe_emergency_stop_completion(stop, outcome, binding)
                .map(|value| CompletionObservation::EmergencyStop(Box::new(value))),
        }
        .map_err(DispatchFailure::Authority)
    }
}

struct Active {
    request: Arc<DispatchRequest>,
    worker: JoinHandle<DispatchAttempt>,
    cancel: Arc<AtomicBool>,
    claimed: Arc<AtomicBool>,
}

/// A single-use continuation. The authorizer invokes it while still holding
/// its dependency-mutation guard. All such guards are released before HTTP.
pub struct ClaimGate<'a> {
    runtime: &'a Arc<Mutex<HandyPlaybackRuntime>>,
    binding: &'a EffectReceiptBinding,
    transport: &'a HandyTransport,
    request: &'a DispatchRequest,
    clock: &'a HostClock,
    deadline: Instant,
    cancelled: &'a AtomicBool,
    claimed: &'a Arc<AtomicBool>,
}
/// Only ClaimGate can create a handoff. Returning bare checked facts cannot
/// release the authority guard before the runtime claim.
pub struct ClaimedHandoff {
    deadline: Instant,
    window: Option<DispatchWindow>,
    claim_owner: Arc<AtomicBool>,
}
impl ClaimGate<'_> {
    pub fn apply(self, authority: DispatchAuthority) -> Result<ClaimedHandoff, DispatchFailure> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(DispatchFailure::Cancelled);
        }
        let mut runtime = self.runtime.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => DispatchFailure::Busy,
            std::sync::TryLockError::Poisoned(_) => DispatchFailure::WorkerUnavailable,
        })?;
        let now = Instant::now();
        if now >= self.deadline {
            return Err(DispatchFailure::InvalidBudget);
        }
        let effect = self.request.effect();
        if !self.transport.matches(effect) {
            return Err(DispatchFailure::BindingMismatch);
        }
        let tick = if effect.is_stop() {
            None
        } else {
            Some((self.clock)()?)
        };
        let window = if let DispatchRequest::EmergencyStop(stop) = self.request {
            stop.validate(self.binding)
                .map_err(DispatchFailure::Authority)?;
            if !runtime.effect_is_current(effect) || runtime.claimed_stop == Some(effect.sequence) {
                return Err(DispatchFailure::Authority(PlaybackFailure::RetiredCommand));
            }
            if authority.admission.configuration != effect.admission.configuration {
                return Err(DispatchFailure::Authority(
                    PlaybackFailure::AdmissionChanged,
                ));
            }
            runtime.claimed_stop = Some(effect.sequence);
            None
        } else {
            let (_, receipt) = self.request.parts().expect("durable dispatch variant");
            claim(
                &mut runtime,
                effect,
                receipt,
                self.binding,
                &authority,
                tick,
            )?
        };
        self.claimed.store(true, Ordering::Release);
        let deadline = window.as_ref().map_or(self.deadline, |w| {
            now.checked_add(Duration::from_nanos(w.expires_ns - w.last_tick_ns))
                .map_or(self.deadline, |end| self.deadline.min(end))
        });
        Ok(ClaimedHandoff {
            deadline,
            window,
            claim_owner: self.claimed.clone(),
        })
    }
}

/// One owned job, no queue and no automatic retry. Reap the previous completion
/// before starting another job. Shutdown retains the result; Drop only joins,
/// leaving an unread outcome unresolved in the original prepared journal row.
pub struct HandyDispatcher {
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    binding: EffectReceiptBinding,
    transport: Arc<HandyTransport>,
    clock: HostClock,
    active: Option<Active>,
}
impl HandyDispatcher {
    #[cfg(test)]
    pub(crate) fn set_test_clock(
        &mut self,
        clock: Arc<dyn Fn() -> Result<u64, ProtocolError> + Send + Sync>,
    ) {
        self.clock = Arc::new(move || {
            clock().map_err(|_| DispatchFailure::Authority(PlaybackFailure::InvalidClock))
        });
    }
    pub fn new(
        runtime: Arc<Mutex<HandyPlaybackRuntime>>,
        binding: EffectReceiptBinding,
        transport: HandyTransport,
    ) -> Self {
        Self::from_shared(runtime, binding, Arc::new(transport))
    }
    pub(crate) fn from_shared(
        runtime: Arc<Mutex<HandyPlaybackRuntime>>,
        binding: EffectReceiptBinding,
        transport: Arc<HandyTransport>,
    ) -> Self {
        Self {
            runtime,
            binding,
            transport,
            clock: Arc::new(|| {
                crate::handy_clock::boottime_ns()
                    .map_err(|_| DispatchFailure::Authority(PlaybackFailure::InvalidClock))
            }),
            active: None,
        }
    }
    pub fn start<F>(
        &mut self,
        request: DispatchRequest,
        deadline: Instant,
        authorize: F,
    ) -> Result<(), Box<StartFailure>>
    where
        F: FnOnce(Instant, &AtomicBool, ClaimGate<'_>) -> Result<ClaimedHandoff, DispatchFailure>
            + Send
            + 'static,
    {
        let refusal = if self.active.is_some() {
            Some(DispatchFailure::Busy)
        } else if deadline
            .checked_duration_since(Instant::now())
            .is_none_or(|d| d.is_zero() || d > MAX_IO_BUDGET)
        {
            Some(DispatchFailure::InvalidBudget)
        } else {
            self.transport.validate().err()
        };
        if let Some(reason) = refusal {
            return Err(Box::new(StartFailure { reason, request }));
        }
        let request = Arc::new(request);
        let captured = request.clone();
        let runtime = self.runtime.clone();
        let binding = self.binding.clone();
        let transport = self.transport.clone();
        let clock = self.clock.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let claimed = Arc::new(AtomicBool::new(false));
        let claim_record = claimed.clone();
        let worker = thread::Builder::new()
            .name("pulsar-handy-dispatch".into())
            .spawn(move || {
                let prepare = || -> Result<ClaimedHandoff, DispatchFailure> {
                    if cancelled.load(Ordering::Acquire) {
                        return Err(DispatchFailure::Cancelled);
                    }
                    let handoff = authorize(
                        deadline,
                        &cancelled,
                        ClaimGate {
                            runtime: &runtime,
                            binding: &binding,
                            transport: &transport,
                            request: &captured,
                            clock: &clock,
                            deadline,
                            cancelled: &cancelled,
                            claimed: &claim_record,
                        },
                    )?;
                    if !Arc::ptr_eq(&handoff.claim_owner, &claim_record)
                        || !claim_record.load(Ordering::Acquire)
                    {
                        return Err(DispatchFailure::BindingMismatch);
                    }
                    Ok(handoff)
                };
                let ClaimedHandoff {
                    deadline, window, ..
                } = match prepare() {
                    Ok(prepared) => prepared,
                    Err(reason) => {
                        return if claim_record.load(Ordering::Acquire) {
                            DispatchAttempt::Observed {
                                outcome: DispatchOutcome::Unknown,
                                response: None,
                            }
                        } else {
                            DispatchAttempt::Rejected(reason)
                        }
                    }
                };
                let response = if cancelled.load(Ordering::Acquire) {
                    None
                } else {
                    transport.execute(captured.effect(), deadline, window, &clock, &cancelled)
                };
                let response = if cancelled.load(Ordering::Acquire) {
                    None
                } else {
                    response
                };
                let outcome = if response.is_some() {
                    DispatchOutcome::Acknowledged
                } else {
                    DispatchOutcome::Unknown
                };
                DispatchAttempt::Observed { outcome, response }
            });
        match worker {
            Ok(worker) => {
                self.active = Some(Active {
                    request,
                    worker,
                    cancel,
                    claimed,
                });
                Ok(())
            }
            Err(_) => Err(Box::new(StartFailure {
                reason: DispatchFailure::WorkerUnavailable,
                request: unwrap_request(request),
            })),
        }
    }
    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Release);
        }
    }
    pub fn poll(&mut self) -> Option<DispatchCompletion> {
        if !self.active.as_ref()?.worker.is_finished() {
            return None;
        }
        self.join()
    }
    fn join(&mut self) -> Option<DispatchCompletion> {
        let active = self.active.take()?;
        let attempt = active.worker.join().unwrap_or_else(|_| {
            if active.claimed.load(Ordering::Acquire) {
                DispatchAttempt::Observed {
                    outcome: DispatchOutcome::Unknown,
                    response: None,
                }
            } else {
                DispatchAttempt::Rejected(DispatchFailure::WorkerUnavailable)
            }
        });
        Some(DispatchCompletion {
            request: unwrap_request(active.request),
            attempt,
        })
    }
    /// Cancellation does not prove remote cancellation or stopping. The owner
    /// must still observe the result and preserve any required stop intent.
    pub fn shutdown(&mut self) -> Option<DispatchCompletion> {
        self.cancel();
        self.join()
    }
}
impl Drop for HandyDispatcher {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn unwrap_request(request: Arc<DispatchRequest>) -> DispatchRequest {
    match Arc::try_unwrap(request) {
        Ok(request) => request,
        Err(_) => panic!("joined dispatch retained its request"),
    }
}

fn claim(
    runtime: &mut HandyPlaybackRuntime,
    effect: &PlaybackEffect,
    receipt: &PreparedEffectReceipt,
    binding: &EffectReceiptBinding,
    authority: &DispatchAuthority,
    tick: Option<u64>,
) -> Result<Option<DispatchWindow>, DispatchFailure> {
    let reject = DispatchFailure::Authority;
    if !runtime.effect_is_current(effect) {
        return Err(reject(PlaybackFailure::RetiredCommand));
    }
    binding
        .validate_effect_receipt(effect, receipt)
        .map_err(|_| reject(PlaybackFailure::JournalUnavailable))?;
    if authority.admission.configuration != effect.admission.configuration {
        return Err(reject(PlaybackFailure::AdmissionChanged));
    }
    let remaining = if effect.is_stop() {
        None
    } else {
        if authority.admission != effect.admission
            || authority.controller.as_ref() != runtime.session.controller()
        {
            return Err(reject(PlaybackFailure::AdmissionChanged));
        }
        let tick = tick.ok_or_else(|| reject(PlaybackFailure::InvalidClock))?;
        if tick < authority.observed_tick_ns {
            return Err(reject(PlaybackFailure::InvalidClock));
        }
        let clock = effect
            .clock
            .as_ref()
            .ok_or_else(|| reject(PlaybackFailure::InvalidClock))?;
        let timing = runtime
            .timing
            .for_reference(
                effect
                    .reference
                    .ok_or_else(|| reject(PlaybackFailure::InvalidClock))?,
            )
            .map_err(reject)?;
        let age = timing
            .checked_clock_age(clock, &authority.clock_domain, tick)
            .map_err(reject)?;
        let expires_ns = tick
            .saturating_add(timing.max_age_ns - age)
            .min(clock.valid_until_ns)
            .min(authority.lease_deadline_ns);
        if tick >= expires_ns {
            return Err(reject(PlaybackFailure::StaleCommand));
        }
        Some(DispatchWindow {
            last_tick_ns: tick,
            expires_ns,
            source: clock.clone(),
            timing,
        })
    };
    let claimed = if effect.is_stop() {
        &mut runtime.claimed_stop
    } else {
        &mut runtime.claimed_play
    };
    if *claimed == Some(effect.sequence) {
        return Err(reject(PlaybackFailure::RetiredCommand));
    }
    *claimed = Some(effect.sequence);
    Ok(remaining)
}

#[cfg(test)]
mod tests;
