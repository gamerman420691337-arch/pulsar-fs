//! Capture ownership for explicit Engine CUDA generation, preview and fresh retries.
use super::*;
use crate::{
    cuda_devices::PinnedCudaDevices, native_capture::SupervisedNativeDependencies,
    resources::GpuPriority,
};

pub(super) struct PreparedCudaGeneration {
    pub(super) image: SupervisedNativeDependencies,
    pub(super) devices: PinnedCudaDevices,
}
impl PreparedCudaGeneration {
    pub(super) fn bind(&self, engine: &Engine, manifest: &mut WorkerRequest) -> PResult<()> {
        let owner = engine
            .cuda_owner_policy
            .as_ref()
            .ok_or_else(|| ProtocolError::unsupported("CUDA owner policy unavailable"))?;
        let profile = match &manifest.operation {
            WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
            WorkerOperation::Preview {
                inference_profile, ..
            } => inference_profile.as_deref(),
            _ => None,
        }
        .ok_or_else(|| {
            ProtocolError::unsupported("retained CUDA capture requires explicit inference")
        })?;
        if profile.provider != InferenceProvider::Cuda {
            return Err(ProtocolError::invalid(
                "CUDA capture supplied to another provider",
            ));
        }
        owner
            .policy()
            .match_runtime(manifest.tools.onnx_runtime.as_ref())
            .map_err(unavailable)?;
        let execution = owner
            .worker_execution_supervised(
                &self.image,
                profile,
                &manifest.budget,
                if matches!(manifest.operation, WorkerOperation::Preview { .. }) {
                    GpuPriority::Interactive
                } else {
                    GpuPriority::Offline
                },
            )
            .map_err(unavailable)?;
        let device = self.devices.expectation();
        if device.device_uuid != execution.device_uuid
            || device.pci_bus_address != execution.pci_bus_address
            || device.device_minor != execution.device_minor
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CUDA capture/device binding differs",
            ));
        }
        manifest
            .tools
            .onnx_runtime
            .as_mut()
            .expect("checked runtime")
            .path = self
            .image
            .captured()
            .library_directory()
            .join("libonnxruntime.so.1");
        for identity in std::iter::once(&profile.model)
            .chain(std::iter::once(&execution.owner_policy))
            .chain(execution.libraries.iter().map(|lib| &lib.identity))
        {
            if !manifest.dependencies.contains(identity) {
                manifest.dependencies.push(identity.clone());
            }
        }
        manifest.cuda_execution = Some(execution);
        Ok(())
    }
}
impl Engine {
    pub(super) fn prepare_cuda_generation(
        &self,
        request: &Request,
        pack_settings: Option<&GenerationSettings>,
    ) -> PResult<Option<PreparedCudaGeneration>> {
        let previous = if let Command::ResumeJob { job_id } = &request.command {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(
                &db,
                request,
                request.session.as_ref().expect("authenticated"),
            )?;
            let job = job_snapshot(&db, job_id, project(request)?)?;
            if job.state != "interrupted" {
                return Err(ProtocolError::invalid("only interrupted jobs can resume"));
            }
            let raw: String = db
                .query_row(
                    "SELECT manifest FROM jobs WHERE id=?1",
                    [job_id.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            Some(decode::<WorkerRequest>(&raw)?)
        } else {
            None
        };
        let profile = match &request.command {
            Command::Generate { settings, .. } => settings
                .as_ref()
                .and_then(|settings| settings.inference_profile.as_ref()),
            Command::GenerateFromModelPack { .. } => {
                pack_settings.and_then(|settings| settings.inference_profile.as_ref())
            }
            Command::ResumeJob { .. } => previous.as_ref().and_then(|p| match &p.operation {
                WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
                _ => None,
            }),
            _ => None,
        };
        let Some(profile) = profile.filter(|profile| profile.provider == InferenceProvider::Cuda)
        else {
            return Ok(None);
        };
        let budget = crate::inference_budget::admit(
            ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: OUTPUT_LIMIT,
                wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128) as u64,
                cpu_threads: 2,
            },
            Some(profile),
            previous.as_ref().map(|manifest| &manifest.budget),
            self.config.max_ram_bytes,
        )?;
        if let Some(previous) = &previous {
            self.validate_dependencies(previous)?;
            self.validate_checkpoint(previous)?;
            let owner = self
                .cuda_owner_policy
                .as_ref()
                .ok_or_else(|| ProtocolError::unsupported("CUDA owner policy unavailable"))?;
            if previous
                .cuda_execution
                .as_ref()
                .is_none_or(|execution| execution.owner_policy != owner.status().source_identity)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "CUDA resume requires the same owner policy and fresh capture",
                ));
            }
        }
        self.capture_cuda_inference(
            request,
            profile,
            &budget,
            GpuPriority::Offline,
            &AtomicBool::new(false),
        )
        .map(Some)
    }

    pub(super) fn capture_cuda_inference(
        &self,
        request: &Request,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
        cancel: &AtomicBool,
    ) -> PResult<PreparedCudaGeneration> {
        self.capture_cuda_inference_with_admission(profile, budget, priority, cancel, |tx| {
            self.authorize(
                tx,
                request,
                request.session.as_ref().expect("authenticated"),
            )?;
            check_revision(tx, request)?;
            Ok(match &request.command {
                Command::ResumeJob { job_id } => Some(job_id.clone()),
                _ => None,
            })
        })
    }

    /// The caller's admission runs in the transaction that accounts for worker
    /// capacity and arms the worker fence. This method has no wire entry point.
    pub(super) fn capture_cuda_inference_with_admission(
        &self,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
        cancel: &AtomicBool,
        admission: impl FnOnce(&Transaction<'_>) -> PResult<Option<JobId>>,
    ) -> PResult<PreparedCudaGeneration> {
        if !cfg!(feature = "cuda") {
            return Err(ProtocolError::unsupported(
                "unsupported pinned provider; CUDA inference requires a CUDA-enabled build",
            ));
        }
        let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
            ProtocolError::unsupported(
                "unsupported pinned provider; no owner-configured CUDA policy",
            )
        })?;
        owner
            .policy()
            .authorize_profile(profile, budget, priority)
            .map_err(unavailable)?;
        crate::inference_profiles::admit_tensor_adapter(profile, budget).map_err(unavailable)?;
        if !owner
            .policy()
            .native_dependencies
            .libraries
            .iter()
            .any(|lib| lib.names.iter().any(|name| name == "libnvidia-ml.so.1"))
        {
            return Err(ProtocolError::unsupported(
                "CUDA inference policy requires captured NVML memory observation",
            ));
        }
        let total = owner
            .policy()
            .native_dependencies
            .validate()
            .map_err(unavailable)?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            let excluded_job = admission(&tx)?;
            let offline: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE state IN ('queued','running','fallback_pending') AND id!=?1",
                    [excluded_job.as_ref().map_or("", |job_id| job_id.as_str())],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if priority == GpuPriority::Offline
                && offline as usize >= self.config.max_jobs.saturating_sub(1)
            {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "offline work cannot consume reserved interactive worker slot",
                ));
            }
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        let devices = PinnedCudaDevices::open(&owner.policy().device).map_err(unavailable)?;
        let capture_budget = crate::native_capture::preparation_budget(total, budget.wall_time_ms)?;
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(budget.wall_time_ms))
            .ok_or_else(|| ProtocolError::invalid("native preparation deadline overflow"))?;
        let image = self.native_image_reuse.capture_with(
            &owner.policy().native_dependencies,
            cancel,
            deadline,
            || {
                crate::native_capture::capture_tracked_in_lease(
                    &self.config,
                    &self.pool,
                    &owner.policy().native_dependencies,
                    crate::native_capture::CaptureLimits {
                        budget: capture_budget,
                        address_space_bytes: 1536 << 20,
                    },
                    cancel,
                    self.worker_fence.clone(),
                    deadline,
                )
            },
        )?;
        Ok(PreparedCudaGeneration { image, devices })
    }
}
