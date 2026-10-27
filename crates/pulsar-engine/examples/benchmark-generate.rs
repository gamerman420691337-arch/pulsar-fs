//! Single production generation attempt and independent candidate conversion.
use anyhow::{ensure, Result};
use pulsar_engine::{benchmark, EngineConfig};
use pulsar_protocol::*;
use serde::Deserialize;
use std::{io::Read, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    worker_path: PathBuf,
    worker_identity: ArtifactIdentity,
    source: SourceArtifact,
    tools: WorkerTools,
    settings: GenerationSettings,
    model: Option<SourceArtifact>,
    #[serde(default)]
    profile: Option<DeclaredInferenceProfile>,
    preset: String,
    budget: ResourceBudget,
    subject: String,
    target: String,
    window_start_ns: String,
    window_end_ns: String,
    #[serde(default)]
    evaluation_window_policy: benchmark::EvaluationWindowPolicy,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: benchmark-generate INPUT.json NEW-OUTPUT-DIRECTORY"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "generation input exceeds bound");
    let input: Input = serde_json::from_slice(&bytes)?;
    let window = TimeRange::new(
        ProjectTime::from_nanos(input.window_start_ns.parse()?),
        ProjectTime::from_nanos(input.window_end_ns.parse()?),
    )?;
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    let root = root.canonicalize()?;
    let output_dir = root.join("artifacts");
    std::fs::create_dir(&output_dir)?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut manifest = WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new(id.clone())?,
        job_id: JobId::new(id.clone())?,
        project_id: ProjectId::new(id)?,
        base_revision: RevisionId::new(0),
        source: input.source,
        output_dir,
        operation: WorkerOperation::Generate {
            preset: input.preset,
            settings: (input.settings).into(),
            model: input.model,
        },
        budget: input.budget,
        dependencies: Vec::new(),
        tools: input.tools,
    };
    if let WorkerOperation::Generate { settings, .. } = &mut manifest.operation {
        if let Some(profile) = &input.profile {
            if let Some(existing) = &settings.inference_profile {
                ensure!(
                    serde_json::to_value(existing)? == serde_json::to_value(profile)?,
                    "conflicting campaign profiles"
                );
            }
            settings.inference_profile = Some(profile.clone());
        }
    }
    let selected = match &manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        _ => None,
    };
    let admitted = match selected {
        Some(profile) => Some(benchmark::admit_generation_profile(profile, &manifest)?),
        None => {
            ensure!(
                !matches!(
                    &manifest.operation,
                    WorkerOperation::Generate { model: Some(_), .. }
                ),
                "model-backed campaign requires an explicit inference profile"
            );
            None
        }
    };
    let config = EngineConfig::new(root.join("unused-state"), input.worker_path);
    let mut execution = benchmark::execute_campaign_attempt(
        &config,
        &manifest,
        &input.worker_identity,
        &AtomicBool::new(false),
    )?;
    execution.declared_profile = admitted;
    benchmark::write_execution_receipt(&root.join("execution.json"), &execution)?;
    let (generated, review) = match &execution.result {
        Ok(WorkerOutput::Generated {
            program, review, ..
        }) => (program, review),
        other => anyhow::bail!("generation failed; receipt retained: {other:?}"),
    };
    let mut program = Vec::new();
    std::fs::File::open(&generated.path)?
        .take(MAX_MOTION_BYTES + 1)
        .read_to_end(&mut program)?;
    let mut converted = benchmark::motion_candidate_for_window(
        &program,
        &generated.identity.sha256,
        &input.subject,
        &input.target,
        window,
        input.evaluation_window_policy,
    )?;
    converted["receipt"]["worker_review"] = serde_json::to_value(review)?;
    converted["receipt"]["worker_peak_memory_bytes"] =
        serde_json::to_value(execution.worker_peak_memory_bytes)?;
    converted["receipt"]["memory_scope"] = serde_json::json!("whole-confined-worker-cgroup");
    converted["candidate"]["flags"] = benchmark::benchmark_review_flags_for_window(
        review,
        Axis::Stroke,
        window,
        input.evaluation_window_policy,
    )?;
    converted["receipt"]["review_window_policy"] =
        serde_json::to_value(input.evaluation_window_policy)?;
    converted["receipt"]["review_evidence"] =
        serde_json::json!("native-global-and-stroke-flags-mapped-v1");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("candidate.json"))?;
    serde_json::to_writer(&mut file, &converted)?;
    file.sync_all()?;
    Ok(())
}
