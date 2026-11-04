//! Protocol-only modality orchestration shared by command-line entry points.
use crate::{absolute_path, create_project, get_snapshot, session::EngineApi};
use anyhow::{bail, Context, Result};
use pulsar_protocol::*;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) enum Input {
    Text {
        prompt: String,
    },
    Image {
        path: PathBuf,
        prompt: String,
    },
    Audio {
        path: PathBuf,
        mapping: AudioMapping,
        mode: AudioMode,
        analysis: Option<AudioAnalysisOptions>,
    },
}

pub(crate) fn synthesize(api: &mut impl EngineApi, input: Input, output: &Path) -> Result<()> {
    let mut project = create_project(api, "Synthesized motion".into())?;
    let input = match input {
        Input::Text { prompt } => GenerationInput::Text { prompt },
        Input::Image { path, prompt } => {
            let source_version = import_source(api, &project, &path)?;
            project = get_snapshot(api, &project.project_id)?;
            GenerationInput::Image {
                source_version,
                prompt,
            }
        }
        Input::Audio {
            path,
            mapping,
            mode,
            analysis,
        } => {
            let source_version = import_source(api, &project, &path)?;
            project = get_snapshot(api, &project.project_id)?;
            GenerationInput::Audio {
                source_version,
                mapping,
                mode,
                analysis,
            }
        }
    };
    eprintln!(
        "Project: {}. Deterministic synthesis is not local AI inference.",
        project.project_id
    );
    let job = match api.execute(
        Command::GenerateInput { input },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Job(job) => job,
        _ => bail!("Engine returned an unexpected synthesis response"),
    };
    let candidate = await_candidate(api, &project, job)?;
    let axis = match candidate.program.tracks() {
        [track] => track.axis(),
        _ => {
            bail!("Synthesis returned multiple or missing axes; use explicit selected-axis export")
        }
    };
    commit_and_export(api, &project, candidate, output, axis)
}

/// One captured source and one candidate/commit; export each selected neutral track.
/// A new directory prevents accidental replacement. Failure retains recoverable
/// partial exports and the exact committed project, never reports bundle success.
pub(crate) fn synthesize_audio_bundle(
    api: &mut impl EngineApi,
    source: &Path,
    recipe: AudioBundleRecipe,
    output: &Path,
) -> Result<()> {
    let output = absolute_path(output)?;
    std::fs::create_dir(&output)?;
    eprintln!("Audio bundle directory reserved: {}. Partial exports remain recoverable if generation or publication fails.", output.display());
    let committed = prepare_audio_bundle(api, source, recipe)?;
    let mut exported = Vec::new();
    for axis in Axis::ALL {
        let filename = format!(
            "{}.funscript",
            serde_json::to_value(axis)?
                .as_str()
                .context("Invalid neutral axis name")?
        );
        let path = output.join(filename);
        match api.execute(Command::ExportAxis { path: path.clone(), axis }, Some(committed.project_id.clone()), Some(committed.revision)) {
            Ok(ResponseBody::AxisExported { path: actual_path, revision, axis: actual_axis }) if actual_path == path && revision == committed.revision && actual_axis == axis => exported.push(path),
            Ok(_) => bail!("Unexpected audio bundle export response; retained project {}, revision {}, completed files {:?}", committed.project_id, committed.revision, exported),
            Err(error) => bail!("Audio bundle publication incomplete; retained project {}, revision {}, completed files {:?}: {error}", committed.project_id, committed.revision, exported),
        }
    }
    crate::print_json(
        &serde_json::json!({"project_id": committed.project_id, "revision": committed.revision,
        "files": exported, "evidence": "synthesized", "qualified": false,
        "publication": "six individual recoverable Engine exports; no aggregate atomic transaction"}),
    )
}

fn prepare_audio_bundle(
    api: &mut impl EngineApi,
    source: &Path,
    recipe: AudioBundleRecipe,
) -> Result<ProjectSnapshot> {
    let mut project = create_project(api, "Synthesized six-axis audio".into())?;
    eprintln!("Audio bundle project retained: {}. Audio mappings are synthesized, not physical axis measurements.", project.project_id);
    let source_version = import_source(api, &project, source)?;
    project = get_snapshot(api, &project.project_id)?;
    let job = match api.execute(
        Command::GenerateInput {
            input: GenerationInput::AudioBundle {
                source_version,
                recipe,
            },
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Job(job) => job,
        _ => bail!("Engine returned an unexpected audio bundle response"),
    };
    let candidate = await_candidate(api, &project, job)?;
    anyhow::ensure!(
        candidate.project_id == project.project_id && candidate.base_revision == project.revision,
        "Audio bundle candidate requires explicit rebase and fresh review"
    );
    anyhow::ensure!(
        candidate.program.tracks().len() == Axis::ALL.len(),
        "Audio bundle candidate lacks six neutral tracks"
    );
    print_candidate_review(&candidate);
    let committed = match api.execute(
        Command::CommitCandidate {
            candidate_id: candidate.candidate_id,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Project(project) => project,
        _ => bail!("Engine returned an unexpected audio bundle commit response"),
    };
    Ok(*committed)
}

pub(crate) fn synthesize_atomic_audio_bundle(
    api: &mut crate::SessionClient,
    source: &Path,
    recipe: AudioBundleRecipe,
    output: &Path,
) -> Result<()> {
    crate::print_json(&publish_atomic_audio_bundle(api, source, recipe, output)?)
}

pub(crate) fn publish_atomic_audio_bundle(
    api: &mut crate::SessionClient,
    source: &Path,
    recipe: AudioBundleRecipe,
    output: &Path,
) -> Result<crate::neutral_bundle::PublishedNeutralBundle> {
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (api, source, recipe, output);
        return Err(ProtocolError::unsupported(
            "atomic neutral bundle publication on this platform",
        )
        .into());
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let output = absolute_path(output)?;
        match std::fs::symlink_metadata(&output) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!("Bundle destination exists; nothing overwritten"),
        }
        output
            .file_name()
            .context("New bundle directory name required")?;
        let parent = output
            .parent()
            .context("Bundle destination parent required")?
            .canonicalize()?;
        anyhow::ensure!(
            parent.is_dir(),
            "Bundle destination parent must be a directory"
        );
        let committed = prepare_audio_bundle(api, source, recipe)?;
        let receipt = crate::neutral_bundle::export_neutral_bundle(
            api,
            committed.project_id,
            committed.revision,
            &output,
            &crate::project_package::PackageDownloadControl::default(),
        )?;
        Ok(receipt)
    }
}

fn import_source(
    api: &mut impl EngineApi,
    project: &ProjectSnapshot,
    path: &Path,
) -> Result<SourceVersionId> {
    match api.execute(
        Command::ImportSource {
            path: absolute_path(path)?,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Source { source_version } => Ok(source_version),
        _ => bail!("Engine returned an unexpected source import response"),
    }
}

/// Upload only checked numeric values; the engine derives evidence and lineage.
pub(crate) fn edit_values(
    api: &mut impl EngineApi,
    project: &str,
    file: &Path,
    revision: RevisionId,
    label: &str,
) -> Result<()> {
    use std::io::Read;
    let mut input = std::fs::File::open(file)?;
    anyhow::ensure!(
        input.metadata()?.is_file() && input.metadata()?.len() <= MAX_MOTION_BYTES,
        "Edit values require one bounded regular file"
    );
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_MOTION_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_MOTION_BYTES,
        "Edit values exceed the bounded upload"
    );
    let values: EditValuesProgram = serde_json::from_slice(&bytes)?;
    let program = MotionProgram::new(
        values
            .tracks()
            .iter()
            .map(|track| {
                let actions = track
                    .actions()
                    .iter()
                    .map(|action| {
                        MotionAction::new(
                            action.time(),
                            action.position(),
                            EvidenceKind::Synthesized,
                        )
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                MotionTrack::with_gaps(track.axis(), actions, track.gaps().to_vec())
            })
            .collect::<std::result::Result<Vec<_>, _>>()?,
    )?;
    let candidate = api.upload_edit(ProjectId::new(project)?, revision, &program, label)?;
    eprintln!("Manual values are an uncommitted candidate. Review and explicitly commit.");
    crate::print_response(ResponseBody::Candidate((candidate).into()))
}

fn print_candidate_review(candidate: &CandidateSnapshot) {
    for flag in candidate.review.iter().take(100) {
        eprintln!(
            "REVIEW {} {:?} {:.3}..{:.3}s {:?}",
            flag.reason.as_str(),
            flag.axis,
            flag.range.start().as_nanos() as f64 / 1e9,
            flag.range.end().as_nanos() as f64 / 1e9,
            flag.evidence
        );
    }
    if candidate.review.len() > 100 {
        eprintln!(
            "{} additional review flags retained on candidate {}",
            candidate.review.len() - 100,
            candidate.candidate_id
        );
    }
}

/// Existing project generation does not replace protected or absent axes.
pub(crate) fn regenerate(
    api: &mut impl EngineApi,
    project: &str,
    source: &str,
    revision: RevisionId,
    settings: GenerationSettings,
    model: Option<&Path>,
    preset: &str,
) -> Result<()> {
    settings.validate()?;
    let project = get_snapshot(api, &ProjectId::new(project)?)?;
    anyhow::ensure!(
        project.revision == revision,
        "Regeneration requires the current explicitly named revision"
    );
    let job = match api.execute(
        Command::Generate {
            source_version: SourceVersionId::new(source)?,
            preset: preset.into(),
            settings: Some(settings),
            model_path: model.map(absolute_path).transpose()?,
        },
        Some(project.project_id.clone()),
        Some(revision),
    )? {
        ResponseBody::Job(job) => job,
        _ => bail!("Unexpected regeneration job response"),
    };
    let candidate = await_candidate(api, &project, job)?;
    print_candidate_review(&candidate);
    let range = candidate.project_interval;
    let axes = candidate
        .program
        .tracks()
        .iter()
        .map(|track| track.axis())
        .collect();
    eprintln!("Applying unqualified regeneration only to unlocked candidate axes. Quality remains unqualified; no file export follows.");
    crate::print_response(api.execute(
        Command::MergeCandidate {
            candidate_id: candidate.candidate_id,
            axes,
            range,
        },
        Some(project.project_id),
        Some(revision),
    )?)
}

pub(crate) fn import_script(
    api: &mut impl EngineApi,
    path: &Path,
    existing_project: Option<&str>,
) -> Result<()> {
    let project = match existing_project {
        Some(id) => match api.execute(
            Command::OpenProject {
                project_id: ProjectId::new(id)?,
            },
            None,
            None,
        )? {
            ResponseBody::Project(project) => project,
            _ => bail!("Engine returned an unexpected project response"),
        },
        None => Box::new(create_project(
            api,
            format!(
                "Imported {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        )?),
    };
    let response = api.execute(
        Command::ImportFunscript {
            path: absolute_path(path)?,
        },
        Some(project.project_id),
        Some(project.revision),
    )?;
    eprintln!("Imported script is an uncommitted candidate. Review before explicit merge/commit.");
    crate::print_response(response)
}

pub(crate) fn await_candidate(
    api: &mut impl EngineApi,
    project: &ProjectSnapshot,
    mut job: JobSnapshot,
) -> Result<CandidateSnapshot> {
    eprintln!(
        "Job: {}. Disconnecting does not cancel engine-owned work.",
        job.job_id
    );
    let candidate_id = loop {
        if let Some(error) = &job.error {
            bail!("Job {}: {error}", job.job_id);
        }
        if let Some(candidate_id) = job.candidate_id {
            break candidate_id;
        }
        if matches!(
            job.state.to_ascii_lowercase().as_str(),
            "cancelled" | "canceled" | "failed" | "interrupted"
        ) {
            bail!(
                "Job {} ended with state {}; project retained",
                job.job_id,
                job.state
            );
        }
        std::thread::sleep(Duration::from_millis(250));
        job = match api.execute(
            Command::JobStatus {
                job_id: job.job_id.clone(),
            },
            Some(project.project_id.clone()),
            None,
        )? {
            ResponseBody::Job(job) => job,
            _ => bail!("Engine returned an unexpected job response"),
        };
    };
    match api.execute(
        Command::GetCandidate { candidate_id },
        Some(project.project_id.clone()),
        None,
    )? {
        ResponseBody::Candidate(candidate) => Ok(*candidate),
        _ => bail!("Engine returned an unexpected candidate response"),
    }
}

pub(crate) fn commit_and_export(
    api: &mut impl EngineApi,
    project: &ProjectSnapshot,
    candidate: CandidateSnapshot,
    output: &Path,
    axis: Axis,
) -> Result<()> {
    if candidate.project_id != project.project_id || candidate.base_revision != project.revision {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "candidate requires explicit rebase and fresh review",
        )
        .into());
    }
    print_candidate_review(&candidate);
    let committed = match api.execute(
        Command::CommitCandidate {
            candidate_id: candidate.candidate_id,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Project(project) => project,
        _ => bail!("Engine returned an unexpected candidate commit response"),
    };
    crate::print_response(api.execute(
        Command::ExportAxis {
            path: absolute_path(output)?,
            axis,
        },
        Some(committed.project_id),
        Some(committed.revision),
    )?)
}

pub(crate) fn parse_axis(value: &str) -> Result<Axis> {
    match value {
        "stroke" => Ok(Axis::Stroke),
        "sway" => Ok(Axis::Sway),
        "surge" => Ok(Axis::Surge),
        "roll" => Ok(Axis::Roll),
        "pitch" => Ok(Axis::Pitch),
        "yaw" | "twist" => Ok(Axis::Yaw),
        _ => Err(ProtocolError::unsupported(format!(
            "axis '{value}'; supported: stroke,sway,surge,roll,pitch,yaw"
        ))
        .into()),
    }
}

pub(crate) fn parse_axes(value: &str) -> Result<Vec<Axis>> {
    let axes = value
        .split(',')
        .map(parse_axis)
        .collect::<Result<Vec<_>>>()?;
    if axes.is_empty()
        || axes
            .iter()
            .enumerate()
            .any(|(index, axis)| axes[..index].contains(axis))
    {
        bail!("Choose nonempty distinct axes");
    }
    Ok(axes)
}

pub(crate) fn range(start: Option<f64>, end: Option<f64>) -> Result<Option<TimeRange>> {
    match (start, end) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) => {
            let convert = |value: f64| -> Result<ProjectTime> {
                let nanos = value * 1e9;
                if !nanos.is_finite() || nanos < 0.0 || nanos >= i64::MAX as f64 {
                    bail!("Project time is not a finite representable nonnegative timestamp");
                }
                Ok(ProjectTime::from_nanos(nanos.round() as i64))
            };
            Ok(Some(TimeRange::new(convert(start)?, convert(end)?)?))
        }
        _ => bail!("Both --start and --end are required for a bounded merge"),
    }
}

/// Capture a standalone script as an uncommitted candidate, then query it.
/// The retained diagnostic project is an explicit effect; the script is never edited.
pub(crate) fn doctor(api: &mut impl EngineApi, path: &Path, maximum: f64) -> Result<()> {
    validate_segment_speed_limit(maximum)?;
    let path = absolute_path(path)?;
    let project = create_project(api, "Script diagnostics".into())?;
    eprintln!("Diagnostic project retained: {}. Script remains an uncommitted candidate; original file is unchanged.", project.project_id);
    let candidate = match api.execute(
        Command::ImportFunscript { path },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Candidate(candidate) => candidate,
        _ => bail!("Engine returned an unexpected script import response"),
    };
    review(
        api,
        project.project_id.as_str(),
        Some(candidate.candidate_id.as_str()),
        Some(maximum),
    )
}

pub(crate) fn review(
    api: &mut impl EngineApi,
    project: &str,
    candidate: Option<&str>,
    maximum: Option<f64>,
) -> Result<()> {
    if let Some(limit) = maximum {
        validate_segment_speed_limit(limit)?;
    }
    crate::print_response(api.execute(
        Command::Diagnostics {
            candidate_id: candidate.map(CandidateId::new).transpose()?,
            maximum_speed_percent_per_second: maximum,
        },
        Some(ProjectId::new(project)?),
        None,
    )?)
}

pub(crate) fn merge(
    api: &mut impl EngineApi,
    project: &str,
    candidate: &str,
    axes: &str,
    start: Option<f64>,
    end: Option<f64>,
    output: Option<&Path>,
) -> Result<()> {
    let project = get_snapshot(api, &ProjectId::new(project)?)?;
    let selected_axes = parse_axes(axes)?;
    let export_axis = match (output, selected_axes.as_slice()) {
        (Some(_), [axis]) => Some(*axis),
        (Some(_), _) => bail!("Multi-axis merge cannot imply a single-axis export; use export-axis explicitly afterward"),
        (None, _) => None,
    };
    let merged = api.execute(
        Command::MergeCandidate {
            candidate_id: CandidateId::new(candidate)?,
            axes: selected_axes,
            range: range(start, end)?,
        },
        Some(project.project_id),
        Some(project.revision),
    )?;
    match (merged, output) {
        (ResponseBody::Project(project), Some(path)) => crate::print_response(api.execute(
            Command::ExportAxis {
                path: absolute_path(path)?,
                axis: export_axis.expect("export axis validated before merge"),
            },
            Some(project.project_id),
            Some(project.revision),
        )?),
        (response, None) => crate::print_response(response),
        _ => bail!("Engine returned an unexpected merge response"),
    }
}

use crate::motion::{CandidateSnapshot, ProjectSnapshot, ResponseBody};

/// Capture a standalone source through the existing project import authority.
pub(crate) fn info(api: &mut impl EngineApi, path: &Path) -> Result<()> {
    let project = create_project(api, "Media inspection".into())?;
    eprintln!("Inspection project: {}", project.project_id);
    let source_version = import_source(api, &project, path)?;
    let project = get_snapshot(api, &project.project_id)?;
    let ResponseBody::MediaProbe(report) = api.execute(
        Command::ProbeMedia {
            source_version: source_version.clone(),
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )?
    else {
        bail!("Unexpected media inspection response");
    };
    report.validate()?;
    anyhow::ensure!(
        report.project_id == project.project_id
            && report.revision == project.revision
            && report.source_version == source_version,
        "Media inspection source/revision mismatch"
    );
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_arguments_preserve_checked_units_and_reject_ambiguous_ranges() {
        assert!(range(Some(f64::NAN), Some(3.0)).is_err());
        assert!(range(Some(3.0), Some(1.0)).is_err());
        assert!(range(Some(0.0), None).is_err());
        let range = range(Some(0.125), Some(0.5)).unwrap().unwrap();
        assert_eq!(range.start().as_nanos(), 125_000_000);
        assert_eq!(range.end().as_nanos(), 500_000_000);
        assert!(parse_axes("stroke,stroke").is_err());
        assert!(parse_axes("suction").is_err());
        assert_eq!(parse_axis("twist").unwrap(), Axis::Yaw);
    }
}
