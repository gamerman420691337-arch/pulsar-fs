//! Authored native desktop-builder and history controller; no device effects.
use anyhow::{ensure, Result};
use pulsar_clients::{build_pattern_input, get_snapshot, EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: axis-pattern-fixture STATE PROJECT PATTERNS"
    );
    let state = PathBuf::from(&args[0]);
    let project = ProjectId::new(
        args[1]
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("project must be UTF-8"))?,
    )?;
    let text = std::fs::read_to_string(&args[2])?;
    ensure!(text.len() <= 24576, "bounded authored text required");
    let input = build_pattern_input(&text)?;
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let before = get_snapshot(&mut client, &project)?;
    ensure!(
        before.program.tracks().len() == 6,
        "committed six-axis fixture required"
    );
    let ResponseBody::Job(mut job) = client.execute(
        Command::GenerateInput { input },
        Some(project.clone()),
        Some(before.revision),
    )?
    else {
        anyhow::bail!("job expected")
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    let id = loop {
        if let Some(id) = job.candidate_id {
            break id;
        }
        ensure!(
            job.error.is_none() && Instant::now() < deadline,
            "candidate not completed: {:?}",
            job.error
        );
        std::thread::sleep(Duration::from_millis(20));
        let ResponseBody::Job(next) = client.execute(
            Command::JobStatus { job_id: job.job_id },
            Some(project.clone()),
            None,
        )?
        else {
            anyhow::bail!("job expected")
        };
        job = next;
    };
    let ResponseBody::Candidate(candidate) = client.execute(
        Command::GetCandidate { candidate_id: id },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("candidate expected")
    };
    ensure!(
        candidate.program == before.program,
        "CLI and desktop-builder programs differ"
    );
    let unchanged = get_snapshot(&mut client, &project)?;
    ensure!(
        unchanged.program == before.program && unchanged.revision == before.revision,
        "generation changed master"
    );
    let ResponseBody::Project(undone) = client.execute(
        Command::Undo,
        Some(project.clone()),
        Some(unchanged.revision),
    )?
    else {
        anyhow::bail!("undo project expected")
    };
    ensure!(
        undone.program.tracks().is_empty(),
        "undo did not restore empty master"
    );
    let ResponseBody::Project(redone) =
        client.execute(Command::Redo, Some(project), Some(undone.revision))?
    else {
        anyhow::bail!("redo project expected")
    };
    ensure!(
        redone.program == before.program,
        "redo did not restore exact six-axis master"
    );
    println!(
        "{}",
        serde_json::json!({"qualified":false,"undo_redo":"PASS","pattern_program":candidate.program})
    );
    Ok(())
}
