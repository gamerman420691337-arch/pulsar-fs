//! Qualified-fact Handy HSSP playback coordinator. No network or device discovery occurs
//! here; the engine supplies verified qualification, authority and clock facts.
use crate::handy_wire::EncodedRequest;
use pulsar_protocol::*;

const NS_PER_MS: u64 = 1_000_000;

pub mod dispatch;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackFailure {
    Domain(CoreError),
    InvalidTimingQualification,
    InvalidClock,
    RetiredCommand,
    StaleCommand,
    AdmissionChanged,
    JournalUnavailable,
}
impl From<CoreError> for PlaybackFailure {
    fn from(value: CoreError) -> Self {
        Self::Domain(value)
    }
}

/// Construct only after the engine verifies that this qualification record
/// covers Handy HSSP curve execution, command age and clock error for the
/// exact device configuration. This constructor does not verify evidence.
/// No Deserialize or default limits are provided.
#[derive(Clone, Debug)]
pub struct QualifiedCommandTiming {
    qualification: DeviceQualification,
    max_age_ns: u64,
    max_clock_uncertainty_ns: u64,
    drift_ppm: u32,
}
impl QualifiedCommandTiming {
    fn for_reference(&self, reference: StartReference) -> Result<Self, PlaybackFailure> {
        let mut value = self.clone();
        if let StartReference::SharedInstant {
            media_rate_error_ppm,
        } = reference
        {
            if media_rate_error_ppm >= 1_000_000 {
                return Err(PlaybackFailure::InvalidTimingQualification);
            }
            value.drift_ppm = value
                .drift_ppm
                .checked_add(media_rate_error_ppm)
                .ok_or(PlaybackFailure::InvalidTimingQualification)?;
        }
        Ok(value)
    }
    fn checked_clock_age(
        &self,
        clock: &PlaybackClock,
        clock_domain: &ArtifactId,
        now_ns: u64,
    ) -> Result<u64, PlaybackFailure> {
        if clock_domain != &clock.domain {
            return Err(PlaybackFailure::InvalidClock);
        }
        let age = now_ns
            .checked_sub(clock.monotonic_ns)
            .ok_or(PlaybackFailure::InvalidClock)?;
        if age > self.max_age_ns || now_ns > clock.valid_until_ns {
            return Err(PlaybackFailure::StaleCommand);
        }
        // Conservative ceiling for drift, plus all discarded sub-ms time.
        let drift = (u128::from(age) * u128::from(self.drift_ppm)).div_ceil(u128::from(NS_PER_MS));
        let uncertainty = u128::from(clock.uncertainty_ns) + drift + u128::from(age % NS_PER_MS);
        if uncertainty > u128::from(self.max_clock_uncertainty_ns) {
            return Err(PlaybackFailure::InvalidClock);
        }
        Ok(age)
    }

    pub(crate) fn configuration(&self) -> &DeviceConfiguration {
        self.qualification.configuration()
    }
    pub fn new(
        qualification: DeviceQualification,
        max_age_ns: u64,
        max_clock_uncertainty_ns: u64,
        drift_ppm: u32,
    ) -> Result<Self, PlaybackFailure> {
        if max_age_ns == 0 || max_clock_uncertainty_ns == 0 {
            return Err(PlaybackFailure::InvalidTimingQualification);
        }
        Ok(Self {
            qualification,
            max_age_ns,
            max_clock_uncertainty_ns,
            drift_ppm,
        })
    }
}

/// Host/server mapping supplied by the engine clock service. Its domain ID
/// changes on clock reset/restart. This is not a client authorization DTO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackClock {
    pub domain: ArtifactId,
    pub monotonic_ns: u64,
    /// Source validity deadline survives reanchoring; callers cannot refresh source age.
    pub valid_until_ns: u64,
    pub estimated_server_ms: u64,
    pub uncertainty_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStart {
    serial: u64,
    session: DeviceSessionId,
    controller: ClientId,
    admission: PlaybackAdmission,
    clock: PlaybackClock,
    start_time_ms: u64,
    reference: StartReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartReference {
    FixedOffset,
    SharedInstant { media_rate_error_ppm: u32 },
}

/// An acknowledgment is a transport outcome, never physical stopping evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    Acknowledged,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EffectKind {
    Play,
    Stop,
}

/// A sealed effect intent. Send outside the coordinator lock, after checking
/// current authority. Completions must return this exact token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackEffect {
    sequence: u64,
    session: DeviceSessionId,
    request: EncodedRequest,
    kind: EffectKind,
    admission: PlaybackAdmission,
    controller: Option<ClientId>,
    qualification_record: ArtifactId,
    clock: Option<PlaybackClock>,
    reference: Option<StartReference>,
}
impl PlaybackEffect {
    pub(crate) fn retained_clock(&self) -> Option<&PlaybackClock> {
        self.clock.as_ref()
    }
    pub(crate) fn is_referenced_start(&self) -> bool {
        self.referenced_media_rate_error_ppm().is_some()
    }
    pub(crate) fn referenced_media_rate_error_ppm(&self) -> Option<u32> {
        match (self.kind, self.reference) {
            (
                EffectKind::Play,
                Some(StartReference::SharedInstant {
                    media_rate_error_ppm,
                }),
            ) => Some(media_rate_error_ppm),
            _ => None,
        }
    }
    pub fn request(&self) -> &EncodedRequest {
        &self.request
    }
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }
    pub(crate) fn session_id(&self) -> &DeviceSessionId {
        &self.session
    }
    pub(crate) fn is_stop(&self) -> bool {
        self.kind == EffectKind::Stop
    }
    pub(crate) fn admission(&self) -> &PlaybackAdmission {
        &self.admission
    }
    pub(crate) fn controller(&self) -> Option<&ClientId> {
        self.controller.as_ref()
    }
    pub(crate) fn qualification_record(&self) -> &ArtifactId {
        &self.qualification_record
    }
}

pub struct HandyPlaybackRuntime {
    session: DeviceSession,
    timing: QualifiedCommandTiming,
    pending: Option<PendingStart>,
    next_serial: u64,
    play_inflight: Option<PlaybackEffect>,
    stop_inflight: Option<PlaybackEffect>,
    claimed_play: Option<u64>,
    claimed_stop: Option<u64>,
}

/// Stop transport handoff with its exact durable metadata receipt. Construction
/// is engine-owned; this token carries no physical stopping evidence.
pub struct PreparedStopEffect {
    effect: PlaybackEffect,
    receipt: crate::device_effect_journal::PreparedEffectReceipt,
}
mod emergency_stop;
pub use emergency_stop::{EmergencyStopEffect, EmergencyStopObservation};
impl PreparedStopEffect {
    pub fn request(&self) -> &EncodedRequest {
        self.effect.request()
    }
}

/// A checked callback observation. It can retry only its original metadata
/// write; it cannot issue an effect or supply physical stopping evidence.
pub struct EffectCompletionRecord {
    effect: PlaybackEffect,
    receipt: crate::device_effect_journal::PreparedEffectReceipt,
    outcome: DispatchOutcome,
    recorded: bool,
}
/// Compatibility name for the observation returned by stop completion.
pub type StopCompletionRecord = EffectCompletionRecord;

/// A matched play callback and its independent stop-construction result.
/// Neither a recorded acknowledgment nor a stop intent proves physical stopping.
/// Stop failure must not discard the accepted transport observation.
#[must_use = "retain the observation and handle the stop result"]
pub struct PlayCompletion {
    pub observation: EffectCompletionRecord,
    pub stop: Result<Option<PlaybackEffect>, PlaybackFailure>,
}

impl EffectCompletionRecord {
    pub(crate) fn try_persist_play_operation(
        &mut self,
        claimed: &crate::device_effect_journal::ClaimedPlayReceipt,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<(), PlaybackFailure> {
        self.recorded = false;
        if &self.receipt != claimed.effect_receipt() {
            return Err(PlaybackFailure::JournalUnavailable);
        }
        journal
            .validate_effect_receipt(&self.effect, &self.receipt)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        journal
            .try_complete_play_operation(claimed, self.outcome)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        self.recorded = true;
        Ok(())
    }
    pub fn outcome(&self) -> DispatchOutcome {
        self.outcome
    }

    pub fn journal_pending(&self) -> bool {
        !self.recorded
    }

    /// Explicit same-receipt retry. Boot ownership, immutable correspondence,
    /// persisted integrity and terminal-outcome consistency still apply.
    pub fn persist(
        &mut self,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<(), PlaybackFailure> {
        self.persist_with(journal, false)
    }
    pub(crate) fn try_persist(
        &mut self,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<(), PlaybackFailure> {
        self.persist_with(journal, true)
    }
    fn persist_with(
        &mut self,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
        nonblocking: bool,
    ) -> Result<(), PlaybackFailure> {
        self.recorded = false;
        journal
            .validate_effect_receipt(&self.effect, &self.receipt)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        if nonblocking {
            journal.try_complete(&self.receipt, self.outcome)
        } else {
            journal.complete(&self.receipt, self.outcome)
        }
        .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        self.recorded = true;
        Ok(())
    }
}

impl HandyPlaybackRuntime {
    /// Attach a durable receipt after recording a detached intent. A retired
    /// stop cannot become current again merely because its write completed.
    pub(crate) fn attach_stop_receipt(
        &self,
        effect: PlaybackEffect,
        receipt: crate::device_effect_journal::PreparedEffectReceipt,
        binding: &crate::device_effect_journal::EffectReceiptBinding,
    ) -> Result<PreparedStopEffect, PlaybackFailure> {
        if !effect.is_stop() || !self.effect_is_current(&effect) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        binding
            .validate_effect_receipt(&effect, &receipt)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        Ok(PreparedStopEffect { effect, receipt })
    }
    /// Journal a conditional stop, including a stop returned by unknown play
    /// completion, before releasing its request to a transport dispatcher.
    pub fn prepare_stop_durable(
        &mut self,
        effect: PlaybackEffect,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<PreparedStopEffect, PlaybackFailure> {
        if effect.kind != EffectKind::Stop || !self.effect_is_current(&effect) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        match journal.prepare(self, &effect) {
            Ok(receipt) => Ok(PreparedStopEffect { effect, receipt }),
            Err(_) => {
                // No durable handoff was returned. Retire this token without
                // clearing uncertainty; an explicit fresh stop can be requested.
                self.stop_inflight = None;
                self.session.mark_stop_unconfirmed()?;
                Err(PlaybackFailure::JournalUnavailable)
            }
        }
    }

    fn prepare_optional_stop(
        &mut self,
        effect: Option<PlaybackEffect>,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<Option<PreparedStopEffect>, PlaybackFailure> {
        effect
            .map(|effect| self.prepare_stop_durable(effect, journal))
            .transpose()
    }

    pub fn request_stop_durable(
        &mut self,
        controller: &ClientId,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<Option<PreparedStopEffect>, PlaybackFailure> {
        let effect = self.request_stop(controller)?;
        self.prepare_optional_stop(effect, journal)
    }

    pub fn controller_lost_durable(
        &mut self,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<Option<PreparedStopEffect>, PlaybackFailure> {
        let effect = self.controller_lost()?;
        self.prepare_optional_stop(effect, journal)
    }

    pub fn revoke_controller_durable(
        &mut self,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<Option<PreparedStopEffect>, PlaybackFailure> {
        let effect = self.revoke_controller()?;
        self.prepare_optional_stop(effect, journal)
    }

    /// Recheck under the coordinator lock immediately before transport handoff.
    pub fn prepared_stop_is_current(&self, stop: &PreparedStopEffect) -> bool {
        self.effect_is_current(&stop.effect)
    }

    /// A matching callback retires host authority even if recording its outcome
    /// fails. Neither acknowledgment nor successful journaling proves stopping.
    pub fn complete_stop_durable(
        &mut self,
        stop: &PreparedStopEffect,
        outcome: DispatchOutcome,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<(), PlaybackFailure> {
        let record = self.complete_stop_with_recovery(stop, outcome, journal)?;
        if record.journal_pending() {
            return Err(PlaybackFailure::JournalUnavailable);
        }
        Ok(())
    }

    /// Ok means the callback matched and host authority retired. The caller
    /// must inspect journal_pending and retain this record for explicit retry.
    pub fn complete_stop_with_recovery(
        &mut self,
        stop: &PreparedStopEffect,
        outcome: DispatchOutcome,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<StopCompletionRecord, PlaybackFailure> {
        let mut record = self.observe_stop_completion(stop, outcome, &journal.receipt_binding())?;
        let _ = record.persist(journal);
        Ok(record)
    }

    /// Retire a matched stop callback without journal I/O. The owner may release
    /// its control-state lock before explicitly persisting the returned record.
    pub fn observe_stop_completion(
        &mut self,
        stop: &PreparedStopEffect,
        outcome: DispatchOutcome,
        binding: &crate::device_effect_journal::EffectReceiptBinding,
    ) -> Result<StopCompletionRecord, PlaybackFailure> {
        if !self.prepared_stop_is_current(stop) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        binding
            .validate_effect_receipt(&stop.effect, &stop.receipt)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        self.complete_stop(stop.effect.clone(), outcome)?;
        Ok(EffectCompletionRecord {
            effect: stop.effect.clone(),
            receipt: stop.receipt.clone(),
            outcome,
            recorded: false,
        })
    }

    /// Accept a matched play callback and attempt its outcome write. Callers
    /// requiring a nonblocking control path use observe_play_completion first.
    pub fn complete_play_with_recovery(
        &mut self,
        effect: &PlaybackEffect,
        receipt: &crate::device_effect_journal::PreparedEffectReceipt,
        outcome: DispatchOutcome,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<PlayCompletion, PlaybackFailure> {
        let mut completion =
            self.observe_play_completion(effect, receipt, outcome, &journal.receipt_binding())?;
        let _ = completion.observation.persist(journal);
        Ok(completion)
    }

    /// Capture a matched play observation without journal I/O. Stop construction
    /// failure cannot discard that observation. Receipt binding is not a fresh
    /// ownership grant; persistence and any transport handoff recheck authority.
    pub fn observe_play_completion(
        &mut self,
        effect: &PlaybackEffect,
        receipt: &crate::device_effect_journal::PreparedEffectReceipt,
        outcome: DispatchOutcome,
        binding: &crate::device_effect_journal::EffectReceiptBinding,
    ) -> Result<PlayCompletion, PlaybackFailure> {
        if effect.kind != EffectKind::Play || !self.effect_is_current(effect) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        binding
            .validate_effect_receipt(effect, receipt)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        let stop = self.complete_play(effect.clone(), outcome);
        Ok(PlayCompletion {
            observation: EffectCompletionRecord {
                effect: effect.clone(),
                receipt: receipt.clone(),
                outcome,
                recorded: false,
            },
            stop,
        })
    }

    /// Return a start intent only after its exact metadata is durably recorded.
    pub fn dispatch_start_durable(
        &mut self,
        command: PendingStart,
        current: &PlaybackAdmission,
        controller: &ClientId,
        clock_domain: &ArtifactId,
        now_ns: u64,
        journal: &mut crate::device_effect_journal::DeviceEffectJournal,
    ) -> Result<
        (
            PlaybackEffect,
            crate::device_effect_journal::PreparedEffectReceipt,
        ),
        PlaybackFailure,
    > {
        let effect = self.dispatch_start(command, current, controller, clock_domain, now_ns)?;
        match journal.prepare(self, &effect) {
            Ok(receipt) => Ok((effect, receipt)),
            Err(_) => {
                self.play_inflight = None;
                self.session
                    .request_stop(controller, &self.session.session().clone())?;
                self.session.mark_stop_unconfirmed()?;
                Err(PlaybackFailure::JournalUnavailable)
            }
        }
    }
    pub fn new(
        device: DeviceId,
        session: DeviceSessionId,
        timing: QualifiedCommandTiming,
    ) -> Result<Self, PlaybackFailure> {
        if device != timing.qualification.configuration().device {
            return Err(CoreError::MissingQualification.into());
        }
        Ok(Self {
            session: DeviceSession::new(device, session),
            timing,
            pending: None,
            next_serial: 0,
            play_inflight: None,
            stop_inflight: None,
            claimed_play: None,
            claimed_stop: None,
        })
    }
    pub fn session(&self) -> &DeviceSession {
        &self.session
    }
    pub fn recover_unconfirmed(
        device: DeviceId,
        fresh_session: DeviceSessionId,
        timing: QualifiedCommandTiming,
    ) -> Result<Self, PlaybackFailure> {
        let mut runtime = Self::new(device.clone(), fresh_session.clone(), timing)?;
        runtime.session = DeviceSession::recover_unconfirmed(device, fresh_session);
        Ok(runtime)
    }
    pub fn arm(
        &mut self,
        controller: ClientId,
        admission: PlaybackAdmission,
        headless: Option<&HeadlessApproval>,
    ) -> Result<(), PlaybackFailure> {
        self.session
            .arm(controller, admission, &self.timing.qualification, headless)?;
        Ok(())
    }
    pub fn prepare_start(
        &mut self,
        controller: &ClientId,
        clock: PlaybackClock,
        start_time_ms: u64,
    ) -> Result<PendingStart, PlaybackFailure> {
        self.prepare_start_with_reference(
            controller,
            clock,
            start_time_ms,
            StartReference::FixedOffset,
        )
    }

    /// The engine supplies a qualified phase/server pair at the clock's exact
    /// native anchor. Dispatch may age its validity but must not advance just
    /// one member of that immutable pair. This constructor verifies no browser
    /// evidence; native admission and the original-play journal own that proof.
    pub fn prepare_referenced_start(
        &mut self,
        controller: &ClientId,
        clock: PlaybackClock,
        start_time_ms: u64,
        media_rate_error_ppm: u32,
    ) -> Result<PendingStart, PlaybackFailure> {
        self.prepare_start_with_reference(
            controller,
            clock,
            start_time_ms,
            StartReference::SharedInstant {
                media_rate_error_ppm,
            },
        )
    }

    fn prepare_start_with_reference(
        &mut self,
        controller: &ClientId,
        clock: PlaybackClock,
        start_time_ms: u64,
        reference: StartReference,
    ) -> Result<PendingStart, PlaybackFailure> {
        self.timing.for_reference(reference)?;
        if self.session.state() != DeviceState::Armed
            || self.session.controller() != Some(controller)
            || self.pending.is_some()
        {
            return Err(CoreError::DeviceAuthorityMismatch.into());
        }
        if clock.monotonic_ns > clock.valid_until_ns
            || clock.uncertainty_ns > self.timing.max_clock_uncertainty_ns
            || EncodedRequest::hssp_play(start_time_ms, clock.estimated_server_ms).is_err()
        {
            return Err(PlaybackFailure::InvalidClock);
        }
        let next = self.next_serial.checked_add(1).ok_or(CoreError::Overflow)?;
        let pending = PendingStart {
            serial: next,
            session: self.session.session().clone(),
            controller: controller.clone(),
            admission: self
                .session
                .admission()
                .ok_or(CoreError::MissingQualification)?
                .clone(),
            clock,
            start_time_ms,
            reference,
        };
        self.next_serial = next;
        self.pending = Some(pending.clone());
        Ok(pending)
    }
    /// Current authority/revision/configuration must be re-read by the engine
    /// just before this call. Retirement precedes effects, preventing replay.
    pub fn dispatch_start(
        &mut self,
        command: PendingStart,
        current: &PlaybackAdmission,
        controller: &ClientId,
        clock_domain: &ArtifactId,
        now_ns: u64,
    ) -> Result<PlaybackEffect, PlaybackFailure> {
        let Some(pending) = &self.pending else {
            return Err(PlaybackFailure::RetiredCommand);
        };
        if pending != &command {
            return Err(PlaybackFailure::RetiredCommand);
        }
        self.pending = None;
        let checked = (|| {
            if self.session.admission() != Some(&command.admission) || current != &command.admission
            {
                return Err(PlaybackFailure::AdmissionChanged);
            }
            if self.session.controller() != Some(controller)
                || controller != &command.controller
                || self.session.session() != &command.session
                || self.session.state() != DeviceState::Armed
            {
                return Err(CoreError::DeviceAuthorityMismatch.into());
            }
            let age = self
                .timing
                .for_reference(command.reference)?
                .checked_clock_age(&command.clock, clock_domain, now_ns)?;
            let server = match command.reference {
                StartReference::FixedOffset => command
                    .clock
                    .estimated_server_ms
                    .checked_add(age / NS_PER_MS)
                    .ok_or(PlaybackFailure::InvalidClock)?,
                StartReference::SharedInstant { .. } => command.clock.estimated_server_ms,
            };
            EncodedRequest::hssp_play(command.start_time_ms, server)
                .map_err(|_| PlaybackFailure::InvalidClock)
        })();
        let request = match checked {
            Ok(request) => request,
            Err(error) => {
                if self.session.state() == DeviceState::Armed {
                    self.session.disarm_before_dispatch()?;
                }
                return Err(error);
            }
        };
        // A crash or lost acknowledgment after this transition is not a clean
        // cancellation: the physical effect may already have occurred.
        let mut effect = self.effect(EffectKind::Play, request)?;
        effect.clock = Some(command.clock.clone());
        effect.reference = Some(command.reference);
        self.session.start(controller, &command.session)?;
        self.play_inflight = Some(effect.clone());
        Ok(effect)
    }
    fn effect(
        &mut self,
        kind: EffectKind,
        request: EncodedRequest,
    ) -> Result<PlaybackEffect, PlaybackFailure> {
        self.next_serial = self.next_serial.checked_add(1).ok_or(CoreError::Overflow)?;
        Ok(PlaybackEffect {
            sequence: self.next_serial,
            session: self.session.session().clone(),
            request,
            kind,
            admission: self
                .session
                .admission()
                .ok_or(CoreError::MissingQualification)?
                .clone(),
            controller: self.session.controller().cloned(),
            qualification_record: self.timing.qualification.record().clone(),
            clock: None,
            reference: None,
        })
    }
    pub fn effect_is_current(&self, effect: &PlaybackEffect) -> bool {
        effect.session == *self.session.session()
            && match effect.kind {
                EffectKind::Play => {
                    self.session.state() == DeviceState::Playing
                        && self.play_inflight.as_ref() == Some(effect)
                }
                EffectKind::Stop => {
                    self.session.state() == DeviceState::StopRequested
                        && self.stop_inflight.as_ref() == Some(effect)
                }
            }
    }
    pub fn complete_play(
        &mut self,
        effect: PlaybackEffect,
        outcome: DispatchOutcome,
    ) -> Result<Option<PlaybackEffect>, PlaybackFailure> {
        if effect.kind != EffectKind::Play || !self.effect_is_current(&effect) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        self.play_inflight = None;
        match outcome {
            DispatchOutcome::Acknowledged => Ok(None),
            DispatchOutcome::Unknown => {
                let controller = self
                    .session
                    .controller()
                    .ok_or(CoreError::DeviceAuthorityMismatch)?
                    .clone();
                self.request_stop(&controller)
            }
        }
    }
    pub fn request_stop(
        &mut self,
        controller: &ClientId,
    ) -> Result<Option<PlaybackEffect>, PlaybackFailure> {
        self.session
            .request_stop(controller, &self.session.session().clone())?;
        self.pending = None;
        self.play_inflight = None;
        self.stop_effect()
    }
    fn stop_effect(&mut self) -> Result<Option<PlaybackEffect>, PlaybackFailure> {
        if !self.session.physical_stop_pending() {
            self.session.disarm_before_dispatch()?;
            return Ok(None);
        }
        if self.stop_inflight.is_some() {
            return Ok(None);
        }
        let effect = self.effect(EffectKind::Stop, EncodedRequest::hssp_stop())?;
        self.stop_inflight = Some(effect.clone());
        Ok(Some(effect))
    }
    pub fn complete_stop(
        &mut self,
        effect: PlaybackEffect,
        _outcome: DispatchOutcome,
    ) -> Result<(), PlaybackFailure> {
        if effect.kind != EffectKind::Stop || !self.effect_is_current(&effect) {
            return Err(PlaybackFailure::RetiredCommand);
        }
        self.stop_inflight = None;
        self.session.mark_stop_unconfirmed()?;
        Ok(())
    }
    pub fn controller_lost(&mut self) -> Result<Option<PlaybackEffect>, PlaybackFailure> {
        self.pending = None;
        self.session.controller_lost();
        if self.session.state() == DeviceState::StopRequested {
            self.play_inflight = None;
            return self.stop_effect();
        }
        Ok(None)
    }
    pub fn revoke_controller(&mut self) -> Result<Option<PlaybackEffect>, PlaybackFailure> {
        self.pending = None;
        self.play_inflight = None;
        self.session.revoke_controller();
        if self.session.state() == DeviceState::StopRequested {
            return self.stop_effect();
        }
        Ok(None)
    }
    pub fn reconnect(&mut self, new_session: DeviceSessionId) -> Result<(), PlaybackFailure> {
        if &new_session == self.session.session() {
            return Err(CoreError::DeviceAuthorityMismatch.into());
        }
        self.pending = None;
        self.play_inflight = None;
        self.stop_inflight = None;
        self.session.reconnect(new_session)?;
        Ok(())
    }
    /// Called only with engine-qualified physical observations or clock facts.
    pub fn confirm_stopped(
        &mut self,
        session: &DeviceSessionId,
        evidence: StopEvidence,
    ) -> Result<(), PlaybackFailure> {
        self.session.confirm_stopped(session, evidence)?;
        self.pending = None;
        self.play_inflight = None;
        self.stop_inflight = None;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::device_effect_journal::DeviceEffectJournal;

    #[test]
    fn stop_completion_write_failure_retries_exact_observation_without_effect_replay() {
        for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
            let dir = tempfile::tempdir().unwrap();
            let (mut runtime, admission, controller) = fixture();
            let mut journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap())
                    .unwrap();
            start(&mut runtime, &admission, &controller);
            let stop = runtime
                .request_stop_durable(&controller, &mut journal)
                .unwrap()
                .unwrap();
            let db = rusqlite::Connection::open(journal_path(dir.path())).unwrap();
            db.execute_batch("CREATE TRIGGER fail_completion BEFORE UPDATE OF outcome ON effects BEGIN SELECT RAISE(ABORT,'synthetic outcome write failure'); END;").unwrap();
            let mut record = runtime
                .complete_stop_with_recovery(&stop, outcome, &mut journal)
                .unwrap();
            assert!(record.journal_pending());
            assert_eq!(record.outcome(), outcome);
            assert!(!runtime.prepared_stop_is_current(&stop));
            assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
            assert!(runtime.session().physical_stop_pending());
            assert_eq!(
                journal
                    .history(&admission.configuration.device, 10)
                    .unwrap()[0]
                    .outcome,
                None
            );
            assert_eq!(
                record.persist(&mut journal),
                Err(PlaybackFailure::JournalUnavailable)
            );
            assert!(record.journal_pending());
            assert!(matches!(
                runtime.complete_stop_with_recovery(&stop, outcome, &mut journal),
                Err(PlaybackFailure::RetiredCommand)
            ));
            db.execute_batch("DROP TRIGGER fail_completion").unwrap();
            let fresh = runtime
                .request_stop_durable(&controller, &mut journal)
                .unwrap()
                .unwrap();
            record.persist(&mut journal).unwrap();
            record.persist(&mut journal).unwrap();
            assert!(!record.journal_pending());
            assert!(runtime.prepared_stop_is_current(&fresh));
            assert_eq!(runtime.session().state(), DeviceState::StopRequested);
            assert!(runtime.session().physical_stop_pending());
            let history = journal
                .history(&admission.configuration.device, 10)
                .unwrap();
            assert_eq!(history.len(), 2);
            assert_eq!(history[0].outcome, None);
            assert_eq!(history[1].outcome, Some(outcome));
        }
    }

    #[test]
    fn stop_completion_pending_observation_cannot_cross_engine_boot() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut old =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("old-boot").unwrap())
                .unwrap();
        start(&mut runtime, &admission, &controller);
        let stop = runtime
            .request_stop_durable(&controller, &mut old)
            .unwrap()
            .unwrap();
        let mut current =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap())
                .unwrap();
        let mut record = runtime
            .complete_stop_with_recovery(&stop, DispatchOutcome::Acknowledged, &mut old)
            .unwrap();
        assert!(record.journal_pending());
        assert_eq!(
            record.persist(&mut old),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert_eq!(
            record.persist(&mut current),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert!(record.journal_pending());
        assert_eq!(
            current
                .history(&admission.configuration.device, 10)
                .unwrap()[0]
                .outcome,
            None
        );
        assert!(runtime.session().physical_stop_pending());
        assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
    }

    #[test]
    fn stop_completion_historical_retry_cannot_authorize_a_reconnected_session() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let old_session = runtime.session().session().clone();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        start(&mut runtime, &admission, &controller);
        let stop = runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .unwrap();
        let mut record = runtime
            .complete_stop_with_recovery(&stop, DispatchOutcome::Acknowledged, &mut journal)
            .unwrap();
        assert!(!record.journal_pending());
        runtime
            .reconnect(DeviceSessionId::new("fresh-session").unwrap())
            .unwrap();
        record.persist(&mut journal).unwrap();
        assert_eq!(runtime.session().state(), DeviceState::Disarmed);
        assert!(runtime.session().physical_stop_pending());
        assert!(runtime.arm(controller, admission.clone(), None).is_err());
        assert!(matches!(
            runtime.complete_stop_with_recovery(&stop, DispatchOutcome::Unknown, &mut journal),
            Err(PlaybackFailure::RetiredCommand)
        ));
        let history = journal
            .history(&admission.configuration.device, 10)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].record.device_session, old_session);
        assert_eq!(history[0].outcome, Some(DispatchOutcome::Acknowledged));
    }

    #[test]
    fn stop_completion_receipt_substitution_creates_no_recovery_record() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let play = start(&mut runtime, &admission, &controller);
        let play_receipt = journal.prepare(&runtime, &play).unwrap();
        let stop = runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .unwrap();
        let forged = PreparedStopEffect {
            effect: stop.effect.clone(),
            receipt: play_receipt,
        };
        assert!(matches!(
            runtime.complete_stop_with_recovery(
                &forged,
                DispatchOutcome::Acknowledged,
                &mut journal
            ),
            Err(PlaybackFailure::JournalUnavailable)
        ));
        assert!(runtime.prepared_stop_is_current(&stop));
        assert!(journal
            .history(&admission.configuration.device, 10)
            .unwrap()
            .iter()
            .all(|row| row.outcome.is_none()));
    }

    #[test]
    fn durable_stop_records_handoff_and_outcome_without_physical_proof() {
        for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
            let dir = tempfile::tempdir().unwrap();
            let (mut runtime, admission, controller) = fixture();
            let mut journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("stop-boot").unwrap())
                    .unwrap();
            start(&mut runtime, &admission, &controller);
            let stop = runtime
                .request_stop_durable(&controller, &mut journal)
                .unwrap()
                .unwrap();
            assert!(runtime.prepared_stop_is_current(&stop));
            assert_eq!(stop.request().path(), "/hssp/stop");
            assert!(stop.request().body().is_none());
            let history = journal
                .history(&admission.configuration.device, 10)
                .unwrap();
            assert_eq!(history.len(), 1);
            assert!(history[0].record.stop);
            assert_eq!(history[0].outcome, None);
            assert_eq!(history[0].record.admission, admission);
            assert_eq!(history[0].record.controller.as_ref(), Some(&controller));
            assert!(runtime
                .request_stop_durable(&controller, &mut journal)
                .unwrap()
                .is_none());
            runtime
                .complete_stop_durable(&stop, outcome, &mut journal)
                .unwrap();
            assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
            assert!(runtime.session().physical_stop_pending());
            assert!(runtime
                .arm(controller.clone(), admission.clone(), None)
                .is_err());
            drop(journal);
            let journal = DeviceEffectJournal::open(
                dir.path(),
                DeviceSessionId::new("fresh-stop-boot").unwrap(),
            )
            .unwrap();
            assert_eq!(
                journal
                    .history(&admission.configuration.device, 10)
                    .unwrap()[0]
                    .outcome,
                Some(outcome)
            );
            let recovered = HandyPlaybackRuntime::recover_unconfirmed(
                admission.configuration.device.clone(),
                DeviceSessionId::new("fresh-device-session").unwrap(),
                runtime.timing,
            )
            .unwrap();
            assert_eq!(recovered.session().state(), DeviceState::Disarmed);
            assert!(recovered.session().physical_stop_pending());
        }
    }

    #[test]
    fn failed_stop_preparation_retires_token_and_allows_explicit_fresh_stop() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut old =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("old-boot").unwrap())
                .unwrap();
        let mut current =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap())
                .unwrap();
        start(&mut runtime, &admission, &controller);
        let abandoned = runtime.request_stop(&controller).unwrap().unwrap();
        assert!(matches!(
            runtime.prepare_stop_durable(abandoned.clone(), &mut old),
            Err(PlaybackFailure::JournalUnavailable)
        ));
        assert!(!runtime.effect_is_current(&abandoned));
        assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
        assert!(runtime.session().physical_stop_pending());
        assert!(current
            .history(&admission.configuration.device, 10)
            .unwrap()
            .is_empty());
        let fresh = runtime
            .request_stop_durable(&controller, &mut current)
            .unwrap()
            .unwrap();
        assert!(runtime.prepared_stop_is_current(&fresh));
        assert_ne!(fresh.effect.sequence, abandoned.sequence);
        assert_eq!(
            runtime.complete_stop(abandoned, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        runtime
            .complete_stop_durable(&fresh, DispatchOutcome::Acknowledged, &mut current)
            .unwrap();
        assert!(runtime.session().physical_stop_pending());
    }

    #[test]
    fn durable_stop_receipt_substitution_cannot_complete_runtime_or_history() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let play = start(&mut runtime, &admission, &controller);
        let play_receipt = journal.prepare(&runtime, &play).unwrap();
        let stop = runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .unwrap();
        let substituted = PreparedStopEffect {
            effect: stop.effect.clone(),
            receipt: play_receipt,
        };
        assert_eq!(
            runtime.complete_stop_durable(
                &substituted,
                DispatchOutcome::Acknowledged,
                &mut journal
            ),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert!(runtime.prepared_stop_is_current(&stop));
        let foreign_dir = tempfile::tempdir().unwrap();
        let mut foreign = DeviceEffectJournal::open(
            foreign_dir.path(),
            DeviceSessionId::new("foreign-boot").unwrap(),
        )
        .unwrap();
        assert_eq!(
            runtime.complete_stop_durable(&stop, DispatchOutcome::Acknowledged, &mut foreign),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert!(runtime.prepared_stop_is_current(&stop));
        assert!(journal
            .history(&admission.configuration.device, 10)
            .unwrap()
            .iter()
            .all(|observation| observation.outcome.is_none()));
        runtime
            .complete_stop_durable(&stop, DispatchOutcome::Unknown, &mut journal)
            .unwrap();
        let history = journal
            .history(&admission.configuration.device, 10)
            .unwrap();
        assert_eq!(history[0].outcome, Some(DispatchOutcome::Unknown));
        assert_eq!(history[1].outcome, None);
    }

    #[test]
    fn stop_callback_journal_failure_still_retires_host_authority() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut old =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("old-boot").unwrap())
                .unwrap();
        start(&mut runtime, &admission, &controller);
        let stop = runtime
            .request_stop_durable(&controller, &mut old)
            .unwrap()
            .unwrap();
        let effect = stop.effect.clone();
        let current =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new-boot").unwrap())
                .unwrap();
        assert_eq!(
            runtime.complete_stop_durable(&stop, DispatchOutcome::Acknowledged, &mut old),
            Err(PlaybackFailure::JournalUnavailable)
        );
        assert!(!runtime.effect_is_current(&effect));
        assert_eq!(runtime.session().state(), DeviceState::StopUnconfirmed);
        assert!(runtime.session().physical_stop_pending());
        assert_eq!(
            current
                .history(&admission.configuration.device, 10)
                .unwrap()[0]
                .outcome,
            None
        );
    }

    #[test]
    fn reconnect_retires_durable_stop_without_settling_recorded_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        start(&mut runtime, &admission, &controller);
        let stop = runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .unwrap();
        runtime
            .reconnect(DeviceSessionId::new("new-session").unwrap())
            .unwrap();
        assert!(!runtime.prepared_stop_is_current(&stop));
        assert_eq!(
            runtime.complete_stop_durable(&stop, DispatchOutcome::Acknowledged, &mut journal),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert_eq!(
            journal
                .history(&admission.configuration.device, 10)
                .unwrap()[0]
                .outcome,
            None
        );
        assert!(runtime.session().physical_stop_pending());
        assert!(runtime.arm(controller, admission, None).is_err());
    }

    #[test]
    fn durable_stop_routes_loss_revocation_and_unknown_play_without_retries() {
        for route in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let (mut runtime, admission, controller) = fixture();
            let mut journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap())
                    .unwrap();
            let play = start(&mut runtime, &admission, &controller);
            let stop = match route {
                0 => runtime
                    .controller_lost_durable(&mut journal)
                    .unwrap()
                    .unwrap(),
                1 => runtime
                    .revoke_controller_durable(&mut journal)
                    .unwrap()
                    .unwrap(),
                _ => {
                    let effect = runtime
                        .complete_play(play.clone(), DispatchOutcome::Unknown)
                        .unwrap()
                        .unwrap();
                    runtime.prepare_stop_durable(effect, &mut journal).unwrap()
                }
            };
            assert!(runtime.prepared_stop_is_current(&stop));
            assert_eq!(
                runtime.complete_play(play, DispatchOutcome::Acknowledged),
                Err(PlaybackFailure::RetiredCommand)
            );
            assert!(runtime
                .controller_lost_durable(&mut journal)
                .unwrap()
                .is_none());
            assert!(runtime
                .revoke_controller_durable(&mut journal)
                .unwrap()
                .is_none());
            assert_eq!(
                journal
                    .history(&admission.configuration.device, 10)
                    .unwrap()
                    .len(),
                1
            );
            runtime
                .complete_stop_durable(&stop, DispatchOutcome::Acknowledged, &mut journal)
                .unwrap();
            assert!(runtime.session().physical_stop_pending());
        }
    }

    #[test]
    fn durable_stop_preserves_headless_loss_and_pre_dispatch_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut runtime, admission, controller) = fixture();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        runtime.prepare_start(&controller, clock(), 0).unwrap();
        assert!(runtime
            .request_stop_durable(&controller, &mut journal)
            .unwrap()
            .is_none());
        assert_eq!(runtime.session().state(), DeviceState::Disarmed);
        assert!(!runtime.session().physical_stop_pending());
        assert!(journal
            .history(&admission.configuration.device, 10)
            .unwrap()
            .is_empty());
        let approval = HeadlessApproval::new(&runtime.timing.qualification).unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), Some(&approval))
            .unwrap();
        let play = start(&mut runtime, &admission, &controller);
        assert!(matches!(
            runtime.prepare_stop_durable(play.clone(), &mut journal),
            Err(PlaybackFailure::RetiredCommand)
        ));
        assert!(runtime.effect_is_current(&play));
        assert!(runtime
            .controller_lost_durable(&mut journal)
            .unwrap()
            .is_none());
        assert!(runtime.effect_is_current(&play));
        assert!(journal
            .history(&admission.configuration.device, 10)
            .unwrap()
            .is_empty());
        let stop = runtime
            .revoke_controller_durable(&mut journal)
            .unwrap()
            .unwrap();
        assert!(runtime.prepared_stop_is_current(&stop));
        assert!(!runtime.effect_is_current(&play));
        assert!(journal
            .history(&admission.configuration.device, 10)
            .unwrap()[0]
            .record
            .controller
            .is_none());
        runtime
            .complete_stop_durable(&stop, DispatchOutcome::Unknown, &mut journal)
            .unwrap();
        assert!(runtime.session().physical_stop_pending());
    }
    pub(super) fn journal_path(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join("device-effects/journal.sqlite3")
    }
    #[test]
    fn durable_start_is_prepared_before_handoff_and_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let (mut r, a, c) = fixture();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                .unwrap();
        let pending = r.prepare_start(&c, clock(), 0).unwrap();
        let (play, _receipt) = r
            .dispatch_start_durable(pending, &a, &c, &clock().domain, 100_000_000, &mut journal)
            .unwrap();
        assert!(r.effect_is_current(&play));
        let before = journal.history(&a.configuration.device, 10).unwrap();
        assert_eq!(before.len(), 1);
        assert!(before[0].outcome.is_none());
        assert_eq!(before[0].record.admission, a);
        assert_eq!(before[0].record.controller, Some(c.clone()));
        drop(journal);
        let after =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-second").unwrap())
                .unwrap();
        assert!(after.history(&a.configuration.device, 10).unwrap()[0]
            .outcome
            .is_none());
        let mut recovered = HandyPlaybackRuntime::recover_unconfirmed(
            a.configuration.device.clone(),
            DeviceSessionId::new("fresh-host-session").unwrap(),
            r.timing.clone(),
        )
        .unwrap();
        assert!(recovered.session().physical_stop_pending());
        assert_eq!(recovered.session().state(), DeviceState::Disarmed);
        assert!(recovered.arm(c, a, None).is_err());
        assert_eq!(
            recovered.complete_play(play, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
    }
    #[test]
    fn saved_acknowledgment_never_restores_admission_or_physical_proof() {
        for outcome in [DispatchOutcome::Acknowledged, DispatchOutcome::Unknown] {
            let dir = tempfile::tempdir().unwrap();
            let (mut r, a, c) = fixture();
            let mut journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                    .unwrap();
            let play = start(&mut r, &a, &c);
            let receipt = journal.prepare(&r, &play).unwrap();
            journal.complete(&receipt, outcome).unwrap();
            journal.complete(&receipt, outcome).unwrap();
            let changed = if outcome == DispatchOutcome::Acknowledged {
                DispatchOutcome::Unknown
            } else {
                DispatchOutcome::Acknowledged
            };
            assert!(journal.complete(&receipt, changed).is_err());
            drop(journal);
            let journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-second").unwrap())
                    .unwrap();
            assert_eq!(
                journal.history(&a.configuration.device, 10).unwrap()[0].outcome,
                Some(outcome)
            );
            let old_epoch = r.session().session().clone();
            let fresh = DeviceSessionId::new("fresh-host-session").unwrap();
            let mut recovered = HandyPlaybackRuntime::recover_unconfirmed(
                a.configuration.device.clone(),
                fresh.clone(),
                r.timing.clone(),
            )
            .unwrap();
            assert!(recovered.arm(c.clone(), a.clone(), None).is_err());
            assert!(recovered
                .confirm_stopped(&old_epoch, StopEvidence::DeviceObservedStopped)
                .is_err());
            assert!(recovered
                .confirm_stopped(
                    &fresh,
                    StopEvidence::QualifiedBoundElapsed {
                        elapsed_ns: u64::MAX
                    }
                )
                .is_err());
            recovered
                .confirm_stopped(&fresh, StopEvidence::DeviceObservedStopped)
                .unwrap();
            recovered.arm(c, a, None).unwrap();
            assert_eq!(recovered.session().state(), DeviceState::Armed);
        }
    }
    #[test]
    fn newer_boot_fences_old_prepare_and_completion_without_erasing_history() {
        let dir = tempfile::tempdir().unwrap();
        let (mut r, a, c) = fixture();
        let play = start(&mut r, &a, &c);
        let mut old =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                .unwrap();
        let receipt = old.prepare(&r, &play).unwrap();
        let fresh =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-second").unwrap())
                .unwrap();
        assert!(old.prepare(&r, &play).is_err());
        assert!(old
            .complete(&receipt, DispatchOutcome::Acknowledged)
            .is_err());
        assert!(fresh.history(&a.configuration.device, 10).unwrap()[0]
            .outcome
            .is_none());
    }
    #[test]
    fn failed_durable_preparation_returns_no_play_effect_and_preserves_uncertainty() {
        let dir = tempfile::tempdir().unwrap();
        let (mut r, a, c) = fixture();
        let mut old =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                .unwrap();
        let _new =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-second").unwrap())
                .unwrap();
        let pending = r.prepare_start(&c, clock(), 0).unwrap();
        assert_eq!(
            r.dispatch_start_durable(pending, &a, &c, &clock().domain, 100_000_000, &mut old)
                .unwrap_err(),
            PlaybackFailure::JournalUnavailable
        );
        assert_eq!(r.session().state(), DeviceState::StopUnconfirmed);
        assert!(r.session().physical_stop_pending());
        assert!(r.request_stop(&c).unwrap().is_some());
    }
    #[test]
    fn journal_identity_uses_immutable_effect_facts_after_revocation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut r, a, c) = fixture();
        let _play = start(&mut r, &a, &c);
        let stop = r.request_stop(&c).unwrap().unwrap();
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                .unwrap();
        let receipt = journal.prepare(&r, &stop).unwrap();
        r.revoke_controller().unwrap();
        let retry = journal.prepare(&r, &stop).unwrap();
        journal
            .complete(&retry, DispatchOutcome::Acknowledged)
            .unwrap();
        journal
            .complete(&receipt, DispatchOutcome::Acknowledged)
            .unwrap();
        let history = journal.history(&a.configuration.device, 10).unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].record.stop);
        assert_eq!(history[0].record.controller, Some(c));
        r.complete_stop(stop, DispatchOutcome::Acknowledged)
            .unwrap();
        assert!(r.session().physical_stop_pending());
    }
    #[test]
    fn future_schema_and_malformed_layout_are_refused_without_repair() {
        for mutation in [
            "PRAGMA user_version=13",
            "ALTER TABLE effects RENAME TO moved_effects",
            "DELETE FROM owner",
        ] {
            let dir = tempfile::tempdir().unwrap();
            drop(
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                    .unwrap(),
            );
            let path = journal_path(dir.path());
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(mutation).unwrap();
            drop(db);
            let before = std::fs::read(&path).unwrap();
            assert!(DeviceEffectJournal::open(
                dir.path(),
                DeviceSessionId::new("boot-second").unwrap()
            )
            .is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }
    #[test]
    fn corrupt_and_oversized_records_are_not_returned_as_history() {
        for value in ["{}".to_string(), "x".repeat(9000)] {
            let dir = tempfile::tempdir().unwrap();
            let (mut r, a, c) = fixture();
            let play = start(&mut r, &a, &c);
            let mut journal =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                    .unwrap();
            let receipt = journal.prepare(&r, &play).unwrap();
            let db = rusqlite::Connection::open(journal_path(dir.path())).unwrap();
            db.execute("UPDATE effects SET record=?1", [value]).unwrap();
            assert!(journal.history(&a.configuration.device, 10).is_err());
            assert!(journal
                .complete(&receipt, DispatchOutcome::Acknowledged)
                .is_err());
            assert!(journal.history(&a.configuration.device, 0).is_err());
            assert!(journal.history(&a.configuration.device, 129).is_err());
        }
    }
    #[test]
    fn journal_capacity_refuses_handoff_without_erasing_prior_records() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let (mut r, a, c) = fixture();
        let play = start(&mut r, &a, &c);
        let mut journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot-first").unwrap())
                .unwrap();
        journal.prepare(&r, &play).unwrap();
        let template = journal.history(&a.configuration.device, 1).unwrap()[0]
            .record
            .clone();
        // Synthetic persisted declarations fill the metadata quota; they do
        // not assert that these physical effects occurred or were qualified.
        let mut db = rusqlite::Connection::open(journal_path(dir.path())).unwrap();
        let tx = db.transaction().unwrap();
        for n in 0..1023 {
            let mut record = template.clone();
            record.device_session = DeviceSessionId::new("synthetic-history-session").unwrap();
            record.sequence = 100 + n;
            let text = serde_json::to_string(&record).unwrap();
            let id = format!("{:x}", Sha256::digest(text.as_bytes()));
            tx.execute(
                "INSERT INTO effects(id,device,record,outcome) VALUES(?1,?2,?3,'unknown')",
                rusqlite::params![id, a.configuration.device.as_str(), text],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        assert!(matches!(
            r.request_stop_durable(&c, &mut journal),
            Err(PlaybackFailure::JournalUnavailable)
        ));
        assert_eq!(r.session().state(), DeviceState::StopUnconfirmed);
        assert!(r.session().physical_stop_pending());
        confirm(&mut r, StopEvidence::DeviceObservedStopped).unwrap();
        r.arm(c.clone(), a.clone(), None).unwrap();
        let pending = r.prepare_start(&c, clock(), 0).unwrap();
        assert_eq!(
            r.dispatch_start_durable(pending, &a, &c, &clock().domain, 100_000_000, &mut journal)
                .unwrap_err(),
            PlaybackFailure::JournalUnavailable
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM effects", [], |row| row
                .get::<_, u64>(0))
                .unwrap(),
            1024
        );
        assert!(r.session().physical_stop_pending());
        assert_eq!(r.session().state(), DeviceState::StopUnconfirmed);
    }
    fn id(s: &str) -> ArtifactId {
        ArtifactId::new(s).unwrap()
    }
    pub(crate) fn fixture() -> (HandyPlaybackRuntime, PlaybackAdmission, ClientId) {
        let configuration = DeviceConfiguration {
            device: DeviceId::new("synthetic-handy").unwrap(),
            firmware: id("firmware-fixture"),
            driver: id("driver-fixture"),
            transport: id("transport-fixture"),
            profile: id("profile-fixture"),
        };
        // Synthetic qualified facts exercise conditional software behavior;
        // this constructor call is not actual hardware qualification evidence.
        let qualification = DeviceQualification::new(
            configuration.clone(),
            id("synthetic-record"),
            QualificationAuthority::LocalHumanAcceptance,
            Some(7_000_000),
        )
        .unwrap();
        let timing =
            QualifiedCommandTiming::new(qualification, 10_000_000, 1_000_000, 100).unwrap();
        let mut runtime = HandyPlaybackRuntime::new(
            configuration.device.clone(),
            DeviceSessionId::new("host-epoch-fixture").unwrap(),
            timing,
        )
        .unwrap();
        let admission = PlaybackAdmission {
            project: ProjectId::new("project-fixture").unwrap(),
            revision: RevisionId::new(4),
            configuration,
            program_artifact: id("program-fixture"),
        };
        let controller = ClientId::new("owner-fixture").unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), None)
            .unwrap();
        (runtime, admission, controller)
    }
    fn confirm(
        runtime: &mut HandyPlaybackRuntime,
        evidence: StopEvidence,
    ) -> Result<(), PlaybackFailure> {
        let session = runtime.session().session().clone();
        runtime.confirm_stopped(&session, evidence)
    }
    pub(crate) fn clock() -> PlaybackClock {
        PlaybackClock {
            domain: id("clock-epoch-fixture"),
            monotonic_ns: 100_000_000,
            valid_until_ns: u64::MAX,
            estimated_server_ms: 1_797_000_000_000,
            uncertainty_ns: 100_000,
        }
    }
    #[test]
    fn reanchored_clock_cannot_extend_its_original_source_expiry() {
        let (mut r, a, c) = fixture();
        let mut mapping = clock();
        let deadline = mapping.monotonic_ns + 10;
        mapping.valid_until_ns = deadline;
        mapping.monotonic_ns = deadline - 1;
        let command = r.prepare_start(&c, mapping.clone(), 1234).unwrap();
        assert_eq!(
            r.dispatch_start(command, &a, &c, &mapping.domain, deadline + 1)
                .unwrap_err(),
            PlaybackFailure::StaleCommand
        );
    }
    fn start(r: &mut HandyPlaybackRuntime, a: &PlaybackAdmission, c: &ClientId) -> PlaybackEffect {
        let command = r.prepare_start(c, clock(), 1234).unwrap();
        r.dispatch_start(
            command,
            a,
            c,
            &clock().domain,
            clock().monotonic_ns + 2_000_000,
        )
        .unwrap()
    }
    #[test]
    fn exact_admission_clock_and_start_are_encoded_before_transport() {
        let (mut r, a, c) = fixture();
        let effect = start(&mut r, &a, &c);
        assert_eq!(
            effect.request().body().unwrap(),
            br#"{"start_time":1234,"server_time":1797000000002,"playback_rate":1.0,"loop":false}"#
        );
        assert!(r.effect_is_current(&effect));
        assert!(r.session().physical_stop_pending());
        assert_eq!(r.session().admission(), Some(&a));
        assert!(r
            .complete_play(effect.clone(), DispatchOutcome::Acknowledged)
            .unwrap()
            .is_none());
        assert_eq!(
            r.complete_play(effect, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert_eq!(r.session().state(), DeviceState::Playing);
    }
    #[test]
    fn referenced_start_preserves_original_pair_and_clock_when_dispatch_is_delayed() {
        let (mut r, a, c) = fixture();
        let original = clock();
        let pending = r
            .prepare_referenced_start(&c, original.clone(), 1234, 0)
            .unwrap();
        let effect = r
            .dispatch_start(
                pending,
                &a,
                &c,
                &original.domain,
                original.monotonic_ns + 2_000_000,
            )
            .unwrap();
        assert_eq!(
            effect.request().body().unwrap(),
            br#"{"start_time":1234,"server_time":1797000000000,"playback_rate":1.0,"loop":false}"#
        );
        assert_eq!(effect.retained_clock(), Some(&original));
        assert!(effect.is_referenced_start());
        // Later checks age the original immutable reference, not a newly
        // assigned timestamp or a server-only reanchor.
        assert_eq!(
            r.timing
                .checked_clock_age(
                    effect.retained_clock().unwrap(),
                    &original.domain,
                    original.monotonic_ns + 4_000_000
                )
                .unwrap(),
            4_000_000
        );
        assert_eq!(effect.retained_clock(), Some(&original));
    }
    #[test]
    fn referenced_start_refuses_original_expiry_reversal_and_late_uncertainty() {
        for (delay, changed, expected) in [
            (10_000_001i64, false, PlaybackFailure::StaleCommand),
            (-1, false, PlaybackFailure::InvalidClock),
            (1_000_000, true, PlaybackFailure::InvalidClock),
        ] {
            let (mut r, a, c) = fixture();
            let mut original = clock();
            if changed {
                original.uncertainty_ns = 999_999;
            }
            let pending = r
                .prepare_referenced_start(&c, original.clone(), 1234, 0)
                .unwrap();
            let now = u64::try_from(i128::from(original.monotonic_ns) + i128::from(delay)).unwrap();
            assert_eq!(
                r.dispatch_start(pending, &a, &c, &original.domain, now),
                Err(expected)
            );
            assert_eq!(r.session().state(), DeviceState::Disarmed);
            assert!(!r.session().physical_stop_pending());
        }
        let (mut r, a, c) = fixture();
        let mut original = clock();
        original.valid_until_ns = original.monotonic_ns + 1;
        let pending = r
            .prepare_referenced_start(&c, original.clone(), 1234, 0)
            .unwrap();
        assert_eq!(
            r.dispatch_start(pending, &a, &c, &original.domain, original.monotonic_ns + 2),
            Err(PlaybackFailure::StaleCommand)
        );
    }
    #[test]
    fn referenced_pending_mode_and_controller_are_part_of_the_single_use_token() {
        let (mut r, a, c) = fixture();
        let pending = r.prepare_referenced_start(&c, clock(), 1234, 0).unwrap();
        let mut changed = pending.clone();
        changed.reference = StartReference::FixedOffset;
        assert_eq!(
            r.dispatch_start(changed, &a, &c, &clock().domain, clock().monotonic_ns),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert!(r.request_stop(&c).unwrap().is_none());
        assert_eq!(
            r.dispatch_start(pending, &a, &c, &clock().domain, clock().monotonic_ns),
            Err(PlaybackFailure::RetiredCommand)
        );
    }
    #[test]
    fn referenced_media_progression_error_is_checked_without_changing_manual_drift() {
        let (mut r, a, c) = fixture();
        let before = r.timing.drift_ppm;
        let pending = r
            .prepare_referenced_start(&c, clock(), 1234, 500_000)
            .unwrap();
        assert_eq!(
            r.dispatch_start(
                pending,
                &a,
                &c,
                &clock().domain,
                clock().monotonic_ns + 2_000_000
            ),
            Err(PlaybackFailure::InvalidClock)
        );
        assert_eq!(r.timing.drift_ppm, before);
        assert_eq!(r.session().state(), DeviceState::Disarmed);
        let (mut r, a, c) = fixture();
        let pending = r.prepare_start(&c, clock(), 1234).unwrap();
        assert!(r
            .dispatch_start(
                pending,
                &a,
                &c,
                &clock().domain,
                clock().monotonic_ns + 2_000_000
            )
            .is_ok());
        let (mut r, _, c) = fixture();
        assert!(r
            .prepare_referenced_start(&c, clock(), 1234, 1_000_000)
            .is_err());
        assert!(r.pending.is_none());
        r.timing.drift_ppm = u32::MAX;
        assert!(r.prepare_referenced_start(&c, clock(), 1234, 1).is_err());
        assert!(r.pending.is_none());
    }
    #[test]
    fn stop_before_dispatch_retires_queue_without_a_physical_effect() {
        let (mut r, a, c) = fixture();
        let pending = r.prepare_start(&c, clock(), 0).unwrap();
        assert!(r.request_stop(&c).unwrap().is_none());
        assert_eq!(r.session().state(), DeviceState::Disarmed);
        assert!(!r.session().physical_stop_pending());
        assert_eq!(
            r.dispatch_start(pending, &a, &c, &clock().domain, 100_000_000),
            Err(PlaybackFailure::RetiredCommand)
        );
    }
    #[test]
    fn cancellation_cannot_erase_uncertainty_after_a_play_effect_is_issued() {
        let (mut r, a, c) = fixture();
        let _play = start(&mut r, &a, &c);
        assert!(r.session.disarm_before_dispatch().is_err());
        let _stop = r.request_stop(&c).unwrap();
        assert!(r.session.disarm_before_dispatch().is_err());
        assert!(r.session().physical_stop_pending());
        assert_eq!(r.session().state(), DeviceState::StopRequested);
    }
    #[test]
    fn stale_backward_and_changed_clock_retire_admission() {
        for (now, domain, error) in [
            (
                110_000_001,
                id("clock-epoch-fixture"),
                PlaybackFailure::StaleCommand,
            ),
            (
                99_999_999,
                id("clock-epoch-fixture"),
                PlaybackFailure::InvalidClock,
            ),
            (
                100_000_000,
                id("reset-clock"),
                PlaybackFailure::InvalidClock,
            ),
        ] {
            let (mut r, a, c) = fixture();
            let pending = r.prepare_start(&c, clock(), 0).unwrap();
            assert_eq!(r.dispatch_start(pending, &a, &c, &domain, now), Err(error));
            assert_eq!(r.session().state(), DeviceState::Disarmed);
            assert!(!r.session().physical_stop_pending());
        }
    }
    #[test]
    fn drift_and_submillisecond_quantization_count_toward_uncertainty() {
        let (mut r, a, c) = fixture();
        let mut mapping = clock();
        mapping.uncertainty_ns = 999_999;
        let pending = r.prepare_start(&c, mapping, 0).unwrap();
        assert_eq!(
            r.dispatch_start(pending, &a, &c, &clock().domain, 101_000_000),
            Err(PlaybackFailure::InvalidClock)
        );
        let (mut r, a, c) = fixture();
        let mut mapping = clock();
        mapping.uncertainty_ns = 500_000;
        let pending = r.prepare_start(&c, mapping, 0).unwrap();
        assert_eq!(
            r.dispatch_start(pending, &a, &c, &clock().domain, 100_500_000),
            Err(PlaybackFailure::InvalidClock)
        );
    }
    #[test]
    fn changed_revision_program_configuration_or_controller_cannot_dispatch() {
        for change in 0..4 {
            let (mut r, mut a, c) = fixture();
            let pending = r.prepare_start(&c, clock(), 0).unwrap();
            let mut controller = c.clone();
            match change {
                0 => a.revision = RevisionId::new(5),
                1 => a.program_artifact = id("changed-master"),
                2 => a.configuration.profile = id("changed-profile"),
                _ => controller = ClientId::new("different-owner").unwrap(),
            }
            assert!(r
                .dispatch_start(pending, &a, &controller, &clock().domain, 100_000_000)
                .is_err());
            assert_eq!(r.session().state(), DeviceState::Disarmed);
            assert!(!r.session().physical_stop_pending());
        }
    }
    #[test]
    fn late_play_ack_cannot_restore_authority_after_stop() {
        let (mut r, a, c) = fixture();
        let play = start(&mut r, &a, &c);
        let stop = r.request_stop(&c).unwrap().unwrap();
        assert!(!r.effect_is_current(&play));
        assert!(r.effect_is_current(&stop));
        assert_eq!(stop.request().path(), "/hssp/stop");
        assert!(stop.request().body().is_none());
        assert_eq!(
            r.complete_play(play, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        r.complete_stop(stop, DispatchOutcome::Acknowledged)
            .unwrap();
        assert_eq!(r.session().state(), DeviceState::StopUnconfirmed);
        assert!(r.session().physical_stop_pending());
        assert!(r.arm(c, a, None).is_err());
        assert!(confirm(
            &mut r,
            StopEvidence::QualifiedBoundElapsed {
                elapsed_ns: 6_999_999
            }
        )
        .is_err());
        confirm(
            &mut r,
            StopEvidence::QualifiedBoundElapsed {
                elapsed_ns: 7_000_000,
            },
        )
        .unwrap();
        assert_eq!(r.session().state(), DeviceState::Disarmed);
    }
    #[test]
    fn unknown_play_issues_one_stop_and_no_retry() {
        let (mut r, a, c) = fixture();
        let play = start(&mut r, &a, &c);
        let stop = r
            .complete_play(play.clone(), DispatchOutcome::Unknown)
            .unwrap()
            .unwrap();
        assert_eq!(
            r.complete_play(play, DispatchOutcome::Unknown),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert!(r.request_stop(&c).unwrap().is_none());
        r.complete_stop(stop.clone(), DispatchOutcome::Unknown)
            .unwrap();
        assert_eq!(r.session().state(), DeviceState::StopUnconfirmed);
        let later = r.request_stop(&c).unwrap().unwrap();
        assert_ne!(later, stop);
        assert_eq!(
            r.complete_stop(stop, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert!(r.effect_is_current(&later));
    }
    #[test]
    fn reconnect_retires_callbacks_and_preserves_physical_uncertainty() {
        let (mut r, a, c) = fixture();
        let old_epoch = r.session().session().clone();
        let play = start(&mut r, &a, &c);
        let stop = r.request_stop(&c).unwrap().unwrap();
        r.reconnect(DeviceSessionId::new("new-host-epoch").unwrap())
            .unwrap();
        assert_eq!(r.session().state(), DeviceState::Disarmed);
        assert!(r.session().physical_stop_pending());
        assert_eq!(
            r.complete_play(play, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert_eq!(
            r.complete_stop(stop, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert!(r.arm(c.clone(), a.clone(), None).is_err());
        assert!(r
            .confirm_stopped(&old_epoch, StopEvidence::DeviceObservedStopped)
            .is_err());
        assert!(r.session().physical_stop_pending());
        assert!(confirm(
            &mut r,
            StopEvidence::QualifiedBoundElapsed {
                elapsed_ns: u64::MAX
            }
        )
        .is_err());
        confirm(&mut r, StopEvidence::DeviceObservedStopped).unwrap();
        r.arm(c, a, None).unwrap();
        assert_eq!(r.session().state(), DeviceState::Armed);
    }
    #[test]
    fn disconnect_stops_ordinary_playback_and_revocation_always_stops_headless() {
        let (mut r, a, c) = fixture();
        let play = start(&mut r, &a, &c);
        assert!(r.controller_lost().unwrap().is_some());
        assert!(!r.effect_is_current(&play));
        let (mut r, a, c) = fixture();
        r.session.disarm_before_dispatch().unwrap();
        let approval = HeadlessApproval::new(&r.timing.qualification).unwrap();
        r.arm(c.clone(), a.clone(), Some(&approval)).unwrap();
        let play = start(&mut r, &a, &c);
        assert!(r.controller_lost().unwrap().is_none());
        assert!(r.effect_is_current(&play));
        let stop = r.revoke_controller().unwrap().unwrap();
        assert!(!r.effect_is_current(&play));
        assert!(r.effect_is_current(&stop));
    }
    #[test]
    fn retired_queue_cannot_consume_fresh_admission_or_forge_clock_data() {
        let (mut r, a, c) = fixture();
        let old = r.prepare_start(&c, clock(), 0).unwrap();
        r.request_stop(&c).unwrap();
        r.arm(c.clone(), a.clone(), None).unwrap();
        let pending = r.prepare_start(&c, clock(), 0).unwrap();
        assert_eq!(
            r.dispatch_start(old, &a, &c, &clock().domain, 100_000_000),
            Err(PlaybackFailure::RetiredCommand)
        );
        let mut forged = pending.clone();
        forged.clock.estimated_server_ms += 1;
        assert_eq!(
            r.dispatch_start(forged, &a, &c, &clock().domain, 100_000_000),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert!(r
            .dispatch_start(pending, &a, &c, &clock().domain, 100_000_000)
            .is_ok());
    }
    #[test]
    fn zero_bounds_wrong_configuration_and_unqualified_clock_are_refused() {
        let (mut r, mut a, c) = fixture();
        assert!(QualifiedCommandTiming::new(r.timing.qualification.clone(), 0, 1, 0).is_err());
        assert!(QualifiedCommandTiming::new(r.timing.qualification.clone(), 1, 0, 0).is_err());
        let mut mapping = clock();
        mapping.uncertainty_ns = 1_000_001;
        assert!(r.prepare_start(&c, mapping, 0).is_err());
        r.session.disarm_before_dispatch().unwrap();
        a.configuration.firmware = id("different-firmware");
        assert!(r.arm(c, a, None).is_err());
    }
    #[test]
    fn physical_evidence_retires_an_inflight_stop_callback() {
        let (mut r, a, c) = fixture();
        let _play = start(&mut r, &a, &c);
        let stop = r.request_stop(&c).unwrap().unwrap();
        confirm(&mut r, StopEvidence::DeviceObservedStopped).unwrap();
        assert_eq!(
            r.complete_stop(stop, DispatchOutcome::Acknowledged),
            Err(PlaybackFailure::RetiredCommand)
        );
        assert_eq!(r.session().state(), DeviceState::Disarmed);
        assert!(!r.session().physical_stop_pending());
    }
}

#[cfg(test)]
#[path = "playback_runtime/play_completion_tests.rs"]
mod play_completion_tests;
