//! Actual shared desktop audio controller, candidate review and explicit commit.
use anyhow::{ensure, Result};
use pulsar_clients::{
    audio_panel::AudioPanel, create_project, get_snapshot, EngineApi, ResponseBody, SessionClient,
};
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: desktop-audio-bundle-fixture STATE SOURCE RECIPE"
    );
    let state = PathBuf::from(&args[0]);
    let source = PathBuf::from(&args[1]);
    let recipe_path = PathBuf::from(&args[2]);
    ensure!(
        state.is_absolute() && source.is_absolute() && recipe_path.is_absolute(),
        "absolute paths required"
    );
    let bytes = std::fs::read(&recipe_path)?;
    ensure!(bytes.len() <= 65536, "bounded authored recipe required");
    let recipe: AudioBundleRecipe = serde_json::from_slice(&bytes)?;
    let mut panel = AudioPanel::default();
    panel.set_bundle_recipe(&recipe);
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let project = create_project(&mut client, "Owned desktop audio bundle fixture".into())?;
    let ResponseBody::Source { source_version } = client.execute(
        Command::ImportSource { path: source },
        Some(project.project_id.clone()),
        Some(project.revision),
    )?
    else {
        anyhow::bail!("source required")
    };
    let before = get_snapshot(&mut client, &project.project_id)?;
    let input = panel.input(source_version)?;
    input.validate()?;
    let ResponseBody::Job(job) = client.execute(
        Command::GenerateInput {
            input: input.clone(),
        },
        Some(project.project_id.clone()),
        Some(before.revision),
    )?
    else {
        anyhow::bail!("job required")
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    let candidate_id = loop {
        let ResponseBody::Job(status) = client.execute(
            Command::JobStatus {
                job_id: job.job_id.clone(),
            },
            Some(project.project_id.clone()),
            None,
        )?
        else {
            anyhow::bail!("status required")
        };
        ensure!(
            status.project_id == project.project_id && status.base_revision == before.revision,
            "job binding mismatch"
        );
        if let Some(id) = status.candidate_id {
            break id;
        }
        ensure!(
            status.error.is_none() && Instant::now() < deadline,
            "audio generation failed/timed out: {:?}",
            status.error
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let ResponseBody::Candidate(candidate) = client.execute(
        Command::GetCandidate {
            candidate_id: candidate_id.clone(),
        },
        Some(project.project_id.clone()),
        None,
    )?
    else {
        anyhow::bail!("candidate required")
    };
    ensure!(
        candidate.project_id == before.project_id
            && candidate.base_revision == before.revision
            && candidate.program.tracks().len() == 6,
        "candidate binding/axes mismatch"
    );
    ensure!(
        candidate
            .program
            .tracks()
            .iter()
            .flat_map(|track| track.actions())
            .all(|action| action.evidence() == EvidenceKind::Synthesized),
        "wrong evidence"
    );
    let current = get_snapshot(&mut client, &project.project_id)?;
    ensure!(
        current.revision == before.revision && current.program == before.program,
        "proposal changed master"
    );
    let ResponseBody::Project(committed) = client.execute(
        Command::CommitCandidate { candidate_id },
        Some(project.project_id.clone()),
        Some(before.revision),
    )?
    else {
        anyhow::bail!("commit required")
    };
    ensure!(
        committed.program == candidate.program,
        "commit changed candidate"
    );
    let ResponseBody::Project(undone) = client.execute(
        Command::Undo,
        Some(project.project_id.clone()),
        Some(committed.revision),
    )?
    else {
        anyhow::bail!("undo required")
    };
    ensure!(undone.program == before.program, "undo changed baseline");
    let ResponseBody::Project(redone) = client.execute(
        Command::Redo,
        Some(project.project_id.clone()),
        Some(undone.revision),
    )?
    else {
        anyhow::bail!("redo required")
    };
    ensure!(
        redone.program == candidate.program,
        "redo changed candidate"
    );
    let error = client
        .execute(
            Command::GenerateInput { input },
            Some(project.project_id.clone()),
            Some(before.revision),
        )
        .unwrap_err();
    ensure!(
        error
            .downcast_ref::<ProtocolError>()
            .is_some_and(|error| error.code == ErrorCode::RevisionConflict),
        "stale source snapshot must refuse"
    );
    ensure!(
        get_snapshot(&mut client, &project.project_id)?.program == redone.program,
        "stale request changed master"
    );
    println!(
        "{}",
        serde_json::json!({"qualified":false,"master_unchanged_before_explicit_commit":true,"undo_redo":"PASS","stale_refusal":"PASS","candidate":candidate})
    );
    Ok(())
}
