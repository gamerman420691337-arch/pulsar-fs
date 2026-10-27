//! Bounded read-only trace consistency adapter; no runtime loading or execution authority.
use anyhow::{ensure, Result};
use pulsar_engine::inference_trace::{validate_node_trace, MAX_TRACE_BYTES};
use pulsar_protocol::InferenceProvider;
use std::io::Read;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: inference-trace-check cpu|cuda EXPECTED-RUNS TRACE.json"
    );
    let provider = match args[0].to_str() {
        Some("cpu") => InferenceProvider::Cpu,
        Some("cuda") => InferenceProvider::Cuda,
        _ => anyhow::bail!("unsupported strict provider"),
    };
    let runs = args[1]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid run count"))?
        .parse()?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(&args[2])?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && before.len() <= MAX_TRACE_BYTES as u64,
        "trace file admission"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_TRACE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        bytes.len() as u64 == before.len()
            && before.len() == after.len()
            && before.modified()? == after.modified()?,
        "trace changed during reading"
    );
    println!(
        "{}",
        serde_json::to_string(&validate_node_trace(&bytes, provider, runs)?)?
    );
    Ok(())
}
