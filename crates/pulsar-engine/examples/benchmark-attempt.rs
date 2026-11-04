//! Explicit offline harness. No project commits or physical device operations.
use anyhow::{ensure, Result};
use pulsar_engine::{benchmark, EngineConfig};
use pulsar_protocol::{ArtifactIdentity, WorkerRequest};
use serde::Deserialize;
use std::{io::Read, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    worker_path: PathBuf,
    worker_identity: ArtifactIdentity,
    manifest: WorkerRequest,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: benchmark-attempt INPUT.json NEW-RECEIPT.json"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "campaign input exceeds 1 MiB");
    let input: Input = serde_json::from_slice(&bytes)?;
    let state = tempfile::tempdir()?;
    let config = EngineConfig::new(state.path().to_owned(), input.worker_path);
    let receipt = benchmark::execute_campaign_attempt(
        &config,
        &input.manifest,
        &input.worker_identity,
        &AtomicBool::new(false),
    )?;
    benchmark::write_execution_receipt(std::path::Path::new(&args[1]), &receipt)?;
    ensure!(
        receipt.result.is_ok(),
        "worker failed; retained exact error in receipt"
    );
    Ok(())
}
