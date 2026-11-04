//! One source-owned CPU/browser HSP stream. History is descriptive; only the
//! retained preparation, source subscription and visible timing pin authorize it.
use super::*;
use super::{
    browser_timing::{BrowserTimingClaimFailure, BrowserTimingPin},
    handy_hsp_preparation::{HspContinuityClockOwner, HspPreparedSourceContext, HspSourceHandoff},
    handy_hsp_source_qualification::{self as source_qualification, AcceptedHspSource},
    live_captures::{
        NativeAcceptedCapture, NativeCaptureEnd, NativeCaptureSelection, NativeCaptureSink,
        NativeCaptureSourceSnapshot, NativeCaptureSubscription, NativeCaptureSuccessor,
        NativeCaptureTransfer, NativeCaptureTransferFailure,
    },
};
use crate::{
    handy_hsp_dispatch::{
        HspClaimGate, HspClaimedHandoff, HspDispatchFailure, HspNativeOwner, HspPreclaimProgress,
    },
    handy_hsp_execution::{
        HspAuthorityEvaluation, HspAuthoritySnapshot, HspNativeAuthority, HspPlayIntent,
        HspSessionHandle, HspWeakSessionHandle,
    },
    handy_hsp_runtime::{HspAuthorityWindow, HspBinding, HspDispatchFacts, HspPlayReference},
    handy_playback_clock::{HspClockPin, NativePlaybackClock},
};
use arc_swap::ArcSwapOption;
use pulsar_core::{HspSourceCursor, HspSourceTransition, HspSourceTransitionSpec};
#[path = "handy_hsp_live_continuity.rs"]
mod continuity;
use continuity::{ContinuityState, EpochSink, SourceBundle};
#[cfg(test)]
#[path = "handy_hsp_producer_read_test_hooks.rs"]
mod producer_read_test_hooks;
#[cfg(test)]
pub(in crate::authority) use producer_read_test_hooks::HspProducerReadTestPhase;
#[cfg(test)]
use producer_read_test_hooks::{ProducerReadGuard, ProducerReadPause};
use std::{
    collections::BTreeMap,
    sync::{atomic::AtomicU64, OnceLock, Weak},
    time::Instant,
};

const MAX_OWNERS: usize = 64;
const MAX_STAGED: usize = 2;
const NS_PER_MS: i128 = 1_000_000;

fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "original live HSP source is unavailable",
    )
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "live HSP source binding differs",
    )
}
/// A borrowed view of an already native-admitted CPU reply. Its original enum
/// remains in NativeAcceptedCapture; this view creates no worker/source authority.
struct SelectedCpuEstimate<'a> {
    attempt_id: &'a AttemptId,
    identity: &'a LiveIdentity,
    profile_sha256: &'a String,
    input: &'a LiveInferenceInput,
    position: &'a NormalizedPosition,
}
fn selected_cpu_estimate<'a>(
    reply: &'a LiveInferenceReply,
    expected_manifest_sha256: Option<&str>,
) -> PResult<SelectedCpuEstimate<'a>> {
    let (attempt_id, identity, profile_sha256, input, position) = match reply {
        LiveInferenceReply::Estimated {
            attempt_id,
            identity,
            profile_sha256,
            input,
            position,
            ..
        } if expected_manifest_sha256.is_none() => {
            (attempt_id, identity, profile_sha256, input, position)
        }
        LiveInferenceReply::CapturedCpuEstimated {
            attempt_id,
            identity,
            profile_sha256,
            manifest_sha256,
            input,
            position,
            ..
        } if expected_manifest_sha256 == Some(manifest_sha256.as_str()) => {
            (attempt_id, identity, profile_sha256, input, position)
        }
        _ => return Err(mismatch()),
    };
    Ok(SelectedCpuEstimate {
        attempt_id,
        identity,
        profile_sha256,
        input,
        position,
    })
}
fn dispatch(_: ProtocolError) -> HspDispatchFailure {
    HspDispatchFailure::BindingMismatch
}
fn now() -> PResult<u64> {
    crate::handy_clock::boottime_ns()
}

#[derive(Default)]
pub(super) struct Registry {
    entries: BTreeMap<(SessionId, RequestId), Arc<Controller>>,
    next_generation: u64,
}
struct Delivery {
    staged: Vec<NativeAcceptedCapture>,
    envelope: HspEnvelopeState,
    last: Option<NativeAcceptedCapture>,
    last_sample: Option<HspSample>,
    pending_end: Option<NativeCaptureEnd>,
    ended: Option<(RequestId, u64)>,
}
#[derive(Default)]
struct Controls {
    play: Option<RequestId>,
    stop: Option<RequestId>,
    error: Option<ProtocolError>,
}
#[cfg(test)]
#[derive(Default)]
struct PreclaimTestCounts {
    capture: AtomicU64,
    timing: AtomicU64,
}
/// Read-only counters of actual source preclaim contention. This probe owns no
/// stream, capture, observation or authority and cannot alter retry outcomes.
#[cfg(test)]
pub(in crate::authority) struct HspLivePreclaimProbe(Arc<PreclaimTestCounts>);
#[cfg(test)]
impl HspLivePreclaimProbe {
    pub fn capture_contentions(&self) -> u64 {
        self.0.capture.load(Ordering::Acquire)
    }
    pub fn timing_contentions(&self) -> u64 {
        self.0.timing.load(Ordering::Acquire)
    }
}
/// The capture owns this sink and the session owns its authority. Both back
/// references are weak: no Engine or registry/session ownership cycle exists.
struct Controller {
    binding: HspBinding,
    initial_source: HspSourceCursor,
    active: ArcSwapOption<SourceBundle>,
    engine: Weak<Engine>,
    original_request: Request,
    preparation_request: RequestId,
    captures: Weak<Mutex<live_captures::Registry>>,
    handle: OnceLock<HspWeakSessionHandle>,
    pool: ResourcePool,
    clocks: Arc<NativePlaybackClock>,
    zero: ExactSeconds,
    clock_until_ns: u64,
    clock_owner: OnceLock<Arc<HspContinuityClockOwner>>,
    continuity_enable: OnceLock<RequestId>,
    continuity: Mutex<ContinuityState>,
    source_custody: ArcSwapOption<NativeCaptureSuccessor>,
    staged_source: ArcSwapOption<continuity::StagedSuccessor>,
    retained_transfer: ArcSwapOption<NativeCaptureTransfer>,
    pending_source_work: AtomicBool,
    advancing_source: AtomicBool,
    policy: HspPolicy,
    finite_drain: Option<handy_qualification::QualifiedHspFiniteDrainPolicy>,
    shutdown: Arc<AtomicBool>,
    retired: AtomicBool,
    retirement_complete: AtomicBool,
    delivered: AtomicU64,
    delivery: Mutex<Delivery>,
    controls: Mutex<Controls>,
    #[cfg(test)]
    preclaim_test_counts: Arc<PreclaimTestCounts>,
    #[cfg(test)]
    producer_read_test: Mutex<Option<Arc<ProducerReadPause>>>,
}
impl Controller {
    fn active_source(&self) -> PResult<Arc<SourceBundle>> {
        self.active.load_full().ok_or_else(unavailable)
    }
    fn current(&self, source: &SourceBundle, at: u64) -> bool {
        !self.retired.load(Ordering::Acquire)
            && !self.shutdown.load(Ordering::Acquire)
            && source.current(at)
            && (self.clock_owner.get().is_some() || at < self.clock_until_ns)
    }
    fn clock_pin(&self, at: u64) -> PResult<Option<HspClockPin>> {
        self.clock_owner
            .get()
            .map(|owner| owner.snapshot(at))
            .transpose()
    }
    fn retire(&self) {
        if self.retired.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(clock) = self.clock_owner.get() {
            clock.request_stop();
        }
        if let Some(successor) = self.source_custody.load_full() {
            successor.cancel();
            // Promotion may concurrently remove its now-consumed reservation.
            // Republish only an actual cancellation's exact join custody.
            if successor.cancellation_requested() {
                self.source_custody.store(Some(successor));
            }
        }
        if let Some(handle) = self.handle.get() {
            handle.cancel();
        }
        if let Some(source) = self.active.load_full() {
            source.subscription.retire();
        }
        self.retirement_complete.store(true, Ordering::Release);
    }
    fn fail(&self, error: ProtocolError) -> ProtocolError {
        self.retire();
        if let Ok(mut controls) = self.controls.try_lock() {
            if controls.error.is_none() {
                controls.error = Some(error.clone());
            }
        }
        error
    }
    fn source_project(
        source: &SourceBundle,
        accepted: &NativeAcceptedCapture,
    ) -> PResult<ExactSeconds> {
        source
            .source
            .subject()
            .clock
            .project_at(
                accepted.frame.source_time,
                &source.source.subject().selection.placement.spec,
            )
            .map_err(internal)
    }
    fn facts_from(
        &self,
        source: &SourceBundle,
        at: u64,
        observation: &BrowserTimingObservation,
        pin: Option<&HspClockPin>,
    ) -> PResult<HspDispatchFacts> {
        if !self.current(source, at)
            || at < observation.bracket.native_receive_ns
            || at >= observation.expires_at_boottime_ns
        {
            return Err(unavailable());
        }
        let clock = match pin {
            Some(pin) => pin.at(at).map_err(internal)?,
            None => self.clocks.at(at)?,
        };
        let clock_until = pin.map_or(self.clock_until_ns, HspClockPin::valid_until_ns);
        let subject = source.source.subject();
        let alignment = subject
            .timing
            .phase_request(
                observation,
                at,
                500_000,
                clock.uncertainty_ns,
                source.until_ns.min(clock_until).min(clock.valid_until_ns),
            )?
            .calculate()
            .map_err(internal)?;
        // Visible playback independently measures phase. Subtract the immutable
        // first accepted source coordinate, retaining the rational-origin and
        // integer-wire uncertainty; completion/receipt clocks never set phase.
        let zero = self.zero.quantize_nanoseconds().map_err(internal)?;
        let base = i128::from(zero.time.as_nanos());
        let extra = i128::from(clock.uncertainty_ns)
            .checked_add(i128::from(subject.timing.physical_execution_error_ns))
            .and_then(|n| n.checked_add(500_001))
            .ok_or_else(mismatch)?;
        let low = i128::from(alignment.phase_earliest_ns)
            .checked_sub(base)
            .and_then(|n| n.checked_sub(extra))
            .ok_or_else(mismatch)?;
        let high = i128::from(alignment.phase_latest_ns)
            .checked_sub(base)
            .and_then(|n| n.checked_add(extra))
            .ok_or_else(mismatch)?;
        let earliest = low.div_euclid(NS_PER_MS);
        let latest = high
            .div_euclid(NS_PER_MS)
            .checked_add(i128::from(high.rem_euclid(NS_PER_MS) != 0))
            .ok_or_else(mismatch)?;
        let center_ns = low.checked_add(high).ok_or_else(mismatch)?.div_euclid(2);
        let center = center_ns
            .checked_add(NS_PER_MS / 2)
            .ok_or_else(mismatch)?
            .div_euclid(NS_PER_MS);
        let reference = HspPlayReference::new(
            at,
            HspPhaseWindow {
                earliest_ms: i64::try_from(earliest).map_err(internal)?,
                latest_ms: i64::try_from(latest).map_err(internal)?,
            },
            i32::try_from(center).map_err(internal)?,
            clock.estimated_server_ms,
        )
        .map_err(|_| mismatch())?;
        let until = alignment
            .valid_until_boottime_ns
            .min(clock.valid_until_ns)
            .min(clock_until);
        let facts = HspDispatchFacts::new(
            self.binding.clone(),
            reference,
            HspAuthorityWindow::new(source.until_ns, source.until_ns, until),
        )
        .map_err(|_| mismatch())?;
        match pin {
            Some(pin) => facts.with_clock_pin(pin.clone()).map_err(|_| mismatch()),
            None => Ok(facts),
        }
    }
    fn facts(&self, at: u64) -> PResult<HspDispatchFacts> {
        let source = self.active_source()?;
        let pin = self.clock_pin(at)?;
        let observation = source.timing.try_snapshot(at)?;
        self.facts_from(&source, at, &observation, pin.as_ref())
    }
    fn snapshot_from(
        &self,
        source: &SourceBundle,
        at: u64,
        observation: &BrowserTimingObservation,
        pin: Option<&HspClockPin>,
        project_tail: bool,
    ) -> PResult<HspAuthoritySnapshot> {
        let facts = self.facts_from(source, at, observation, pin)?;
        let projected = if project_tail && source.input_closed.load(Ordering::Acquire) {
            self.finite_drain
                .as_ref()
                .map(|policy| {
                    if !source
                        .media
                        .covers_duration(std::time::Duration::from_nanos(
                            policy.policy.stop_budget_ns,
                        ))
                    {
                        return Err(unavailable());
                    }
                    let stop_at = at
                        .checked_add(policy.policy.stop_budget_ns)
                        .ok_or_else(mismatch)?;
                    // Prediction retains these same selected observations and
                    // their original bounds; it does not sample another clock.
                    self.facts_from(source, stop_at, observation, pin)
                })
                .transpose()?
        } else {
            None
        };
        Ok(HspAuthoritySnapshot {
            facts: Some(facts),
            projected,
        })
    }
    fn observe(
        &self,
        clock: &mut dyn FnMut() -> std::result::Result<u64, HspDispatchFailure>,
        project_tail: bool,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        let selected = (|| {
            let source = self.active_source().map_err(dispatch)?;
            let pin = self.clock_owner.get().map(|owner| owner.select_read());
            let timing = source
                .timing
                .select_read()
                .map_err(|_| HspDispatchFailure::BindingMismatch)?;
            Ok::<_, HspDispatchFailure>((source, pin, timing))
        })();
        // Even a failed selection receives one fresh evaluation tick. Callers
        // settling Stop/Unknown may retain that tick with no authority facts;
        // neither a failed selection nor a callback failure is retried here.
        let sampled_at_ns = clock()?;
        let (source, selected_pin, timing) = selected?;
        let pin = selected_pin
            .map(|selected| selected.evaluate_at(sampled_at_ns))
            .transpose()
            .map_err(dispatch)?;
        let observation = timing
            .evaluate_at(sampled_at_ns)
            .map_err(|_| HspDispatchFailure::BindingMismatch)?;
        let snapshot = self
            .snapshot_from(
                &source,
                sampled_at_ns,
                &observation,
                pin.as_ref(),
                project_tail,
            )
            .map_err(dispatch)?;
        Ok(HspAuthorityEvaluation {
            sampled_at_ns,
            snapshot,
        })
    }
    fn deliver_locked(
        &self,
        state: &mut Delivery,
        source: &SourceBundle,
        accepted: NativeAcceptedCapture,
        handle: &HspSessionHandle,
    ) -> PResult<()> {
        #[cfg(test)]
        let probe = ProducerReadGuard::take(&self.producer_read_test);
        // The caller retains this exact source epoch under delivery custody.
        // Select its immutable observations before the one precheck clock;
        // publication replacement cannot change this evaluation's inputs.
        let selected_pin = self.clock_owner.get().map(|owner| owner.select_read());
        let selected_timing = source.timing.select_read();
        #[cfg(test)]
        probe.enter(HspProducerReadTestPhase::BeforeSample, None)?;
        let at = now()?;
        #[cfg(test)]
        probe.enter(HspProducerReadTestPhase::AfterSample, Some(at))?;
        if !self.current(source, at)
            || accepted.frame.identity != source.selection.identity
            || accepted.continuity_generation != source.continuity_generation
            || accepted.accepted_boottime.latest_ns > at
        {
            return Err(mismatch());
        }
        let SelectedCpuEstimate {
            attempt_id,
            identity,
            profile_sha256,
            input,
            position,
        } = selected_cpu_estimate(
            &accepted.estimate.result,
            source.selection.cpu_manifest_sha256.as_deref(),
        )?;
        if attempt_id != &source.selection.attempt_id
            || identity != &source.selection.identity
            || profile_sha256 != &source.selection.profile_sha256
            || !accepted.estimate.fresh
            || accepted.estimate.qualified
        {
            return Err(mismatch());
        }
        if let Some(previous) = &state.last {
            let last = selected_cpu_estimate(
                &previous.estimate.result,
                source.selection.cpu_manifest_sha256.as_deref(),
            )?
            .input;
            if !accepted.continuous_from_previous
                || accepted.previous_frame != previous.frame
                || input.sequence != last.sequence.checked_add(1).ok_or_else(mismatch)?
            {
                return Err(mismatch());
            }
        }
        let relative = Self::source_project(source, &accepted)?
            .checked_sub(self.zero)
            .map_err(internal)?;
        let selected = quantize_hsp_point(
            relative.quantize_nanoseconds().map_err(internal)?.time,
            *position,
        )
        .map_err(internal)?;
        let mut envelope = state.envelope.clone();
        envelope
            .accept(selected.point, accepted.continuous_from_previous)
            .map_err(internal)?;
        let pin = selected_pin
            .map(|selected| selected.evaluate_at(at))
            .transpose()?;
        let observation = selected_timing
            .and_then(|selected| selected.evaluate_at(at))
            .map_err(|failure| match failure {
                BrowserTimingClaimFailure::Contended => unavailable(),
                BrowserTimingClaimFailure::Refused(error) => error,
            })?;
        let facts = self.facts_from(source, at, &observation, pin.as_ref())?;
        #[cfg(test)]
        probe.observed(&observation, pin.as_ref());
        let phase = facts.reference().phase();
        let min_lead = i128::from(selected.point.time_ms) - i128::from(phase.latest_ms);
        let max_lead = i128::from(selected.point.time_ms) - i128::from(phase.earliest_ms);
        let lookahead = source.source.subject().lookahead;
        if min_lead.checked_mul(1000).ok_or_else(mismatch)? < i128::from(lookahead.minimum_lead_us)
            || max_lead.checked_mul(1000).ok_or_else(mismatch)?
                > i128::from(lookahead.maximum_lead_us)
        {
            return Err(mismatch());
        }
        // Capture is an HTTP receipt bound. Include the entire independently
        // accepted acquisition→receipt allowance; never rename receipt exposure.
        let captured = accepted
            .capture_boottime
            .earliest_ns
            .checked_sub(source.source.subject().maximum_capture_to_engine_receipt_ns)
            .ok_or_else(mismatch)?;
        if at
            .checked_sub(captured)
            .is_none_or(|age| age > self.policy.max_sample_age_ns)
        {
            return Err(unavailable());
        }
        let sample = HspSample {
            point: selected.point,
            source_sequence: input.sequence,
            captured_at_ns: captured,
            accepted_at_ns: accepted.accepted_boottime.latest_ns,
            continuous_from_previous: accepted.continuous_from_previous,
        };
        #[cfg(test)]
        probe.sample(sample);
        if handle
            .offer(&source.selection.identity, sample)
            .map_err(|_| unavailable())?
            != HspOfferOutcome::Accepted
        {
            return Err(unavailable());
        }
        state.envelope = envelope;
        state.last_sample = Some(sample);
        state.last = Some(accepted);
        self.delivered.fetch_add(1, Ordering::AcqRel);
        #[cfg(test)]
        probe.delivered();
        Ok(())
    }
    fn finish_locked(
        &self,
        delivery: &mut Delivery,
        source: &SourceBundle,
        end: NativeCaptureEnd,
        handle: &HspSessionHandle,
    ) -> PResult<()> {
        let Some(policy) = &self.finite_drain else {
            self.retire();
            return Ok(());
        };
        if end.selection() != &source.selection
            || end.continuity_generation() != source.continuity_generation
        {
            return Err(mismatch());
        }
        if end.disposition() != LiveCaptureInputFinalDisposition::Accepted {
            self.retire();
            return Ok(());
        }
        let final_accepted = end.final_accepted().ok_or_else(mismatch)?;
        let last = delivery.last.as_ref().ok_or_else(mismatch)?;
        let header = end.final_frame().ok_or_else(mismatch)?;
        let input = selected_cpu_estimate(
            &last.estimate.result,
            source.selection.cpu_manifest_sha256.as_deref(),
        )?
        .input;
        if header.frame != last.frame.frame
            || header.source_time != last.frame.source_time
            || header.capture_tick != last.frame.capture_tick
            || header.sha256 != input.current.sha256
            || final_accepted.frame != last.frame
            || final_accepted.previous_frame != last.previous_frame
            || final_accepted.capture_boottime != last.capture_boottime
            || final_accepted.accepted_boottime != last.accepted_boottime
            || serde_json::to_vec(&final_accepted.estimate.result).map_err(internal)?
                != serde_json::to_vec(&last.estimate.result).map_err(internal)?
        {
            return Err(mismatch());
        }
        let terminal = delivery.last_sample.ok_or_else(mismatch)?;
        if let Some((request, closed_at)) = &delivery.ended {
            return if request == end.close_request_id() && *closed_at == end.closed_at_boottime_ns()
            {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        handle
            .seal_input(
                &source.selection.identity,
                terminal,
                end.closed_at_boottime_ns(),
                policy.policy,
                policy.maximum_stop_dispatch_delay_ns,
            )
            .map_err(|_| unavailable())?;
        delivery.ended = Some((end.close_request_id().clone(), end.closed_at_boottime_ns()));
        Ok(())
    }
    fn capture_play(&self, original: &RequestId) -> PResult<HspPlayIntent> {
        let mut controls = self
            .controls
            .try_lock()
            .map_err(|_| self.fail(unavailable()))?;
        if controls.play.is_some() || self.retired.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        controls.play = Some(original.clone());
        drop(controls);
        let handle = self
            .handle
            .get()
            .and_then(HspWeakSessionHandle::upgrade)
            .ok_or_else(|| self.fail(unavailable()))?;
        handle
            .capture_play_intent()
            .map_err(|_| self.fail(unavailable()))
    }
    fn play(&self, intent: HspPlayIntent) -> PResult<()> {
        let handle = self
            .handle
            .get()
            .and_then(HspWeakSessionHandle::upgrade)
            .ok_or_else(|| self.fail(unavailable()))?;
        handle
            .request_original_play(intent)
            .map_err(|_| self.fail(unavailable()))
    }
    fn stop(&self, original: &RequestId) {
        self.retire();
        if let Ok(mut controls) = self.controls.try_lock() {
            if controls.stop.is_none() {
                controls.stop = Some(original.clone());
            }
        }
    }
    fn status(&self) -> PResult<HandyHspLiveNativeStatus> {
        let controls = self.controls.lock().map_err(internal)?;
        Ok(HandyHspLiveNativeStatus {
            retired: self.retired.load(Ordering::Acquire)
                || self
                    .active
                    .load_full()
                    .is_none_or(|source| source.sink.is_retired())
                || self
                    .handle
                    .get()
                    .is_some_and(HspWeakSessionHandle::is_retired),
            original_play_request_id: controls.play.clone(),
            original_stop_request_id: controls.stop.clone(),
            delivered_samples: self.delivered.load(Ordering::Acquire),
            active_source: self.active_source_status()?,
            error: controls.error.clone(),
        })
    }
}
impl HspNativeAuthority for Controller {
    fn initial_source_cursor(&self) -> Option<HspSourceCursor> {
        Some(self.initial_source.clone())
    }
    fn has_pending_source_transfer(&self) -> bool {
        self.pending_source_work.load(Ordering::Acquire)
    }
    fn advance_source_with_db(
        &self,
        db: &Connection,
    ) -> std::result::Result<(), HspDispatchFailure> {
        self.progress_source(db)
            .map_err(|error| dispatch(self.fail(error)))
    }
    fn request_owned_stop(&self) {
        self.retire();
    }
    fn owned_work_joined(&self) -> bool {
        self.successor_work_joined()
    }
    fn join_owned_work(&self) -> std::result::Result<(), HspDispatchFailure> {
        self.join_successor_work().map_err(dispatch)
    }
    fn current_snapshot(
        &self,
        at: u64,
    ) -> std::result::Result<HspAuthoritySnapshot, HspDispatchFailure> {
        let source = self.active_source().map_err(dispatch)?;
        let pin = self.clock_pin(at).map_err(dispatch)?;
        let observation = source.timing.try_snapshot(at).map_err(dispatch)?;
        self.snapshot_from(&source, at, &observation, pin.as_ref(), true)
            .map_err(dispatch)
    }
    fn observe_current(
        &self,
        clock: &mut dyn FnMut() -> std::result::Result<u64, HspDispatchFailure>,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        self.observe(clock, false)
    }
    fn observe_snapshot(
        &self,
        clock: &mut dyn FnMut() -> std::result::Result<u64, HspDispatchFailure>,
    ) -> std::result::Result<HspAuthorityEvaluation, HspDispatchFailure> {
        self.observe(clock, true)
    }
    fn admission_failed(&self) {
        self.retire();
    }
    fn attach(&self, handle: &HspSessionHandle) -> std::result::Result<(), HspDispatchFailure> {
        let attached = (|| {
            if self.retired.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            self.handle
                .set(handle.downgrade())
                .map_err(|_| mismatch())?;
            let mut delivery = self.delivery.try_lock().map_err(|_| unavailable())?;
            let source = self.active_source()?;
            let staged = std::mem::take(&mut delivery.staged);
            for original in staged {
                self.deliver_locked(&mut delivery, &source, original, handle)?;
            }
            if let Some(end) = delivery.pending_end.take() {
                self.finish_locked(&mut delivery, &source, end, handle)?;
            }
            Ok(())
        })();
        attached.map_err(|error| dispatch(self.fail(error)))
    }
    fn current_facts(
        &self,
        at: u64,
    ) -> std::result::Result<Option<HspDispatchFacts>, HspDispatchFailure> {
        self.facts(at).map(Some).map_err(dispatch)
    }
    fn authorize(
        &self,
        _: Instant,
        _: &AtomicBool,
        _: HspClaimGate<'_>,
        _: &HspNativeOwner,
    ) -> std::result::Result<HspClaimedHandoff, HspDispatchFailure> {
        // Production must compose beneath preparation's retained actor/DB guard.
        Err(HspDispatchFailure::BindingMismatch)
    }
    fn authorize_with_db(
        &self,
        db: &Connection,
        deadline: Instant,
        cancel: &AtomicBool,
        gate: HspClaimGate<'_>,
        owner: &HspNativeOwner,
    ) -> std::result::Result<HspClaimedHandoff, HspDispatchFailure> {
        match self.try_authorize_with_db(db, deadline, cancel, gate, owner)? {
            HspPreclaimProgress::Claimed(claimed) => Ok(claimed),
            HspPreclaimProgress::Contended(_) => Err(HspDispatchFailure::Busy),
        }
    }
    fn try_authorize_with_db<'a>(
        &self,
        db: &Connection,
        deadline: Instant,
        cancel: &AtomicBool,
        mut gate: HspClaimGate<'a>,
        owner: &HspNativeOwner,
    ) -> std::result::Result<HspPreclaimProgress<'a>, HspDispatchFailure> {
        gate.check_preclaim_wait(owner)?;
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(HspDispatchFailure::Cancelled);
        }
        let source = self.active_source().map_err(dispatch)?;
        let pin = self.clock_pin(now().map_err(dispatch)?).map_err(dispatch)?;
        source_qualification::revalidate_accepted_hsp_source(db, &self.pool, &source.source)
            .map_err(dispatch)?;
        let captures = self
            .captures
            .upgrade()
            .ok_or(HspDispatchFailure::BindingMismatch)?;
        let mut registry = match captures.try_lock() {
            Ok(registry) => registry,
            Err(std::sync::TryLockError::WouldBlock) => {
                #[cfg(test)]
                self.preclaim_test_counts
                    .capture
                    .fetch_add(1, Ordering::AcqRel);
                return Ok(HspPreclaimProgress::Contended(gate));
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(HspDispatchFailure::BindingMismatch)
            }
        };
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(HspDispatchFailure::Cancelled);
        }
        let timing = match source.timing.try_claim_native(now) {
            Ok(timing) => timing,
            Err(BrowserTimingClaimFailure::Contended) => {
                #[cfg(test)]
                self.preclaim_test_counts
                    .timing
                    .fetch_add(1, Ordering::AcqRel);
                drop(registry);
                return Ok(HspPreclaimProgress::Contended(gate));
            }
            Err(BrowserTimingClaimFailure::Refused(error)) => return Err(dispatch(error)),
        };
        gate.check_preclaim_wait(owner)?;
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(HspDispatchFailure::Cancelled);
        }
        registry.sweep(db).map_err(dispatch)?;
        let subscription = &source.subscription;
        let current = registry
            .native_source_snapshot_claimed(subscription, &timing)
            .map_err(dispatch)?;
        if current.selection != source.selection
            || current.continuity_generation != source.continuity_generation
            || current.lease_deadline_boottime_ns != source.until_ns
        {
            return Err(HspDispatchFailure::BindingMismatch);
        }
        self.facts_from(
            &source,
            timing.admitted_at_ns,
            timing
                .observation_for(&source.timing, timing.admitted_at_ns)
                .map_err(dispatch)?,
            pin.as_ref(),
        )
        .map_err(dispatch)?;
        // All native source/presentation guards remain held through both fact
        // evaluations around the durable claim. The closure never re-locks them.
        gate.try_apply(owner, |at| {
            self.facts_from(
                &source,
                at,
                timing
                    .observation_for(&source.timing, at)
                    .map_err(dispatch)?,
                pin.as_ref(),
            )
            .map(Some)
            .map_err(dispatch)
        })
    }
}

pub(super) fn is_command(command: &Command) -> bool {
    continuity::is_command(command)
        || matches!(
            command,
            Command::BindHandyHspLive { .. }
                | Command::PlayHandyHspLive { .. }
                | Command::StopHandyHspLive { .. }
                | Command::HandyHspLiveStatus { .. }
        )
}
fn original(
    db: &Connection,
    actor: &SessionId,
    project: &ProjectId,
    id: &RequestId,
) -> PResult<HandyHspLiveStatus> {
    let raw: Option<String> = db.query_row(
        "SELECT CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
        params![actor.as_str(), id.as_str()], |row| row.get(0)).optional().map_err(internal)?;
    let raw = raw.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "original live HSP request not found")
    })?;
    let ResponseBody::HandyHspLive(value) = decode(&raw)? else {
        return Err(mismatch());
    };
    if value.project_id != *project || value.stream_request_id != *id {
        return Err(mismatch());
    }
    value.validate()?;
    Ok(*value)
}
fn persist_original(
    db: &mut Connection,
    actor: &SessionId,
    request: &Request,
    fingerprint: &str,
    value: &HandyHspLiveStatus,
) -> PResult<()> {
    check_request_identity(db, actor.as_str(), request, fingerprint)?;
    let mut historical = value.clone();
    historical.native = None;
    historical.stream = None;
    historical.validate()?;
    let tx = db.transaction().map_err(internal)?;
    let changed = tx
        .execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3 AND fingerprint=?4",
            params![
                encode(&ResponseBody::HandyHspLive(Box::new(historical)))?,
                actor.as_str(),
                request.request_id.as_str(),
                fingerprint
            ],
        )
        .map_err(internal)?;
    if changed != 1 {
        return Err(mismatch());
    }
    tx.commit().map_err(internal)
}
fn origin(
    selection: &NativeCaptureSelection,
    accepted: &NativeAcceptedCapture,
    generation: u64,
    stream_id: u32,
    stream_record_id: ArtifactId,
) -> PResult<HandyHspLiveOrigin> {
    let SelectedCpuEstimate {
        input, position, ..
    } = selected_cpu_estimate(
        &accepted.estimate.result,
        selection.cpu_manifest_sha256.as_deref(),
    )?;
    Ok(HandyHspLiveOrigin {
        stream_record_id,
        frame: accepted.frame.frame,
        source_time: accepted.frame.source_time,
        source_sequence: input.sequence,
        first_position: *position,
        capture_earliest_boottime_ns: accepted.capture_boottime.earliest_ns,
        capture_latest_boottime_ns: accepted.capture_boottime.latest_ns,
        accepted_earliest_boottime_ns: accepted.accepted_boottime.earliest_ns,
        accepted_latest_boottime_ns: accepted.accepted_boottime.latest_ns,
        generation,
        stream_id,
    })
}
fn validate_basis(
    basis: &NativeCaptureSourceSnapshot,
    subject: &HandyHspSourceSubject,
    timing: &BrowserTimingPin,
) -> PResult<()> {
    let report = &basis.report;
    let declaration = report
        .options
        .browser_analysis
        .as_ref()
        .ok_or_else(mismatch)?;
    let inference = report.options.inference.as_ref().ok_or_else(mismatch)?;
    let cpu_manifest = report
        .inference
        .as_ref()
        .ok_or_else(mismatch)?
        .cpu_manifest
        .as_deref();
    if report.options.lease_ms > 30_000
        || report.continuation.is_some()
        || report.options.source_time_basis != subject.source_time_basis
        || report.age_basis != subject.age_basis
        || report.options.policy.maximum_analysis_age_ns != subject.maximum_analysis_age_ns
        || crate::inference_profiles::profile_identity(&inference.profile)
            .map_err(internal)?
            .sha256
            != subject.inference_profile_sha256()?
        || cpu_manifest
            .map(LiveCpuInferenceManifest::identity_sha256)
            .transpose()?
            != basis.selection.cpu_manifest_sha256
        || cpu_manifest
            .map(|manifest| manifest.image.identity())
            .transpose()?
            != subject.cpu_runtime_image
        || basis.selection.browser_media.selection != subject.selection
        || basis.selection.browser_media.clock != subject.clock
        || timing.binding() != basis.browser_timing.binding()
        || timing.binding().browser_media != basis.selection.browser_media
        || declaration.timing_binding_id != timing.binding().binding_id
        || declaration.player != timing.binding().player
        || declaration.policy != subject.lookahead
    {
        return Err(mismatch());
    }
    Ok(())
}
impl Engine {
    #[cfg(test)]
    pub(in crate::authority) fn hsp_live_preclaim_probe_for_test(
        &self,
        actor: &SessionId,
        original_request: &RequestId,
    ) -> HspLivePreclaimProbe {
        let registry = self.handy_hsp_live.lock().unwrap();
        let controller = registry
            .entries
            .get(&(actor.clone(), original_request.clone()))
            .expect("original native live controller");
        HspLivePreclaimProbe(controller.preclaim_test_counts.clone())
    }
    pub(in crate::authority) fn retire_handy_hsp_live(&self) {
        if let Ok(registry) = self.handy_hsp_live.try_lock() {
            for controller in registry.entries.values() {
                controller.retire();
            }
        }
        // The Engine shutdown flag independently fences a controller whose
        // registry is momentarily unavailable; no runtime lock is needed here.
    }
    fn hsp_live_report(
        &self,
        actor: &SessionId,
        project_id: &ProjectId,
        id: &RequestId,
    ) -> PResult<HandyHspLiveStatus> {
        let mut status = {
            let db = self.db.lock().map_err(internal)?;
            original(&db, actor, project_id, id)?
        };
        let controller = self
            .handy_hsp_live
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), id.clone()))
            .cloned();
        status.native = controller.as_ref().map(|c| c.status()).transpose()?;
        let owner = self
            .handy_hsp_execution
            .lock()
            .map_err(internal)?
            .as_ref()
            .map(|pump| pump.handle());
        status.stream = if let Some(owner) = owner {
            match owner.snapshot_for_session(actor, project_id, id) {
                Ok(snapshot) => Some(snapshot.into_status().map_err(|_| unavailable())?),
                Err(crate::handy_hsp_execution::HspExecutionError::NotFound) => None,
                Err(_) => return Err(unavailable()),
            }
        } else {
            None
        };
        status.validate()?;
        Ok(status)
    }
    pub(in crate::authority) fn handy_hsp_live_control(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        if continuity::is_command(&request.command) {
            return self.handy_hsp_continuity_control(request, actor, fingerprint);
        }
        request.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        let project_id = project(request)?.clone();
        let target = match &request.command {
            Command::BindHandyHspLive { .. } => request.request_id.clone(),
            Command::PlayHandyHspLive {
                stream_request_id, ..
            }
            | Command::StopHandyHspLive {
                stream_request_id, ..
            }
            | Command::HandyHspLiveStatus {
                stream_request_id, ..
            } => stream_request_id.clone(),
            _ => return Err(ProtocolError::invalid("expected live HSP command")),
        };
        if matches!(request.command, Command::HandyHspLiveStatus { .. }) {
            original(&db, actor, &project_id, &target)?;
            drop(db);
            return self
                .hsp_live_report(actor, &project_id, &target)
                .map(|s| ResponseBody::HandyHspLive(Box::new(s)));
        }
        if let Some(body) = replay(&db, actor.as_str(), request, fingerprint)? {
            let ResponseBody::HandyHspLive(saved) = body else {
                return Err(mismatch());
            };
            if saved.project_id != project_id || saved.stream_request_id != target {
                return Err(mismatch());
            }
            drop(db);
            return self
                .hsp_live_report(actor, &project_id, &target)
                .map(|s| ResponseBody::HandyHspLive(Box::new(s)));
        }
        if let Command::BindHandyHspLive {
            selection,
            acknowledge_current_environment,
            allow_online_control,
            ..
        } = &request.command
        {
            if !acknowledge_current_environment {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "live HSP binding requires explicit current environment acknowledgment",
                ));
            }
            if !allow_online_control {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "live HSP binding requires explicit online control consent",
                ));
            }
            if self.is_shutting_down() {
                return Err(unavailable());
            }
            check_revision(&db, request)?;
            let status = HandyHspLiveStatus {
                project_id: project_id.clone(),
                stream_request_id: target.clone(),
                selection: selection.clone(),
                source_subject: None,
                origin: None,
                native: None,
                stream: None,
                error: None,
                qualified: false,
            };
            let tx = db.transaction().map_err(internal)?;
            remember(
                &tx,
                actor.as_str(),
                request,
                fingerprint,
                &ResponseBody::HandyHspLive(Box::new(status)),
            )?;
            tx.commit().map_err(internal)?;
            drop(db);
            // The original actor/request/fingerprint is durable before any
            // readiness is consumed or a source controller can enter HSP Work.
            let admitted = self.bind_hsp_live(request, actor, fingerprint, selection);
            if let Err(error) = admitted {
                if let Some(controller) = self
                    .handy_hsp_live
                    .lock()
                    .map_err(internal)?
                    .entries
                    .get(&(actor.clone(), target.clone()))
                    .cloned()
                {
                    controller.fail(error.clone());
                }
                let mut db = self.db.lock().map_err(internal)?;
                let mut status = original(&db, actor, &project_id, &target)?;
                status.error = Some(error);
                persist_original(&mut db, actor, request, fingerprint, &status)?;
            }
        } else {
            let mut saved = original(&db, actor, &project_id, &target)?;
            let controller = self
                .handy_hsp_live
                .lock()
                .map_err(internal)?
                .entries
                .get(&(actor.clone(), target.clone()))
                .cloned();
            let intent = if matches!(request.command, Command::PlayHandyHspLive { .. }) {
                check_revision(&db, request)?;
                // Native command-admission time, not the browser click. Capture
                // before persistence and never refresh it on an original replay.
                Some(
                    controller
                        .as_ref()
                        .ok_or_else(unavailable)?
                        .capture_play(&request.request_id)?,
                )
            } else {
                None
            };
            saved.native = None;
            saved.stream = None;
            let retained = (|| {
                let tx = db.transaction().map_err(internal)?;
                remember(
                    &tx,
                    actor.as_str(),
                    request,
                    fingerprint,
                    &ResponseBody::HandyHspLive(Box::new(saved)),
                )?;
                tx.commit().map_err(internal)
            })();
            if let Err(error) = retained {
                if let Some(controller) = &controller {
                    controller.fail(error.clone());
                }
                return Err(error);
            }
            drop(db);
            if let Some(controller) = controller {
                if let Some(intent) = intent {
                    controller.play(intent)?;
                } else {
                    controller.stop(&request.request_id);
                }
            }
        }
        self.hsp_live_report(actor, &project_id, &target)
            .map(|s| ResponseBody::HandyHspLive(Box::new(s)))
    }
    fn bind_hsp_live(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        selection: &HandyHspLiveSelection,
    ) -> PResult<()> {
        let project_id = project(request)?.clone();
        self.handoff_handy_hsp_preparation_factory(
            actor,
            &project_id,
            &selection.preparation_request_id,
            |prepared| self.make_hsp_live_source(request, actor, fingerprint, selection, prepared),
        )?;
        Ok(())
    }
    fn make_hsp_live_source(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        selected: &HandyHspLiveSelection,
        prepared: HspPreparedSourceContext,
    ) -> PResult<(HspSourceHandoff, Arc<Controller>)> {
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_revision(&db, request)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        let subject =
            source_qualification::accepted_hsp_source_subject(&db, &selected.source_qualification)?;
        if subject.device_qualification != prepared.record.qualification
            || subject.project_id != prepared.record.project_id
            || subject.revision != prepared.record.revision
        {
            return Err(mismatch());
        }
        let timing = self.capture_browser_timing_locked(&db, request, &selected.browser_timing)?;
        let media = timing.binding().browser_media.clone();
        let source = self.capture_handy_hsp_source_locked(
            &db,
            request,
            &selected.source_qualification,
            &media,
            &subject,
        )?;
        if source.binding() != &selected.source_qualification {
            return Err(mismatch());
        }
        let finite_drain = source_qualification::accepted_hsp_finite_drain_policy(
            &db,
            &self.pool,
            &source,
            &prepared.record.qualification,
            &prepared.record.registration,
            selected.finite_drain_policy_sha256.as_deref(),
        )?;
        if finite_drain
            .as_ref()
            .map(|policy| policy.policy_sha256.as_str())
            != selected.finite_drain_policy_sha256.as_deref()
            || finite_drain.as_ref().is_some_and(|selected| {
                selected
                    .maximum_stop_dispatch_delay_ns
                    .checked_add(selected.stopping_bound_ns)
                    != Some(selected.policy.stop_budget_ns)
            })
        {
            return Err(mismatch());
        }
        let identity = LiveIdentity {
            epoch: selected.capture_epoch.clone(),
            project: subject.project_id.clone(),
            revision: subject.revision,
            source: subject.selection.source_version.clone(),
        };
        let selection = NativeCaptureSelection {
            owner: actor.clone(),
            identity: identity.clone(),
            attempt_id: selected.inference_attempt_id.clone(),
            profile_sha256: crate::inference_profiles::profile_identity(&subject.inference_profile)
                .map_err(internal)?
                .sha256,
            cpu_manifest_sha256: None,
            browser_media: media,
        };
        let mut captures = self.live_captures.try_lock().map_err(|_| unavailable())?;
        captures.sweep(&db)?;
        let selection = captures.bind_native_cpu_mode(selection)?;
        let basis = captures.native_source_basis(&selection)?;
        validate_basis(&basis, &subject, &timing)?;
        let first = basis.latest.clone().ok_or_else(unavailable)?;
        let zero = subject
            .clock
            .project_at(first.frame.source_time, &subject.selection.placement.spec)
            .map_err(internal)?;
        let mut registry = self.handy_hsp_live.lock().map_err(internal)?;
        if registry.entries.len() >= MAX_OWNERS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "live HSP source owner capacity reached",
            ));
        }
        registry.next_generation = registry
            .next_generation
            .checked_add(1)
            .ok_or_else(unavailable)?;
        let generation = registry.next_generation;
        let stream_id = u32::try_from(generation).map_err(internal)?;
        let binding = HspBinding::new(
            HspStreamIdentity {
                generation,
                stream_id,
                live: identity,
            },
            ClientId::new(actor.as_str()).map_err(internal)?,
            prepared.record.device_session.clone(),
            prepared.clocks.server.clock.domain.clone(),
        );
        let stream_record_id = ArtifactId::new(
            prepared
                .stream_record(actor, &request.request_id, &binding)
                .id()
                .map_err(internal)?,
        )
        .map_err(internal)?;
        let envelope = HspEnvelope {
            minimum_position_percent: prepared.profile.minimum_position_percent,
            maximum_position_percent: prepared.profile.maximum_position_percent,
            maximum_step_percent: prepared.profile.maximum_step_percent,
            minimum_segment_ms: prepared.profile.minimum_segment_ms,
        };
        let controller = Arc::new(Controller {
            binding: binding.clone(),
            initial_source: HspSourceCursor {
                identity: selection.identity.clone(),
                attempt_id: selection.attempt_id.clone(),
            },
            active: ArcSwapOption::empty(),
            engine: Arc::downgrade(self),
            original_request: request.clone(),
            preparation_request: selected.preparation_request_id.clone(),
            captures: Arc::downgrade(&self.live_captures),
            handle: OnceLock::new(),
            pool: self.pool.clone(),
            clocks: prepared.clocks,
            zero,
            clock_until_ns: prepared.valid_until_ns,
            clock_owner: OnceLock::new(),
            continuity_enable: OnceLock::new(),
            continuity: Mutex::new(ContinuityState::default()),
            source_custody: ArcSwapOption::empty(),
            staged_source: ArcSwapOption::empty(),
            retained_transfer: ArcSwapOption::empty(),
            pending_source_work: AtomicBool::new(false),
            advancing_source: AtomicBool::new(false),
            policy: prepared.policy,
            finite_drain,
            shutdown: self.shutdown_requested.clone(),
            retired: AtomicBool::new(false),
            retirement_complete: AtomicBool::new(false),
            delivered: AtomicU64::new(0),
            delivery: Mutex::new(Delivery {
                staged: vec![first.clone()],
                envelope: HspEnvelopeState::new(envelope).map_err(internal)?,
                last: None,
                last_sample: None,
                pending_end: None,
                ended: None,
            }),
            controls: Mutex::new(Controls::default()),
            #[cfg(test)]
            preclaim_test_counts: Arc::new(PreclaimTestCounts::default()),
            #[cfg(test)]
            producer_read_test: Mutex::new(None),
        });
        let sink = Arc::new(EpochSink::new(&controller, selection.clone()));
        let subscription = captures.register_native_delivery(selection.clone(), sink.clone())?;
        controller.active.store(Some(Arc::new(SourceBundle {
            generation: 1,
            selection,
            subscription,
            timing,
            source,
            media: basis.source_media,
            until_ns: basis.lease_deadline_boottime_ns,
            continuity_generation: basis.continuity_generation,
            sink,
            input_closed: AtomicBool::new(false),
            predecessor_epoch: None,
            transfer_request: None,
        })));
        registry.entries.insert(
            (actor.clone(), request.request_id.clone()),
            controller.clone(),
        );
        drop(registry);
        drop(captures);
        let mut status = original(&db, actor, &subject.project_id, &request.request_id)?;
        status.source_subject = Some(subject);
        status.origin = Some(origin(
            &basis.selection,
            &first,
            generation,
            stream_id,
            stream_record_id,
        )?);
        if let Err(error) = persist_original(&mut db, actor, request, fingerprint, &status) {
            return Err(controller.fail(error));
        }
        drop(db);
        Ok((
            HspSourceHandoff {
                child_request: request.request_id.clone(),
                allow_online_control: true,
                binding,
                authority: controller.clone(),
            },
            controller,
        ))
    }
}

/// Request history is a witness only. Startup must not accept a saved native
/// owner or restore one from a structurally valid source/stream description.
pub(super) fn validate_history(db: &Connection, state_dir: &std::path::Path) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let records =
        crate::device_effect_journal::DeviceEffectJournal::retained_hsp_stream_records(state_dir)?;
    let mut query = db.prepare("SELECT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_hsp_live' ELSE response LIKE '%handy_hsp_live%' END LIMIT 4097")?;
    let mut count = 0;
    for row in query.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (actor, raw) = row?;
        count += 1;
        anyhow::ensure!(raw.len() <= 262144, "live HSP history exceeds bound");
        let ResponseBody::HandyHspLive(value) = serde_json::from_str(&raw)? else {
            continue;
        };
        value.validate()?;
        anyhow::ensure!(
            value.native.is_none() && value.stream.is_none(),
            "native live HSP ownership cannot be restored from request history"
        );
        let resident = records.iter().any(|(record, _)| {
            record.actor_session.as_str() == actor && record.request_id == value.stream_request_id
        });
        anyhow::ensure!(
            !resident || value.origin.is_some(),
            "live HSP journal history lacks its preceding source witness"
        );
        if let (Some(origin), Some(subject)) = (&value.origin, &value.source_subject) {
            // Historical consistency does not require current acceptance. A
            // withdrawn dossier or vanished authored-test permit cannot restore
            // authority, and cannot erase the original immutable subject.
            let retained = source_qualification::retained_hsp_source_subject(
                db,
                &value.selection.source_qualification.dossier_id,
            )?;
            anyhow::ensure!(
                subject.identity_sha256()? == retained.identity_sha256()?,
                "live HSP source witness differs from retained dossier subject"
            );
            for (record, link) in records.iter().filter(|(record, _)| {
                record.id().ok().as_deref() == Some(origin.stream_record_id.as_str())
                    || (record.actor_session.as_str() == actor
                        && record.request_id == value.stream_request_id)
            }) {
                anyhow::ensure!(
                    record.id()? == origin.stream_record_id.as_str()
                        && record.actor_session.as_str() == actor
                        && record.request_id == value.stream_request_id
                        && record.binding.owner.as_str() == actor
                        && record.binding.identity.live.project == value.project_id
                        && record.binding.identity.live.revision == subject.revision
                        && record.binding.identity.live.source == subject.selection.source_version
                        && record.binding.identity.live.epoch == value.selection.capture_epoch
                        && record.binding.identity.generation == origin.generation
                        && record.binding.identity.stream_id == origin.stream_id
                        && record.qualification_sha256
                            == subject.device_qualification.dossier_id.as_str()
                        && link
                            .as_ref()
                            .is_some_and(|link| link.qualification == subject.device_qualification),
                    "live HSP request/source witness differs from journal history"
                );
            }
        }
    }
    anyhow::ensure!(count <= 4096, "live HSP histories exceed startup bound");
    continuity::validate_history(db)
}

#[cfg(test)]
#[path = "handy_hsp_live_cpu_tests.rs"]
mod cpu_mode_tests;
