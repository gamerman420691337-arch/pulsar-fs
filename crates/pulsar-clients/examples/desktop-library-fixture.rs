//! Native IPC exercise of the actual desktop controller; headless drawing is separate.
use anyhow::{ensure, Result};
use pulsar_clients::library_panel::{
    execute, LibraryAction, LibraryPanel, LibraryReply, WorkflowControl,
};
use pulsar_protocol::RequestId;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn wait(panel: &mut LibraryPanel) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while panel.busy() {
        panel.poll();
        ensure!(
            Instant::now() < deadline,
            "desktop controller did not return"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4 || args.len() == 5,
        "usage: desktop-library-fixture STATE FOLDER RECIPE WORKFLOW_DIRECTORY [editor|visual-editor|pack-editor]"
    );
    let state = PathBuf::from(&args[0]);
    let folder = PathBuf::from(&args[1]);
    let recipe_path = PathBuf::from(&args[2]);
    let directory = PathBuf::from(&args[3]);
    ensure!(
        [&state, &folder, &recipe_path, &directory]
            .iter()
            .all(|p| p.is_absolute()),
        "absolute paths required"
    );
    let endpoint = state.join("engine.sock");
    let bootstrap = state.join("pairing.token");
    let mut panel = LibraryPanel::new(endpoint.clone(), bootstrap.clone());
    ensure!(
        !panel.busy() && panel.reply.is_none(),
        "startup must not dispatch"
    );
    if args.len() == 5 && args[4] == "retire-pending" {
        let sha256 = std::fs::read_to_string(&recipe_path)?.trim().to_owned();
        panel.submit(LibraryAction::RetirePending {
            directory,
            pending: folder,
            sha256,
        })?;
        wait(&mut panel)?;
        let Some(LibraryReply::PendingRetired(receipt)) = &panel.reply else {
            anyhow::bail!("pending retirement failed: {}", panel.message);
        };
        ensure!(
            receipt.file_and_directory_synced && !receipt.qualified,
            "retirement scope differs"
        );
        println!("{}", serde_json::to_string(&panel.reply)?);
        return Ok(());
    }
    if args.len() == 5 && args[4] == "migrate" {
        let observed: pulsar_protocol::LibraryBatchReport =
            serde_json::from_slice(&std::fs::read(&folder)?)?;
        let recipe: pulsar_protocol::LibraryBatchRecipe =
            serde_json::from_slice(&std::fs::read(&recipe_path)?)?;
        let migration_id = RequestId::new("desktop-fixture-migrate")?;
        panel.submit(LibraryAction::MigrateIntake {
            batch: observed.batch_request_id.clone(),
            expected_report_sha256: observed.report_sha256()?,
            recipe_path: recipe_path.clone(),
            request_id: migration_id.clone(),
        })?;
        wait(&mut panel)?;
        let Some(LibraryReply::Generation(migrated)) = &panel.reply else {
            anyhow::bail!("desktop migration failed: {}", panel.message)
        };
        ensure!(
            migrated.batch_request_id == migration_id
                && migrated
                    .migration
                    .as_ref()
                    .is_some_and(|m| m.parent_batch_request_id == observed.batch_request_id),
            "migration acknowledgement differs"
        );
        println!("{}", serde_json::to_string(&panel.reply)?);
        let retry = execute(
            &endpoint,
            &bootstrap,
            LibraryAction::Migrate {
                batch: observed.batch_request_id.clone(),
                expected_report_sha256: observed.report_sha256()?,
                recipe,
                request_id: migration_id.clone(),
            },
            &WorkflowControl::default(),
        );
        ensure!(
            matches!(retry.result, Ok(LibraryReply::Generation(_))),
            "exact desktop migration retry failed"
        );
        for _ in 0..100 {
            panel.submit(LibraryAction::Run {
                batch: migration_id.clone(),
                directory: directory.clone(),
                steps: 10,
                poll_ms: 1,
            })?;
            wait(&mut panel)?;
            let Some(LibraryReply::Workflow(report)) = &panel.reply else {
                anyhow::bail!("desktop successor run failed: {}", panel.message)
            };
            ensure!(!report.needs_attention, "desktop successor needs attention");
            if report.completed {
                println!("{}", serde_json::to_string(&panel.reply)?);
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        anyhow::bail!("desktop successor did not complete within step budget")
    }
    let prepare = RequestId::new("desktop-fixture-prepare")?;
    let action = if args.len() == 5 && args[4] == "pack-editor" {
        let recipe: pulsar_protocol::LibraryVisualRecipe =
            serde_json::from_slice(&std::fs::read(&recipe_path)?)?;
        panel.submit(LibraryAction::PackSelectionStatus)?;
        wait(&mut panel)?;
        let Some(LibraryReply::PackSelection(selection)) = &panel.reply else {
            anyhow::bail!("pack refresh failed: {}", panel.message)
        };
        ensure!(selection.choice.is_some(), "current pack absent");
        println!("{}", serde_json::to_string(&panel.reply)?);
        let editor = pulsar_clients::library_visual_panel::LibraryVisualPanel {
            preset: recipe.preset,
            use_pack: true,
            pack: recipe
                .model_pack
                .expect("explicit pack digest")
                .manifest_sha256,
            override_dimensions: true,
            width: recipe.settings.analysis_width,
            height: recipe.settings.analysis_height,
            override_threshold: recipe.settings.detector_score_threshold.is_some(),
            threshold: recipe.settings.detector_score_threshold.unwrap_or(0.25),
            stride: recipe.settings.source_sample_stride,
            video_view: recipe.settings.video_view.clone(),
            ..Default::default()
        };
        LibraryAction::IntakeVisual {
            folder,
            recursive: false,
            generation: editor.capture()?,
            video_view: editor.video_view.clone(),
            request_id: prepare.clone(),
        }
    } else if args.len() == 5 && args[4] == "visual-editor" {
        let recipe: pulsar_protocol::LibraryVisualRecipe =
            serde_json::from_slice(&std::fs::read(&recipe_path)?)?;
        recipe.validate()?;
        let input = recipe
            .settings
            .model_input
            .as_ref()
            .expect("explicit detector contract");
        let editor = pulsar_clients::library_visual_panel::LibraryVisualPanel {
            preset: recipe.preset.clone(),
            use_model: true,
            model: recipe
                .model_path
                .as_ref()
                .expect("file model")
                .to_string_lossy()
                .into(),
            class_count: input.class_count,
            override_dimensions: true,
            width: recipe.settings.analysis_width,
            height: recipe.settings.analysis_height,
            override_threshold: recipe.settings.detector_score_threshold.is_some(),
            threshold: recipe.settings.detector_score_threshold.unwrap_or(0.25),
            stride: recipe.settings.source_sample_stride,
            video_view: recipe.settings.video_view.clone(),
            ..Default::default()
        };
        LibraryAction::IntakeVisual {
            folder,
            recursive: false,
            generation: editor.capture()?,
            video_view: editor.video_view.clone(),
            request_id: prepare.clone(),
        }
    } else if args.len() == 5 {
        ensure!(args[4] == "editor", "only editor mode is supported");
        let recipe: pulsar_protocol::AudioBundleRecipe =
            serde_json::from_slice(&std::fs::read(&recipe_path)?)?;
        let mut editor = pulsar_clients::audio_panel::AudioPanel::default();
        editor.set_bundle_recipe(&recipe);
        let snapshot = editor.bundle_recipe()?;
        for channel in snapshot.channels() {
            let expected = recipe
                .channels()
                .iter()
                .find(|c| c.mapping.axis() == channel.mapping.axis())
                .expect("checked recipe");
            ensure!(
                serde_json::to_value(channel)? == serde_json::to_value(expected)?,
                "editor axis recipe mismatch"
            );
        }
        LibraryAction::IntakeRecipe {
            folder,
            recursive: false,
            recipe: snapshot,
            request_id: prepare.clone(),
        }
    } else {
        LibraryAction::Intake {
            folder,
            recursive: false,
            recipe_path,
            request_id: prepare.clone(),
        }
    };
    panel.submit(action)?;
    wait(&mut panel)?;
    let Some(LibraryReply::Generation(generation)) = &panel.reply else {
        anyhow::bail!("intake failed: {}", panel.message);
    };
    ensure!(
        generation.batch_request_id == prepare && generation.items.len() == 2,
        "intake scope/count differs"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    let control = WorkflowControl::default();
    control.request_stop();
    let paused = execute(
        &endpoint,
        &bootstrap,
        LibraryAction::Run {
            batch: prepare.clone(),
            directory: directory.clone(),
            steps: 1000,
            poll_ms: 1,
        },
        &control,
    );
    let LibraryReply::Workflow(paused) = paused.result.map_err(anyhow::Error::msg)? else {
        anyhow::bail!("paused workflow required");
    };
    ensure!(
        paused.local_stop_requested
            && paused.steps_used == 0
            && paused.generation_attempts == 0
            && paused.publication_attempts == 0
            && !paused.completed
            && !paused.budget_exhausted,
        "pre-stopped local scheduler dispatched work"
    );
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({"kind":"paused","report":paused}))?
    );
    panel.submit(LibraryAction::Run {
        batch: prepare.clone(),
        directory: directory.clone(),
        steps: 1,
        poll_ms: 1,
    })?;
    wait(&mut panel)?;
    let Some(LibraryReply::Workflow(one)) = &panel.reply else {
        anyhow::bail!("one step failed: {}", panel.message);
    };
    ensure!(
        one.steps_used == 1 && one.generation_attempts == 1 && one.budget_exhausted,
        "one-step scope differs"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    // Inspect through the actual background controller before completing the retained batch.
    panel.submit(LibraryAction::WorkflowStatus {
        batch: prepare.clone(),
        directory: directory.clone(),
    })?;
    wait(&mut panel)?;
    let Some(LibraryReply::Workflow(status)) = &panel.reply else {
        anyhow::bail!("status failed: {}", panel.message);
    };
    ensure!(status.steps_used == 0, "status advanced workflow");
    println!("{}", serde_json::to_string(&panel.reply)?);
    panel.submit(LibraryAction::Run {
        batch: prepare.clone(),
        directory: directory.clone(),
        steps: 1000,
        poll_ms: 1,
    })?;
    wait(&mut panel)?;
    let Some(LibraryReply::Workflow(completed)) = &panel.reply else {
        anyhow::bail!("run failed: {}", panel.message);
    };
    ensure!(
        completed.completed
            && !completed.needs_attention
            && completed
                .publication
                .items
                .iter()
                .all(|i| i.phase == pulsar_clients::library_panel::PublicationPhase::Published),
        "desktop workflow incomplete: {}",
        panel.message
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    panel.submit(LibraryAction::List)?;
    wait(&mut panel)?;
    let Some(LibraryReply::Catalog(catalog)) = &panel.reply else {
        anyhow::bail!("catalog failed");
    };
    ensure!(
        catalog
            .batches
            .iter()
            .any(|b| b.batch_request_id == prepare),
        "batch missing from owned catalog"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    let cancel = RequestId::new("desktop-fixture-cancel")?;
    panel.submit(LibraryAction::Cancel {
        batch: prepare.clone(),
        request_id: cancel.clone(),
    })?;
    wait(&mut panel)?;
    let Some(LibraryReply::Generation(stopped)) = &panel.reply else {
        anyhow::bail!("cancel failed: {}", panel.message);
    };
    ensure!(
        stopped.cancellation.as_ref().is_some_and(|c| c.completed
            && c.request_id == cancel
            && !c.worker_retirement_verified),
        "cancel acknowledgement differs"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    panel.submit(LibraryAction::Run {
        batch: prepare.clone(),
        directory,
        steps: 1,
        poll_ms: 1,
    })?;
    wait(&mut panel)?;
    let Some(LibraryReply::Workflow(stopped)) = &panel.reply else {
        anyhow::bail!("stopped run failed");
    };
    ensure!(
        stopped.steps_used == 0 && stopped.cancellation_acknowledged && !stopped.completed,
        "stopped workflow continued"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    panel.submit(LibraryAction::Release {
        batch: prepare,
        request_id: RequestId::new("desktop-fixture-release")?,
    })?;
    wait(&mut panel)?;
    ensure!(
        matches!(panel.reply, Some(LibraryReply::Released(_))),
        "release failed: {}",
        panel.message
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    ensure!(!panel.busy(), "owned client thread remains live");
    Ok(())
}
