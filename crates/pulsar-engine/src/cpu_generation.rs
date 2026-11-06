//! Explicit owner-selected CPU image preparation. Capture is not qualification.
use super::*;
use crate::{native_capture::SupervisedNativeDependencies, resources::GpuPriority};

pub(super) struct PreparedCpuGeneration {
    pub(super) image: SupervisedNativeDependencies,
    pub(super) descriptor: CpuRuntimeImage,
}

pub(super) fn image_dependency(manifest: &WorkerRequest) -> Option<&ArtifactIdentity> {
    manifest.dependencies.iter().find(|d| {
        d.artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    })
}
fn profile(manifest: &WorkerRequest) -> Option<&DeclaredInferenceProfile> {
    match &manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        _ => None,
    }
}
impl PreparedCpuGeneration {
    pub(super) fn bind(&self, engine: &Engine, manifest: &mut WorkerRequest) -> PResult<()> {
        let owner = engine
            .cpu_owner_policy
            .as_ref()
            .ok_or_else(|| ProtocolError::unsupported("captured CPU owner policy unavailable"))?;
        let selected = profile(manifest).ok_or_else(|| {
            ProtocolError::invalid("captured CPU execution requires an explicit inference profile")
        })?;
        owner.match_profile(selected).map_err(unavailable)?;
        let expected = owner
            .captured_image_supervised(&self.image)
            .map_err(unavailable)?;
        if encode(&expected)? != encode(&self.descriptor)? || manifest.cuda_execution.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "captured CPU image differs from selected policy",
            ));
        }
        let runtime = owner.runtime_identity().map_err(unavailable)?;
        manifest.tools.onnx_runtime = Some(ArtifactReference {
            identity: runtime,
            path: Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY).join("libonnxruntime.so.1"),
        });
        let image = self.descriptor.identity()?;
        for identity in std::iter::once(&image)
            .chain(std::iter::once(&self.descriptor.owner_policy))
            .chain(
                self.descriptor
                    .libraries
                    .iter()
                    .map(|library| &library.identity),
            )
        {
            if !manifest.dependencies.contains(identity) {
                manifest.dependencies.push(identity.clone());
            }
        }
        Ok(())
    }
}

impl Engine {
    /// Checks retained mode without reading the retired capture directory.
    pub(super) fn validate_cpu_image_dependencies(
        &self,
        manifest: &WorkerRequest,
    ) -> PResult<bool> {
        let Some(binding) = image_dependency(manifest) else {
            return Ok(false);
        };
        if manifest
            .dependencies
            .iter()
            .filter(|d| {
                d.artifact_id
                    .as_str()
                    .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
            })
            .count()
            != 1
            || manifest.cuda_execution.is_some()
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "CPU image dependency mode is ambiguous",
            ));
        }
        let owner = self.cpu_owner_policy.as_ref().ok_or_else(|| {
            ProtocolError::unsupported(
                "retained closed CPU attempt requires its owner-selected runtime policy",
            )
        })?;
        let selected = profile(manifest)
            .ok_or_else(|| ProtocolError::invalid("retained CPU image profile missing"))?;
        owner.match_profile(selected).map_err(unavailable)?;
        let image = owner.image().map_err(unavailable)?;
        if image.identity()? != *binding
            || !manifest.dependencies.contains(&image.owner_policy)
            || image
                .libraries
                .iter()
                .any(|library| !manifest.dependencies.contains(&library.identity))
            || manifest.tools.onnx_runtime.as_ref().is_none_or(|runtime| {
                runtime.identity != selected.runtime
                    || runtime.path
                        != Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY)
                            .join("libonnxruntime.so.1")
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "closed CPU attempt requires its exact original runtime image",
            ));
        }
        Ok(true)
    }

    pub(super) fn prepare_cpu_generation(
        &self,
        request: &Request,
        pack_settings: Option<&GenerationSettings>,
    ) -> PResult<Option<PreparedCpuGeneration>> {
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
                    |row| row.get(0),
                )
                .map_err(internal)?;
            let manifest: WorkerRequest = decode(&raw)?;
            let retained = crate::cpu_runtime_db::retained(&db, &manifest).map_err(unavailable)?;
            Some((manifest, retained))
        } else {
            None
        };
        let selected = match &request.command {
            Command::Generate { settings, .. } => {
                settings.as_ref().and_then(|s| s.inference_profile.as_ref())
            }
            Command::GenerateFromModelPack { .. } => {
                pack_settings.and_then(|s| s.inference_profile.as_ref())
            }
            Command::ResumeJob { .. } => previous.as_ref().and_then(|(m, _)| profile(m)),
            _ => return Ok(None),
        };
        if selected.is_some_and(|p| p.provider != InferenceProvider::Cpu) {
            return Ok(None);
        }
        let Some(owner) = self.cpu_owner_policy.as_ref() else {
            if previous.as_ref().is_some_and(|(_, image)| image.is_some()) {
                return Err(ProtocolError::unsupported(
                    "closed CPU resume requires its owner-selected runtime policy",
                ));
            }
            return Ok(None);
        };
        let model_backed = match &request.command {
            Command::Generate { model_path, .. } => model_path.is_some(),
            Command::GenerateFromModelPack { .. } => true,
            Command::ResumeJob { .. } => previous.as_ref().is_some_and(|(m, _)| {
                matches!(
                    &m.operation,
                    WorkerOperation::Generate { model: Some(_), .. }
                )
            }),
            _ => false,
        };
        if !model_backed {
            return Ok(None);
        }
        let selected = selected.ok_or_else(|| {
            ProtocolError::invalid(
                "configured captured CPU runtime requires an explicit inference profile",
            )
        })?;
        owner.match_profile(selected).map_err(unavailable)?;
        let budget = crate::inference_budget::admit(
            ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: OUTPUT_LIMIT,
                wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128) as u64,
                cpu_threads: 2,
            },
            Some(selected),
            previous.as_ref().map(|(m, _)| &m.budget),
            self.config.max_ram_bytes,
        )?;
        if let Some((manifest, image)) = &previous {
            let image = image.as_ref().ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "CPU runtime mode changed; create a fresh generation instead of resuming",
                )
            })?;
            if encode(image)? != encode(&owner.image().map_err(unavailable)?)? {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "CPU resume requires the exact original runtime policy and image",
                ));
            }
            self.validate_dependencies(manifest)?;
            self.validate_checkpoint(manifest)?;
        }
        self.capture_cpu_inference(
            request,
            selected,
            &budget,
            GpuPriority::Offline,
            &AtomicBool::new(false),
        )
        .map(Some)
    }

    pub(super) fn capture_cpu_inference(
        &self,
        request: &Request,
        selected: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
        cancel: &AtomicBool,
    ) -> PResult<PreparedCpuGeneration> {
        self.capture_cpu_inference_with_admission(selected, budget, priority, cancel, |tx| {
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
    pub(super) fn capture_cpu_inference_with_admission(
        &self,
        selected: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
        cancel: &AtomicBool,
        admission: impl FnOnce(&Transaction<'_>) -> PResult<Option<JobId>>,
    ) -> PResult<PreparedCpuGeneration> {
        let owner = self
            .cpu_owner_policy
            .as_ref()
            .ok_or_else(|| ProtocolError::unsupported("captured CPU owner policy unavailable"))?;
        owner.match_profile(selected).map_err(unavailable)?;
        crate::inference_profiles::admit_tensor_adapter(selected, budget).map_err(unavailable)?;
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
                    |row| row.get(0),
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
        let capture_ms = budget.wall_time_ms.min(
            u64::try_from(self.config.native_image_preparation_timeout.as_millis())
                .unwrap_or(u64::MAX),
        );
        if capture_ms == 0 {
            return Err(ProtocolError::invalid("CPU capture deadline is empty"));
        }
        let capture_budget = crate::native_capture::preparation_budget(total, capture_ms)?;
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(capture_ms))
            .ok_or_else(|| ProtocolError::invalid("CPU capture deadline overflow"))?;
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
        let descriptor = owner
            .captured_image_supervised(&image)
            .map_err(unavailable)?;
        Ok(PreparedCpuGeneration { image, descriptor })
    }
}
