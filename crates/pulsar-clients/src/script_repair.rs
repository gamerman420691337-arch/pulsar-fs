//! Explicit standalone repair through existing import, edit and history contracts.
use crate::{absolute_path, create_project, modality, session::EngineApi};
use anyhow::{bail, Context, Result};
use pulsar_protocol::*;
use std::path::{Path, PathBuf};

pub(crate) fn output_path(input: &Path, output: Option<&Path>) -> Result<PathBuf> {
    let path = if let Some(path) = output {
        absolute_path(path)?
    } else {
        let input = absolute_path(input)?;
        let mut name = input
            .file_stem()
            .context("script requires a filename")?
            .to_os_string();
        name.push(".fixed.funscript");
        input.with_file_name(name)
    };
    let parent = path
        .parent()
        .context("output requires a parent")?
        .canonicalize()?;
    let output = parent.join(path.file_name().context("output requires a filename")?);
    match std::fs::symlink_metadata(&output) {
        Ok(_) => bail!("repair output already exists; choose a new destination"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(output)
}

pub(crate) fn fix(
    api: &mut impl EngineApi,
    input: &Path,
    output: &Path,
    options: ScriptRepairOptions,
) -> Result<()> {
    let project = create_project(api, "Script repair".into())?;
    eprintln!("Repair project retained: {}. Raw input unchanged; sanitized baseline and numerical edit have separate history. Script slopes do not establish motor safety.",project.project_id);
    let candidate = match api.execute(
        Command::ImportFunscriptForRepair {
            path: absolute_path(input)?,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        crate::motion::ResponseBody::Candidate(candidate) => candidate,
        _ => bail!("unexpected repair import response"),
    };
    if candidate.project_id != project.project_id || candidate.base_revision != project.revision {
        bail!("repair import returned mismatched scope or revision");
    }
    let repaired = repair_standard_script(&candidate.program, options)?;
    eprintln!("Numerical repair: {} interior actions removed, {} positions slope-clamped; policy={options:?}.",repaired.removed_jitters,repaired.clamped_positions);
    let committed = match api.execute(
        Command::CommitCandidate {
            candidate_id: candidate.candidate_id,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        crate::motion::ResponseBody::Project(project) => project,
        _ => bail!("unexpected baseline commit response"),
    };
    if committed.project_id != project.project_id
        || committed.revision != project.revision.checked_next()?
    {
        bail!("repair baseline commit returned mismatched scope or revision");
    }
    let label = format!("neutral-script-repair-v1 {options:?}");
    let candidate = api.upload_edit(
        committed.project_id.clone(),
        committed.revision,
        &repaired.program,
        &label,
    )?;
    modality::commit_and_export(api, &committed, candidate, output, Axis::Stroke)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destinations_preserve_default_names_and_refuse_existing_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("original.funscript");
        std::fs::write(&input, b"original").unwrap();
        assert_eq!(
            output_path(&input, None).unwrap(),
            dir.path()
                .canonicalize()
                .unwrap()
                .join("original.fixed.funscript")
        );
        assert!(output_path(&input, Some(&input)).is_err());
        let alias = dir.path().join("alias.funscript");
        std::fs::hard_link(&input, &alias).unwrap();
        assert!(output_path(&input, Some(&alias)).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("broken"))
                .unwrap();
            assert!(output_path(&input, Some(&dir.path().join("broken"))).is_err());
        }
        assert_eq!(std::fs::read(input).unwrap(), b"original");
    }
}
