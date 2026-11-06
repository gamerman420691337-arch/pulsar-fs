#[test]
fn cpu_ceiling_is_independent_of_memory_and_worker_slots() {
    let pool = ResourcePool::new(4096, 8).with_cpu_limit(4).unwrap();
    let first = pool.reserve_worker(1, 3).unwrap();
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 3);
    assert_eq!(pool.reserve_worker(1, 2).err().unwrap().code, ErrorCode::ResourceExhausted);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 3);
    let second = pool.reserve_worker(1, 1).unwrap();
    let metadata = pool.reserve_memory(1).unwrap();
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 4);
    assert!(first.admits_worker_budget(1, 3));
    assert!(!first.admits_worker_budget(1, 4));
    assert!(!first.admits_worker_budget(1, 0));
    assert!(first.require_cpu_worker(&pool, 1, 2).is_err());
    first.require_cpu_worker(&pool, 1, 3).unwrap();
    drop(first);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 1);
    drop((second, metadata));
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
}
#[test]
fn cpu_and_ram_retire_once_while_storage_remains_charged() {
    let pool = ResourcePool::new(4096, 8).with_cpu_limit(4).unwrap();
    let mut worker = pool.reserve_worker(1, 4).unwrap().with_storage(32, 32 << 20).unwrap();
    let storage = worker.split_storage().unwrap();
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 4);
    worker.release_cpu_worker().unwrap();
    worker.release_cpu_worker().unwrap();
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
    assert_eq!(pool.reserved_storage(), 32);
    let new_worker = pool.reserve_worker(1, 4).unwrap();
    drop(worker);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 4);
    drop(storage);
    assert_eq!(pool.reserved_storage(), 0);
    drop(new_worker);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
}
#[test]
fn cpu_policy_cannot_change_after_sharing_or_admission() {
    assert!(ResourcePool::new(4096, 8).with_cpu_limit(0).is_err());
    assert!(ResourcePool::new(4096, 8).with_cpu_limit(1025).is_err());
    let pool = ResourcePool::new(4096, 8);
    let shared = pool.clone();
    assert!(shared.with_cpu_limit(4).is_err());
    let claim = pool.reserve_worker(1, 1).unwrap();
    assert!(pool.with_cpu_limit(4).is_err());
    drop(claim);
}
#[test]
fn cpu_admission_is_atomic_under_contention() {
    let pool = ResourcePool::new(4096, 8).with_cpu_limit(4).unwrap();
    let start = Arc::new(std::sync::Barrier::new(9));
    let held = Arc::new(std::sync::Barrier::new(9));
    let release = Arc::new(std::sync::Barrier::new(9));
    let successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handles: Vec<_> = (0..8).map(|_| {
        let (pool, start, held, release, successes) = (pool.clone(), start.clone(), held.clone(), release.clone(), successes.clone());
        std::thread::spawn(move || {
            start.wait();
            let claim = pool.reserve_worker(1, 2);
            if claim.is_ok() { successes.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
            else { assert_eq!(claim.as_ref().err().unwrap().code, ErrorCode::ResourceExhausted); }
            held.wait();
            release.wait();
            drop(claim);
        })
    }).collect();
    start.wait(); held.wait();
    assert_eq!(successes.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 4);
    release.wait();
    for handle in handles { handle.join().unwrap(); }
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
}
#[test]
fn gpu_and_cpu_claims_commit_atomically() {
    let device = GpuDeviceBudget { device_uuid: "GPU-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(), device_ordinal: 0, maximum_reserved_bytes: 100, interactive_reserved_bytes: 20, control_reserve_bytes: 10 };
    let pool = ResourcePool::new_with_gpu_devices(4096, 8, vec![device.clone()]).unwrap().with_cpu_limit(4).unwrap();
    let limits = CudaResourceLimits { device_ordinal: 0, device_uuid: device.device_uuid.clone(), device_reservation_bytes: 60, arena_limit_bytes: 50, address_space_bytes: 4096 };
    let observation = GpuMemoryObservation { device_uuid: device.device_uuid.clone(), device_ordinal: 0, total_bytes: 200, available_bytes: 200, observed_at: Instant::now() };
    let cpu = pool.reserve_worker(1, 4).unwrap();
    assert!(pool.reserve_cuda_worker(1, 1, &limits, &observation, GpuPriority::Offline).is_err());
    assert_eq!(pool.gpu_usage(&device.device_uuid).unwrap().reserved_bytes, 0);
    drop(cpu);
    let gpu = pool.reserve_cuda_worker(1, 3, &limits, &observation, GpuPriority::Offline).unwrap();
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 3);
    assert!(pool.reserve_cuda_worker(1, 1, &limits, &observation, GpuPriority::Offline).is_err());
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 3);
    assert_eq!(pool.gpu_usage(&device.device_uuid).unwrap().reserved_bytes, 60);
    drop(gpu);
    assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
    assert_eq!(pool.gpu_usage(&device.device_uuid).unwrap().reserved_bytes, 0);
}
