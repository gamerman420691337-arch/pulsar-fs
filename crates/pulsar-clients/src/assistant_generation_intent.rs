//! Private exact generation approval declarations; never contain session tokens.
use crate::{assistant_client, motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenerationIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_patterns: Option<Vec<PatternSpec>>,
    pub schema_version: u16,
    pub endpoint: PathBuf,
    pub session: SessionId,
    pub credential_binding: String,
    pub request_id: RequestId,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub command: Command,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_stride: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_target: Option<String>,
}
impl GenerationIntent {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1
                && self.endpoint.is_absolute()
                && self.credential_binding.len() == 64
                && self
                    .credential_binding
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid saved generation binding"
        );
        if let Command::GenerateCreativeWithAssistant { approval } = &self.command {
            ensure!(
                self.proposed_stride.is_none() && self.proposed_target.is_none(),
                "creative intent cannot contain target or stride changes"
            );
            let patterns = self
                .proposed_patterns
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("saved creative patterns required"))?;
            GenerationInput::Patterns {
                patterns: patterns.clone(),
            }
            .validate()?;
            let proposal = Some(AssistantProposal::SynthesizePatterns {
                patterns: patterns.clone(),
            });
            ensure!(
                approval.proposal_sha256
                    == format!("{:x}", Sha256::digest(serde_json::to_vec(&proposal)?)),
                "saved patterns differ from approved proposal digest"
            );
            Request::new(self.request_id.clone(), self.command.clone())
                .in_project(self.project.clone(), Some(self.revision))
                .validate()?;
            return Ok(());
        }
        ensure!(
            self.proposed_patterns.is_none(),
            "ordinary generation cannot contain creative patterns"
        );
        let Command::GenerateWithAssistant { approval, .. } = &self.command else {
            anyhow::bail!("saved request must be assistant-approved generation")
        };
        let proposal = Some(match (self.proposed_stride, &self.proposed_target) {
            (Some(stride), None) if (1..=64).contains(&stride) => {
                AssistantProposal::SetSampleStride { stride }
            }
            (None, Some(target_id))
                if target_id.len() == 64
                    && target_id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
            {
                AssistantProposal::SelectTarget {
                    target_id: target_id.clone(),
                }
            }
            _ => anyhow::bail!(
                "saved generation requires exactly one bounded stride or target proposal"
            ),
        });
        ensure!(
            approval.proposal_sha256
                == format!("{:x}", Sha256::digest(serde_json::to_vec(&proposal)?)),
            "saved proposal differs from approved proposal digest"
        );
        Request::new(self.request_id.clone(), self.command.clone())
            .in_project(self.project.clone(), Some(self.revision))
            .validate()?;
        Ok(())
    }
    pub fn proposal_label(&self) -> String {
        if let Some(patterns) = &self.proposed_patterns {
            return format!("synthetic patterns on {} axes", patterns.len());
        }
        match (self.proposed_stride, self.proposed_target.as_ref()) {
            (Some(stride), None) => format!("sample stride {stride}"),
            (None, Some(target)) => format!("observed target {target}"),
            _ => "invalid proposal".into(),
        }
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
            "saved generation belongs to another endpoint/session/credential namespace"
        );
        Ok(())
    }
}
pub(crate) fn save(path: &Path, intent: &GenerationIntent) -> Result<()> {
    intent.validate()?;
    assistant_client::save_private(path, intent)
}
pub(crate) fn load(path: &Path) -> Result<GenerationIntent> {
    let intent: GenerationIntent = assistant_client::load_private(path)?;
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
    ensure!(approved, "explicit generation approval required");
    let intent = load(path)?;
    intent.check_namespace(endpoint, session, credential)?;
    if let Some(digest) = expected_digest {
        ensure!(
            intent.digest()? == digest,
            "saved generation changed since review"
        );
    }
    client.execute_identified(
        intent.request_id,
        intent.command,
        Some(intent.project),
        Some(intent.revision),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn saved_creative_intent_binds_exact_patterns_and_preserves_private_recovery() {
        let mut saved = intent();
        let patterns = vec![parse_pattern_prompt(
            "pattern=hold axis=stroke duration=2s frequency=0hz amplitude=0 offset=0.5 sample_hz=8",
        )
        .unwrap()];
        let proposal = Some(AssistantProposal::SynthesizePatterns {
            patterns: patterns.clone(),
        });
        saved.command = Command::GenerateCreativeWithAssistant {
            approval: AssistantCreativeApproval {
                attempt_id: AttemptId::new("creative").unwrap(),
                context_sha256: "b".repeat(64),
                proposal_sha256: format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(&proposal).unwrap())
                ),
            },
        };
        saved.proposed_stride = None;
        saved.proposed_target = None;
        saved.proposed_patterns = Some(patterns);
        saved.validate().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("creative.json");
        save(&path, &saved).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.digest().unwrap(), saved.digest().unwrap());
        assert_eq!(loaded.proposal_label(), "synthetic patterns on 1 axes");
        let mut changed = saved.clone();
        changed.proposed_patterns.as_mut().unwrap()[0] = parse_pattern_prompt(
            "pattern=hold axis=stroke duration=3s frequency=0hz amplitude=0 offset=0.5 sample_hz=8",
        )
        .unwrap();
        assert!(changed.validate().is_err());
        let mut changed = saved.clone();
        changed.proposed_stride = Some(1);
        assert!(changed.validate().is_err());
        let mut changed = saved;
        changed.proposed_patterns = None;
        assert!(changed.validate().is_err());
        let ordinary = intent();
        assert!(serde_json::to_value(&ordinary)
            .unwrap()
            .get("proposed_patterns")
            .is_none());
    }
    use std::os::unix::fs::PermissionsExt;
    fn intent() -> GenerationIntent {
        let proposed_stride = 8;
        GenerationIntent {
            proposed_patterns: None,
            schema_version: 1,
            endpoint: "/private/engine.sock".into(),
            session: SessionId::new("session").unwrap(),
            credential_binding: "b".repeat(64),
            request_id: RequestId::new("original-generation").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(7),
            proposed_stride: Some(proposed_stride),
            proposed_target: None,
            command: Command::GenerateWithAssistant {
                approval: AssistantGenerationApproval {
                    attempt_id: AttemptId::new("attempt").unwrap(),
                    context_sha256: "c".repeat(64),
                    proposal_sha256: format!(
                        "{:x}",
                        Sha256::digest(
                            serde_json::to_vec(&Some(AssistantProposal::SetSampleStride {
                                stride: proposed_stride
                            }))
                            .unwrap()
                        )
                    ),
                },
                source_version: SourceVersionId::new("source").unwrap(),
                preset: "fast".into(),
                settings: Box::new(GenerationSettings {
                    axis: Axis::Yaw,
                    source_sample_stride: 2,
                    ..Default::default()
                }),
                model_path: Some("/private/model.onnx".into()),
            },
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
    fn generation_intent_is_private_strict_complete_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generation.json");
        let i = intent();
        save(&path, &i).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(load(&path).unwrap().digest().unwrap(), i.digest().unwrap());
        let bytes = std::fs::read(&path).unwrap();
        assert!(save(&path, &i).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(load(&alias).is_err());
        let linked = dir.path().join("linked");
        std::fs::hard_link(&path, &linked).unwrap();
        assert!(load(&path).is_err());
        let mut unknown = serde_json::to_value(&i).unwrap();
        unknown["auth_token"] = "invented".into();
        assert!(serde_json::from_value::<GenerationIntent>(unknown).is_err());
        let mut wrong = i;
        wrong.command = Command::Undo;
        assert!(wrong.validate().is_err());
        wrong = intent();
        wrong.proposed_stride = Some(4);
        assert!(wrong.validate().is_err());
    }
    #[test]
    fn generation_saved_retry_preserves_complete_identity_and_refuses_changed_review() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generation.json");
        let i = intent();
        save(&path, &i).unwrap();
        let digest = i.digest().unwrap();
        let mut client = Recording::default();
        assert!(dispatch(
            &mut client,
            &i.endpoint,
            &i.session,
            &i.credential_binding,
            &path,
            Some(&digest),
            false
        )
        .is_err());
        assert!(client.calls.is_empty());
        for _ in 0..2 {
            assert!(dispatch(
                &mut client,
                &i.endpoint,
                &i.session,
                &i.credential_binding,
                &path,
                Some(&digest),
                true
            )
            .is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        let expected = serde_json::to_vec(&(
            i.request_id.clone(),
            i.command.clone(),
            Some(i.project.clone()),
            Some(i.revision),
        ))
        .unwrap();
        assert_eq!(client.calls[0], expected);
        for (endpoint, session, credential) in [
            (
                Path::new("/private/other.sock"),
                i.session.clone(),
                i.credential_binding.clone(),
            ),
            (
                i.endpoint.as_path(),
                SessionId::new("other").unwrap(),
                i.credential_binding.clone(),
            ),
            (i.endpoint.as_path(), i.session.clone(), "a".repeat(64)),
        ] {
            assert!(dispatch(
                &mut client,
                endpoint,
                &session,
                &credential,
                &path,
                Some(&digest),
                true
            )
            .is_err());
        }
        let mut changed = i.clone();
        if let Command::GenerateWithAssistant { source_version, .. } = &mut changed.command {
            *source_version = SourceVersionId::new("different-source").unwrap();
        }
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(dispatch(
            &mut client,
            &i.endpoint,
            &i.session,
            &i.credential_binding,
            &path,
            Some(&digest),
            true
        )
        .is_err());
        assert_eq!(client.calls.len(), 2);
    }
    #[test]
    fn generation_target_intent_roundtrips_and_legacy_stride_bytes_stay_identical() {
        let stride = intent();
        let old = serde_json::to_value(&stride).unwrap();
        assert_eq!(old["proposed_stride"], 8);
        assert!(old.get("proposed_target").is_none());
        let decoded: GenerationIntent = serde_json::from_value(old.clone()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), old);
        let mut target = intent();
        target.proposed_stride = None;
        target.proposed_target = Some("d".repeat(64));
        let Command::GenerateWithAssistant { approval, .. } = &mut target.command else {
            panic!("command");
        };
        approval.proposal_sha256 = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&Some(AssistantProposal::SelectTarget {
                    target_id: "d".repeat(64)
                }))
                .unwrap()
            )
        );
        target.validate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target-intent.json");
        save(&path, &target).unwrap();
        assert_eq!(
            load(&path).unwrap().digest().unwrap(),
            target.digest().unwrap()
        );
        let mut client = Recording::default();
        let digest = target.digest().unwrap();
        assert!(dispatch(
            &mut client,
            &target.endpoint,
            &target.session,
            &target.credential_binding,
            &path,
            Some(&digest),
            false
        )
        .is_err());
        assert!(client.calls.is_empty());
        for _ in 0..2 {
            assert!(dispatch(
                &mut client,
                &target.endpoint,
                &target.session,
                &target.credential_binding,
                &path,
                Some(&digest),
                true
            )
            .is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        target.proposed_target = Some("e".repeat(64));
        assert!(target.validate().is_err());
        target.proposed_stride = Some(8);
        assert!(target.validate().is_err());
    }
}
