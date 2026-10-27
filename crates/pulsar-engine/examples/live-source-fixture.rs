//! Authored confined native source acceptance, without capture/device authority.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Result};
    use pulsar_engine::{live_source::LiveSourceWorker, resources::ResourcePool, EngineConfig};
    use pulsar_protocol::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
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
        source_path: PathBuf,
        manifest: LiveDecodeManifest,
        #[serde(default)]
        cancel_after_first: bool,
        #[serde(default)]
        idle_ms: u64,
        #[serde(default)]
        drop_input: bool,
        #[serde(default)]
        wait_for_first_frame: bool,
        #[serde(default)]
        producer_hold_ms: u64,
        #[serde(default)]
        lease_elapsed_ms: u64,
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 1, "usage: live-source-fixture INPUT.json");
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(65537)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "bounded fixture manifest required");
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(input.idle_ms <= 5000, "bounded fixture idle required");
    ensure!(
        input.producer_hold_ms <= 5000,
        "bounded producer hold required"
    );
    ensure!(input.lease_elapsed_ms <= 5000, "bounded lease age required");
    let config = EngineConfig::new("/unused-source-fixture-state".into(), input.worker_path);
    let pool = ResourcePool::new(config.max_ram_bytes, config.max_jobs);
    let cancel = Arc::new(AtomicBool::new(false));
    let origin = Instant::now() - Duration::from_millis(input.lease_elapsed_ms);
    let end = origin + Duration::from_millis(input.manifest.budget.wall_time_ms);
    let mut worker = LiveSourceWorker::start_in_lease(
        &config,
        &pool,
        input.manifest,
        cancel.clone(),
        origin,
        end,
    )?;
    let clock = worker.clock_probe().clone();
    let pid = worker.process_id()?;
    let mut producer = worker.take_input()?;
    ensure!(worker.take_input().is_err(), "input handle must be unique");
    let mut local_bounds_refused = false;
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let thread = if input.drop_input {
        drop(producer);
        None
    } else {
        // Local admission errors must not poison otherwise valid input.
        ensure!(producer.push(&[]).is_err(), "empty chunk must fail");
        ensure!(
            producer.push(&vec![0; 65537]).is_err(),
            "large chunk must fail"
        );
        local_bounds_refused = true;
        Some(std::thread::spawn(move || -> Result<()> {
            let mut source = std::fs::File::open(input.source_path)?;
            let mut chunk = [0; 65536];
            loop {
                let size = source.read(&mut chunk)?;
                if size == 0 {
                    break;
                }
                producer.push(&chunk[..size])?;
            }
            if input.wait_for_first_frame {
                wait.recv_timeout(Duration::from_secs(5))?;
            }
            std::thread::sleep(Duration::from_millis(input.producer_hold_ms));
            producer.finish()?;
            Ok(())
        }))
    };
    let mut frames = Vec::new();
    let mut refused = None;
    let mut completed = false;
    let mut idle_process_reaped = None;
    loop {
        if frames.len() == 1 {
            if input.cancel_after_first {
                cancel.store(true, Ordering::Release);
            }
            if input.idle_ms > 0 {
                std::thread::sleep(Duration::from_millis(input.idle_ms));
                idle_process_reaped = Some(unsafe { libc::kill(pid as i32, 0) } != 0);
            }
        }
        match worker.next_frame() {
            Ok(Some(frame)) => {
                if frames.is_empty() {
                    let _ = release.try_send(());
                }
                frames.push(serde_json::json!({"frame":frame.facts().frame,"source_time":frame.facts().source_time,"delivered_elapsed_ns":frame.facts().worker_delivered_elapsed_ns,"delivery":frame.facts(),"conservative_delivery_age_ns":frame.delivery_age_at(Instant::now())?,"pixel_sha256":format!("{:x}",Sha256::digest(frame.pixels())),"pixel_bytes":frame.pixels().len()}));
            }
            Ok(None) => {
                completed = true;
                break;
            }
            Err(error) => {
                refused = Some(error);
                break;
            }
        }
    }
    let peak = worker.peak_memory_bytes()?;
    drop(worker);
    let producer_result = thread.map(|t| {
        t.join()
            .expect("source producer panic")
            .map_err(|e| e.to_string())
    });
    ensure!(
        pool.reserved_storage() == 0,
        "source snapshots still reserved after drop"
    );
    println!(
        "{}",
        serde_json::json!({"frames":frames,"clock_probe":clock,"lease_elapsed_ms":input.lease_elapsed_ms,"completed":completed,"refused":refused,"producer_result":producer_result,"local_bounds_refused":local_bounds_refused,"idle_process_reaped":idle_process_reaped,"process_reaped":unsafe{libc::kill(pid as i32,0)}!=0,"worker_peak_memory_bytes":peak,"confined":true,"lease_integration":false,"physical_dispatch":false})
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("native source confinement unsupported on this platform")
}
