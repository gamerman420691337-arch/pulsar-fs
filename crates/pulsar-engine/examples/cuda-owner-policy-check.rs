//! Opens an isolated real engine with owner policy; never generates GPU work.
#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use pulsar_engine::{Engine, EngineConfig};
    use std::path::PathBuf;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: cuda-owner-policy-check PRIVATE-POLICY NEW-STATE WORKER"
    );
    let state = PathBuf::from(&args[1]);
    ensure!(!state.exists(), "use a new isolated engine state");
    let mut config = EngineConfig::new(state, PathBuf::from(&args[2]));
    config.cuda_owner_policy = Some(PathBuf::from(&args[0]));
    let engine = Engine::open(config)?;
    let status = engine
        .cuda_owner_policy_status()?
        .ok_or_else(|| anyhow::anyhow!("owner policy missing"))?;
    ensure!(
        !status.execution_enabled,
        "owner configuration unexpectedly enabled GPU jobs"
    );
    println!("{}", serde_json::to_string(&status)?);
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("CUDA owner policy requires Linux")
}
