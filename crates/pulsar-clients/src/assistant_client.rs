//! First-party proposal controls and explicit generation approval; no inference libraries.
use super::*;
use crate::cli::AssistantCommands;
use pulsar_protocol::{AssistantStart, SessionId};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::io::{Read, Write};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    pub(crate) schema_version: u16,
    pub(crate) endpoint: PathBuf,
    pub(crate) session: SessionId,
    pub(crate) credential_binding: String,
    pub(crate) request_id: RequestId,
    pub(crate) project: ProjectId,
    pub(crate) revision: RevisionId,
    pub(crate) spec: AssistantStart,
}
impl Intent {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == 1
                && self.endpoint.is_absolute()
                && self.credential_binding.len() == 64
                && self
                    .credential_binding
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid assistant intent binding"
        );
        self.spec.validate()?;
        if let Some(image) = &self.spec.image {
            anyhow::ensure!(
                image.project_id == self.project
                    && image.session_id == self.session
                    && image.revision == self.revision,
                "prepared image differs from saved intent namespace"
            );
        }
        Ok(())
    }
}
/// Check draft invariants before decoding. Only Engine-issued contexts can be saved.
pub(crate) fn validate_preparation(
    spec: &AssistantStart,
    image_source: Option<&SourceVersionId>,
) -> Result<()> {
    spec.profile.validate()?;
    anyhow::ensure!(spec.image.is_none(), "preparation requires a fresh draft");
    anyhow::ensure!(
        spec.profile.vision_projector.is_some() == image_source.is_some(),
        "vision profile requires an image source; text profile does not accept one"
    );
    if image_source.is_some() {
        anyhow::ensure!(
            spec.creative_patterns.is_some(),
            "image theme requires creative scope"
        );
    }
    let mut text = spec.clone();
    text.profile.vision_projector = None;
    text.validate()?;
    Ok(())
}
pub(crate) fn prepare_start(
    client: &mut impl EngineApi,
    session: &SessionId,
    project: &ProjectId,
    revision: RevisionId,
    mut spec: AssistantStart,
    image_source: Option<&SourceVersionId>,
) -> Result<AssistantStart> {
    validate_preparation(&spec, image_source)?;
    if let Some(source) = image_source {
        let ResponseBody::AssistantImage(image) = client.execute(
            Command::PrepareAssistantImage {
                source_version: source.clone(),
            },
            Some(project.clone()),
            Some(revision),
        )?
        else {
            bail!("unexpected prepared assistant image response")
        };
        image.validate()?;
        anyhow::ensure!(
            image.project_id == *project
                && image.session_id == *session
                && image.revision == revision
                && image.source_version == *source,
            "prepared image differs from requested namespace or source"
        );
        spec.image = Some(*image);
    }
    spec.validate()?;
    Ok(spec)
}
pub(crate) fn save(path: &Path, intent: &Intent) -> Result<()> {
    intent.validate()?;
    save_private(path, intent)
}
pub(crate) fn save_private<T: Serialize>(path: &Path, intent: &T) -> Result<()> {
    anyhow::ensure!(
        path.is_absolute(),
        "assistant intent output must be absolute"
    );
    let bytes = serde_json::to_vec_pretty(intent)?;
    anyhow::ensure!(
        bytes.len() <= 32768,
        "assistant intent exceeds capture bound"
    );
    #[cfg(not(unix))]
    {
        let _ = bytes;
        bail!("private assistant intent publication unavailable on this platform");
    }
    #[cfg(unix)]
    {
        use rustix::fs::{open, openat, Mode, OFlags};
        use std::os::unix::fs::MetadataExt;
        let parent = path.parent().context("missing intent parent")?;
        let directory = std::fs::File::from(open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = directory.metadata()?;
        anyhow::ensure!(
            metadata.is_dir()
                && metadata.uid() == rustix::process::geteuid().as_raw()
                && metadata.mode() & 0o022 == 0,
            "assistant intent parent must be owned and not writable by other users"
        );
        let mut file = std::fs::File::from(openat(
            &directory,
            path.file_name().context("intent filename missing")?,
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        // Even an uncertain write/sync outcome leaves only this newly created private
        // file. Never overwrite or automatically delete a user's existing path.
        file.write_all(&bytes)?;
        file.sync_all()?;
        directory.sync_all()?;
        Ok(())
    }
}
pub(crate) fn load(path: &Path) -> Result<Intent> {
    let intent: Intent = load_private(path)?;
    intent.validate()?;
    Ok(intent)
}
pub(crate) fn load_private<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    anyhow::ensure!(
        path.is_absolute(),
        "assistant intent input must be absolute"
    );
    #[cfg(not(unix))]
    {
        bail!("private assistant intent reading unavailable on this platform");
    }
    #[cfg(unix)]
    {
        use rustix::fs::{open, Mode, OFlags};
        use std::os::unix::fs::MetadataExt;
        let mut file = std::fs::File::from(open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let before = file.metadata()?;
        anyhow::ensure!(
            before.is_file()
                && before.uid() == rustix::process::geteuid().as_raw()
                && before.mode() & 0o077 == 0
                && before.nlink() == 1
                && before.len() <= 32768,
            "unsafe or excessive assistant intent"
        );
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(32769)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        anyhow::ensure!(
            bytes.len() <= 32768
                && before.len() == after.len()
                && before.mode() == after.mode()
                && before.nlink() == after.nlink()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec(),
            "assistant intent changed during capture"
        );
        Ok(serde_json::from_slice(&bytes)?)
    }
}
pub(super) fn run(
    client: &mut SessionClient,
    endpoint: &Path,
    command: AssistantCommands,
) -> Result<()> {
    let session = client.session_id().clone();
    let binding = client.recovery_credential_binding();
    run_bound(client, endpoint, &session, &binding, command)
}
fn run_bound(
    client: &mut impl EngineApi,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    command: AssistantCommands,
) -> Result<()> {
    let response = match command {
        AssistantCommands::ApplySavedEdit {
            intent,
            approve_edit,
        } => crate::assistant_edit_intent::dispatch(
            client,
            endpoint,
            session,
            binding,
            &intent,
            None,
            approve_edit,
        )?,
        AssistantCommands::PrepareEdit {
            project,
            attempt,
            revision,
            request_id,
            output,
        } => {
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                bail!("expected edit proposal");
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "edit proposal belongs to another owner/project/revision"
            );
            let ResponseBody::AssistantEditRecipes(recipes) =
                client.execute(Command::AssistantEditRecipes, Some(project), None)?
            else {
                bail!("expected current recipes");
            };
            let Some(AssistantProposal::ApplyEditRecipe { recipe_id }) = &report.proposal else {
                bail!("completed edit proposal required");
            };
            let recipe = recipes
                .into_iter()
                .find(|r| &r.recipe_id == recipe_id)
                .context("selected edit recipe is absent")?;
            let intent = crate::assistant_edit_intent::EditIntent::from_report(
                endpoint,
                session,
                binding,
                RequestId::new(request_id)?,
                &report,
                recipe,
            )?;
            crate::assistant_edit_intent::save(&output, &intent)?;
            println!(
                "Saved edit request: {} (request {}, {:?} {:?})",
                output.display(),
                intent.request_id,
                intent.recipe.operation,
                intent.recipe.axis
            );
            return Ok(());
        }
        AssistantCommands::Recipes { project } => client.execute(
            Command::AssistantEditRecipes,
            Some(ProjectId::new(project)?),
            None,
        )?,
        AssistantCommands::ApplyEdit {
            project,
            attempt,
            revision,
            request_id,
            approve_edit,
        } => {
            anyhow::ensure!(approve_edit, "explicit edit approval required");
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                bail!("expected assistant proposal");
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "edit proposal belongs to another owner/project/revision"
            );
            let approval = AssistantEditApproval::from_report(&report)?;
            client.execute_identified(
                RequestId::new(request_id)?,
                Command::ApplyAssistantEdit { approval },
                Some(project),
                Some(report.base_revision),
            )?
        }
        AssistantCommands::Targets { project } => client.execute(
            Command::AssistantTargets,
            Some(ProjectId::new(project)?),
            None,
        )?,
        AssistantCommands::Profiles { project } => client.execute(
            Command::AssistantProfiles,
            Some(ProjectId::new(project)?),
            None,
        )?,
        AssistantCommands::Prepare {
            image_source,
            project,
            profile,
            revision,
            prompt,
            creative_scope,
            edit_focus,
            target_focus,
            review_focus,
            request_id,
            output,
        } => {
            let creative_patterns = creative_scope
                .as_deref()
                .map(|path| -> Result<pulsar_protocol::CreativePatternScope> {
                    let scope: pulsar_protocol::CreativePatternScope =
                        serde_json::from_slice(&crate::inference::read_bounded_profile(path)?)?;
                    scope.validate()?;
                    Ok(scope)
                })
                .transpose()?;
            let project = ProjectId::new(project)?;
            let ResponseBody::AssistantProfiles(profiles) =
                client.execute(Command::AssistantProfiles, Some(project.clone()), None)?
            else {
                bail!("unexpected assistant profile response");
            };
            let profile = profiles
                .into_iter()
                .find(|p| p.profile_id.as_str() == profile)
                .context("selected profile is not currently owner registered")?;
            let image_source = image_source.map(SourceVersionId::new).transpose()?;
            let spec = prepare_start(
                client,
                session,
                &project,
                RevisionId::new(revision),
                AssistantStart {
                    image: None,
                    creative_patterns,
                    edit_focus,
                    target_focus,
                    review_focus,
                    profile,
                    user_request: prompt,
                },
                image_source.as_ref(),
            )?;
            let intent = Intent {
                schema_version: 1,
                endpoint: endpoint.to_path_buf(),
                session: session.clone(),
                credential_binding: binding.into(),
                request_id: RequestId::new(request_id)?,
                project,
                revision: RevisionId::new(revision),
                spec,
            };
            save(&output, &intent)?;
            println!(
                "Saved assistant start intent: {} (request {})",
                output.display(),
                intent.request_id
            );
            return Ok(());
        }
        AssistantCommands::Explain {
            project,
            attempt,
            revision,
        } => {
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                anyhow::bail!("expected assistant report")
            };
            println!(
                "{}",
                recorded_review_explanation(
                    client,
                    session,
                    &project,
                    RevisionId::new(revision),
                    &report
                )?
            );
            return Ok(());
        }
        AssistantCommands::Start {
            intent,
            approve_inference,
        } => execute_saved(
            client,
            endpoint,
            session,
            binding,
            &intent,
            approve_inference,
        )?,
        AssistantCommands::GenerateSaved {
            intent,
            approve_generation,
        } => crate::assistant_generation_intent::dispatch(
            client,
            endpoint,
            session,
            binding,
            &intent,
            None,
            approve_generation,
        )?,
        AssistantCommands::PrepareGeneration {
            project,
            attempt,
            source,
            preset,
            revision,
            request_id,
            settings,
            model,
            output,
        } => {
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                anyhow::bail!("expected retained assistant report")
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "assistant proposal project, owner or revision differs"
            );
            let settings = settings
                .as_deref()
                .map(|path| -> Result<GenerationSettings> {
                    Ok(serde_json::from_slice(
                        &crate::inference::read_bounded_profile(path)?,
                    )?)
                })
                .transpose()?
                .unwrap_or_default();
            let command = approved_generation_command(
                &report,
                SourceVersionId::new(source)?,
                preset,
                settings,
                model,
                true,
            )?;
            let intent = crate::assistant_generation_intent::GenerationIntent {
                proposed_patterns: None,
                schema_version: 1,
                endpoint: endpoint.to_path_buf(),
                session: session.clone(),
                credential_binding: binding.into(),
                request_id: RequestId::new(request_id)?,
                project,
                revision: RevisionId::new(revision),
                proposed_stride: match &report.proposal {
                    Some(AssistantProposal::SetSampleStride { stride }) => Some(*stride),
                    _ => None,
                },
                proposed_target: match &report.proposal {
                    Some(AssistantProposal::SelectTarget { target_id }) => Some(target_id.clone()),
                    _ => None,
                },
                command,
            };
            crate::assistant_generation_intent::save(&output, &intent)?;
            println!(
                "Saved generation request: {} (request {})",
                output.display(),
                intent.request_id
            );
            return Ok(());
        }
        AssistantCommands::PrepareCreativeGeneration {
            project,
            attempt,
            revision,
            request_id,
            output,
        } => {
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                bail!("expected retained creative report");
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "creative proposal project, owner or revision differs"
            );
            let approval = pulsar_protocol::AssistantCreativeApproval::from_report(&report)?;
            let Some(AssistantProposal::SynthesizePatterns { patterns }) = report.proposal else {
                bail!("synthetic proposal required");
            };
            let intent = crate::assistant_generation_intent::GenerationIntent {
                schema_version: 1,
                endpoint: endpoint.to_path_buf(),
                session: session.clone(),
                credential_binding: binding.into(),
                request_id: RequestId::new(request_id)?,
                project,
                revision: RevisionId::new(revision),
                command: Command::GenerateCreativeWithAssistant { approval },
                proposed_stride: None,
                proposed_target: None,
                proposed_patterns: Some(patterns),
            };
            crate::assistant_generation_intent::save(&output, &intent)?;
            println!(
                "Saved creative generation intent: {} (request {})",
                output.display(),
                intent.request_id
            );
            return Ok(());
        }
        AssistantCommands::CreativeCandidate {
            project,
            attempt,
            revision,
            request_id,
            approve_generation,
        } => {
            anyhow::ensure!(
                approve_generation,
                "explicit creative generation approval required"
            );
            let project = ProjectId::new(project)?;
            let ResponseBody::Assistant(report) = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?
            else {
                bail!("expected retained creative report");
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "creative proposal project, owner or revision differs"
            );
            let approval = pulsar_protocol::AssistantCreativeApproval::from_report(&report)?;
            client.execute_identified(
                RequestId::new(request_id)?,
                Command::GenerateCreativeWithAssistant { approval },
                Some(project),
                Some(report.base_revision),
            )?
        }
        AssistantCommands::Generate {
            project,
            attempt,
            source,
            preset,
            revision,
            request_id,
            settings,
            model,
            approve_generation,
        } => {
            anyhow::ensure!(approve_generation, "explicit generation approval required");
            let project = ProjectId::new(project)?;
            let body = client.execute(
                Command::AssistantStatus {
                    attempt_id: AttemptId::new(attempt)?,
                },
                Some(project.clone()),
                None,
            )?;
            let ResponseBody::Assistant(report) = body else {
                anyhow::bail!("expected retained assistant report")
            };
            anyhow::ensure!(
                report.project_id == project
                    && report.session_id == *session
                    && report.base_revision == RevisionId::new(revision),
                "assistant proposal project, owner or revision differs"
            );
            let settings = settings
                .as_deref()
                .map(|path| -> Result<GenerationSettings> {
                    Ok(serde_json::from_slice(
                        &crate::inference::read_bounded_profile(path)?,
                    )?)
                })
                .transpose()?
                .unwrap_or_default();
            let command = approved_generation_command(
                &report,
                SourceVersionId::new(source)?,
                preset,
                settings,
                model,
                approve_generation,
            )?;
            client.execute_identified(
                RequestId::new(request_id)?,
                command,
                Some(project),
                Some(RevisionId::new(revision)),
            )?
        }
        AssistantCommands::Resolve {
            intent,
            acknowledge_withdrawing_unpublished,
        } => {
            anyhow::ensure!(
                acknowledge_withdrawing_unpublished,
                "explicit withdrawal acknowledgement required"
            );
            resolve_intent(client, endpoint, session, binding, load(&intent)?)?
        }
        AssistantCommands::Status { project, attempt } => client.execute(
            Command::AssistantStatus {
                attempt_id: AttemptId::new(attempt)?,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
        AssistantCommands::List { project } => client.execute(
            Command::ListAssistantAttempts,
            Some(ProjectId::new(project)?),
            None,
        )?,
        AssistantCommands::Cancel {
            project,
            attempt,
            request_id,
        } => client.execute_identified(
            RequestId::new(request_id)?,
            Command::CancelAssistant {
                attempt_id: AttemptId::new(attempt)?,
            },
            Some(ProjectId::new(project)?),
            None,
        )?,
    };
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
/// Dispatch only the durable exact request; approval and namespace are checked
/// independently of any disposable presentation state.
pub(crate) fn execute_saved(
    client: &mut impl EngineApi,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    path: &Path,
    approved: bool,
) -> Result<ResponseBody> {
    anyhow::ensure!(approved, "explicit inference approval required");
    execute_intent(client, endpoint, session, binding, load(path)?, approved)
}

pub(crate) fn execute_intent(
    client: &mut impl EngineApi,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: Intent,
    approved: bool,
) -> Result<ResponseBody> {
    anyhow::ensure!(approved, "explicit inference approval required");
    intent.validate()?;
    anyhow::ensure!(
        intent.endpoint == endpoint
            && &intent.session == session
            && intent.credential_binding == binding,
        "assistant intent belongs to another endpoint/session/credential namespace"
    );
    client.execute_identified(
        intent.request_id,
        Command::StartAssistant { spec: intent.spec },
        Some(intent.project),
        Some(intent.revision),
    )
}

pub(crate) fn resolve_intent(
    client: &mut impl EngineApi,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: Intent,
) -> Result<ResponseBody> {
    intent.validate()?;
    anyhow::ensure!(
        intent.endpoint == endpoint
            && &intent.session == session
            && intent.credential_binding == binding,
        "assistant intent belongs to another endpoint/session/credential namespace"
    );
    let reference = AssistantStartReference {
        original_request_id: intent.request_id,
        base_revision: intent.revision,
        spec: intent.spec,
    };
    let request = reference.resolution_request_id(&intent.project)?;
    client.execute_identified(
        request,
        Command::ResolveAssistantStart { reference },
        Some(intent.project),
        None,
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn intent() -> Intent {
        let artifact = pulsar_protocol::ArtifactIdentity {
            artifact_id: pulsar_protocol::ArtifactId::new("fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        Intent {
            schema_version: 1,
            endpoint: PathBuf::from("/private/engine.sock"),
            session: SessionId::new("session").unwrap(),
            credential_binding: "b".repeat(64),
            request_id: RequestId::new("original").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(7),
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: pulsar_protocol::AssistantProfile {
                    vision_projector: None,
                    profile_id: AttemptId::new("profile").unwrap(),
                    worker: artifact.clone(),
                    runtime: artifact.clone(),
                    model: artifact,
                    cpu_threads: 1,
                    wall_time_ms: 1000,
                    worker_memory_bytes: 512 * 1024 * 1024,
                },
                user_request: "stride 8".into(),
            },
        }
    }
    #[test]
    fn intent_is_private_exact_and_never_overwrites_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("intent");
        let i = intent();
        save(&p, &i).unwrap();
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let loaded = load(&p).unwrap();
        assert_eq!(loaded.request_id, i.request_id);
        assert_eq!(loaded.spec.profile, i.spec.profile);
        let before = std::fs::read(&p).unwrap();
        assert!(save(&p, &i).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), before);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&p).is_err());
    }
    #[test]
    fn intent_refuses_symlink_hardlink_unknown_fields_and_changed_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("intent");
        save(&p, &intent()).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&p, &alias).unwrap();
        assert!(load(&alias).is_err());
        let linked = dir.path().join("linked");
        std::fs::hard_link(&p, &linked).unwrap();
        assert!(load(&p).is_err());
        let mut raw = serde_json::to_value(intent()).unwrap();
        raw["auth_token"] = "invented".into();
        assert!(serde_json::from_value::<Intent>(raw).is_err());
    }
    #[test]
    fn approved_generation_requires_separate_approval_and_completed_exact_proposal() {
        let intent = intent();
        let mut report = AssistantReport {
            image: None,
            attempt_id: AttemptId::new("attempt").unwrap(),
            project_id: intent.project,
            session_id: intent.session,
            request_id: intent.request_id,
            base_revision: intent.revision,
            engine_epoch: AttemptId::new("epoch").unwrap(),
            profile: intent.spec.profile,
            context_sha256: "c".repeat(64),
            scope: AssistantProposalScope {
                allowed_sample_strides: vec![8],
                ..Default::default()
            },
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::SetSampleStride { stride: 8 }),
            error: None,
            worker_peak_memory_bytes: Some(1),
        };
        let build = |r: &AssistantReport, approved| {
            approved_generation_command(
                r,
                SourceVersionId::new("source").unwrap(),
                "fast".into(),
                GenerationSettings::default(),
                None,
                approved,
            )
        };
        assert!(build(&report, false).is_err());
        let Command::GenerateWithAssistant {
            approval,
            source_version,
            settings,
            ..
        } = build(&report, true).unwrap()
        else {
            panic!("approved request")
        };
        assert_eq!(
            approval.proposal_sha256,
            AssistantGenerationApproval::from_report(&report)
                .unwrap()
                .proposal_sha256
        );
        assert_eq!(source_version.as_str(), "source");
        assert_eq!(settings.source_sample_stride, 1); // Engine derives 8 from the retained report.
        report.state = AssistantState::Running;
        report.proposal = None;
        report.worker_peak_memory_bytes = None;
        assert!(build(&report, true).is_err());
    }
    #[derive(Default)]
    struct Recording {
        calls: Vec<(RequestId, Command, Option<ProjectId>, Option<RevisionId>)>,
    }
    impl EngineApi for Recording {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            bail!("unexpected discovery on exact start")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls.push((id, command, project, revision));
            bail!("simulated acknowledgement loss")
        }
    }
    #[test]
    fn exact_retry_preserves_identity_and_changed_session_never_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("intent");
        let original = intent();
        save(&path, &original).unwrap();
        let mut client = Recording::default();
        for _ in 0..2 {
            assert!(run_bound(
                &mut client,
                &original.endpoint,
                &original.session,
                &original.credential_binding,
                AssistantCommands::Start {
                    intent: path.clone(),
                    approve_inference: true
                }
            )
            .is_err());
        }
        assert_eq!(client.calls.len(), 2);
        for (id, command, project, revision) in &client.calls {
            assert_eq!(id, &original.request_id);
            assert_eq!(project.as_ref(), Some(&original.project));
            assert_eq!(revision.as_ref(), Some(&original.revision));
            let Command::StartAssistant { spec } = command else {
                panic!("start")
            };
            assert_eq!(spec.profile, original.spec.profile);
            assert_eq!(spec.user_request, original.spec.user_request);
        }
        for (session, binding) in [
            (
                SessionId::new("different").unwrap(),
                original.credential_binding.clone(),
            ),
            (original.session.clone(), "c".repeat(64)),
        ] {
            assert!(run_bound(
                &mut client,
                &original.endpoint,
                &session,
                &binding,
                AssistantCommands::Start {
                    intent: path.clone(),
                    approve_inference: true
                }
            )
            .is_err());
        }
        assert!(run_bound(
            &mut client,
            &original.endpoint,
            &original.session,
            &original.credential_binding,
            AssistantCommands::Start {
                intent: path,
                approve_inference: false
            }
        )
        .is_err());
        assert_eq!(client.calls.len(), 2);
    }
    #[test]
    fn recorded_review_explanation_preserves_evidence_and_refuses_stale_or_changed_records() {
        struct Reader {
            status: ReviewStatus,
            calls: usize,
        }
        impl EngineApi for Reader {
            fn execute(
                &mut self,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                assert!(matches!(command, Command::ReviewStatus));
                assert_eq!(project, Some(self.status.project_id.clone()));
                assert!(revision.is_none());
                self.calls += 1;
                Ok(ResponseBody::ReviewStatus(self.status.clone()))
            }
        }
        let i = intent();
        let flag = pulsar_protocol::ReviewFlag {
            axis: Some(pulsar_protocol::Axis::Stroke),
            range: pulsar_protocol::TimeRange::new(
                pulsar_protocol::ProjectTime::ZERO,
                pulsar_protocol::ProjectTime::from_nanos(1_000_000),
            )
            .unwrap(),
            reason: pulsar_protocol::ReviewReason::new("tracking_gap").unwrap(),
            evidence: pulsar_protocol::EvidenceKind::Predicted,
            confidence: None,
        };
        let review_id = review_identity(&flag).unwrap();
        let mut report = AssistantReport {
            image: None,
            attempt_id: AttemptId::new("attempt").unwrap(),
            project_id: i.project.clone(),
            session_id: i.session.clone(),
            request_id: i.request_id,
            base_revision: i.revision,
            engine_epoch: AttemptId::new("epoch").unwrap(),
            profile: i.spec.profile,
            context_sha256: "c".repeat(64),
            scope: AssistantProposalScope {
                review_ids: vec![review_id.clone()],
                ..Default::default()
            },
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::ExplainReview {
                review_id: review_id.clone(),
            }),
            error: None,
            worker_peak_memory_bytes: Some(1),
        };
        let mut reader = Reader {
            status: ReviewStatus {
                project_id: i.project.clone(),
                revision: i.revision,
                entries: vec![pulsar_protocol::ReviewEntry {
                    review_id,
                    flag,
                    decision: None,
                    decision_history: vec![],
                }],
                unknown: true,
                conservative: true,
            },
            calls: 0,
        };
        let text =
            recorded_review_explanation(&mut reader, &i.session, &i.project, i.revision, &report)
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["recorded_review"]["flag"],
            serde_json::to_value(&reader.status.entries[0].flag).unwrap()
        );
        assert_eq!(value["confidence_available"], false);
        assert_eq!(value["unknown"], true);
        reader.status.entries[0].flag.confidence =
            Some(pulsar_protocol::Confidence::new(0.4, None).unwrap());
        assert!(recorded_review_explanation(
            &mut reader,
            &i.session,
            &i.project,
            i.revision,
            &report
        )
        .is_err());
        let changed_id = review_identity(&reader.status.entries[0].flag).unwrap();
        reader.status.entries[0].review_id = changed_id.clone();
        report.scope.review_ids = vec![changed_id.clone()];
        report.proposal = Some(AssistantProposal::ExplainReview {
            review_id: changed_id,
        });
        let text =
            recorded_review_explanation(&mut reader, &i.session, &i.project, i.revision, &report)
                .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["confidence_available"], true);
        assert_eq!(
            value["recorded_review"]["flag"]["confidence"],
            serde_json::to_value(&reader.status.entries[0].flag.confidence).unwrap()
        );
        reader.status.entries.clear();
        assert!(recorded_review_explanation(
            &mut reader,
            &i.session,
            &i.project,
            i.revision,
            &report
        )
        .is_err());
        reader.status.revision = RevisionId::new(8);
        assert!(recorded_review_explanation(
            &mut reader,
            &i.session,
            &i.project,
            i.revision,
            &report
        )
        .is_err());
        let calls = reader.calls;
        report.session_id = SessionId::new("other").unwrap();
        assert!(recorded_review_explanation(
            &mut reader,
            &i.session,
            &i.project,
            i.revision,
            &report
        )
        .is_err());
        assert_eq!(reader.calls, calls);
    }
    #[test]
    fn review_focus_preserves_legacy_request_bytes_and_refuses_invalid_ids() {
        let mut spec = intent().spec;
        let legacy = serde_json::to_value(&spec).unwrap();
        assert!(legacy.get("review_focus").is_none());
        assert!(serde_json::from_value::<AssistantStart>(legacy)
            .unwrap()
            .review_focus
            .is_empty());
        spec.review_focus = vec!["a".repeat(64)];
        assert!(spec.validate().is_ok());
        spec.review_focus.push("a".repeat(64));
        assert!(spec.validate().is_err());
        spec.review_focus = vec!["invented".into()];
        assert!(spec.validate().is_err());
        spec.review_focus = (0..9).map(|i| format!("{i:064x}")).collect();
        assert!(spec.validate().is_err());
    }
    #[test]
    fn creative_image_preparation_saves_exact_context_and_rejects_foreign_namespace() {
        use pulsar_protocol::{AssistantImageContext, Axis, CreativePatternScope, ProjectTime};
        let mut expected = intent();
        expected.spec.profile.vision_projector = Some(expected.spec.profile.model.clone());
        let mut rgb = expected.spec.profile.model.clone();
        rgb.byte_len = 3;
        let image = AssistantImageContext {
            image_id: "c".repeat(64),
            project_id: expected.project.clone(),
            session_id: expected.session.clone(),
            revision: expected.revision,
            source_version: SourceVersionId::new("still").unwrap(),
            source: expected.spec.profile.model.clone(),
            rgb,
            width: 1,
            height: 1,
            preprocessing_version: 1,
            ffmpeg: expected.spec.profile.model.clone(),
            ffprobe: expected.spec.profile.model.clone(),
        };
        let scope = CreativePatternScope {
            axes: vec![Axis::Stroke],
            duration: ProjectTime::from_nanos(2_000_000_000),
            sample_hz: 8,
            max_amplitude: 0.1,
        };
        struct Reader {
            profile: pulsar_protocol::AssistantProfile,
            image: AssistantImageContext,
            calls: Vec<Command>,
        }
        impl EngineApi for Reader {
            fn execute(
                &mut self,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                assert_eq!(project.as_ref(), Some(&self.image.project_id));
                self.calls.push(command.clone());
                match command {
                    Command::AssistantProfiles => {
                        Ok(ResponseBody::AssistantProfiles(vec![self.profile.clone()]))
                    }
                    Command::PrepareAssistantImage { source_version } => {
                        assert_eq!(source_version, self.image.source_version);
                        assert_eq!(revision, Some(self.image.revision));
                        Ok(ResponseBody::AssistantImage(Box::new(self.image.clone())))
                    }
                    _ => panic!("preparation must not dispatch inference"),
                }
            }
        }
        let root = tempfile::tempdir().unwrap();
        let scope_path = root.path().join("scope.json");
        std::fs::write(&scope_path, serde_json::to_vec(&scope).unwrap()).unwrap();
        let output = root.path().join("intent.json");
        let command = || AssistantCommands::Prepare {
            image_source: Some("still".into()),
            project: expected.project.to_string(),
            profile: expected.spec.profile.profile_id.to_string(),
            revision: 7,
            prompt: "Suggest an image theme".into(),
            creative_scope: Some(scope_path.clone()),
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            request_id: "image-request".into(),
            output: output.clone(),
        };
        let mut client = Reader {
            profile: expected.spec.profile.clone(),
            image: image.clone(),
            calls: vec![],
        };
        run_bound(
            &mut client,
            &expected.endpoint,
            &expected.session,
            &expected.credential_binding,
            command(),
        )
        .unwrap();
        assert_eq!(client.calls.len(), 2);
        let saved = load(&output).unwrap();
        assert_eq!(saved.spec.image.as_ref(), Some(&image));
        std::fs::remove_file(&output).unwrap();
        client.image.session_id = SessionId::new("foreign").unwrap();
        assert!(run_bound(
            &mut client,
            &expected.endpoint,
            &expected.session,
            &expected.credential_binding,
            command()
        )
        .is_err());
        assert!(!output.exists());
        let mut forged = saved;
        forged.project = ProjectId::new("other").unwrap();
        assert!(save(&output, &forged).is_err());
        assert!(!output.exists());
    }
}

pub(crate) fn approved_generation_command(
    report: &AssistantReport,
    source_version: SourceVersionId,
    preset: String,
    settings: GenerationSettings,
    model_path: Option<PathBuf>,
    approved: bool,
) -> Result<Command> {
    anyhow::ensure!(approved, "explicit generation approval required");
    settings.validate()?;
    Ok(Command::GenerateWithAssistant {
        approval: AssistantGenerationApproval::from_report(report)?,
        source_version,
        preset,
        settings: Box::new(settings),
        model_path,
    })
}

pub(crate) fn recorded_review_explanation(
    client: &mut impl EngineApi,
    session: &SessionId,
    project: &ProjectId,
    revision: RevisionId,
    report: &AssistantReport,
) -> Result<String> {
    report.validate()?;
    anyhow::ensure!(
        report.session_id == *session
            && report.project_id == *project
            && report.base_revision == revision
            && report.state == AssistantState::Completed,
        "review explanation owner, project, revision or state differs"
    );
    let Some(AssistantProposal::ExplainReview { review_id }) = &report.proposal else {
        anyhow::bail!("completed review explanation proposal required")
    };
    let ResponseBody::ReviewStatus(status) =
        client.execute(Command::ReviewStatus, Some(project.clone()), None)?
    else {
        anyhow::bail!("expected review status")
    };
    status.validate()?;
    anyhow::ensure!(
        status.project_id == *project && status.revision == revision,
        "review evidence belongs to a different view"
    );
    let entry = status
        .entries
        .iter()
        .find(|e| &e.review_id == review_id)
        .ok_or_else(|| anyhow::anyhow!("selected review evidence is absent"))?;
    anyhow::ensure!(
        review_identity(&entry.flag)? == *review_id,
        "recorded review identity differs"
    );
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({"project_id":project,"revision":revision,"recorded_review":entry,"confidence_available":entry.flag.confidence.is_some(),"unknown":status.unknown,"conservative":status.conservative}),
    )?)
}
