//! Native shared-frame CPU succession. Public history never reconstructs these
//! reservations, shared pixels, worker join proofs or one-use transfer grants.
use super::*;
use crate::live_frames::{LiveRgbBoundary, NativeSeedToken};

fn refused(message: &'static str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::PrepareBrowserAnalysisSuccessor { .. }
            | Command::TransferBrowserAnalysisInput { .. }
            | Command::BrowserAnalysisTransferStatus { .. }
            | Command::CancelBrowserAnalysisTransfer { .. }
    )
}
struct SuccessorIdentity {
    id: ArtifactId,
    preparation_request: RequestId,
    predecessor: NativeCaptureSelection,
    successor: NativeCaptureSelection,
    expires_at_ns: u64,
    retired: AtomicBool,
    transfer_started: AtomicBool,
    predecessor_cancel: Arc<AtomicBool>,
    successor_cancel: Arc<AtomicBool>,
    cancel_requested: AtomicBool,
    worker_retirement: crate::live_inference::CpuWorkerRetirement,
}
#[derive(Clone)]
pub(in crate::authority) struct NativeCaptureSuccessor(Arc<SuccessorIdentity>);
impl PartialEq for NativeCaptureSuccessor {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for NativeCaptureSuccessor {}
impl std::fmt::Debug for NativeCaptureSuccessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCaptureSuccessor")
            .field("prepare", self.preparation_request_id())
            .field("predecessor", &self.predecessor().identity.epoch)
            .field("successor", &self.successor().identity.epoch)
            .finish_non_exhaustive()
    }
}
impl NativeCaptureSuccessor {
    pub fn predecessor(&self) -> &NativeCaptureSelection {
        &self.0.predecessor
    }
    pub fn successor(&self) -> &NativeCaptureSelection {
        &self.0.successor
    }
    pub fn expires_at_ns(&self) -> u64 {
        self.0.expires_at_ns
    }
    pub fn preparation_request_id(&self) -> &RequestId {
        &self.0.preparation_request
    }
    pub fn transfer_started(&self) -> bool {
        self.0.transfer_started.load(Ordering::Acquire)
    }
    /// Retire only this original pending reservation; registry maintenance owns
    /// cancellation/joins. This latch cannot promote, replace or reopen an epoch.
    pub fn cancel(&self) {
        if self.0.retired.swap(true, Ordering::AcqRel) {
            return;
        }
        self.0.cancel_requested.store(true, Ordering::Release);
        self.0.successor_cancel.store(true, Ordering::Release);
        if self.transfer_started() {
            self.0.predecessor_cancel.store(true, Ordering::Release);
        }
    }
    pub fn cancellation_requested(&self) -> bool {
        self.0.cancel_requested.load(Ordering::Acquire)
    }
    pub fn cancelled_worker_joined(&self) -> bool {
        self.0.cancel_requested.load(Ordering::Acquire) && self.0.worker_retirement.joined()
    }
    pub fn join_cancelled(&self, deadline: Instant) -> PResult<()> {
        if !self.0.cancel_requested.load(Ordering::Acquire) {
            return Err(refused("original successor cancellation not requested"));
        }
        self.0.worker_retirement.wait(deadline)
    }
}
/// Original and explicitly mapped successor descriptions of ONE pixel event.
#[derive(Clone, Debug)]
pub(in crate::authority) struct NativeBoundaryObservation {
    pub predecessor_frame: LiveFrame,
    pub successor_seed: LiveFrame,
    pub predecessor_capture: BootBounds,
    pub successor_capture: BootBounds,
}
struct TransferProof {
    reservation: NativeCaptureSuccessor,
    request: RequestId,
    predecessor_final: NativeAcceptedCapture,
    successor_first: NativeAcceptedCapture,
    boundary: NativeBoundaryObservation,
    reserved_at_ns: u64,
}
#[derive(Clone)]
pub(in crate::authority) struct NativeCaptureTransfer(Arc<TransferProof>);
impl PartialEq for NativeCaptureTransfer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for NativeCaptureTransfer {}
impl std::fmt::Debug for NativeCaptureTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCaptureTransfer")
            .field("request", self.request_id())
            .field("preparation_request", self.preparation_request_id())
            .field("reserved_at_ns", &self.reserved_at_ns())
            .field("predecessor", &self.predecessor().identity.epoch)
            .field("successor", &self.successor().identity.epoch)
            .finish_non_exhaustive()
    }
}
impl NativeCaptureTransfer {
    pub fn request_id(&self) -> &RequestId {
        &self.0.request
    }
    pub fn predecessor(&self) -> &NativeCaptureSelection {
        self.0.reservation.predecessor()
    }
    pub fn successor(&self) -> &NativeCaptureSelection {
        self.0.reservation.successor()
    }
    pub fn predecessor_final(&self) -> &NativeAcceptedCapture {
        &self.0.predecessor_final
    }
    pub fn successor_first(&self) -> &NativeAcceptedCapture {
        &self.0.successor_first
    }
    pub fn boundary(&self) -> &NativeBoundaryObservation {
        &self.0.boundary
    }
    pub fn reserved_at_ns(&self) -> u64 {
        self.0.reserved_at_ns
    }
    pub fn expires_at_ns(&self) -> u64 {
        self.0.reservation.expires_at_ns()
    }
    pub fn preparation_request_id(&self) -> &RequestId {
        self.0.reservation.preparation_request_id()
    }
    pub fn successor_reservation(&self) -> &NativeCaptureSuccessor {
        &self.0.reservation
    }
}
pub(super) struct TransferWork {
    report: BrowserAnalysisTransferReport,
    boundary: Option<LiveRgbBoundary>,
    seed: Option<NativeSeedToken>,
    observation: Option<NativeBoundaryObservation>,
    reserved_at_ns: u64,
    successor_pair: Option<NativeAcceptedCapture>,
    proof: Option<NativeCaptureTransfer>,
    notified: bool,
}
pub(super) struct SuccessorEntry {
    native: NativeCaptureSuccessor,
    phase: BrowserAnalysisSuccessorPhase,
    transfer: Option<TransferWork>,
}
/// Retry custody is limited to an observed local mutex WouldBlock. No generic
/// protocol error or native clock/authority refusal becomes retry permission.
#[derive(Debug)]
pub(in crate::authority) enum NativeCaptureTransferFailure {
    Contended,
    Refused(ProtocolError),
}
impl From<ProtocolError> for NativeCaptureTransferFailure {
    fn from(error: ProtocolError) -> Self {
        Self::Refused(error)
    }
}
impl From<super::super::browser_timing::BrowserTimingClaimFailure>
    for NativeCaptureTransferFailure
{
    fn from(error: super::super::browser_timing::BrowserTimingClaimFailure) -> Self {
        match error {
            super::super::browser_timing::BrowserTimingClaimFailure::Contended => Self::Contended,
            super::super::browser_timing::BrowserTimingClaimFailure::Refused(error) => {
                Self::Refused(error)
            }
        }
    }
}
impl NativeCaptureTransferFailure {
    pub(super) fn into_protocol(self) -> ProtocolError {
        match self {
            Self::Contended => ProtocolError::new(
                ErrorCode::Unavailable,
                "native transfer observation is contended",
            ),
            Self::Refused(error) => error,
        }
    }
}
pub(in crate::authority) struct NativeCaptureTransferGuard<'a> {
    registry: &'a mut Registry,
    proof: NativeCaptureTransfer,
    predecessor: NativeCaptureSourceSnapshot,
    successor: NativeCaptureSourceSnapshot,
}
impl NativeCaptureTransferGuard<'_> {
    pub fn predecessor(&self) -> &NativeCaptureSourceSnapshot {
        &self.predecessor
    }
    pub fn successor(&self) -> &NativeCaptureSourceSnapshot {
        &self.successor
    }
    /// The closure performs only the bounded native cursor/point transition,
    /// using supplied snapshots instead of reacquiring DB/capture/timing guards.
    /// After its success producer promotion is infallible under this same guard.
    pub fn commit_with<T>(
        self,
        promote: impl FnOnce(&NativeCaptureSourceSnapshot, &NativeCaptureSourceSnapshot) -> PResult<T>,
    ) -> PResult<T> {
        let original = &self.proof.0.reservation.0;
        if original.retired.load(Ordering::Acquire)
            || original.cancel_requested.load(Ordering::Acquire)
            || original.predecessor_cancel.load(Ordering::Acquire)
            || original.successor_cancel.load(Ordering::Acquire)
        {
            return Err(refused("original transfer retired before native commit"));
        }
        let result = promote(&self.predecessor, &self.successor)?;
        let id = self.proof.0.reservation.0.id.clone();
        let a = self.proof.predecessor().identity.epoch.to_string();
        let b = self.proof.successor().identity.epoch.to_string();
        let successor = self
            .registry
            .entries
            .get_mut(&b)
            .expect("held transfer successor");
        successor.native_successor = None;
        let predecessor = self
            .registry
            .entries
            .get_mut(&a)
            .expect("held joined predecessor");
        // The actual CPU worker and every original callback are joined. No
        // outstanding pixel token remains to retire, so dropping the runtime
        // cannot strand or detach work and introduces no fallible post-commit IO.
        predecessor.cancel.store(true, Ordering::Release);
        predecessor.retire_native_delivery();
        predecessor.runtime = None;
        predecessor.browser_media = None;
        predecessor.browser_analysis_pin = None;
        predecessor.native_accepted = None;
        predecessor.report.active = false;
        predecessor.report.retirement = Some(LiveCaptureRetirement::Superseded);
        predecessor.report.admitted_pixel_bytes = 0;
        predecessor.report.queue = None;
        if let Some(info) = &mut predecessor.report.inference {
            info.state = LiveCaptureInferenceState::Retired;
            info.latest = None;
            info.error = None;
        }
        let slot = self
            .registry
            .successors
            .get_mut(&id)
            .expect("held transfer reservation");
        slot.phase = BrowserAnalysisSuccessorPhase::Active;
        slot.transfer
            .as_mut()
            .expect("held original transfer")
            .report
            .phase = BrowserAnalysisTransferPhase::Transferred;
        slot.native.0.retired.store(true, Ordering::Release);
        let transfer = slot.transfer.as_mut().expect("held original transfer");
        transfer.boundary = None;
        transfer.seed = None;
        Ok(result)
    }
}
fn selection(entry: &Entry) -> PResult<NativeCaptureSelection> {
    let info = entry
        .report
        .inference
        .as_ref()
        .ok_or_else(|| refused("successor requires CPU inference"))?;
    Ok(NativeCaptureSelection {
        owner: entry.owner.clone(),
        identity: entry.report.identity.clone(),
        attempt_id: info.attempt_id.clone(),
        profile_sha256: info.profile_sha256.clone(),
        cpu_manifest_sha256: cpu_manifest_hash(info)?,
        browser_media: entry
            .report
            .options
            .browser_media
            .clone()
            .ok_or_else(|| refused("successor requires browser media"))?,
    })
}
pub(super) fn check_ingress_state(entry: &Entry) -> PResult<()> {
    if entry.native_transfer.is_some()
        || (entry.native_successor.is_some() && entry.report.received_frames != 1)
    {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "source transfer ingress is not open",
        ));
    }
    Ok(())
}
pub(super) fn check_ingress(entry: &Entry, header: &LiveFrameHeader) -> PResult<()> {
    check_ingress_state(entry)?;
    if entry.native_successor.is_some() && header.frame != FrameId::new(1) {
        return Err(refused("successor requires its first actual pair frame"));
    }
    Ok(())
}
impl Registry {
    pub(super) fn transfer_joined_current(&self, entry: &Entry) -> bool {
        entry.native_transfer_joined
            && entry
                .native_transfer
                .as_ref()
                .and_then(|id| self.successors.get(id))
                .is_some_and(|slot| {
                    slot.phase == BrowserAnalysisSuccessorPhase::Transferring
                        && !slot.native.0.retired.load(Ordering::Acquire)
                        && slot.native.predecessor().identity == entry.report.identity
                        && slot.native.predecessor().owner == entry.owner
                        && entry.report.inference.as_ref().is_some_and(|info| {
                            info.attempt_id == slot.native.predecessor().attempt_id
                                && info.profile_sha256 == slot.native.predecessor().profile_sha256
                        })
                        && slot.transfer.as_ref().is_some_and(|transfer| {
                            transfer.report.predecessor == entry.report.identity
                                && transfer.report.predecessor_worker_joined
                                && transfer.report.predecessor_delivery_settled
                                && !matches!(
                                    transfer.report.phase,
                                    BrowserAnalysisTransferPhase::Transferred
                                        | BrowserAnalysisTransferPhase::Aborted
                                )
                        })
                })
    }
    fn check_successor(&self, native: &NativeCaptureSuccessor) -> PResult<()> {
        let slot = self
            .successors
            .get(&native.0.id)
            .filter(|s| Arc::ptr_eq(&s.native.0, &native.0))
            .ok_or_else(|| refused("foreign successor reservation"))?;
        if native.0.retired.load(Ordering::Acquire)
            || matches!(
                slot.phase,
                BrowserAnalysisSuccessorPhase::Aborted | BrowserAnalysisSuccessorPhase::Active
            )
        {
            return Err(refused(
                "original successor reservation retired or consumed",
            ));
        }
        for (predecessor, selected) in [(true, native.predecessor()), (false, native.successor())] {
            let source_refusal = |prior: &'static str, next: &'static str| {
                refused(if predecessor { prior } else { next })
            };
            let entry = self
                .entries
                .get(selected.identity.epoch.as_str())
                .ok_or_else(|| refused("transfer epoch absent"))?;
            if !entry.report.active || entry.cancel.load(Ordering::Acquire) {
                return Err(source_refusal(
                    "original transfer predecessor capture retired",
                    "original transfer successor capture retired",
                ));
            }
            if entry
                .native_delivery
                .as_ref()
                .is_some_and(|sink| !sink.current())
            {
                return Err(source_refusal(
                    "original transfer predecessor delivery retired",
                    "original transfer successor delivery retired",
                ));
            }
            if selection(entry)? != *selected {
                return Err(source_refusal(
                    "original transfer predecessor selection changed",
                    "original transfer successor selection changed",
                ));
            }
            if tick(entry).is_err() {
                return Err(source_refusal(
                    "original transfer predecessor native clock refused",
                    "original transfer successor native clock refused",
                ));
            }
            if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                return Err(source_refusal(
                    "original transfer predecessor capture lease expired",
                    "original transfer successor capture lease expired",
                ));
            }
            if entry.native_clock.latest_native()?.latest_ns >= native.expires_at_ns() {
                return Err(refused("original transfer reservation deadline expired"));
            }
            if entry
                .browser_media
                .as_ref()
                .is_none_or(|pin| !pin.current())
            {
                return Err(source_refusal(
                    "original transfer predecessor media authority retired or expired",
                    "original transfer successor media authority retired or expired",
                ));
            }
            if entry.browser_analysis_pin.as_ref().is_none_or(|pin| {
                entry
                    .native_clock
                    .latest_native()
                    .map_or(true, |at| !pin.current(at.latest_ns))
            }) {
                return Err(source_refusal(
                    "original transfer predecessor timing authority retired or expired",
                    "original transfer successor timing authority retired or expired",
                ));
            }
        }
        Ok(())
    }
    pub(in crate::authority) fn native_successor_basis(
        &self,
        native: &NativeCaptureSuccessor,
    ) -> PResult<NativeCaptureSourceSnapshot> {
        self.check_successor(native)?;
        self.native_source_basis_scoped(native.successor(), false, true, None, false)
    }
    pub(in crate::authority) fn try_native_successor_basis(
        &self,
        native: &NativeCaptureSuccessor,
    ) -> PResult<Option<NativeCaptureSourceSnapshot>> {
        self.check_successor(native)?;
        let entry = self
            .entries
            .get(native.successor().identity.epoch.as_str())
            .expect("checked successor");
        if entry
            .report
            .inference
            .as_ref()
            .is_some_and(|i| i.state == LiveCaptureInferenceState::Starting)
        {
            return Ok(None);
        }
        self.native_successor_basis(native).map(Some)
    }
    pub(in crate::authority) fn register_native_successor_delivery(
        &mut self,
        native: &NativeCaptureSuccessor,
        sink: Arc<dyn NativeCaptureSink>,
    ) -> PResult<NativeCaptureSubscription> {
        self.check_successor(native)?;
        let entry = self
            .entries
            .get(native.successor().identity.epoch.as_str())
            .expect("checked successor");
        if entry
            .report
            .inference
            .as_ref()
            .is_none_or(|i| i.completed != 0)
        {
            return Err(refused(
                "successor sink must precede original boundary work",
            ));
        }
        self.register_native_delivery_scoped(native.successor().clone(), sink, true)
    }
    #[cfg(test)]
    pub(in crate::authority) fn claim_native_transfer(
        &mut self,
        proof: &NativeCaptureTransfer,
    ) -> PResult<NativeCaptureTransferGuard<'_>> {
        self.try_claim_native_transfer(proof)
            .map_err(NativeCaptureTransferFailure::into_protocol)
    }
    pub(in crate::authority) fn try_claim_native_transfer(
        &mut self,
        proof: &NativeCaptureTransfer,
    ) -> Result<NativeCaptureTransferGuard<'_>, NativeCaptureTransferFailure> {
        self.check_successor(&proof.0.reservation)?;
        let slot = self
            .successors
            .get(&proof.0.reservation.0.id)
            .expect("checked reservation");
        let transfer = slot
            .transfer
            .as_ref()
            .filter(|t| {
                t.proof
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(&p.0, &proof.0))
                    && t.report.phase == BrowserAnalysisTransferPhase::ReadyForTransfer
            })
            .ok_or_else(|| refused("transfer proof is absent or consumed"))?;
        if !transfer.report.predecessor_worker_joined
            || !transfer.report.predecessor_delivery_settled
            || !transfer.report.successor_pair_accepted
        {
            return Err(refused("transfer custody is not settled").into());
        }
        let predecessor =
            self.try_native_source_basis_scoped(proof.predecessor(), true, false, None, false)?;
        let successor =
            self.try_native_source_basis_scoped(proof.successor(), true, true, None, false)?;
        Ok(NativeCaptureTransferGuard {
            registry: self,
            proof: proof.clone(),
            predecessor,
            successor,
        })
    }
    pub(super) fn sweep_successors(&mut self) -> PResult<()> {
        let expired: Vec<_> = self
            .successors
            .values()
            .filter(|s| {
                !matches!(
                    s.phase,
                    BrowserAnalysisSuccessorPhase::Active | BrowserAnalysisSuccessorPhase::Aborted
                )
            })
            .filter_map(|s| {
                self.check_successor(&s.native)
                    .err()
                    .map(|error| (s.native.0.id.clone(), error))
            })
            .collect();
        for (id, error) in expired {
            self.abort_successor(&id, error)?;
        }
        Ok(())
    }
    fn abort_successor(&mut self, id: &ArtifactId, error: ProtocolError) -> PResult<()> {
        let slot = self
            .successors
            .get_mut(id)
            .ok_or_else(|| refused("successor absent"))?;
        if matches!(
            slot.phase,
            BrowserAnalysisSuccessorPhase::Active | BrowserAnalysisSuccessorPhase::Aborted
        ) {
            return Ok(());
        }
        slot.phase = BrowserAnalysisSuccessorPhase::Aborted;
        slot.native.0.retired.store(true, Ordering::Release);
        slot.native
            .0
            .cancel_requested
            .store(true, Ordering::Release);
        let a = slot.native.predecessor().identity.epoch.to_string();
        let b = slot.native.successor().identity.epoch.to_string();
        let frozen = slot.transfer.is_some();
        if let Some(transfer) = &mut slot.transfer {
            transfer.report.phase = BrowserAnalysisTransferPhase::Aborted;
            transfer.report.error = Some(error);
            transfer.boundary = None;
            transfer.seed = None;
        }
        if let Some(entry) = self.entries.get_mut(&b) {
            retire(entry, LiveCaptureRetirement::AuthorityLost)?;
        }
        if frozen {
            if let Some(entry) = self.entries.get_mut(&a) {
                retire(entry, LiveCaptureRetirement::AuthorityLost)?;
            }
        }
        Ok(())
    }
    fn successor_report(&self, id: &ArtifactId) -> PResult<BrowserAnalysisSuccessorReport> {
        let slot = self
            .successors
            .get(id)
            .ok_or_else(|| refused("successor absent"))?;
        let entry = self
            .entries
            .get(slot.native.successor().identity.epoch.as_str())
            .ok_or_else(|| refused("successor epoch absent"))?;
        let attached = self
            .entries
            .get(slot.native.predecessor().identity.epoch.as_str())
            .is_some_and(|a| a.native_delivery.is_some());
        let phase = if slot.phase == BrowserAnalysisSuccessorPhase::Preparing
            && entry
                .report
                .inference
                .as_ref()
                .is_some_and(|i| i.state == LiveCaptureInferenceState::Running)
            && (!attached
                || entry
                    .native_delivery
                    .as_ref()
                    .is_some_and(|sink| sink.current()))
        {
            BrowserAnalysisSuccessorPhase::Prepared
        } else {
            slot.phase
        };
        let value = BrowserAnalysisSuccessorReport {
            original_request_id: slot.native.preparation_request_id().clone(),
            predecessor: slot.native.predecessor().identity.clone(),
            successor: report(entry)?,
            phase,
            transfer_request_id: slot
                .transfer
                .as_ref()
                .map(|t| t.report.original_request_id.clone()),
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
impl Registry {
    /// Authored CPU state only. This launches no process and does not construct
    /// an actual worker-join receipt; native campaigns cover actual custody.
    pub(in crate::authority) fn authored_successor_for_test(
        &mut self,
        engine: &Engine,
        predecessor: &AttemptId,
    ) -> PResult<NativeCaptureSuccessor> {
        self.authored_successor_sources_for_test(engine, predecessor, None)
    }
    pub(in crate::authority) fn authored_successor_sources_for_test(
        &mut self,
        engine: &Engine,
        predecessor: &AttemptId,
        sources: Option<(
            super::super::browser_media::SourceMediaPin,
            super::super::browser_timing::BrowserTimingPin,
        )>,
    ) -> PResult<NativeCaptureSuccessor> {
        let prior = self
            .entries
            .get(predecessor.as_str())
            .ok_or_else(|| refused("test predecessor absent"))?;
        let selected = selection(prior)?;
        tick(prior)?;
        let a_until = prior
            .native_clock
            .lease_deadline(u64::from(prior.report.options.lease_ms) * 1_000_000)?;
        let created = Instant::now();
        let native_clock = clock::NativeCaptureClock::new(created);
        let now = native_clock.tick(created)?;
        let identity = LiveIdentity {
            epoch: id!(AttemptId),
            ..prior.report.identity.clone()
        };
        let mut options = prior.report.options.clone();
        options.continuation_mode = LiveContinuationMode::Immediate;
        options
            .browser_analysis
            .as_mut()
            .expect("browser test")
            .feed_id = id!(ArtifactId);
        if let Some((media, timing)) = &sources {
            options.browser_media = Some(media.binding().clone());
            options
                .browser_analysis
                .as_mut()
                .expect("browser test")
                .timing_binding_id = timing.binding().binding_id.clone();
        }
        let clock = options.clock(identity.epoch.clone());
        let mut info = prior
            .report
            .inference
            .as_ref()
            .expect("CPU fixture")
            .clone();
        info.attempt_id = id!(AttemptId);
        info.state = LiveCaptureInferenceState::Running;
        info.completed = 0;
        info.accepted = 0;
        info.stale = 0;
        info.latest = None;
        info.error = None;
        let mut report = prior.report.clone();
        report.identity = identity.clone();
        report.clock = clock.clone();
        report.options = options;
        report.continuation = None;
        report.input_close = None;
        report.server_elapsed = Some(now);
        report.received_frames = 0;
        report.last_frame = None;
        report.queue = None;
        report.inference = Some(info.clone());
        let runtime = LiveRgbRuntime::new(
            &engine.pool,
            identity.clone(),
            report.options.policy,
            Some(clock),
            now,
        )?;
        let b_cancel = Arc::new(AtomicBool::new(false));
        let (worker_retirement, unstarted) = crate::live_inference::CpuWorkerRetirement::new();
        drop(unstarted); // The authored fixture owns no CPU launch or worker.
        let native = NativeCaptureSuccessor(Arc::new(SuccessorIdentity {
            id: id!(ArtifactId),
            preparation_request: id!(RequestId),
            predecessor: selected,
            successor: NativeCaptureSelection {
                cpu_manifest_sha256: cpu_manifest_hash(&info)?,
                owner: prior.owner.clone(),
                identity: identity.clone(),
                attempt_id: info.attempt_id,
                profile_sha256: info.profile_sha256,
                browser_media: report.options.browser_media.clone().unwrap(),
            },
            expires_at_ns: a_until
                .min(native_clock.lease_deadline(u64::from(report.options.lease_ms) * 1_000_000)?),
            retired: AtomicBool::new(false),
            transfer_started: AtomicBool::new(false),
            predecessor_cancel: prior.cancel.clone(),
            successor_cancel: b_cancel.clone(),
            cancel_requested: AtomicBool::new(false),
            worker_retirement,
        }));
        report.validate()?;
        let entry = Entry {
            owner: prior.owner.clone(),
            created,
            native_clock,
            native_accepted: None,
            native_delivery: None,
            native_input_close: None,
            native_successor: Some(native.0.id.clone()),
            native_transfer: None,
            native_transfer_joined: false,
            report,
            runtime: Some(runtime),
            cancel: b_cancel,
            origin: None,
            recording: None,
            browser_media: sources
                .as_ref()
                .map(|(media, _)| media.clone())
                .or_else(|| prior.browser_media.clone()),
            browser_analysis_pin: sources
                .as_ref()
                .map(|(_, timing)| timing.clone())
                .or_else(|| prior.browser_analysis_pin.clone()),
        };
        self.entries.insert(identity.epoch.to_string(), entry);
        self.successors.insert(
            native.0.id.clone(),
            SuccessorEntry {
                native: native.clone(),
                phase: BrowserAnalysisSuccessorPhase::Preparing,
                transfer: None,
            },
        );
        Ok(native)
    }
}

fn same_frame(header: &LiveFrameHeader, stamp: &NativeAcceptedCapture) -> bool {
    header.frame == stamp.frame.frame
        && header.source_time == stamp.frame.source_time
        && header.capture_tick == stamp.frame.capture_tick
        && header.sha256 == stamp.frame.artifact.as_str()
}
fn delivery_settled(entry: &Entry, expected: &LiveFrame) -> PResult<bool> {
    let Some(delivery) = &entry.native_delivery else {
        return Ok(true);
    };
    if !delivery.current() {
        return Err(refused("original transfer consumer retired"));
    }
    if delivery.pending.load(Ordering::Acquire) != 0 || delivery.delivering.load(Ordering::Acquire)
    {
        return Ok(false);
    }
    match delivery.last_delivered.try_lock() {
        Ok(last) => Ok(last.as_ref() == Some(expected)),
        Err(std::sync::TryLockError::WouldBlock) => Ok(false),
        Err(std::sync::TryLockError::Poisoned(_)) => {
            Err(refused("transfer delivery observation poisoned"))
        }
    }
}
impl Registry {
    pub(super) fn successor_seed(&self, epoch: &AttemptId) -> Option<NativeSeedToken> {
        let id = self
            .entries
            .get(epoch.as_str())?
            .native_successor
            .as_ref()?;
        self.successors.get(id)?.transfer.as_ref()?.seed.clone()
    }
    pub(super) fn observe_successor_acceptance(
        &mut self,
        epoch: &AttemptId,
        original_seed: bool,
        stamp: Option<NativeAcceptedCapture>,
    ) -> PResult<()> {
        let id = self
            .entries
            .get(epoch.as_str())
            .and_then(|e| e.native_successor.clone())
            .ok_or_else(|| refused("successor acceptance lacks reservation"))?;
        let slot = self
            .successors
            .get_mut(&id)
            .ok_or_else(|| refused("successor native reservation absent"))?;
        let transfer = slot
            .transfer
            .as_mut()
            .ok_or_else(|| refused("successor acceptance preceded boundary"))?;
        let valid = stamp.as_ref().is_some_and(|s| {
            let (LiveInferenceReply::Estimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                ..
            }
            | LiveInferenceReply::CapturedCpuEstimated {
                attempt_id,
                identity,
                profile_sha256,
                input,
                ..
            }) = &s.estimate.result
            else {
                return false;
            };
            slot.native.successor().matches_result(&s.estimate.result)
                && original_seed
                && transfer.seed.as_ref().is_some_and(|seed| {
                    seed.frame() == &s.previous_frame
                        && transfer
                            .boundary
                            .as_ref()
                            .is_some_and(|boundary| seed.belongs_to(boundary))
                })
                && input.sequence == 1
                && !s.continuous_from_previous
                && *identity == slot.native.successor().identity
                && *attempt_id == slot.native.successor().attempt_id
                && *profile_sha256 == slot.native.successor().profile_sha256
                && s.continuity_generation == 0
                && transfer.successor_pair.is_none()
        });
        if !valid {
            return self.abort_successor(
                &id,
                refused("successor first accepted pair does not use original native boundary"),
            );
        }
        transfer.successor_pair = stamp;
        transfer.report.successor_pair_accepted = true;
        Ok(())
    }
    pub(super) fn cpu_transfer_step(
        &mut self,
        epoch: &AttemptId,
        attempt: &AttemptId,
    ) -> PResult<inference::CpuStep> {
        let entry = self
            .entries
            .get(epoch.as_str())
            .ok_or_else(|| refused("transfer predecessor absent"))?;
        let id = entry
            .native_transfer
            .clone()
            .ok_or_else(|| refused("transfer join purpose absent"))?;
        let slot = self
            .successors
            .get(&id)
            .ok_or_else(|| refused("native transfer reservation absent"))?;
        self.check_successor(&slot.native)?;
        let transfer = slot
            .transfer
            .as_ref()
            .ok_or_else(|| refused("native transfer boundary absent"))?;
        let info = entry
            .report
            .inference
            .as_ref()
            .ok_or_else(|| refused("transfer inference absent"))?;
        if &info.attempt_id != attempt
            || !entry
                .runtime
                .as_ref()
                .expect("active transfer")
                .input_closed()
        {
            return Err(refused("transfer join differs from original input owner"));
        }
        let Some(disposition) = entry.runtime.as_ref().expect("active").input_disposition() else {
            return Ok(inference::CpuStep::Wait);
        };
        if disposition != LiveCaptureInputFinalDisposition::Accepted
            || entry
                .native_accepted
                .as_ref()
                .is_none_or(|s| !same_frame(&transfer.report.boundary, s))
        {
            self.abort_successor(
                &id,
                refused("predecessor boundary is not actually accepted"),
            )?;
            return Ok(inference::CpuStep::Retired);
        }
        let stamp = entry.native_accepted.as_ref().expect("checked final");
        if !delivery_settled(entry, &stamp.frame)? {
            return Ok(inference::CpuStep::Wait);
        }
        let ticket = inference::CpuJoinTicket {
            identity: entry.report.identity.clone(),
            attempt: attempt.clone(),
            close_request: transfer.report.original_request_id.clone(),
            completed: info.completed,
            transfer: Some(id.clone()),
        };
        let transfer = self
            .successors
            .get_mut(&id)
            .expect("slot")
            .transfer
            .as_mut()
            .expect("transfer");
        transfer.report.predecessor_delivery_settled = true;
        transfer.report.phase = BrowserAnalysisTransferPhase::JoiningPredecessor;
        Ok(inference::CpuStep::Join(ticket))
    }
    fn ready_transfer_notifications(
        &mut self,
    ) -> PResult<Vec<(NativeCaptureTransfer, Option<Arc<NativeDelivery>>)>> {
        let ids: Vec<_> = self
            .successors
            .values()
            .filter(|s| {
                s.phase == BrowserAnalysisSuccessorPhase::Transferring
                    && s.transfer.as_ref().is_some_and(|t| {
                        t.report.predecessor_worker_joined
                            && t.successor_pair.is_some()
                            && (!t.notified
                                || self
                                    .entries
                                    .get(s.native.predecessor().identity.epoch.as_str())
                                    .is_some_and(|a| a.native_delivery.is_none()))
                    })
            })
            .map(|s| s.native.0.id.clone())
            .collect();
        let mut notifications = Vec::new();
        for id in ids {
            let slot = self.successors.get(&id).expect("selected slot");
            if let Err(error) = self.check_successor(&slot.native) {
                self.abort_successor(&id, error)?;
                continue;
            }
            let transfer = slot.transfer.as_ref().expect("transfer");
            let a = self
                .entries
                .get(slot.native.predecessor().identity.epoch.as_str())
                .expect("checked A");
            let b = self
                .entries
                .get(slot.native.successor().identity.epoch.as_str())
                .expect("checked B");
            let final_a = a
                .native_accepted
                .clone()
                .ok_or_else(|| refused("joined predecessor lost original acceptance"))?;
            let first_b = transfer
                .successor_pair
                .as_ref()
                .expect("selected acceptance");
            if !same_frame(&transfer.report.boundary, &final_a) || !a.native_transfer_joined {
                return Err(refused("transfer lost joined predecessor boundary"));
            }
            if !delivery_settled(a, &final_a.frame)? || !delivery_settled(b, &first_b.frame)? {
                continue;
            }
            let proof = transfer.proof.clone().unwrap_or_else(|| {
                NativeCaptureTransfer(Arc::new(TransferProof {
                    reservation: slot.native.clone(),
                    request: transfer.report.original_request_id.clone(),
                    predecessor_final: final_a,
                    successor_first: first_b.clone(),
                    boundary: transfer
                        .observation
                        .clone()
                        .expect("accepted shared seed mapping"),
                    reserved_at_ns: transfer.reserved_at_ns,
                }))
            });
            let sink = a.native_delivery.clone();
            let transfer = self
                .successors
                .get_mut(&id)
                .expect("slot")
                .transfer
                .as_mut()
                .expect("transfer");
            transfer.report.phase = BrowserAnalysisTransferPhase::ReadyForTransfer;
            transfer.proof = Some(proof.clone());
            transfer.notified = true;
            notifications.push((proof, sink));
        }
        Ok(notifications)
    }
}
impl Engine {
    pub(super) fn live_cpu_transfer_joined(
        &self,
        ticket: inference::CpuJoinTicket,
        joined: crate::live_inference::CpuInputJoined,
    ) -> PResult<()> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let id = ticket
            .transfer
            .as_ref()
            .ok_or_else(|| refused("transfer join ticket purpose absent"))?;
        let slot = registry
            .successors
            .get(id)
            .ok_or_else(|| refused("original transfer absent"))?;
        let transfer = slot
            .transfer
            .as_ref()
            .ok_or_else(|| refused("transfer boundary absent"))?;
        let entry = registry
            .entries
            .get(ticket.identity.epoch.as_str())
            .ok_or_else(|| refused("transfer predecessor absent"))?;
        let info = entry
            .report
            .inference
            .as_ref()
            .ok_or_else(|| refused("transfer predecessor inference absent"))?;
        if entry.report.identity != ticket.identity
            || info.attempt_id != ticket.attempt
            || info.completed != ticket.completed
            || transfer.report.original_request_id != ticket.close_request
            || !transfer.report.predecessor_delivery_settled
            || !inference::cpu_join_matches(&joined, info, &ticket)?
        {
            return Err(refused(
                "actual CPU join differs from original transfer custody",
            ));
        }
        if slot.phase == BrowserAnalysisSuccessorPhase::Aborted {
            return Ok(());
        }
        registry.check_successor(&slot.native)?;
        let entry = registry
            .entries
            .get_mut(ticket.identity.epoch.as_str())
            .expect("checked predecessor");
        entry.native_transfer_joined = true;
        entry.report.inference.as_mut().expect("inference").state =
            LiveCaptureInferenceState::TransferJoined;
        let transfer = registry
            .successors
            .get_mut(id)
            .expect("slot")
            .transfer
            .as_mut()
            .expect("transfer");
        transfer.report.predecessor_worker_joined = true;
        transfer.report.phase = BrowserAnalysisTransferPhase::AwaitingSuccessorPair;
        drop(registry);
        drop(db);
        self.advance_browser_analysis_transfers()
    }
    fn commit_preview_transfer(
        &self,
        guard: NativeCaptureTransferGuard<'_>,
    ) -> Result<(), NativeCaptureTransferFailure> {
        let old_timing = guard.predecessor().browser_timing.clone();
        let new_timing = guard.successor().browser_timing.clone();
        let media = super::super::browser_media::NativeBrowserMediaSuccessor::from_pins(
            guard.predecessor().source_media.clone(),
            guard.successor().source_media.clone(),
        )?;
        let mut timing = old_timing
            .try_claim_successor_native(&new_timing, &media, || self.browser_timing_now())?;
        guard
            .commit_with(|old, new| {
                timing.observations_for(
                    &old.browser_timing,
                    &new.browser_timing,
                    self.browser_timing_now()?,
                )?;
                // Browser-only preview owns the same native source/timing cutover
                // as a device consumer, without granting any device authority.
                media.commit()?;
                timing.commit(self.browser_timing_now()?)
            })
            .map_err(Into::into)
    }
    pub(super) fn advance_browser_analysis_transfers(&self) -> PResult<()> {
        let notifications = {
            let db = self.db.lock().map_err(internal)?;
            let mut registry = self.live_captures.lock().map_err(internal)?;
            registry.sweep(&db)?;
            registry.ready_transfer_notifications()?
        };
        for (proof, delivery) in notifications {
            let result = if let Some(delivery) = delivery {
                if !delivery.current() {
                    Err(refused("original transfer consumer retired"))
                } else {
                    delivery
                        .sink
                        .upgrade()
                        .ok_or_else(|| refused("original transfer consumer absent"))
                        .and_then(|sink| {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                sink.transfer_input(proof.clone())
                            }))
                            .unwrap_or_else(|_| Err(refused("native transfer consumer failed")))
                        })
                }
            } else {
                // Analysis preview has no HSP consumer. It still consumes the
                // very same native joined/accepted boundary guard, never Ready.
                let db = self.db.lock().map_err(internal)?;
                let mut registry = self.live_captures.lock().map_err(internal)?;
                registry.sweep(&db)?;
                // Only actual local contention preserves retry custody. Native
                // refusal remains terminal; neither path repeats input or work.
                let outcome = match registry
                    .try_claim_native_transfer(&proof)
                    .and_then(|guard| self.commit_preview_transfer(guard))
                {
                    Ok(()) | Err(NativeCaptureTransferFailure::Contended) => Ok(()),
                    Err(NativeCaptureTransferFailure::Refused(error)) => Err(error),
                };
                outcome
            };
            if let Err(error) = result {
                self.live_captures
                    .lock()
                    .map_err(internal)?
                    .abort_successor(&proof.0.reservation.0.id, error)?;
            }
        }
        Ok(())
    }
}

impl Engine {
    pub(super) fn browser_analysis_successor_command(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        if let Some(saved) = replay(&db, actor.as_str(), request, fingerprint)? {
            return match saved {
                ResponseBody::BrowserAnalysisSuccessor(mut old) => {
                    if old.original_request_id != request.request_id
                        || old.predecessor.project != *project(request)?
                    {
                        return Err(refused("successor replay differs"));
                    }
                    if let Some(id) = registry
                        .successors
                        .values()
                        .find(|s| {
                            s.native.preparation_request_id() == &request.request_id
                                && &s.native.predecessor().owner == actor
                        })
                        .map(|s| s.native.0.id.clone())
                    {
                        Ok(ResponseBody::BrowserAnalysisSuccessor(Box::new(
                            registry.successor_report(&id)?,
                        )))
                    } else {
                        old.phase = BrowserAnalysisSuccessorPhase::Aborted;
                        old.successor.active = false;
                        old.successor.retirement = Some(LiveCaptureRetirement::EngineRestarted);
                        old.successor.server_elapsed = None;
                        old.successor.queue = None;
                        old.successor.admitted_pixel_bytes = 0;
                        if let Some(info) = &mut old.successor.inference {
                            info.state = LiveCaptureInferenceState::Retired;
                            info.latest = None;
                            info.error = None;
                        }
                        old.validate()?;
                        Ok(ResponseBody::BrowserAnalysisSuccessor(old))
                    }
                }
                ResponseBody::BrowserAnalysisTransfer(old) => {
                    if old.predecessor.project != *project(request)?
                        || old.successor.project != *project(request)?
                    {
                        return Err(refused("transfer replay project differs"));
                    }
                    let value = registry
                        .transfer_report_for(actor, project(request)?, &old.original_request_id)
                        .unwrap_or_else(|_| {
                            let mut history = *old;
                            history.phase = BrowserAnalysisTransferPhase::Aborted;
                            history.error =
                                Some(refused("native transfer lifetime is absent after restart"));
                            history
                        });
                    value.validate()?;
                    Ok(ResponseBody::BrowserAnalysisTransfer(Box::new(value)))
                }
                _ => Err(refused("original successor request type differs")),
            };
        }
        match &request.command {
            Command::PrepareBrowserAnalysisSuccessor {
                engine_epoch,
                predecessor,
                options,
            } => {
                check_revision(&db, request)?;
                options.validate()?;
                if engine_epoch.as_str() != self.engine_epoch
                    || registry.successors.len() >= MAX_RETAINED
                {
                    return Err(refused("successor boot or retained capacity unavailable"));
                }
                let prior = registry
                    .entries
                    .get(predecessor.as_str())
                    .ok_or_else(|| refused("successor predecessor absent"))?;
                if &prior.owner != actor
                    || prior.report.identity.project != *project(request)?
                    || Some(prior.report.identity.revision) != request.expected_revision
                    || !prior.report.active
                    || prior.cancel.load(Ordering::Acquire)
                    || prior.native_input_close.is_some()
                    || prior.native_transfer.is_some()
                    || prior.native_successor.is_some()
                    || prior.report.options.browser_analysis.is_none()
                    || prior
                        .report
                        .options
                        .inference
                        .as_ref()
                        .is_none_or(|i| i.profile.provider != InferenceProvider::Cpu)
                    || prior
                        .report
                        .inference
                        .as_ref()
                        .is_none_or(|i| i.state != LiveCaptureInferenceState::Running)
                    || registry.successors.values().any(|s| {
                        s.native.predecessor().identity.epoch == *predecessor
                            && !matches!(
                                s.phase,
                                BrowserAnalysisSuccessorPhase::Active
                                    | BrowserAnalysisSuccessorPhase::Aborted
                            )
                    })
                {
                    return Err(refused(
                        "successor requires one current original browser CPU predecessor",
                    ));
                }
                let predecessor_selection = selection(prior)?;
                registry.native_source_basis_scoped(
                    &predecessor_selection,
                    true,
                    false,
                    None,
                    false,
                )?;
                let previous_media = prior
                    .report
                    .options
                    .browser_media
                    .as_ref()
                    .expect("validated source");
                let previous_analysis = prior
                    .report
                    .options
                    .browser_analysis
                    .as_ref()
                    .expect("validated analysis");
                if options.browser_media.selection != previous_media.selection
                    || options.browser_media.clock != previous_media.clock
                    || options.browser_analysis.player != previous_analysis.player
                    || options.browser_analysis.policy != previous_analysis.policy
                    || options.browser_analysis.feed_id == previous_analysis.feed_id
                    || registry.entries.values().any(|e| {
                        e.report
                            .options
                            .browser_analysis
                            .as_ref()
                            .is_some_and(|a| a.feed_id == options.browser_analysis.feed_id)
                    })
                {
                    return Err(refused(
                        "successor changes original source, player, lookahead or reused feed",
                    ));
                }
                if registry
                    .entries
                    .values()
                    .filter(|e| e.runtime.is_some())
                    .count()
                    >= MAX_ACTIVE
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "successor overlap capacity exhausted",
                    ));
                }
                let predecessor_until = prior
                    .native_clock
                    .lease_deadline(u64::from(prior.report.options.lease_ms) * 1_000_000)?;
                let notify = prior.native_delivery.clone();
                let predecessor_cancel = prior.cancel.clone();
                let created = Instant::now();
                let native_clock = clock::NativeCaptureClock::new(created);
                let mut derived = prior.report.options.clone();
                derived.browser_media = Some(options.browser_media.clone());
                derived.browser_analysis = Some(options.browser_analysis.clone());
                derived.lease_ms = options.lease_ms;
                derived.continuation_mode = LiveContinuationMode::Immediate;
                derived.validate()?;
                let media = self.pin_browser_media_locked(&db, request, &options.browser_media)?;
                let timing = self.capture_browser_timing_locked(
                    &db,
                    request,
                    &options.browser_analysis.timing_binding_id,
                )?;
                if timing.binding().browser_media != options.browser_media
                    || timing.binding().player != options.browser_analysis.player
                {
                    return Err(refused(
                        "successor timing does not bind exact native media/player",
                    ));
                }
                let identity = LiveIdentity {
                    epoch: id!(AttemptId),
                    project: predecessor_selection.identity.project.clone(),
                    revision: predecessor_selection.identity.revision,
                    source: predecessor_selection.identity.source.clone(),
                };
                let clock = derived.clock(identity.epoch.clone());
                let manifest = inference::manifest(self, &derived, &identity, &clock)?
                    .ok_or_else(|| refused("successor CPU manifest absent"))?;
                let info = inference::initial_report_with_mode(self, &manifest)?;
                if info.profile_sha256 != predecessor_selection.profile_sha256
                    || !inference::same_cpu_mode(prior.report.inference.as_ref(), Some(&info))?
                {
                    return Err(refused("successor profile differs"));
                }
                let runtime = LiveRgbRuntime::new(
                    &self.pool,
                    identity.clone(),
                    derived.policy,
                    Some(clock.clone()),
                    LiveClockTick(0),
                )?;
                let cancel = Arc::new(AtomicBool::new(false));
                let now = native_clock.tick(created)?;
                let until = native_clock
                    .lease_deadline(u64::from(derived.lease_ms) * 1_000_000)?
                    .min(predecessor_until);
                if native_clock.latest_native()?.latest_ns >= until {
                    return Err(refused("successor expired during admission"));
                }
                let capture = LiveCaptureReport {
                    input_close: None,
                    continuation: None,
                    age_basis: LiveCaptureAgeBasis::ProducerDeclared,
                    encoded_source: None,
                    engine_epoch: engine_epoch.clone(),
                    identity: identity.clone(),
                    clock,
                    options: derived.clone(),
                    active: true,
                    retirement: None,
                    server_elapsed: Some(now),
                    admitted_pixel_bytes: crate::live_frames::LIVE_RGB_ADMISSION_BYTES,
                    received_frames: 0,
                    last_frame: None,
                    inference: Some(info.clone()),
                    queue: None,
                };
                let (worker_retirement, retirement_guard) =
                    crate::live_inference::CpuWorkerRetirement::new();
                let native = NativeCaptureSuccessor(Arc::new(SuccessorIdentity {
                    id: id!(ArtifactId),
                    preparation_request: request.request_id.clone(),
                    predecessor: predecessor_selection.clone(),
                    successor: NativeCaptureSelection {
                        cpu_manifest_sha256: cpu_manifest_hash(&info)?,
                        owner: actor.clone(),
                        identity: identity.clone(),
                        attempt_id: info.attempt_id.clone(),
                        profile_sha256: info.profile_sha256,
                        browser_media: options.browser_media.clone(),
                    },
                    expires_at_ns: until,
                    retired: AtomicBool::new(false),
                    transfer_started: AtomicBool::new(false),
                    predecessor_cancel,
                    successor_cancel: cancel.clone(),
                    cancel_requested: AtomicBool::new(false),
                    worker_retirement,
                }));
                let value = BrowserAnalysisSuccessorReport {
                    original_request_id: request.request_id.clone(),
                    predecessor: predecessor_selection.identity,
                    successor: capture.clone(),
                    phase: BrowserAnalysisSuccessorPhase::Preparing,
                    transfer_request_id: None,
                };
                value.validate()?;
                let body = ResponseBody::BrowserAnalysisSuccessor(Box::new(value));
                let tx = db.transaction().map_err(internal)?;
                self.arm_worker_fence(&tx)?;
                self.arm_staging_fence(&tx)?;
                remember(&tx, actor.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                let id = native.0.id.clone();
                registry.entries.insert(
                    identity.epoch.to_string(),
                    Entry {
                        owner: actor.clone(),
                        created,
                        native_clock,
                        native_accepted: None,
                        native_delivery: None,
                        native_input_close: None,
                        native_successor: Some(id.clone()),
                        native_transfer: None,
                        native_transfer_joined: false,
                        report: capture,
                        runtime: Some(runtime),
                        cancel: cancel.clone(),
                        origin: None,
                        recording: None,
                        browser_media: Some(media),
                        browser_analysis_pin: Some(timing),
                    },
                );
                registry.successors.insert(
                    id.clone(),
                    SuccessorEntry {
                        native: native.clone(),
                        phase: BrowserAnalysisSuccessorPhase::Preparing,
                        transfer: None,
                    },
                );
                drop(registry);
                drop(db);
                let notification = notify
                    .map(|sink| {
                        sink.sink
                            .upgrade()
                            .ok_or_else(|| refused("original native successor consumer is absent"))
                            .and_then(|sink| {
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    sink.prepare_successor(native.clone())
                                }))
                                .unwrap_or_else(|_| {
                                    Err(refused("native successor consumer failed"))
                                })
                            })
                    })
                    .transpose();
                let launch = notification.and_then(|_| {
                    inference::spawn_tracked(
                        self,
                        identity.epoch.clone(),
                        manifest,
                        cancel,
                        inference::CpuLaunchLifetime {
                            origin: created,
                            lease_end: created + Duration::from_millis(u64::from(derived.lease_ms)),
                            retirement: Some(retirement_guard),
                        },
                    )
                });
                let mut registry = self.live_captures.lock().map_err(internal)?;
                if let Err(error) = launch {
                    registry.abort_successor(&id, error)?;
                }
                Ok(ResponseBody::BrowserAnalysisSuccessor(Box::new(
                    registry.successor_report(&id)?,
                )))
            }
            Command::TransferBrowserAnalysisInput {
                predecessor,
                successor,
                boundary,
            } => {
                check_revision(&db, request)?;
                let id = registry
                    .entries
                    .get(successor.as_str())
                    .and_then(|e| e.native_successor.clone())
                    .ok_or_else(|| refused("original pending successor absent"))?;
                let native = registry
                    .successors
                    .get(&id)
                    .expect("native slot")
                    .native
                    .clone();
                registry.check_successor(&native)?;
                if &native.predecessor().identity.epoch != predecessor
                    || &native.predecessor().owner != actor
                    || native.predecessor().identity.project != *project(request)?
                    || Some(native.predecessor().identity.revision) != request.expected_revision
                    || native.preparation_request_id() == &request.request_id
                {
                    return Err(refused("transfer differs from exact original successor"));
                }
                if registry.successor_report(&id)?.phase != BrowserAnalysisSuccessorPhase::Prepared
                    || registry
                        .successors
                        .get(&id)
                        .expect("slot")
                        .transfer
                        .is_some()
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "successor worker/consumer not prepared for original boundary",
                    ));
                }
                let a = registry
                    .entries
                    .get(predecessor.as_str())
                    .expect("checked A");
                let b = registry.entries.get(successor.as_str()).expect("checked B");
                if a.native_input_close.is_some()
                    || a.native_transfer.is_some()
                    || a.report.last_frame.as_ref() != Some(boundary)
                    || b.report.received_frames != 0
                {
                    return Err(refused(
                        "transfer boundary changed or successor input already exists",
                    ));
                }
                let pixels = a
                    .runtime
                    .as_ref()
                    .expect("active A")
                    .inspect_transfer_boundary(boundary)?;
                tick(a)?;
                tick(b)?;
                let original = a
                    .native_clock
                    .map(boundary.capture_tick)?
                    .ok_or_else(|| refused("boundary native capture correspondence absent"))?;
                let mapped = b.native_clock.shared_boundary(
                    b.created,
                    a.created,
                    boundary.capture_tick,
                    original,
                )?;
                let presentation = b
                    .report
                    .clock
                    .map(boundary.source_time)
                    .map_err(internal)?
                    .project
                    .time;
                if presentation != pixels.frame().presentation_time {
                    return Err(refused("shared boundary project correspondence differs"));
                }
                let reserved_at_ns = a.native_clock.latest_native()?.earliest_ns;
                let b_now = b.native_clock.retirement_tick();
                let value = BrowserAnalysisTransferReport {
                    original_request_id: request.request_id.clone(),
                    preparation_request_id: native.preparation_request_id().clone(),
                    predecessor: native.predecessor().identity.clone(),
                    successor: native.successor().identity.clone(),
                    boundary: boundary.clone(),
                    phase: BrowserAnalysisTransferPhase::BoundaryReserved,
                    predecessor_worker_joined: false,
                    predecessor_delivery_settled: false,
                    successor_pair_accepted: false,
                    error: None,
                };
                value.validate()?;
                let tx = db.transaction().map_err(internal)?;
                remember(
                    &tx,
                    actor.as_str(),
                    request,
                    fingerprint,
                    &ResponseBody::BrowserAnalysisTransfer(Box::new(value.clone())),
                )?;
                tx.commit().map_err(internal)?;
                registry.successors.get_mut(&id).expect("slot").transfer = Some(TransferWork {
                    report: value,
                    boundary: Some(pixels.clone()),
                    seed: None,
                    observation: None,
                    reserved_at_ns,
                    successor_pair: None,
                    proof: None,
                    notified: false,
                });
                registry.successors.get_mut(&id).expect("slot").phase =
                    BrowserAnalysisSuccessorPhase::Transferring;
                let result = (|| -> PResult<()> {
                    let a = registry.entries.get_mut(predecessor.as_str()).expect("A");
                    a.runtime
                        .as_mut()
                        .expect("A runtime")
                        .reserve_transfer_boundary(boundary)?;
                    a.native_transfer = Some(id.clone());
                    native.0.transfer_started.store(true, Ordering::Release);
                    let b = registry.entries.get_mut(successor.as_str()).expect("B");
                    let seed = b
                        .runtime
                        .as_mut()
                        .expect("B runtime")
                        .seed_transfer_boundary(&pixels, mapped.tick, presentation, b_now)?;
                    if !seed.belongs_to(&pixels) {
                        return Err(refused("native shared seed differs"));
                    }
                    let seed_frame = seed.frame().clone();
                    b.report.received_frames = 1;
                    b.report.last_frame = Some(LiveFrameHeader {
                        frame: seed_frame.frame,
                        source_time: seed_frame.source_time,
                        capture_tick: seed_frame.capture_tick,
                        sha256: seed_frame.artifact.as_str().into(),
                    });
                    let transfer = registry
                        .successors
                        .get_mut(&id)
                        .expect("slot")
                        .transfer
                        .as_mut()
                        .expect("transfer");
                    transfer.seed = Some(seed);
                    transfer.observation = Some(NativeBoundaryObservation {
                        predecessor_frame: pixels.frame().clone(),
                        successor_seed: seed_frame,
                        predecessor_capture: mapped.original,
                        successor_capture: mapped.mapped,
                    });
                    Ok(())
                })();
                if let Err(error) = result {
                    registry.abort_successor(&id, error)?;
                }
                Ok(ResponseBody::BrowserAnalysisTransfer(Box::new(
                    registry.transfer_report_for(actor, project(request)?, &request.request_id)?,
                )))
            }
            Command::BrowserAnalysisTransferStatus {
                transfer_request_id,
            }
            | Command::CancelBrowserAnalysisTransfer {
                transfer_request_id,
            } => {
                let mut value =
                    registry.transfer_report_for(actor, project(request)?, transfer_request_id)?;
                if matches!(
                    request.command,
                    Command::CancelBrowserAnalysisTransfer { .. }
                ) {
                    let tx = db.transaction().map_err(internal)?;
                    remember(
                        &tx,
                        actor.as_str(),
                        request,
                        fingerprint,
                        &ResponseBody::BrowserAnalysisTransfer(Box::new(value.clone())),
                    )?;
                    tx.commit().map_err(internal)?;
                    let id = registry
                        .successors
                        .values()
                        .find(|s| {
                            s.transfer.as_ref().is_some_and(|t| {
                                t.report.original_request_id == *transfer_request_id
                            }) && &s.native.predecessor().owner == actor
                        })
                        .expect("checked transfer")
                        .native
                        .0
                        .id
                        .clone();
                    registry.abort_successor(
                        &id,
                        ProtocolError::new(
                            ErrorCode::Cancelled,
                            "original source transfer cancelled",
                        ),
                    )?;
                    value = registry.transfer_report_for(
                        actor,
                        project(request)?,
                        transfer_request_id,
                    )?;
                }
                Ok(ResponseBody::BrowserAnalysisTransfer(Box::new(value)))
            }
            _ => Err(refused("expected browser analysis successor command")),
        }
    }
}
impl Registry {
    fn transfer_report_for(
        &self,
        owner: &SessionId,
        project: &ProjectId,
        request: &RequestId,
    ) -> PResult<BrowserAnalysisTransferReport> {
        let slot = self
            .successors
            .values()
            .find(|s| {
                &s.native.predecessor().owner == owner
                    && &s.native.predecessor().identity.project == project
                    && s.transfer
                        .as_ref()
                        .is_some_and(|t| &t.report.original_request_id == request)
            })
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "original source transfer absent")
            })?;
        let report = slot.transfer.as_ref().expect("selected").report.clone();
        report.validate()?;
        Ok(report)
    }
}
