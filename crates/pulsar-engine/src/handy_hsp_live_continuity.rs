//! Source-epoch routing and native continuity promotion for one retained HSP
//! controller. Serialized descriptors never recreate any of these capabilities.
use super::*;
use crate::{
    handy_hsp_execution::HspExecutionError,
    handy_hsp_runtime::{HspRuntimeError, HspRuntimeState},
};
use pulsar_core::HspStreamError;
use std::sync::TryLockError;

pub(super) struct SourceBundle {
    pub generation: u64,
    pub selection: NativeCaptureSelection,
    pub subscription: NativeCaptureSubscription,
    pub timing: BrowserTimingPin,
    pub source: AcceptedHspSource,
    pub media: browser_media::SourceMediaPin,
    pub until_ns: u64,
    pub continuity_generation: u64,
    // The producer retains only a Weak sink. This is the epoch's strong owner;
    // the sink's reverse Controller reference is Weak, so no cycle is formed.
    pub sink: Arc<EpochSink>,
    pub input_closed: AtomicBool,
    pub predecessor_epoch: Option<AttemptId>,
    pub transfer_request: Option<RequestId>,
}
impl SourceBundle {
    pub fn current(&self, at: u64) -> bool {
        at < self.until_ns
            && !self.sink.retired.load(Ordering::Acquire)
            && self.subscription.current()
            && self.source.current()
            && self.media.current()
    }
    fn retired_snapshot(&self) -> Self {
        Self {
            generation: self.generation,
            selection: self.selection.clone(),
            subscription: self.subscription.clone(),
            timing: self.timing.clone(),
            source: self.source.clone(),
            media: self.media.clone(),
            until_ns: self.until_ns,
            continuity_generation: self.continuity_generation,
            sink: self.sink.clone(),
            input_closed: AtomicBool::new(self.input_closed.load(Ordering::Acquire)),
            predecessor_epoch: self.predecessor_epoch.clone(),
            transfer_request: self.transfer_request.clone(),
        }
    }
    fn cursor(&self) -> HspSourceCursor {
        HspSourceCursor {
            identity: self.selection.identity.clone(),
            attempt_id: self.selection.attempt_id.clone(),
        }
    }
}

pub(super) struct EpochSink {
    controller: Weak<Controller>,
    selection: NativeCaptureSelection,
    retired: AtomicBool,
    reservation: Option<NativeCaptureSuccessor>,
    first: OnceLock<NativeAcceptedCapture>,
}
impl EpochSink {
    pub(super) fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }
    pub fn new(controller: &Arc<Controller>, selection: NativeCaptureSelection) -> Self {
        Self {
            controller: Arc::downgrade(controller),
            selection,
            retired: AtomicBool::new(false),
            reservation: None,
            first: OnceLock::new(),
        }
    }
    fn successor(controller: &Arc<Controller>, reservation: NativeCaptureSuccessor) -> Self {
        Self {
            controller: Arc::downgrade(controller),
            selection: reservation.successor().clone(),
            retired: AtomicBool::new(false),
            reservation: Some(reservation),
            first: OnceLock::new(),
        }
    }
    fn stage_first(&self, owner: &Controller, accepted: NativeAcceptedCapture) -> PResult<()> {
        let Some(reservation) = &self.reservation else {
            return Ok(());
        };
        if owner.active_source()?.selection != *reservation.predecessor() {
            return Ok(());
        }
        let SelectedCpuEstimate {
            attempt_id,
            identity,
            profile_sha256,
            input,
            ..
        } = selected_cpu_estimate(
            &accepted.estimate.result,
            self.selection.cpu_manifest_sha256.as_deref(),
        )?;
        if !reservation.transfer_started()
            || self.retired.load(Ordering::Acquire)
            || accepted.frame.identity != self.selection.identity
            || identity != &self.selection.identity
            || attempt_id != &self.selection.attempt_id
            || profile_sha256 != &self.selection.profile_sha256
            || input.sequence != 1
            || accepted.continuous_from_previous
            || !accepted.estimate.fresh
            || accepted.estimate.qualified
        {
            return Err(mismatch());
        }
        self.first.set(accepted).map_err(|_| mismatch())?;
        owner.pending_source_work.store(true, Ordering::Release);
        Ok(())
    }
    fn owner(&self) -> PResult<Arc<Controller>> {
        self.controller.upgrade().ok_or_else(unavailable)
    }
    fn active(&self, controller: &Controller) -> Option<Arc<SourceBundle>> {
        controller
            .active
            .load_full()
            .filter(|source| source.selection == self.selection)
    }
    fn reject(&self, error: ProtocolError) -> ProtocolError {
        self.retire();
        error
    }
}
impl NativeCaptureSink for EpochSink {
    fn offer(&self, accepted: NativeAcceptedCapture) -> PResult<()> {
        let owner = self.owner()?;
        if self.active(&owner).is_none() {
            return self
                .stage_first(&owner, accepted)
                .map_err(|e| self.reject(e));
        }
        let mut delivery = owner
            .delivery
            .try_lock()
            .map_err(|_| self.reject(unavailable()))?;
        let Some(source) = self.active(&owner) else {
            return Ok(());
        };
        if source.input_closed.load(Ordering::Acquire) || self.retired.load(Ordering::Acquire) {
            return Err(self.reject(unavailable()));
        }
        let result =
            if let Some(handle) = owner.handle.get().and_then(HspWeakSessionHandle::upgrade) {
                owner.deliver_locked(&mut delivery, &source, accepted, &handle)
            } else if owner.handle.get().is_some() || delivery.staged.len() >= MAX_STAGED {
                Err(unavailable())
            } else {
                delivery.staged.push(accepted);
                Ok(())
            };
        result.map_err(|e| self.reject(e))
    }
    fn finish_input(&self, end: NativeCaptureEnd) -> PResult<()> {
        let owner = self.owner()?;
        if self.active(&owner).is_none() {
            return Ok(());
        }
        let mut delivery = owner
            .delivery
            .try_lock()
            .map_err(|_| self.reject(unavailable()))?;
        let Some(source) = self.active(&owner) else {
            return Ok(());
        };
        if end.selection() != &source.selection || self.retired.load(Ordering::Acquire) {
            return Err(self.reject(mismatch()));
        }
        source.input_closed.store(true, Ordering::Release);
        let result =
            if let Some(handle) = owner.handle.get().and_then(HspWeakSessionHandle::upgrade) {
                owner.finish_locked(&mut delivery, &source, end, &handle)
            } else if owner.handle.get().is_some() || delivery.pending_end.is_some() {
                Err(unavailable())
            } else {
                delivery.pending_end = Some(end);
                Ok(())
            };
        result.map_err(|e| self.reject(e))
    }
    fn prepare_successor(&self, successor: NativeCaptureSuccessor) -> PResult<()> {
        let owner = self.owner()?;
        if self.active(&owner).is_none() {
            return Err(unavailable());
        }
        owner.retain_successor(&self.selection, successor)
    }
    fn transfer_input(&self, transfer: NativeCaptureTransfer) -> PResult<()> {
        let owner = self.owner()?;
        if self.active(&owner).is_none() {
            return Ok(());
        }
        owner
            .retain_transfer(&self.selection, transfer)
            .map_err(|e| self.reject(e))
    }
    fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        if let Some(owner) = self.controller.upgrade() {
            owner.pending_source_work.store(true, Ordering::Release);
            if owner.retired.load(Ordering::Acquire) {
                return;
            }
            let Some(active) = self.active(&owner) else {
                return;
            };
            // Retirement and promotion compete on the SAME active Arc pointer.
            // If retirement wins, the current owner is cancelled immediately.
            // If promotion wins, this old epoch can never cancel its successor.
            let retired = Arc::new(active.retired_snapshot());
            let previous = owner
                .active
                .compare_and_swap(&Some(active.clone()), Some(retired));
            if previous
                .as_ref()
                .is_some_and(|prior| Arc::ptr_eq(prior, &active))
            {
                owner.retire();
            }
        }
    }
}

pub(super) struct StagedSuccessor {
    reservation: NativeCaptureSuccessor,
    bundle: Arc<SourceBundle>,
    media: browser_media::NativeBrowserMediaSuccessor,
}
#[derive(Default)]
pub(super) struct ContinuityState {
    successor: Option<NativeCaptureSuccessor>,
    staged: Option<Arc<StagedSuccessor>>,
    transition: Option<HspSourceTransition>,
    transition_pin: Option<HspClockPin>,
}
struct Advancing<'a>(&'a AtomicBool);
impl Drop for Advancing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn same_successor(a: &NativeCaptureSuccessor, b: &NativeCaptureSuccessor) -> bool {
    // Producer equality compares its private Arc identity, never DTO fields.
    a == b
}

fn same_accepted(a: &NativeAcceptedCapture, b: &NativeAcceptedCapture) -> PResult<bool> {
    Ok(a.frame == b.frame
        && a.previous_frame == b.previous_frame
        && a.capture_boottime == b.capture_boottime
        && a.previous_capture_boottime == b.previous_capture_boottime
        && a.accepted_boottime == b.accepted_boottime
        && a.continuity_generation == b.continuity_generation
        && a.continuous_from_previous == b.continuous_from_previous
        && serde_json::to_vec(&a.estimate).map_err(internal)?
            == serde_json::to_vec(&b.estimate).map_err(internal)?)
}
fn transition_wait(error: &HspExecutionError) -> bool {
    matches!(
        error,
        HspExecutionError::Busy
            | HspExecutionError::Dispatch(HspDispatchFailure::Authority(
                HspRuntimeError::NativeDispatchBusy
            ))
            | HspExecutionError::Dispatch(HspDispatchFailure::Authority(HspRuntimeError::Core(
                HspStreamError::NotReady
            )))
    )
}

impl Controller {
    fn retain_successor(
        &self,
        caller: &NativeCaptureSelection,
        successor: NativeCaptureSuccessor,
    ) -> PResult<()> {
        let source = self.active_source()?;
        let at = now()?;
        if caller != &source.selection
            || successor.predecessor() != caller
            || !self.current(&source, at)
            || source.input_closed.load(Ordering::Acquire)
            || self.clock_owner.get().is_none()
            || at >= successor.expires_at_ns()
        {
            return Err(unavailable());
        }
        self.clock_pin(at)?.ok_or_else(unavailable)?;
        let mut pending = self.continuity.try_lock().map_err(|_| unavailable())?;
        if let Some(original) = &pending.successor {
            return if same_successor(original, &successor) {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        if pending.staged.is_some()
            || self.retained_transfer.load_full().is_some()
            || pending.transition.is_some()
        {
            return Err(mismatch());
        }
        if self.source_custody.load_full().is_some() {
            return Err(unavailable());
        }
        self.source_custody.store(Some(Arc::new(successor.clone())));
        if self.retired.load(Ordering::Acquire) {
            successor.cancel();
            return Err(unavailable());
        }
        pending.successor = Some(successor);
        self.pending_source_work.store(true, Ordering::Release);
        Ok(())
    }
    fn retain_transfer(
        &self,
        caller: &NativeCaptureSelection,
        transfer: NativeCaptureTransfer,
    ) -> PResult<()> {
        let source = self.active_source()?;
        if caller != &source.selection {
            return Ok(());
        }
        // The producer has already settled A delivery and B's first callback.
        // This guard only serializes publication against a simultaneous commit;
        // it performs no dispatch, registry lookup or clock acquisition.
        let _delivery = match self.delivery.try_lock() {
            Ok(delivery) => delivery,
            Err(TryLockError::WouldBlock) => {
                if self.active_source()?.selection != *caller {
                    return Ok(());
                }
                return if self
                    .retained_transfer
                    .load_full()
                    .is_some_and(|original| original.as_ref() == &transfer)
                {
                    Ok(())
                } else {
                    Err(unavailable())
                };
            }
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        if self.active_source()?.selection != *caller {
            return Ok(());
        }
        if transfer.predecessor() != caller || now()? >= transfer.expires_at_ns() {
            return Err(mismatch());
        }
        let reservation = self.source_custody.load_full().ok_or_else(mismatch)?;
        let staged = self.staged_source.load_full().ok_or_else(mismatch)?;
        if !same_successor(&reservation, transfer.successor_reservation())
            || staged.bundle.selection != *transfer.successor()
            || !same_accepted(
                staged.bundle.sink.first.get().ok_or_else(mismatch)?,
                transfer.successor_first(),
            )?
        {
            return Err(mismatch());
        }
        if let Some(original) = self.retained_transfer.load_full() {
            return if original.as_ref() == &transfer
                && original.expires_at_ns() == transfer.expires_at_ns()
            {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        self.retained_transfer.store(Some(Arc::new(transfer)));
        self.pending_source_work.store(true, Ordering::Release);
        Ok(())
    }
    fn discard_unused_successor(&self) -> PResult<()> {
        let mut pending = self.continuity.try_lock().map_err(|_| unavailable())?;
        if pending
            .successor
            .as_ref()
            .is_some_and(NativeCaptureSuccessor::transfer_started)
            || self.retained_transfer.load_full().is_some()
            || pending.transition.is_some()
        {
            return Err(unavailable());
        }
        if let Some(reservation) = &pending.successor {
            reservation.cancel();
        }
        if let Some(staged) = pending.staged.take() {
            staged.bundle.subscription.retire();
        }
        *pending = ContinuityState::default();
        self.retained_transfer.store(None);
        self.staged_source.store(None);
        self.pending_source_work.store(true, Ordering::Release);
        Ok(())
    }
    pub(super) fn progress_source(&self, db: &Connection) -> PResult<()> {
        if self.advancing_source.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let _advancing = Advancing(&self.advancing_source);
        let source = self.active_source()?;
        if !self.current(&source, now()?) {
            return Err(unavailable());
        }
        let (reservation, staged, transfer) = {
            let pending = match self.continuity.try_lock() {
                Ok(pending) => pending,
                Err(TryLockError::WouldBlock) => return Ok(()),
                Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
            };
            (
                pending.successor.clone(),
                pending.staged.clone(),
                self.retained_transfer
                    .load_full()
                    .map(|proof| (*proof).clone()),
            )
        };
        let Some(reservation) = reservation else {
            if let Some(custody) = self.source_custody.load_full() {
                if !custody.cancelled_worker_joined() {
                    return Ok(());
                }
                self.source_custody.store(None);
            }
            self.pending_source_work.store(false, Ordering::Release);
            return Ok(());
        };
        if reservation.predecessor() != &source.selection {
            return Err(mismatch());
        }
        if let Some(staged) = &staged {
            if staged.bundle.sink.retired.load(Ordering::Acquire) {
                return if reservation.transfer_started() {
                    Err(unavailable())
                } else {
                    self.discard_unused_successor()
                };
            }
        }
        if staged.is_none() {
            return self.prepare_successor_bundle(db, source, reservation);
        }
        let Some(transfer) = transfer else {
            return Ok(());
        };
        self.promote_successor(db, source, staged.expect("checked staged source"), transfer)
    }
    fn prepare_successor_bundle(
        &self,
        db: &Connection,
        source: Arc<SourceBundle>,
        reservation: NativeCaptureSuccessor,
    ) -> PResult<()> {
        let engine = self.engine.upgrade().ok_or_else(unavailable)?;
        let captures = self.captures.upgrade().ok_or_else(unavailable)?;
        // Resolve/verify media and accepted-subject pins before taking the final
        // capture/timing/controller lock group. These helpers may read evidence.
        let media = engine.capture_browser_media_successor_locked(
            db,
            &self.original_request,
            &source.selection.browser_media,
            &reservation.successor().browser_media,
        )?;
        if !media.matches(&source.media, media.successor()) {
            return Err(mismatch());
        }
        let accepted = engine.capture_handy_hsp_source_successor_locked(
            db,
            &self.original_request,
            &source.source,
            &media,
            source.source.subject(),
        )?;
        let registry = match captures.try_lock() {
            Ok(registry) => registry,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        let basis = match registry.try_native_successor_basis(&reservation) {
            Ok(Some(basis)) => basis,
            Ok(None) => return Ok(()),
            Err(_error) if !reservation.transfer_started() => {
                drop(registry);
                self.discard_unused_successor()?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if !media.matches(&source.media, &basis.source_media) {
            return Err(mismatch());
        }
        let declaration = basis
            .report
            .options
            .browser_analysis
            .as_ref()
            .ok_or_else(mismatch)?;
        // This native capture only looks up the independently prepared timing
        // binding; it does not synthesize or refresh its observation.
        drop(registry);
        let timing = engine.capture_browser_timing_locked(
            db,
            &self.original_request,
            &declaration.timing_binding_id,
        )?;
        validate_successor_basis(&basis, &accepted, &timing, &reservation)?;
        let mut registry = match captures.try_lock() {
            Ok(registry) => registry,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        let Some(current) = registry.try_native_successor_basis(&reservation)? else {
            return Ok(());
        };
        validate_successor_basis(&current, &accepted, &timing, &reservation)?;
        if !media.matches(&source.media, &current.source_media) {
            return Err(mismatch());
        }
        let mut pending = match self.continuity.try_lock() {
            Ok(pending) => pending,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        if pending
            .successor
            .as_ref()
            .is_none_or(|original| !same_successor(original, &reservation))
            || pending.staged.is_some()
            || self.active_source()?.selection != source.selection
        {
            return Err(mismatch());
        }
        let controller = source.sink.controller.upgrade().ok_or_else(unavailable)?;
        let sink = Arc::new(EpochSink::successor(&controller, reservation.clone()));
        let subscription =
            registry.register_native_successor_delivery(&reservation, sink.clone())?;
        let generation = source.generation.checked_add(1).ok_or_else(unavailable)?;
        let bundle = Arc::new(SourceBundle {
            generation,
            selection: current.selection,
            subscription,
            timing,
            source: accepted,
            media: media.successor().clone(),
            until_ns: current.lease_deadline_boottime_ns,
            continuity_generation: current.continuity_generation,
            sink,
            input_closed: AtomicBool::new(false),
            predecessor_epoch: Some(source.selection.identity.epoch.clone()),
            transfer_request: None,
        });
        let staged = Arc::new(StagedSuccessor {
            reservation,
            bundle,
            media,
        });
        self.staged_source.store(Some(staged.clone()));
        pending.staged = Some(staged);
        Ok(())
    }
}

fn validate_successor_basis(
    basis: &NativeCaptureSourceSnapshot,
    source: &AcceptedHspSource,
    timing: &BrowserTimingPin,
    reservation: &NativeCaptureSuccessor,
) -> PResult<()> {
    if &basis.selection != reservation.successor() {
        return Err(mismatch());
    }
    // The opaque reservation, rather than a report's continuation field, proves
    // that this is the independently prepared successor of the active epoch.
    let checked = &basis.report;
    let subject = source.subject();
    let declaration = checked
        .options
        .browser_analysis
        .as_ref()
        .ok_or_else(mismatch)?;
    let profile = checked.options.inference.as_ref().ok_or_else(mismatch)?;
    if checked.options.lease_ms > 30_000
        || checked.options.source_time_basis != subject.source_time_basis
        || checked.age_basis != subject.age_basis
        || checked.options.policy.maximum_analysis_age_ns != subject.maximum_analysis_age_ns
        || crate::inference_profiles::profile_identity(&profile.profile)
            .map_err(internal)?
            .sha256
            != subject.inference_profile_sha256()?
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

impl Controller {
    pub(super) fn successor_work_joined(&self) -> bool {
        if self.advancing_source.load(Ordering::Acquire)
            || (self.retired.load(Ordering::Acquire)
                && !self.retirement_complete.load(Ordering::Acquire))
        {
            return false;
        }
        self.source_custody
            .load_full()
            .is_none_or(|cap| cap.cancelled_worker_joined())
    }
    pub(super) fn join_successor_work(&self) -> PResult<()> {
        if self.advancing_source.load(Ordering::Acquire)
            || (self.retired.load(Ordering::Acquire)
                && !self.retirement_complete.load(Ordering::Acquire))
        {
            return Err(unavailable());
        }
        let Some(cap) = self.source_custody.load_full() else {
            return Ok(());
        };
        cap.cancel();
        cap.join_cancelled(Instant::now() + crate::handy_hsp_execution::DRAIN_BUDGET)?;
        if !cap.cancelled_worker_joined() {
            return Err(unavailable());
        }
        self.source_custody.store(None);
        Ok(())
    }
    pub(super) fn active_source_status(&self) -> PResult<Option<HandyHspLiveActiveSource>> {
        if self.clock_owner.get().is_none() {
            return Ok(None);
        }
        let active = self.active_source()?;
        let successor = self.source_custody.load_full().filter(|cap| {
            cap.predecessor() == &active.selection && cap.successor() != &active.selection
        });
        let transfer = self.retained_transfer.load_full().filter(|proof| {
            proof.predecessor() == &active.selection
                && successor
                    .as_ref()
                    .is_some_and(|cap| same_successor(cap, proof.successor_reservation()))
        });
        Ok(Some(HandyHspLiveActiveSource {
            generation: active.generation,
            capture_epoch: active.selection.identity.epoch.clone(),
            inference_attempt_id: active.selection.attempt_id.clone(),
            browser_timing: active.timing.binding().binding_id.clone(),
            media_preparation_request_id: active
                .selection
                .browser_media
                .preparation_request_id
                .clone(),
            predecessor_capture_epoch: active.predecessor_epoch.clone(),
            last_transfer_request_id: active.transfer_request.clone(),
            pending_successor_epoch: successor.map(|cap| cap.successor().identity.epoch.clone()),
            pending_transfer_request_id: transfer.map(|proof| proof.request_id().clone()),
        }))
    }
    fn successor_sample(
        &self,
        source: &SourceBundle,
        accepted: &NativeAcceptedCapture,
        at: u64,
        observation: &BrowserTimingObservation,
        pin: &HspClockPin,
    ) -> PResult<HspSample> {
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
        if !self.current(source, at)
            || accepted.frame.identity != source.selection.identity
            || identity != &source.selection.identity
            || attempt_id != &source.selection.attempt_id
            || profile_sha256 != &source.selection.profile_sha256
            || input.sequence != 1
            || accepted.continuous_from_previous
            || !accepted.estimate.fresh
            || accepted.estimate.qualified
            || accepted.continuity_generation != source.continuity_generation
            || accepted.accepted_boottime.latest_ns > at
        {
            return Err(mismatch());
        }
        let relative = Self::source_project(source, accepted)?
            .checked_sub(self.zero)
            .map_err(internal)?;
        let selected = quantize_hsp_point(
            relative.quantize_nanoseconds().map_err(internal)?.time,
            *position,
        )
        .map_err(internal)?;
        let facts = self.facts_from(source, at, observation, Some(pin))?;
        let phase = facts.reference().phase();
        let minimum = (i128::from(selected.point.time_ms) - i128::from(phase.latest_ms))
            .checked_mul(1000)
            .ok_or_else(mismatch)?;
        let maximum = (i128::from(selected.point.time_ms) - i128::from(phase.earliest_ms))
            .checked_mul(1000)
            .ok_or_else(mismatch)?;
        let lookahead = source.source.subject().lookahead;
        let captured_at_ns = accepted
            .capture_boottime
            .earliest_ns
            .checked_sub(source.source.subject().maximum_capture_to_engine_receipt_ns)
            .ok_or_else(mismatch)?;
        if minimum < i128::from(lookahead.minimum_lead_us)
            || maximum > i128::from(lookahead.maximum_lead_us)
            || at
                .checked_sub(captured_at_ns)
                .is_none_or(|age| age > self.policy.max_sample_age_ns)
        {
            return Err(unavailable());
        }
        Ok(HspSample {
            point: selected.point,
            source_sequence: input.sequence,
            captured_at_ns,
            accepted_at_ns: accepted.accepted_boottime.latest_ns,
            continuous_from_previous: accepted.continuous_from_previous,
        })
    }
    fn promote_successor(
        &self,
        db: &Connection,
        source: Arc<SourceBundle>,
        staged: Arc<StagedSuccessor>,
        proof: NativeCaptureTransfer,
    ) -> PResult<()> {
        let at = now()?;
        if at >= proof.expires_at_ns()
            || source.input_closed.load(Ordering::Acquire)
            || proof.predecessor() != &source.selection
            || proof.successor() != &staged.bundle.selection
            || !same_successor(&staged.reservation, proof.successor_reservation())
            || !staged.media.current()
            || !self.current(&source, at)
        {
            return Err(unavailable());
        }
        source_qualification::revalidate_accepted_hsp_source(db, &self.pool, &source.source)?;
        source_qualification::revalidate_accepted_hsp_source(
            db,
            &self.pool,
            &staged.bundle.source,
        )?;
        let captures = self.captures.upgrade().ok_or_else(unavailable)?;
        let mut registry = match captures.try_lock() {
            Ok(registry) => registry,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        let guard = match registry.try_claim_native_transfer(&proof) {
            Ok(guard) => guard,
            Err(NativeCaptureTransferFailure::Contended) => return Ok(()),
            Err(NativeCaptureTransferFailure::Refused(error)) => return Err(error),
        };
        if !staged.media.matches(
            &guard.predecessor().source_media,
            &guard.successor().source_media,
        ) || guard.predecessor().selection != source.selection
            || guard.successor().selection != staged.bundle.selection
            || guard.predecessor().lease_deadline_boottime_ns != source.until_ns
            || guard.successor().lease_deadline_boottime_ns != staged.bundle.until_ns
        {
            return Err(mismatch());
        }
        let mut timing = match source.timing.try_claim_successor_native(
            &staged.bundle.timing,
            &staged.media,
            now,
        ) {
            Ok(timing) => timing,
            Err(BrowserTimingClaimFailure::Contended) => return Ok(()),
            Err(BrowserTimingClaimFailure::Refused(error)) => return Err(error),
        };
        let mut delivery = match self.delivery.try_lock() {
            Ok(delivery) => delivery,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        let mut pending = match self.continuity.try_lock() {
            Ok(pending) => pending,
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Poisoned(_)) => return Err(unavailable()),
        };
        if self.active_source()?.selection != source.selection
            || self
                .retained_transfer
                .load_full()
                .is_none_or(|original| original.as_ref() != &proof)
            || pending
                .staged
                .as_ref()
                .is_none_or(|original| !Arc::ptr_eq(original, &staged))
            || !same_accepted(
                delivery.last.as_ref().ok_or_else(mismatch)?,
                proof.predecessor_final(),
            )?
            || !same_accepted(
                staged.bundle.sink.first.get().ok_or_else(mismatch)?,
                proof.successor_first(),
            )?
            || proof.predecessor_final().frame != proof.boundary().predecessor_frame
            || proof.successor_first().previous_frame != proof.boundary().successor_seed
            || proof.predecessor_final().capture_boottime != proof.boundary().predecessor_capture
            || proof.successor_first().previous_capture_boottime
                != proof.boundary().successor_capture
        {
            return Err(mismatch());
        }
        let pin = if let Some(pin) = &pending.transition_pin {
            pin.clone()
        } else {
            self.clock_pin(now()?)?.ok_or_else(unavailable)?
        };
        let evaluated = now()?;
        let (_, successor_observation) =
            timing.observations_for(&source.timing, &staged.bundle.timing, evaluated)?;
        let first = self.successor_sample(
            &staged.bundle,
            proof.successor_first(),
            evaluated,
            successor_observation,
            &pin,
        )?;
        let last = delivery.last_sample.ok_or_else(mismatch)?;
        // The opaque producer proof above establishes the cross-epoch edge.
        // Only the pure envelope check receives that proved edge; the original
        // successor HspSample remains sequence 1 with its original false flag.
        let mut envelope = delivery.envelope.clone();
        envelope.accept(first.point, true).map_err(internal)?;
        let next_delivered = self
            .delivered
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or_else(unavailable)?;
        let handle = self
            .handle
            .get()
            .and_then(HspWeakSessionHandle::upgrade)
            .ok_or_else(unavailable)?;
        if pending.transition.is_none() {
            let spec = HspSourceTransitionSpec {
                predecessor: source.cursor(),
                successor: staged.bundle.cursor(),
                predecessor_tail: last,
                first_successor: first,
                expires_at_ns: proof.expires_at_ns().min(pin.valid_until_ns()),
            };
            match handle.prepare_source_transition(spec, |at| {
                let (observation, _) = timing
                    .observations_for(&source.timing, &staged.bundle.timing, at)
                    .map_err(dispatch)?;
                self.facts_from(&source, at, observation, Some(&pin))
                    .map_err(dispatch)
            }) {
                Ok(token) => {
                    pending.transition = Some(token);
                    pending.transition_pin = Some(pin.clone());
                }
                Err(error) if transition_wait(&error) => return Ok(()),
                Err(error) => {
                    return Err(internal(format!(
                        "native source transition admission failed: {error:?}"
                    )))
                }
            }
        }
        let token = pending.transition.as_ref().ok_or_else(mismatch)?;
        if token.predecessor() != &source.cursor()
            || token.successor() != &staged.bundle.cursor()
            || token.predecessor_tail() != last
            || token.first_successor() != first
        {
            return Err(mismatch());
        }
        let next_source = Arc::new(SourceBundle {
            generation: staged.bundle.generation,
            selection: staged.bundle.selection.clone(),
            subscription: staged.bundle.subscription.clone(),
            timing: staged.bundle.timing.clone(),
            source: staged.bundle.source.clone(),
            media: staged.bundle.media.clone(),
            until_ns: staged.bundle.until_ns,
            continuity_generation: staged.bundle.continuity_generation,
            sink: staged.bundle.sink.clone(),
            input_closed: AtomicBool::new(false),
            predecessor_epoch: Some(source.selection.identity.epoch.clone()),
            transfer_request: Some(proof.request_id().clone()),
        });
        let mut busy_before_commit = false;
        let promoted = guard.commit_with(|_, _| {
            let at = now()?;
            if !self.current(&source, at)
                || !self.current(&staged.bundle, at)
                || source.input_closed.load(Ordering::Acquire)
                || at >= proof.expires_at_ns()
            {
                return Err(unavailable());
            }
            timing.observations_for(&source.timing, &staged.bundle.timing, at)?;
            match handle.commit_source_transition(&mut pending.transition, |at| {
                let (observation, _) = timing
                    .observations_for(&source.timing, &staged.bundle.timing, at)
                    .map_err(dispatch)?;
                self.facts_from(&source, at, observation, Some(&pin))
                    .map_err(dispatch)
            }) {
                Ok(()) => {}
                Err(HspExecutionError::Busy) => {
                    busy_before_commit = true;
                    return Err(unavailable());
                }
                Err(error) => {
                    return Err(internal(format!(
                        "native source transition commit failed: {error:?}"
                    )))
                }
            }
            // From the first native commit onward every refusal is terminal.
            // No caller may restore the predecessor cursor or remint a token.
            staged.media.commit()?;
            timing.commit(now()?)?;
            if self.retired.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            delivery.envelope = envelope;
            delivery.last = Some(proof.successor_first().clone());
            delivery.last_sample = Some(first);
            delivery.pending_end = None;
            delivery.ended = None;
            self.delivered.store(next_delivered, Ordering::Release);
            let previous = self
                .active
                .compare_and_swap(&Some(source.clone()), Some(next_source));
            if previous
                .as_ref()
                .is_none_or(|prior| !Arc::ptr_eq(prior, &source))
            {
                return Err(unavailable());
            }
            Ok(())
        });
        if let Err(error) = promoted {
            if busy_before_commit && pending.transition.is_some() {
                return Ok(());
            }
            return Err(self.fail(error));
        }
        *pending = ContinuityState::default();
        self.source_custody.store(None);
        // Retirement publishes its cancellation after setting `retired`. If it
        // raced this removal, either this recheck or retirement's own store
        // retains the exact cancelled capability until its actual worker joins.
        if self.retired.load(Ordering::Acquire) && staged.reservation.cancellation_requested() {
            self.source_custody
                .store(Some(Arc::new(staged.reservation.clone())));
        }
        self.retained_transfer.store(None);
        self.staged_source.store(None);
        self.pending_source_work.store(false, Ordering::Release);
        Ok(())
    }
}

pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::EnableHandyHspContinuity { .. } | Command::HandyHspContinuityStatus { .. }
    )
}
fn original_continuity(
    db: &Connection,
    actor: &SessionId,
    project: &ProjectId,
    id: &RequestId,
) -> PResult<HandyHspContinuityStatus> {
    let raw: Option<String> = db.query_row(
        "SELECT CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
        params![actor.as_str(),id.as_str()], |row| row.get(0)).optional().map_err(internal)?;
    let raw = raw.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "original HSP continuity request not found",
        )
    })?;
    let ResponseBody::HandyHspContinuity(value) = decode(&raw)? else {
        return Err(mismatch());
    };
    if value.project_id != *project || value.enable_request_id != *id {
        return Err(mismatch());
    }
    value.validate()?;
    Ok(*value)
}
fn persist_continuity(
    db: &mut Connection,
    actor: &SessionId,
    request: &Request,
    fingerprint: &str,
    status: &HandyHspContinuityStatus,
) -> PResult<()> {
    check_request_identity(db, actor.as_str(), request, fingerprint)?;
    let mut historical = status.clone();
    historical.native = None;
    historical.validate()?;
    let tx = db.transaction().map_err(internal)?;
    let changed = tx
        .execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3 AND fingerprint=?4",
            params![
                encode(&ResponseBody::HandyHspContinuity(Box::new(historical)))?,
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
impl Engine {
    fn continuity_report_locked(
        &self,
        db: &Connection,
        actor: &SessionId,
        project: &ProjectId,
        id: &RequestId,
    ) -> PResult<ResponseBody> {
        let mut status = original_continuity(db, actor, project, id)?;
        let controller = self
            .handy_hsp_live
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), status.stream_request_id.clone()))
            .cloned();
        if let Some(controller) = controller {
            if controller.continuity_enable.get() == Some(id) {
                if let Some(owner) = controller.clock_owner.get() {
                    if status.history.as_ref() != Some(owner.history()) {
                        return Err(mismatch());
                    }
                    status.native = Some(owner.status(now()?));
                }
            }
        }
        status.validate()?;
        Ok(ResponseBody::HandyHspContinuity(Box::new(status)))
    }
    pub(in crate::authority) fn handy_hsp_continuity_control(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        // Original consent time precedes all lock waits and persistence. An
        // exact replay reads history; it never captures a replacement deadline.
        let admitted_at = Instant::now();
        let admitted_at_ns = now()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        let project_id = project(request)?.clone();
        if let Command::HandyHspContinuityStatus {
            original_request_id,
            ..
        } = &request.command
        {
            return self.continuity_report_locked(&db, actor, &project_id, original_request_id);
        }
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
        if replay(&db, actor.as_str(), request, fingerprint)?.is_some() {
            return self.continuity_report_locked(&db, actor, &project_id, &request.request_id);
        }
        check_revision(&db, request)?;
        let original = original(&db, actor, &project_id, stream_request_id)?;
        let controller = self
            .handy_hsp_live
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), stream_request_id.clone()))
            .cloned()
            .ok_or_else(unavailable)?;
        if controller.continuity_enable.get().is_some() || controller.clock_owner.get().is_some() {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "original HSP continuity admission already exists",
            ));
        }
        let active = controller.active_source()?;
        let live_at = now()?;
        if !controller.current(&active, live_at) || active.input_closed.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let execution = self
            .handy_hsp_execution
            .try_lock()
            .map_err(|_| unavailable())?
            .as_ref()
            .ok_or_else(unavailable)?
            .handle();
        let child = execution
            .snapshot_for_session(actor, &project_id, stream_request_id)
            .map_err(|_| unavailable())?;
        if !child.post_setup_bound
            || child.retired
            || !matches!(
                child.runtime_state,
                HspRuntimeState::Prefilling | HspRuntimeState::Playing
            )
            || original
                .origin
                .as_ref()
                .is_none_or(|origin| origin.stream_record_id.as_str() != child.stream_record_id)
        {
            return Err(unavailable());
        }
        let mut status = HandyHspContinuityStatus {
            project_id: project_id.clone(),
            stream_request_id: stream_request_id.clone(),
            enable_request_id: request.request_id.clone(),
            duration_ms: *duration_ms,
            history: None,
            native: None,
            error: None,
            qualified: false,
        };
        status.validate()?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            request,
            fingerprint,
            &ResponseBody::HandyHspContinuity(Box::new(status.clone())),
        )?;
        tx.commit().map_err(internal)?;
        let admitted = self.admit_handy_hsp_continuity_clock_locked(
            &db,
            handy_hsp_preparation::HspClockAdmission {
                request,
                preparation_request_id: &controller.preparation_request,
                binding: &controller.binding,
                admitted_at_ns,
                admitted_at,
            },
        );
        match admitted {
            Ok(owner) => {
                status.history = Some(owner.history().clone());
                if controller.clock_owner.set(owner.clone()).is_err()
                    || controller
                        .continuity_enable
                        .set(request.request_id.clone())
                        .is_err()
                {
                    owner.request_stop();
                    return Err(controller.fail(mismatch()));
                }
            }
            Err(error) => status.error = Some(error),
        }
        if let Err(error) = persist_continuity(&mut db, actor, request, fingerprint, &status) {
            return Err(controller.fail(error));
        }
        self.continuity_report_locked(&db, actor, &project_id, &request.request_id)
    }
}

/// Validate only retained relationships and original arithmetic. This never
/// checks the current boot clock, acceptance or liveness and creates no owner.
pub(super) fn validate_history(db: &Connection) -> Result<()> {
    let mut query = db.prepare("SELECT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(request AS BLOB))<=128 THEN request ELSE NULL END,CASE WHEN length(CAST(fingerprint AS BLOB))<=64 THEN fingerprint ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=262144 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_hsp_continuity' ELSE response LIKE '%handy_hsp_continuity%' END LIMIT 4097")?;
    let mut count = 0;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })? {
        let (actor, id, fingerprint, raw) = row?;
        count += 1;
        anyhow::ensure!(
            count <= 4096,
            "HSP continuity histories exceed startup bound"
        );
        let actor = SessionId::new(actor)?;
        let id = RequestId::new(id)?;
        validate_sha256(&fingerprint)?;
        let ResponseBody::HandyHspContinuity(status) = serde_json::from_str(&raw)? else {
            anyhow::bail!("invalid HSP continuity history kind");
        };
        status.validate()?;
        anyhow::ensure!(
            status.native.is_none() && !status.qualified && status.enable_request_id == id,
            "native HSP continuity ownership cannot be restored from request history"
        );
        let identity: Option<String> = db
            .query_row(
                "SELECT fingerprint FROM request_identities WHERE session=?1 AND request=?2",
                params![actor.as_str(), id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        anyhow::ensure!(
            identity.as_deref() == Some(fingerprint.as_str()),
            "HSP continuity history differs from its original request identity"
        );
        let stream = original(db, &actor, &status.project_id, &status.stream_request_id)?;
        anyhow::ensure!(
            stream.native.is_none() && stream.stream.is_none(),
            "HSP continuity history references serialized native stream ownership"
        );
        let preparation_raw: String = db.query_row(
            "SELECT CASE WHEN length(CAST(response AS BLOB))<=32768 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
            params![actor.as_str(),stream.selection.preparation_request_id.as_str()], |row| row.get(0))?;
        let ResponseBody::HandyHspPreparation(preparation) =
            serde_json::from_str(&preparation_raw)?
        else {
            anyhow::bail!("HSP continuity history references another preparation kind");
        };
        preparation.validate()?;
        anyhow::ensure!(
            preparation.project_id == status.project_id
                && preparation.preparation_request_id == stream.selection.preparation_request_id
                && preparation.native.is_none(),
            "HSP continuity preparation differs from its original actor/project/stream"
        );
        if let Some(history) = &status.history {
            let origin = stream.origin.as_ref().ok_or_else(mismatch)?;
            anyhow::ensure!(
                history.preparation_request_id == stream.selection.preparation_request_id
                    && history.stream_record_id == origin.stream_record_id
                    && history.admitted_at_boottime_ns >= origin.accepted_latest_boottime_ns,
                "HSP continuity admission differs from its original Bind witness"
            );
            // The owner ID binds the original Enable identity/fingerprint, the
            // immutable stream journal ID and the already-validated deadline.
            let expected_owner = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(
                    &id,
                    &fingerprint,
                    history.stream_record_id.as_str().to_owned(),
                    history.deadline_boottime_ns
                ))?)
            );
            anyhow::ensure!(
                history.clock_owner_id.as_str() == expected_owner,
                "HSP continuity owner differs from its original admission"
            );
        }
    }
    Ok(())
}

/// Tests may retain an actual old sink and its actual accepted stamp. No
/// constructor accepts a DTO, synthetic frame or replacement timestamp.
#[cfg(test)]
pub(in crate::authority) struct HspLiveEpochProbe {
    sink: Arc<EpochSink>,
    accepted: NativeAcceptedCapture,
}
#[cfg(test)]
impl HspLiveEpochProbe {
    pub(in crate::authority) fn replay_offer(&self) -> PResult<()> {
        self.sink.offer(self.accepted.clone())
    }
    pub(in crate::authority) fn retire(&self) {
        self.sink.retire();
    }
    /// Existing owner counters only; never evaluates or replaces authority.
    pub(in crate::authority) fn watchdog_observation(&self) -> PResult<(u64, u64, bool)> {
        let controller = self.sink.owner()?;
        let weak = controller.handle.get().ok_or_else(unavailable)?;
        let handle = weak.upgrade().ok_or_else(unavailable)?;
        let (successful, _) = handle.watchdog_progress_for_test();
        Ok((
            successful,
            handle.watchdog_busy_ticks_for_test(),
            weak.is_retired(),
        ))
    }
}
#[cfg(test)]
impl Engine {
    pub(in crate::authority) fn hsp_live_epoch_probe_for_test(
        &self,
        actor: &SessionId,
        stream: &RequestId,
    ) -> PResult<HspLiveEpochProbe> {
        let controller = self
            .handy_hsp_live
            .lock()
            .map_err(internal)?
            .entries
            .get(&(actor.clone(), stream.clone()))
            .cloned()
            .ok_or_else(unavailable)?;
        let delivery = controller.delivery.lock().map_err(internal)?;
        let active = controller.active_source()?;
        let accepted = delivery.last.as_ref().ok_or_else(unavailable)?;
        if accepted.frame.identity != active.selection.identity {
            return Err(mismatch());
        }
        Ok(HspLiveEpochProbe {
            sink: active.sink.clone(),
            accepted: accepted.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn historical_reservation() -> (Connection, HandyHspContinuityStatus) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE requests(session TEXT,request TEXT,fingerprint TEXT,response TEXT,PRIMARY KEY(session,request)); CREATE TABLE request_identities(session TEXT,request TEXT,fingerprint TEXT,PRIMARY KEY(session,request));").unwrap();
        let status = HandyHspContinuityStatus {
            project_id: ProjectId::new("project").unwrap(),
            stream_request_id: RequestId::new("stream").unwrap(),
            enable_request_id: RequestId::new("enable").unwrap(),
            duration_ms: 1_000,
            history: None,
            native: None,
            error: None,
            qualified: false,
        };
        let stream = HandyHspLiveStatus {
            project_id: status.project_id.clone(),
            stream_request_id: status.stream_request_id.clone(),
            selection: HandyHspLiveSelection {
                preparation_request_id: RequestId::new("prepare").unwrap(),
                capture_epoch: AttemptId::new("capture").unwrap(),
                inference_attempt_id: AttemptId::new("inference").unwrap(),
                browser_timing: ArtifactId::new("timing").unwrap(),
                source_qualification: HandyHspSourceQualificationBinding {
                    dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                    revision: 2,
                },
                finite_drain_policy_sha256: None,
            },
            source_subject: None,
            origin: None,
            native: None,
            stream: None,
            error: None,
            qualified: false,
        };
        let preparation = HandyHspPreparationStatus {
            project_id: status.project_id.clone(),
            preparation_request_id: stream.selection.preparation_request_id.clone(),
            history: None,
            native: None,
            armed: false,
            playback_enabled: false,
        };
        for (id, body) in [
            (
                "enable",
                ResponseBody::HandyHspContinuity(Box::new(status.clone())),
            ),
            ("stream", ResponseBody::HandyHspLive(Box::new(stream))),
            (
                "prepare",
                ResponseBody::HandyHspPreparation(Box::new(preparation)),
            ),
        ] {
            db.execute(
                "INSERT INTO requests VALUES('actor',?1,?2,?3)",
                params![id, "f".repeat(64), encode(&body).unwrap()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO request_identities VALUES('actor',?1,?2)",
                params![id, "f".repeat(64)],
            )
            .unwrap();
        }
        (db, status)
    }
    fn replace_history(db: &Connection, status: HandyHspContinuityStatus) {
        db.execute(
            "UPDATE requests SET response=?1 WHERE request='enable'",
            [encode(&ResponseBody::HandyHspContinuity(Box::new(status))).unwrap()],
        )
        .unwrap();
    }
    #[test]
    fn continuity_history_keeps_pending_and_failed_reservations_descriptive() {
        let (db, mut status) = historical_reservation();
        validate_history(&db).unwrap();
        status.error = Some(unavailable());
        replace_history(&db, status);
        validate_history(&db).unwrap();
    }
    #[test]
    fn continuity_history_refuses_changed_original_scope_and_serialized_authority() {
        for case in 0..8 {
            let (db, mut status) = historical_reservation();
            match case {
                0 => status.enable_request_id = RequestId::new("replacement").unwrap(),
                1 => status.project_id = ProjectId::new("foreign-project").unwrap(),
                2 => status.stream_request_id = RequestId::new("foreign-stream").unwrap(),
                3 => status.qualified = true,
                4 => {
                    db.execute(
                        "UPDATE request_identities SET fingerprint=?1 WHERE request='enable'",
                        ["e".repeat(64)],
                    )
                    .unwrap();
                }
                5 => {
                    db.execute(
                        "UPDATE requests SET session='foreign-actor' WHERE request='enable'",
                        [],
                    )
                    .unwrap();
                }
                6 => {
                    db.execute("DELETE FROM requests WHERE request='prepare'", [])
                        .unwrap();
                }
                _ => {
                    status.native = Some(HandyHspContinuityNativeStatus {
                        observed_at_boottime_ns: 1,
                        generation: 1,
                        clock_valid_until_boottime_ns: 2,
                        current: true,
                        retired: false,
                        worker_active: false,
                        worker_joined: true,
                        error: None,
                    })
                }
            }
            replace_history(&db, status);
            assert!(validate_history(&db).is_err(), "case {case}");
        }
    }
    #[test]
    fn continuity_history_cannot_invent_admission_without_original_bind_witness() {
        let (db, mut status) = historical_reservation();
        status.history = Some(HandyHspContinuityHistory {
            preparation_request_id: RequestId::new("prepare").unwrap(),
            stream_record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            clock_owner_id: ArtifactId::new("b".repeat(64)).unwrap(),
            admitted_at_boottime_ns: 100,
            deadline_boottime_ns: 1_000_000_100,
            synchronization_sent_boottime_ns: 10,
            synchronization_ceiling_boottime_ns: 2_000_000_000,
            initial_clock_valid_until_boottime_ns: 1_000,
        });
        status.validate().unwrap();
        replace_history(&db, status);
        assert!(validate_history(&db).is_err());
    }
    #[test]
    fn source_transition_wait_classifies_only_original_unsettled_work() {
        assert!(transition_wait(&HspExecutionError::Busy));
        assert!(transition_wait(&HspExecutionError::Dispatch(
            HspDispatchFailure::Authority(HspRuntimeError::NativeDispatchBusy)
        )));
        assert!(transition_wait(&HspExecutionError::Dispatch(
            HspDispatchFailure::Authority(HspRuntimeError::Core(HspStreamError::NotReady))
        )));
        for error in [
            HspStreamError::SourceTransitionPending,
            HspStreamError::SourceTransitionExpired,
            HspStreamError::ForeignSourceTransition,
        ] {
            assert!(!transition_wait(&HspExecutionError::Dispatch(
                HspDispatchFailure::Authority(HspRuntimeError::Core(error))
            )));
        }
        assert!(!transition_wait(&HspExecutionError::Unavailable));
        assert!(!transition_wait(&HspExecutionError::Dispatch(
            HspDispatchFailure::BindingMismatch
        )));
    }
}
