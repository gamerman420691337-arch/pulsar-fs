//! Actual desktop recovery controller IPC fixture; no rendered-window claim.
use anyhow::{ensure, Result};
use pulsar_clients::library_panel::{LibraryAction, LibraryPanel, LibraryReply};
use pulsar_protocol::RequestId;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn wait(panel: &mut LibraryPanel) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while panel.busy() {
        panel.poll();
        ensure!(Instant::now() < deadline, "controller deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
#[cfg(unix)]
struct RestorePermissions(PathBuf);
#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len()==6 || args.len()==7,"usage: desktop-library-recovery-fixture STATE BATCH WORKFLOW INDEX STAGE RECEIPT [retry-write|refuse]");
    let state = PathBuf::from(&args[0]);
    let batch = RequestId::new(
        args[1]
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("batch UTF-8"))?,
    )?;
    let directory = PathBuf::from(&args[2]);
    let index: u16 = args[3]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("index UTF-8"))?
        .parse()?;
    let staging = PathBuf::from(&args[4]);
    let preparation_receipt = PathBuf::from(&args[5]);
    ensure!(
        [&state, &directory, &staging, &preparation_receipt]
            .iter()
            .all(|p| p.is_absolute()),
        "absolute fixture paths"
    );
    let mode = args.get(6).and_then(|s| s.to_str()).unwrap_or("");
    ensure!(
        ["", "retry-write", "refuse"].contains(&mode),
        "fixture mode"
    );
    let mut panel = LibraryPanel::new(state.join("engine.sock"), state.join("pairing.token"));
    ensure!(
        !panel.busy() && panel.reply.is_none(),
        "startup must not dispatch"
    );
    let before = std::fs::read(directory.join("record.json"))?;
    #[cfg(unix)]
    let restore = if mode == "retry-write" {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))?;
        Some(RestorePermissions(directory.clone()))
    } else {
        None
    };
    #[cfg(not(unix))]
    ensure!(
        mode != "retry-write",
        "fixture requires Unix owned permissions"
    );
    panel.submit(LibraryAction::RecoverPreparation {
        batch: batch.clone(),
        directory: directory.clone(),
        index,
        staging: staging.clone(),
        preparation_receipt,
    })?;
    wait(&mut panel)?;
    if mode == "refuse" {
        ensure!(
            panel.reply.is_none() && panel.message.contains("Operation failed"),
            "expected preflight refusal"
        );
        ensure!(
            panel.retry().is_err(),
            "known preflight refusal must remain editable"
        );
        ensure!(
            std::fs::read(directory.join("record.json"))? == before,
            "preflight refusal changed record"
        );
        println!(
            "{}",
            serde_json::json!({"preflight_refused":true,"recovery_retained":false,"record_preserved":true})
        );
        return Ok(());
    }
    if mode == "retry-write" {
        ensure!(
            panel.reply.is_none() && panel.message.contains("could not be acknowledged"),
            "expected uncertain metadata write"
        );
        ensure!(
            std::fs::read(directory.join("record.json"))? == before,
            "owned read-only fixture changed record"
        );
        ensure!(
            panel
                .submit(LibraryAction::Cancel {
                    batch: batch.clone(),
                    request_id: RequestId::new("must-not-dispatch")?
                })
                .is_err(),
            "unknown recovery allowed replacement mutation"
        );
        #[cfg(unix)]
        drop(restore);
        panel.retry()?;
        wait(&mut panel)?;
    }
    let Some(LibraryReply::Workflow(report)) = &panel.reply else {
        anyhow::bail!("recovery failed: {}", panel.message)
    };
    ensure!(
        report.publication.batch_request_id == batch
            && report.steps_used == 0
            && report.generation_attempts == 0
            && report.publication_attempts == 0
            && !report.qualified,
        "recovery report scope"
    );
    let item = report
        .publication
        .items
        .get(index as usize)
        .ok_or_else(|| anyhow::anyhow!("entry absent"))?;
    ensure!(
        item.prepared.as_ref().is_some_and(|p| p.staging == staging),
        "original stage not retained"
    );
    ensure!(
        panel
            .message
            .contains("No generation or export was dispatched"),
        "recovery presentation"
    );
    println!("{}", serde_json::to_string(&panel.reply)?);
    Ok(())
}
