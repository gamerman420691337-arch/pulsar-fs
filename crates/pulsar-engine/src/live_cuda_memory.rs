//! Supervised live-specific GPU observation and interactive admission.
use super::*;
use crate::cuda_mounts::{verify_live_bootstrap, OwnedLiveCudaMountCommand};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveMemoryReply {
    schema_version: u16,
    manifest_sha256: String,
    sample: MemorySample,
}
impl LiveMemoryReply {
    fn observation(&self, manifest: &LiveCudaInferenceManifest) -> Result<GpuMemoryObservation> {
        ensure!(
            self.schema_version == 1 && self.manifest_sha256 == manifest.identity_sha256()?,
            "CUDA live memory observation manifest differs"
        );
        self.sample.observation(&manifest.execution)
    }
    fn checked_observation(
        &self,
        manifest: &LiveCudaInferenceManifest,
        stage: &str,
    ) -> Result<GpuMemoryObservation> {
        self.observation(manifest).map_err(|error| {
            let age_ns = cuda_bootstrap::now_ns()
                .ok()
                .and_then(|now| now.checked_sub(self.sample.sample_started_ns));
            let message = format!(
                "CUDA live memory validation at {stage} (sample_age_ns={age_ns:?}): {error:#}"
            );
            error.context(message)
        })
    }
}
pub(crate) struct ObservedLiveCudaImage {
    reply: LiveMemoryReply,
    deadline_ns: u64,
    image: SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
}
pub(crate) struct AdmittedLiveCudaImage {
    manifest_sha256: String,
    deadline_ns: u64,
    image: SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
    reservation: Arc<Reservation>,
}
impl AdmittedLiveCudaImage {
    fn validate(&self, manifest: &LiveCudaInferenceManifest) -> Result<()> {
        ensure!(
            self.manifest_sha256 == manifest.identity_sha256()?,
            "CUDA live admitted image manifest differs"
        );
        cuda_bootstrap::deadline_instant(self.deadline_ns, manifest.stream.budget.wall_time_ms)?;
        Ok(())
    }
    pub(crate) fn into_launch_parts(
        self,
        manifest: &LiveCudaInferenceManifest,
    ) -> Result<(
        SupervisedNativeDependencies,
        PinnedCudaDevices,
        Arc<Reservation>,
        u64,
    )> {
        self.validate(manifest)?;
        Ok((self.image, self.devices, self.reservation, self.deadline_ns))
    }
}
impl ObservedLiveCudaImage {
    pub(crate) fn admit(
        self,
        pool: &ResourcePool,
        policy: &LoadedCudaOwnerPolicy,
        manifest: &LiveCudaInferenceManifest,
    ) -> Result<AdmittedLiveCudaImage> {
        manifest.validate()?;
        cuda_bootstrap::deadline_instant(self.deadline_ns, manifest.stream.budget.wall_time_ms)?;
        let stream = &manifest.stream;
        policy.policy().authorize_profile(
            &stream.profile,
            &stream.budget,
            GpuPriority::Interactive,
        )?;
        ensure!(
            manifest.execution.owner_policy == policy.status().source_identity,
            "CUDA live observation owner policy differs"
        );
        let expected = policy.worker_execution_supervised(
            &self.image,
            &stream.profile,
            &stream.budget,
            GpuPriority::Interactive,
        )?;
        ensure!(
            serde_json::to_vec(&expected)? == serde_json::to_vec(&manifest.execution)?,
            "CUDA live observation capture differs from owner policy"
        );
        let observation = self.reply.checked_observation(manifest, "pool admission")?;
        let reservation = pool.reserve_cuda_worker(
            stream.budget.memory_bytes,
            stream.budget.cpu_threads,
            stream
                .profile
                .cuda_resources
                .as_ref()
                .context("CUDA live resources missing")?,
            &observation,
            GpuPriority::Interactive,
        )?;
        Ok(AdmittedLiveCudaImage {
            manifest_sha256: self.reply.manifest_sha256,
            deadline_ns: self.deadline_ns,
            image: self.image,
            devices: self.devices,
            reservation: Arc::new(reservation),
        })
    }
}

pub(crate) fn observe(
    config: &EngineConfig,
    manifest: &LiveCudaInferenceManifest,
    deadline_ns: u64,
    image: SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
    custody: WorkerCustody,
    cancel: &AtomicBool,
) -> Result<ObservedLiveCudaImage> {
    let mut plan = OwnedLiveCudaMountCommand::memory_observer(
        config,
        manifest,
        deadline_ns,
        image,
        devices,
        custody,
    )?;
    let deadline =
        cuda_bootstrap::deadline_instant(deadline_ns, manifest.stream.budget.wall_time_ms)?;
    let transport = (|| -> Result<LiveMemoryReply> {
        ensure!(
            !cancel.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < deadline,
            "CUDA live memory observation cancelled or expired before spawn"
        );
        plan.spawn()?;
        let reply: LiveMemoryReply = probe(&mut plan.custody, &plan.witness, deadline, cancel)?;
        reply.checked_observation(manifest, "completed observer transport")?;
        Ok(reply)
    })();
    let reply = match transport {
        Ok(reply) => reply,
        Err(error) => {
            let _ = plan.custody.retire(Instant::now() + Duration::from_secs(2));
            return Err(error);
        }
    };
    let (image, devices) = plan.retire_with_image(deadline)?;
    reply.checked_observation(manifest, "image retirement")?;
    cuda_bootstrap::deadline_instant(deadline_ns, manifest.stream.budget.wall_time_ms)?;
    Ok(ObservedLiveCudaImage {
        reply,
        deadline_ns,
        image,
        devices,
    })
}
pub fn run_live_worker() -> Result<()> {
    cuda_mounts::clear_inherited_descriptors()?;
    let witness = read_message(&mut std::io::stdin().lock())?;
    cuda_bootstrap::reexec_live_memory(witness)
}
pub fn run_live_worker_exec() -> Result<()> {
    let relay = cuda_bootstrap::read_live_relay()?;
    cuda_mounts::clear_inherited_descriptors()?;
    let bootstrap = verify_live_bootstrap(&relay.witness)?;
    let sample = crate::worker::cuda_memory::measure_live(&bootstrap)?;
    let reply = LiveMemoryReply {
        schema_version: 1,
        manifest_sha256: bootstrap.manifest().identity_sha256()?,
        sample,
    };
    write_message(&mut std::io::stdout().lock(), &reply)?;
    Ok(())
}

#[cfg(test)]
#[path = "live_cuda_memory_tests.rs"]
pub(super) mod tests;
