//! Actor/revision/boot-bound ephemeral pixel leases and bounded RGB ingress.
use super::*;
use crate::live_frames::LiveRgbRuntime;
use std::time::{Duration, Instant};
#[path = "live_capture_clock.rs"]
mod clock;
#[path = "live_capture_stamp.rs"]
mod stamp;
pub(in crate::authority) use clock::{BootBounds, NativeAcceptedCapture};
#[path = "live_capture_inference.rs"]
mod inference;
#[path = "live_capture_successor.rs"]
mod successor;
pub(in crate::authority) use successor::{
    NativeCaptureSuccessor, NativeCaptureTransfer, NativeCaptureTransferFailure,
};
#[cfg(test)]
#[path = "live_capture_read_order_test_hooks.rs"]
mod read_order_test;
#[path = "live_capture_source.rs"]
mod source;
#[cfg(test)]
use read_order_test::CaptureReadGuard;
#[cfg(test)]
pub(in crate::authority) use read_order_test::{
    CaptureReadTestHold, CaptureReadTestPhase, CaptureReadTestTarget,
};
const MAX_ACTIVE: usize = 8;
const MAX_RETAINED: usize = 64;
// Preserve the ordinary timing snapshot's protocol error shape at admission.
fn browser_timing_read_error(
    error: super::browser_timing::BrowserTimingClaimFailure,
) -> ProtocolError {
    match error {
        super::browser_timing::BrowserTimingClaimFailure::Contended => ProtocolError::new(
            ErrorCode::Unavailable,
            "browser timing binding is not current",
        ),
        super::browser_timing::BrowserTimingClaimFailure::Refused(error) => error,
    }
}
#[path = "live_capture_pack.rs"]
mod pack;
#[path = "live_recording.rs"]
mod recording;
struct Entry {
    owner: SessionId,
    created: Instant,
    native_clock: clock::NativeCaptureClock,
    native_accepted: Option<clock::NativeAcceptedCapture>,
    native_delivery: Option<Arc<NativeDelivery>>,
    native_input_close: Option<NativeInputClose>,
    native_successor: Option<ArtifactId>,
    native_transfer: Option<ArtifactId>,
    native_transfer_joined: bool,
    report: LiveCaptureReport,
    runtime: Option<LiveRgbRuntime>,
    cancel: Arc<AtomicBool>,
    origin: Option<crate::live_cuda_capture_db::LiveCudaOrigin>,
    recording: Option<recording::Recording>,
    browser_media: Option<super::browser_media::SourceMediaPin>,
    browser_analysis_pin: Option<super::browser_timing::BrowserTimingPin>,
}
impl Drop for Entry {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.retire_native_delivery();
    }
}
impl Entry {
    fn retire_native_delivery(&self) {
        if let Some(delivery) = &self.native_delivery {
            delivery.retire();
        }
    }
}

/// A native consumer of the original accepted CPU result, never a status-poll
/// producer or a deserialized dispatch grant. `offer` runs outside Engine DB
/// and capture guards and must be bounded. `retire` MUST be nonblocking: it is
/// called while authority guards may be held and must only latch retirement,
/// never wait on HSP/runtime/DB locks. Both methods may race; the consumer must
/// refuse effects after its retirement latch is set.
pub(in crate::authority) trait NativeCaptureSink: Send + Sync {
    fn offer(&self, accepted: NativeAcceptedCapture) -> PResult<()>;
    fn retire(&self);
    fn prepare_successor(&self, _successor: NativeCaptureSuccessor) -> PResult<()> {
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "native consumer does not support source succession",
        ))
    }
    fn finish_input(&self, _end: NativeCaptureEnd) -> PResult<()> {
        self.retire();
        Ok(())
    }
    fn transfer_input(&self, _transfer: NativeCaptureTransfer) -> PResult<()> {
        self.retire();
        Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "native consumer does not support source transfer",
        ))
    }
}

/// Exact native input settlement. Only the CPU owner can construct this after
/// its original input, direct delivery and worker custody have settled. No
/// serde constructor or public report can restore this capability.
#[derive(Clone, Debug)]
pub(in crate::authority) struct NativeCaptureEnd {
    selection: NativeCaptureSelection,
    close_request_id: RequestId,
    final_frame: Option<LiveFrameHeader>,
    final_accepted: Option<NativeAcceptedCapture>,
    disposition: LiveCaptureInputFinalDisposition,
    closed_at_boottime_ns: u64,
    continuity_generation: u64,
}
impl NativeCaptureEnd {
    pub fn selection(&self) -> &NativeCaptureSelection {
        &self.selection
    }
    pub fn close_request_id(&self) -> &RequestId {
        &self.close_request_id
    }
    pub fn final_frame(&self) -> Option<&LiveFrameHeader> {
        self.final_frame.as_ref()
    }
    pub fn final_accepted(&self) -> Option<&NativeAcceptedCapture> {
        self.final_accepted.as_ref()
    }
    pub fn disposition(&self) -> LiveCaptureInputFinalDisposition {
        self.disposition
    }
    pub fn closed_at_boottime_ns(&self) -> u64 {
        self.closed_at_boottime_ns
    }
    pub fn continuity_generation(&self) -> u64 {
        self.continuity_generation
    }
}
struct NativeInputClose {
    closed_at_boottime_ns: u64,
    continuity_generation: u64,
    disposition: Option<LiveCaptureInputFinalDisposition>,
    end: Option<NativeCaptureEnd>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::authority) struct NativeCaptureSelection {
    pub owner: SessionId,
    pub identity: LiveIdentity,
    pub attempt_id: AttemptId,
    pub profile_sha256: String,
    pub cpu_manifest_sha256: Option<String>,
    pub browser_media: BrowserMediaBinding,
}
impl NativeCaptureSelection {
    /// Correspondence with this original selection; no current timing or grant
    /// is established by these result fields.
    pub fn matches_result(&self, result: &LiveInferenceReply) -> bool {
        let (attempt, identity, profile) = match result {
            LiveInferenceReply::Estimated {
                attempt_id,
                identity,
                profile_sha256,
                ..
            } if self.cpu_manifest_sha256.is_none() => (attempt_id, identity, profile_sha256),
            LiveInferenceReply::CapturedCpuEstimated {
                attempt_id,
                identity,
                profile_sha256,
                manifest_sha256,
                ..
            } if self.cpu_manifest_sha256.as_ref() == Some(manifest_sha256) => {
                (attempt_id, identity, profile_sha256)
            }
            _ => return false,
        };
        attempt == &self.attempt_id && identity == &self.identity && profile == &self.profile_sha256
    }
}
fn cpu_manifest_hash(info: &LiveCaptureInferenceReport) -> PResult<Option<String>> {
    info.cpu_manifest
        .as_ref()
        .map(|manifest| manifest.identity_sha256())
        .transpose()
}

/// Existing native source custody, inspected under the registry guard. The
/// latest stamp is retained original metadata, not a new delivery. An admission
/// may capture it explicitly; reading this snapshot never calls the sink.
#[derive(Clone)]
pub(in crate::authority) struct NativeCaptureSourceSnapshot {
    pub selection: NativeCaptureSelection,
    pub report: LiveCaptureReport,
    pub source_media: super::browser_media::SourceMediaPin,
    pub browser_timing: super::browser_timing::BrowserTimingPin,
    pub latest: Option<NativeAcceptedCapture>,
    pub native_now: BootBounds,
    pub lease_deadline_boottime_ns: u64,
    pub continuity_generation: u64,
}
impl std::fmt::Debug for NativeCaptureSourceSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCaptureSourceSnapshot")
            .field("selection", &self.selection)
            .field("native_now", &self.native_now)
            .field(
                "lease_deadline_boottime_ns",
                &self.lease_deadline_boottime_ns,
            )
            .field("continuity_generation", &self.continuity_generation)
            .finish_non_exhaustive()
    }
}

struct NativeDelivery {
    selection: NativeCaptureSelection,
    sink: std::sync::Weak<dyn NativeCaptureSink>,
    active: AtomicBool,
    capture_cancel: Arc<AtomicBool>,
    continuity_generation: u64,
    next_sequence: std::sync::atomic::AtomicU64,
    delivering: AtomicBool,
    pending: std::sync::atomic::AtomicUsize,
    last_delivered: Mutex<Option<LiveFrame>>,
    finish_claimed: AtomicBool,
    #[cfg(test)]
    test_hold: Mutex<Option<Arc<NativeDeliveryPause>>>,
}
#[cfg(test)]
struct NativeDeliveryPause {
    entered: AtomicBool,
    released: AtomicBool,
}
#[cfg(test)]
pub(in crate::authority) struct NativeDeliveryTestHold {
    pause: Arc<NativeDeliveryPause>,
    delivery: std::sync::Weak<NativeDelivery>,
}
#[cfg(test)]
impl NativeDeliveryTestHold {
    pub fn wait_entered(&self, timeout: Duration) -> bool {
        let Some(end) = Instant::now().checked_add(timeout) else {
            return false;
        };
        while !self.pause.entered.load(Ordering::Acquire) {
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }
    pub fn release(&self) {
        self.pause.released.store(true, Ordering::Release);
    }
    pub fn settled(&self) -> bool {
        self.pause.entered.load(Ordering::Acquire)
            && self.delivery.upgrade().is_none_or(|delivery| {
                delivery.pending.load(Ordering::Acquire) == 0
                    && !delivery.delivering.load(Ordering::Acquire)
            })
    }
}
#[cfg(test)]
impl Drop for NativeDeliveryTestHold {
    fn drop(&mut self) {
        self.release();
    }
}
impl NativeDelivery {
    fn current(&self) -> bool {
        if !self.active.load(Ordering::Acquire) || self.capture_cancel.load(Ordering::Acquire) {
            return false;
        }
        if self.sink.strong_count() == 0 {
            self.active.store(false, Ordering::Release);
            return false;
        }
        true
    }
    fn retire(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            // A failing native consumer must not unwind through capture guards
            // or poison ordinary preview. Its authority is already retired.
            if let Some(sink) = self.sink.upgrade() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.retire()));
            }
        }
    }
    fn deliver(&self, accepted: NativeAcceptedCapture) {
        if !self.current() {
            return;
        }
        #[cfg(test)]
        {
            let hold = self.test_hold.lock().unwrap().take();
            if let Some(hold) = hold {
                hold.entered.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(10);
                while !hold.released.load(Ordering::Acquire)
                    && self.current()
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if !hold.released.load(Ordering::Acquire) {
                    self.retire();
                    return;
                }
            }
        }
        if !self.selection.matches_result(&accepted.estimate.result) {
            self.retire();
            return;
        }
        let input = match &accepted.estimate.result {
            LiveInferenceReply::Estimated { input, .. }
            | LiveInferenceReply::CapturedCpuEstimated { input, .. } => input,
            _ => unreachable!("selected CPU result"),
        };
        let Some(next) = input.sequence.checked_add(1) else {
            self.retire();
            return;
        };
        // No callback overlap, deferred queue, out-of-order callback or replay.
        // Contention retires this consumer while ordinary preview stays usable.
        if self
            .next_sequence
            .compare_exchange(input.sequence, next, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
            || self.delivering.swap(true, Ordering::AcqRel)
        {
            self.retire();
            return;
        }
        if self.current() {
            if let Some(sink) = self.sink.upgrade() {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sink.offer(accepted.clone())
                }));
                if !matches!(result, Ok(Ok(()))) {
                    self.retire();
                } else if let Ok(mut last) = self.last_delivered.lock() {
                    *last = Some(accepted.frame);
                } else {
                    self.retire();
                }
            } else {
                self.retire();
            }
        }
        self.delivering.store(false, Ordering::Release);
    }
    fn finish(&self, end: NativeCaptureEnd) {
        if self.finish_claimed.swap(true, Ordering::AcqRel) || !self.current() {
            return;
        }
        if let Some(sink) = self.sink.upgrade() {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.finish_input(end)));
            if !matches!(result, Ok(Ok(()))) {
                self.retire();
            }
        } else {
            self.retire();
        }
    }
}

/// Reserved under the capture guard before publishing scheduler completion.
/// Drop settles the reservation even if a consumer unwinds or is retired.
struct PendingNativeDelivery {
    sink: Arc<NativeDelivery>,
    accepted: Option<NativeAcceptedCapture>,
}
impl PendingNativeDelivery {
    fn new(sink: Arc<NativeDelivery>, accepted: NativeAcceptedCapture) -> Self {
        sink.pending.fetch_add(1, Ordering::AcqRel);
        Self {
            sink,
            accepted: Some(accepted),
        }
    }
    fn deliver(mut self) {
        self.sink
            .deliver(self.accepted.take().expect("original delivery"));
    }
}
impl Drop for PendingNativeDelivery {
    fn drop(&mut self) {
        self.sink.pending.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Opaque subscription to one original epoch. Clones identify that same sink;
/// no serialized selection can recreate or replace this native capability.
#[derive(Clone)]
pub(in crate::authority) struct NativeCaptureSubscription(Arc<NativeDelivery>);
impl NativeCaptureSubscription {
    pub fn current(&self) -> bool {
        self.0.current()
    }
    pub fn selection(&self) -> &NativeCaptureSelection {
        &self.0.selection
    }
    pub fn retire(&self) {
        self.0.retire();
    }
}
impl std::fmt::Debug for NativeCaptureSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCaptureSubscription")
            .field("selection", self.selection())
            .field("active", &self.current())
            .finish()
    }
}
#[derive(Default)]
pub(super) struct Registry {
    sweeper_started: bool,
    entries: HashMap<String, Entry>,
    successors: HashMap<ArtifactId, successor::SuccessorEntry>,
    #[cfg(test)]
    read_order_test: Mutex<Option<Arc<read_order_test::CaptureReadPause>>>,
}
impl Registry {
    /// The caller already owns current actor/project authority. This operation
    /// only inspects the original native capture; it never reacquires the DB,
    /// changes a lease, subscribes a consumer or replays its latest sample.
    pub(in crate::authority) fn native_source_basis(
        &self,
        selection: &NativeCaptureSelection,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.native_source_basis_inner(selection, false, None)
    }
    pub(in crate::authority) fn bind_native_cpu_mode(
        &self,
        mut selection: NativeCaptureSelection,
    ) -> PResult<NativeCaptureSelection> {
        let entry = self
            .entries
            .get(selection.identity.epoch.as_str())
            .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "capture epoch is absent"))?;
        let info = entry
            .report
            .inference
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("capture inference is absent"))?;
        if selection.cpu_manifest_sha256.is_some()
            || entry.owner != selection.owner
            || entry.report.identity != selection.identity
            || info.attempt_id != selection.attempt_id
            || info.profile_sha256 != selection.profile_sha256
            || entry.report.options.browser_media.as_ref() != Some(&selection.browser_media)
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "CPU mode binding differs from original capture subject",
            ));
        }
        selection.cpu_manifest_sha256 = cpu_manifest_hash(info)?;
        Ok(selection)
    }
    fn native_source_basis_inner(
        &self,
        selection: &NativeCaptureSelection,
        existing_subscription: bool,
        timing_claim: Option<&super::browser_timing::BrowserTimingClaim<'_>>,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.native_source_basis_scoped(selection, existing_subscription, false, timing_claim, true)
    }

    fn native_source_basis_scoped(
        &self,
        selection: &NativeCaptureSelection,
        existing_subscription: bool,
        successor_scope: bool,
        timing_claim: Option<&super::browser_timing::BrowserTimingClaim<'_>>,
        retire_failures: bool,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.try_native_source_basis_scoped(
            selection,
            existing_subscription,
            successor_scope,
            timing_claim,
            retire_failures,
        )
        .map_err(successor::NativeCaptureTransferFailure::into_protocol)
    }
    fn try_native_source_basis_scoped(
        &self,
        selection: &NativeCaptureSelection,
        existing_subscription: bool,
        successor_scope: bool,
        timing_claim: Option<&super::browser_timing::BrowserTimingClaim<'_>>,
        retire_failures: bool,
    ) -> Result<NativeCaptureSourceSnapshot, successor::NativeCaptureTransferFailure> {
        let entry = self
            .entries
            .get(selection.identity.epoch.as_str())
            .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "capture epoch is absent"))?;
        if entry.native_successor.is_some() && !successor_scope {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "pending successor is not an ordinary source",
            )
            .into());
        }
        // A foreign selection must not retire another actor's source/sink.
        let info = entry.report.inference.as_ref().ok_or_else(|| {
            ProtocolError::new(ErrorCode::Unavailable, "capture inference is absent")
        })?;
        if entry.owner != selection.owner
            || entry.report.identity != selection.identity
            || info.attempt_id != selection.attempt_id
            || info.profile_sha256 != selection.profile_sha256
            || cpu_manifest_hash(info)? != selection.cpu_manifest_sha256
            || entry.report.options.browser_media.as_ref() != Some(&selection.browser_media)
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "native capture selection differs from its original subject",
            )
            .into());
        }
        if !existing_subscription && entry.native_input_close.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "new source binding requires open capture input",
            )
            .into());
        }
        let checked =
            (|| -> Result<NativeCaptureSourceSnapshot, successor::NativeCaptureTransferFailure> {
                let sealed = existing_subscription && self.transfer_joined_current(entry)
                    || existing_subscription
                        && entry
                            .native_input_close
                            .as_ref()
                            .is_some_and(|input| input.end.is_some());
                selection.browser_media.validate()?;
                let runtime = entry.runtime.as_ref().ok_or_else(|| {
                    ProtocolError::new(ErrorCode::Unavailable, "native capture is retired")
                })?;
                if !entry.report.active
                    || entry.cancel.load(Ordering::Acquire)
                    || finishing(entry)
                    || !(info.state == LiveCaptureInferenceState::Running
                        || (sealed
                            && matches!(
                                info.state,
                                LiveCaptureInferenceState::InputSealed
                                    | LiveCaptureInferenceState::TransferJoined
                            )))
                    || info.cuda_manifest_sha256.is_some()
                    || entry.report.options.encoded_source.is_some()
                    || entry
                        .report
                        .options
                        .inference
                        .as_ref()
                        .is_none_or(|inference| {
                            inference.profile.provider != InferenceProvider::Cpu
                                || crate::inference_profiles::profile_identity(&inference.profile)
                                    .map_or(true, |id| id.sha256 != selection.profile_sha256)
                        })
                    || entry.report.options.source_time_basis
                        != LiveSourceTimeBasis::SourceMediaPresentation
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "native source requires the original running browser CPU capture",
                    )
                    .into());
                }
                let source_media = entry
                    .browser_media
                    .as_ref()
                    .filter(|pin| pin.current())
                    .ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::Unavailable,
                            "native browser source is retired",
                        )
                    })?;
                let declaration =
                    entry
                        .report
                        .options
                        .browser_analysis
                        .as_ref()
                        .ok_or_else(|| {
                            ProtocolError::new(
                                ErrorCode::Unavailable,
                                "native source lacks an original browser analysis declaration",
                            )
                        })?;
                declaration.validate()?;
                let browser_timing = entry.browser_analysis_pin.as_ref().ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "native browser analysis timing pin is absent",
                    )
                })?;
                let binding = browser_timing.binding();
                if binding.binding_id != declaration.timing_binding_id
                    || binding.player != declaration.player
                    || binding.browser_media != selection.browser_media
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "native browser analysis timing subject differs",
                    )
                    .into());
                }
                let lease_ns = u64::from(entry.report.options.lease_ms)
                    .checked_mul(1_000_000)
                    .ok_or_else(|| ProtocolError::invalid("capture lease overflow"))?;
                // A fresh read selects its immutable publication before the
                // capture clock sample. A supplied claim keeps its original
                // guarded observation and evaluation semantics.
                let selected_timing = if timing_claim.is_none() {
                    Some(browser_timing.select_read()?)
                } else {
                    None
                };
                #[cfg(test)]
                let read_test = if timing_claim.is_none() {
                    CaptureReadGuard::take(
                        self,
                        &CaptureReadTestTarget::Basis(selection.identity.epoch.clone()),
                    )
                } else {
                    CaptureReadGuard::default()
                };
                #[cfg(test)]
                read_test.enter(CaptureReadTestPhase::BeforeSample, None)?;
                let elapsed = tick(entry)?;
                if elapsed.0 >= lease_ns
                    || entry.created.elapsed() >= Duration::from_nanos(lease_ns)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "native capture lease expired",
                    )
                    .into());
                }
                let native_now = entry.native_clock.latest_native()?;
                #[cfg(test)]
                read_test.enter(
                    CaptureReadTestPhase::AfterSample,
                    Some(native_now.latest_ns),
                )?;
                let visible_current = match timing_claim {
                    Some(claim) => {
                        let observed =
                            claim.observation_for(browser_timing, native_now.latest_ns)?;
                        #[cfg(test)]
                        read_test.observed(observed);
                        observed.frame.is_current()
                    }
                    None => {
                        let observed = selected_timing
                            .as_ref()
                            .expect("unclaimed read selected its publication")
                            .evaluate_at(native_now.latest_ns)?;
                        #[cfg(test)]
                        read_test.observed(&observed);
                        observed.frame.is_current()
                    }
                };
                if !visible_current {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "native browser analysis player is not currently visible and playing",
                    )
                    .into());
                }
                let deadline = entry.native_clock.lease_deadline(lease_ns)?;
                if native_now.latest_ns >= deadline {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "native capture lease interval expired",
                    )
                    .into());
                }
                let continuity_generation = runtime.status().pair_discontinuities;
                if let Some(stamp) = &entry.native_accepted {
                    if let Some(error) = (stamp::Observation {
                        now_latest_ns: native_now.latest_ns,
                        capture_earliest_ns: stamp.capture_boottime.earliest_ns,
                        maximum_age_ns: entry.report.options.policy.maximum_analysis_age_ns,
                        stamped_generation: stamp.continuity_generation,
                        current_generation: continuity_generation,
                        sealed,
                        successor_scope,
                    })
                    .refusal()
                    {
                        return Err(error.into());
                    }
                }
                let mut report = entry.report.clone();
                report.server_elapsed = Some(elapsed);
                report.queue = Some(runtime.status().queue);
                if let Some(latest) = report
                    .inference
                    .as_mut()
                    .and_then(|info| info.latest.as_mut())
                {
                    latest.capture_age_ns = elapsed
                        .0
                        .checked_sub(latest.capture_tick.0)
                        .ok_or_else(|| ProtocolError::invalid("capture age regressed"))?;
                    latest.fresh =
                        latest.capture_age_ns <= report.options.policy.maximum_analysis_age_ns;
                }
                report.validate()?;
                Ok(NativeCaptureSourceSnapshot {
                    selection: selection.clone(),
                    report,
                    source_media: source_media.clone(),
                    browser_timing: browser_timing.clone(),
                    latest: entry.native_accepted.clone(),
                    native_now,
                    lease_deadline_boottime_ns: deadline,
                    continuity_generation,
                })
            })();
        // Transfer-scoped callers retain the exact proof on an unavailable
        // observation. They still cannot claim until all original checks pass;
        // cancellation, expiry and revocation are independently swept.
        if retire_failures && checked.is_err() {
            entry.retire_native_delivery();
        }
        checked
    }

    /// One original sink per epoch. Callers may inspect the basis and subscribe
    /// under this same registry guard. Registration never offers `basis.latest`.
    /// The caller must retain its consumer owner. Registry/subscription custody
    /// is weak so a consumer may retain this subscription without forming a
    /// cycle; a dropped consumer permanently retires its original subscription.
    pub(in crate::authority) fn register_native_delivery(
        &mut self,
        selection: NativeCaptureSelection,
        sink: Arc<dyn NativeCaptureSink>,
    ) -> PResult<NativeCaptureSubscription> {
        self.register_native_delivery_scoped(selection, sink, false)
    }
    fn register_native_delivery_scoped(
        &mut self,
        selection: NativeCaptureSelection,
        sink: Arc<dyn NativeCaptureSink>,
        successor_scope: bool,
    ) -> PResult<NativeCaptureSubscription> {
        let basis = if successor_scope {
            self.native_source_basis_scoped(&selection, false, true, None, false)?
        } else {
            self.native_source_basis_inner(&selection, false, None)?
        };
        let entry = self
            .entries
            .get_mut(selection.identity.epoch.as_str())
            .expect("checked epoch");
        let sink_ref = Arc::downgrade(&sink);
        if let Some(existing) = &entry.native_delivery {
            if existing.selection == selection
                && existing.sink.ptr_eq(&sink_ref)
                && existing.current()
            {
                return Ok(NativeCaptureSubscription(existing.clone()));
            }
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "capture epoch already has its original native delivery",
            ));
        }
        let sequence = entry
            .report
            .inference
            .as_ref()
            .expect("checked CPU inference")
            .completed
            .checked_add(1)
            .ok_or_else(|| ProtocolError::invalid("native delivery sequence exhausted"))?;
        let delivery = Arc::new(NativeDelivery {
            selection,
            sink: sink_ref,
            active: AtomicBool::new(true),
            capture_cancel: entry.cancel.clone(),
            continuity_generation: basis.continuity_generation,
            next_sequence: std::sync::atomic::AtomicU64::new(sequence),
            delivering: AtomicBool::new(false),
            pending: std::sync::atomic::AtomicUsize::new(0),
            last_delivered: Mutex::new(None),
            finish_claimed: AtomicBool::new(false),
            #[cfg(test)]
            test_hold: Mutex::new(None),
        });
        entry.native_delivery = Some(delivery.clone());
        Ok(NativeCaptureSubscription(delivery))
    }

    #[cfg(test)]
    pub(in crate::authority) fn native_source_snapshot(
        &self,
        subscription: &NativeCaptureSubscription,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.native_source_snapshot_inner(subscription, None)
    }
    /// The caller retains the exact native timing registry claim through its
    /// final dispatch gate. Validate that claim at this capture's freshly read
    /// native time instead of copying an independently changing observation.
    pub(in crate::authority) fn native_source_snapshot_claimed(
        &self,
        subscription: &NativeCaptureSubscription,
        timing_claim: &super::browser_timing::BrowserTimingClaim<'_>,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.native_source_snapshot_inner(subscription, Some(timing_claim))
    }
    fn native_source_snapshot_inner(
        &self,
        subscription: &NativeCaptureSubscription,
        timing_claim: Option<&super::browser_timing::BrowserTimingClaim<'_>>,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        let selected = subscription.selection();
        let entry = self
            .entries
            .get(selected.identity.epoch.as_str())
            .ok_or_else(|| {
                subscription.retire();
                ProtocolError::new(ErrorCode::Unavailable, "original native capture is absent")
            })?;
        if !subscription.current()
            || entry
                .native_delivery
                .as_ref()
                .is_none_or(|stored| !Arc::ptr_eq(stored, &subscription.0))
        {
            subscription.retire();
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "native capture subscription is retired or replaced",
            ));
        }
        let basis = self
            .native_source_basis_inner(selected, true, timing_claim)
            .inspect_err(|_| subscription.retire())?;
        if basis.continuity_generation != subscription.0.continuity_generation {
            subscription.retire();
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "native capture source pairing changed",
            ));
        }
        Ok(basis)
    }
    #[cfg(test)]
    pub(in crate::authority) fn hold_next_native_delivery_for_test(
        &self,
        identity: &LiveIdentity,
    ) -> NativeDeliveryTestHold {
        let entry = self
            .entries
            .get(identity.epoch.as_str())
            .expect("authored active capture");
        assert_eq!(&entry.report.identity, identity);
        let delivery = entry
            .native_delivery
            .as_ref()
            .expect("registered native consumer");
        let pause = Arc::new(NativeDeliveryPause {
            entered: AtomicBool::new(false),
            released: AtomicBool::new(false),
        });
        assert!(delivery
            .test_hold
            .lock()
            .unwrap()
            .replace(pause.clone())
            .is_none());
        NativeDeliveryTestHold {
            pause,
            delivery: Arc::downgrade(delivery),
        }
    }

    pub(super) fn cancel_all(&mut self) {
        for entry in self.entries.values_mut() {
            entry.cancel.store(true, Ordering::Release);
            let _ = retire(entry, LiveCaptureRetirement::Stopped);
        }
    }
}
pub(super) fn is_command(command: &Command) -> bool {
    successor::is_command(command)
        || matches!(
            command,
            Command::LiveCaptureLimits
                | Command::StartLiveCapture { .. }
                | Command::ContinueLiveCapture { .. }
                | Command::ContinueLiveCaptureAt { .. }
                | Command::LiveCaptureStatus { .. }
                | Command::StartLiveRecording { .. }
                | Command::LiveRecordingPage { .. }
                | Command::DiscardLiveRecording { .. }
                | Command::StopLiveCapture { .. }
                | Command::FinishLiveCapture { .. }
                | Command::CloseBrowserAnalysisInput { .. }
        )
}
fn start_sweeper(engine: &Arc<Engine>) -> PResult<()> {
    let weak = Arc::downgrade(engine);
    engine
        .spawn_task("pulsar-live-leases", None, move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let Some(engine) = weak.upgrade() else {
                break;
            };
            if engine.is_shutting_down() {
                break;
            }
            let _ = engine.sweep_live_captures();
        })
        .map_err(internal)?;
    Ok(())
}
fn tick(entry: &Entry) -> PResult<LiveClockTick> {
    entry.native_clock.tick(entry.created).inspect_err(|_| {
        entry.cancel.store(true, Ordering::Release);
        entry.retire_native_delivery();
    })
}
fn finishing(entry: &Entry) -> bool {
    entry.report.inference.as_ref().is_some_and(|i| {
        matches!(
            i.state,
            LiveCaptureInferenceState::FinishRequested | LiveCaptureInferenceState::Finishing
        )
    })
}
fn retire(entry: &mut Entry, reason: LiveCaptureRetirement) -> PResult<()> {
    entry.cancel.store(true, Ordering::Release);
    let (now, reason) = match tick(entry) {
        Ok(now) => (now, reason),
        Err(_) => (
            entry.native_clock.retirement_tick(),
            LiveCaptureRetirement::AuthorityLost,
        ),
    };
    retire_at(entry, reason, now)
}
fn retire_at(entry: &mut Entry, reason: LiveCaptureRetirement, now: LiveClockTick) -> PResult<()> {
    entry.cancel.store(true, Ordering::Release);
    entry.retire_native_delivery();
    entry.browser_media = None;
    entry.browser_analysis_pin = None;
    entry.native_accepted = None;
    if let Some(input) = &mut entry.report.input_close {
        if input.phase != LiveCaptureInputPhase::Sealed {
            input.phase = LiveCaptureInputPhase::Aborted;
            input.disposition = Some(if reason == LiveCaptureRetirement::WorkerFailed {
                LiveCaptureInputFinalDisposition::Failed
            } else {
                LiveCaptureInputFinalDisposition::Retired
            });
        }
    }
    if let Some(mut runtime) = entry.runtime.take() {
        runtime.retire(now)?;
    }
    entry.report.active = false;
    if let Some(source) = &mut entry.report.encoded_source {
        source.state = if source.error.is_some() {
            LiveEncodedSourceState::Failed
        } else {
            LiveEncodedSourceState::Retired
        };
    }
    if let Some(info) = &mut entry.report.inference {
        info.latest = None;
        info.state = if reason == LiveCaptureRetirement::WorkerFailed {
            LiveCaptureInferenceState::Failed
        } else {
            LiveCaptureInferenceState::Retired
        };
        if info.state != LiveCaptureInferenceState::Failed {
            info.error = None;
        }
    }
    entry.report.retirement = Some(reason);
    entry.report.server_elapsed = Some(now);
    entry.report.admitted_pixel_bytes = 0;
    Ok(())
}
fn report(entry: &Entry) -> PResult<LiveCaptureReport> {
    let mut report = entry.report.clone();
    report.queue = entry.runtime.as_ref().map(|runtime| runtime.status().queue);
    if report.active {
        report.server_elapsed = Some(tick(entry)?);
    }
    if let Some(latest) = report
        .inference
        .as_mut()
        .and_then(|info| info.latest.as_mut())
    {
        latest.capture_age_ns = report
            .server_elapsed
            .expect("active result")
            .0
            .checked_sub(latest.capture_tick.0)
            .ok_or_else(|| ProtocolError::invalid("capture age regressed"))?;
        latest.fresh = latest.capture_age_ns <= report.options.policy.maximum_analysis_age_ns;
    }
    report.validate()?;
    Ok(report)
}
fn pending_predecessor(entry: &Entry) -> Option<&LiveIdentity> {
    if entry.runtime.is_some()
        && entry.report.options.continuation_mode == LiveContinuationMode::WhenReady
        && entry
            .report
            .inference
            .as_ref()
            .is_some_and(|i| i.state == LiveCaptureInferenceState::Starting)
    {
        entry.report.continuation.as_ref().map(|c| &c.predecessor)
    } else {
        None
    }
}
#[cfg(all(test, feature = "cuda"))]
pub(super) struct TestCaptureSession {
    pub owner: SessionId,
    pub created: Instant,
    pub engine_epoch: AttemptId,
}

impl Registry {
    pub(super) fn recording_pages(
        &self,
        request: &Request,
        session: &SessionId,
        options: &LiveRecordingStageOptions,
    ) -> PResult<Vec<LiveRecordingPage>> {
        options.validate()?;
        let mut pages = Vec::new();
        let mut total = 0u32;
        for selected in &options.recordings {
            let entry = self.entries.get(selected.epoch.as_str()).ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::NotFound,
                    "recording scope absent from this Engine boot",
                )
            })?;
            if entry.owner != *session || entry.report.identity.project != *project(request)? {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "recording belongs to another actor or project",
                ));
            }
            if request.expected_revision != Some(entry.report.identity.revision) {
                return Err(ProtocolError::new(
                    ErrorCode::RevisionConflict,
                    "recording admitted revision differs",
                ));
            }
            if entry.report.engine_epoch != options.engine_epoch {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "recording Engine boot differs",
                ));
            }
            let mut after = 0;
            loop {
                let ResponseBody::LiveRecordingPage(page) = recording::page(entry, after, 64)?
                else {
                    unreachable!()
                };
                if page.recording_request_id != selected.recording_request_id {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "recording original intent differs",
                    ));
                }
                if page.active || page.discarded {
                    return Err(ProtocolError::invalid(
                        "stage stopped, retained recordings only",
                    ));
                }
                if after == 0 {
                    total = total
                        .checked_add(page.total_samples)
                        .ok_or_else(|| ProtocolError::invalid("recording aggregate overflow"))?;
                    if total > 4096 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "candidate exceeds 4096 accepted samples",
                        ));
                    }
                }
                let next = page.next;
                pages.push(*page);
                if pages.len() > 256 {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "candidate recording pages exceeded",
                    ));
                }
                let Some(cursor) = next else {
                    break;
                };
                after = cursor;
            }
        }
        Ok(pages)
    }
    fn activate_prepared(&mut self, epoch: &AttemptId) -> PResult<()> {
        let entry = self.entries.get(epoch.as_str()).expect("validated Ready");
        let Some(prior_identity) = pending_predecessor(entry).cloned() else {
            return Ok(());
        };
        let facts = entry
            .report
            .continuation
            .as_ref()
            .expect("prepared predecessor");
        let prior = self
            .entries
            .get(prior_identity.epoch.as_str())
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "prepared predecessor is absent",
                )
            })?;
        if prior.runtime.is_none()
            || !prior.report.active
            || finishing(prior)
            || prior.cancel.load(Ordering::Acquire)
            || prior.report.identity != prior_identity
            || prior.owner != entry.owner
            || prior.report.engine_epoch != entry.report.engine_epoch
            || prior.report.clock != facts.predecessor_clock
            || prior.created.elapsed()
                >= Duration::from_millis(u64::from(prior.report.options.lease_ms))
            || entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            || serde_json::to_value(&prior.report.options.inference).map_err(internal)?
                != serde_json::to_value(&entry.report.options.inference).map_err(internal)?
            || serde_json::to_value(&prior.report.options.encoded_source).map_err(internal)?
                != serde_json::to_value(&entry.report.options.encoded_source).map_err(internal)?
            || prior.report.options.policy != entry.report.options.policy
            || prior.report.options.browser_media != entry.report.options.browser_media
            || entry
                .browser_media
                .as_ref()
                .is_some_and(|pin| !pin.current())
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "prepared predecessor changed before Ready",
            ));
        }
        facts.validate(&entry.report.identity, &entry.report.options)?;
        retire(
            self.entries
                .get_mut(prior_identity.epoch.as_str())
                .expect("validated predecessor"),
            LiveCaptureRetirement::Superseded,
        )
    }
    /// Stages authored state only; no worker, receipt or native evidence is created.
    #[cfg(test)]
    pub(super) fn prepare_for_test(
        &mut self,
        engine: &Engine,
        predecessor: &AttemptId,
    ) -> (AttemptId, AttemptId) {
        let prior = self.entries.get(predecessor.as_str()).unwrap();
        let prior_report = report(prior).unwrap();
        let mut next = prior_report.clone();
        let epoch = id!(AttemptId);
        let attempt = id!(AttemptId);
        next.options = prior_report
            .continuation_options(SourceTimestamp::new(1, 1).unwrap(), 30_000)
            .unwrap();
        next.options.continuation_mode = LiveContinuationMode::WhenReady;
        next.identity.epoch = epoch.clone();
        next.clock = next.options.clock(epoch.clone());
        next.continuation = Some(LiveCaptureContinuation {
            predecessor: prior_report.identity,
            predecessor_clock: prior_report.clock,
            predecessor_elapsed: prior_report.server_elapsed.unwrap(),
            predecessor_last_frame: prior_report.last_frame,
        });
        next.received_frames = 0;
        next.last_frame = None;
        next.queue = None;
        next.server_elapsed = Some(LiveClockTick(0));
        let info = next.inference.as_mut().unwrap();
        info.attempt_id = attempt.clone();
        info.state = LiveCaptureInferenceState::Starting;
        info.completed = 0;
        info.accepted = 0;
        info.stale = 0;
        info.latest = None;
        let runtime = LiveRgbRuntime::new(
            &engine.pool,
            next.identity.clone(),
            next.options.policy,
            Some(next.clock.clone()),
            LiveClockTick(0),
        )
        .unwrap();
        next.validate().unwrap();
        let created = Instant::now();
        let entry = Entry {
            owner: prior.owner.clone(),
            native_clock: clock::NativeCaptureClock::new(created),
            native_accepted: None,
            native_delivery: None,
            native_input_close: None,
            native_successor: None,
            native_transfer: None,
            native_transfer_joined: false,
            created,
            report: next,
            runtime: Some(runtime),
            cancel: Arc::new(AtomicBool::new(false)),
            origin: None,
            recording: None,
            browser_media: prior.browser_media.clone(),
            browser_analysis_pin: None,
        };
        self.entries.insert(epoch.to_string(), entry);
        (epoch, attempt)
    }
    #[cfg(test)]
    pub(super) fn arm_for_test(&mut self, epoch: &AttemptId, manifest: &LiveInferenceManifest) {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        entry.report.options.inference = Some(LiveCaptureInference {
            model_path: manifest.model.path.clone(),
            profile: manifest.profile.clone(),
        });
        let mut info = inference::initial_report(manifest).unwrap();
        info.state = LiveCaptureInferenceState::Running;
        entry.report.inference = Some(info);
    }
    #[cfg(test)]
    pub(super) fn cuda_for_test(&mut self, epoch: &AttemptId) -> String {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        let profile = &mut entry.report.options.inference.as_mut().unwrap().profile;
        profile.schema_version = 2;
        profile.provider = InferenceProvider::Cuda;
        profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 0,
            device_uuid: "GPU-12345678-1234-1234-1234-123456789abc".into(),
            address_space_bytes: 8 << 30,
            device_reservation_bytes: 96 << 20,
            arena_limit_bytes: 64 << 20,
        });
        let hash = crate::inference_profiles::profile_identity(profile)
            .unwrap()
            .sha256;
        let info = entry.report.inference.as_mut().unwrap();
        info.profile_sha256 = hash.clone();
        info.cuda_manifest_sha256 = Some("b".repeat(64));
        hash
    }
    /// Authored phase fixture only; it supplies no native worker or publication authority.
    #[cfg(test)]
    pub(super) fn finishing_for_test(&mut self, epoch: &AttemptId) -> Arc<AtomicBool> {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        entry.retire_native_delivery();
        entry.native_accepted = None;
        let info = entry.report.inference.as_mut().unwrap();
        info.state = LiveCaptureInferenceState::Finishing;
        info.latest = None;
        report(entry).unwrap();
        entry.cancel.clone()
    }
    #[cfg(all(test, feature = "cuda"))]
    pub(super) fn insert_cuda_for_test(
        &mut self,
        session: TestCaptureSession,
        stream: &LiveInferenceManifest,
        options: LiveCaptureOptions,
        runtime: LiveRgbRuntime,
        cancel: Arc<AtomicBool>,
    ) {
        let TestCaptureSession {
            owner,
            created,
            engine_epoch,
        } = session;
        let report = LiveCaptureReport {
            input_close: None,
            continuation: None,
            age_basis: LiveCaptureAgeBasis::ProducerDeclared,
            encoded_source: None,
            engine_epoch,
            identity: stream.identity.clone(),
            clock: stream.clock.clone(),
            options,
            active: true,
            retirement: None,
            server_elapsed: Some(LiveClockTick(0)),
            admitted_pixel_bytes: crate::live_frames::LIVE_RGB_ADMISSION_BYTES,
            received_frames: 0,
            last_frame: None,
            inference: Some(inference::initial_report(stream).unwrap()),
            queue: None,
        };
        report.validate().unwrap();
        assert!(!self.entries.contains_key(stream.identity.epoch.as_str()));
        self.entries.insert(
            stream.identity.epoch.to_string(),
            Entry {
                owner,
                native_clock: clock::NativeCaptureClock::new(created),
                native_accepted: None,
                native_delivery: None,
                native_input_close: None,
                native_successor: None,
                native_transfer: None,
                native_transfer_joined: false,
                created,
                report,
                runtime: Some(runtime),
                cancel,
                origin: None,
                recording: None,
                browser_media: None,
                browser_analysis_pin: None,
            },
        );
    }
    /// Native-only future-producer seam. This reads an existing accepted CPU
    /// result and never renews the capture, creates HSP ownership or qualifies
    /// its source clock. Unsupported clock acquisition is an explicit error.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn native_accepted(
        &self,
        owner: &SessionId,
        identity: &LiveIdentity,
    ) -> PResult<Option<&clock::NativeAcceptedCapture>> {
        let entry = self
            .entries
            .get(identity.epoch.as_str())
            .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "capture epoch is absent"))?;
        if entry.owner != *owner || entry.report.identity != *identity {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "native capture subject differs",
            ));
        }
        if entry.runtime.is_none()
            || !entry.report.active
            || finishing(entry)
            || entry.cancel.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        tick(entry)?;
        let now = entry.native_clock.latest_native()?;
        let Some(stamp) = &entry.native_accepted else {
            return Ok(None);
        };
        let Some(age) = now
            .latest_ns
            .checked_sub(stamp.capture_boottime.earliest_ns)
        else {
            return Err(ProtocolError::invalid("native capture age regressed"));
        };
        if age > entry.report.options.policy.maximum_analysis_age_ns
            || entry
                .runtime
                .as_ref()
                .expect("active")
                .status()
                .pair_discontinuities
                != stamp.continuity_generation
        {
            return Ok(None);
        }
        Ok(Some(stamp))
    }
    #[cfg(test)]
    pub(super) fn suspend_for_test(&self, epoch: &AttemptId) {
        self.entries
            .get(epoch.as_str())
            .unwrap()
            .native_clock
            .advance_for_test(60_000_000_000);
    }
    #[cfg(test)]
    pub(super) fn unavailable_clock_for_test(&self, epoch: &AttemptId) {
        self.entries
            .get(epoch.as_str())
            .unwrap()
            .native_clock
            .unavailable_for_test();
    }
    #[cfg(test)]
    pub(super) fn age_for_test(&mut self, epoch: &AttemptId) {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        entry.created -= Duration::from_millis(201);
        entry.native_clock.advance_for_test(201_000_000);
    }
    #[cfg(test)]
    pub(super) fn take_for_test(&mut self, epoch: &AttemptId) -> crate::live_frames::LiveRgbWork {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        let now = tick(entry).unwrap();
        entry.runtime.as_mut().unwrap().take(now).unwrap().unwrap()
    }
    #[cfg(test)]
    pub(super) fn validate_report_for_test(&self, epoch: &AttemptId) {
        report(self.entries.get(epoch.as_str()).unwrap())
            .unwrap()
            .validate()
            .unwrap();
    }
    #[cfg(test)]
    pub(super) fn active_for_test(&self, epoch: &AttemptId) -> bool {
        self.entries
            .get(epoch.as_str())
            .is_some_and(|entry| entry.runtime.is_some())
    }
    #[cfg(test)]
    pub(super) fn expire_for_test(&mut self, epoch: &AttemptId) {
        let entry = self.entries.get_mut(epoch.as_str()).unwrap();
        entry.created -= Duration::from_secs(31);
        entry.native_clock.advance_for_test(31_000_000_000);
    }
    pub(in crate::authority) fn sweep(&mut self, db: &Connection) -> PResult<()> {
        let transfer_joined: std::collections::HashSet<_> = self
            .entries
            .values()
            .filter(|entry| self.transfer_joined_current(entry))
            .map(|entry| entry.report.identity.epoch.clone())
            .collect();
        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.runtime.is_some())
        {
            if tick(entry).is_err() {
                retire(entry, LiveCaptureRetirement::AuthorityLost)?;
                continue;
            }
            if let Some(delivery) = &entry.native_delivery {
                let sealed = transfer_joined.contains(&entry.report.identity.epoch)
                    || entry
                        .native_input_close
                        .as_ref()
                        .is_some_and(|input| input.end.is_some());
                let stale = !sealed
                    && entry.native_accepted.as_ref().is_some_and(|stamp| {
                        entry.native_clock.latest_native().map_or(true, |now| {
                            now.latest_ns
                                .checked_sub(stamp.capture_boottime.earliest_ns)
                                .is_none_or(|age| {
                                    age > entry.report.options.policy.maximum_analysis_age_ns
                                })
                        })
                    });
                if finishing(entry)
                    || stale
                    || entry
                        .runtime
                        .as_ref()
                        .expect("active capture")
                        .status()
                        .pair_discontinuities
                        != delivery.continuity_generation
                {
                    delivery.retire();
                }
            }
            if entry.report.options.browser_media.is_some() != entry.browser_media.is_some()
                || entry
                    .browser_media
                    .as_ref()
                    .is_some_and(|pin| !pin.current())
            {
                retire(entry, LiveCaptureRetirement::AuthorityLost)?;
                continue;
            }
            if entry.report.options.browser_analysis.is_some()
                != entry.browser_analysis_pin.is_some()
                || entry.browser_analysis_pin.as_ref().is_some_and(|pin| {
                    // Passive retirement depends on the admitted lifetime. A
                    // contended or newer phase snapshot supplies no evidence
                    // that this lifetime expired or was revoked. Binding and
                    // dispatch still acquire their exact current observations.
                    entry
                        .native_clock
                        .latest_native()
                        .map_or(true, |now| !pin.current(now.latest_ns))
                })
            {
                retire(entry, LiveCaptureRetirement::AuthorityLost)?;
                continue;
            }
            if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                retire(entry, LiveCaptureRetirement::Expired)?;
                continue;
            }
            let current: Option<i64> = db
                .query_row(
                    "SELECT revision FROM projects WHERE id=?1",
                    [entry.report.identity.project.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(internal)?;
            let reason = if current != Some(rev_sql(entry.report.identity.revision)?) {
                Some(LiveCaptureRetirement::RevisionChanged)
            } else if entry
                .report
                .options
                .model_pack
                .as_ref()
                .is_some_and(|choice| {
                    crate::model_pack_selection::status(db)
                        .ok()
                        .and_then(|s| s.choice)
                        .as_ref()
                        != Some(choice)
                })
                || [Scope::Read, Scope::ImportSource, Scope::Generate]
                    .iter()
                    .any(|scope| {
                        require_grant(db, &entry.owner, &entry.report.identity.project, *scope)
                            .is_err()
                    })
            {
                Some(LiveCaptureRetirement::AuthorityLost)
            } else if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                Some(LiveCaptureRetirement::Expired)
            } else {
                None
            };
            if let Some(reason) = reason {
                retire(entry, reason)?;
            }
        }
        let orphaned: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(epoch, entry)| {
                let prior = pending_predecessor(entry)?;
                let valid = self.entries.get(prior.epoch.as_str()).is_some_and(|p| {
                    p.runtime.is_some()
                        && p.report.active
                        && !finishing(p)
                        && p.report.identity == *prior
                        && p.owner == entry.owner
                        && p.report.engine_epoch == entry.report.engine_epoch
                });
                (!valid).then(|| epoch.clone())
            })
            .collect();
        for epoch in orphaned {
            inference::fail_entry(
                self.entries.get_mut(&epoch).expect("pending dependency"),
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "prepared predecessor retired before Ready",
                ),
            )?;
        }
        while self.entries.len() > MAX_RETAINED {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| entry.runtime.is_none())
                .min_by_key(|(_, entry)| entry.created)
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.entries.remove(&id);
            } else {
                break;
            }
        }
        self.sweep_successors()?;
        Ok(())
    }
}
impl Engine {
    pub(super) fn sweep_live_captures(&self) -> PResult<()> {
        let db = self.db.lock().map_err(internal)?;
        self.live_captures.lock().map_err(internal)?.sweep(&db)?;
        drop(db);
        self.advance_browser_analysis_transfers()
    }
    pub(super) fn live_capture_command(
        self: &Arc<Self>,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        if successor::is_command(&request.command) {
            return self.browser_analysis_successor_command(request, session, fingerprint);
        }
        let original_request = request;
        let admission = match &request.command {
            Command::StartLiveCapture { options, .. }
            | Command::ContinueLiveCapture { options, .. } => options.model_pack.is_some(),
            Command::ContinueLiveCaptureAt { predecessor, .. } => self
                .live_captures
                .lock()
                .map_err(internal)?
                .entries
                .get(predecessor.as_str())
                .is_some_and(|entry| entry.report.options.model_pack.is_some()),
            _ => false,
        };
        let _pack_gate = admission
            .then(|| self.artifact_gate.lock().map_err(internal))
            .transpose()?;
        let resolved = if admission {
            Some(pack::resolve(self, request, session, fingerprint)?)
        } else {
            None
        };
        let request = resolved.as_ref().unwrap_or(request);
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        if matches!(request.command, Command::LiveCaptureLimits) {
            check_revision(&db, request)?;
            return Ok(ResponseBody::LiveCaptureLimits(LiveCaptureLimits {
                epoch_continuation: true,
                encoded_source_ingress: cfg!(target_os = "linux")
                    && self.tools.is_ok()
                    && launch::available(),
                engine_epoch: AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
                maximum_active: MAX_ACTIVE as u32,
                maximum_lease_ms: 30_000,
                native_acquisition: false,
                continuous_inference: self
                    .tools
                    .as_ref()
                    .ok()
                    .is_some_and(|tools| tools.onnx_runtime.is_some())
                    && launch::available(),
                physical_dispatch: false,
            }));
        }
        let admission = match &request.command {
            Command::StartLiveCapture {
                engine_epoch,
                options,
            } => Some((engine_epoch, Some(options), None, None)),
            Command::ContinueLiveCapture {
                engine_epoch,
                predecessor,
                options,
            } => Some((engine_epoch, Some(options), Some(predecessor), None)),
            Command::ContinueLiveCaptureAt {
                engine_epoch,
                predecessor,
                source_anchor,
                lease_ms,
            } => Some((
                engine_epoch,
                None,
                Some(predecessor),
                Some((*source_anchor, *lease_ms)),
            )),
            _ => None,
        };
        if let Some((engine_epoch, options, predecessor, derive_anchor)) = admission {
            check_revision(&db, request)?;
            if let Some(ResponseBody::LiveCapture(mut prior)) =
                replay(&db, session.as_str(), request, fingerprint)?
            {
                prior.validate()?;
                if prior.identity.project != *project(request)?
                    || Some(prior.identity.revision) != request.expected_revision
                    || prior.engine_epoch != *engine_epoch
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "capture start receipt differs from admitted scope",
                    ));
                }
                if let Some(entry) = registry.entries.get(prior.identity.epoch.as_str()) {
                    if entry.owner != *session || entry.report.identity != prior.identity {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "capture replay owner differs",
                        ));
                    }
                    return Ok(ResponseBody::LiveCapture((report(entry)?).into()));
                }
                prior.active = false;
                if let Some(source) = &mut prior.encoded_source {
                    source.state = LiveEncodedSourceState::Retired;
                    source.error = None;
                }
                prior.retirement = Some(if prior.engine_epoch.as_str() == self.engine_epoch {
                    LiveCaptureRetirement::LeaseEvicted
                } else {
                    LiveCaptureRetirement::EngineRestarted
                });
                prior.server_elapsed = None;
                prior.admitted_pixel_bytes = 0;
                if let Some(info) = &mut prior.inference {
                    info.state = LiveCaptureInferenceState::Retired;
                    info.error = None;
                    info.latest = None;
                }
                prior.validate()?;
                return Ok(ResponseBody::LiveCapture(prior));
            }
            // An exact receipt is historical observation, never launch authority.
            if engine_epoch.as_str() != self.engine_epoch {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "capture start belongs to a retired engine boot",
                ));
            }
            let derived_options;
            let options = if let Some(options) = options {
                options
            } else {
                let epoch = predecessor.expect("derived continuation");
                let entry = registry.entries.get(epoch.as_str()).ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, "continuation predecessor is absent")
                })?;
                if entry.owner != *session || entry.report.identity.project != *project(request)? {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "continuation predecessor belongs to another actor or project",
                    ));
                }
                if Some(entry.report.identity.revision) != request.expected_revision {
                    return Err(ProtocolError::new(
                        ErrorCode::RevisionConflict,
                        "continuation predecessor revision differs",
                    ));
                }
                let (anchor, lease_ms) = derive_anchor.expect("declared anchor");
                derived_options = report(entry)?.continuation_options(anchor, lease_ms)?;
                &derived_options
            };
            let prepared = options.continuation_mode == LiveContinuationMode::WhenReady;
            if predecessor.is_some_and(|epoch| {
                options.browser_analysis.is_some()
                    || registry
                        .entries
                        .get(epoch.as_str())
                        .is_some_and(|entry| entry.report.options.browser_analysis.is_some())
            }) {
                return Err(ProtocolError::invalid("browser analysis requires a fresh explicit epoch; continuation is not admitted"));
            }
            let continuation = predecessor
                .map(|epoch| {
                    let entry = registry.entries.get(epoch.as_str()).ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::NotFound,
                            "continuation predecessor is absent",
                        )
                    })?;
                    if entry.owner != *session
                        || entry.report.identity.project != *project(request)?
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::Forbidden,
                            "continuation predecessor belongs to another actor or project",
                        ));
                    }
                    if Some(entry.report.identity.revision) != request.expected_revision {
                        return Err(ProtocolError::new(
                            ErrorCode::RevisionConflict,
                            "continuation predecessor revision differs",
                        ));
                    }
                    if entry.runtime.is_none() || !entry.report.active || finishing(entry) {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "retired predecessor cannot rearm or continue",
                        ));
                    }
                    if prepared
                        && entry
                            .report
                            .inference
                            .as_ref()
                            .is_none_or(|i| i.state != LiveCaptureInferenceState::Running)
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "prepared continuation requires a running predecessor",
                        ));
                    }
                    // Source and preprocessing declarations cannot be relabeled as continuity.
                    if serde_json::to_value(&entry.report.options.encoded_source)
                        .map_err(internal)?
                        != serde_json::to_value(&options.encoded_source).map_err(internal)?
                        || serde_json::to_value(&entry.report.options.inference)
                            .map_err(internal)?
                            != serde_json::to_value(&options.inference).map_err(internal)?
                        || entry.report.options.model_pack != options.model_pack
                        || entry.report.options.policy != options.policy
                        || entry.report.options.source_time_basis != options.source_time_basis
                        || entry.report.options.browser_media != options.browser_media
                    {
                        return Err(ProtocolError::invalid(
                            "continuation source, inference and policy must remain unchanged",
                        ));
                    }
                    Ok(LiveCaptureContinuation {
                        predecessor: entry.report.identity.clone(),
                        predecessor_clock: entry.report.clock.clone(),
                        predecessor_elapsed: tick(entry)?,
                        predecessor_last_frame: entry.report.last_frame.clone(),
                    })
                })
                .transpose()?;
            // A serialized descriptor is provenance, never live read authority.
            // Resolve the native pin while the same project/revision lock is held.
            let browser_media = options
                .browser_media
                .as_ref()
                .map(|binding| self.pin_browser_media_locked(&db, request, binding))
                .transpose()?;
            let browser_analysis_pin = options.browser_analysis.as_ref().map(|declaration| {
                declaration.validate()?;
                let media = options.browser_media.as_ref().ok_or_else(|| {
                    ProtocolError::invalid("browser analysis requires exact imported browser media")
                })?;
                if registry.entries.values().any(|entry| {
                    entry.report.options.browser_analysis.as_ref().is_some_and(|old| {
                        old.feed_id == declaration.feed_id
                    })
                }) {
                    return Err(ProtocolError::new(ErrorCode::RequestConflict, "browser analysis feed already belongs to a retained capture epoch"));
                }
                let pin = self.browser_timing_scope(request, &declaration.timing_binding_id)?;
                if pin.binding().browser_media != *media || pin.binding().player != declaration.player {
                    return Err(ProtocolError::new(ErrorCode::DependencyMismatch, "browser analysis declaration differs from its native player and source"));
                }
                let selected_timing = pin.select_read().map_err(browser_timing_read_error)?;
                #[cfg(test)]
                let read_test = CaptureReadGuard::take(&registry, &CaptureReadTestTarget::AdmissionFirst(request.request_id.clone()));
                #[cfg(test)]
                read_test.enter(CaptureReadTestPhase::BeforeSample, None)?;
                let now = crate::handy_clock::boottime_ns()?;
                #[cfg(test)]
                read_test.enter(CaptureReadTestPhase::AfterSample, Some(now))?;
                let observed = selected_timing.evaluate_at(now).map_err(browser_timing_read_error)?;
                #[cfg(test)]
                read_test.observed(&observed);
                if !observed.frame.is_current() {
                    return Err(ProtocolError::new(ErrorCode::Unavailable, "browser analysis player is not currently visible and playing"));
                }
                Ok(pin)
            }).transpose()?;
            if registry
                .entries
                .values()
                .filter(|entry| entry.runtime.is_some())
                .count()
                >= MAX_ACTIVE
                && (predecessor.is_none() || prepared)
                || registry.entries.values().any(|entry| {
                    entry.runtime.is_some()
                        && Some(&entry.report.identity.epoch) != predecessor
                        && entry.owner == *session
                        && entry.report.identity.project == *project(request).expect("validated")
                })
            {
                return Err(ProtocolError::new(ErrorCode::ResourceExhausted,"capture lease capacity exhausted or actor already has an active project capture"));
            }
            if !registry.sweeper_started {
                start_sweeper(self)?;
                registry.sweeper_started = true;
            }
            let epoch = id!(AttemptId);
            let identity = LiveIdentity {
                epoch: epoch.clone(),
                project: project(request)?.clone(),
                revision: request.expected_revision.expect("validated"),
                source: continuation.as_ref().map_or_else(
                    || {
                        options.browser_media.as_ref().map_or_else(
                            || id!(SourceVersionId),
                            |binding| binding.selection.source_version.clone(),
                        )
                    },
                    |c| c.predecessor.source.clone(),
                ),
            };
            let clock = options.clock(epoch);
            if options.encoded_source.is_some() {
                source::check_available(self)?;
            }
            let manifest = inference::manifest(self, options, &identity, &clock)?;
            let info = manifest
                .as_ref()
                .map(|manifest| inference::initial_report_with_mode(self, manifest))
                .transpose()?;
            if let Some(epoch) = predecessor {
                let previous = registry
                    .entries
                    .get(epoch.as_str())
                    .expect("validated predecessor");
                if !inference::same_cpu_mode(previous.report.inference.as_ref(), info.as_ref())? {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "capture continuation changed its CPU execution image or mode",
                    ));
                }
            }
            let runtime = LiveRgbRuntime::new(
                &self.pool,
                identity.clone(),
                options.policy,
                Some(clock.clone()),
                LiveClockTick(0),
            )?;
            let report = LiveCaptureReport {
                input_close: None,
                continuation,
                age_basis: if options.encoded_source.is_some() {
                    LiveCaptureAgeBasis::DecodedStdoutLocalMonotonicEqualRate
                } else {
                    LiveCaptureAgeBasis::ProducerDeclared
                },
                encoded_source: options
                    .encoded_source
                    .as_ref()
                    .map(|_| LiveEncodedSourceReport {
                        state: LiveEncodedSourceState::Pending,
                        manifest: None,
                        input_bytes: 0,
                        latest_delivery: None,
                        completion: None,
                        error: None,
                    }),
                engine_epoch: engine_epoch.clone(),
                identity,
                clock,
                options: options.clone(),
                active: true,
                retirement: None,
                server_elapsed: Some(LiveClockTick(0)),
                admitted_pixel_bytes: crate::live_frames::LIVE_RGB_ADMISSION_BYTES,
                received_frames: 0,
                last_frame: None,
                inference: info,
                queue: None,
            };
            report.validate()?;
            let origin = if options
                .inference
                .as_ref()
                .is_some_and(|i| i.profile.provider == InferenceProvider::Cuda)
            {
                Some(
                    crate::live_cuda_capture_db::LiveCudaOrigin::new(
                        session,
                        &report,
                        &request.request_id,
                    )
                    .map_err(unavailable)?,
                )
            } else {
                None
            };
            // All admission work above is fallible without retiring the predecessor.
            // The receipt commits before either epoch is published or cancelled.
            if let Some(epoch) = predecessor {
                let prior = registry
                    .entries
                    .get(epoch.as_str())
                    .expect("validated predecessor");
                if prior.created.elapsed()
                    >= Duration::from_millis(u64::from(prior.report.options.lease_ms))
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "continuation predecessor expired during admission",
                    ));
                }
            }
            let body = ResponseBody::LiveCapture((report.clone()).into());
            if browser_media.as_ref().is_some_and(|pin| !pin.current()) {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "browser source media retired during capture admission",
                ));
            }
            if let Some(pin) = &browser_analysis_pin {
                let selected_timing = pin.select_read().map_err(browser_timing_read_error)?;
                #[cfg(test)]
                let read_test = CaptureReadGuard::take(
                    &registry,
                    &CaptureReadTestTarget::AdmissionFinal(request.request_id.clone()),
                );
                #[cfg(test)]
                read_test.enter(CaptureReadTestPhase::BeforeSample, None)?;
                let now = crate::handy_clock::boottime_ns()?;
                #[cfg(test)]
                read_test.enter(CaptureReadTestPhase::AfterSample, Some(now))?;
                let observed = selected_timing
                    .evaluate_at(now)
                    .map_err(browser_timing_read_error)?;
                #[cfg(test)]
                read_test.observed(&observed);
                if !observed.frame.is_current() {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "browser analysis player retired during capture admission",
                    ));
                }
            }
            let tx = db.transaction().map_err(internal)?;
            if manifest.is_some() || options.encoded_source.is_some() {
                self.arm_worker_fence(&tx)?;
                if options.inference.is_some() {
                    self.arm_staging_fence(&tx)?;
                }
            }
            remember(&tx, session.as_str(), original_request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            if let Some(epoch) = predecessor.filter(|_| !prepared) {
                retire(
                    registry
                        .entries
                        .get_mut(epoch.as_str())
                        .expect("validated predecessor"),
                    LiveCaptureRetirement::Superseded,
                )?;
            }
            let capture_epoch = report.identity.epoch.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let created = Instant::now();
            let lease_end = created + Duration::from_millis(u64::from(options.lease_ms));
            registry.entries.insert(
                report.identity.epoch.to_string(),
                Entry {
                    owner: session.clone(),
                    native_clock: clock::NativeCaptureClock::new(created),
                    native_accepted: None,
                    native_delivery: None,
                    native_input_close: None,
                    native_successor: None,
                    native_transfer: None,
                    native_transfer_joined: false,
                    created,
                    report,
                    runtime: Some(runtime),
                    cancel: cancel.clone(),
                    origin,
                    recording: None,
                    browser_media,
                    browser_analysis_pin,
                },
            );
            if let Some(manifest) = manifest {
                let mut intent = request.clone();
                if let Command::ContinueLiveCaptureAt {
                    engine_epoch,
                    predecessor,
                    ..
                } = &request.command
                {
                    intent.command = Command::ContinueLiveCapture {
                        engine_epoch: engine_epoch.clone(),
                        predecessor: predecessor.clone(),
                        options: options.clone(),
                    };
                }
                if let Err(error) = inference::spawn(
                    self,
                    capture_epoch.clone(),
                    manifest,
                    cancel,
                    created,
                    lease_end,
                    intent,
                ) {
                    inference::fail_entry(
                        registry
                            .entries
                            .get_mut(capture_epoch.as_str())
                            .expect("published"),
                        error,
                    )?;
                }
            }
            registry.sweep(&db)?;
            return Ok(ResponseBody::LiveCapture(
                (self::report(
                    registry
                        .entries
                        .get(capture_epoch.as_str())
                        .expect("published"),
                )?)
                .into(),
            ));
        }
        let epoch = match &request.command {
            Command::LiveCaptureStatus { epoch }
            | Command::StopLiveCapture { epoch }
            | Command::FinishLiveCapture { epoch }
            | Command::CloseBrowserAnalysisInput { epoch, .. }
            | Command::StartLiveRecording { epoch, .. }
            | Command::LiveRecordingPage { epoch, .. }
            | Command::DiscardLiveRecording { epoch, .. } => epoch,
            _ => unreachable!(),
        };
        let entry = registry.entries.get_mut(epoch.as_str()).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                "capture lease absent or retired with its engine boot",
            )
        })?;
        if entry.owner != *session || entry.report.identity.project != *project(request)? {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "capture lease belongs to another actor or project",
            ));
        }
        if request.expected_revision != Some(entry.report.identity.revision) {
            return Err(ProtocolError::new(
                ErrorCode::RevisionConflict,
                "capture scope differs from its admitted revision",
            ));
        }
        if let Command::CloseBrowserAnalysisInput { final_frame, .. } = &request.command {
            if replay(&db, session.as_str(), request, fingerprint)?.is_some() {
                return Ok(ResponseBody::LiveCapture(report(entry)?.into()));
            }
            if entry.report.input_close.is_some() {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "capture input already has its original close request",
                ));
            }
            if entry.native_successor.is_some() || entry.native_transfer.is_some() {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "transfer input cannot also acquire an EOF close",
                ));
            }
            let info = entry.report.inference.as_ref().ok_or_else(|| {
                ProtocolError::unsupported("input closure requires browser CPU inference")
            })?;
            if !entry.report.active
                || entry.cancel.load(Ordering::Acquire)
                || entry.report.options.browser_analysis.is_none()
                || entry.browser_analysis_pin.is_none()
                || entry
                    .report
                    .options
                    .inference
                    .as_ref()
                    .is_none_or(|inference| inference.profile.provider != InferenceProvider::Cpu)
                || !matches!(
                    info.state,
                    LiveCaptureInferenceState::Starting | LiveCaptureInferenceState::Running
                )
                || entry.report.options.encoded_source.is_some()
                || final_frame != &entry.report.last_frame
                || final_frame.is_none() != (entry.report.received_frames == 0)
            {
                return Err(ProtocolError::invalid(
                    "input close differs from active original browser CPU ingress",
                ));
            }
            entry
                .runtime
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("capture runtime absent"))?
                .check_input_close(final_frame.as_ref())?;
            tick(entry)?;
            let closed_at_boottime_ns = entry.native_clock.latest_native()?.earliest_ns;
            let continuity_generation = entry
                .runtime
                .as_ref()
                .expect("checked")
                .status()
                .pair_discontinuities;
            let input = LiveCaptureInputCloseReport {
                original_request_id: request.request_id.clone(),
                final_frame: final_frame.clone(),
                phase: LiveCaptureInputPhase::Closing,
                disposition: None,
                worker_joined: false,
                delivery_settled: entry
                    .native_delivery
                    .as_ref()
                    .is_none_or(|sink| sink.pending.load(Ordering::Acquire) == 0),
            };
            let mut proposed = report(entry)?;
            proposed.input_close = Some(input.clone());
            proposed.validate()?;
            let body = ResponseBody::LiveCapture(proposed.into());
            let tx = db.transaction().map_err(internal)?;
            remember(&tx, session.as_str(), original_request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            entry
                .runtime
                .as_mut()
                .expect("checked under same guard")
                .close_input(final_frame.as_ref())?;
            entry.native_input_close = Some(NativeInputClose {
                closed_at_boottime_ns,
                continuity_generation,
                disposition: None,
                end: None,
            });
            entry.report.input_close = Some(input);
            return Ok(body);
        }
        if matches!(
            request.command,
            Command::StartLiveRecording { .. }
                | Command::LiveRecordingPage { .. }
                | Command::DiscardLiveRecording { .. }
        ) {
            return recording::command(self, entry, request);
        }
        if matches!(request.command, Command::FinishLiveCapture { .. }) {
            if replay(&db, session.as_str(), request, fingerprint)?.is_some() {
                return Ok(ResponseBody::LiveCapture((report(entry)?).into()));
            }
            let info = entry
                .report
                .inference
                .as_ref()
                .ok_or_else(|| ProtocolError::unsupported("Finish requires CUDA inference"))?;
            if entry
                .report
                .options
                .inference
                .as_ref()
                .expect("admitted")
                .profile
                .provider
                != InferenceProvider::Cuda
            {
                return Err(ProtocolError::unsupported("Finish requires CUDA inference"));
            }
            if !entry.report.active
                && entry.report.retirement != Some(LiveCaptureRetirement::Completed)
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "retired capture cannot Finish",
                ));
            }
            if entry.report.active
                && (info.state == LiveCaptureInferenceState::Starting
                    || (info.completed == 0
                        && entry
                            .runtime
                            .as_ref()
                            .expect("active")
                            .status()
                            .queue
                            .in_flight_frames
                            == 0))
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Finish requires Ready completed or in-flight CUDA work",
                ));
            }
            let mut proposed = report(entry)?;
            if entry.report.active && !finishing(entry) {
                let info = proposed.inference.as_mut().expect("admitted");
                info.state = LiveCaptureInferenceState::FinishRequested;
                info.latest = None;
            }
            proposed.validate()?;
            let body = ResponseBody::LiveCapture((proposed).into());
            let tx = db.transaction().map_err(internal)?;
            remember(&tx, session.as_str(), original_request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            if entry.report.active && !finishing(entry) {
                let info = entry.report.inference.as_mut().expect("admitted");
                info.state = LiveCaptureInferenceState::FinishRequested;
                info.latest = None;
                entry.native_accepted = None;
                entry.retire_native_delivery();
            }
            registry.sweep(&db)?;
            return Ok(ResponseBody::LiveCapture(
                (report(registry.entries.get(epoch.as_str()).expect("retained"))?).into(),
            ));
        }
        if matches!(request.command, Command::StopLiveCapture { .. }) {
            if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                return Ok(body);
            }
            if entry.runtime.is_some() {
                retire(entry, LiveCaptureRetirement::Stopped)?;
            }
            let body = ResponseBody::LiveCapture((report(entry)?).into());
            let tx = db.transaction().map_err(internal)?;
            remember(&tx, session.as_str(), original_request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            return Ok(body);
        }
        Ok(ResponseBody::LiveCapture((report(entry)?).into()))
    }
}

fn live_entry<'a>(
    registry: &'a mut Registry,
    handshake: &LiveBulkHandshake,
) -> PResult<&'a mut Entry> {
    let entry = registry
        .entries
        .get_mut(handshake.identity.epoch.as_str())
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                "live capture absent from this engine boot",
            )
        })?;
    if entry.owner != handshake.session
        || entry.report.identity.project != handshake.identity.project
    {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "live capture belongs to another actor or project",
        ));
    }
    if entry.report.identity != handshake.identity
        || entry.report.engine_epoch != handshake.engine_epoch
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "live capture scope differs",
        ));
    }
    if entry.runtime.is_none() || finishing(entry) {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "live capture retired",
        ));
    }
    Ok(entry)
}
fn check_frame(entry: &Entry, header: &LiveFrameHeader) -> PResult<bool> {
    header.validate()?;
    if let Some(binding) = &entry.report.options.browser_media {
        if entry
            .browser_media
            .as_ref()
            .is_none_or(|pin| !pin.current())
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "browser source media is no longer current",
            ));
        }
        let mapped = binding
            .clock
            .project_at(header.source_time, &binding.selection.placement.spec)
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        let expected = entry
            .report
            .clock
            .project_at_source(header.source_time)
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        if mapped != expected
            || ExactSeconds::from_source(header.source_time)
                .checked_cmp(ExactSeconds::from_source(
                    entry.report.options.source_anchor,
                ))
                .map_err(|error| ProtocolError::invalid(error.to_string()))?
                == std::cmp::Ordering::Less
        {
            return Err(ProtocolError::invalid(
                "browser frame differs from declared source placement clock",
            ));
        }
    }
    if entry.report.continuation.is_some()
        && ExactSeconds::from_source(header.source_time)
            .checked_cmp(ExactSeconds::from_source(
                entry.report.options.source_anchor,
            ))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            == std::cmp::Ordering::Less
    {
        return Err(ProtocolError::invalid(
            "continuation frame precedes its source anchor",
        ));
    }
    if let Some(last) = &entry.report.last_frame {
        if header.frame == last.frame {
            if header != last {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "live frame identity reused with different metadata",
                ));
            }
            return Ok(true);
        }
        if header.frame < last.frame {
            return Err(ProtocolError::invalid(
                "only the latest live frame can be replayed",
            ));
        }
        if header.capture_tick < last.capture_tick
            || ExactSeconds::from_source(header.source_time)
                .checked_cmp(ExactSeconds::from_source(last.source_time))
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                != std::cmp::Ordering::Greater
            || entry
                .report
                .clock
                .map(header.source_time)
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                .project
                .time
                <= entry
                    .report
                    .clock
                    .map(last.source_time)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?
                    .project
                    .time
        {
            return Err(ProtocolError::invalid(
                "live frame source, capture or mapped project clock regressed",
            ));
        }
    }
    if header.capture_tick > tick(entry)? {
        return Err(ProtocolError::invalid(
            "capture tick is ahead of the engine lease clock",
        ));
    }
    entry
        .report
        .clock
        .map(header.source_time)
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
    Ok(false)
}
impl Engine {
    fn authorize_live_bulk(&self, db: &Connection, handshake: &LiveBulkHandshake) -> PResult<()> {
        handshake.validate()?;
        self.authenticate(db, &handshake.session, Some(&handshake.auth_token))?;
        if handshake.engine_epoch.as_str() != self.engine_epoch {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live capture engine boot retired",
            ));
        }
        for scope in [Scope::Read, Scope::ImportSource, Scope::Generate] {
            require_grant(db, &handshake.session, &handshake.identity.project, scope)?;
        }
        Ok(())
    }
    /// Fresh authentication precedes all frame bodies; the effect repeats this
    /// check after I/O, under the same database/registry lock order as retirement.
    pub(crate) fn live_bulk_ready(
        &self,
        handshake: &LiveBulkHandshake,
        header: Option<&LiveFrameHeader>,
    ) -> PResult<LiveCaptureReport> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        self.authorize_live_bulk(&db, handshake)?;
        let entry = live_entry(&mut registry, handshake)?;
        successor::check_ingress_state(entry)?;
        if entry.native_input_close.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "original browser analysis input is closed",
            ));
        }
        if handshake.channel != LiveBulkChannel::LiveCapture
            || entry.report.options.encoded_source.is_some()
        {
            return Err(ProtocolError::invalid(
                "RGB ingress requires a producer-declared RGB capture",
            ));
        }
        if let Some(header) = header {
            check_frame(entry, header)?;
        }
        report(entry)
    }
    pub(crate) fn live_bulk_upload(
        &self,
        handshake: &LiveBulkHandshake,
        header: &LiveFrameHeader,
        bytes: &[u8],
    ) -> PResult<(LiveCaptureReport, bool)> {
        if bytes.len() != LIVE_FRAME_BYTES
            || format!("{:x}", Sha256::digest(bytes)) != header.sha256
        {
            return Err(ProtocolError::invalid(
                "live RGB byte count or hash differs",
            ));
        }
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        self.authorize_live_bulk(&db, handshake)?;
        let entry = live_entry(&mut registry, handshake)?;
        successor::check_ingress(entry, header)?;
        if entry.native_input_close.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "original browser analysis input is closed",
            ));
        }
        if handshake.channel != LiveBulkChannel::LiveCapture
            || entry.report.options.encoded_source.is_some()
        {
            return Err(ProtocolError::invalid(
                "RGB ingress requires a producer-declared RGB capture",
            ));
        }
        let replayed = check_frame(entry, header)?;
        if !replayed {
            let received = entry
                .report
                .received_frames
                .checked_add(1)
                .ok_or_else(|| ProtocolError::invalid("live frame count overflow"))?;
            let presentation = entry
                .report
                .clock
                .map(header.source_time)
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                .project
                .time;
            let now = tick(entry)?;
            let continuity = entry
                .runtime
                .as_ref()
                .expect("active entry")
                .status()
                .pair_discontinuities;
            entry.runtime.as_mut().expect("active entry").offer(
                header.frame,
                header.source_time,
                header.capture_tick,
                presentation,
                bytes,
                now,
            )?;
            if continuity
                != entry
                    .runtime
                    .as_ref()
                    .expect("active")
                    .status()
                    .pair_discontinuities
            {
                entry.native_accepted = None;
                entry.retire_native_delivery();
            }
            entry.report.received_frames = received;
            entry.report.last_frame = Some(header.clone());
        }
        Ok((report(entry)?, replayed))
    }
}
