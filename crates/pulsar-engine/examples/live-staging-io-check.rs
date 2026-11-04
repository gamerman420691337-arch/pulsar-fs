//! Controlled live-staging ownership probe. No project or physical effects.
use anyhow::{ensure, Result};
use pulsar_engine::{live_inference::LiveInferenceWorker, resources::ResourcePool, EngineConfig};
use pulsar_protocol::LiveInferenceManifest;
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
struct Input {
    worker_path: PathBuf,
    manifest: LiveInferenceManifest,
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: live-staging-io-check INPUT RETURN-REPORT RELEASE CANCEL-AFTER-MS"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(65537)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "bounded authored input required");
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(
        input.manifest.identity.source.as_str() == "authored-live-source",
        "authored source required"
    );
    let cancel_ms: i64 = args[3]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid cancel timer"))?
        .parse()?;
    ensure!(
        (-1..=5000).contains(&cancel_ms),
        "bounded cancellation timer required"
    );
    let config = EngineConfig::new("/unused-live-staging-state".into(), input.worker_path);
    let pool = ResourcePool::new(input.manifest.budget.memory_bytes, 1);
    let cancel = Arc::new(AtomicBool::new(false));
    let timer = if cancel_ms >= 0 {
        let cancel = cancel.clone();
        Some(std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(cancel_ms as u64));
            cancel.store(true, Ordering::Release);
        }))
    } else {
        None
    };
    let memory = input.manifest.budget.memory_bytes;
    let start = Instant::now();
    let end = start + Duration::from_millis(input.manifest.budget.wall_time_ms);
    let result =
        LiveInferenceWorker::start_in_lease(&config, &pool, input.manifest, cancel, start, end);
    let returned_ms = start.elapsed().as_millis();
    let worker_started = result.is_ok();
    let error = result.as_ref().err().map(|e| e.message.clone());
    drop(result);
    let retained = pool.reserve(memory).is_err();
    let report = serde_json::json!({"returned_ms":returned_ms,"worker_started":worker_started,
        "error":error,"admission_retained":retained,"reserved_storage":pool.reserved_storage(),
        "physical_dispatch":false});
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    use std::io::Write;
    options
        .open(&args[1])?
        .write_all(serde_json::to_vec(&report)?.as_slice())?;
    let release_deadline = Instant::now() + Duration::from_secs(15);
    while !std::path::Path::new(&args[2]).exists() {
        ensure!(
            Instant::now() < release_deadline,
            "owned producer release deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let cleanup_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(reservation) = pool.reserve(memory) {
            drop(reservation);
            ensure!(
                pool.reserved_storage() == 0,
                "storage claim survived confirmed cleanup"
            );
            break;
        }
        ensure!(
            Instant::now() < cleanup_deadline,
            "owned staging cleanup deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(timer) = timer {
        timer
            .join()
            .map_err(|_| anyhow::anyhow!("cancel timer failed"))?;
    }
    println!(
        "{}",
        serde_json::json!({"return":report,"capacity_released_after_producer_release":true,
        "storage_released":true,"physical_dispatch":false})
    );
    Ok(())
}
