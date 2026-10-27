//! Actual shared desktop proposal, review/commit, stale and protection checks.
use anyhow::{ensure, Result};
use pulsar_clients::repair_panel::{propose, RepairRequest};
use pulsar_clients::{create_project, get_snapshot, EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::*;
use std::path::PathBuf;
fn request(project: &pulsar_clients::ProjectSnapshot) -> RepairRequest {
    RepairRequest {
        project: project.project_id.clone(),
        revision: project.revision,
        program: project.program.clone(),
        options: ScriptRepairOptions::new(450.0, 20, 2).unwrap(),
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: desktop-repair-fixture STATE AUTHORED_SCRIPT"
    );
    let state = PathBuf::from(&args[0]);
    let source = PathBuf::from(&args[1]);
    ensure!(
        state.is_absolute() && source.is_absolute(),
        "absolute paths required"
    );
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let project = create_project(&mut client, "Owned desktop repair fixture".into())?;
    let ResponseBody::Candidate(imported) = client.execute(
        Command::ImportFunscriptForRepair { path: source },
        Some(project.project_id.clone()),
        Some(project.revision),
    )?
    else {
        anyhow::bail!("import candidate required")
    };
    ensure!(
        get_snapshot(&mut client, &project.project_id)?.revision == project.revision,
        "import changed master"
    );
    let ResponseBody::Project(baseline) = client.execute(
        Command::CommitCandidate {
            candidate_id: imported.candidate_id,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )?
    else {
        anyhow::bail!("baseline required")
    };
    let candidate = propose(&mut client, request(&baseline))?;
    let untouched = get_snapshot(&mut client, &project.project_id)?;
    ensure!(
        untouched.revision == baseline.revision && untouched.program == baseline.program,
        "proposal changed master"
    );
    let actions = candidate.program.track(Axis::Stroke).unwrap().actions();
    ensure!(actions.len() == 4, "four independently expected actions");
    for (a, (ms, pct)) in actions
        .iter()
        .zip([(0, 0.0), (20, 9.0), (40, 18.0), (100, 45.0)])
    {
        ensure!(
            a.time().as_nanos() == ms * 1_000_000
                && (a.position().value() - pct / 100.0).abs() < 1e-12,
            "wrong repair proposal"
        );
    }
    let ResponseBody::Project(committed) = client.execute(
        Command::CommitCandidate {
            candidate_id: candidate.candidate_id,
        },
        Some(project.project_id.clone()),
        Some(baseline.revision),
    )?
    else {
        anyhow::bail!("commit required")
    };
    let error = propose(&mut client, request(&baseline)).unwrap_err();
    ensure!(
        error
            .downcast_ref::<ProtocolError>()
            .is_some_and(|e| e.code == ErrorCode::RevisionConflict),
        "stale proposal must refuse"
    );
    ensure!(
        get_snapshot(&mut client, &project.project_id)?.program == committed.program,
        "stale proposal changed master"
    );
    let ResponseBody::Project(undone) = client.execute(
        Command::Undo,
        Some(project.project_id.clone()),
        Some(committed.revision),
    )?
    else {
        anyhow::bail!("Undo required")
    };
    ensure!(undone.program == baseline.program, "Undo changed baseline");
    let ResponseBody::Project(redone) = client.execute(
        Command::Redo,
        Some(project.project_id.clone()),
        Some(undone.revision),
    )?
    else {
        anyhow::bail!("Redo required")
    };
    ensure!(redone.program == committed.program, "Redo changed repair");
    let ResponseBody::Project(original) = client.execute(
        Command::Undo,
        Some(project.project_id.clone()),
        Some(redone.revision),
    )?
    else {
        anyhow::bail!("second Undo required")
    };
    client.execute(
        Command::SetProtectedRegions {
            regions: vec![ProtectedRegion {
                axis: Some(Axis::Stroke),
                range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(101_000_000))?,
            }],
        },
        Some(project.project_id.clone()),
        Some(original.revision),
    )?;
    let protected = get_snapshot(&mut client, &project.project_id)?;
    let result = match propose(&mut client, request(&protected)) {
        Ok(candidate) => client.execute(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(project.project_id.clone()),
            Some(protected.revision),
        ),
        Err(error) => Err(error),
    };
    let error = result.unwrap_err();
    ensure!(
        error
            .downcast_ref::<ProtocolError>()
            .is_some_and(|e| e.code == ErrorCode::Forbidden),
        "protected change must refuse"
    );
    let after = get_snapshot(&mut client, &project.project_id)?;
    ensure!(
        after.revision == protected.revision && after.program == protected.program,
        "protected refusal changed master"
    );
    println!(
        "{}",
        serde_json::json!({"qualified":false,"project_id":project.project_id,"sanitized_import_uncommitted":true,"repair_proposal_uncommitted":true,"independent_actions":4,"explicit_commit":true,"stale_refusal":true,"protected_refusal":true,"undo_redo":true,"native_rendered_gui":"UNKNOWN"})
    );
    Ok(())
}
