//! Trusted Engine embedding API; authenticated client dispatch is separate.
use super::*;
use crate::{AssistantExecution, AssistantLaunchAssets};
use std::time::{Duration, Instant};
impl Engine {
    /// Run one confined learned proposal using this Engine's shared admission
    /// and durable worker ownership fence. No session/project approval, effect,
    /// native exception, model qualification or job resumption is granted.
    pub fn propose_assistant(
        &self,
        request: &AssistantWorkerRequest,
        assets: &AssistantLaunchAssets,
        cancel: &AtomicBool,
    ) -> PResult<AssistantExecution> {
        self.propose_assistant_admitted(request, assets, cancel, None)
    }
    pub(super) fn propose_assistant_admitted(
        &self,
        request: &AssistantWorkerRequest,
        assets: &AssistantLaunchAssets,
        cancel: &AtomicBool,
        admission: Option<Reservation>,
    ) -> PResult<AssistantExecution> {
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(request.wall_time_ms))
            .ok_or_else(|| ProtocolError::invalid("assistant deadline overflow"))?;
        crate::assistant_launch::validate_request(request)?;
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "assistant cancelled before admission",
            ));
        }
        if Duration::from_millis(request.wall_time_ms) > self.config.worker_timeout {
            return Err(ProtocolError::invalid(
                "assistant exceeds Engine owner deadline",
            ));
        }
        if !crate::launch::available() {
            return Err(ProtocolError::unsupported(
                "assistant Linux confinement unavailable",
            ));
        }
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        crate::assistant_launch::execute(
            crate::assistant_launch::Parent {
                config: &self.config,
                pool: &self.pool,
                fence: self.worker_fence.clone(),
                worker_identity: &self.binary_identity,
                admission,
            },
            request,
            assets,
            cancel,
            deadline,
        )
    }
}
