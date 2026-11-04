//! One selected capture/admission read rendezvous; compiled only in test builds.
use super::*;
use std::sync::Condvar;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::authority) enum CaptureReadTestTarget {
    Basis(AttemptId),
    AdmissionFirst(RequestId),
    AdmissionFinal(RequestId),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::authority) enum CaptureReadTestPhase {
    BeforeSample,
    AfterSample,
}
#[derive(Clone, Default)]
pub(in crate::authority) struct CaptureReadTestSnapshot {
    pub entered: bool,
    pub sampled_at_ns: Option<u64>,
    pub observation: Option<BrowserTimingObservation>,
    pub completed: bool,
    pub timed_out: bool,
}
#[derive(Default)]
struct State {
    receipt: CaptureReadTestSnapshot,
    released: bool,
}
pub(super) struct CaptureReadPause {
    target: CaptureReadTestTarget,
    phase: CaptureReadTestPhase,
    state: Mutex<State>,
    changed: Condvar,
}
pub(in crate::authority) struct CaptureReadTestHold(Arc<CaptureReadPause>);
impl CaptureReadTestHold {
    pub fn wait_entered(&self, timeout: Duration) -> bool {
        let state = self.0.state.lock().unwrap();
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |s| !s.receipt.entered)
            .unwrap();
        state.receipt.entered && !state.receipt.timed_out
    }
    pub fn release(&self) {
        self.0.state.lock().unwrap().released = true;
        self.0.changed.notify_all();
    }
    pub fn snapshot(&self) -> CaptureReadTestSnapshot {
        self.0.state.lock().unwrap().receipt.clone()
    }
}
impl Drop for CaptureReadTestHold {
    fn drop(&mut self) {
        self.release();
    }
}
#[derive(Default)]
pub(super) struct CaptureReadGuard(Option<Arc<CaptureReadPause>>);
impl CaptureReadGuard {
    pub(super) fn take(registry: &Registry, target: &CaptureReadTestTarget) -> Self {
        let mut slot = registry.read_order_test.lock().unwrap();
        Self(if slot.as_ref().is_some_and(|p| &p.target == target) {
            slot.take()
        } else {
            None
        })
    }
    pub(super) fn enter(
        &self,
        phase: CaptureReadTestPhase,
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
            .wait_timeout_while(state, Duration::from_secs(2), |s| !s.released)
            .unwrap();
        if timeout.timed_out() && !state.released {
            state.receipt.timed_out = true;
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "test capture read rendezvous expired",
            ));
        }
        Ok(())
    }
    pub(super) fn observed(&self, observation: &BrowserTimingObservation) {
        if let Some(pause) = &self.0 {
            pause.state.lock().unwrap().receipt.observation = Some(observation.clone());
        }
    }
}
impl Drop for CaptureReadGuard {
    fn drop(&mut self) {
        if let Some(pause) = &self.0 {
            pause.state.lock().unwrap().receipt.completed = true;
            pause.changed.notify_all();
        }
    }
}
impl Engine {
    pub(in crate::authority) fn hold_capture_read_for_test(
        &self,
        target: CaptureReadTestTarget,
        phase: CaptureReadTestPhase,
    ) -> CaptureReadTestHold {
        let registry = self.live_captures.lock().unwrap();
        let pause = Arc::new(CaptureReadPause {
            target,
            phase,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        assert!(registry
            .read_order_test
            .lock()
            .unwrap()
            .replace(pause.clone())
            .is_none());
        CaptureReadTestHold(pause)
    }
}
