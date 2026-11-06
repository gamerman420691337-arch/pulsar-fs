//! Research driver identity check. Does not authorize jobs or execute inference.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use pulsar_engine::{
        artifacts,
        worker::cuda_identity::{CudaSdkDevice, CudaSdkExpectation},
    };
    use std::{io::Read, path::PathBuf};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: cuda-sdk-check CAPTURED-DRIVER EXPECTATION-JSON"
    );
    let mut file = artifacts::open_regular(&PathBuf::from(&args[1]))?;
    let mut bytes = Vec::new();
    file.by_ref().take(8193).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8192, "CUDA SDK expectation exceeds limit");
    let expected: CudaSdkExpectation = serde_json::from_slice(&bytes)?;
    let device = CudaSdkDevice::open(&PathBuf::from(&args[0]), expected)?;
    let rechecked = device.recheck()?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"initial": device.initial(), "rechecked": rechecked})
        )?
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("CUDA SDK identity probe requires Linux")
}
