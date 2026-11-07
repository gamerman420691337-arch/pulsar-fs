//! Private exact assistant edit declarations, pinned before approved dispatch.
use crate::{assistant_client, motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditIntent {
    pub schema_version: u16,
    pub endpoint: PathBuf,
    pub session: SessionId,
    pub credential_binding: String,
    pub request_id: RequestId,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub recipe: AssistantEditRecipe,
    pub approval: AssistantEditApproval,
}
impl EditIntent {
    pub fn from_report(
        endpoint: &Path,
        session: &SessionId,
        credential: &str,
        request_id: RequestId,
        report: &AssistantReport,
        recipe: AssistantEditRecipe,
    ) -> Result<Self> {
        ensure!(
            report.session_id == *session,
            "edit proposal belongs to another owner"
        );
        let value = Self {
            schema_version: 1,
            endpoint: endpoint.into(),
            session: session.clone(),
            credential_binding: credential.into(),
            request_id,
            project: report.project_id.clone(),
            revision: report.base_revision,
            recipe,
            approval: AssistantEditApproval::from_report(report)?,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn command(&self) -> Command {
        Command::ApplyAssistantEdit {
            approval: self.approval.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1
                && self.endpoint.is_absolute()
                && self.credential_binding.len() == 64
                && self
                    .credential_binding
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid saved edit namespace"
        );
        self.recipe.validate()?;
        ensure!(
            self.recipe.project_id == self.project && self.recipe.revision == self.revision,
            "saved recipe belongs to another project/revision"
        );
        let proposal = Some(AssistantProposal::ApplyEditRecipe {
            recipe_id: self.recipe.recipe_id.clone(),
        });
        ensure!(
            self.approval.proposal_sha256
                == format!("{:x}", Sha256::digest(serde_json::to_vec(&proposal)?)),
            "displayed edit differs from approval digest"
        );
        Request::new(self.request_id.clone(), self.command())
            .in_project(self.project.clone(), Some(self.revision))
            .validate()?;
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn check_namespace(
        &self,
        endpoint: &Path,
        session: &SessionId,
        credential: &str,
    ) -> Result<()> {
        self.validate()?;
        ensure!(
            self.endpoint == endpoint
                && self.session == *session
                && self.credential_binding == credential,
            "saved edit belongs to another endpoint/session/credential namespace"
        );
        Ok(())
    }
}
pub(crate) fn save(path: &Path, intent: &EditIntent) -> Result<()> {
    intent.validate()?;
    assistant_client::save_private(path, intent)
}
pub(crate) fn load(path: &Path) -> Result<EditIntent> {
    let intent: EditIntent = assistant_client::load_private(path)?;
    intent.validate()?;
    Ok(intent)
}
pub(crate) fn dispatch(
    client: &mut impl EngineApi,
    endpoint: &Path,
    session: &SessionId,
    credential: &str,
    path: &Path,
    expected_digest: Option<&str>,
    approved: bool,
) -> Result<ResponseBody> {
    ensure!(approved, "explicit saved edit approval required");
    let intent = load(path)?;
    intent.check_namespace(endpoint, session, credential)?;
    if let Some(digest) = expected_digest {
        ensure!(
            intent.digest()? == digest,
            "saved edit changed since review"
        );
    }
    client.execute_identified(
        intent.request_id.clone(),
        intent.command(),
        Some(intent.project),
        Some(intent.revision),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn intent() -> EditIntent {
        let mut recipe = AssistantEditRecipe {
            recipe_id: String::new(),
            project_id: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(7),
            program_sha256: "a".repeat(64),
            axis: Axis::Stroke,
            operation: ValueEditOperation::Invert,
        };
        recipe.recipe_id = recipe.identity().unwrap();
        EditIntent {
            schema_version: 1,
            endpoint: "/private/engine.sock".into(),
            session: SessionId::new("owner").unwrap(),
            credential_binding: "b".repeat(64),
            request_id: RequestId::new("original-edit").unwrap(),
            project: recipe.project_id.clone(),
            revision: recipe.revision,
            approval: AssistantEditApproval {
                attempt_id: AttemptId::new("attempt").unwrap(),
                context_sha256: "c".repeat(64),
                proposal_sha256: format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&Some(AssistantProposal::ApplyEditRecipe {
                            recipe_id: recipe.recipe_id.clone()
                        }))
                        .unwrap()
                    )
                ),
            },
            recipe,
        }
    }
    #[derive(Default)]
    struct Recording {
        calls: Vec<Vec<u8>>,
    }
    impl EngineApi for Recording {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unexpected discovery")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls
                .push(serde_json::to_vec(&(id, command, project, revision))?);
            anyhow::bail!("injected acknowledgement loss")
        }
    }
    #[test]
    fn restored_edit_replays_exact_identity_without_discovery_and_rechecks_review() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("edit.json");
        let original = intent();
        save(&path, &original).unwrap();
        let restored = load(&path).unwrap();
        let digest = restored.digest().unwrap();
        let mut c = Recording::default();
        assert!(dispatch(
            &mut c,
            &restored.endpoint,
            &restored.session,
            &restored.credential_binding,
            &path,
            Some(&digest),
            false
        )
        .is_err());
        assert!(c.calls.is_empty());
        for _ in 0..2 {
            assert!(dispatch(
                &mut c,
                &restored.endpoint,
                &restored.session,
                &restored.credential_binding,
                &path,
                Some(&digest),
                true
            )
            .is_err());
        }
        assert_eq!(c.calls.len(), 2);
        assert_eq!(c.calls[0], c.calls[1]);
        let (id, command, project, revision): (
            RequestId,
            Command,
            Option<ProjectId>,
            Option<RevisionId>,
        ) = serde_json::from_slice(&c.calls[0]).unwrap();
        assert_eq!(id, original.request_id);
        assert!(matches!(command, Command::ApplyAssistantEdit { .. }));
        assert_eq!(project, Some(original.project.clone()));
        assert_eq!(revision, Some(original.revision));
        let mut changed = original.clone();
        changed.request_id = RequestId::new("replacement").unwrap();
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(dispatch(
            &mut c,
            &original.endpoint,
            &original.session,
            &original.credential_binding,
            &path,
            Some(&digest),
            true
        )
        .is_err());
        assert_eq!(c.calls.len(), 2);
    }
    #[test]
    fn edit_semantics_and_namespace_are_bound_and_extensions_refused() {
        let x = intent();
        x.validate().unwrap();
        let mut changed = x.clone();
        changed.recipe.operation = ValueEditOperation::HalveAmplitude;
        changed.recipe.recipe_id = changed.recipe.identity().unwrap();
        assert!(changed.validate().is_err());
        let mut changed = x.clone();
        changed.recipe.revision = RevisionId::new(8);
        changed.recipe.recipe_id = changed.recipe.identity().unwrap();
        assert!(changed.validate().is_err());
        assert!(x
            .check_namespace(Path::new("/other.sock"), &x.session, &x.credential_binding)
            .is_err());
        assert!(x
            .check_namespace(
                &x.endpoint,
                &SessionId::new("foreign").unwrap(),
                &x.credential_binding
            )
            .is_err());
        assert!(x
            .check_namespace(&x.endpoint, &x.session, &"d".repeat(64))
            .is_err());
        let mut raw = serde_json::to_value(x).unwrap();
        raw["coordinates"] = serde_json::json!([0.2]);
        assert!(serde_json::from_value::<EditIntent>(raw).is_err());
    }
    #[test]
    fn private_saved_edit_refuses_overwrite_public_files_and_aliases() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("edit.json");
        let x = intent();
        save(&path, &x).unwrap();
        assert!(save(&path, &x).is_err());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = d.path().join("symlink");
        symlink(&path, &link).unwrap();
        assert!(load(&link).is_err());
        let hard = d.path().join("hardlink");
        std::fs::hard_link(&path, &hard).unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_file(hard).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&path).is_err());
    }
}
