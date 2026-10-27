//! Admission is aggregate and explicit; inability to reserve is not a quality downgrade.
use pulsar_protocol::{CudaResourceLimits, ErrorCode, ProtocolError};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Owner-configured accounting policy; not discovery or hardware enforcement.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuDeviceBudget {
    pub device_uuid: String,
    pub device_ordinal: u32,
    pub maximum_reserved_bytes: u64,
    pub interactive_reserved_bytes: u64,
    pub control_reserve_bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuPriority {
    Offline,
    Interactive,
}

/// Trusted broker supplies fresh facts. Constructing this is not authorization.
pub struct GpuMemoryObservation {
    pub device_uuid: String,
    pub device_ordinal: u32,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub observed_at: Instant,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct GpuUsage {
    pub reserved_bytes: u64,
    pub offline_reserved_bytes: u64,
}
struct GpuClaim {
    uuid: String,
    bytes: u64,
    priority: GpuPriority,
    observed_at: Instant,
}

use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct ResourcePool {
    inner: Arc<Mutex<Usage>>,
    max_memory: u64,
    max_jobs: usize,
    max_cpu_threads: u16,
    gpu_devices: Arc<BTreeMap<String, GpuDeviceBudget>>,
}
#[derive(Default)]
struct Usage {
    memory: u64,
    jobs: usize,
    cpu_threads: u32,
    storage: u64,
    gpu: BTreeMap<String, GpuUsage>,
    #[cfg(target_os = "linux")]
    listeners: Vec<ReleaseListener>,
    #[cfg(target_os = "linux")]
    next_listener: u64,
}
#[cfg(target_os = "linux")]
struct ReleaseListener {
    id: u64,
    owner: std::thread::ThreadId,
    sender: std::sync::mpsc::SyncSender<()>,
}
#[cfg(target_os = "linux")]
pub(crate) struct ReleaseWatcher {
    pool: ResourcePool,
    id: u64,
    receiver: std::sync::mpsc::Receiver<()>,
}
#[cfg(target_os = "linux")]
impl ReleaseWatcher {
    pub(crate) fn wait(&self, timeout: Duration) -> Result<(), std::sync::mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}
#[cfg(target_os = "linux")]
impl Drop for ReleaseWatcher {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.pool.inner.lock() {
            usage.listeners.retain(|entry| entry.id != self.id);
        }
    }
}
#[cfg(target_os = "linux")]
fn notify_release(usage: &mut Usage) {
    let releasing = std::thread::current().id();
    usage.listeners.retain(|entry| {
        if entry.owner == releasing {
            return true;
        }
        match entry.sender.try_send(()) {
            Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => true,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
        }
    });
}
pub struct Reservation {
    pool: ResourcePool,
    memory: u64,
    storage: u64,
    worker: bool,
    cpu_threads: u16,
    gpu: Option<GpuClaim>,
}
impl Reservation {
    pub(crate) fn require_cpu_worker(
        &self,
        pool: &ResourcePool,
        memory: u64,
        cpu_threads: u16,
    ) -> Result<(), ProtocolError> {
        if !Arc::ptr_eq(&self.pool.inner, &pool.inner)
            || self.memory != memory
            || self.cpu_threads != cpu_threads
            || !self.worker
            || self.gpu.is_some()
            || self.storage != 0
        {
            return Err(ProtocolError::invalid(
                "assistant admission loan differs from this pool/budget",
            ));
        }
        Ok(())
    }
}
impl ResourcePool {
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn release_watcher_count(&self) -> usize {
        self.inner.lock().unwrap().listeners.len()
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn watch_releases(&self) -> Result<ReleaseWatcher, ProtocolError> {
        let mut usage = self.inner.lock().map_err(|_| {
            ProtocolError::new(ErrorCode::Internal, "resource accounting unavailable")
        })?;
        if usage.listeners.len() >= 8 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "resource release watcher capacity exhausted",
            ));
        }
        usage.next_listener = usage
            .next_listener
            .checked_add(1)
            .ok_or_else(|| ProtocolError::invalid("resource release watcher identity exhausted"))?;
        let id = usage.next_listener;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        usage.listeners.push(ReleaseListener {
            id,
            owner: std::thread::current().id(),
            sender,
        });
        Ok(ReleaseWatcher {
            pool: self.clone(),
            id,
            receiver,
        })
    }
    pub fn new(max_memory: u64, max_jobs: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Usage::default())),
            max_memory,
            max_jobs,
            max_cpu_threads: max_jobs.saturating_mul(2).min(u16::MAX as usize) as u16,
            gpu_devices: Arc::new(BTreeMap::new()),
        }
    }
    /// Set the owner ceiling before the pool is shared or any claim exists.
    pub fn with_cpu_limit(mut self, cpu_threads: u16) -> Result<Self, ProtocolError> {
        if cpu_threads == 0 || cpu_threads > 1024 || Arc::strong_count(&self.inner) != 1 {
            return Err(ProtocolError::invalid(
                "invalid or already shared CPU pool policy",
            ));
        }
        let used = self.inner.lock().map_err(|_| {
            ProtocolError::new(ErrorCode::Internal, "resource accounting unavailable")
        })?;
        if used.jobs != 0 || used.memory != 0 || used.storage != 0 {
            return Err(ProtocolError::invalid("CPU pool policy has active claims"));
        }
        drop(used);
        self.max_cpu_threads = cpu_threads;
        Ok(self)
    }
    pub fn reserved_cpu_threads(&self) -> Result<u32, ProtocolError> {
        self.inner
            .lock()
            .map(|used| used.cpu_threads)
            .map_err(|_| ProtocolError::new(ErrorCode::Internal, "resource accounting unavailable"))
    }
    pub fn new_with_gpu_devices(
        max_memory: u64,
        max_jobs: usize,
        devices: Vec<GpuDeviceBudget>,
    ) -> Result<Self, ProtocolError> {
        if devices.is_empty() || devices.len() > 64 {
            return Err(ProtocolError::invalid("invalid GPU accounting inventory"));
        }
        let mut inventory = BTreeMap::new();
        let mut ordinals = std::collections::BTreeSet::new();
        for device in devices {
            pulsar_protocol::validate_cuda_device_identity(
                device.device_ordinal,
                &device.device_uuid,
            )?;
            if device.maximum_reserved_bytes == 0
                || device.maximum_reserved_bytes > (1u64 << 40)
                || device.interactive_reserved_bytes > device.maximum_reserved_bytes
                || device.control_reserve_bytes == 0
                || device.control_reserve_bytes > (1u64 << 40)
                || !ordinals.insert(device.device_ordinal)
                || inventory.contains_key(&device.device_uuid)
            {
                return Err(ProtocolError::invalid(
                    "invalid or duplicate GPU accounting policy",
                ));
            }
            inventory.insert(device.device_uuid.clone(), device);
        }
        let mut pool = Self::new(max_memory, max_jobs);
        pool.gpu_devices = Arc::new(inventory);
        Ok(pool)
    }

    pub fn reserve_cuda(
        &self,
        memory: u64,
        limits: &CudaResourceLimits,
        observation: &GpuMemoryObservation,
        priority: GpuPriority,
    ) -> Result<Reservation, ProtocolError> {
        self.reserve_cuda_worker(memory, 2, limits, observation, priority)
    }
    pub fn reserve_cuda_worker(
        &self,
        memory: u64,
        cpu_threads: u16,
        limits: &CudaResourceLimits,
        observation: &GpuMemoryObservation,
        priority: GpuPriority,
    ) -> Result<Reservation, ProtocolError> {
        limits.validate(memory)?;
        let device = self.gpu_devices.get(&limits.device_uuid).ok_or_else(|| {
            ProtocolError::unsupported("GPU is not in the owner-configured resource pool")
        })?;
        let age = Instant::now().checked_duration_since(observation.observed_at);
        if device.device_ordinal != limits.device_ordinal
            || observation.device_uuid != limits.device_uuid
            || observation.device_ordinal != limits.device_ordinal
            || observation.available_bytes > observation.total_bytes
            || age.is_none()
            || age.is_some_and(|age| age > Duration::from_secs(1))
        {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "GPU memory observation is stale or inconsistent",
            ));
        }
        let minimum_total = device
            .maximum_reserved_bytes
            .checked_add(device.control_reserve_bytes)
            .ok_or_else(|| ProtocolError::invalid("GPU accounting policy overflow"))?;
        let required_free = limits
            .device_reservation_bytes
            .checked_add(device.control_reserve_bytes)
            .ok_or_else(|| ProtocolError::invalid("GPU admission overflow"))?;
        if observation.total_bytes < minimum_total || observation.available_bytes < required_free {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "observed GPU capacity cannot preserve the control reserve",
            ));
        }
        self.reserve_request(
            memory,
            true,
            cpu_threads,
            Some(GpuClaim {
                uuid: limits.device_uuid.clone(),
                bytes: limits.device_reservation_bytes,
                priority,
                observed_at: observation.observed_at,
            }),
        )
    }

    pub fn gpu_usage(&self, uuid: &str) -> Result<GpuUsage, ProtocolError> {
        if !self.gpu_devices.contains_key(uuid) {
            return Err(ProtocolError::invalid("unknown GPU accounting identity"));
        }
        let used = self
            .inner
            .lock()
            .map_err(|_| ProtocolError::new(ErrorCode::Internal, "GPU accounting unavailable"))?;
        Ok(used.gpu.get(uuid).copied().unwrap_or_default())
    }

    pub fn reserve(&self, memory: u64) -> Result<Reservation, ProtocolError> {
        self.reserve_worker(memory, 2)
    }
    pub fn reserve_worker(
        &self,
        memory: u64,
        cpu_threads: u16,
    ) -> Result<Reservation, ProtocolError> {
        if cpu_threads == 0 {
            return Err(ProtocolError::invalid("worker CPU claim must be positive"));
        }
        self.reserve_request(memory, true, cpu_threads, None)
    }
    pub fn reserve_memory(&self, memory: u64) -> Result<Reservation, ProtocolError> {
        self.reserve_request(memory, false, 0, None)
    }
    fn reserve_request(
        &self,
        memory: u64,
        worker: bool,
        cpu_threads: u16,
        gpu: Option<GpuClaim>,
    ) -> Result<Reservation, ProtocolError> {
        let mut used = self.inner.lock().map_err(|_| {
            ProtocolError::new(ErrorCode::Internal, "resource accounting unavailable")
        })?;
        let next = used.memory.checked_add(memory).ok_or_else(|| {
            ProtocolError::new(ErrorCode::ResourceExhausted, "resource budget overflow")
        })?;
        let next_cpu = used
            .cpu_threads
            .checked_add(u32::from(cpu_threads))
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::ResourceExhausted, "CPU accounting overflow")
            })?;
        if memory == 0
            || (worker && cpu_threads == 0)
            || (worker && used.jobs >= self.max_jobs)
            || next > self.max_memory
            || next_cpu > u32::from(self.max_cpu_threads)
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "aggregate worker admission budget exhausted",
            ));
        }
        #[cfg(target_os = "linux")]
        {
            if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
                let available = info
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("MemAvailable:")
                            .and_then(|s| s.split_whitespace().next())
                            .and_then(|s| s.parse::<u64>().ok())
                    })
                    .unwrap_or(0)
                    .saturating_mul(1024);
                if available < memory.saturating_add(512 * 1024 * 1024) {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "insufficient available memory including control reserve",
                    ));
                }
            } else {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "cannot establish available worker memory",
                ));
            }
        }
        let next_gpu = if let Some(claim) = &gpu {
            if Instant::now()
                .checked_duration_since(claim.observed_at)
                .is_none_or(|age| age > Duration::from_secs(1))
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "GPU memory observation expired before atomic admission",
                ));
            }
            let device = self
                .gpu_devices
                .get(&claim.uuid)
                .ok_or_else(|| ProtocolError::invalid("unknown GPU accounting identity"))?;
            let current = used.gpu.get(&claim.uuid).copied().unwrap_or_default();
            let total = current
                .reserved_bytes
                .checked_add(claim.bytes)
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::ResourceExhausted, "GPU accounting overflow")
                })?;
            let offline = current
                .offline_reserved_bytes
                .checked_add(if claim.priority == GpuPriority::Offline {
                    claim.bytes
                } else {
                    0
                })
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::ResourceExhausted, "GPU accounting overflow")
                })?;
            if total > device.maximum_reserved_bytes
                || offline > device.maximum_reserved_bytes - device.interactive_reserved_bytes
            {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "aggregate GPU admission budget exhausted",
                ));
            }
            Some((
                claim.uuid.clone(),
                GpuUsage {
                    reserved_bytes: total,
                    offline_reserved_bytes: offline,
                },
            ))
        } else {
            None
        };
        if let Some((uuid, usage)) = next_gpu {
            used.gpu.insert(uuid, usage);
        }
        used.cpu_threads = next_cpu;
        used.memory = next;
        used.jobs += usize::from(worker);
        Ok(Reservation {
            pool: self.clone(),
            memory,
            storage: 0,
            worker,
            cpu_threads,
            gpu,
        })
    }
    pub fn reserved_storage(&self) -> u64 {
        self.inner
            .lock()
            .map(|used| used.storage)
            .unwrap_or(u64::MAX)
    }
}
impl Reservation {
    /// Move the storage charge without changing aggregate accounting. The worker
    /// claim can then retire independently while byte custody retains storage.
    pub(crate) fn split_storage(&mut self) -> Result<Self, ProtocolError> {
        if self.storage == 0 {
            return Err(ProtocolError::invalid(
                "reservation has no storage to transfer",
            ));
        }
        Ok(Self {
            pool: self.pool.clone(),
            memory: 0,
            storage: std::mem::take(&mut self.storage),
            worker: false,
            cpu_threads: 0,
            gpu: None,
        })
    }
    /// Move a storage-only claim between reservations in the same accounting
    /// pool without releasing or double charging the reserved bytes.
    pub(crate) fn absorb_storage(&mut self, storage: &mut Self) -> Result<(), ProtocolError> {
        if self.storage != 0
            || storage.storage == 0
            || storage.memory != 0
            || storage.worker
            || storage.gpu.is_some()
            || !Arc::ptr_eq(&self.pool.inner, &storage.pool.inner)
        {
            return Err(ProtocolError::invalid(
                "storage transfer requires a same-pool storage-only claim",
            ));
        }
        self.storage = std::mem::take(&mut storage.storage);
        Ok(())
    }
    pub(crate) fn admits_worker_memory(&self, bytes: u64) -> bool {
        self.worker && bytes > 0 && bytes <= self.memory
    }
    pub(crate) fn admits_worker_budget(&self, bytes: u64, threads: u16) -> bool {
        self.admits_worker_memory(bytes) && threads > 0 && threads <= self.cpu_threads
    }
    /// Only after independent CPU worker retirement. Keep storage charged for
    /// the captured image's lifetime while releasing its RAM and worker slot.
    pub fn release_cpu_worker(&mut self) -> Result<(), ProtocolError> {
        if self.gpu.is_some() {
            return Err(ProtocolError::invalid(
                "GPU reservation cannot retire as a CPU capture",
            ));
        }
        let mut used = self.pool.inner.lock().map_err(|_| {
            ProtocolError::new(ErrorCode::Internal, "resource accounting unavailable")
        })?;
        used.memory = used.memory.saturating_sub(self.memory);
        used.jobs = used.jobs.saturating_sub(usize::from(self.worker));
        used.cpu_threads = used.cpu_threads.saturating_sub(u32::from(self.cpu_threads));
        #[cfg(target_os = "linux")]
        if self.memory != 0 || self.worker {
            notify_release(&mut used);
        }
        self.memory = 0;
        self.cpu_threads = 0;
        self.worker = false;
        Ok(())
    }
    pub fn with_storage(mut self, bytes: u64, available: u64) -> Result<Self, ProtocolError> {
        self.attach_storage(bytes, available)?;
        Ok(self)
    }
    /// Attach storage without consuming custody on failure.
    pub(crate) fn attach_storage(
        &mut self,
        bytes: u64,
        available: u64,
    ) -> Result<(), ProtocolError> {
        if self.storage != 0 {
            return Err(ProtocolError::invalid(
                "storage is already attached to this reservation",
            ));
        }
        {
            let mut used = self.pool.inner.lock().map_err(|_| {
                ProtocolError::new(ErrorCode::Internal, "storage accounting unavailable")
            })?;
            let next = used.storage.checked_add(bytes).ok_or_else(|| {
                ProtocolError::new(ErrorCode::ResourceExhausted, "storage reservation overflow")
            })?;
            if available < next.saturating_add(16 * 1024 * 1024) {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "insufficient unreserved output storage",
                ));
            }
            used.storage = next;
            self.storage = bytes;
        }
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut used) = self.pool.inner.lock() {
            if let Some(claim) = &self.gpu {
                if let Some(gpu) = used.gpu.get_mut(&claim.uuid) {
                    gpu.reserved_bytes = gpu.reserved_bytes.saturating_sub(claim.bytes);
                    if claim.priority == GpuPriority::Offline {
                        gpu.offline_reserved_bytes =
                            gpu.offline_reserved_bytes.saturating_sub(claim.bytes);
                    }
                }
            }
            used.memory = used.memory.saturating_sub(self.memory);
            used.jobs = used.jobs.saturating_sub(usize::from(self.worker));
            used.cpu_threads = used.cpu_threads.saturating_sub(u32::from(self.cpu_threads));
            used.storage = used.storage.saturating_sub(self.storage);
            #[cfg(target_os = "linux")]
            if self.memory != 0 || self.worker || self.storage != 0 || self.gpu.is_some() {
                notify_release(&mut used);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_admission_loans_require_exact_pool_worker_kind_and_memory() {
        let pool = ResourcePool::new(1024, 2);
        let other = ResourcePool::new(1024, 2);
        let claim = pool.reserve(512).unwrap();
        claim.require_cpu_worker(&pool, 512, 2).unwrap();
        assert!(claim.require_cpu_worker(&other, 512, 2).is_err());
        assert!(claim.require_cpu_worker(&pool, 513, 2).is_err());
        drop(claim);
        let metadata = pool.reserve_memory(512).unwrap();
        assert!(metadata.require_cpu_worker(&pool, 512, 2).is_err());
    }
    #[test]
    fn split_storage_keeps_charge_independent_of_cpu_worker_retirement() {
        for storage_first in [false, true] {
            let pool = ResourcePool::new(1024, 1);
            let mut worker = pool
                .reserve(1024)
                .unwrap()
                .with_storage(4096, 32 * 1024 * 1024)
                .unwrap();
            let storage = worker.split_storage().unwrap();
            assert!(worker.split_storage().is_err());
            assert_eq!(pool.reserved_storage(), 4096);
            assert!(pool.reserve(1024).is_err());
            if storage_first {
                drop(storage);
                assert_eq!(pool.reserved_storage(), 0);
                assert!(pool.reserve(1024).is_err());
                drop(worker);
            } else {
                drop(worker);
                assert!(pool.reserve(1024).is_ok());
                assert_eq!(pool.reserved_storage(), 4096);
                drop(storage);
            }
            assert_eq!(pool.reserved_storage(), 0);
            assert!(pool.reserve(1024).is_ok());
        }
    }
    #[test]
    fn failed_storage_attachment_preserves_existing_custody() {
        let pool = ResourcePool::new(1024, 1);
        let mut reservation = pool.reserve(1024).unwrap();
        assert!(reservation.attach_storage(1024, 0).is_err());
        assert!(pool.reserve(1).is_err());
        assert_eq!(pool.reserved_storage(), 0);
        drop(reservation);
        assert!(pool.reserve(1024).is_ok());
    }
    #[test]
    fn retired_cpu_worker_keeps_storage_without_holding_compute_capacity() {
        let pool = ResourcePool::new(1024, 1);
        let mut capture = pool
            .reserve(1024)
            .unwrap()
            .with_storage(512, 16 * 1024 * 1024 + 1024)
            .unwrap();
        assert!(pool.reserve(1).is_err());
        capture.release_cpu_worker().unwrap();
        capture.release_cpu_worker().unwrap();
        let next = pool.reserve(1024).unwrap();
        assert_eq!(pool.reserved_storage(), 512);
        drop(next);
        assert_eq!(pool.reserved_storage(), 512);
        drop(capture);
        assert_eq!(pool.reserved_storage(), 0);
    }
    #[test]
    fn aggregate_limits_and_release() {
        let pool = ResourcePool::new(1024, 1);
        let first = pool.reserve(512).unwrap();
        assert!(pool.reserve(1).is_err());
        drop(first);
        assert!(pool.reserve(512).is_ok());
        assert!(pool.reserve(2048).is_err());
    }
    #[test]
    fn storage_reservations_are_aggregate_and_release() {
        let pool = ResourcePool::new(2048, 2);
        let margin = 16 * 1024 * 1024;
        let first = pool
            .reserve(1)
            .unwrap()
            .with_storage(1024, margin + 1536)
            .unwrap();
        assert_eq!(pool.reserved_storage(), 1024);
        assert!(pool
            .reserve(1)
            .unwrap()
            .with_storage(1024, margin + 1536)
            .is_err());
        drop(first);
        assert_eq!(pool.reserved_storage(), 0);
    }
    const UUID: &str = "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351";
    fn device() -> GpuDeviceBudget {
        GpuDeviceBudget {
            device_uuid: UUID.into(),
            device_ordinal: 0,
            maximum_reserved_bytes: 100,
            interactive_reserved_bytes: 40,
            control_reserve_bytes: 10,
        }
    }
    fn observation() -> GpuMemoryObservation {
        GpuMemoryObservation {
            device_uuid: UUID.into(),
            device_ordinal: 0,
            total_bytes: 1000,
            available_bytes: 1000,
            observed_at: Instant::now(),
        }
    }
    fn limits(bytes: u64) -> CudaResourceLimits {
        CudaResourceLimits {
            device_uuid: UUID.into(),
            device_ordinal: 0,
            address_space_bytes: 1000,
            device_reservation_bytes: bytes,
            arena_limit_bytes: 1,
        }
    }
    #[test]
    fn gpu_aggregate_preserves_interactive_capacity_and_releases_all_claims() {
        let pool = ResourcePool::new_with_gpu_devices(1000, 4, vec![device()]).unwrap();
        let offline = pool
            .reserve_cuda(1, &limits(60), &observation(), GpuPriority::Offline)
            .unwrap();
        assert!(pool
            .reserve_cuda(1, &limits(1), &observation(), GpuPriority::Offline)
            .is_err());
        let interactive = pool
            .reserve_cuda(1, &limits(40), &observation(), GpuPriority::Interactive)
            .unwrap();
        assert_eq!(
            pool.gpu_usage(UUID).unwrap(),
            GpuUsage {
                reserved_bytes: 100,
                offline_reserved_bytes: 60
            }
        );
        assert!(pool
            .reserve_cuda(1, &limits(1), &observation(), GpuPriority::Interactive)
            .is_err());
        drop(offline);
        drop(interactive);
        assert_eq!(pool.gpu_usage(UUID).unwrap(), GpuUsage::default());
        assert!(pool
            .reserve_cuda(1, &limits(60), &observation(), GpuPriority::Offline)
            .is_ok());
    }
    #[test]
    fn gpu_observation_identity_time_and_capacity_fail_without_claims() {
        let pool = ResourcePool::new_with_gpu_devices(1000, 4, vec![device()]).unwrap();
        let mut o = observation();
        o.observed_at -= Duration::from_secs(2);
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        o = observation();
        o.observed_at += Duration::from_secs(1);
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        o = observation();
        o.device_ordinal = 1;
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        o = observation();
        o.device_uuid = "GPU-00000000-0000-0000-0000-000000000000".into();
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        o = observation();
        o.available_bytes = 10;
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        o = observation();
        o.total_bytes = 100;
        o.available_bytes = 100;
        assert!(pool
            .reserve_cuda(1, &limits(1), &o, GpuPriority::Offline)
            .is_err());
        assert!(pool
            .reserve_cuda(1001, &limits(1), &observation(), GpuPriority::Offline)
            .is_err());
        assert_eq!(pool.gpu_usage(UUID).unwrap(), GpuUsage::default());
        assert!(ResourcePool::new(1000, 4)
            .reserve_cuda(1, &limits(1), &observation(), GpuPriority::Offline)
            .is_err());
    }
    #[test]
    fn gpu_policy_requires_unique_ids_and_ordinals_and_bounded_capacity() {
        assert!(ResourcePool::new_with_gpu_devices(1000, 4, vec![device(), device()]).is_err());
        let mut d = device();
        d.interactive_reserved_bytes = 101;
        assert!(ResourcePool::new_with_gpu_devices(1000, 4, vec![d]).is_err());
        d = device();
        d.control_reserve_bytes = 0;
        assert!(ResourcePool::new_with_gpu_devices(1000, 4, vec![d]).is_err());
    }
    #[test]
    fn storage_failure_and_double_attachment_release_ram_job_and_gpu() {
        let pool = ResourcePool::new_with_gpu_devices(1000, 1, vec![device()]).unwrap();
        let r = pool
            .reserve_cuda(1, &limits(60), &observation(), GpuPriority::Offline)
            .unwrap();
        assert!(r.with_storage(1000, 1000).is_err());
        assert_eq!(pool.gpu_usage(UUID).unwrap(), GpuUsage::default());
        let r = pool
            .reserve_cuda(1, &limits(60), &observation(), GpuPriority::Offline)
            .unwrap()
            .with_storage(10, 1 << 25)
            .unwrap();
        assert!(r.with_storage(10, 1 << 25).is_err());
        assert_eq!(pool.reserved_storage(), 0);
        assert_eq!(pool.gpu_usage(UUID).unwrap(), GpuUsage::default());
        assert!(pool.reserve(1000).is_ok());
    }
    #[test]
    fn concurrent_gpu_claims_are_atomic_and_release_on_thread_exit() {
        let mut d = device();
        d.interactive_reserved_bytes = 0;
        let pool = ResourcePool::new_with_gpu_devices(1000, 8, vec![d]).unwrap();
        let start = Arc::new(std::sync::Barrier::new(9));
        let hold = Arc::new(std::sync::Barrier::new(9));
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut threads = Vec::new();
        for _ in 0..8 {
            let p = pool.clone();
            let s = start.clone();
            let h = hold.clone();
            let tx = sender.clone();
            threads.push(std::thread::spawn(move || {
                s.wait();
                let r = p.reserve_cuda(1, &limits(50), &observation(), GpuPriority::Offline);
                tx.send(r.is_ok()).unwrap();
                h.wait();
                drop(r);
            }));
        }
        start.wait();
        let admitted = (0..8).filter(|_| receiver.recv().unwrap()).count();
        assert_eq!(admitted, 2);
        assert_eq!(pool.gpu_usage(UUID).unwrap().reserved_bytes, 100);
        hold.wait();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(pool.gpu_usage(UUID).unwrap(), GpuUsage::default());
    }
}

#[cfg(test)]
mod storage_rejoin_tests {
    use super::*;
    #[test]
    fn transfer_preserves_charge_and_refuses_foreign_or_nonstorage_claims() {
        let pool = ResourcePool::new(2048, 2);
        let mut queued = pool
            .reserve(1024)
            .unwrap()
            .with_storage(256, 32 << 20)
            .unwrap();
        let mut storage = queued.split_storage().unwrap();
        assert_eq!(pool.reserved_storage(), 256);
        drop(queued);
        let mut next = pool.reserve(1024).unwrap();
        let foreign = ResourcePool::new(1024, 1);
        let mut wrong = foreign
            .reserve(1)
            .unwrap()
            .with_storage(128, 32 << 20)
            .unwrap();
        assert!(next.absorb_storage(&mut wrong).is_err());
        assert_eq!(foreign.reserved_storage(), 128);
        assert_eq!(pool.reserved_storage(), 256);
        assert!(next.absorb_storage(&mut next_pool_claim(&pool)).is_err());
        next.absorb_storage(&mut storage).unwrap();
        assert_eq!(pool.reserved_storage(), 256);
        assert!(next.absorb_storage(&mut storage).is_err());
        drop(storage);
        assert_eq!(pool.reserved_storage(), 256);
        drop(next);
        assert_eq!(pool.reserved_storage(), 0);
    }
    fn next_pool_claim(pool: &ResourcePool) -> Reservation {
        pool.reserve(1).unwrap()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod release_watch_tests {
    use super::*;
    #[test]
    fn release_watch_is_bounded_coalesced_external_and_removed_on_drop() {
        let pool = ResourcePool::new(4096, 8);
        let watcher = pool.watch_releases().unwrap();
        drop(pool.reserve_memory(1).unwrap());
        assert!(matches!(
            watcher.wait(Duration::from_millis(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        let first = pool.reserve_memory(1).unwrap();
        let second = pool.reserve_memory(1).unwrap();
        std::thread::spawn(move || {
            drop(first);
            drop(second);
        })
        .join()
        .unwrap();
        watcher.wait(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            watcher.wait(Duration::from_millis(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(watcher);
        assert!(pool.inner.lock().unwrap().listeners.is_empty());
        let listeners: Vec<_> = (0..8).map(|_| pool.watch_releases().unwrap()).collect();
        assert!(pool.watch_releases().is_err());
        drop(listeners);
        assert!(pool.inner.lock().unwrap().listeners.is_empty());
    }
}

#[cfg(test)]
mod cpu_tests {
    use super::*;
    include!("resource_cpu_tests.rs");
}
