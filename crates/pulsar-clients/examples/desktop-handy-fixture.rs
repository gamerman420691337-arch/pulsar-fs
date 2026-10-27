//! Actual first-party inspection preflight; no online credentials or hardware.
use anyhow::{ensure, Result};
use pulsar_clients::{create_project, get_snapshot, handy_panel::inspect, SessionClient};
use pulsar_protocol::*;
use std::path::PathBuf;
fn main() -> Result<()> {
    let state = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("usage: desktop-handy-fixture STATE"))?,
    );
    ensure!(state.is_absolute(), "absolute state required");
    let mut client =
        SessionClient::connect(&state.join("engine.sock"), &state.join("pairing.token"))?;
    let project = create_project(&mut client, "Owned Handy preflight fixture".into())?;
    let request = HandyInspectionRequest {
        api_key_env: "PULSAR_FIXTURE_ABSENT_HANDY_API".into(),
        connection_key_env: "PULSAR_FIXTURE_ABSENT_HANDY_CONNECTION".into(),
        allow_online_inspection: true,
    };
    let error = inspect(&mut client, &state, request)
        .expect_err("absent encrypted credentials must refuse before network");
    let typed = error
        .downcast_ref::<ProtocolError>()
        .ok_or_else(|| anyhow::anyhow!("typed credential preflight refusal required"))?;
    ensure!(
        typed
            .message
            .contains("credential is absent from the engine environment"),
        "unexpected preflight refusal: {}",
        typed.message
    );
    let after = get_snapshot(&mut client, &project.project_id)?;
    ensure!(
        after.revision == project.revision && after.program == project.program,
        "inspection changed project"
    );
    println!(
        "{}",
        serde_json::json!({"qualified":false,"physical_device_used":false,"production_inspection_helper":true,"credential_preflight_refused":true,"master_unchanged":true,"native_rendered_gui":"UNKNOWN"})
    );
    Ok(())
}
