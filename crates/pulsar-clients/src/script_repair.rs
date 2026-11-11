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
    let candidate = crate::import_funscript_for_repair(
        api,
        project.project_id.clone(),
        project.revision,
        absolute_path(input)?,
    )?;
    let repaired = repair_standard_script(&candidate.program, options)?;
    eprintln!("Numerical repair: {} interior actions removed, {} positions slope-clamped; policy={options:?}.",repaired.removed_jitters,repaired.clamped_positions);
    let committed = crate::commit_candidate(
        api,
        project.project_id.clone(),
        project.revision,
        candidate.candidate_id,
        None,
    )?;
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
