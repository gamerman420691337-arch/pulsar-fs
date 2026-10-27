//! Per-worker-thread counters. No media, tensor contents, or heavy native trace.
use std::{cell::RefCell, time::Duration};
#[derive(Default)]
struct Counters {
    runs: u64,
    elapsed_ns: u128,
}
thread_local! { static COUNTERS:RefCell<Option<Counters>>=const {RefCell::new(None)}; }
thread_local! { static CUDA:RefCell<Option<pulsar_protocol::WorkerCudaArtifacts>>=const {RefCell::new(None)}; }

#[cfg(all(target_os = "linux", feature = "cuda"))]
pub(super) fn set_cuda(evidence: pulsar_protocol::WorkerCudaArtifacts) {
    CUDA.with(|state| *state.borrow_mut() = Some(evidence));
}
pub(super) fn take_cuda() -> Option<pulsar_protocol::WorkerCudaArtifacts> {
    CUDA.with(|state| state.borrow_mut().take())
}

pub fn clear() {
    COUNTERS.with(|state| *state.borrow_mut() = None);
    CUDA.with(|state| *state.borrow_mut() = None);
}

pub fn reset() {
    clear();
    COUNTERS.with(|state| *state.borrow_mut() = Some(Counters::default()));
}
pub fn completed(elapsed: Duration) {
    COUNTERS.with(|state| {
        if let Some(value) = state.borrow_mut().as_mut() {
            value.runs = value.runs.saturating_add(1);
            value.elapsed_ns = value.elapsed_ns.saturating_add(elapsed.as_nanos());
        }
    });
}
pub fn receipt() -> Option<serde_json::Value> {
    COUNTERS.with(|state| {
        state.borrow().as_ref().filter(|value|value.runs>0).map(|value|
        serde_json::json!({"provider":"cpu","provider_evidence":"explicit-cpu-session-success",
            "successful_runs":value.runs,"inference_elapsed_ns":value.elapsed_ns.to_string(),
            "peak_memory_bytes":null,"native_node_assignment_trace":false}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attempt_reset_cannot_reuse_previous_inference_evidence() {
        reset();
        completed(Duration::from_nanos(1));
        assert!(receipt().is_some());
        clear();
        assert!(receipt().is_none());
    }
}
