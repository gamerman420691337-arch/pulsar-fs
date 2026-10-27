//! First-party inference equivalence inspection and explicit owner decisions.
use crate::{
    cli::InferenceEquivalenceCommands,
    inference_equivalence_recovery as recovery,
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::*;
use std::path::Path;

/// Read a captured profile on a background thread. The returned typed value is
/// the exact request subject; neither its path nor metadata grants authority.
pub(crate) fn read_environment_profile(path: &Path) -> Result<DeclaredInferenceProfile> {
    use std::{fs, io::Read, path::Component};
    const LIMIT: u64 = 128 * 1024;
    ensure!(
        path.is_absolute()
            && path.to_str().is_some_and(|v| v.len() <= 4096)
            && !path.components().any(|c| matches!(c, Component::ParentDir)),
        "Environment profile requires a bounded absolute path without traversal"
    );
    let named = fs::symlink_metadata(path).context("Inspect environment profile")?;
    ensure!(
        named.is_file() && !named.file_type().is_symlink(),
        "Environment profile must be a regular file"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    let mut file = options.open(path).context("Open environment profile")?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && (1..=LIMIT).contains(&before.len()),
        "Environment profile must contain 1..128 KiB of regular-file JSON"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    let named_after = fs::symlink_metadata(path)?;
    ensure!(
        bytes.len() as u64 == before.len()
            && after.len() == before.len()
            && named_after.is_file()
            && !named_after.file_type().is_symlink()
            && named_after.len() == before.len()
            && before.modified().ok() == after.modified().ok(),
        "Environment profile changed during intake"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |m: &fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            )
        };
        ensure!(
            identity(&named) == identity(&before)
                && identity(&before) == identity(&after)
                && identity(&before) == identity(&named_after),
            "Environment profile identity changed during intake"
        );
    }
    let profile: DeclaredInferenceProfile =
        serde_json::from_slice(&bytes).context("Parse declared environment profile")?;
    profile.validate()?;
    Ok(profile)
}

/// Observe one explicit profile through the global owner-bound API. Returned
/// hardware/platform data remains report-only; no decision or retry is sent.
pub(crate) fn inspect_environment(
    client: &mut impl EngineApi,
    directory: &Path,
    profile: &DeclaredInferenceProfile,
) -> Result<InferenceEnvironmentReport> {
    profile.validate()?;
    let owner_proof =
        crate::project_package::read_local_owner_proof(directory, "model-pack-owner.token")?;
    let response = client.execute(
        Command::InspectInferenceEnvironment {
            profile: Box::new(profile.clone()),
            owner_proof,
        },
        None,
        None,
    )?;
    let ResponseBody::InferenceEnvironment(report) = response else {
        bail!("Engine returned an unexpected inference environment response");
    };
    report.validate_for_profile(profile)?;
    Ok(*report)
}

pub(crate) fn run(
    command: InferenceEquivalenceCommands,
    endpoint: &Path,
    bootstrap: &Path,
) -> Result<()> {
    let directory = bootstrap
        .parent()
        .context("Missing private Engine directory")?;
    let mut client = SessionClient::connect(endpoint, bootstrap)?;
    match command {
        InferenceEquivalenceCommands::Environment { profile } => {
            let profile = read_environment_profile(&profile)?;
            let report = inspect_environment(&mut client, directory, &profile)?;
            crate::print_json(&report)
        }
        InferenceEquivalenceCommands::Import {
            path,
            manifest_sha256,
            request_id,
        } => {
            let path = crate::absolute_path(&path)?
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Equivalence manifest path must be UTF-8"))?;
            let intent = recovery::Intent::import(path, manifest_sha256)?;
            crate::print_response(recovery::execute(
                directory,
                &mut client,
                RequestId::new(request_id)?,
                intent,
            )?)
        }
        InferenceEquivalenceCommands::Accept {
            dossier_id,
            expected_record_revision,
            operator,
            acknowledge_measured_equivalence,
            request_id,
        } => {
            ensure!(
                acknowledge_measured_equivalence,
                "Measured equivalence acceptance requires explicit acknowledgment"
            );
            crate::print_response(recovery::execute(
                directory,
                &mut client,
                RequestId::new(request_id)?,
                recovery::Intent::Accept {
                    dossier_id: ArtifactId::new(dossier_id)?,
                    expected_record_revision,
                    operator,
                },
            )?)
        }
        InferenceEquivalenceCommands::Withdraw {
            dossier_id,
            expected_record_revision,
            reason,
            request_id,
        } => crate::print_response(recovery::execute(
            directory,
            &mut client,
            RequestId::new(request_id)?,
            recovery::Intent::Withdraw {
                dossier_id: ArtifactId::new(dossier_id)?,
                expected_record_revision,
                reason,
            },
        )?),
        InferenceEquivalenceCommands::Status { dossier_id } => {
            let dossier_id = ArtifactId::new(dossier_id)?;
            validate_sha256(dossier_id.as_str())?;
            let owner_proof = crate::project_package::read_local_owner_proof(
                directory,
                "model-pack-owner.token",
            )?;
            let response = client.execute(
                Command::InferenceEquivalenceStatus {
                    dossier_id: dossier_id.clone(),
                    owner_proof,
                },
                None,
                None,
            )?;
            validate_status(&response, &dossier_id)?;
            crate::print_response(response)
        }
        InferenceEquivalenceCommands::List { after, limit } => {
            let after = after.map(ArtifactId::new).transpose()?;
            validate_inference_equivalence_page_limit(limit)?;
            if let Some(id) = &after {
                validate_sha256(id.as_str())?;
            }
            let owner_proof = crate::project_package::read_local_owner_proof(
                directory,
                "model-pack-owner.token",
            )?;
            let response = client.execute(
                Command::ListInferenceEquivalences {
                    after: after.clone(),
                    limit,
                    owner_proof,
                },
                None,
                None,
            )?;
            validate_page(&response, after.as_ref(), limit)?;
            crate::print_response(response)
        }
        InferenceEquivalenceCommands::RecoveryList => crate::print_json(&recovery::load(
            directory,
            client.session_id(),
            &client.recovery_credential_binding(),
        )?),
        InferenceEquivalenceCommands::RecoveryRetry { request_id } => crate::print_response(
            recovery::replay(directory, &mut client, &RequestId::new(request_id)?)?,
        ),
        InferenceEquivalenceCommands::RecoveryDismiss { request_id } => {
            let id = RequestId::new(request_id)?;
            recovery::dismiss(
                directory,
                client.session_id(),
                &client.recovery_credential_binding(),
                &id,
            )?;
            crate::print_json(
                &serde_json::json!({"request_id":id,"local_recovery_entry_dismissed":true,"engine_decision_unchanged":true}),
            )
        }
    }
}
fn validate_status(response: &ResponseBody, id: &ArtifactId) -> Result<()> {
    let ResponseBody::InferenceEquivalenceStatus(status) = response else {
        bail!("Engine returned an unexpected equivalence status response");
    };
    status.validate()?;
    ensure!(
        status.report.dossier_id == *id,
        "Equivalence status belongs to another dossier"
    );
    Ok(())
}
fn validate_page(response: &ResponseBody, after: Option<&ArtifactId>, limit: u16) -> Result<()> {
    validate_inference_equivalence_page_limit(limit)?;
    let ResponseBody::InferenceEquivalenceList(page) = response else {
        bail!("Engine returned an unexpected equivalence list response");
    };
    page.validate()?;
    ensure!(
        page.entries.len() <= limit as usize
            && page
                .entries
                .iter()
                .all(|e| after.is_none_or(|cursor| e.report.dossier_id.as_str() > cursor.as_str())),
        "Equivalence page differs from requested cursor or limit"
    );
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::inference_equivalence_recovery::tests::pending_report;
    fn status() -> InferenceEquivalenceStatus {
        InferenceEquivalenceStatus {
            report: pending_report(),
            withdrawal_pending: false,
            clock_floor_unix_ms: None,
            report_only: true,
        }
    }
    #[test]
    fn status_and_page_are_matched_to_requested_identity_and_bounds() {
        let entry = status();
        validate_status(
            &ResponseBody::InferenceEquivalenceStatus(Box::new(entry.clone())),
            &entry.report.dossier_id,
        )
        .unwrap();
        assert!(validate_status(
            &ResponseBody::InferenceEquivalenceStatus(Box::new(entry.clone())),
            &ArtifactId::new("f".repeat(64)).unwrap()
        )
        .is_err());
        let page = InferenceEquivalencePage {
            entries: vec![entry.clone()],
            next_after: Some(entry.report.dossier_id.clone()),
            report_only: true,
        };
        validate_page(
            &ResponseBody::InferenceEquivalenceList(Box::new(page.clone())),
            None,
            1,
        )
        .unwrap();
        assert!(validate_page(
            &ResponseBody::InferenceEquivalenceList(Box::new(page.clone())),
            Some(&entry.report.dossier_id),
            1
        )
        .is_err());
        // A byte-bounded page may continue before filling the requested count.
        validate_page(
            &ResponseBody::InferenceEquivalenceList(Box::new(page.clone())),
            None,
            2,
        )
        .unwrap();
        assert!(validate_page(
            &ResponseBody::InferenceEquivalenceList(Box::new(page)),
            None,
            0
        )
        .is_err());
    }
}

#[cfg(test)]
pub(crate) mod environment_tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn artifact(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }

    fn platform() -> InferencePlatformObservation {
        InferencePlatformObservation {
            schema_version: 1,
            os: "linux".into(),
            architecture: "x86_64".into(),
            kernel_release: "6.12.0-fixture".into(),
            kernel_version: "#1 SMP test observation".into(),
        }
    }

    fn model() -> InferenceCpuModelObservation {
        InferenceCpuModelObservation {
            vendor_id: "GenuineIntel".into(),
            family: 6,
            model: 85,
            model_name: "Fixture CPU".into(),
            stepping: 4,
            microcode: 0x2000065,
            flags: vec!["avx".into(), "sse2".into()],
        }
    }

    fn hardware() -> InferenceHardwareObservation {
        InferenceHardwareObservation {
            schema_version: 1,
            architecture: "x86_64".into(),
            cpu_models: vec![model()],
            logical_cpus: vec![
                InferenceLogicalCpuObservation {
                    processor_id: 0,
                    package_id: 0,
                    core_id: 0,
                    model_index: 0,
                },
                InferenceLogicalCpuObservation {
                    processor_id: 1,
                    package_id: 0,
                    core_id: 1,
                    model_index: 0,
                },
            ],
            cpu_packages: vec![InferenceCpuPackageObservation {
                package_id: 0,
                reported_logical_cpus: 4,
                reported_physical_cores: 2,
            }],
            allowed_cpu_ids: vec![0, 1],
            physical_memory_bytes: 16 << 30,
            gpu: None,
        }
    }

    pub(crate) fn profile() -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact(b"declared model, not installed"),
            runtime: artifact(b"observed fixture runtime"),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 320,
                    height: 180,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }

    pub(crate) fn report() -> InferenceEnvironmentReport {
        let declared_profile = profile();
        let platform = platform();
        let hardware = hardware();
        InferenceEnvironmentReport {
            schema_version: 1,
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&declared_profile).unwrap())
            ),
            environment: InferenceEquivalenceEnvironment {
                engine: artifact(b"fixture engine"),
                worker: artifact(b"fixture worker"),
                os: "linux".into(),
                architecture: "x86_64".into(),
                platform_build: platform.identity().unwrap(),
                hardware: hardware.identity().unwrap(),
                dependencies: vec![declared_profile.runtime.clone()],
            },
            declared_profile,
            platform,
            hardware,
            observed_at_unix_ms: 1,
            runtime_dependency_closure_verified: false,
            qualified: false,
            report_only: true,
        }
    }

    #[test]
    fn environment_profile_intake_accepts_exact_bounded_json_and_refuses_bad_paths() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.json");
        let expected = profile();
        let mut bytes = serde_json::to_vec(&expected).unwrap();
        bytes.resize(128 * 1024, b' ');
        std::fs::write(&path, &bytes).unwrap();
        let loaded = read_environment_profile(&path).unwrap();
        assert_eq!(
            serde_json::to_vec(&loaded).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        bytes.push(b' ');
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_environment_profile(&path).is_err());
        assert!(read_environment_profile(directory.path()).is_err());
        assert!(read_environment_profile(Path::new("profile.json")).is_err());
        let parent = directory
            .path()
            .join("..")
            .join(directory.path().file_name().unwrap())
            .join("profile.json");
        assert!(read_environment_profile(&parent).is_err());
    }

    #[test]
    fn environment_profile_intake_refuses_duplicate_fields_and_invalid_contracts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.json");
        let valid = serde_json::to_string(&profile()).unwrap();
        let duplicate = valid.replacen('{', "{\"schema_version\":1,", 1);
        std::fs::write(&path, duplicate).unwrap();
        assert!(read_environment_profile(&path).is_err());
        let mut invalid = profile();
        invalid.cpu_threads = 0;
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(read_environment_profile(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn environment_profile_intake_refuses_leaf_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile.json");
        std::fs::write(&path, serde_json::to_vec(&profile()).unwrap()).unwrap();
        let alias = directory.path().join("alias.json");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(read_environment_profile(&alias).is_err());
    }

    #[cfg(unix)]
    fn owner_directory() -> tempfile::TempDir {
        use std::{
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.path().join("model-pack-owner.token"))
            .unwrap();
        file.write_all("a".repeat(64).as_bytes()).unwrap();
        directory
    }

    #[cfg(unix)]
    struct EnvironmentApi {
        response: Option<ResponseBody>,
        calls: usize,
    }
    #[cfg(unix)]
    impl EngineApi for EnvironmentApi {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls += 1;
            ensure!(
                project.is_none() && revision.is_none(),
                "unexpected project scope"
            );
            let Command::InspectInferenceEnvironment {
                profile: actual,
                owner_proof,
            } = command
            else {
                bail!("unexpected command")
            };
            ensure!(owner_proof == "a".repeat(64), "unexpected fixture proof");
            ensure!(
                serde_json::to_vec(actual.as_ref())? == serde_json::to_vec(&profile())?,
                "unexpected profile"
            );
            self.response.take().context("unexpected additional call")
        }
    }

    #[cfg(unix)]
    #[test]
    fn environment_inspection_sends_one_exact_global_request_and_checks_response_binding() {
        let directory = owner_directory();
        let mut api = EnvironmentApi {
            response: Some(ResponseBody::InferenceEnvironment(Box::new(report()))),
            calls: 0,
        };
        let observed = inspect_environment(&mut api, directory.path(), &profile()).unwrap();
        observed.validate_for_profile(&profile()).unwrap();
        assert_eq!(api.calls, 1);
        assert!(
            observed.report_only
                && !observed.qualified
                && !observed.runtime_dependency_closure_verified
        );

        let mut different = report();
        different.declared_profile.minimum_memory_bytes += 1;
        different.profile_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&different.declared_profile).unwrap())
        );
        different.validate().unwrap();
        let mut api = EnvironmentApi {
            response: Some(ResponseBody::InferenceEnvironment(Box::new(different))),
            calls: 0,
        };
        assert!(inspect_environment(&mut api, directory.path(), &profile()).is_err());
        assert_eq!(api.calls, 1);
    }

    #[cfg(unix)]
    #[test]
    fn environment_inspection_refuses_authority_claims_unexpected_response_and_missing_owner() {
        let directory = owner_directory();
        let mut claimed = report();
        claimed.qualified = true;
        let mut api = EnvironmentApi {
            response: Some(ResponseBody::InferenceEnvironment(Box::new(claimed))),
            calls: 0,
        };
        assert!(inspect_environment(&mut api, directory.path(), &profile()).is_err());
        assert_eq!(api.calls, 1);
        let mut api = EnvironmentApi {
            response: Some(ResponseBody::InferenceEquivalenceList(Box::new(
                InferenceEquivalencePage {
                    entries: vec![],
                    next_after: None,
                    report_only: true,
                },
            ))),
            calls: 0,
        };
        assert!(inspect_environment(&mut api, directory.path(), &profile()).is_err());
        assert_eq!(api.calls, 1);
        let missing = tempfile::tempdir().unwrap();
        assert!(inspect_environment(&mut api, missing.path(), &profile()).is_err());
        assert_eq!(api.calls, 1);
    }
}
