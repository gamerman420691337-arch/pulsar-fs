//! Read-only host observation and local accounting scenario. No GPU execution or engine authorization.
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::resources::{
        GpuDeviceBudget, GpuMemoryObservation, GpuPriority, GpuUsage, ResourcePool,
    };
    use pulsar_protocol::CudaResourceLimits;
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: gpu-reservation-check EXPECTED-UUID ORDINAL (caller must impose process deadline)"
    );
    let ordinal: u32 = args[1].parse()?;
    pulsar_protocol::validate_cuda_device_identity(ordinal, &args[0])?;
    let output = Command::new("/usr/bin/nvidia-smi")
        .env_clear()
        .args([
            "--id",
            &args[1],
            "--query-gpu=uuid,index,memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()?;
    ensure!(
        output.status.success() && output.stdout.len() <= 65536 && output.stderr.len() <= 65536,
        "GPU observation command refused"
    );
    let observed_at = Instant::now();
    let text = std::str::from_utf8(&output.stdout)?;
    ensure!(
        text.lines().count() == 1,
        "GPU observation must identify one device"
    );
    let fields: Vec<_> = text.trim().split(',').map(str::trim).collect();
    ensure!(fields.len() == 4, "GPU observation columns");
    ensure!(
        fields[0] == args[0] && fields[1].parse::<u32>()? == ordinal,
        "GPU observation differs from explicit expectation"
    );
    let mib = 1024u64 * 1024;
    let total = fields[2]
        .parse::<u64>()?
        .checked_mul(mib)
        .context("GPU total overflow")?;
    let free = fields[3]
        .parse::<u64>()?
        .checked_mul(mib)
        .context("GPU free overflow")?;
    let observation = GpuMemoryObservation {
        device_uuid: args[0].clone(),
        device_ordinal: ordinal,
        total_bytes: total,
        available_bytes: free,
        observed_at,
    };
    let pool = ResourcePool::new_with_gpu_devices(
        4 << 30,
        2,
        vec![GpuDeviceBudget {
            device_uuid: args[0].clone(),
            device_ordinal: ordinal,
            maximum_reserved_bytes: 2 << 30,
            interactive_reserved_bytes: 1 << 30,
            control_reserve_bytes: 256 << 20,
        }],
    )?;
    let limits = CudaResourceLimits {
        device_uuid: args[0].clone(),
        device_ordinal: ordinal,
        address_space_bytes: 16 << 30,
        device_reservation_bytes: 1 << 30,
        arena_limit_bytes: 64 << 20,
    };
    let offline = pool.reserve_cuda(1536 << 20, &limits, &observation, GpuPriority::Offline)?;
    ensure!(
        pool.reserve_cuda(1536 << 20, &limits, &observation, GpuPriority::Offline)
            .is_err(),
        "offline claim consumed interactive reserve"
    );
    let interactive =
        pool.reserve_cuda(1536 << 20, &limits, &observation, GpuPriority::Interactive)?;
    ensure!(
        pool.gpu_usage(&args[0])?
            == GpuUsage {
                reserved_bytes: 2 << 30,
                offline_reserved_bytes: 1 << 30
            },
        "aggregate accounting mismatch"
    );
    drop(offline);
    drop(interactive);
    ensure!(
        pool.gpu_usage(&args[0])? == GpuUsage::default(),
        "GPU claims not released"
    );
    let stale = GpuMemoryObservation {
        observed_at: Instant::now() - Duration::from_secs(2),
        ..observation
    };
    ensure!(
        pool.reserve_cuda(1536 << 20, &limits, &stale, GpuPriority::Offline)
            .is_err(),
        "stale observation admitted"
    );
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"scope":"read-only host memory observation and process-local cooperative GPU accounting",
        "device_uuid":args[0],"device_ordinal":ordinal,"observed_total_bytes":total,"observed_available_bytes":free,
        "offline_claim_bytes":1u64<<30,"interactive_claim_bytes":1u64<<30,"control_reserve_bytes":256u64<<20,
        "second_offline_refused":true,"interactive_capacity_preserved":true,"release_checked":true,"stale_observation_refused":true,
        "gpu_execution":false,"production_cuda_admitted":false,"hardware_vram_quota":false,"device_access_authorized":false})
    );
    Ok(())
}
