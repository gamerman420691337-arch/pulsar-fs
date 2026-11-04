//! Independent native repair/history oracle; authored data, no hardware.
use anyhow::{ensure, Result};
use pulsar_clients::{get_snapshot, EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::*;
use std::path::PathBuf;
fn check(program: &MotionProgram, points: &[(i64, f64)]) -> Result<()> {
    let track = program
        .track(Axis::Stroke)
        .ok_or_else(|| anyhow::anyhow!("missing stroke"))?;
    ensure!(
        program.tracks().len() == 1
            && track.gaps().is_empty()
            && track.actions().len() == points.len(),
        "wrong repair support"
    );
    for (action, (ms, pct)) in track.actions().iter().zip(points) {
        ensure!(
            action.time().as_nanos() == ms * 1_000_000
                && (action.position().value() - pct / 100.0).abs() < 1e-12,
            "wrong repaired action"
        );
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: script-repair-fixture STATE PROJECT"
    );
    let state = PathBuf::from(&args[0]);
    ensure!(state.is_absolute(), "absolute state required");
    let project = ProjectId::new(
        args[1]
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("UTF-8 project required"))?,
    )?;
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let fixed = get_snapshot(&mut client, &project)?;
    ensure!(fixed.revision.value() == 2, "two explicit commits required");
    check(
        &fixed.program,
        &[(0, 0.0), (20, 9.0), (40, 18.0), (100, 45.0)],
    )?;
    let ResponseBody::Diagnostics(diagnostics) = client.execute(
        Command::Diagnostics {
            candidate_id: None,
            maximum_speed_percent_per_second: Some(450.0),
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("diagnostics required")
    };
    ensure!(
        diagnostics
            .segment_speed
            .as_ref()
            .is_some_and(|s| s.violations.is_empty() && !s.truncated),
        "slope diagnostic failed"
    );
    let ResponseBody::Project(original) =
        client.execute(Command::Undo, Some(project.clone()), Some(fixed.revision))?
    else {
        anyhow::bail!("Undo required")
    };
    check(
        &original.program,
        &[(0, 0.0), (10, 1.0), (20, 100.0), (40, 100.0), (100, 50.0)],
    )?;
    let ResponseBody::Project(restored) = client.execute(
        Command::Redo,
        Some(project.clone()),
        Some(original.revision),
    )?
    else {
        anyhow::bail!("Redo required")
    };
    ensure!(
        restored.program == fixed.program,
        "Redo changed repaired values/evidence"
    );
    println!(
        "{}",
        serde_json::json!({"qualified":false,"repair":"PASS","undo_redo":"PASS","diagnostics":"PASS","restored_revision":restored.revision,"source_count":restored.sources.len()})
    );
    Ok(())
}
