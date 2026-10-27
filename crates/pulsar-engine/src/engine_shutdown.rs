//! Admission retirement and joining are explicit; cancellation alone is not cleanup.
use super::*;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

impl Engine {
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire) || self.background.is_closing()
    }
    pub(super) fn spawn_task(
        &self,
        name: &str,
        stack: Option<usize>,
        task: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<()> {
        if self.is_shutting_down() {
            return Err(std::io::Error::other("Engine is shutting down"));
        }
        self.background.spawn(name, stack, task)
    }
    pub(crate) fn request_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::Release);
        self.fallback_lifetimes.cancel_all();
        self.retire_handy_hsp_live();
        if let Ok(mut preparations) = self.handy_hsp_preparations.try_lock() {
            preparations.cancel_all();
        }
        // The HSP pump also observes this shared flag independently. The fast
        // path retires native ordinary ownership without waiting for metadata.
        if let Ok(slot) = self.handy_hsp_execution.try_lock() {
            if let Some(execution) = slot.as_ref() {
                execution.request_shutdown();
            }
        }
        self.background.close();
        self.browser_timing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.handy_browser_start_qualifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.handy_hsp_source_qualifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.browser_media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.handy_reconciliations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        for cancel in self
            .workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            cancel.store(true, Ordering::Release);
        }
        self.live_captures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.package_exports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        self.package_imports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_all();
        #[cfg(target_os = "linux")]
        for entry in self
            .native_preparations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .values()
        {
            entry.cancel.store(true, Ordering::Release);
        }
        // Retire controller authority before waiting on metadata or application work.
        self.retire_handy_for_shutdown();
    }
    pub(crate) fn finish_shutdown(&self) -> Result<()> {
        self.request_shutdown();
        // Join the independently owned device workers while their original resources live.
        let mut failure = None;
        let mut monitor = self
            .handy_controller_monitor
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(monitor) = &mut monitor {
            if let Err(error) = monitor.shutdown() {
                failure.get_or_insert_with(|| anyhow::Error::from(error));
            }
        }
        drop(monitor);
        let mut execution = self
            .handy_execution
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(execution) = &mut execution {
            if let Err(error) = execution.shutdown() {
                failure.get_or_insert_with(|| anyhow::Error::from(error));
            }
        }
        drop(execution);
        if let Err(error) = self.shutdown_handy_hsp_preparations() {
            failure.get_or_insert_with(|| {
                anyhow::anyhow!("HSP preparation shutdown unresolved: {error}")
            });
        }
        let mut hsp = self
            .handy_hsp_execution
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(hsp) = &mut hsp {
            if let Err(error) = hsp.shutdown() {
                failure
                    .get_or_insert_with(|| anyhow::anyhow!("HSP shutdown unresolved: {error:?}"));
            }
        }
        drop(hsp);
        if let Err(error) = self.background.join() {
            failure.get_or_insert_with(|| anyhow::Error::from(error));
        }
        #[cfg(target_os = "linux")]
        {
            let entries = std::mem::take(
                &mut self
                    .native_preparations
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entries,
            );
            for mut entry in entries.into_values() {
                if let Some(producer) = entry.producer.take() {
                    if producer.join().is_err() {
                        failure.get_or_insert_with(|| {
                            anyhow::anyhow!("native preparation producer panicked")
                        });
                    }
                }
                drop(entry);
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                match self.worker_fence.has_pending_worker_custody() {
                    Ok(false) => break,
                    Ok(true) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Ok(true) => {
                        failure.get_or_insert_with(|| {
                            anyhow::anyhow!("shutdown retains unresolved native worker custody")
                        });
                        break;
                    }
                    Err(error) => {
                        failure.get_or_insert(error);
                        break;
                    }
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
