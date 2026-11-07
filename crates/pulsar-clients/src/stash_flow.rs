//! First-party batch submission/observation only. The engine owns scheduling,
//! phase identities, source capture, jobs, commits and publication/scan/tag effects.
use crate::{session::new_request_id, EngineApi, SessionClient};
use anyhow::{bail, Result};
use pulsar_protocol::*;

pub(crate) struct BatchConfig {
    pub connection: StashConnection,
    pub mappings: Vec<StashRootMapping>,
    pub limit: u32,
    pub owner_proof: String,
    pub preset: String,
    pub settings: GenerationSettings,
    pub model_path: Option<std::path::PathBuf>,
    pub model_pack: Option<ModelPackChoice>,
    pub annotated_views: Vec<StashBatchAnnotatedView>,
    pub source_plans: Vec<StashBatchSourcePlan>,
    pub scan: bool,
    pub tag: Option<StashBatchTag>,
    pub request_id: Option<RequestId>,
    pub detach: bool,
}
pub(crate) type BatchReport = StashBatchReport;
pub(crate) fn generate_missing(
    client: &mut SessionClient,
    config: &BatchConfig,
) -> Result<BatchReport> {
    let options = StashBatchOptions {
        source_plans: config.source_plans.clone(),
        connection: config.connection.clone(),
        mappings: config.mappings.clone(),
        limit: config.limit,
        preset: config.preset.clone(),
        settings: config.settings.clone(),
        annotated_views: config.annotated_views.clone(),
        model_path: config.model_path.clone(),
        model_pack: config.model_pack.clone(),
        scan: config.scan,
        tag: config.tag.clone(),
    };
    options.validate()?;
    if options.model_pack.is_none() || options.settings.model_input.is_some() {
        options.settings.validate_execution_preset(
            &options.preset,
            options.model_path.is_some() || options.model_pack.is_some(),
        )?;
    }

    let id = config.request_id.clone().unwrap_or_else(new_request_id);
    eprintln!("Stash batch {id}. Disconnecting does not cancel engine-owned scheduling; retain this ID for status/resume.");
    let crate::motion::ResponseBody::StashBatch(initial) = client.execute_with_id(
        id.clone(),
        Command::StashStartBatch {
            options,
            owner_proof: config.owner_proof.clone(),
        },
        None,
        None,
    )?
    else {
        bail!("unexpected batch submission response");
    };
    if config.detach {
        return Ok(initial);
    }
    loop {
        let mut report = status(client, &id, &config.owner_proof, 0)?;
        if report.state != "running" {
            while report.has_more {
                let next = status(client, &id, &config.owner_proof, report.items.len() as u32)?;
                report.has_more = next.has_more;
                report.items.extend(next.items);
            }
            return Ok(report);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
fn status(
    client: &mut SessionClient,
    id: &RequestId,
    proof: &str,
    offset: u32,
) -> Result<BatchReport> {
    match client.execute(
        Command::StashBatchStatus {
            batch_request_id: id.clone(),
            offset,
            owner_proof: proof.into(),
        },
        None,
        None,
    )? {
        crate::motion::ResponseBody::StashBatch(report) => Ok(report),
        _ => bail!("unexpected batch status response"),
    }
}
