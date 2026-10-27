//! Owned diagnostic consumer of the production snapshot helper.
use anyhow::{ensure, Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 2,
        "expected authored source and existing snapshot directory"
    );
    let source = PathBuf::from(&args[0]);
    let directory = PathBuf::from(&args[1]);
    let (path, sha256, byte_len) = pulsar_engine::artifacts::snapshot(&source, &directory, 1024)
        .context("production snapshot refused")?;
    println!(
        "{}",
        serde_json::json!({"path": path, "sha256": sha256, "byte_len": byte_len})
    );
    Ok(())
}
