//! Stop-only fallback custody. No fabricated durable receipt or physical proof.
use super::*;
use crate::device_effect_journal::{DeviceEffectJournal, EffectIntent, EffectReceiptBinding};

/// A current native stop whose prewrite could not be confirmed. Only Engine
/// custody can construct this envelope; it shares the ordinary stop claim slot.
pub struct EmergencyStopEffect {
    pub(super) effect: PlaybackEffect,
    pub(super) intent: EffectIntent,
}
impl EmergencyStopEffect {
    pub(super) fn validate(&self, binding: &EffectReceiptBinding) -> Result<(), PlaybackFailure> {
        if !self.effect.is_stop()
            || self.effect.request.method() != "PUT"
            || self.effect.request.path() != "/hssp/stop"
            || self.effect.request.body().is_some()
        {
            return Err(PlaybackFailure::RetiredCommand);
        }
        binding
            .validate_intent(&self.effect, &self.intent)
            .map_err(|_| PlaybackFailure::JournalUnavailable)
    }
    pub(super) fn observation(
        &self,
        outcome: DispatchOutcome,
        binding: &EffectReceiptBinding,
    ) -> Result<EmergencyStopObservation, PlaybackFailure> {
        self.validate(binding)?;
        Ok(EmergencyStopObservation {
            intent: self.intent.clone(),
            outcome,
            recorded: false,
        })
    }
}

/// An exact attempted stop observation retained independently of persistence.
/// Retrying this value only writes metadata and cannot issue another command.
pub struct EmergencyStopObservation {
    intent: EffectIntent,
    outcome: DispatchOutcome,
    recorded: bool,
}
impl EmergencyStopObservation {
    pub fn outcome(&self) -> DispatchOutcome {
        self.outcome
    }
    pub fn journal_pending(&self) -> bool {
        !self.recorded
    }
    pub(crate) fn try_persist(
        &mut self,
        journal: &mut DeviceEffectJournal,
    ) -> Result<(), PlaybackFailure> {
        self.recorded = false;
        journal
            .try_record_emergency_stop(&self.intent, self.outcome)
            .map_err(|_| PlaybackFailure::JournalUnavailable)?;
        self.recorded = true;
        Ok(())
    }
}
impl HandyPlaybackRuntime {
    pub(crate) fn prepare_emergency_stop(
        &self,
        effect: PlaybackEffect,
        intent: EffectIntent,
        binding: &EffectReceiptBinding,
    ) -> Result<EmergencyStopEffect, PlaybackFailure> {
        if !effect.is_stop()
            || !self.effect_is_current(&effect)
            || self.claimed_stop == Some(effect.sequence)
        {
            return Err(PlaybackFailure::RetiredCommand);
        }
        let stop = EmergencyStopEffect { effect, intent };
        stop.validate(binding)?;
        Ok(stop)
    }
    pub(crate) fn observe_emergency_stop_completion(
        &mut self,
        stop: &EmergencyStopEffect,
        outcome: DispatchOutcome,
        binding: &EffectReceiptBinding,
    ) -> Result<EmergencyStopObservation, PlaybackFailure> {
        if !self.effect_is_current(&stop.effect) || self.claimed_stop != Some(stop.effect.sequence)
        {
            return Err(PlaybackFailure::RetiredCommand);
        }
        let observed = stop.observation(outcome, binding)?;
        self.complete_stop(stop.effect.clone(), outcome)?;
        Ok(observed)
    }
}
