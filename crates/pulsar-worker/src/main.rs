//! Dedicated confined worker composition. No client or engine command parsing.
use anyhow::{bail, Result};
fn main() {
    if let Err(error) = run() {
        eprintln!("pulsar-worker: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("pulsar-worker {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--help" {
        println!("pulsar-worker: engine-launched data-only worker; no client or engine commands");
        return Ok(());
    }
    if !pulsar_engine::worker_entry::run_worker_role(&args)? {
        bail!("expected an engine-launched worker role");
    }
    Ok(())
}
