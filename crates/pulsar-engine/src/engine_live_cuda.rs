//! Engine-owned private GPU live startup. The caller owns active-lease checks,
//! issued-work completion, provisional results and final evidence persistence.
use super::*;
use crate::{
    cuda_devices::PinnedCudaDevices, live_cuda_stream::LiveCudaWorker, live_frames::LiveRgbScope,
    resources::GpuPriority,
};
use std::time::{Duration, Instant};

pub(super) enum LiveCudaStartup {
    Ready(Box<(LiveCudaInferenceManifest, LiveCudaWorker)>),
    ProviderUnavailable(Box<crate::live_cuda_stream::SettledLiveCudaProviderFailure>),
}

impl Engine {
    pub(super) fn start_live_cuda(
        &self,
        request: &Request,
        mut stream: LiveInferenceManifest,
        scope: LiveRgbScope,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
    ) -> PResult<LiveCudaStartup> {
        if !cfg!(feature = "cuda") {
            return Err(ProtocolError::unsupported(
                "live CUDA requires a CUDA-enabled build",
            ));
        }
        let deadline = crate::live_worker_lifetime::deadline(
            origin,
            lease_end,
            stream.budget.wall_time_ms,
            Instant::now(),
        )?;
        scope.validate_manifest(&stream)?;
        let (engine_epoch, options) = match &request.command {
            Command::StartLiveCapture {
                engine_epoch,
                options,
            }
            | Command::ContinueLiveCapture {
                engine_epoch,
                options,
                ..
            } => (engine_epoch, options),
            _ => {
                return Err(ProtocolError::invalid(
                    "live CUDA requires explicitly declared capture options",
                ))
            }
        };
        options.validate()?;
        stream.validate_browser_source(engine_epoch, options)?;
        let declaration = options
            .inference
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("live CUDA inference absent"))?;
        let declared_profile = serde_json::to_vec(&declaration.profile).map_err(internal)?;
        let profile_bytes = serde_json::to_vec(&stream.profile).map_err(internal)?;
        if engine_epoch.as_str() != self.engine_epoch
            || stream.identity.project != *project(request)?
            || Some(stream.identity.revision) != request.expected_revision
            || declaration.model_path != stream.model.path
            || declared_profile != profile_bytes
            || options.clock(stream.identity.epoch.clone()) != stream.clock
            || u64::from(options.lease_ms) != stream.budget.wall_time_ms
            || stream.profile.provider != InferenceProvider::Cuda
            || stream.worker != self.binary_identity
            || stream.budget.memory_bytes > self.config.max_ram_bytes
            || Duration::from_millis(stream.budget.wall_time_ms) > self.config.worker_timeout
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live CUDA request, boot, profile or budget differs",
            ));
        }
        let owner = self
            .cuda_owner_policy
            .as_ref()
            .ok_or_else(|| ProtocolError::unsupported("live CUDA requires an owner policy"))?;
        owner
            .policy()
            .authorize_profile(&stream.profile, &stream.budget, GpuPriority::Interactive)
            .map_err(unavailable)?;
        owner
            .policy()
            .match_runtime(Some(&stream.runtime))
            .map_err(unavailable)?;
        crate::inference_profiles::admit_tensor_adapter(&stream.profile, &stream.budget)
            .map_err(unavailable)?;
        let total = owner
            .policy()
            .native_dependencies
            .validate()
            .map_err(unavailable)?;
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "live CUDA startup cancelled",
            ));
        }
        let fallback_stream = if options.inference_fallback.is_some() {
            live_captures::fallback::verify_dispatch(self, &stream)?;
            Some(stream.clone())
        } else {
            None
        };
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.authorize(
                &tx,
                request,
                request
                    .session
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("live CUDA session absent"))?,
            )?;
            check_revision(&tx, request)?;
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        let devices = PinnedCudaDevices::open(&owner.policy().device).map_err(unavailable)?;
        #[cfg(test)]
        eprintln!(
            "live CUDA startup phase=native_capture_begin elapsed_ms={}",
            origin.elapsed().as_millis()
        );
        let image = self.native_image_reuse.capture_with(
            &owner.policy().native_dependencies,
            &cancel,
            deadline,
            || {
                crate::native_capture::capture_tracked_in_lease(
                    &self.config,
                    &self.pool,
                    &owner.policy().native_dependencies,
                    crate::native_capture::CaptureLimits {
                        budget: crate::native_capture::preparation_budget(
                            total,
                            stream.budget.wall_time_ms,
                        )?,
                        address_space_bytes: 1536 << 20,
                    },
                    &cancel,
                    self.worker_fence.clone(),
                    deadline,
                )
            },
        )?;
        #[cfg(test)]
        eprintln!(
            "live CUDA startup phase=native_capture_complete elapsed_ms={}",
            origin.elapsed().as_millis()
        );
        let prepared = crate::live_staging::prepare(
            &self.config,
            &self.pool,
            [
                ArtifactReference {
                    path: self.config.worker_executable.clone(),
                    identity: stream.worker.clone(),
                },
                stream.model.clone(),
            ],
            crate::live_staging::PreparationBudget {
                memory_bytes: 64 << 20,
                cpu_threads: 1,
            },
            cancel.clone(),
            deadline,
            Some(self.worker_fence.clone()),
        )?
        .into_gpu()?;
        #[cfg(test)]
        eprintln!(
            "live CUDA startup phase=worker_staging_complete elapsed_ms={}",
            origin.elapsed().as_millis()
        );
        stream.model.path = "/input/model".into();
        stream.runtime.path = image
            .captured()
            .library_directory()
            .join("libonnxruntime.so.1");
        let execution = owner
            .worker_execution_supervised(
                &image,
                &stream.profile,
                &stream.budget,
                GpuPriority::Interactive,
            )
            .map_err(unavailable)?;
        let mut dependencies = vec![
            stream.model.identity.clone(),
            stream.worker.clone(),
            execution.owner_policy.clone(),
        ];
        for lib in &execution.libraries {
            if !dependencies.contains(&lib.identity) {
                dependencies.push(lib.identity.clone());
            }
        }
        let manifest = LiveCudaInferenceManifest {
            schema_version: 1,
            stream,
            execution,
            dependencies,
            output_dir: "/live-evidence".into(),
        };
        manifest.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(
                &db,
                request,
                request.session.as_ref().expect("validated session"),
            )?;
            check_revision(&db, request)?;
            self.require_worker_fence(&db)?;
        }
        let end_ns = crate::cuda_bootstrap::deadline_ns_in_lease(
            deadline,
            manifest.stream.budget.wall_time_ms,
        )
        .map_err(unavailable)?;
        let custody = self
            .worker_fence
            .admit_group(
                Arc::new(self.pool.reserve_worker(
                    manifest.stream.budget.memory_bytes,
                    manifest.stream.budget.cpu_threads,
                )?),
                manifest.stream.budget.memory_bytes,
                manifest.stream.budget.cpu_threads,
            )
            .map_err(unavailable)?
            .into_worker();
        let observed = crate::cuda_memory::observe_live(
            &self.config,
            &manifest,
            end_ns,
            image,
            devices,
            custody,
            &cancel,
        )
        .map_err(unavailable)?;
        #[cfg(test)]
        eprintln!(
            "live CUDA startup phase=gpu_observation_complete elapsed_ms={}",
            origin.elapsed().as_millis()
        );
        let admitted = observed
            .admit(&self.pool, owner, &manifest)
            .map_err(unavailable)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(
                &db,
                request,
                request.session.as_ref().expect("validated session"),
            )?;
            check_revision(&db, request)?;
            self.require_worker_fence(&db)?;
        }
        if let Some(original) = &fallback_stream {
            live_captures::fallback::verify_dispatch(self, original)?;
        }
        let started = LiveCudaWorker::start(
            &self.config,
            manifest.clone(),
            admitted,
            prepared,
            scope,
            cancel,
        );
        #[cfg(test)]
        eprintln!(
            "live CUDA startup phase=worker_start_returned elapsed_ms={}",
            origin.elapsed().as_millis()
        );
        match started {
            Ok(worker) => Ok(LiveCudaStartup::Ready(Box::new((manifest, worker)))),
            Err(error) => {
                match error.downcast::<crate::live_cuda_stream::SettledLiveCudaProviderFailure>() {
                    Ok(settled) => {
                        settled.matches_manifest(&manifest).map_err(unavailable)?;
                        Ok(LiveCudaStartup::ProviderUnavailable(Box::new(settled)))
                    }
                    Err(error) => Err(unavailable(error)),
                }
            }
        }
    }
}
