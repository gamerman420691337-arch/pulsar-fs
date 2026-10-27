//! Scoped native producer rendezvous. This module exists only in test builds.
use super::*;
use std::{sync::Condvar, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::authority) enum HspProducerReadTestPhase {
    BeforeSample,
    AfterSample,
}

#[derive(Clone, Default)]
pub(in crate::authority) struct HspProducerReadTestSnapshot {
    pub entered: bool,
    pub sampled_at_ns: Option<u64>,
    pub observation: Option<BrowserTimingObservation>,
    pub clock_pin: Option<HspClockPin>,
    pub sample: Option<HspSample>,
    pub completed: bool,
    pub delivered: bool,
    pub timed_out: bool,
}
#[derive(Default)]
struct State {
    receipt: HspProducerReadTestSnapshot,
    released: bool,
}
pub(super) struct ProducerReadPause {
    phase: HspProducerReadTestPhase,
    state: Mutex<State>,
    changed: Condvar,
}
pub(in crate::authority) struct HspProducerReadTestHold(Arc<ProducerReadPause>);
impl HspProducerReadTestHold {
    pub fn wait_entered(&self, timeout: Duration) -> bool {
        let state = self.0.state.lock().unwrap();
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |state| !state.receipt.entered)
            .unwrap();
        state.receipt.entered && !state.receipt.timed_out
    }
    pub fn release(&self) {
        self.0.state.lock().unwrap().released = true;
        self.0.changed.notify_all();
    }
    pub fn snapshot(&self) -> HspProducerReadTestSnapshot {
        self.0.state.lock().unwrap().receipt.clone()
    }
}
impl Drop for HspProducerReadTestHold {
    fn drop(&mut self) {
        self.release();
    }
}
pub(super) struct ProducerReadGuard(Option<Arc<ProducerReadPause>>);
impl ProducerReadGuard {
    pub(super) fn take(slot: &Mutex<Option<Arc<ProducerReadPause>>>) -> Self {
        Self(slot.lock().unwrap().take())
    }
    pub(super) fn enter(
        &self,
        phase: HspProducerReadTestPhase,
        sampled_at_ns: Option<u64>,
    ) -> PResult<()> {
        let Some(pause) = &self.0 else { return Ok(()) };
        let mut state = pause.state.lock().unwrap();
        if sampled_at_ns.is_some() {
            state.receipt.sampled_at_ns = sampled_at_ns;
        }
        if pause.phase != phase {
            return Ok(());
        }
        state.receipt.entered = true;
        pause.changed.notify_all();
        let (mut state, timeout) = pause
            .changed
            .wait_timeout_while(state, Duration::from_secs(2), |state| !state.released)
            .unwrap();
        if timeout.timed_out() && !state.released {
            state.receipt.timed_out = true;
            return Err(unavailable());
        }
        Ok(())
    }
    pub(super) fn observed(&self, value: &BrowserTimingObservation, pin: Option<&HspClockPin>) {
        if let Some(pause) = &self.0 {
            let mut state = pause.state.lock().unwrap();
            state.receipt.observation = Some(value.clone());
            state.receipt.clock_pin = pin.cloned();
        }
    }
    pub(super) fn sample(&self, value: HspSample) {
        if let Some(pause) = &self.0 {
            pause.state.lock().unwrap().receipt.sample = Some(value);
        }
    }
    pub(super) fn delivered(&self) {
        if let Some(pause) = &self.0 {
            pause.state.lock().unwrap().receipt.delivered = true;
        }
    }
}
impl Drop for ProducerReadGuard {
    fn drop(&mut self) {
        if let Some(pause) = &self.0 {
            pause.state.lock().unwrap().receipt.completed = true;
            pause.changed.notify_all();
        }
    }
}
impl Engine {
    pub(in crate::authority) fn hold_next_hsp_producer_read_for_test(
        &self,
        actor: &SessionId,
        request: &RequestId,
        phase: HspProducerReadTestPhase,
    ) -> HspProducerReadTestHold {
        let registry = self.handy_hsp_live.lock().unwrap();
        let controller = registry
            .entries
            .get(&(actor.clone(), request.clone()))
            .unwrap();
        let pause = Arc::new(ProducerReadPause {
            phase,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        assert!(controller
            .producer_read_test
            .lock()
            .unwrap()
            .replace(pause.clone())
            .is_none());
        HspProducerReadTestHold(pause)
    }
}
