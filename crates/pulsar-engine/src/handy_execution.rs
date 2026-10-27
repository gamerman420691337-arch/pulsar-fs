//! One independent play/stop owner. No worker retains Engine itself.
use super::*;
use crate::{
    device_effect_journal::{
        ClaimedPlayReceipt, CustodyKind, DeviceEffectJournal, EffectIntent, EffectReceiptBinding,
        PreparedEffectReceipt,
    },
    playback_runtime::{
        dispatch::{
            CompletionObservation, DispatchAttempt, DispatchAuthority, DispatchCompletion,
            DispatchFailure, DispatchRequest, HandyDispatcher,
        },
        EffectCompletionRecord, EmergencyStopObservation, PlaybackEffect, PlaybackFailure,
    },
};
use controller::play::{PlayAuthority, PlayRequest};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub(in crate::authority) struct ExecutionPump {
    stop: Arc<AtomicBool>,
    commands: SyncSender<DrainRequest>,
    thread: Option<JoinHandle<PResult<()>>>,
}
/// A bounded command capability, not ownership of the worker or Engine.
/// Callers release the pump owner's slot before waiting through this handle.
#[derive(Clone)]
pub(in crate::authority) struct ReconciliationHandle {
    stop: Arc<AtomicBool>,
    commands: SyncSender<DrainRequest>,
    wake: thread::Thread,
}
struct DrainRequest {
    ids: Vec<ArtifactId>,
    stage: DrainStage,
    deadline: Instant,
    abandoned: Arc<AtomicBool>,
    reply: SyncSender<PResult<()>>,
}
#[derive(Clone, Copy)]
enum DrainStage {
    Quiesce,
    Finish,
}
fn drain_wait(deadline: Instant, cancelled: bool) -> PResult<()> {
    if cancelled {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "Handy reconciliation drain cancelled; unresolved work remains owned",
        ));
    }
    if Instant::now() >= deadline {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "Handy reconciliation drain expired; unresolved work remains owned",
        ));
    }
    Ok(())
}
struct Resources {
    shutdown_requested: Arc<AtomicBool>,
    registry: Arc<Mutex<Registry>>,
    journal: Arc<Mutex<DeviceEffectJournal>>,
    db: Arc<Mutex<Connection>>,
    pool: ResourcePool,
    binding: EffectReceiptBinding,
}
struct Stop {
    effect: PlaybackEffect,
    intent: Option<EffectIntent>,
    receipt: Option<PreparedEffectReceipt>,
    submitted: bool,
    finished: bool,
    activity: Option<HandyStopActivity>,
    rejected: bool,
}
impl Stop {
    fn new(effect: PlaybackEffect) -> Self {
        Self {
            effect,
            intent: None,
            receipt: None,
            submitted: false,
            finished: false,
            activity: None,
            rejected: false,
        }
    }
}
struct Play {
    request: Option<PlayRequest>,
    claimed: Option<ClaimedPlayReceipt>,
    attempted: bool,
    finished: bool,
}
struct PlayObservation {
    record: EffectCompletionRecord,
    claimed: ClaimedPlayReceipt,
}
struct Work {
    id: ArtifactId,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    pending: Arc<AtomicBool>,
    activity: Arc<Mutex<Option<HandyStopActivity>>>,
    dispatcher: HandyDispatcher,
    play: Option<Play>,
    stop: Option<Stop>,
    inflight: bool,
    completion: Option<DispatchCompletion>,
    play_observation: Option<PlayObservation>,
    stop_observation: Option<EffectCompletionRecord>,
    emergency_observation: Option<EmergencyStopObservation>,
    rejected: Vec<DispatchCompletion>,
    rejected_metadata_recorded: bool,
    failure: bool,
    retire: bool,
}
enum StopObservation {
    Durable(EffectCompletionRecord),
    Emergency(EmergencyStopObservation),
}
fn stop_outcome(value: crate::playback_runtime::DispatchOutcome) -> HandyStopOutcome {
    match value {
        crate::playback_runtime::DispatchOutcome::Acknowledged => HandyStopOutcome::Acknowledged,
        crate::playback_runtime::DispatchOutcome::Unknown => HandyStopOutcome::Unknown,
    }
}
impl Work {
    fn begin_drain(&mut self) {
        if let Some(play) = &mut self.play {
            if !play.attempted {
                // Retirement already removed authority. Leave the durable original
                // preparation intact for separate cancellation/reconciliation.
                play.request = None;
                play.attempted = true;
                play.finished = true;
            } else if !play.finished && self.inflight {
                self.dispatcher.cancel();
            }
        }
    }
    fn reconciliation_quiescent(&self) -> bool {
        !self.inflight
            && self.completion.is_none()
            && self.play_observation.is_none()
            && self.stop_observation.is_none()
            && self.emergency_observation.is_none()
            && !self.retire
            && self.play.as_ref().is_none_or(|p| p.finished)
            && self.stop.as_ref().is_none_or(|s| s.finished)
    }
    fn register_rejected_metadata(&mut self, resources: &Resources) -> bool {
        if self.rejected_metadata_recorded {
            return true;
        }
        for completion in &self.rejected {
            if !matches!(completion.attempt, DispatchAttempt::Rejected(_)) {
                return false;
            }
            if !matches!(completion.request, DispatchRequest::EmergencyStop(_)) {
                continue;
            }
            let Some(intent) = self.stop.as_ref().and_then(|s| s.intent.as_ref()) else {
                return false;
            };
            if resources
                .binding
                .validate_intent(completion.request.effect(), intent)
                .is_err()
            {
                return false;
            }
            let Ok(mut journal) = resources.journal.try_lock() else {
                return false;
            };
            if journal.try_register_rejected_stop(intent).is_err() {
                return false;
            }
        }
        // This registers unresolved original metadata only. A local rejection
        // remains a rejection; no transport outcome or receipt is created.
        self.rejected_metadata_recorded = true;
        true
    }
    fn stop_phase(
        &mut self,
        phase: HandyStopPhase,
        outcome: Option<crate::playback_runtime::DispatchOutcome>,
        recorded: bool,
    ) {
        if let Some(activity) = self.stop.as_mut().and_then(|s| s.activity.as_mut()) {
            activity.phase = phase;
            activity.outcome = outcome.map(stop_outcome);
            activity.metadata_recorded = recorded;
        }
    }
    fn publish_activity(&self) -> bool {
        let Ok(mut slot) = self.activity.try_lock() else {
            return false;
        };
        *slot = self.stop.as_ref().and_then(|s| s.activity.clone());
        true
    }
    fn retire(&mut self, resources: &Resources) {
        if !self.retire {
            return;
        }
        if let Ok(mut registry) = resources.registry.try_lock() {
            if let Some(e) = registry.entries.get_mut(&self.id) {
                controller::retire_entry(e);
            }
            self.retire = false;
        }
    }
    fn observe(&mut self, binding: &EffectReceiptBinding) {
        let Some(completion) = &self.completion else {
            return;
        };
        let is_play = matches!(completion.request, DispatchRequest::Play { .. });
        let is_emergency = matches!(completion.request, DispatchRequest::EmergencyStop(_));
        if matches!(completion.attempt, DispatchAttempt::Rejected(_)) {
            if is_play {
                self.retire = true;
                if let Some(p) = &mut self.play {
                    p.finished = true;
                }
            } else if let Some(s) = &mut self.stop {
                s.finished = true;
                s.rejected = true;
                self.stop_phase(HandyStopPhase::Rejected, None, false);
            }
            self.rejected
                .push(self.completion.take().expect("checked completion"));
            self.rejected_metadata_recorded = false;
            self.failure = true;
            return;
        }
        let Ok(mut runtime) = self.runtime.try_lock() else {
            return;
        };
        let observed = completion.observe(&mut runtime, binding);
        let result = match observed {
            Ok(CompletionObservation::Play(value)) => {
                match value.stop {
                    Ok(Some(stop)) => {
                        self.stop = Some(Stop::new(stop));
                        self.retire = true;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.failure = true;
                        self.retire = true;
                    }
                }
                Ok(StopObservation::Durable(value.observation))
            }
            Ok(CompletionObservation::Stop(value)) => Ok(StopObservation::Durable(*value)),
            Ok(CompletionObservation::EmergencyStop(value)) => {
                Ok(StopObservation::Emergency(*value))
            }
            Err(DispatchFailure::Authority(PlaybackFailure::RetiredCommand)) => {
                if is_emergency {
                    completion
                        .historical_emergency_observation(binding)
                        .map(StopObservation::Emergency)
                } else {
                    completion
                        .historical_observation(binding)
                        .map(StopObservation::Durable)
                }
            }
            Err(error) => Err(error),
        };
        drop(runtime);
        match result {
            Ok(StopObservation::Durable(record)) if is_play => {
                let Some(play) = &mut self.play else {
                    self.failure = true;
                    return;
                };
                let Some(claimed) = play.claimed.take() else {
                    self.failure = true;
                    return;
                };
                play.finished = true;
                self.play_observation = Some(PlayObservation { record, claimed });
                self.completion = None;
            }
            Ok(StopObservation::Durable(record)) => {
                if let Some(stop) = &mut self.stop {
                    stop.finished = true;
                }
                self.stop_phase(HandyStopPhase::Observed, Some(record.outcome()), false);
                self.stop_observation = Some(record);
                self.completion = None;
            }
            Ok(StopObservation::Emergency(record)) => {
                if let Some(stop) = &mut self.stop {
                    stop.finished = true;
                }
                self.stop_phase(HandyStopPhase::Observed, Some(record.outcome()), false);
                self.emergency_observation = Some(record);
                self.completion = None;
            }
            Err(_) => {
                self.failure = true;
                self.retire |= is_play;
            }
        }
    }
    fn persist(&mut self, resources: &Resources) {
        if let Some(observed) = &mut self.play_observation {
            if let Ok(mut journal) = resources.journal.try_lock() {
                if observed
                    .record
                    .try_persist_play_operation(&observed.claimed, &mut journal)
                    .is_ok()
                {
                    self.play_observation = None;
                }
            }
        }
        if let Some(observed) = &mut self.stop_observation {
            if let Ok(mut journal) = resources.journal.try_lock() {
                if observed.try_persist(&mut journal).is_ok() {
                    let outcome = observed.outcome();
                    self.stop_observation = None;
                    self.stop_phase(HandyStopPhase::Observed, Some(outcome), true);
                }
            }
        }
        if let Some(observed) = &mut self.emergency_observation {
            if let Ok(mut journal) = resources.journal.try_lock() {
                if observed.try_persist(&mut journal).is_ok() {
                    let outcome = observed.outcome();
                    self.emergency_observation = None;
                    self.stop_phase(HandyStopPhase::Observed, Some(outcome), true);
                }
            }
        }
    }
    fn submit_stop(&mut self, resources: &Resources) {
        let Some(stop) = &mut self.stop else {
            return;
        };
        if stop.submitted {
            return;
        }
        if stop.intent.is_none() {
            let Ok(runtime) = self.runtime.try_lock() else {
                return;
            };
            match resources.binding.capture_intent(&runtime, &stop.effect) {
                Ok(intent) => {
                    let Ok(id) = intent
                        .record_id()
                        .and_then(|id| ArtifactId::new(id).map_err(Into::into))
                    else {
                        self.failure = true;
                        stop.submitted = true;
                        stop.finished = true;
                        stop.rejected = true;
                        return;
                    };
                    stop.activity = Some(HandyStopActivity {
                        record_id: id,
                        emergency: false,
                        phase: HandyStopPhase::Pending,
                        outcome: None,
                        metadata_recorded: false,
                    });
                    stop.intent = Some(intent);
                }
                Err(_) => {
                    self.failure = true;
                    stop.submitted = true;
                    stop.finished = true;
                    stop.rejected = true;
                    return;
                }
            }
        }
        // The same captured native stop remains eligible when persistence cannot
        // confirm its prewrite. A positively observed foreign owner still refuses.
        if let Ok(mut journal) = resources.journal.try_lock() {
            match journal.try_prepare_intent(stop.intent.as_ref().expect("captured intent")) {
                Ok(receipt) => stop.receipt = Some(receipt),
                Err(_) if journal.current_owner().is_ok_and(|current| !current) => {
                    self.failure = true;
                    stop.submitted = true;
                    stop.finished = true;
                    stop.rejected = true;
                    if let Some(activity) = &mut stop.activity {
                        activity.phase = HandyStopPhase::Rejected;
                    }
                    return;
                }
                Err(_) => {}
            }
        }
        let request = {
            let Ok(runtime) = self.runtime.try_lock() else {
                return;
            };
            let prepared = if let Some(receipt) = &stop.receipt {
                runtime
                    .attach_stop_receipt(stop.effect.clone(), receipt.clone(), &resources.binding)
                    .map(DispatchRequest::Stop)
            } else {
                runtime
                    .prepare_emergency_stop(
                        stop.effect.clone(),
                        stop.intent.as_ref().expect("captured intent").clone(),
                        &resources.binding,
                    )
                    .map(|stop| DispatchRequest::EmergencyStop(Box::new(stop)))
            };
            match prepared {
                Ok(request) => request,
                Err(_) => {
                    self.failure = true;
                    stop.submitted = true;
                    stop.finished = true;
                    stop.rejected = true;
                    if let Some(activity) = &mut stop.activity {
                        activity.phase = HandyStopPhase::Rejected;
                    }
                    return;
                }
            }
        };
        let admission = stop.effect.admission().clone();
        stop.submitted = true;
        if let Some(activity) = &mut stop.activity {
            activity.emergency = matches!(&request, DispatchRequest::EmergencyStop(_));
            activity.phase = HandyStopPhase::InFlight;
        }
        let result = self.dispatcher.start(
            request,
            Instant::now() + Duration::from_secs(10),
            move |_, _, gate| {
                gate.apply(DispatchAuthority {
                    admission,
                    controller: None,
                    clock_domain: ArtifactId::new("stop-without-play-clock").expect("constant ID"),
                    observed_tick_ns: 0,
                    lease_deadline_ns: 0,
                })
            },
        );
        self.start_result(result);
    }
    fn start_result(
        &mut self,
        result: Result<(), Box<crate::playback_runtime::dispatch::StartFailure>>,
    ) {
        match result {
            Ok(()) => self.inflight = true,
            Err(e) => {
                self.completion = Some(DispatchCompletion {
                    request: e.request,
                    attempt: DispatchAttempt::Rejected(e.reason),
                })
            }
        }
    }
    fn submit_play(&mut self, resources: &Resources) {
        let Some(play) = &mut self.play else {
            return;
        };
        if play.attempted {
            return;
        }
        let Ok(mut journal) = resources.journal.try_lock() else {
            return;
        };
        let request = play.request.take().expect("unattempted play request");
        play.attempted = true;
        let claimed = match journal.try_claim_play(request.prepared) {
            Ok(claimed) => claimed,
            Err(_) => {
                play.finished = true;
                self.failure = true;
                self.retire = true;
                return;
            }
        };
        drop(journal);
        let effect = request.effect;
        let receipt = claimed.effect_receipt().clone();
        play.claimed = Some(claimed);
        let authority = PlayAuthority {
            shutdown_requested: resources.shutdown_requested.clone(),
            db: resources.db.clone(),
            registry: resources.registry.clone(),
            journal: resources.journal.clone(),
            pool: resources.pool.clone(),
            runtime: self.runtime.clone(),
            pins: request.pins,
            request: request.request,
            clock: request.clock,
        };
        let result = self.dispatcher.start(
            DispatchRequest::Play { effect, receipt },
            Instant::now() + Duration::from_secs(10),
            move |end, cancel, gate| authority.claim(end, cancel, gate),
        );
        self.start_result(result);
    }
    fn step(&mut self, resources: &Resources) -> bool {
        if let Some(c) = self.dispatcher.poll() {
            self.inflight = false;
            self.completion = Some(c);
        }
        self.observe(&resources.binding);
        self.persist(resources);
        self.retire(resources);
        if !self.inflight && self.completion.is_none() {
            if self.stop.is_some() {
                // An unclaimed queued play is never sent after retirement. Its
                // original request remains inspectable/cancellable as metadata.
                if let Some(p) = &mut self.play {
                    if !p.attempted {
                        p.attempted = true;
                        p.finished = true;
                        p.request = None;
                    }
                }
                self.submit_stop(resources);
            } else if !self.failure {
                self.submit_play(resources);
            }
        }
        let published = self.publish_activity();
        let done = published
            && self.stop.as_ref().is_some_and(|s| s.finished)
            && !self.inflight
            && self.completion.is_none()
            && self.play_observation.is_none()
            && self.stop_observation.is_none()
            && self.emergency_observation.is_none()
            && self.rejected.is_empty()
            && !self.failure;
        if done {
            self.pending.store(false, Ordering::Release);
        }
        done
    }
    fn quiescent_failure(&self) -> bool {
        self.failure
            && !self.inflight
            && self.completion.is_none()
            && self.play_observation.is_none()
            && self.stop_observation.is_none()
            && self.emergency_observation.is_none()
            && self.stop.as_ref().is_none_or(|s| s.finished)
    }
    fn finish(&mut self, resources: &Resources) {
        if let Some(c) = self.dispatcher.shutdown() {
            self.inflight = false;
            self.completion = Some(c);
        }
        self.observe(&resources.binding);
        self.persist(resources);
        self.publish_activity();
        if !self.failure
            && self.completion.is_none()
            && self.play_observation.is_none()
            && self.stop_observation.is_none()
            && self.emergency_observation.is_none()
            && self.stop.as_ref().is_some_and(|s| s.finished)
        {
            self.pending.store(false, Ordering::Release);
        }
    }
    fn unresolved_stop(&self) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|s| !s.finished || s.rejected)
            || self.stop_observation.is_some()
            || self.emergency_observation.is_some()
            || self
                .completion
                .as_ref()
                .is_some_and(|c| !matches!(c.request, DispatchRequest::Play { .. }))
            || self
                .rejected
                .iter()
                .any(|c| !matches!(c.request, DispatchRequest::Play { .. }))
    }
}
fn collect_entry(
    resources: &Resources,
    work: &mut HashMap<ArtifactId, Work>,
    id: &ArtifactId,
    entry: &mut Entry,
) {
    if let Some(w) = work.get_mut(id) {
        if w.stop.is_none() {
            if let Some(effect) = entry.pending_controller_stop.take() {
                w.stop = Some(Stop::new(effect));
                w.dispatcher.cancel();
            }
        }
        if w.play.is_none() {
            if let Some(request) = entry.pending_play.take() {
                #[cfg(test)]
                w.dispatcher.set_test_clock(request.clock.clone());
                w.play = Some(Play {
                    request: Some(request),
                    claimed: None,
                    attempted: false,
                    finished: false,
                });
            }
        }
        return;
    }
    if entry.pending_play.is_none() && entry.pending_controller_stop.is_none() {
        return;
    }
    entry.execution_pending.store(true, Ordering::Release);
    let play = entry.pending_play.take().map(|request| Play {
        request: Some(request),
        claimed: None,
        attempted: false,
        finished: false,
    });
    let stop = entry.pending_controller_stop.take().map(Stop::new);
    let dispatcher = HandyDispatcher::from_shared(
        entry.runtime.clone(),
        resources.binding.clone(),
        entry.transport.clone(),
    );
    #[cfg(test)]
    let dispatcher = {
        let mut dispatcher = dispatcher;
        if let Some(request) = play.as_ref().and_then(|p| p.request.as_ref()) {
            dispatcher.set_test_clock(request.clock.clone());
        }
        dispatcher
    };
    work.insert(
        id.clone(),
        Work {
            id: id.clone(),
            runtime: entry.runtime.clone(),
            pending: entry.execution_pending.clone(),
            activity: entry.stop_activity.clone(),
            dispatcher,
            play,
            stop,
            inflight: false,
            completion: None,
            play_observation: None,
            stop_observation: None,
            emergency_observation: None,
            rejected: Vec::new(),
            rejected_metadata_recorded: false,
            failure: false,
            retire: false,
        },
    );
}
fn collect_pending(resources: &Resources, work: &mut HashMap<ArtifactId, Work>) {
    if let Ok(mut registry) = resources.registry.try_lock() {
        for (id, entry) in &mut registry.entries {
            collect_entry(resources, work, id, entry);
        }
    }
}
struct DrainEntryIdentity {
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    transport: Arc<HandyTransport>,
    pending: Arc<AtomicBool>,
}
impl DrainEntryIdentity {
    fn capture(entry: &Entry) -> Self {
        Self {
            runtime: entry.runtime.clone(),
            transport: entry.transport.clone(),
            pending: entry.execution_pending.clone(),
        }
    }
    fn matches(&self, entry: &Entry) -> bool {
        Arc::ptr_eq(&self.runtime, &entry.runtime)
            && Arc::ptr_eq(&self.transport, &entry.transport)
            && Arc::ptr_eq(&self.pending, &entry.execution_pending)
    }
}
fn collect_quiescent_targets(
    resources: &Resources,
    work: &mut HashMap<ArtifactId, Work>,
    registry: &mut Registry,
    ids: &[ArtifactId],
) -> PResult<bool> {
    for id in ids {
        if let Some(entry) = registry.entries.get_mut(id) {
            if work.get(id).is_some_and(|w| {
                !Arc::ptr_eq(&w.runtime, &entry.runtime)
                    || !Arc::ptr_eq(&w.pending, &entry.execution_pending)
            }) {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Handy reconciliation execution owner changed",
                ));
            }
            collect_entry(resources, work, id, entry);
            if entry.pending_play.is_some() || entry.pending_controller_stop.is_some() {
                return Ok(false);
            }
            if !work.contains_key(id) && entry.execution_pending.load(Ordering::Acquire) {
                return Ok(false);
            }
        }
        if let Some(w) = work.get_mut(id) {
            w.begin_drain();
            if !w.reconciliation_quiescent() || !w.publish_activity() {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn finish_drain(
    resources: &Resources,
    work: &mut HashMap<ArtifactId, Work>,
    drain: &DrainRequest,
) -> PResult<bool> {
    drain_wait(drain.deadline, drain.abandoned.load(Ordering::Acquire))?;
    let identities = {
        let mut registry = match resources.registry.try_lock() {
            Ok(registry) => registry,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy reconciliation registry unavailable",
                ));
            }
        };
        // An empty Work map cannot establish that retirement handed us its last stop.
        if !collect_quiescent_targets(resources, work, &mut registry, &drain.ids)? {
            return Ok(false);
        }
        drain
            .ids
            .iter()
            .map(|id| registry.entries.get(id).map(DrainEntryIdentity::capture))
            .collect::<Vec<_>>()
    };
    // SQLite access must never retain registry/runtime guards. In particular,
    // another device's stop retirement must progress while this metadata waits.
    if matches!(drain.stage, DrainStage::Quiesce) {
        for id in &drain.ids {
            if let Some(w) = work.get_mut(id) {
                if !w.register_rejected_metadata(resources) {
                    return Ok(false);
                }
            }
        }
    } else {
        let journal = match resources.journal.try_lock() {
            Ok(journal) => journal,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy reconciliation journal unavailable",
                ));
            }
        };
        for id in &drain.ids {
            if let Some(w) = work.get(id) {
                for rejected in &w.rejected {
                    let record = resources
                        .binding
                        .record_for_effect(rejected.request.effect())
                        .map_err(internal)?;
                    let effect_id = format!(
                        "{:x}",
                        Sha256::digest(serde_json::to_vec(&record).map_err(internal)?)
                    );
                    if !journal
                        .is_resolved(CustodyKind::Effect, &effect_id)
                        .map_err(internal)?
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "rejected Handy effect has no durable reconciliation",
                        ));
                    }
                }
            }
        }
    }
    drain_wait(drain.deadline, drain.abandoned.load(Ordering::Acquire))?;
    let mut registry = match resources.registry.try_lock() {
        Ok(registry) => registry,
        Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
        Err(std::sync::TryLockError::Poisoned(_)) => {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Handy reconciliation registry unavailable",
            ));
        }
    };
    for (id, identity) in drain.ids.iter().zip(&identities) {
        let same = match (identity, registry.entries.get(id)) {
            (Some(identity), Some(entry)) => identity.matches(entry),
            (None, None) => true,
            _ => false,
        };
        if !same {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy reconciliation entry changed while checking metadata",
            ));
        }
    }
    // Recollect under the final guard: newly pending work must not disappear
    // between the metadata check and removal of its execution owner.
    if !collect_quiescent_targets(resources, work, &mut registry, &drain.ids)? {
        return Ok(false);
    }
    drain_wait(drain.deadline, drain.abandoned.load(Ordering::Acquire))?;
    for id in &drain.ids {
        if matches!(drain.stage, DrainStage::Quiesce)
            && work.get(id).is_some_and(|w| !w.rejected.is_empty())
        {
            // A local refusal stays owned until durable reconciliation accounts
            // for its original uncertainty; no outcome is invented.
            continue;
        }
        if let Some(w) = work.remove(id) {
            // No dispatcher is active here: poll already joined its worker.
            // The retired Entry and its physical uncertainty remain untouched.
            w.pending.store(false, Ordering::Release);
        }
    }
    Ok(true)
}
fn service_drain(
    resources: &Resources,
    work: &mut HashMap<ArtifactId, Work>,
    drain: &mut Option<DrainRequest>,
) {
    let Some(request) = drain.as_ref() else {
        return;
    };
    let result = match finish_drain(resources, work, request) {
        Ok(false) => return,
        Ok(true) => Ok(()),
        Err(error) => Err(error),
    };
    let request = drain.take().expect("checked drain");
    let _ = request.reply.try_send(result);
}
impl ExecutionPump {
    pub(super) fn start(
        registry: Arc<Mutex<Registry>>,
        journal: Arc<Mutex<DeviceEffectJournal>>,
        db: Arc<Mutex<Connection>>,
        pool: ResourcePool,
        shutdown_requested: Arc<AtomicBool>,
    ) -> PResult<Self> {
        let binding = journal.try_lock().map_err(internal)?.receipt_binding();
        let resources = Resources {
            shutdown_requested,
            registry,
            journal,
            db,
            pool,
            binding,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let (commands, incoming) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("pulsar-handy-effects".into())
            .spawn(move || {
                let mut work: HashMap<ArtifactId, Work> = HashMap::new();
                let mut shutdown_deadline = None;
                let mut drain: Option<DrainRequest> = None;
                loop {
                    if shutdown.load(Ordering::Acquire) && shutdown_deadline.is_none() {
                        shutdown_deadline = Some(Instant::now() + Duration::from_secs(12));
                        if let Ok(mut registry) = resources.registry.lock() {
                            registry.entries.retain(|_, e| !controller::retire_entry(e));
                        }
                    }
                    if drain.is_none() && shutdown_deadline.is_none() {
                        drain = incoming.try_recv().ok();
                    }
                    collect_pending(&resources, &mut work);
                    if let Some(request) = &drain {
                        if drain_wait(
                            request.deadline,
                            request.abandoned.load(Ordering::Acquire),
                        )
                        .is_ok()
                        {
                            for id in &request.ids {
                                if let Some(w) = work.get_mut(id) {
                                    w.begin_drain();
                                }
                            }
                        }
                    }
                    work.retain(|_, w| !w.step(&resources));
                    service_drain(&resources, &mut work, &mut drain);
                    if shutdown_deadline.is_some_and(|d| {
                        work.is_empty()
                            || work.values().all(Work::quiescent_failure)
                            || Instant::now() >= d
                    }) {
                        break;
                    }
                    thread::park_timeout(Duration::from_millis(20));
                }
                if let Some(request) = drain.take() {
                    let _ = request.reply.try_send(Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "Handy execution stopped during reconciliation drain",
                    )));
                }
                for w in work.values_mut() {
                    w.finish(&resources);
                }
                if work.values().any(Work::unresolved_stop) {
                    return Err(ProtocolError::new(ErrorCode::Unavailable,"stop dispatch or metadata remains unresolved; physical stopping is unconfirmed"));
                }
                Ok(())
            })
            .map_err(|_| {
                ProtocolError::new(ErrorCode::Unavailable, "Handy execution worker unavailable")
            })?;
        Ok(Self {
            stop,
            commands,
            thread: Some(worker),
        })
    }
    pub(super) fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
    pub(in crate::authority) fn reconciliation_handle(&self) -> PResult<ReconciliationHandle> {
        if self.stop.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Handy execution worker unavailable for reconciliation",
            ));
        }
        let worker = self
            .thread
            .as_ref()
            .filter(|worker| !worker.is_finished())
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy execution worker unavailable for reconciliation",
                )
            })?;
        Ok(ReconciliationHandle {
            stop: self.stop.clone(),
            commands: self.commands.clone(),
            wake: worker.thread().clone(),
        })
    }
    #[cfg(test)]
    fn drain_for_reconciliation(
        &self,
        ids: Vec<ArtifactId>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        self.reconciliation_handle()?
            .drain_for_reconciliation(ids, deadline, cancel)
    }
    #[cfg(test)]
    fn finish_reconciliation(
        &self,
        ids: Vec<ArtifactId>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        self.reconciliation_handle()?
            .finish_reconciliation(ids, deadline, cancel)
    }
    pub(in crate::authority) fn shutdown(&mut self) -> PResult<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            t.join().map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy execution worker panicked during retirement",
                )
            })??;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::authority) fn panicked_for_test() -> Self {
        let (commands, _) = mpsc::sync_channel(1);
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            commands,
            thread: Some(thread::spawn(|| {
                panic!("synthetic Handy-execution failure")
            })),
        }
    }
}

impl ReconciliationHandle {
    /// The caller has retired these entries while excluding new admission.
    /// It must release registry and project guards before waiting. This only
    /// drains execution custody; it never establishes physical stop evidence.
    pub(in crate::authority) fn drain_for_reconciliation(
        &self,
        ids: Vec<ArtifactId>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        self.reconciliation_command(ids, DrainStage::Quiesce, deadline, cancel)
    }
    /// Only call after committing the exact durable reconciliation. Local
    /// rejections are released only when their original effect is resolved.
    pub(in crate::authority) fn finish_reconciliation(
        &self,
        ids: Vec<ArtifactId>,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        self.reconciliation_command(ids, DrainStage::Finish, deadline, cancel)
    }
    fn reconciliation_command(
        &self,
        mut ids: Vec<ArtifactId>,
        stage: DrainStage,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        if ids.len() > HANDY_REGISTRATION_LIMIT {
            return Err(ProtocolError::new(
                ErrorCode::InvalidRequest,
                "Handy reconciliation drain target limit exceeded",
            ));
        }
        ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        ids.dedup();
        drain_wait(deadline, cancel.load(Ordering::Acquire))?;
        let abandoned = Arc::new(AtomicBool::new(false));
        let (reply, response) = mpsc::sync_channel(1);
        let mut request = DrainRequest {
            ids,
            stage,
            deadline,
            abandoned: abandoned.clone(),
            reply,
        };
        loop {
            drain_wait(deadline, cancel.load(Ordering::Acquire))?;
            if self.stop.load(Ordering::Acquire) {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy execution worker unavailable for reconciliation drain",
                ));
            }
            match self.commands.try_send(request) {
                Ok(()) => break,
                Err(TrySendError::Full(value)) => request = value,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "Handy reconciliation drain mailbox closed",
                    ));
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        self.wake.unpark();
        loop {
            if let Err(error) = drain_wait(deadline, cancel.load(Ordering::Acquire)) {
                abandoned.store(true, Ordering::Release);
                self.wake.unpark();
                return Err(error);
            }
            if self.stop.load(Ordering::Acquire) {
                abandoned.store(true, Ordering::Release);
                self.wake.unpark();
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy execution stopped during reconciliation wait",
                ));
            }
            match response.recv_timeout(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10)),
            ) {
                Ok(result) => {
                    drain_wait(deadline, cancel.load(Ordering::Acquire))?;
                    if self.stop.load(Ordering::Acquire) {
                        return Err(ProtocolError::new(
                            ErrorCode::Unavailable,
                            "Handy execution stopped during reconciliation wait",
                        ));
                    }
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "Handy execution lost reconciliation drain custody",
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "handy_execution_drain_tests.rs"]
mod drain_tests;
impl Drop for ExecutionPump {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
impl Engine {
    pub(in crate::authority) fn ensure_handy_execution(&self) -> PResult<()> {
        if self.is_shutting_down() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Engine is shutting down",
            ));
        }
        let mut slot = self.handy_execution.lock().map_err(internal)?;
        if let Some(p) = slot.as_ref() {
            if !p.is_running() {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Handy execution worker stopped",
                ));
            }
        } else {
            *slot = Some(ExecutionPump::start(
                self.handy_preparations.clone(),
                self._device_effect_journal.clone(),
                self.db.clone(),
                self.pool.clone(),
                self.shutdown_requested.clone(),
            )?);
        }
        Ok(())
    }
}

#[cfg(test)]
impl Engine {
    pub(in crate::authority) fn bind_handy_play_transport_for_test(
        &self,
        preparation: &ArtifactId,
        origin: String,
    ) -> (Arc<Mutex<HandyPlaybackRuntime>>, Arc<AtomicBool>) {
        let mut registry = self.handy_preparations.lock().unwrap();
        let entry = registry.entries.get_mut(preparation).unwrap();
        Arc::get_mut(&mut entry.transport)
            .unwrap()
            .set_test_origin(origin);
        (entry.runtime.clone(), entry.execution_pending.clone())
    }
    pub(in crate::authority) fn seed_handy_execution_for_test(
        &self,
        preparation: &ArtifactId,
        origin: String,
    ) -> (Arc<Mutex<HandyPlaybackRuntime>>, Arc<AtomicBool>) {
        let (runtime, pending, controller, admission, clock) = {
            let mut registry = self.handy_preparations.lock().unwrap();
            let e = registry.entries.get_mut(preparation).unwrap();
            Arc::get_mut(&mut e.transport)
                .unwrap()
                .set_test_origin(origin);
            (
                e.runtime.clone(),
                e.execution_pending.clone(),
                e.controller.as_ref().unwrap().report.controller_id.clone(),
                e.report.admission.clone(),
                e.playback_clock
                    .as_ref()
                    .unwrap()
                    .native
                    .at(1_000_000_020)
                    .unwrap(),
            )
        };
        let mut journal = self._device_effect_journal.lock().unwrap();
        {
            let mut runtime = runtime.lock().unwrap();
            let start = runtime
                .prepare_start(&controller, clock.clone(), 0)
                .unwrap();
            runtime
                .dispatch_start_durable(
                    start,
                    &admission,
                    &controller,
                    &clock.domain,
                    clock.monotonic_ns,
                    &mut journal,
                )
                .unwrap();
        }
        (runtime, pending)
    }
    pub(in crate::authority) fn retire_handy_execution_for_test(&self, preparation: &ArtifactId) {
        let _db = self.db.lock().unwrap();
        self.retire_handy_preparations(Retirement::Id(preparation));
    }
}
