//! Owned actual learned-candidate controller; authored numeric oracle, no device.
use anyhow::{ensure, Result};
use pulsar_clients::{get_snapshot, EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3 || args.len() == 4,
        "usage: creative-candidate-fixture STATE PROJECT JOB [triangle|pulse25|pulse75]"
    );
    let state = PathBuf::from(&args[0]);
    ensure!(state.is_absolute(), "absolute state path required");
    let project = ProjectId::new(
        args[1]
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("UTF-8 project required"))?,
    )?;
    let job_id = JobId::new(
        args[2]
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("UTF-8 job required"))?,
    )?;
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let before = get_snapshot(&mut client, &project)?;
    ensure!(
        before.revision == RevisionId::new(0) && before.program.tracks().is_empty(),
        "untouched authored project required"
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let candidate_id = loop {
        let ResponseBody::Job(job) = client.execute(
            Command::JobStatus {
                job_id: job_id.clone(),
            },
            Some(project.clone()),
            None,
        )?
        else {
            anyhow::bail!("job expected");
        };
        ensure!(
            job.project_id == project && job.base_revision == before.revision,
            "job binding differs"
        );
        if let Some(id) = job.candidate_id {
            break id;
        }
        ensure!(
            job.error.is_none() && Instant::now() < deadline,
            "candidate failed or timed out: {:?}",
            job.error
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let ResponseBody::Candidate(candidate) = client.execute(
        Command::GetCandidate {
            candidate_id: candidate_id.clone(),
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("candidate expected");
    };
    let raw = serde_json::to_value(&candidate.program)?;
    let tracks = raw["tracks"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("tracks required"))?;
    ensure!(
        tracks.len() == 1 && tracks[0]["axis"] == "stroke",
        "one stroke track required"
    );
    let actions = tracks[0]["actions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("actions required"))?;
    ensure!(actions.len() == 17, "17 authored sample points required");
    let pattern_case = args
        .get(3)
        .map(|value| {
            value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("UTF-8 pattern case required"))
        })
        .transpose()?
        .unwrap_or("triangle");
    let cycle = match pattern_case {
        "triangle" => [0.0, 0.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5],
        "pulse25" => [1.0, 1.0, -1.0, -1.0, -1.0, -1.0, -1.0, -1.0],
        "pulse75" => [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0],
        _ => anyhow::bail!("unsupported authored pattern case"),
    };
    for (k, action) in actions.iter().enumerate() {
        ensure!(
            action["time"] == serde_json::json!(k as i64 * 125_000_000)
                && action["evidence"] == "synthesized",
            "time/evidence differs"
        );
        let position = action["position"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("position required"))?;
        ensure!(
            (position - (0.5 + 0.1 * cycle[k % 8])).abs() < 1e-12,
            "independent pattern oracle differs"
        );
    }
    let current = get_snapshot(&mut client, &project)?;
    ensure!(
        current.program == before.program && current.revision == before.revision,
        "generation changed master"
    );
    let ResponseBody::Project(committed) = client.execute(
        Command::CommitCandidate { candidate_id },
        Some(project.clone()),
        Some(current.revision),
    )?
    else {
        anyhow::bail!("commit project required");
    };
    ensure!(
        committed.program == candidate.program,
        "commit changed program"
    );
    let ResponseBody::Project(undone) = client.execute(
        Command::Undo,
        Some(project.clone()),
        Some(committed.revision),
    )?
    else {
        anyhow::bail!("undo project required");
    };
    ensure!(undone.program.tracks().is_empty(), "undo failed");
    let ResponseBody::Project(redone) =
        client.execute(Command::Redo, Some(project), Some(undone.revision))?
    else {
        anyhow::bail!("redo project required");
    };
    ensure!(redone.program == candidate.program, "redo changed program");
    println!(
        "{}",
        serde_json::json!({"qualified":false,"pattern_case":pattern_case,"independent_actions":17,"master_unchanged_before_explicit_commit":true,"undo_redo":"PASS","candidate":candidate})
    );
    Ok(())
}
