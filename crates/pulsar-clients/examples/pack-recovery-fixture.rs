//! Authored native fixture; invokes the same recovery store/runner as the desktop.
use anyhow::{Context, Result};
use pulsar_clients::{
    pack_recovery as recovery,
    session::{EngineApi, SessionClient},
    ResponseBody,
};
use pulsar_protocol::*;
use std::path::PathBuf;
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    anyhow::ensure!(args.len() >= 2, "mode and fixture state directory required");
    let directory = PathBuf::from(&args[1]);
    let mut client = SessionClient::connect(
        &directory.join("engine.sock"),
        &directory.join("pairing.token"),
    )?;
    let id = RequestId::new(
        args.get(2)
            .cloned()
            .unwrap_or_else(|| "fixture-recovery".into()),
    )?;
    match args[0].as_str() {
        "stage-catalog" => {
            let source = PathBuf::from(args.get(3).context("catalog source required")?);
            let bootstrap = PathBuf::from(args.get(4).context("bootstrap file required")?);
            let digest = args.get(5).context("bootstrap digest required")?.clone();
            let revision = args.get(6).context("catalog revision required")?.parse()?;
            recovery::stage(
                &directory,
                &client,
                &id,
                &recovery::PackRecoveryIntent::CatalogRefresh {
                    source,
                    bootstrap,
                    bootstrap_sha256: digest,
                    expected_revision: revision,
                },
            )?;
            println!("{{\"staged\":true}}");
        }
        "stage-catalog-select" => {
            let digest = args.get(3).context("manifest required")?.clone();
            let bootstrap = args.get(4).context("bootstrap digest required")?.clone();
            let catalog = args.get(5).context("catalog revision required")?.parse()?;
            let revision = args
                .get(6)
                .context("selection revision required")?
                .parse()?;
            recovery::stage(
                &directory,
                &client,
                &id,
                &recovery::PackRecoveryIntent::CatalogSelect {
                    manifest_sha256: digest,
                    binding: ModelCatalogBinding {
                        bootstrap_sha256: bootstrap,
                        catalog_revision: catalog,
                    },
                    expected_revision: revision,
                },
            )?;
            println!("{{\"staged\":true}}");
        }
        "stage" => {
            recovery::stage(
                &directory,
                &client,
                &id,
                &recovery::PackRecoveryIntent::Deactivate {
                    expected_revision: 0,
                },
            )?;
            println!("{{\"staged\":true}}");
        }
        "lost-ack" => {
            let entry = recovery::load(
                &directory,
                client.session_id(),
                &client.recovery_credential_binding(),
            )?
            .into_iter()
            .find(|entry| entry.id == id)
            .context("Missing authored staged intent")?;
            // Fixture-owned state only: drop acknowledgement after actual engine response.
            let proof = std::fs::read_to_string(directory.join("model-pack-owner.token"))?;
            let command = entry.intent.command(proof.trim().into());
            recovery::prepare(
                &directory,
                client.session_id(),
                &client.recovery_credential_binding(),
                &id,
                &command,
            )?;
            print_receipt(client.execute_identified(id, command, None, None)?)?;
        }
        "replay" => {
            print_receipt(recovery::replay(&directory, &mut client, &id)?)?;
        }
        "list" => println!(
            "{}",
            serde_json::to_string(&recovery::load(
                &directory,
                client.session_id(),
                &client.recovery_credential_binding()
            )?)?
        ),
        "dismiss" => {
            recovery::dismiss(
                &directory,
                client.session_id(),
                &client.recovery_credential_binding(),
                &id,
            )?;
            println!("{{\"dismissed\":true}}");
        }
        "wrong-binding" => {
            recovery::load(&directory, client.session_id(), &"a".repeat(64))?;
            anyhow::bail!("Wrong binding unexpectedly accepted");
        }
        _ => anyhow::bail!("Unknown fixture mode"),
    }
    Ok(())
}

fn print_receipt(body: ResponseBody) -> Result<()> {
    match body {
        ResponseBody::ModelPackSelection(report) => println!("{}", serde_json::to_string(&report)?),
        ResponseBody::ModelCatalog(report) => println!("{}", serde_json::to_string(&report)?),
        _ => anyhow::bail!("Unexpected fixture receipt"),
    };
    Ok(())
}
