//! One-operation native HSP transport. This module does not admit public streams.
//!
//! Read preflight binds the observed device session, mode generation and stream.
//! HSP has no server-side expected-stream precondition: another controller can
//! replace the stream after that read. Sole-controller qualification remains an
//! external requirement; these checks do not establish an atomic device lease.
use crate::{
    device_effect_journal::{ClaimedHspReceipt, DeviceEffectJournal, PreparedHspReceipt},
    handy_credentials::{Credentials, HandyRouteIdentity},
    handy_hsp_runtime::{
        HandyHspRuntime, HspBinding, HspCompletionRecord, HspDispatchFacts, HspEffect,
        HspEffectKind, HspRuntimeError, HspSettledTransport, NativeHspProbeReservation,
        NativeHspReservation, NativeHspTransportIdentity,
    },
    handy_hsp_wire::{self, HspPlayState, HspState},
    handy_wire::{self, DeviceIdentity},
};
use pulsar_core::HspPolicy;
use pulsar_protocol::{HandyDeviceMode, HandyIdentity, HandyInspectionRequest, HandyRegistration};
use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, TryLockError,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
const MAX_BUDGET: Duration = Duration::from_secs(10);
const MAX_REQUEST: usize = 16 * 1024;
const PRECLAIM_WAIT_CAP: Duration = Duration::from_millis(100);
const PRECLAIM_RETRY_DELAY: Duration = Duration::from_millis(1);
type HostClock = Arc<dyn Fn() -> Result<u64, HspDispatchFailure> + Send + Sync>;

/// Errors intentionally contain no URLs, credentials or peer-controlled text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HspDispatchFailure {
    Busy,
    ConsentRequired,
    InvalidBudget,
    InvalidClock,
    Credentials,
    BindingMismatch,
    RemoteMismatch,
    InvalidResponse,
    Transport,
    Journal,
    Cancelled,
    WorkerUnavailable,
    Authority(HspRuntimeError),
}

/// Setup replaces a stream. Require an explicit previously observed idle state,
/// including zero for a genuinely uninitialized remote stream.
#[derive(Clone, Debug)]
pub struct HspSetupPreviousState {
    stream_id: u32,
    play_state: HspPlayState,
}
impl HspSetupPreviousState {
    pub fn new(stream_id: u32, play_state: HspPlayState) -> Result<Self, HspDispatchFailure> {
        if !matches!(
            play_state,
            HspPlayState::NotInitialized | HspPlayState::Stopped
        ) {
            return Err(HspDispatchFailure::RemoteMismatch);
        }
        Ok(Self {
            stream_id,
            play_state,
        })
    }
}

/// Native verified configuration, never reconstructed from journal history.
/// Setup may replace the mode generation. After its acknowledgment, the owner
/// must separately observe and bind the new mode4 generation before Append/Play;
/// an old transport never silently adopts a new generation. This preparation and
/// authenticated admission are required integration work outside this module.
#[derive(Clone)]
pub struct HspTransportBinding {
    pub binding: HspBinding,
    pub registration: HandyRegistration,
    pub device: DeviceIdentity,
    pub mode: HandyDeviceMode,
    pub setup_previous: HspSetupPreviousState,
}
struct OwnerState {
    current: AtomicBool,
    ordinary: AtomicBool,
}
/// Retained native owner. Source/clock retirement disables ordinary operations
/// while preserving a bound Stop; replacing the native owner disables both.
#[derive(Clone)]
pub struct HspNativeOwner(Arc<OwnerState>);
impl HspNativeOwner {
    pub fn retire_ordinary(&self) {
        self.0.ordinary.store(false, Ordering::Release);
    }
    pub fn retire_owner(&self) {
        self.retire_ordinary();
        self.0.current.store(false, Ordering::Release);
    }
    fn permits(&self, effect: &HspEffect) -> bool {
        self.0.current.load(Ordering::Acquire)
            && (effect.kind() == HspEffectKind::Stop || self.0.ordinary.load(Ordering::Acquire))
    }
}

/// Borrowed request descriptor; ordinary effects retain their exact sealed
/// wire bytes, while native preflight reads carry no body.
struct HspHttpRequest<'a> {
    method: &'static str,
    path: &'static str,
    body: Option<&'a [u8]>,
}
impl<'a> HspHttpRequest<'a> {
    fn read(path: &'static str) -> Self {
        Self {
            method: "GET",
            path,
            body: None,
        }
    }
    fn effect(effect: &'a HspEffect) -> Self {
        Self {
            method: effect.request().method(),
            path: effect.request().path(),
            body: effect.request().body(),
        }
    }
}

/// Descriptive final-send observation. It never authorizes or suppresses Stop.
#[derive(Default)]
pub(crate) struct HspStopSendTracker {
    deadline_ns: AtomicU64,
    dispatched_at_ns: AtomicU64,
    missed: AtomicBool,
}
impl HspStopSendTracker {
    pub(crate) fn arm(&self, deadline_ns: u64) -> Result<(), HspDispatchFailure> {
        if deadline_ns == 0 {
            return Err(HspDispatchFailure::InvalidClock);
        }
        match self
            .deadline_ns
            .compare_exchange(0, deadline_ns, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(old) if old == deadline_ns => Ok(()),
            Err(_) => Err(HspDispatchFailure::BindingMismatch),
        }
    }
    fn sent(&self, now_ns: u64) {
        let deadline = self.deadline_ns.load(Ordering::Acquire);
        if deadline == 0 {
            return;
        }
        // Keep the first gate immutable. A later descriptive poll or duplicate
        // report cannot replace its timestamp or extend the original budget.
        let _ =
            self.dispatched_at_ns
                .compare_exchange(0, now_ns, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(crate) fn observe(&self, now_ns: u64) {
        let deadline = self.deadline_ns.load(Ordering::Acquire);
        if deadline != 0 && self.dispatched_at_ns.load(Ordering::Acquire) == 0 && now_ns > deadline
        {
            self.missed.store(true, Ordering::Release);
        }
    }
    pub(crate) fn status(&self) -> (Option<u64>, Option<u64>, bool) {
        let deadline = self.deadline_ns.load(Ordering::Acquire);
        let sent = self.dispatched_at_ns.load(Ordering::Acquire);
        (
            (deadline != 0).then_some(deadline),
            (sent != 0).then_some(sent),
            // A poll can run between the writer's clock sample and publication.
            // Once published, the actual first gate decides whether it was
            // late; until then this is an explicitly provisional unsent miss.
            if sent == 0 {
                self.missed.load(Ordering::Acquire)
            } else {
                sent > deadline
            },
        )
    }
}

/// Credentials remain in native memory. No configurable production origin,
/// redirect, proxy, automatic retry, or unrestricted peer body is exposed.
pub struct HandyHspTransport {
    instance: NativeHspTransportIdentity,
    binding: HspTransportBinding,
    owner: HspNativeOwner,
    credentials: Arc<Credentials>,
    route: HandyRouteIdentity,
    origin: String,
    stop_send: Option<Arc<HspStopSendTracker>>,
}
impl HandyHspTransport {
    pub fn from_environment(
        binding: HspTransportBinding,
        request: &HandyInspectionRequest,
        allow_online_control: bool,
        owner_capability: &str,
    ) -> Result<Self, HspDispatchFailure> {
        if !allow_online_control {
            return Err(HspDispatchFailure::ConsentRequired);
        }
        if request.api_key_env != binding.registration.settings.api_key_env
            || request.connection_key_env != binding.registration.settings.connection_key_env
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        let credentials =
            Credentials::from_environment(request).map_err(|_| HspDispatchFailure::Credentials)?;
        Self::from_snapshot(binding, Arc::new(credentials), true, owner_capability)
    }
    pub(crate) fn from_snapshot(
        binding: HspTransportBinding,
        credentials: Arc<Credentials>,
        allow_online_dispatch: bool,
        owner_capability: &str,
    ) -> Result<Self, HspDispatchFailure> {
        if !allow_online_dispatch {
            return Err(HspDispatchFailure::ConsentRequired);
        }
        credentials
            .validate()
            .map_err(|_| HspDispatchFailure::Credentials)?;
        let route = HandyRouteIdentity::from_snapshot(owner_capability, &credentials.connection)
            .map_err(|_| HspDispatchFailure::Credentials)?;
        // This initial transport only operates inside an already observed HSP
        // mode generation. Mode acquisition is a separate admission concern.
        if binding.registration.validate().is_err()
            || binding.registration.disabled
            || native_identity(&binding.device).validate().is_err()
            || binding.mode.mode != 4
            || binding.mode.validate().is_err()
            || binding.binding.identity().stream_id == 0
            || binding.device.session_id.is_empty()
            || binding.device.session_id.len() > 128
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        if credentials.echoed_text(&binding.device.session_id)
            || credentials.echoed_text(&binding.device.firmware)
            || credentials.echoed_text(&binding.device.model_name)
        {
            return Err(HspDispatchFailure::Credentials);
        }
        Ok(Self {
            instance: NativeHspTransportIdentity::new(),
            binding,
            credentials,
            route,
            origin: ORIGIN.into(),
            stop_send: None,
            owner: HspNativeOwner(Arc::new(OwnerState {
                current: AtomicBool::new(true),
                ordinary: AtomicBool::new(true),
            })),
        })
    }
    pub(crate) fn with_stop_send_tracker(mut self, tracker: Arc<HspStopSendTracker>) -> Self {
        self.stop_send = Some(tracker);
        self
    }
    pub fn owner(&self) -> HspNativeOwner {
        self.owner.clone()
    }
    #[cfg(test)]
    pub(crate) fn set_test_origin(&mut self, origin: String) {
        let address: std::net::SocketAddr =
            origin.strip_prefix("http://").unwrap().parse().unwrap();
        assert!(address.ip().is_loopback());
        self.origin = origin;
    }
    fn check(&self, effect: &HspEffect) -> Result<(), HspDispatchFailure> {
        if effect.binding() != &self.binding.binding || !self.owner.permits(effect) {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        let path = match effect.kind() {
            HspEffectKind::Setup => "/hsp/setup",
            HspEffectKind::Append => "/hsp/add",
            HspEffectKind::Play => "/hsp/play",
            HspEffectKind::Stop => "/hsp/stop",
        };
        if effect.request().method() != "PUT"
            || effect.request().path() != path
            || effect
                .request()
                .body()
                .is_some_and(|v| v.len() > MAX_REQUEST)
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        Ok(())
    }
    fn check_probe(&self, setup: &HspEffect) -> Result<(), HspDispatchFailure> {
        if setup.kind() != HspEffectKind::Setup
            || setup.binding() != &self.binding.binding
            || !self.owner.0.current.load(Ordering::Acquire)
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        Ok(())
    }
    fn http(
        &self,
        request: HspHttpRequest<'_>,
        effect: &HspEffect,
        window: &mut DispatchWindow,
        clock: &HostClock,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, HspDispatchFailure> {
        self.http_bound(request, effect, window, clock, cancelled, false)
    }
    fn http_bound(
        &self,
        request: HspHttpRequest<'_>,
        effect: &HspEffect,
        window: &mut DispatchWindow,
        clock: &HostClock,
        cancelled: &AtomicBool,
        probe: bool,
    ) -> Result<Vec<u8>, HspDispatchFailure> {
        let check = || {
            if probe {
                self.check_probe(effect)
            } else {
                self.check(effect)
            }
        };
        let HspHttpRequest { method, path, body } = request;
        if probe && (method != "GET" || body.is_some()) {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        check()?;
        let remaining = window.remaining(clock, cancelled)?;
        // A fresh agent for every request avoids pooled-connection recovery.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .proxy(None)
            .http_status_as_error(false)
            .max_idle_connections(0)
            .build()
            .into();
        let uri = format!("{}{}", self.origin, path);
        let mut request = match method {
            "GET" => agent.get(uri).force_send_body(),
            "PUT" => agent.put(uri),
            _ => return Err(HspDispatchFailure::BindingMismatch),
        }
        .header("X-Api-Key", &self.credentials.api)
        .header("X-Connection-Key", &self.credentials.connection)
        .header("Accept", "application/json");
        for name in ["X-Api-Key", "X-Connection-Key"] {
            request
                .headers_mut()
                .and_then(|h| h.get_mut(name))
                .ok_or(HspDispatchFailure::Credentials)?
                .set_sensitive(true);
        }
        check()?;
        let remaining = window.remaining(clock, cancelled)?;
        let request = request.config().timeout_global(Some(remaining)).build();
        if method == "PUT" && path == "/hsp/stop" && effect.kind() == HspEffectKind::Stop {
            // Last native gate before entering the first HTTP write attempt;
            // connection/preflight delays do not reset the original deadline.
            let at = window.tick(clock, cancelled)?;
            if let Some(tracker) = &self.stop_send {
                tracker.sent(at);
            }
        }
        let response = match body {
            Some(body) => request
                .header("Content-Type", "application/json")
                .send(body),
            None => request.send_empty(),
        };
        window.remaining(clock, cancelled)?;
        let mut response = response.map_err(|_| HspDispatchFailure::Transport)?;
        if response.status().as_u16() != 200 {
            return Err(HspDispatchFailure::InvalidResponse);
        }
        let mut bytes = Vec::new();
        let read = response
            .body_mut()
            .as_reader()
            .take(handy_hsp_wire::HSP_RESPONSE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes);
        window.remaining(clock, cancelled)?;
        read.map_err(|_| HspDispatchFailure::Transport)?;
        check()?;
        if bytes.len() > handy_hsp_wire::HSP_RESPONSE_LIMIT
            || std::str::from_utf8(&bytes).map_or(true, |text| self.credentials.echoed_text(text))
        {
            return Err(HspDispatchFailure::InvalidResponse);
        }
        Ok(bytes)
    }
    fn preflight(
        &self,
        effect: &HspEffect,
        window: &mut DispatchWindow,
        clock: &HostClock,
        cancel: &AtomicBool,
    ) -> Result<(), HspDispatchFailure> {
        let info = self.http(HspHttpRequest::read("/info"), effect, window, clock, cancel)?;
        let info =
            handy_wire::decode_info(200, &info).map_err(|_| HspDispatchFailure::InvalidResponse)?;
        if info != self.binding.device {
            return Err(HspDispatchFailure::RemoteMismatch);
        }
        let mode = self.http(HspHttpRequest::read("/mode"), effect, window, clock, cancel)?;
        let mode =
            handy_wire::decode_mode(200, &mode).map_err(|_| HspDispatchFailure::InvalidResponse)?;
        if mode != self.binding.mode {
            return Err(HspDispatchFailure::RemoteMismatch);
        }
        let state = self.http(
            HspHttpRequest::read("/hsp/state"),
            effect,
            window,
            clock,
            cancel,
        )?;
        let state = handy_hsp_wire::decode_state(200, &state)
            .map_err(|_| HspDispatchFailure::InvalidResponse)?;
        let matches = if effect.kind() == HspEffectKind::Setup {
            state.stream_id == self.binding.setup_previous.stream_id
                && state.play_state == self.binding.setup_previous.play_state
        } else {
            state.stream_id == effect.binding().identity().stream_id
        };
        let state_matches = match effect.kind() {
            HspEffectKind::Setup | HspEffectKind::Stop => true,
            HspEffectKind::Play => state.play_state == HspPlayState::Stopped,
            HspEffectKind::Append if effect.expected_running() => {
                state.play_state == HspPlayState::Playing
                    && !state.looping
                    && state.playback_rate == 1.0
                    && !state.pause_on_starving
            }
            HspEffectKind::Append => state.play_state == HspPlayState::Stopped,
        };
        if !matches || !state_matches {
            return Err(HspDispatchFailure::RemoteMismatch);
        }
        Ok(())
    }
}

fn native_identity(device: &DeviceIdentity) -> HandyIdentity {
    HandyIdentity {
        firmware: device.firmware.clone(),
        model_number: device.model_number,
        model_name: device.model_name.clone(),
        model_variant: device.model_variant,
        session_id: device.session_id.clone(),
        firmware_status: device.firmware_status,
    }
}

struct OrdinaryRetirementFlags {
    stopped: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
}
struct DispatchWindow {
    retirement: Option<OrdinaryRetirementFlags>,
    deadline: Instant,
    last_ns: u64,
    expires_ns: u64,
    original: Option<HspDispatchFacts>,
    current: Option<HspDispatchFacts>,
}
impl DispatchWindow {
    fn tick(
        &mut self,
        clock: &HostClock,
        cancelled: &AtomicBool,
    ) -> Result<u64, HspDispatchFailure> {
        if cancelled.load(Ordering::Acquire)
            || self.retirement.as_ref().is_some_and(|flags| {
                flags.stopped.load(Ordering::Acquire) || flags.shutdown.load(Ordering::Acquire)
            })
        {
            return Err(HspDispatchFailure::Cancelled);
        }
        let now = clock()?;
        if now < self.last_ns || now >= self.expires_ns || Instant::now() >= self.deadline {
            return Err(HspDispatchFailure::InvalidClock);
        }
        self.last_ns = now;
        for facts in self.original.iter().chain(self.current.iter()) {
            facts.valid_at(now).map_err(HspDispatchFailure::Authority)?;
        }
        Ok(now)
    }
    fn remaining(
        &mut self,
        clock: &HostClock,
        cancelled: &AtomicBool,
    ) -> Result<Duration, HspDispatchFailure> {
        let instant = Instant::now();
        let now = self.tick(clock, cancelled)?;
        let remaining = self
            .deadline
            .checked_duration_since(instant)
            .ok_or(HspDispatchFailure::InvalidBudget)?
            .min(Duration::from_nanos(self.expires_ns - now));
        if remaining.is_zero() {
            return Err(HspDispatchFailure::InvalidBudget);
        }
        Ok(remaining)
    }
    fn bind(
        &mut self,
        effect: &HspEffect,
        facts: Option<HspDispatchFacts>,
        policy: HspPolicy,
    ) -> Result<(), HspDispatchFailure> {
        if effect.kind() == HspEffectKind::Stop {
            return Ok(());
        }
        let facts = facts.ok_or(HspDispatchFailure::Authority(
            HspRuntimeError::ExpiredAuthority,
        ))?;
        for source in effect.original_facts().into_iter().chain(Some(&facts)) {
            self.expires_ns = self
                .expires_ns
                .min(source.window().lease_until_ns())
                .min(source.window().source_until_ns())
                .min(source.window().clock_until_ns());
        }
        if let Some(chunk) = effect.chunk() {
            for sample in chunk.samples() {
                self.expires_ns = self.expires_ns.min(
                    sample
                        .captured_at_ns
                        .saturating_add(policy.max_sample_age_ns)
                        .saturating_add(1),
                );
                let lead_ms = i64::from(sample.point.time_ms)
                    - facts.reference().phase().latest_ms
                    - i64::from(policy.min_lead_ms);
                let lead_ns = u64::try_from(lead_ms)
                    .map_err(|_| HspDispatchFailure::InvalidClock)?
                    .checked_mul(1_000_000)
                    .ok_or(HspDispatchFailure::InvalidClock)?;
                self.expires_ns = self.expires_ns.min(
                    facts
                        .reference()
                        .at_ns()
                        .saturating_add(lead_ns)
                        .saturating_add(1),
                );
            }
        }
        self.current = Some(facts);
        Ok(())
    }
}

/// Not Clone: a refusal returns the original request; a completion retains it.
pub struct HspDispatchRequest {
    pub effect: HspEffect,
    pub receipt: PreparedHspReceipt,
}
pub struct HspStartFailure {
    pub reason: HspDispatchFailure,
    pub request: HspDispatchRequest,
}
#[derive(Debug)]
pub enum HspDispatchAttempt {
    Rejected(HspDispatchFailure),
    /// The exact native attempt was consumed. Durable dispatch also retains its
    /// journal claim; emergency dispatch does not create one. None is Unknown.
    Observed {
        response: Option<HspState>,
    },
}
#[must_use = "retain the exact effect and claim for completion and metadata retry"]
pub struct HspDispatchCompletion {
    pub request: HspDispatchRequest,
    pub claim: Option<ClaimedHspReceipt>,
    pub attempt: HspDispatchAttempt,
}
impl HspDispatchCompletion {
    /// Called only after the worker has joined. Response bytes are reconstructed
    /// from numeric codec fields; no arbitrary peer text crosses this boundary.
    pub fn observe(
        &self,
        runtime: &mut HandyHspRuntime,
        now_ns: u64,
        facts: Option<&HspDispatchFacts>,
    ) -> Result<HspCompletionRecord, HspDispatchFailure> {
        let response = match &self.attempt {
            HspDispatchAttempt::Rejected(error) => return Err(error.clone()),
            HspDispatchAttempt::Observed { response } => response,
        };
        let bytes = response.map(state_bytes);
        runtime
            .complete_settled(
                &self.request.effect,
                match bytes.as_deref() {
                    Some(body) => HspSettledTransport::Response { status: 200, body },
                    None => HspSettledTransport::Unknown,
                },
                now_ns,
                facts,
            )
            .map_err(HspDispatchFailure::Authority)
    }
    /// Abandon the exact rejected operation only after the journal proves it
    /// was never claimed. A rejected duplicate worker is insufficient evidence:
    /// another worker may already hold the same durable claim. The journal's
    /// atomic cancellation wins against future claims before the runtime slot
    /// is retired. No Unknown or remote acknowledgment is manufactured here.
    pub fn abandon_unclaimed(
        &self,
        runtime: &mut HandyHspRuntime,
        journal: &mut DeviceEffectJournal,
    ) -> Result<(), HspDispatchFailure> {
        if !matches!(&self.attempt, HspDispatchAttempt::Rejected(_)) || self.claim.is_some() {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        if runtime.in_flight() != Some(&self.request.effect) {
            return Err(HspDispatchFailure::Authority(
                HspRuntimeError::ForeignEffect,
            ));
        }
        runtime
            .require_native_released()
            .map_err(HspDispatchFailure::Authority)?;
        let proof = journal
            .try_cancel_hsp_effect(&self.request.receipt, &self.request.effect)
            .map_err(|_| HspDispatchFailure::Journal)?;
        runtime
            .retire_unclaimed(&self.request.effect, &proof)
            .map_err(HspDispatchFailure::Authority)
    }
    /// Metadata-only retry. This method has no transport handle and cannot send.
    pub fn persist(
        &self,
        journal: &mut DeviceEffectJournal,
        record: &HspCompletionRecord,
    ) -> Result<(), HspDispatchFailure> {
        let claim = self.claim.as_ref().ok_or(HspDispatchFailure::Journal)?;
        if record.effect != self.request.effect {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        journal
            .try_complete_hsp(claim, record)
            .map(|_| ())
            .map_err(|_| HspDispatchFailure::Journal)
    }
}
fn state_bytes(state: HspState) -> Vec<u8> {
    let play_state = match state.play_state {
        HspPlayState::NotInitialized => 0,
        HspPlayState::Playing => 1,
        HspPlayState::Stopped => 2,
        HspPlayState::Paused => 3,
        HspPlayState::Starving => 4,
    };
    serde_json::to_vec(&serde_json::json!({"result": {
        "play_state":play_state,"points":state.points,"max_points":state.max_points,
        "current_point":state.current_point,"current_time":state.current_time,
        "loop":state.looping,"playback_rate":state.playback_rate,
        "first_point_time":state.first_point_time,"last_point_time":state.last_point_time,
        "stream_id":state.stream_id,"tail_point_stream_index":state.tail_point_stream_index,
        "tail_point_stream_index_threshold":state.tail_point_stream_index_threshold,
        "pause_on_starving":state.pause_on_starving
    }}))
    .expect("finite checked HSP state")
}

struct ClaimCustody {
    reservation: NativeHspReservation,
    attempted: bool,
    claim: Option<ClaimedHspReceipt>,
}
pub(crate) type HspFactsValidator =
    Arc<dyn Fn(u64, &HspDispatchFacts) -> Result<(), HspDispatchFailure> + Send + Sync>;
/// Worker-bound, consumed continuation invoked while caller dependency guards
/// remain held. The worker obtains no Engine and releases every lock before HTTP.
pub struct HspClaimGate<'a> {
    runtime: &'a Arc<Mutex<HandyHspRuntime>>,
    journal: &'a Arc<Mutex<DeviceEffectJournal>>,
    transport: &'a HandyHspTransport,
    request: &'a HspDispatchRequest,
    clock: &'a HostClock,
    cancel: &'a AtomicBool,
    custody: &'a Arc<Mutex<ClaimCustody>>,
    facts_validator: Option<HspFactsValidator>,
    #[cfg(test)]
    after_claim_hook: Option<Box<dyn FnOnce() + Send + 'static>>,
    window: DispatchWindow,
    preclaim_wait_until: Option<(Instant, u64)>,
}
/// Retry custody exists only for an unchanged, not-yet-attempted gate after
/// local mutex contention. A claimed or refused attempt cannot return a gate.
pub enum HspPreclaimProgress<'a> {
    Claimed(HspClaimedHandoff),
    Contended(HspClaimGate<'a>),
}
/// Only the worker's gate can construct its exact single-use handoff.
pub struct HspClaimedHandoff {
    custody: Arc<Mutex<ClaimCustody>>,
    window: DispatchWindow,
}
impl<'a> HspClaimGate<'a> {
    /// Recheck the original dispatch authority and any existing first-wait
    /// deadline. This checkpoint alone does not start a contention budget.
    pub fn check_preclaim_wait(
        &mut self,
        owner: &HspNativeOwner,
    ) -> Result<u64, HspDispatchFailure> {
        if !Arc::ptr_eq(&owner.0, &self.transport.owner.0) {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.transport.check(&self.request.effect)?;
        let now = self.window.tick(self.clock, self.cancel)?;
        if self
            .preclaim_wait_until
            .is_some_and(|(deadline, expires)| now >= expires || Instant::now() >= deadline)
        {
            return Err(HspDispatchFailure::InvalidBudget);
        }
        Ok(now)
    }

    /// Latch the first contention deadline once, then compute a bounded wait.
    /// Release dependency guards before parking for the returned duration;
    /// this method itself never parks.
    /// The cap and every original effect deadline survive moves between sites.
    pub fn preclaim_retry_delay(
        &mut self,
        owner: &HspNativeOwner,
    ) -> Result<Duration, HspDispatchFailure> {
        let started = Instant::now();
        let now = self.check_preclaim_wait(owner)?;
        if self.preclaim_wait_until.is_none() {
            self.preclaim_wait_until = Some((
                started
                    .checked_add(PRECLAIM_WAIT_CAP)
                    .ok_or(HspDispatchFailure::InvalidBudget)?,
                now.checked_add(100_000_000)
                    .ok_or(HspDispatchFailure::InvalidBudget)?,
            ));
        }
        let (deadline, mut expires) = self.preclaim_wait_until.expect("first wait latched");
        expires = expires.min(self.window.expires_ns);
        for facts in self
            .window
            .original
            .iter()
            .chain(self.window.current.iter())
        {
            expires = expires
                .min(facts.window().lease_until_ns())
                .min(facts.window().source_until_ns())
                .min(facts.window().clock_until_ns());
        }
        let instant = Instant::now();
        let remaining = deadline
            .min(self.window.deadline)
            .checked_duration_since(instant)
            .ok_or(HspDispatchFailure::InvalidBudget)?
            .min(Duration::from_nanos(
                expires
                    .checked_sub(now)
                    .ok_or(HspDispatchFailure::InvalidBudget)?,
            ))
            .min(PRECLAIM_RETRY_DELAY);
        if remaining.is_zero() {
            return Err(HspDispatchFailure::InvalidBudget);
        }
        Ok(remaining)
    }

    /// Compose one native preparation constraint with the source authority.
    /// The validator is pure/bounded and runs on both sides of durable I/O.
    pub(crate) fn with_facts_validator(
        mut self,
        validator: HspFactsValidator,
    ) -> Result<Self, HspDispatchFailure> {
        if self.facts_validator.is_some() {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.facts_validator = Some(validator);
        Ok(self)
    }
    /// Test-only suspension after durable claim while preserving its custody.
    #[cfg(test)]
    pub(crate) fn with_after_claim_hook(
        mut self,
        hook: Box<dyn FnOnce() + Send + 'static>,
    ) -> Self {
        self.after_claim_hook = Some(hook);
        self
    }
    /// Retain native session/Engine retirement flags through the final claim
    /// and transport boundary. A provider stalled before applying this gate
    /// cannot refresh ordinary authority after shutdown. Stop retains its
    /// independent owner-bound obligation and ignores these ordinary flags.
    pub(crate) fn with_retirement_flags(
        mut self,
        stopped: Arc<AtomicBool>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        if self.request.effect.kind() != HspEffectKind::Stop {
            self.window.retirement = Some(OrdinaryRetirementFlags { stopped, shutdown });
        }
        self
    }
    /// The fact provider is evaluated under the runtime lock both before and
    /// after durable I/O. Each call must derive facts for its supplied instant.
    pub fn apply<F>(
        self,
        owner: &HspNativeOwner,
        facts_at: F,
    ) -> Result<HspClaimedHandoff, HspDispatchFailure>
    where
        F: FnMut(u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure>,
    {
        match self.try_apply(owner, facts_at)? {
            HspPreclaimProgress::Claimed(handoff) => Ok(handoff),
            HspPreclaimProgress::Contended(_) => Err(HspDispatchFailure::Busy),
        }
    }

    /// Only preclaim mutex WouldBlock returns this original gate. Durable I/O,
    /// validation and every outcome after begin_claim remain terminal here.
    pub fn try_apply<F>(
        mut self,
        owner: &HspNativeOwner,
        mut facts_at: F,
    ) -> Result<HspPreclaimProgress<'a>, HspDispatchFailure>
    where
        F: FnMut(u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure>,
    {
        self.check_preclaim_wait(owner)?;
        // Durable reads and FULL-synchronous commits can outlast the runtime
        // watchdog's contention budget even with SQLite busy waiting disabled.
        // Keep them outside the mutable runtime critical section.
        let journal_mutex = self.journal;
        let mut journal = match journal_mutex.try_lock() {
            Ok(journal) => journal,
            Err(TryLockError::WouldBlock) => {
                self.preclaim_retry_delay(owner)?;
                return Ok(HspPreclaimProgress::Contended(self));
            }
            Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::Journal),
        };
        self.check_preclaim_wait(owner)?;
        let stream = journal
            .hsp_stream(self.request.receipt.stream_id())
            .map_err(|_| HspDispatchFailure::Journal)?;
        if stream.record.registration != self.transport.binding.registration
            || stream.record.device != native_identity(&self.transport.binding.device)
            || stream.record.route_sha256 != self.transport.route.route_sha256()
            || stream.record.route_namespace_sha256 != self.transport.route.namespace_sha256()
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        let runtime_mutex = self.runtime;
        let mut runtime = match runtime_mutex.try_lock() {
            Ok(runtime) => runtime,
            Err(TryLockError::WouldBlock) => {
                drop(journal);
                self.preclaim_retry_delay(owner)?;
                return Ok(HspPreclaimProgress::Contended(self));
            }
            Err(TryLockError::Poisoned(_)) => return Err(HspDispatchFailure::Busy),
        };
        let now = self.check_preclaim_wait(owner)?;
        let facts = facts_at(now)?;
        if let Some(validate) = &self.facts_validator {
            let facts = facts.as_ref().ok_or(HspDispatchFailure::Authority(
                HspRuntimeError::ExpiredAuthority,
            ))?;
            validate(now, facts)?;
        }
        runtime
            .validate_dispatch(&self.request.effect, now, facts.as_ref())
            .map_err(HspDispatchFailure::Authority)?;
        self.window
            .bind(&self.request.effect, facts, runtime.policy())?;
        self.check_preclaim_wait(owner)?;
        self.custody
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .reservation
            .begin_claim()
            .map_err(HspDispatchFailure::Authority)?;
        // Mark an in-progress durable attempt conservatively for unwinding. A
        // normal Err proves no claim; panic after commit must remain Unknown.
        self.custody
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .attempted = true;
        // ATTEMPTING retains this exact native reservation while the watchdog
        // can run or retire the runtime. It cannot be abandoned as unsent, and
        // no replacement worker can reserve this effect during durable I/O.
        drop(runtime);
        let claim = match journal.try_claim_hsp(&self.request.receipt, &self.request.effect) {
            Ok(claim) => claim,
            Err(_) => {
                self.custody
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .reservation
                    .reject_claim();
                self.custody
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .attempted = false;
                return Err(HspDispatchFailure::Journal);
            }
        };
        {
            let mut custody = self.custody.lock().unwrap_or_else(|p| p.into_inner());
            custody.reservation.claimed();
            custody.claim = Some(claim);
        }
        drop(journal);
        #[cfg(test)]
        if let Some(hook) = self.after_claim_hook.take() {
            hook();
        }
        self.window.tick(self.clock, self.cancel)?;
        // A successful commit is durable custody, not continuing authority.
        // Sample fresh time and facts only after reacquiring this runtime. Any
        // refusal is an Unknown claimed attempt; the dispatcher must not resend.
        let mut runtime = self
            .runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?;
        let now = self.window.tick(self.clock, self.cancel)?;
        let facts = facts_at(now)?;
        if let Some(validate) = &self.facts_validator {
            let facts = facts.as_ref().ok_or(HspDispatchFailure::Authority(
                HspRuntimeError::ExpiredAuthority,
            ))?;
            validate(now, facts)?;
        }
        runtime
            .validate_dispatch(&self.request.effect, now, facts.as_ref())
            .map_err(HspDispatchFailure::Authority)?;
        // bind only narrows the existing expiry/lead window; original facts and
        // all previously consumed validity remain frozen across the commit.
        self.window
            .bind(&self.request.effect, facts, runtime.policy())?;
        self.window.tick(self.clock, self.cancel)?;
        self.transport.check(&self.request.effect)?;
        Ok(HspPreclaimProgress::Claimed(HspClaimedHandoff {
            custody: self.custody.clone(),
            window: self.window,
        }))
    }
}
struct DurableActive {
    request: Arc<HspDispatchRequest>,
    worker: JoinHandle<HspDispatchAttempt>,
    cancel: Arc<AtomicBool>,
    custody: Arc<Mutex<ClaimCustody>>,
}
/// Stop-only native capability. It has no durable receipt and cannot be
/// reconstructed from historical records. Creating it does not reserve a worker.
pub struct HspEmergencyStopRequest {
    transport: Arc<HandyHspTransport>,
    runtime: Arc<Mutex<HandyHspRuntime>>,
    effect: HspEffect,
    owner: HspNativeOwner,
    context: HspEmergencyStopContext,
}
/// Exact non-secret native transport snapshot. This is historical metadata, not
/// authority to restore a runtime, reconstruct credentials, or resend a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspEmergencyStopContext {
    registration: HandyRegistration,
    device: HandyIdentity,
    mode: HandyDeviceMode,
    route_sha256: String,
    route_namespace_sha256: String,
}
impl HspEmergencyStopContext {
    pub fn registration(&self) -> &HandyRegistration {
        &self.registration
    }
    pub fn device(&self) -> &HandyIdentity {
        &self.device
    }
    pub fn mode(&self) -> &HandyDeviceMode {
        &self.mode
    }
    pub fn route_sha256(&self) -> &str {
        &self.route_sha256
    }
    pub fn route_namespace_sha256(&self) -> &str {
        &self.route_namespace_sha256
    }
}
impl HspEmergencyStopRequest {
    pub fn effect(&self) -> &HspEffect {
        &self.effect
    }
}
pub struct HspEmergencyStartFailure {
    pub reason: HspDispatchFailure,
    pub request: HspEmergencyStopRequest,
}
/// Retrospective transport evidence only. No durable journal claim, persisted
/// record, authority reconstruction, physical proof, or resend method is supplied.
#[derive(Clone, Debug, PartialEq)]
pub struct HspEmergencyStopObservation {
    record: HspCompletionRecord,
    context: HspEmergencyStopContext,
}
impl HspEmergencyStopObservation {
    pub fn context(&self) -> &HspEmergencyStopContext {
        &self.context
    }
    pub fn record(&self) -> &HspCompletionRecord {
        &self.record
    }
}
#[must_use = "retain the emergency observation; it has no durable journal record"]
pub struct HspEmergencyStopCompletion {
    reservation: NativeHspReservation,
    request: HspEmergencyStopRequest,
    attempt: HspDispatchAttempt,
    observation: Option<HspEmergencyStopObservation>,
}
impl HspEmergencyStopCompletion {
    pub fn effect(&self) -> &HspEffect {
        &self.request.effect
    }
    pub fn attempt(&self) -> &HspDispatchAttempt {
        &self.attempt
    }
    pub fn observation(&self) -> Option<&HspEmergencyStopObservation> {
        self.observation.as_ref()
    }
    /// Observe exactly once after join. Repeated metadata access returns the
    /// frozen original, including Unknown, without re-observing or sending.
    pub fn observe(
        &mut self,
        runtime: &mut HandyHspRuntime,
        now_ns: u64,
    ) -> Result<&HspEmergencyStopObservation, HspDispatchFailure> {
        if self.observation.is_none() {
            let response = match &self.attempt {
                HspDispatchAttempt::Rejected(error) => return Err(error.clone()),
                HspDispatchAttempt::Observed { response } => response,
            };
            runtime
                .validate_native_completion(&self.request.effect, &self.reservation)
                .map_err(HspDispatchFailure::Authority)?;
            let bytes = response.map(state_bytes);
            let record = runtime
                .complete_settled(
                    &self.request.effect,
                    match bytes.as_deref() {
                        Some(body) => HspSettledTransport::Response { status: 200, body },
                        None => HspSettledTransport::Unknown,
                    },
                    now_ns,
                    None,
                )
                .map_err(HspDispatchFailure::Authority)?;
            self.observation = Some(HspEmergencyStopObservation {
                record,
                context: self.request.context.clone(),
            });
        }
        Ok(self
            .observation
            .as_ref()
            .expect("retained original observation"))
    }
}
struct EmergencyActive {
    request: Arc<HspEmergencyStopRequest>,
    worker: JoinHandle<HspDispatchAttempt>,
    cancel: Arc<AtomicBool>,
    reservation: NativeHspReservation,
}
/// Native custody proves this exact ordinary effect did not reach a claim.
/// This is not durable cancellation and does not release historical route custody.
#[derive(Clone, Debug)]
pub struct HspNativeUnsentObservation {
    effect: HspEffect,
}
impl HspNativeUnsentObservation {
    pub fn effect(&self) -> &HspEffect {
        &self.effect
    }
}
#[derive(Clone, Debug)]
pub struct HspPostSetupObservation {
    observed_ns: u64,
    mode: HandyDeviceMode,
    state: HspState,
}
impl HspPostSetupObservation {
    pub fn mode(&self) -> &HandyDeviceMode {
        &self.mode
    }
    pub fn state(&self) -> &HspState {
        &self.state
    }
}
struct PostSetupProbeActive {
    deadline: Instant,
    expires_ns: u64,
    runtime: Arc<Mutex<HandyHspRuntime>>,
    transport: Arc<HandyHspTransport>,
    setup: HspEffect,
    reservation: NativeHspProbeReservation,
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<Result<HspPostSetupObservation, HspDispatchFailure>>,
}
/// Sealed joined read result. Only the originating dispatcher transport/runtime
/// snapshot can install it; no historical record can create this capability.
pub struct HspPostSetupProbeCompletion {
    deadline: Instant,
    expires_ns: u64,
    runtime: Arc<Mutex<HandyHspRuntime>>,
    transport: Arc<HandyHspTransport>,
    setup: HspEffect,
    reservation: NativeHspProbeReservation,
    result: Result<HspPostSetupObservation, HspDispatchFailure>,
}
impl HspPostSetupProbeCompletion {
    pub fn setup(&self) -> &HspEffect {
        &self.setup
    }
    pub fn result(&self) -> Result<&HspPostSetupObservation, &HspDispatchFailure> {
        self.result.as_ref()
    }
}
enum Active {
    Durable(DurableActive),
    Emergency(EmergencyActive),
    PostSetupProbe(Box<PostSetupProbeActive>),
}
impl Active {
    fn cancel(&self) {
        match self {
            Self::Durable(a) => &a.cancel,
            Self::Emergency(a) => &a.cancel,
            Self::PostSetupProbe(a) => &a.cancel,
        }
        .store(true, Ordering::Release);
    }
    fn is_finished(&self) -> bool {
        match self {
            Self::Durable(a) => a.worker.is_finished(),
            Self::Emergency(a) => a.worker.is_finished(),
            Self::PostSetupProbe(a) => a.worker.is_finished(),
        }
    }
}
#[must_use = "retain joined durable claims or retrospective emergency observations"]
pub enum HspJoinedCompletion {
    Durable(Box<HspDispatchCompletion>),
    Emergency(Box<HspEmergencyStopCompletion>),
    PostSetupProbe(Box<HspPostSetupProbeCompletion>),
}

/// One worker, no queue. Cancellation requests never free the slot; poll and
/// shutdown join first. Dropping joins and leaves durable claimed custody intact.
pub struct HandyHspDispatcher {
    runtime: Arc<Mutex<HandyHspRuntime>>,
    journal: Arc<Mutex<DeviceEffectJournal>>,
    transport: Arc<HandyHspTransport>,
    clock: HostClock,
    active: Option<Active>,
}
impl HandyHspDispatcher {
    pub fn new(
        runtime: Arc<Mutex<HandyHspRuntime>>,
        journal: Arc<Mutex<DeviceEffectJournal>>,
        transport: HandyHspTransport,
    ) -> Self {
        Self {
            runtime,
            journal,
            transport: Arc::new(transport),
            active: None,
            clock: Arc::new(|| {
                crate::handy_clock::boottime_ns().map_err(|_| HspDispatchFailure::InvalidClock)
            }),
        }
    }
    pub fn start<F>(
        &mut self,
        request: HspDispatchRequest,
        deadline: Instant,
        authorize: F,
    ) -> Result<(), Box<HspStartFailure>>
    where
        F: FnOnce(
                Instant,
                &AtomicBool,
                HspClaimGate<'_>,
            ) -> Result<HspClaimedHandoff, HspDispatchFailure>
            + Send
            + 'static,
    {
        let prepare = || -> Result<DispatchWindow, HspDispatchFailure> {
            if self.active.is_some() {
                return Err(HspDispatchFailure::Busy);
            }
            let instant = Instant::now();
            let remaining = deadline
                .checked_duration_since(instant)
                .filter(|d| !d.is_zero() && *d <= MAX_BUDGET)
                .ok_or(HspDispatchFailure::InvalidBudget)?;
            self.transport.check(&request.effect)?;
            let now = (self.clock)()?;
            let expires_ns = now
                .checked_add(
                    u64::try_from(remaining.as_nanos())
                        .map_err(|_| HspDispatchFailure::InvalidBudget)?,
                )
                .ok_or(HspDispatchFailure::InvalidBudget)?;
            Ok(DispatchWindow {
                retirement: None,
                deadline,
                last_ns: now,
                expires_ns,
                original: request.effect.original_facts().cloned(),
                current: None,
            })
        };
        let mut window = match prepare() {
            Ok(w) => w,
            Err(reason) => return Err(Box::new(HspStartFailure { reason, request })),
        };
        let reservation = match self.reserve(&request.effect) {
            Ok(reservation) => reservation,
            Err(reason) => return Err(Box::new(HspStartFailure { reason, request })),
        };
        let request = Arc::new(request);
        let captured = request.clone();
        let runtime = self.runtime.clone();
        let journal = self.journal.clone();
        let transport = self.transport.clone();
        let clock = self.clock.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let custody = Arc::new(Mutex::new(ClaimCustody {
            reservation: reservation.clone(),
            attempted: false,
            claim: None,
        }));
        let claim_record = custody.clone();
        let worker = thread::Builder::new()
            .name("pulsar-hsp-dispatch".into())
            .spawn(move || {
                if let Err(error) =
                    transport.preflight(&captured.effect, &mut window, &clock, &cancelled)
                {
                    return HspDispatchAttempt::Rejected(error);
                }
                let handoff = authorize(
                    deadline,
                    &cancelled,
                    HspClaimGate {
                        runtime: &runtime,
                        journal: &journal,
                        transport: &transport,
                        request: &captured,
                        clock: &clock,
                        cancel: &cancelled,
                        custody: &claim_record,
                        facts_validator: None,
                        #[cfg(test)]
                        after_claim_hook: None,
                        window,
                        preclaim_wait_until: None,
                    },
                );
                let mut window = match handoff {
                    Ok(h)
                        if Arc::ptr_eq(&h.custody, &claim_record)
                            && claim_record
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .claim
                                .is_some() =>
                    {
                        h.window
                    }
                    result => {
                        if claim_record
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .attempted
                        {
                            return HspDispatchAttempt::Observed { response: None };
                        }
                        return HspDispatchAttempt::Rejected(
                            result.err().unwrap_or(HspDispatchFailure::BindingMismatch),
                        );
                    }
                };
                let response = transport
                    .http(
                        HspHttpRequest::effect(&captured.effect),
                        &captured.effect,
                        &mut window,
                        &clock,
                        &cancelled,
                    )
                    .ok()
                    .and_then(|bytes| handy_hsp_wire::decode_state(200, &bytes).ok());
                HspDispatchAttempt::Observed {
                    response: if cancelled.load(Ordering::Acquire) {
                        None
                    } else {
                        response
                    },
                }
            });
        match worker {
            Ok(worker) => {
                self.active = Some(Active::Durable(DurableActive {
                    request,
                    worker,
                    cancel,
                    custody,
                }));
                Ok(())
            }
            Err(_) => {
                reservation.join();
                Err(Box::new(HspStartFailure {
                    reason: HspDispatchFailure::WorkerUnavailable,
                    request: unwrap_request(request),
                }))
            }
        }
    }
    pub fn retire_native_unsent(
        &self,
        effect: &HspEffect,
        owner: &HspNativeOwner,
    ) -> Result<HspNativeUnsentObservation, HspDispatchFailure> {
        if self.active.is_some() {
            return Err(HspDispatchFailure::Busy);
        }
        if !Arc::ptr_eq(&owner.0, &self.transport.owner.0)
            || !owner.0.current.load(Ordering::Acquire)
            || effect.binding() != &self.transport.binding.binding
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?
            .retire_native_unsent(effect, &self.transport.instance)
            .map_err(HspDispatchFailure::Authority)?;
        Ok(HspNativeUnsentObservation {
            effect: effect.clone(),
        })
    }
    /// Owned, read-only acquisition after the exact attempted Setup has joined
    /// and been observed. Source expiry may retire ordinary authority while this
    /// read resolves the exact mode needed for the outstanding Stop obligation.
    pub fn start_post_setup_probe(
        &mut self,
        setup: &HspEffect,
        owner: &HspNativeOwner,
        deadline: Instant,
    ) -> Result<(), HspDispatchFailure> {
        if self.active.is_some() {
            return Err(HspDispatchFailure::Busy);
        }
        if !Arc::ptr_eq(&owner.0, &self.transport.owner.0) {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.transport.check_probe(setup)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero() && *d <= MAX_BUDGET)
            .ok_or(HspDispatchFailure::InvalidBudget)?;
        let now = (self.clock)()?;
        let expires_ns = now
            .checked_add(
                u64::try_from(remaining.as_nanos())
                    .map_err(|_| HspDispatchFailure::InvalidBudget)?,
            )
            .ok_or(HspDispatchFailure::InvalidBudget)?;
        let mut window = DispatchWindow {
            retirement: None,
            deadline,
            last_ns: now,
            expires_ns,
            original: None,
            current: None,
        };
        let reservation = self
            .runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?
            .reserve_post_setup_probe(setup, &self.transport.instance)
            .map_err(HspDispatchFailure::Authority)?;
        let transport = self.transport.clone();
        let captured = setup.clone();
        let journal = self.journal.clone();
        let clock = self.clock.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let worker = thread::Builder::new()
            .name("pulsar-hsp-setup-probe".into())
            .spawn(move || {
                // Journal validation can perform filesystem I/O. Keep it on
                // this owned worker, never on the independent watchdog pump.
                check_emergency_journal_owner(&journal, &transport)?;
                let mut read = |path| {
                    transport.http_bound(
                        HspHttpRequest::read(path),
                        &captured,
                        &mut window,
                        &clock,
                        &cancelled,
                        true,
                    )
                };
                let info = read("/info")?;
                let info = handy_wire::decode_info(200, &info)
                    .map_err(|_| HspDispatchFailure::InvalidResponse)?;
                if info != transport.binding.device {
                    return Err(HspDispatchFailure::RemoteMismatch);
                }
                let mode = read("/mode")?;
                let mode = handy_wire::decode_mode(200, &mode)
                    .map_err(|_| HspDispatchFailure::InvalidResponse)?;
                if mode.mode != 4 {
                    return Err(HspDispatchFailure::RemoteMismatch);
                }
                let state = read("/hsp/state")?;
                let state = handy_hsp_wire::decode_state(200, &state)
                    .map_err(|_| HspDispatchFailure::InvalidResponse)?;
                if state.stream_id != captured.binding().identity().stream_id {
                    return Err(HspDispatchFailure::RemoteMismatch);
                }
                check_emergency_journal_owner(&journal, &transport)?;
                transport.check_probe(&captured)?;
                window.tick(&clock, &cancelled)?;
                Ok(HspPostSetupObservation {
                    observed_ns: window.last_ns,
                    mode,
                    state,
                })
            });
        match worker {
            Ok(worker) => {
                self.active = Some(Active::PostSetupProbe(Box::new(PostSetupProbeActive {
                    deadline,
                    expires_ns,
                    runtime: self.runtime.clone(),
                    transport: self.transport.clone(),
                    setup: setup.clone(),
                    reservation,
                    cancel,
                    worker,
                })));
                Ok(())
            }
            Err(_) => {
                reservation.join();
                Err(HspDispatchFailure::WorkerUnavailable)
            }
        }
    }
    /// Explicitly install only the exact joined native read, preserving captured
    /// credentials, route, registration, device/session and owner retirement flags.
    pub fn install_post_setup_probe(
        &mut self,
        completion: &HspPostSetupProbeCompletion,
    ) -> Result<(), HspDispatchFailure> {
        if self.active.is_some() {
            return Err(HspDispatchFailure::Busy);
        }
        if !Arc::ptr_eq(&completion.runtime, &self.runtime)
            || !Arc::ptr_eq(&completion.transport, &self.transport)
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.transport.check_probe(&completion.setup)?;
        // Installation is a native rebind, not dispatch. The joined worker
        // checked journal ownership; every later sender checks it again before
        // I/O. This nonblocking path trusts no historical journal capability.
        let observation = completion.result.as_ref().map_err(Clone::clone)?;
        let now = (self.clock)()?;
        if now < observation.observed_ns
            || now >= completion.expires_ns
            || Instant::now() >= completion.deadline
        {
            return Err(HspDispatchFailure::InvalidClock);
        }
        let runtime = self
            .runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?;
        runtime
            .validate_post_setup_probe(&completion.reservation)
            .map_err(HspDispatchFailure::Authority)?;
        let mut binding = self.transport.binding.clone();
        binding.mode = observation.mode.clone();
        self.transport = Arc::new(HandyHspTransport {
            instance: NativeHspTransportIdentity::new(),
            binding,
            owner: self.transport.owner.clone(),
            credentials: self.transport.credentials.clone(),
            route: self.transport.route.clone(),
            origin: self.transport.origin.clone(),
            stop_send: self.transport.stop_send.clone(),
        });
        Ok(())
    }
    fn reserve(&self, effect: &HspEffect) -> Result<NativeHspReservation, HspDispatchFailure> {
        self.runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?
            .reserve_native(effect, &self.transport.instance)
            .map_err(HspDispatchFailure::Authority)
    }
    /// Seal only the current already-issued Stop. An unresolved ordinary slot
    /// cannot be abandoned by this API. Owner integration must settle it first.
    pub fn prepare_emergency_stop(
        &self,
        effect: HspEffect,
        owner: &HspNativeOwner,
    ) -> Result<HspEmergencyStopRequest, HspDispatchFailure> {
        if effect.kind() != HspEffectKind::Stop
            || !Arc::ptr_eq(&owner.0, &self.transport.owner.0)
            || effect.request().body().is_some()
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.transport.check(&effect)?;
        let runtime = self
            .runtime
            .try_lock()
            .map_err(|_| HspDispatchFailure::Busy)?;
        if runtime.in_flight() != Some(&effect) {
            return Err(HspDispatchFailure::Authority(
                HspRuntimeError::ForeignEffect,
            ));
        }
        runtime
            .require_native_released()
            .map_err(HspDispatchFailure::Authority)?;
        Ok(HspEmergencyStopRequest {
            transport: self.transport.clone(),
            runtime: self.runtime.clone(),
            effect,
            owner: owner.clone(),
            context: HspEmergencyStopContext {
                registration: self.transport.binding.registration.clone(),
                device: native_identity(&self.transport.binding.device),
                mode: self.transport.binding.mode.clone(),
                route_sha256: self.transport.route.route_sha256().into(),
                route_namespace_sha256: self.transport.route.namespace_sha256().into(),
            },
        })
    }
    /// Stop-only fallback uses the same worker slot and runtime reservation as
    /// durable dispatch. No journal transaction is necessary to claim this native
    /// attempt. A known foreign journal owner still permanently retires transport.
    pub fn start_emergency_stop(
        &mut self,
        request: HspEmergencyStopRequest,
        deadline: Instant,
    ) -> Result<(), Box<HspEmergencyStartFailure>> {
        let prepare = || {
            if self.active.is_some() {
                return Err(HspDispatchFailure::Busy);
            }
            if request.effect.kind() != HspEffectKind::Stop
                || !Arc::ptr_eq(&request.runtime, &self.runtime)
                || !Arc::ptr_eq(&request.owner.0, &self.transport.owner.0)
                || !Arc::ptr_eq(&request.transport, &self.transport)
            {
                return Err(HspDispatchFailure::BindingMismatch);
            }
            self.transport.check(&request.effect)?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero() && *d <= MAX_BUDGET)
                .ok_or(HspDispatchFailure::InvalidBudget)?;
            let now = (self.clock)()?;
            let expires_ns = now
                .checked_add(
                    u64::try_from(remaining.as_nanos())
                        .map_err(|_| HspDispatchFailure::InvalidBudget)?,
                )
                .ok_or(HspDispatchFailure::InvalidBudget)?;
            Ok(DispatchWindow {
                retirement: None,
                deadline,
                last_ns: now,
                expires_ns,
                original: None,
                current: None,
            })
        };
        let mut window = match prepare() {
            Ok(window) => window,
            Err(reason) => return Err(Box::new(HspEmergencyStartFailure { reason, request })),
        };
        let reservation = match self.reserve(&request.effect) {
            Ok(reservation) => reservation,
            Err(reason) => return Err(Box::new(HspEmergencyStartFailure { reason, request })),
        };
        let request = Arc::new(request);
        let captured = request.clone();
        let runtime = self.runtime.clone();
        let journal = self.journal.clone();
        let transport = self.transport.clone();
        let clock = self.clock.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let claimed = reservation.clone();
        let worker = thread::Builder::new()
            .name("pulsar-hsp-emergency-stop".into())
            .spawn(move || {
                let mut prepare = || -> Result<(), HspDispatchFailure> {
                    check_emergency_journal_owner(&journal, &transport)?;
                    transport.preflight(&captured.effect, &mut window, &clock, &cancelled)?;
                    check_emergency_journal_owner(&journal, &transport)?;
                    let mut runtime = runtime.try_lock().map_err(|_| HspDispatchFailure::Busy)?;
                    let now = window.tick(&clock, &cancelled)?;
                    transport.check(&captured.effect)?;
                    runtime
                        .validate_dispatch(&captured.effect, now, None)
                        .map_err(HspDispatchFailure::Authority)?;
                    claimed
                        .begin_claim()
                        .map_err(HspDispatchFailure::Authority)?;
                    claimed.claimed();
                    Ok(())
                };
                if let Err(error) = prepare() {
                    return HspDispatchAttempt::Rejected(error);
                }
                let response = transport
                    .http(
                        HspHttpRequest::effect(&captured.effect),
                        &captured.effect,
                        &mut window,
                        &clock,
                        &cancelled,
                    )
                    .ok()
                    .and_then(|bytes| handy_hsp_wire::decode_state(200, &bytes).ok());
                HspDispatchAttempt::Observed {
                    response: if cancelled.load(Ordering::Acquire) {
                        None
                    } else {
                        response
                    },
                }
            });
        match worker {
            Ok(worker) => {
                self.active = Some(Active::Emergency(EmergencyActive {
                    request,
                    worker,
                    cancel,
                    reservation,
                }));
                Ok(())
            }
            Err(_) => {
                reservation.join();
                Err(Box::new(HspEmergencyStartFailure {
                    reason: HspDispatchFailure::WorkerUnavailable,
                    request: Arc::try_unwrap(request)
                        .unwrap_or_else(|_| panic!("failed worker retained request")),
                }))
            }
        }
    }
    pub fn cancel(&self) {
        if let Some(active) = &self.active {
            active.cancel();
        }
    }
    /// Durable-only convenience poll. Use poll_any when the owner uses fallback.
    pub fn poll(&mut self) -> Option<HspDispatchCompletion> {
        if !matches!(self.active, Some(Active::Durable(_))) {
            return None;
        }
        match self.poll_any()? {
            HspJoinedCompletion::Durable(completion) => Some(*completion),
            HspJoinedCompletion::Emergency(_) | HspJoinedCompletion::PostSetupProbe(_) => {
                unreachable!("checked active kind")
            }
        }
    }
    pub fn poll_any(&mut self) -> Option<HspJoinedCompletion> {
        if !self.active.as_ref()?.is_finished() {
            return None;
        }
        self.join()
    }
    fn join(&mut self) -> Option<HspJoinedCompletion> {
        match self.active.take()? {
            Active::PostSetupProbe(active) => {
                let mut result = active
                    .worker
                    .join()
                    .unwrap_or(Err(HspDispatchFailure::WorkerUnavailable));
                if active.cancel.load(Ordering::Acquire) {
                    result = Err(HspDispatchFailure::Cancelled);
                }
                active.reservation.join();
                Some(HspJoinedCompletion::PostSetupProbe(Box::new(
                    HspPostSetupProbeCompletion {
                        deadline: active.deadline,
                        expires_ns: active.expires_ns,
                        runtime: active.runtime,
                        transport: active.transport,
                        setup: active.setup,
                        reservation: active.reservation,
                        result,
                    },
                )))
            }
            Active::Durable(active) => {
                let result = active.worker.join();
                let custody = active.custody.lock().unwrap_or_else(|p| p.into_inner());
                let attempted = custody.reservation.join();
                let attempt = result.unwrap_or({
                    if attempted {
                        HspDispatchAttempt::Observed { response: None }
                    } else {
                        HspDispatchAttempt::Rejected(HspDispatchFailure::WorkerUnavailable)
                    }
                });
                Some(HspJoinedCompletion::Durable(Box::new(
                    HspDispatchCompletion {
                        request: unwrap_request(active.request),
                        claim: custody.claim.clone(),
                        attempt,
                    },
                )))
            }
            Active::Emergency(active) => {
                let result = active.worker.join();
                let attempted = active.reservation.join();
                let attempt = result.unwrap_or({
                    if attempted {
                        HspDispatchAttempt::Observed { response: None }
                    } else {
                        HspDispatchAttempt::Rejected(HspDispatchFailure::WorkerUnavailable)
                    }
                });
                Some(HspJoinedCompletion::Emergency(Box::new(
                    HspEmergencyStopCompletion {
                        reservation: active.reservation,
                        request: Arc::try_unwrap(active.request)
                            .unwrap_or_else(|_| panic!("joined worker retained request")),
                        attempt,
                        observation: None,
                    },
                )))
            }
        }
    }
    /// Cancellation never frees native custody until the exact worker joins.
    /// Joining needs no runtime or journal mutex, including emergency shutdown.
    pub fn shutdown(&mut self) -> Option<HspJoinedCompletion> {
        self.cancel();
        self.join()
    }
}
impl Drop for HandyHspDispatcher {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn check_emergency_journal_owner(
    journal: &Mutex<DeviceEffectJournal>,
    transport: &HandyHspTransport,
) -> Result<(), HspDispatchFailure> {
    if let Ok(mut journal) = journal.try_lock() {
        if journal.try_current_owner().is_ok_and(|current| !current) {
            transport.owner.retire_owner();
            return Err(HspDispatchFailure::BindingMismatch);
        }
    }
    // Metadata unavailable is not proof of replacement. Only a sealed Stop or
    // exact post-Setup read may proceed using retained native ownership. This
    // helper runs on owned native workers; even a try-lock-protected DB read can
    // block on filesystem I/O and must never run on the watchdog pump.
    Ok(())
}
fn unwrap_request(request: Arc<HspDispatchRequest>) -> HspDispatchRequest {
    Arc::try_unwrap(request).unwrap_or_else(|_| panic!("joined HSP worker retained request"))
}

#[cfg(test)]
#[path = "handy_hsp_dispatch_tests.rs"]
mod tests;

#[cfg(test)]
mod stop_send_tracker_tests {
    use super::*;

    #[test]
    fn finite_stop_deadline_and_first_gate_cannot_be_renewed() {
        let tracker = HspStopSendTracker::default();
        assert!(tracker.arm(0).is_err());
        tracker.arm(100).unwrap();
        tracker.arm(100).unwrap();
        assert!(tracker.arm(101).is_err());
        tracker.sent(99);
        tracker.observe(200);
        tracker.sent(201);
        assert_eq!(tracker.status(), (Some(100), Some(99), false));
    }

    #[test]
    fn finite_stop_late_gate_is_reported_without_suppressing_it() {
        let tracker = HspStopSendTracker::default();
        tracker.arm(100).unwrap();
        tracker.observe(101);
        assert_eq!(tracker.status(), (Some(100), None, true));
        tracker.sent(102);
        tracker.sent(99);
        assert_eq!(tracker.status(), (Some(100), Some(102), true));
    }

    #[test]
    fn earlier_actual_gate_wins_over_later_poll_publication_order() {
        let tracker = HspStopSendTracker::default();
        tracker.arm(100).unwrap();
        // The owned writer sampled its final gate before this poll, then was
        // descheduled before publishing that immutable observation.
        let gate = 99;
        tracker.observe(101);
        tracker.sent(gate);
        assert_eq!(tracker.status(), (Some(100), Some(gate), false));
    }
}
