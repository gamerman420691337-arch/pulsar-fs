//! Public capture's owned GPU scheduler. The actual worker never escapes this
//! thread; Finish requests freeze intake while an issued pair drains.
use super::*;
#[cfg(all(test, feature = "cuda"))]
type FinishHook = Box<
    dyn FnOnce(&mut crate::live_cuda_stream::LiveCudaWorker) -> crate::cgroup::FrozenTestGroup
        + Send,
>;
#[cfg(all(test, feature = "cuda"))]
fn finish_hooks() -> &'static Mutex<HashMap<String, FinishHook>> {
    static HOOKS: std::sync::OnceLock<Mutex<HashMap<String, FinishHook>>> =
        std::sync::OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}
#[cfg(all(test, feature = "cuda"))]
pub(in crate::authority) struct FinishHookGuard(String);
#[cfg(all(test, feature = "cuda"))]
impl Drop for FinishHookGuard {
    fn drop(&mut self) {
        finish_hooks().lock().unwrap().remove(&self.0);
    }
}
#[cfg(all(test, feature = "cuda"))]
impl Engine {
    pub(in crate::authority) fn public_cuda_finish_hook_for_test(
        &self,
        epoch: &AttemptId,
        hook: FinishHook,
    ) -> FinishHookGuard {
        let key = format!("{}:{}", self.engine_epoch, epoch.as_str());
        assert!(finish_hooks()
            .lock()
            .unwrap()
            .insert(key.clone(), hook)
            .is_none());
        FinishHookGuard(key)
    }
}
pub(super) fn preflight(engine: &Engine, manifest: &LiveInferenceManifest) -> PResult<()> {
    if !cfg!(feature = "cuda") {
        return Err(ProtocolError::unsupported(
            "live CUDA requires a CUDA-enabled build",
        ));
    }
    let owner = engine
        .cuda_owner_policy
        .as_ref()
        .ok_or_else(|| ProtocolError::unsupported("live CUDA requires an owner policy"))?;
    owner
        .policy()
        .authorize_profile(
            &manifest.profile,
            &manifest.budget,
            crate::resources::GpuPriority::Interactive,
        )
        .map_err(unavailable)?;
    owner
        .policy()
        .match_runtime(Some(&manifest.runtime))
        .map_err(unavailable)?;
    crate::inference_profiles::admit_tensor_adapter(&manifest.profile, &manifest.budget)
        .map_err(unavailable)?;
    Ok(())
}
pub(super) fn spawn(
    engine: &Arc<Engine>,
    epoch: AttemptId,
    manifest: LiveInferenceManifest,
    cancel: Arc<AtomicBool>,
    origin: Instant,
    lease_end: Instant,
    intent: Request,
) -> PResult<()> {
    let weak = Arc::downgrade(engine);
    engine
        .spawn_task("pulsar-live-cuda", None, move || {
            let result = (|| -> PResult<()> {
                let Some(engine) = weak.upgrade() else {
                    return Ok(());
                };
                if !engine.live_analysis_launch_allowed(&epoch, &manifest.attempt_id)? {
                    return Ok(());
                }
                let scope = {
                    let db = engine.db.lock().map_err(internal)?;
                    let mut registry = engine.live_captures.lock().map_err(internal)?;
                    registry.sweep(&db)?;
                    let Some(entry) = active(&mut registry, &epoch, &manifest.attempt_id) else {
                        return Ok(());
                    };
                    entry.runtime.as_ref().expect("active").inference_scope()
                };
                let started = engine.start_live_cuda(
                    &intent,
                    manifest.clone(),
                    scope,
                    cancel.clone(),
                    origin,
                    lease_end,
                )?;
                let (_, mut worker) = match started {
                    crate::authority::engine_live_cuda::LiveCudaStartup::Ready(ready) => *ready,
                    crate::authority::engine_live_cuda::LiveCudaStartup::ProviderUnavailable(
                        settled,
                    ) => {
                        return fallback::after_provider_failure(
                            &engine,
                            &intent,
                            &epoch,
                            &manifest.attempt_id,
                            *settled,
                        );
                    }
                };
                if !engine.live_cuda_analysis_ready(&epoch, &worker, &manifest.attempt_id)? {
                    return Ok(());
                }
                drop(engine);
                loop {
                    if cancel.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let Some(engine) = weak.upgrade() else {
                        return Ok(());
                    };
                    let finish = {
                        let db = engine.db.lock().map_err(internal)?;
                        let mut registry = engine.live_captures.lock().map_err(internal)?;
                        registry.sweep(&db)?;
                        let Some(entry) = active(&mut registry, &epoch, &manifest.attempt_id)
                        else {
                            return Ok(());
                        };
                        entry.report.inference.as_ref().expect("admitted").state
                            == LiveCaptureInferenceState::FinishRequested
                    };
                    if finish {
                        #[cfg(all(test, feature = "cuda"))]
                        let _frozen = {
                            let hook = finish_hooks().lock().unwrap().remove(&format!(
                                "{}:{}",
                                engine.engine_epoch,
                                epoch.as_str()
                            ));
                            hook.map(|hook| hook(&mut worker))
                        };
                        engine.live_cuda_finalize(&intent, &epoch, &manifest.attempt_id, worker)?;
                        return Ok(());
                    }
                    let work = engine.live_analysis_take(&epoch, &manifest.attempt_id)?;
                    drop(engine);
                    let Some(work) = work else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    let reply = worker.estimate(&work).map_err(unavailable)?;
                    let Some(engine) = weak.upgrade() else {
                        return Ok(());
                    };
                    if !engine.live_cuda_analysis_complete(
                        &epoch,
                        &manifest.attempt_id,
                        &worker,
                        work,
                        reply,
                    )? {
                        return Ok(());
                    }
                }
            })();
            if let Err(error) = result {
                if let Some(engine) = weak.upgrade() {
                    let _ = engine.live_analysis_failed(&epoch, &manifest.attempt_id, error);
                }
            }
        })
        .map_err(internal)?;
    Ok(())
}
