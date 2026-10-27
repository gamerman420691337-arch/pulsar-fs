//! Capacity-release wakeups for pending completion and owned generation recovery.
use super::*;
use std::time::Duration;
impl Engine {
    pub(super) fn schedule_assistant_recovery(self: &Arc<Self>) -> PResult<()> {
        if self
            .assistant_completions
            .lock()
            .map_err(internal)?
            .is_empty()
            || self
                .assistant_recovery_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        let spawned = self.spawn_task("pulsar-assistant-recovery", None, move || {
            let Some(engine) = weak.upgrade() else {
                return;
            };
            let watcher = match engine.pool.watch_releases() {
                Ok(watcher) => watcher,
                Err(_) => {
                    engine
                        .assistant_recovery_running
                        .store(false, Ordering::Release);
                    return;
                }
            };
            drop(engine);
            loop {
                let Some(engine) = weak.upgrade() else {
                    return;
                };
                if engine.is_shutting_down() {
                    return;
                }
                // Subscribe before the first retry, so capacity released around
                // scheduling cannot be lost. This thread's own releases do not
                // notify itself; a failed retry cannot create a busy retry loop.
                let _ = engine.flush_assistant_completions();
                if engine
                    .assistant_completions
                    .lock()
                    .is_ok_and(|pending| pending.is_empty())
                {
                    drop(watcher);
                    engine
                        .assistant_recovery_running
                        .store(false, Ordering::Release);
                    // Recheck after flag reset to fence a concurrent completion.
                    let _ = engine.schedule_assistant_recovery();
                    return;
                }
                drop(engine);
                loop {
                    match watcher.wait(Duration::from_millis(100)) {
                        Ok(()) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if weak.upgrade().is_none_or(|e| e.is_shutting_down()) {
                                return;
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        });
        if let Err(error) = spawned {
            self.assistant_recovery_running
                .store(false, Ordering::Release);
            return Err(internal(error));
        }
        Ok(())
    }
}
