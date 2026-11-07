//! Authored native acceptance runner. No authenticated project or physical effects.
use anyhow::{ensure, Result};
use pulsar_engine::{
    live_frames::LiveRgbRuntime, live_inference::LiveInferenceWorker, resources::ResourcePool,
    EngineConfig,
};
use pulsar_protocol::*;
use serde::Deserialize;
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    worker_path: PathBuf,
    manifest: LiveInferenceManifest,
    #[serde(default)]
    cancel_after_first: bool,
    #[serde(default)]
    idle_ms: u64,
    #[serde(default)]
    lease_elapsed_ms: u64,
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 1, "usage: live-inference-fixture INPUT.json");
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(65537)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "bounded input required");
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(input.idle_ms <= 5000, "bounded fixture idle required");
    ensure!(
        input.lease_elapsed_ms <= 5000,
        "bounded existing lease age required"
    );
    let config = EngineConfig::new(
        PathBuf::from("/unused-live-fixture-state"),
        input.worker_path,
    );
    let pool = ResourcePool::new(config.max_ram_bytes, config.max_jobs);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut runtime = LiveRgbRuntime::new(
        &pool,
        input.manifest.identity.clone(),
        LivePolicy {
            maximum_analysis_age_ns: 2_000_000_000,
            added_buffer_ns: input.manifest.clock.added_buffer_ns,
        },
        Some(input.manifest.clock.clone()),
        LiveClockTick(0),
    )?;
    let origin = Instant::now() - Duration::from_millis(input.lease_elapsed_ms);
    let end = origin + Duration::from_millis(input.manifest.budget.wall_time_ms);
    let mut worker = LiveInferenceWorker::start_in_lease(
        &config,
        &pool,
        input.manifest.clone(),
        cancel.clone(),
        origin,
        end,
    )?;
    let pid = worker.process_id()?;
    let mut results = Vec::new();
    let mut refused = None;
    let mut idle_process_reaped: Option<bool> = None;
    for frame in 0..4 {
        let time = SourceTimestamp::new(frame, 10)?;
        let tick = LiveClockTick(frame as u64 * 100_000_000);
        let project = input.manifest.clock.map(time)?.project.time;
        runtime.offer(
            FrameId::new(frame as u64),
            time,
            tick,
            project,
            &vec![frame as u8 * 64; LIVE_FRAME_BYTES],
            tick,
        )?;
        if let Some(work) = runtime.take(tick)? {
            if results.len() == 1 {
                if input.cancel_after_first {
                    cancel.store(true, Ordering::Release);
                }
                if input.idle_ms > 0 {
                    std::thread::sleep(Duration::from_millis(input.idle_ms));
                    #[cfg(target_os = "linux")]
                    {
                        idle_process_reaped = Some(unsafe { libc::kill(pid as i32, 0) } != 0);
                    }
                }
            }
            match worker.estimate(&work) {
                Ok(result) => {
                    ensure!(worker.process_id()? == pid, "model process changed");
                    results.push(result);
                    runtime.complete(work, tick).map_err(|error| error.error)?;
                }
                Err(error) => {
                    refused = Some(error);
                    break;
                }
            }
        }
    }
    let peak = worker.peak_memory_bytes().ok();
    drop(worker);
    #[cfg(target_os = "linux")]
    let process_reaped = unsafe { libc::kill(pid as i32, 0) } != 0;
    #[cfg(not(target_os = "linux"))]
    let process_reaped = false;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"worker_pid":pid,"lease_elapsed_ms":input.lease_elapsed_ms,"one_process_reused":true,"results":results,"refused":refused,"worker_peak_memory_bytes":peak,"process_reaped":process_reaped,"idle_process_reaped":idle_process_reaped,"qualification":"authored-software-fixture","authenticated_capture_integration":false,"physical_dispatch":false})
        )?
    );
    Ok(())
}
