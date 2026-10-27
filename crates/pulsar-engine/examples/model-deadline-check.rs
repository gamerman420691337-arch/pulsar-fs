//! Actual CPU session deadline checks; no media or GPU qualification.
use anyhow::{ensure, Context, Result};
use pulsar_engine::worker::{pin_artifact, temporal_position::TemporalPosition};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 3 && (args[2] == "fresh" || args[2] == "expired"),
        "usage: model-deadline-check AUTHORED-MODEL CPU-RUNTIME fresh|expired"
    );
    let model = pin_artifact(&PathBuf::from(&args[0]))?;
    let runtime = pin_artifact(&PathBuf::from(&args[1]))?;
    let mut request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))?;
    request.cuda_execution = None;
    request.tools.onnx_runtime = Some(runtime.clone());
    if let WorkerOperation::Generate {
        model: source,
        settings,
        ..
    } = &mut request.operation
    {
        let profile = settings
            .inference_profile
            .as_mut()
            .context("profile missing")?;
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        profile.model = model.identity.clone();
        profile.runtime = runtime.identity.clone();
        *source = Some(SourceArtifact {
            source_version: SourceVersionId::new("authored-model")?,
            path: model.path,
            identity: model.identity,
        });
    }
    request.validate()?;
    let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let error = TemporalPosition::from_request_at(&request, expired)
        .err()
        .context("expired initialization accepted")?;
    ensure!(
        error.to_string() == "model adapter attempt deadline expired",
        "wrong initialization refusal: {error}"
    );
    let deadline = Instant::now() + Duration::from_secs(if args[2] == "fresh" { 5 } else { 1 });
    let mut session = TemporalPosition::from_request_at(&request, deadline)?;
    let previous = vec![0; 64 * 64 * 3];
    let current = vec![128; 64 * 64 * 3];
    if args[2] == "expired" {
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
        );
        let error = session
            .estimate(&previous, &current)
            .err()
            .context("expired run accepted")?;
        ensure!(
            error.to_string() == "model adapter attempt deadline expired",
            "wrong run refusal: {error}"
        );
        ensure!(
            session.finish().is_err(),
            "expired session finalized evidence"
        );
        println!(
            "{}",
            serde_json::json!({"state":"passed","case":"expired",
            "expired_initialization_refused":true,"expired_run_refused":true,"expired_finalization_refused":true,
            "native_cpu_session_initialized":true,"gpu_execution":false})
        );
        return Ok(());
    }
    let position = session.estimate(&previous, &current)?.value();
    ensure!(
        (position - (0.25 + 0.25 * 128.0 / 255.0)).abs() < 1e-5,
        "authored CPU position differs"
    );
    session.finish()?;
    println!(
        "{}",
        serde_json::json!({"state":"passed","case":"fresh","expired_initialization_refused":true,
        "fresh_cpu_position":position,
        "native_cpu_execution":true,"gpu_execution":false})
    );
    Ok(())
}
