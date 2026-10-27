use super::*;
use crate::cli::WasmEditCommands;
use std::io::Read;

fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    anyhow::ensure!(
        std::fs::metadata(path)?.is_file(),
        "WASM declarations must be regular files"
    );
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "WASM declaration changed file type"
    );
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "WASM declaration exceeds byte admission"
    );
    Ok(bytes)
}
pub(super) fn run(client: &mut impl EngineApi, command: WasmEditCommands) -> Result<()> {
    let response = match command {
        WasmEditCommands::ResolveStart {
            project,
            reference,
            request_id,
        } => {
            let reference: WasmStartReference =
                serde_json::from_slice(&read(&reference, 16 * 1024)?)?;
            reference.validate()?;
            client.execute_identified(
                RequestId::new(request_id)?,
                Command::ResolveWasmStart { reference },
                Some(ProjectId::new(project)?),
                None,
            )?
        }
        WasmEditCommands::Info { project } => client.execute(
            Command::WasmEditorInfo,
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::ListArchived {
            project,
            after,
            limit,
        } => client.execute(
            Command::ListArchivedWasmEdits {
                after: after.map(AttemptId::new).transpose()?,
                limit,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::List { project } => {
            client.execute(Command::ListWasmEdits, Some(ProjectId::new(project)?), None)?
        }
        WasmEditCommands::ArchiveCompleted {
            project,
            attempt,
            original_request_id,
            request_id,
        } => client.execute_identified(
            RequestId::new(request_id)?,
            Command::ArchiveCompletedWasmEdit {
                attempt_id: AttemptId::new(attempt)?,
                original_request_id: RequestId::new(original_request_id)?,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::RemoveUnsuccessful {
            project,
            attempt,
            original_request_id,
            expected_state,
            request_id,
        } => client.execute_identified(
            request_id
                .map(RequestId::new)
                .transpose()?
                .unwrap_or_else(session::new_request_id),
            Command::RemoveUnsuccessfulWasmEdit {
                attempt_id: AttemptId::new(attempt)?,
                original_request_id: RequestId::new(original_request_id)?,
                expected_state: match expected_state.as_str() {
                    "failed" => WasmEditState::Failed,
                    "cancelled" => WasmEditState::Cancelled,
                    "interrupted" => WasmEditState::Interrupted,
                    _ => bail!("Unsuccessful terminal state required"),
                },
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::Status { project, attempt } => client.execute(
            Command::WasmEditStatus {
                attempt_id: AttemptId::new(attempt)?,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::Cancel {
            project,
            attempt,
            request_id,
        } => client.execute_identified(
            request_id
                .map(RequestId::new)
                .transpose()?
                .unwrap_or_else(session::new_request_id),
            Command::CancelWasmEdit {
                attempt_id: AttemptId::new(attempt)?,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        WasmEditCommands::Start {
            project,
            module,
            manifest,
            revision,
            worker_sha256,
            approve_execution,
            guest_memory_bytes,
            label,
            request_id,
        } => {
            validate_sha256(&worker_sha256)?;
            let project = ProjectId::new(project)?;
            let ResponseBody::WasmEditorInfo(info) =
                client.execute(Command::WasmEditorInfo, Some(project.clone()), None)?
            else {
                bail!("Unexpected WASM editor information");
            };
            anyhow::ensure!(
                info.supported,
                "WASM editing is unavailable in this engine/confinement configuration"
            );
            anyhow::ensure!(
                info.worker.sha256 == worker_sha256,
                "Native worker differs from the explicitly selected build"
            );
            let manifest: ExtensionManifest = serde_json::from_slice(&read(&manifest, 16 * 1024)?)?;
            let spec = WasmEditSpec {
                manifest,
                module: read(&module, WASM_MODULE_MAX_BYTES)?,
                guest_memory_bytes,
                expected_worker: info.worker,
                approve_execution,
                label,
            };
            spec.validate()?;
            let request_id = request_id
                .map(RequestId::new)
                .transpose()?
                .unwrap_or_else(session::new_request_id);
            eprintln!("WASM edit request: {request_id}");
            client.execute_identified(
                request_id,
                Command::StartWasmEdit { spec },
                Some(project),
                Some(RevisionId::new(revision)),
            )?
        }
    };
    print_response(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn cli_requires_revision_worker_identity_and_execution_approval() {
        let args = [
            "pulsar",
            "wasm-edit",
            "start",
            "project",
            "module.wasm",
            "--manifest",
            "manifest.json",
            "--revision",
            "0",
            "--worker-sha256",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--approve-execution",
        ];
        crate::cli::Cli::try_parse_from(args).unwrap();
        assert!(crate::cli::Cli::try_parse_from(&args[..args.len() - 1]).is_err());
        let missing_revision: Vec<_> = args
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, v)| (![7, 8].contains(&i)).then_some(v))
            .collect();
        assert!(crate::cli::Cli::try_parse_from(missing_revision).is_err());
        let missing_worker: Vec<_> = args
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(i, v)| (![9, 10].contains(&i)).then_some(v))
            .collect();
        assert!(crate::cli::Cli::try_parse_from(missing_worker).is_err());
    }
    #[test]
    fn cleanup_cli_requires_original_request_and_unsuccessful_state() {
        let args = [
            "pulsar",
            "wasm-edit",
            "remove-unsuccessful",
            "project",
            "attempt",
            "--original-request-id",
            "start",
            "--expected-state",
            "failed",
        ];
        crate::cli::Cli::try_parse_from(args).unwrap();
        assert!(crate::cli::Cli::try_parse_from(&args[..7]).is_err());
        let mut bad = args;
        bad[8] = "completed";
        assert!(crate::cli::Cli::try_parse_from(bad).is_err());
        crate::cli::Cli::try_parse_from(["pulsar", "wasm-edit", "list", "project"]).unwrap();
    }
    #[test]
    fn completed_archival_cli_requires_both_original_and_replay_request_identity() {
        use clap::Parser;
        let args = [
            "pulsar",
            "wasm-edit",
            "archive-completed",
            "project",
            "attempt",
            "--original-request-id",
            "start",
            "--request-id",
            "archive",
        ];
        crate::cli::Cli::try_parse_from(args).unwrap();
        assert!(crate::cli::Cli::try_parse_from(&args[..7]).is_err());
        assert!(crate::cli::Cli::try_parse_from(&args[..5]).is_err());
    }
    #[test]
    fn declarations_are_bounded_without_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("module");
        std::fs::write(&path, [1, 2, 3, 4]).unwrap();
        assert_eq!(read(&path, 4).unwrap(), [1, 2, 3, 4]);
        assert!(read(&path, 3).is_err());
        assert!(read(dir.path(), 4).is_err());
    }
}
