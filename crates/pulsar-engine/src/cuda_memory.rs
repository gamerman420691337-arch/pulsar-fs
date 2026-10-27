//! Supervised device-bound memory observations; cooperative admission, not quotas.
#![cfg(target_os = "linux")]
use crate::{
    cuda_bootstrap::{self, BootstrapRole},
    cuda_devices::PinnedCudaDevices,
    cuda_mounts::{self, OwnedCudaMountCommand},
    cuda_owner_policy::LoadedCudaOwnerPolicy,
    native_capture::SupervisedNativeDependencies,
    resources::{GpuMemoryObservation, GpuPriority, Reservation, ResourcePool},
    worker::cuda_identity::CudaSdkObservation,
    worker_retirement::WorkerCustody,
    EngineConfig,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryReply {
    pub schema_version: u16,
    pub request_sha256: String,
    pub device: CudaSdkObservation,
    pub nvml_total_bytes: u64,
    pub nvml_free_bytes: u64,
    pub nvml_used_bytes: u64,
    /// Same-host CLOCK_MONOTONIC, sampled before the memory query.
    pub sample_started_ns: u64,
}
impl MemoryReply {
    fn observation(&self, request: &WorkerRequest) -> Result<GpuMemoryObservation> {
        let now = Instant::now();
        let age = cuda_bootstrap::now_ns()?
            .checked_sub(self.sample_started_ns)
            .context("CUDA memory observation is from the future")?;
        self.observation_at(request, now, Duration::from_nanos(age))
    }
    fn observation_at(
        &self,
        request: &WorkerRequest,
        now: Instant,
        age: Duration,
    ) -> Result<GpuMemoryObservation> {
        let execution = request
            .cuda_execution
            .as_ref()
            .context("CUDA declaration missing")?;
        ensure!(
            self.schema_version == 1 && self.request_sha256 == cuda_mounts::request_hash(request)?,
            "CUDA memory observation request differs"
        );
        self.sample().observation_at(execution, now, age)
    }
    fn sample(&self) -> MemorySample {
        MemorySample {
            device: self.device.clone(),
            nvml_total_bytes: self.nvml_total_bytes,
            nvml_free_bytes: self.nvml_free_bytes,
            nvml_used_bytes: self.nvml_used_bytes,
            sample_started_ns: self.sample_started_ns,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemorySample {
    pub(crate) device: CudaSdkObservation,
    pub(crate) nvml_total_bytes: u64,
    pub(crate) nvml_free_bytes: u64,
    pub(crate) nvml_used_bytes: u64,
    pub(crate) sample_started_ns: u64,
}
impl MemorySample {
    fn observation(&self, execution: &WorkerCudaExecution) -> Result<GpuMemoryObservation> {
        let now = Instant::now();
        let age = cuda_bootstrap::now_ns()?
            .checked_sub(self.sample_started_ns)
            .context("CUDA memory observation is from the future")?;
        self.observation_at(execution, now, Duration::from_nanos(age))
    }
    fn observation_at(
        &self,
        execution: &WorkerCudaExecution,
        now: Instant,
        age: Duration,
    ) -> Result<GpuMemoryObservation> {
        ensure!(
            self.device
                .device_uuid
                .eq_ignore_ascii_case(&execution.device_uuid)
                && self.device.pci_bus_address == execution.pci_bus_address
                && u32::from(self.device.device_ordinal) == execution.device_ordinal
                && self.device.visible_device_count > execution.device_ordinal
                && self.device.visible_device_count <= 1024
                && self.device.driver_version > 0,
            "CUDA memory observation device differs"
        );
        ensure!(
            self.device.total_bytes > 0
                && self.device.total_bytes <= 1u64 << 40
                && self.nvml_total_bytes > 0
                && self.nvml_total_bytes <= 1u64 << 40
                && self.nvml_free_bytes.checked_add(self.nvml_used_bytes)
                    == Some(self.nvml_total_bytes),
            "CUDA memory observation capacities are inconsistent"
        );
        ensure!(
            self.sample_started_ns > 0 && age <= Duration::from_secs(1),
            "CUDA memory observation is stale"
        );
        // Different APIs expose different total capacities. Do not equate them
        // or subtract already used bytes from the SDK total a second time.
        let total = self.device.total_bytes.min(self.nvml_total_bytes);
        Ok(GpuMemoryObservation {
            device_uuid: execution.device_uuid.clone(),
            device_ordinal: execution.device_ordinal,
            total_bytes: total,
            available_bytes: total.min(self.nvml_free_bytes),
            observed_at: now
                .checked_sub(age)
                .context("CUDA memory timestamp underflow")?,
        })
    }
}

/// Constructed only after a supervised observer, fresh response checks and exact
/// retirement. Image/device ownership survives the probe for subsequent launch.
pub(crate) struct ObservedCudaImage {
    reply: MemoryReply,
    image: SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
}
pub(crate) struct AdmittedCudaImage {
    request_sha256: String,
    pub image: SupervisedNativeDependencies,
    pub devices: PinnedCudaDevices,
    pub reservation: std::sync::Arc<Reservation>,
}
impl AdmittedCudaImage {
    pub(crate) fn into_launch_parts(
        self,
        request: &WorkerRequest,
    ) -> Result<(
        SupervisedNativeDependencies,
        PinnedCudaDevices,
        std::sync::Arc<Reservation>,
    )> {
        ensure!(
            self.request_sha256 == cuda_mounts::request_hash(request)?,
            "CUDA admitted image request differs"
        );
        Ok((self.image, self.devices, self.reservation))
    }
}
impl ObservedCudaImage {
    /// Recheck freshness immediately before the aggregate pool's locked check.
    /// Neither a caller-authored observation nor another request can enter here.
    pub(crate) fn admit(
        self,
        pool: &ResourcePool,
        policy: &LoadedCudaOwnerPolicy,
        request: &WorkerRequest,
        priority: GpuPriority,
    ) -> Result<AdmittedCudaImage> {
        let profile = match &request.operation {
            WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
            WorkerOperation::Preview {
                inference_profile, ..
            } => inference_profile.as_deref(),
            _ => None,
        }
        .context("CUDA inference profile missing")?;
        policy
            .policy()
            .authorize_profile(profile, &request.budget, priority)?;
        ensure!(
            request
                .cuda_execution
                .as_ref()
                .context("CUDA declaration missing")?
                .owner_policy
                == policy.status().source_identity,
            "CUDA observation owner policy differs"
        );
        let expected =
            policy.worker_execution_supervised(&self.image, profile, &request.budget, priority)?;
        ensure!(
            serde_json::to_vec(&expected)?
                == serde_json::to_vec(request.cuda_execution.as_ref().unwrap())?,
            "CUDA observation capture differs from owner policy"
        );
        let observation = self.reply.observation(request)?;
        let reservation = pool.reserve_cuda_worker(
            request.budget.memory_bytes,
            request.budget.cpu_threads,
            profile
                .cuda_resources
                .as_ref()
                .context("CUDA resources missing")?,
            &observation,
            priority,
        )?;
        Ok(AdmittedCudaImage {
            request_sha256: self.reply.request_sha256,
            image: self.image,
            devices: self.devices,
            reservation: std::sync::Arc::new(reservation),
        })
    }
}

pub(crate) fn observe(
    config: &EngineConfig,
    request: &WorkerRequest,
    image: SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
    custody: WorkerCustody,
    cancel: &AtomicBool,
) -> Result<ObservedCudaImage> {
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(request.budget.wall_time_ms))
        .context("CUDA memory deadline overflow")?;
    let mut plan =
        OwnedCudaMountCommand::memory_observer(config, request, image, devices, custody)?;
    let transport = (|| -> Result<MemoryReply> {
        ensure!(
            !cancel.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < deadline,
            "CUDA memory observation cancelled or expired before spawn"
        );
        plan.spawn()?;
        let reply: MemoryReply = probe(&mut plan.custody, &plan.witness, deadline, cancel)?;
        reply.observation(request)?;
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
    // Retirement must never renew the measured sample's freshness.
    reply.observation(request)?;
    Ok(ObservedCudaImage {
        reply,
        image,
        devices,
    })
}

/// Shared bounded reply/EOF/child-status transport. Caller owns spawn, reply
/// identity validation and exact retirement of the retained assets.
fn probe<R: serde::de::DeserializeOwned, T: Serialize>(
    custody: &mut WorkerCustody,
    witness: &T,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<R> {
    let (mut input, mut output, diagnostics) = {
        let mut child = custody.child();
        (
            child
                .stdin
                .take()
                .context("memory observer stdin missing")?,
            child
                .stdout
                .take()
                .context("memory observer stdout missing")?,
            child
                .stderr
                .take()
                .context("memory observer stderr missing")?,
        )
    };
    crate::launch_pipes::nonblocking(&input)?;
    crate::launch_pipes::nonblocking(&output)?;
    crate::launch_pipes::nonblocking(&diagnostics)?;
    let mut monitor = crate::launch_pipes::Monitor::new(deadline, cancel, diagnostics);
    write_message(&mut monitor.write(&mut input), witness)?;
    drop(input);
    let reply_result: std::result::Result<R, TransportError> =
        read_message(&mut monitor.read(&mut output));
    let reply = match reply_result {
        Ok(reply) => reply,
        Err(error) => {
            let _ = monitor.tick();
            let status = custody.child().try_wait();
            return Err(error).with_context(|| {
                format!(
                    "CUDA memory observer response failed ({status:?}): {}",
                    String::from_utf8_lossy(monitor.diagnostics())
                )
            });
        }
    };
    let mut trailing = [0u8; 1];
    ensure!(
        monitor.read(&mut output).read(&mut trailing)? == 0,
        "CUDA memory observer sent trailing output"
    );
    let status = loop {
        monitor.tick()?;
        if let Some(status) = custody.child().try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    ensure!(
        status.success(),
        "CUDA memory observer failed: {}",
        String::from_utf8_lossy(monitor.diagnostics())
    );
    Ok(reply)
}

/// Initial trusted executable verifies mounts before switching captured loaders.
pub fn run_worker() -> Result<()> {
    cuda_mounts::clear_inherited_descriptors()?;
    let witness = read_message(&mut std::io::stdin().lock())?;
    cuda_bootstrap::reexec_as(witness, BootstrapRole::MemoryObservation)
}
/// Private sealed relay dispatch; foreign code executes only inside this child.
pub fn run_worker_exec() -> Result<()> {
    let relay = cuda_bootstrap::read_relay_for(BootstrapRole::MemoryObservation)?;
    cuda_mounts::clear_inherited_descriptors()?;
    let deadline = cuda_bootstrap::deadline_instant(
        relay.deadline_ns,
        relay.witness.request.budget.wall_time_ms,
    )?;
    let bootstrap = cuda_mounts::verify_memory_bootstrap_at(&relay.witness, deadline)?;
    let reply = crate::worker::cuda_memory::measure(&bootstrap)?;
    write_message(&mut std::io::stdout().lock(), &reply)?;
    Ok(())
}

#[cfg(test)]
#[path = "cuda_memory_tests.rs"]
mod tests;

#[path = "live_cuda_memory.rs"]
mod live;
pub(crate) use live::observe as observe_live;
pub(crate) use live::AdmittedLiveCudaImage;
pub use live::{run_live_worker, run_live_worker_exec};
