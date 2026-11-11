//! Standalone entry point for the existing Pulsar MCP stdio command.

use anyhow::{Context, Result};
use std::process::Command;

fn run() -> Result<()> {
    let executable = std::env::current_exe().context("locate pulsar-mcp")?;
    let pulsar = executable.with_file_name(format!("pulsar{}", std::env::consts::EXE_SUFFIX));
    let mut command = Command::new(pulsar);
    command.arg("mcp").args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).context("launch the adjacent Pulsar executable")
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .context("launch the adjacent Pulsar executable")?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pulsar-mcp: {error:#}");
        std::process::exit(1);
    }
}
