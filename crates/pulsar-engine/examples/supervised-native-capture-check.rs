//! Actual confined capture and retirement observation; no library loading.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use pulsar_engine::{artifacts, native_capture, resources::ResourcePool, EngineConfig};
    use pulsar_protocol::*;
    use std::{
        fs,
        io::Read,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(args.len()==5,"usage: supervised-native-capture-check WORKER SPEC NEW-ROOT TIMEOUT-MS CANCEL-AFTER-MS (-1 disables cancellation)");
    let worker = PathBuf::from(&args[0]);
    let spec_path = PathBuf::from(&args[1]);
    let root = PathBuf::from(&args[2]);
    ensure!(!root.exists(), "refusing to reuse capture inspection root");
    let mut bytes = Vec::new();
    artifacts::open_regular(&spec_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "capture inspection spec exceeds bound"
    );
    let spec: NativeDependencySpec = serde_json::from_slice(&bytes)?;
    let total = spec.validate()?;
    artifacts::private_directory(&root)?;
    let parent = root.join("captures");
    artifacts::private_directory(&parent)?;
    let budget = ResourceBudget {
        memory_bytes: 512 * 1024 * 1024,
        output_bytes: total + 16 * 1024 * 1024,
        wall_time_ms: args[3].parse()?,
        cpu_threads: 1,
    };
    let cancel_ms: i64 = args[4].parse()?;
    ensure!(cancel_ms >= -1, "invalid cancellation delay");
    let cancel = Arc::new(AtomicBool::new(cancel_ms == 0));
    let finished = Arc::new(AtomicBool::new(false));
    let mut cancellation = None;
    if cancel_ms > 0 {
        let cancel = cancel.clone();
        let finished = finished.clone();
        cancellation = Some(std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_millis(cancel_ms as u64);
            while Instant::now() < end && !finished.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if !finished.load(Ordering::Acquire) {
                cancel.store(true, Ordering::Release);
            }
        }));
    }
    let pool = ResourcePool::new(1024 * 1024 * 1024, 1);
    let config = EngineConfig::new(root.join("unused-state"), worker);
    let start = Instant::now();
    let outcome = native_capture::capture(
        &config,
        &pool,
        &spec,
        &parent,
        budget,
        1536 * 1024 * 1024,
        &cancel,
    );
    let elapsed = start.elapsed();
    finished.store(true, Ordering::Release);
    if let Some(thread) = cancellation {
        thread
            .join()
            .map_err(|_| anyhow::anyhow!("capture cancellation thread failed"))?;
    }
    let result = match outcome {
        Ok(captured) => {
            captured
                .captured()
                .verify()
                .context("independent post-capture byte readback")?;
            let completion = serde_json::to_value(captured.completion())?;
            let held = pool.reserved_storage();
            ensure!(held > 0, "capture storage must remain charged");
            let next = pool.reserve(512 * 1024 * 1024)?;
            drop(next);
            captured.close()?;
            serde_json::json!({"state":"captured","completion":completion,"independent_byte_readback":true,
                "cpu_worker_slot_released":true,"storage_held_before_close":held})
        }
        Err(error) => serde_json::json!({"state":"refused","error":error}),
    };
    let cleanup_end = Instant::now() + Duration::from_secs(5);
    loop {
        // Storage deletion and worker retirement are independent observations.
        // A failed capture may finish deleting bytes before its group clears.
        let empty = fs::read_dir(&parent)?.next().is_none();
        match pool.reserve(512 * 1024 * 1024) {
            Ok(next) => {
                drop(next);
                if pool.reserved_storage() == 0 && empty {
                    break;
                }
            }
            Err(error) if error.code == ErrorCode::ResourceExhausted => (),
            Err(error) => return Err(error.into()),
        }
        ensure!(
            Instant::now() < cleanup_end,
            "capture retirement/storage cleanup remains unconfirmed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let record = serde_json::json!({"result":result,"capture_elapsed_ms":elapsed.as_secs_f64()*1000.0,
        "cancel_requested":cancel.load(Ordering::Acquire),"cleanup_confirmed":true,"storage_after_cleanup":pool.reserved_storage(),
        "gpu_execution":false,"captured_library_loading":false,"durable_jobs":false});
    fs::write(
        root.join("checked.json"),
        serde_json::to_vec_pretty(&record)?,
    )?;
    println!("{}", serde_json::to_string(&record)?);
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("supervised capture inspection requires Linux")
}
