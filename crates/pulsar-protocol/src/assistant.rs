//! Untrusted local-assistant proposals. No command, approval, or effect authority.
//! The engine must revalidate current bindings and obtain approval before dispatch.
use crate::ProtocolError;
use serde::{Deserialize, Serialize};

pub const MAX_ASSISTANT_PROPOSAL_BYTES: usize = 4096;

/// References are opaque host-issued entries in a bounded context, not paths,
/// executable names, model-generated coordinates, evidence, or edit values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantProposal {
    SelectTarget {
        target_id: String,
    },
    ExplainReview {
        review_id: String,
    },
    SetSampleStride {
        stride: u32,
    },
    ApplyEditRecipe {
        recipe_id: String,
    },
    SynthesizePatterns {
        patterns: Vec<pulsar_core::PatternSpec>,
    },
    NoAction {},
}

/// Supplied by the host, not by the model. This is a syntax/context check only;
/// possession does not prove observation, approval, freshness, or edit safety.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantProposalScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creative_patterns: Option<crate::CreativePatternScope>,
    pub target_ids: Vec<String>,
    pub review_ids: Vec<String>,
    pub edit_recipe_ids: Vec<String>,
    pub allowed_sample_strides: Vec<u32>,
}
impl AssistantProposalScope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(creative) = &self.creative_patterns {
            creative.validate()?;
        }
        for ids in [&self.target_ids, &self.review_ids, &self.edit_recipe_ids] {
            let unique: std::collections::BTreeSet<_> = ids.iter().collect();
            if ids.len() > 64
                || unique.len() != ids.len()
                || ids.iter().any(|id| {
                    id.is_empty()
                        || id.len() > 64
                        || !id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
                })
            {
                return Err(ProtocolError::invalid(
                    "invalid assistant context identifiers",
                ));
            }
        }
        let unique: std::collections::BTreeSet<_> = self.allowed_sample_strides.iter().collect();
        if self.allowed_sample_strides.len() > 64
            || unique.len() != self.allowed_sample_strides.len()
            || self
                .allowed_sample_strides
                .iter()
                .any(|stride| !(1..=64).contains(stride))
        {
            return Err(ProtocolError::invalid(
                "invalid assistant sample stride scope",
            ));
        }
        Ok(())
    }
}
impl AssistantProposal {
    /// Parse the whole bounded JSON body. Never extract an object from log text,
    /// code fences, reasoning tags, an incomplete generation, or multiple replies.
    pub fn decode(bytes: &[u8], scope: &AssistantProposalScope) -> Result<Self, ProtocolError> {
        scope.validate()?;
        if bytes.is_empty() || bytes.len() > MAX_ASSISTANT_PROPOSAL_BYTES {
            return Err(ProtocolError::invalid(
                "assistant proposal exceeds byte bound",
            ));
        }
        let proposal: Self = serde_json::from_slice(bytes).map_err(|_| {
            ProtocolError::invalid("assistant proposal is not one strict JSON body")
        })?;
        let allowed = match &proposal {
            Self::SelectTarget { target_id } => scope.target_ids.contains(target_id),
            Self::ExplainReview { review_id } => scope.review_ids.contains(review_id),
            Self::ApplyEditRecipe { recipe_id } => scope.edit_recipe_ids.contains(recipe_id),
            Self::SetSampleStride { stride } => scope.allowed_sample_strides.contains(stride),
            Self::SynthesizePatterns { patterns } => {
                let creative = scope
                    .creative_patterns
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("creative patterns are not offered"))?;
                creative.validate_proposal(patterns)?;
                true
            }
            Self::NoAction {} => true,
        };
        if !allowed {
            return Err(ProtocolError::invalid(
                "assistant proposal references an unavailable context entry",
            ));
        }
        Ok(proposal)
    }
}

/// A host-captured, row-major RGB8 still and its paired vision projector.
/// Identity/dimensions are data bindings, not observed motion or path authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantVisionInput {
    pub projector: crate::ArtifactIdentity,
    pub rgb: crate::ArtifactIdentity,
    pub width: u32,
    pub height: u32,
}
impl AssistantVisionInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.projector.validate()?;
        self.rgb.validate()?;
        if self.projector.byte_len > 2 * 1024 * 1024 * 1024
            || !(1..=512).contains(&self.width)
            || !(1..=512).contains(&self.height)
            || self.rgb.byte_len != u64::from(self.width) * u64::from(self.height) * 3
        {
            return Err(ProtocolError::invalid(
                "assistant vision dimensions or artifact bounds differ",
            ));
        }
        Ok(())
    }
}

/// Private one-shot worker transport. Identity declarations authorize no effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantWorkerRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<AssistantVisionInput>,
    pub version: u16,
    pub attempt_id: crate::AttemptId,
    pub runtime: crate::ArtifactIdentity,
    pub model: crate::ArtifactIdentity,
    pub system_context: String,
    pub user_request: String,
    pub scope: AssistantProposalScope,
    pub cpu_threads: u16,
    pub wall_time_ms: u64,
}
pub const MAX_ASSISTANT_WORKER_REQUEST_BYTES: u64 = 32 * 1024;
pub const MAX_ASSISTANT_WORKER_RESPONSE_BYTES: u64 = 8 * 1024;
impl AssistantWorkerRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.runtime.validate()?;
        self.model.validate()?;
        self.scope.validate()?;
        if let Some(vision) = &self.vision {
            vision.validate()?;
            if self.scope.creative_patterns.is_none()
                || !self.scope.target_ids.is_empty()
                || !self.scope.review_ids.is_empty()
                || !self.scope.edit_recipe_ids.is_empty()
                || !self.scope.allowed_sample_strides.is_empty()
            {
                return Err(ProtocolError::invalid(
                    "vision inference requires exclusively synthetic creative scope",
                ));
            }
        }
        if self.version != crate::PROTOCOL_VERSION
            || self.runtime.byte_len > 128 * 1024 * 1024
            || self.model.byte_len > 2 * 1024 * 1024 * 1024
            || self.system_context.is_empty()
            || self.system_context.len() > 8192
            || self.user_request.trim().is_empty()
            || self.user_request.len() > 4096
            || self.system_context.contains('\0')
            || self.user_request.contains('\0')
            || !(1..=16).contains(&self.cpu_threads)
            || !(1..=120_000).contains(&self.wall_time_ms)
        {
            return Err(ProtocolError::invalid(
                "assistant worker request exceeds explicit limits",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantWorkerResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<AssistantVisionInput>,
    pub version: u16,
    pub attempt_id: crate::AttemptId,
    pub runtime: crate::ArtifactIdentity,
    pub model: crate::ArtifactIdentity,
    pub proposal: AssistantProposal,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> AssistantProposalScope {
        AssistantProposalScope {
            target_ids: vec!["cup2".into()],
            review_ids: vec!["flag1".into()],
            edit_recipe_ids: vec!["smooth_selection".into()],
            allowed_sample_strides: vec![1, 4],
            ..Default::default()
        }
    }
    #[test]
    fn resolves_only_supplied_entries_for_every_action() {
        for body in [
            r#"{"kind":"select_target","target_id":"cup2"}"#,
            r#"{"kind":"explain_review","review_id":"flag1"}"#,
            r#"{"kind":"apply_edit_recipe","recipe_id":"smooth_selection"}"#,
            r#"{"kind":"set_sample_stride","stride":4}"#,
            r#"{"kind":"no_action"}"#,
        ] {
            assert!(AssistantProposal::decode(body.as_bytes(), &scope()).is_ok());
        }
        for body in [
            r#"{"kind":"select_target","target_id":"missing"}"#,
            r#"{"kind":"explain_review","review_id":"invented"}"#,
            r#"{"kind":"apply_edit_recipe","recipe_id":"replace_protection"}"#,
            r#"{"kind":"set_sample_stride","stride":3}"#,
        ] {
            assert!(
                AssistantProposal::decode(body.as_bytes(), &scope()).is_err(),
                "accepted {body}"
            );
        }
    }
    #[test]
    fn refuses_surrounding_text_extensions_duplicate_fields_and_partial_output() {
        for body in [
            r#"{"kind":"no_action"} [end of text]"#,
            r#"log {"kind":"no_action"}"#,
            r#"{"kind":"no_action"} {}"#,
            r#"{"kind":"no_action","approve":true}"#,
            r#"{"kind":"select_target","target_id":"cup2","target_id":"missing"}"#,
            r#"{"kind":"select_target","target_id":"cup2""#,
            r#"{"kind":"actuate_device"}"#,
        ] {
            assert!(
                AssistantProposal::decode(body.as_bytes(), &scope()).is_err(),
                "accepted {body}"
            );
        }
        assert!(AssistantProposal::decode(&vec![b' '; 4097], &scope()).is_err());
    }
    #[test]
    fn removal_invalidates_old_reference_and_invalid_scope_refuses_even_no_action() {
        let body = br#"{"kind":"select_target","target_id":"cup2"}"#;
        let mut current = scope();
        assert!(AssistantProposal::decode(body, &current).is_ok());
        current.target_ids.clear();
        assert!(AssistantProposal::decode(body, &current).is_err());
        current.review_ids.push("flag1".into());
        assert!(AssistantProposal::decode(br#"{"kind":"no_action"}"#, &current).is_err());
        current = scope();
        current.allowed_sample_strides.push(0);
        assert!(current.validate().is_err());
        current = scope();
        current.edit_recipe_ids = vec!["../script".into()];
        assert!(current.validate().is_err());
    }
}

/// Host-registered native exception, scoped to this Engine lifetime. No paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision_projector: Option<crate::ArtifactIdentity>,
    pub profile_id: crate::AttemptId,
    pub worker: crate::ArtifactIdentity,
    pub runtime: crate::ArtifactIdentity,
    pub model: crate::ArtifactIdentity,
    pub cpu_threads: u16,
    pub wall_time_ms: u64,
    pub worker_memory_bytes: u64,
}
impl AssistantProfile {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.worker.validate()?;
        if let Some(projector) = &self.vision_projector {
            projector.validate()?;
            if projector.byte_len > 2 * 1024 * 1024 * 1024 {
                return Err(ProtocolError::invalid(
                    "assistant vision projector exceeds bound",
                ));
            }
        }
        AssistantWorkerRequest {
            vision: None,
            version: crate::PROTOCOL_VERSION,
            attempt_id: self.profile_id.clone(),
            runtime: self.runtime.clone(),
            model: self.model.clone(),
            system_context: "validation".into(),
            user_request: "validation".into(),
            scope: AssistantProposalScope::default(),
            cpu_threads: self.cpu_threads,
            wall_time_ms: self.wall_time_ms,
        }
        .validate()?;
        if self.worker.byte_len > 128 * 1024 * 1024
            || !(512 * 1024 * 1024..=8 * 1024 * 1024 * 1024).contains(&self.worker_memory_bytes)
        {
            return Err(ProtocolError::invalid(
                "assistant profile exceeds native bounds",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantStart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<AssistantImageContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creative_patterns: Option<crate::CreativePatternScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edit_focus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_focus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_focus: Vec<String>,
    pub profile: AssistantProfile,
    pub user_request: String,
}

#[cfg(test)]
mod creative_start_tests {
    use super::*;
    #[test]
    fn creative_start_is_explicit_exclusive_and_approval_is_a_distinct_kind() {
        let mut start = AssistantStart {
            image: None,
            creative_patterns: None,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: super::authenticated_tests::profile(),
            user_request: "Create a triangle stroke".into(),
        };
        assert!(serde_json::to_value(&start)
            .unwrap()
            .get("creative_patterns")
            .is_none());
        start.creative_patterns = Some(crate::CreativePatternScope {
            axes: vec![crate::Axis::Stroke],
            duration: crate::ProjectTime::from_nanos(2_000_000_000),
            sample_hz: 8,
            max_amplitude: 0.1,
        });
        start.validate().unwrap();
        start.edit_focus.push("a".repeat(64));
        assert!(start.validate().is_err());
        start.edit_focus.clear();
        let mut report=AssistantReport { image: None, attempt_id:crate::AttemptId::new("creative").unwrap(),project_id:crate::ProjectId::new("project").unwrap(),session_id:crate::SessionId::new("owner").unwrap(),request_id:crate::RequestId::new("start").unwrap(),base_revision:crate::RevisionId::new(0),engine_epoch:crate::AttemptId::new("epoch").unwrap(),profile:start.profile.clone(),context_sha256:"a".repeat(64),scope:AssistantProposalScope { creative_patterns:start.creative_patterns.clone(),..Default::default() },state:AssistantState::Completed,proposal:Some(AssistantProposal::SynthesizePatterns { patterns:vec![crate::parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap()] }),error:None,worker_peak_memory_bytes:Some(1) };
        let approval = crate::AssistantCreativeApproval::from_report(&report).unwrap();
        approval.validate().unwrap();
        assert!(AssistantGenerationApproval::from_report(&report).is_err());
        assert!(AssistantEditApproval::from_report(&report).is_err());
        let request = crate::Request {
            version: crate::PROTOCOL_VERSION,
            request_id: report.request_id.clone(),
            session: Some(report.session_id.clone()),
            auth_token: None,
            project: Some(report.project_id.clone()),
            expected_revision: Some(report.base_revision),
            command: crate::Command::StartAssistant {
                spec: start.clone(),
            },
        };
        report.validate_for(&request).unwrap();
        report
            .scope
            .creative_patterns
            .as_mut()
            .unwrap()
            .max_amplitude = 0.2;
        assert!(report.validate_for(&request).is_err());
        report.proposal = Some(AssistantProposal::NoAction {});
        assert!(crate::AssistantCreativeApproval::from_report(&report).is_err());
        let request = crate::Request::new(
            crate::RequestId::new("generate").unwrap(),
            crate::Command::GenerateCreativeWithAssistant { approval },
        )
        .in_project(report.project_id.clone(), Some(report.base_revision));
        request.validate().unwrap();
        let mut no_revision = request;
        no_revision.expected_revision = None;
        assert!(no_revision.validate().is_err());
    }
}
impl AssistantStart {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.profile.validate()?;
        if self.image.is_some() != self.profile.vision_projector.is_some()
            || (self.image.is_some() && self.creative_patterns.is_none())
        {
            return Err(ProtocolError::invalid(
                "vision profile requires a prepared image and creative scope",
            ));
        }
        if let Some(image) = &self.image {
            image.validate()?;
        }
        if let Some(scope) = &self.creative_patterns {
            scope.validate()?;
            if !self.edit_focus.is_empty()
                || !self.target_focus.is_empty()
                || !self.review_focus.is_empty()
            {
                return Err(ProtocolError::invalid(
                    "creative starts cannot mix target, review or edit focus",
                ));
            }
        }
        if self.edit_focus.len() > 8
            || self
                .edit_focus
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.edit_focus.len()
        {
            return Err(ProtocolError::invalid(
                "assistant edit focus must be unique and at most eight",
            ));
        }
        for id in &self.edit_focus {
            crate::validate_review_intent(id, "edit focus")?;
        }
        if self.target_focus.len() > 8
            || self
                .target_focus
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.target_focus.len()
        {
            return Err(ProtocolError::invalid(
                "assistant target focus must be unique and at most eight",
            ));
        }
        for id in &self.target_focus {
            crate::validate_review_intent(id, "target focus")?;
        }
        if self.review_focus.len() > 8
            || self
                .review_focus
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.review_focus.len()
        {
            return Err(ProtocolError::invalid(
                "assistant review focus must be unique and at most eight",
            ));
        }
        for id in &self.review_focus {
            crate::validate_review_intent(id, "focus")?;
        }
        if self.user_request.trim().is_empty()
            || self.user_request.len() > 4096
            || self.user_request.contains('\0')
        {
            return Err(ProtocolError::invalid("assistant prompt exceeds bounds"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantState {
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<AssistantImageContext>,
    pub attempt_id: crate::AttemptId,
    pub project_id: crate::ProjectId,
    pub session_id: crate::SessionId,
    pub request_id: crate::RequestId,
    pub base_revision: crate::RevisionId,
    pub engine_epoch: crate::AttemptId,
    pub profile: AssistantProfile,
    pub context_sha256: String,
    pub scope: AssistantProposalScope,
    pub state: AssistantState,
    pub proposal: Option<AssistantProposal>,
    pub error: Option<ProtocolError>,
    pub worker_peak_memory_bytes: Option<u64>,
}
impl AssistantReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.profile.validate()?;
        self.scope.validate()?;
        if self.image.is_some() != self.profile.vision_projector.is_some()
            || (self.image.is_some()
                && (self.scope.creative_patterns.is_none()
                    || !self.scope.target_ids.is_empty()
                    || !self.scope.review_ids.is_empty()
                    || !self.scope.edit_recipe_ids.is_empty()
                    || !self.scope.allowed_sample_strides.is_empty()))
        {
            return Err(ProtocolError::invalid(
                "assistant report vision capability/scope differs",
            ));
        }
        if let Some(image) = &self.image {
            image.validate()?;
            if image.project_id != self.project_id
                || image.session_id != self.session_id
                || image.revision != self.base_revision
            {
                return Err(ProtocolError::invalid(
                    "assistant image report owner/revision differs",
                ));
            }
        }
        if self.context_sha256.len() != 64
            || !self
                .context_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ProtocolError::invalid("invalid assistant context digest"));
        }
        match self.state {
            AssistantState::Running
                if self.proposal.is_none()
                    && self.error.is_none()
                    && self.worker_peak_memory_bytes.is_none() => {}
            AssistantState::Completed
                if self.proposal.is_some()
                    && self.error.is_none()
                    && self
                        .worker_peak_memory_bytes
                        .is_some_and(|n| n <= self.profile.worker_memory_bytes) =>
            {
                let bytes = serde_json::to_vec(self.proposal.as_ref().unwrap())
                    .map_err(|_| ProtocolError::invalid("assistant proposal encoding"))?;
                AssistantProposal::decode(&bytes, &self.scope)?;
            }
            AssistantState::Failed | AssistantState::Cancelled | AssistantState::Interrupted
                if self.proposal.is_none()
                    && self.error.is_some()
                    && self.worker_peak_memory_bytes.is_none() => {}
            _ => return Err(ProtocolError::invalid("inconsistent assistant state")),
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &crate::Request) -> Result<(), ProtocolError> {
        self.validate()?;
        if request.project.as_ref() != Some(&self.project_id)
            || request.session.as_ref() != Some(&self.session_id)
        {
            return Err(ProtocolError::invalid("assistant response owner differs"));
        }
        let valid = match &request.command {
            crate::Command::StartAssistant { spec } => {
                self.request_id == request.request_id
                    && Some(self.base_revision) == request.expected_revision
                    && self.profile == spec.profile
                    && self.image == spec.image
                    && self.scope.creative_patterns == spec.creative_patterns
            }
            crate::Command::AssistantStatus { attempt_id }
            | crate::Command::CancelAssistant { attempt_id } => attempt_id == &self.attempt_id,
            crate::Command::ListAssistantAttempts => true,
            _ => false,
        };
        if !valid {
            return Err(ProtocolError::invalid(
                "assistant response differs from request",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod authenticated_tests {
    use super::*;
    pub(super) fn profile() -> AssistantProfile {
        AssistantProfile {
            vision_projector: None,
            profile_id: crate::AttemptId::new("profile").unwrap(),
            worker: crate::ArtifactIdentity {
                artifact_id: crate::ArtifactId::new("fixture").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 1,
            },
            runtime: crate::ArtifactIdentity {
                artifact_id: crate::ArtifactId::new("fixture").unwrap(),
                sha256: "b".repeat(64),
                byte_len: 1,
            },
            model: crate::ArtifactIdentity {
                artifact_id: crate::ArtifactId::new("fixture").unwrap(),
                sha256: "c".repeat(64),
                byte_len: 1,
            },
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 * 1024 * 1024,
        }
    }
    #[test]
    fn start_requires_revision_and_refuses_client_context_or_asset_paths() {
        let spec = AssistantStart {
            image: None,
            creative_patterns: None,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: profile(),
            user_request: "stride 8".into(),
        };
        let mut raw = serde_json::to_value(&spec).unwrap();
        raw["runtime_path"] = "/tmp/unapproved".into();
        assert!(serde_json::from_value::<AssistantStart>(raw).is_err());
        let mut raw = serde_json::to_value(&spec).unwrap();
        raw["system_context"] = "invented observations".into();
        assert!(serde_json::from_value::<AssistantStart>(raw).is_err());
        let mut request = crate::Request::new(
            crate::RequestId::new("request").unwrap(),
            crate::Command::StartAssistant { spec },
        );
        request.project = Some(crate::ProjectId::new("project").unwrap());
        assert!(request.validate().is_err());
        request.expected_revision = Some(crate::RevisionId::new(0));
        assert!(request.validate().is_ok());
    }
    #[test]
    fn report_states_and_original_owner_identity_are_independent_checks() {
        let mut report = AssistantReport {
            image: None,
            attempt_id: crate::AttemptId::new("attempt").unwrap(),
            project_id: crate::ProjectId::new("project").unwrap(),
            session_id: crate::SessionId::new("session").unwrap(),
            request_id: crate::RequestId::new("request").unwrap(),
            base_revision: crate::RevisionId::new(0),
            engine_epoch: crate::AttemptId::new("epoch").unwrap(),
            profile: profile(),
            context_sha256: "d".repeat(64),
            scope: AssistantProposalScope::default(),
            state: AssistantState::Running,
            proposal: None,
            error: None,
            worker_peak_memory_bytes: None,
        };
        assert!(report.validate().is_ok());
        report.state = AssistantState::Completed;
        assert!(report.validate().is_err());
        report.proposal = Some(AssistantProposal::NoAction {});
        report.worker_peak_memory_bytes = Some(1);
        assert!(report.validate().is_ok());
        assert!(AssistantGenerationApproval::from_report(&report).is_err());
        report.proposal = Some(AssistantProposal::SetSampleStride { stride: 8 });
        report.scope.allowed_sample_strides = vec![8, 4];
        let approval = AssistantGenerationApproval::from_report(&report).unwrap();
        approval.validate().unwrap();
        let mut malformed = approval.clone();
        malformed.proposal_sha256 = "A".repeat(64);
        assert!(malformed.validate().is_err());
        report.proposal = Some(AssistantProposal::SetSampleStride { stride: 4 });
        assert_ne!(
            approval.proposal_sha256,
            AssistantGenerationApproval::from_report(&report)
                .unwrap()
                .proposal_sha256
        );
        report.proposal = Some(AssistantProposal::SelectTarget {
            target_id: "unoffered".into(),
        });
        assert!(report.validate().is_err());
        report.proposal = Some(AssistantProposal::NoAction {});
        let mut request = crate::Request::new(
            crate::RequestId::new("status").unwrap(),
            crate::Command::AssistantStatus {
                attempt_id: report.attempt_id.clone(),
            },
        );
        request.project = Some(report.project_id.clone());
        request.session = Some(report.session_id.clone());
        assert!(report.validate_for(&request).is_ok());
        request.session = Some(crate::SessionId::new("other").unwrap());
        assert!(report.validate_for(&request).is_err());
        report.worker_peak_memory_bytes = Some(9 * 1024 * 1024 * 1024);
        assert!(report.validate().is_err());
    }
}

/// Exact invocation metadata for recovery; this grants no inference authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantStartReference {
    pub original_request_id: crate::RequestId,
    pub base_revision: crate::RevisionId,
    pub spec: AssistantStart,
}
impl AssistantStartReference {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.spec.validate()
    }
    pub fn matches(&self, other: &Self) -> Result<bool, ProtocolError> {
        Ok(
            serde_json::to_vec(self).map_err(|e| ProtocolError::invalid(e.to_string()))?
                == serde_json::to_vec(other).map_err(|e| ProtocolError::invalid(e.to_string()))?,
        )
    }
    pub fn resolution_request_id(
        &self,
        project: &crate::ProjectId,
    ) -> Result<crate::RequestId, ProtocolError> {
        use sha2::{Digest, Sha256};
        self.validate()?;
        let bytes = serde_json::to_vec(&("pulsar-assistant-start-resolution-v1", project, self))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        crate::RequestId::new(format!("assistant-resolve-{:x}", Sha256::digest(bytes)))
            .map_err(|e| ProtocolError::invalid(e.to_string()))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantStartOutcome {
    Accepted { report: Box<AssistantReport> },
    Withdrawn,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantStartResolution {
    pub project_id: crate::ProjectId,
    pub session_id: crate::SessionId,
    pub reference: AssistantStartReference,
    pub outcome: AssistantStartOutcome,
}
impl AssistantStartResolution {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.reference.validate()?;
        if let AssistantStartOutcome::Accepted { report } = &self.outcome {
            report.validate()?;
            if report.project_id != self.project_id
                || report.session_id != self.session_id
                || report.request_id != self.reference.original_request_id
                || report.base_revision != self.reference.base_revision
                || report.profile != self.reference.spec.profile
            {
                return Err(ProtocolError::invalid(
                    "resolved assistant invocation differs",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &crate::Request) -> Result<(), ProtocolError> {
        self.validate()?;
        let crate::Command::ResolveAssistantStart { reference } = &request.command else {
            return Err(ProtocolError::invalid(
                "unexpected assistant resolution response",
            ));
        };
        if request.project.as_ref() != Some(&self.project_id)
            || request.session.as_ref() != Some(&self.session_id)
            || !self.reference.matches(reference)?
        {
            return Err(ProtocolError::invalid("assistant resolution scope differs"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod resolution_contract_tests {
    use super::*;
    fn reference() -> AssistantStartReference {
        let identity = || crate::ArtifactIdentity {
            artifact_id: crate::ArtifactId::new("fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        AssistantStartReference {
            original_request_id: crate::RequestId::new("original").unwrap(),
            base_revision: crate::RevisionId::new(7),
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: AssistantProfile {
                    vision_projector: None,
                    profile_id: crate::AttemptId::new("profile").unwrap(),
                    worker: identity(),
                    runtime: identity(),
                    model: identity(),
                    cpu_threads: 1,
                    wall_time_ms: 5000,
                    worker_memory_bytes: 512 * 1024 * 1024,
                },
                user_request: "no action".into(),
            },
        }
    }
    #[test]
    fn resolution_identity_and_response_are_invocation_and_actor_bound() {
        let reference = reference();
        let project = crate::ProjectId::new("project").unwrap();
        let session = crate::SessionId::new("owner").unwrap();
        let id = reference.resolution_request_id(&project).unwrap();
        assert_eq!(id, reference.resolution_request_id(&project).unwrap());
        let mut changed = reference.clone();
        changed.spec.user_request = "changed".into();
        assert_ne!(id, changed.resolution_request_id(&project).unwrap());
        let mut request = crate::Request::new(
            id,
            crate::Command::ResolveAssistantStart {
                reference: reference.clone(),
            },
        );
        request.project = Some(project.clone());
        request.session = Some(session.clone());
        let mut response = AssistantStartResolution {
            project_id: project,
            session_id: session,
            reference,
            outcome: AssistantStartOutcome::Withdrawn,
        };
        response.validate_for(&request).unwrap();
        response.session_id = crate::SessionId::new("another").unwrap();
        assert!(response.validate_for(&request).is_err());
        let mut raw = serde_json::to_value(&response).unwrap();
        raw["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AssistantStartResolution>(raw).is_err());
        let mut raw = serde_json::to_value(&changed).unwrap();
        raw["run_if_absent"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AssistantStartReference>(raw).is_err());
    }
}

/// Explicit application approval bound to one completed retained proposal.
/// This declaration grants no permission independently of authenticated dispatch.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantGenerationApproval {
    pub attempt_id: crate::AttemptId,
    pub context_sha256: String,
    pub proposal_sha256: String,
}
impl AssistantGenerationApproval {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        for digest in [&self.context_sha256, &self.proposal_sha256] {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(ProtocolError::invalid("invalid assistant approval digest"));
            }
        }
        Ok(())
    }
    pub fn from_report(report: &AssistantReport) -> Result<Self, ProtocolError> {
        use sha2::{Digest, Sha256};
        report.validate()?;
        if report.state != AssistantState::Completed
            || !matches!(
                report.proposal,
                Some(
                    AssistantProposal::SetSampleStride { .. }
                        | AssistantProposal::SelectTarget { .. }
                )
            )
        {
            return Err(ProtocolError::invalid(
                "completed generation proposal required",
            ));
        }
        let proposal_sha256 = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&report.proposal)
                    .map_err(|_| ProtocolError::invalid("proposal encoding"))?
            )
        );
        Ok(Self {
            attempt_id: report.attempt_id.clone(),
            context_sha256: report.context_sha256.clone(),
            proposal_sha256,
        })
    }
}

/// An inert reference to a checked detector observation. Coordinates are captured
/// by the Engine; an assistant may reference the ID, never author its geometry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantObservedTarget {
    pub target_id: String,
    pub context: crate::FrameContext,
    pub frame: crate::ArtifactIdentity,
    pub model: crate::ArtifactIdentity,
    pub observation: crate::DetectionObservation,
    pub source_frame_index: u64,
    pub video_view: Option<crate::VideoView>,
}
impl AssistantObservedTarget {
    pub fn identity(&self) -> Result<String, ProtocolError> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(&(
            &self.context,
            &self.frame,
            &self.model,
            &self.observation,
            self.source_frame_index,
            &self.video_view,
        ))
        .map_err(|_| ProtocolError::invalid("target identity encoding"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn seed(&self) -> crate::AnalysisTargetSeed {
        let (x, y) = self.observation.bounds().center();
        crate::AnalysisTargetSeed {
            source_frame_index: self.source_frame_index,
            x,
            y,
            class_id: None,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.frame.validate()?;
        self.model.validate()?;
        if self.frame.byte_len == 0
            || self.model.byte_len == 0
            || self.context.frame.value() != self.source_frame_index
            || !self.observation.matches_frame(&self.context)
            || self.observation.evidence() != crate::EvidenceKind::Observed
            || self.observation.bounds().space() != crate::CoordinateSpace::ProjectionNormalized
            || self.context.transform != crate::video_view_transform(self.video_view.as_ref())?
            || self.target_id != self.identity()?
        {
            return Err(ProtocolError::invalid(
                "invalid observed assistant target binding",
            ));
        }
        self.seed().validate()
    }
}
/// Ephemeral, session-owned preview facts. This is not approval or model quality.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantTargetCatalog {
    pub project_id: crate::ProjectId,
    pub session_id: crate::SessionId,
    pub revision: crate::RevisionId,
    pub targets: Vec<AssistantObservedTarget>,
}
impl AssistantTargetCatalog {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.targets.len() > 64
            || self
                .targets
                .iter()
                .map(|t| &t.target_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.targets.len()
        {
            return Err(ProtocolError::invalid(
                "invalid assistant target catalog bound",
            ));
        }
        for target in &self.targets {
            target.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod target_focus_tests {
    use super::*;
    #[test]
    fn target_focus_is_optional_strict_unique_and_bounded() {
        let mut value = serde_json::json!({"profile": {"profile_id":"profile","engine_epoch":"epoch","runtime":{"artifact_id":"runtime","sha256":"a".repeat(64),"byte_len":1},"model":{"artifact_id":"model","sha256":"b".repeat(64),"byte_len":1},"cpu_threads":1,"wall_time_ms":1000,"worker_memory_bytes":536870912},"user_request":"select target"});
        // Use an established profile fixture shape, including its derived digest.
        let profile = super::authenticated_tests::profile();
        value["profile"] = serde_json::to_value(profile).unwrap();
        let mut start: AssistantStart = serde_json::from_value(value.clone()).unwrap();
        start.validate().unwrap();
        assert_eq!(serde_json::to_value(&start).unwrap(), value);
        start.target_focus = vec!["d".repeat(64)];
        start.validate().unwrap();
        start.target_focus.push("d".repeat(64));
        assert!(start.validate().is_err());
        start.target_focus = (0..9).map(|i| format!("{i:064x}")).collect();
        assert!(start.validate().is_err());
        start.target_focus = vec!["invented".into()];
        assert!(start.validate().is_err());
    }
}

/// Host-defined recipe bound to one current project program. Possession grants no effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantEditRecipe {
    pub recipe_id: String,
    pub project_id: crate::ProjectId,
    pub revision: crate::RevisionId,
    pub program_sha256: String,
    pub axis: crate::Axis,
    pub operation: crate::ValueEditOperation,
}
impl AssistantEditRecipe {
    pub fn identity(&self) -> Result<String, ProtocolError> {
        use sha2::{Digest, Sha256};
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "pulsar-assistant-edit-recipe-v1",
                    &self.project_id,
                    self.revision,
                    &self.program_sha256,
                    self.axis,
                    self.operation
                ))
                .map_err(|_| ProtocolError::invalid("recipe identity encoding"))?
            )
        ))
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::validate_sha256(&self.program_sha256)?;
        if self.recipe_id != self.identity()? {
            return Err(ProtocolError::invalid("edit recipe binding differs"));
        }
        Ok(())
    }
    pub fn catalog(snapshot: &crate::ProjectSnapshot) -> Result<Vec<Self>, ProtocolError> {
        snapshot.motion.validate()?;
        let mut recipes = Vec::new();
        for axis in snapshot
            .motion
            .program
            .axes
            .iter()
            .filter(|a| a.action_count > 0)
        {
            for operation in [
                crate::ValueEditOperation::Invert,
                crate::ValueEditOperation::HalveAmplitude,
            ] {
                let mut recipe = Self {
                    recipe_id: String::new(),
                    project_id: snapshot.project_id.clone(),
                    revision: snapshot.revision,
                    program_sha256: snapshot.motion.program.sha256.clone(),
                    axis: axis.axis,
                    operation,
                };
                recipe.recipe_id = recipe.identity()?;
                recipes.push(recipe);
            }
        }
        Ok(recipes)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantEditApproval {
    pub attempt_id: crate::AttemptId,
    pub context_sha256: String,
    pub proposal_sha256: String,
}
impl AssistantEditApproval {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        AssistantGenerationApproval {
            attempt_id: self.attempt_id.clone(),
            context_sha256: self.context_sha256.clone(),
            proposal_sha256: self.proposal_sha256.clone(),
        }
        .validate()
    }
    pub fn from_report(report: &AssistantReport) -> Result<Self, ProtocolError> {
        use sha2::{Digest, Sha256};
        report.validate()?;
        if report.state != AssistantState::Completed
            || !matches!(
                report.proposal,
                Some(AssistantProposal::ApplyEditRecipe { .. })
            )
        {
            return Err(ProtocolError::invalid("completed edit proposal required"));
        }
        Ok(Self {
            attempt_id: report.attempt_id.clone(),
            context_sha256: report.context_sha256.clone(),
            proposal_sha256: format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&report.proposal)
                        .map_err(|_| ProtocolError::invalid("edit approval encoding"))?
                )
            ),
        })
    }
}

#[cfg(test)]
mod edit_recipe_tests {
    use super::*;
    #[test]
    fn edit_recipe_identity_and_approval_are_strict_and_separate_from_generation() {
        let mut recipe = AssistantEditRecipe {
            recipe_id: String::new(),
            project_id: crate::ProjectId::new("p").unwrap(),
            revision: crate::RevisionId::new(2),
            program_sha256: "a".repeat(64),
            axis: crate::Axis::Stroke,
            operation: crate::ValueEditOperation::Invert,
        };
        recipe.recipe_id = recipe.identity().unwrap();
        recipe.validate().unwrap();
        recipe.operation = crate::ValueEditOperation::HalveAmplitude;
        assert!(recipe.validate().is_err());
        let mut raw = serde_json::to_value(&recipe).unwrap();
        raw["coordinates"] = serde_json::json!([0.1]);
        assert!(serde_json::from_value::<AssistantEditRecipe>(raw).is_err());
        let mut start = AssistantStart {
            image: None,
            creative_patterns: None,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: super::authenticated_tests::profile(),
            user_request: "invert".into(),
        };
        let old = serde_json::to_value(&start).unwrap();
        assert!(old.get("edit_focus").is_none());
        start.edit_focus = vec!["a".repeat(64), "a".repeat(64)];
        assert!(start.validate().is_err());
        let mut report = AssistantReport {
            image: None,
            attempt_id: crate::AttemptId::new("edit-attempt").unwrap(),
            project_id: recipe.project_id.clone(),
            session_id: crate::SessionId::new("owner").unwrap(),
            request_id: crate::RequestId::new("start").unwrap(),
            base_revision: recipe.revision,
            engine_epoch: crate::AttemptId::new("epoch").unwrap(),
            profile: start.profile,
            context_sha256: "b".repeat(64),
            scope: AssistantProposalScope {
                edit_recipe_ids: vec![recipe.recipe_id.clone()],
                ..Default::default()
            },
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::ApplyEditRecipe {
                recipe_id: recipe.recipe_id,
            }),
            error: None,
            worker_peak_memory_bytes: Some(1),
        };
        AssistantEditApproval::from_report(&report)
            .unwrap()
            .validate()
            .unwrap();
        assert!(AssistantGenerationApproval::from_report(&report).is_err());
        report.scope.allowed_sample_strides = vec![8];
        report.proposal = Some(AssistantProposal::SetSampleStride { stride: 8 });
        AssistantGenerationApproval::from_report(&report).unwrap();
        assert!(AssistantEditApproval::from_report(&report).is_err());
    }
}

#[cfg(test)]
mod vision_tests {
    use super::*;
    fn identity(bytes: u64) -> crate::ArtifactIdentity {
        crate::ArtifactIdentity {
            artifact_id: crate::ArtifactId::new("vision-fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: bytes,
        }
    }
    fn request() -> AssistantWorkerRequest {
        AssistantWorkerRequest {
            vision: Some(AssistantVisionInput {
                projector: identity(4),
                rgb: identity(12),
                width: 2,
                height: 2,
            }),
            version: crate::PROTOCOL_VERSION,
            attempt_id: crate::AttemptId::new("vision").unwrap(),
            runtime: identity(1),
            model: identity(1),
            system_context: "synthetic creative context".into(),
            user_request: "Suggest a theme".into(),
            scope: AssistantProposalScope {
                creative_patterns: Some(crate::CreativePatternScope {
                    axes: vec![crate::Axis::Stroke],
                    duration: crate::ProjectTime::from_nanos(2_000_000_000),
                    sample_hz: 8,
                    max_amplitude: 0.4,
                }),
                ..Default::default()
            },
            cpu_threads: 2,
            wall_time_ms: 120000,
        }
    }
    #[test]
    fn vision_requires_bounded_exact_rgb_and_paired_projector() {
        let good = request().vision.unwrap();
        good.validate().unwrap();
        for case in 0..6 {
            let mut bad = good.clone();
            match case {
                0 => bad.width = 0,
                1 => bad.height = 513,
                2 => bad.rgb.byte_len -= 1,
                3 => bad.projector.byte_len = (2u64 << 30) + 1,
                4 => bad.rgb.sha256 = "bad".into(),
                _ => bad.width = u32::MAX,
            }
            assert!(bad.validate().is_err(), "{case}");
        }
        let maximum = AssistantVisionInput {
            projector: identity(2u64 << 30),
            rgb: identity(512 * 512 * 3),
            width: 512,
            height: 512,
        };
        maximum.validate().unwrap();
    }
    #[test]
    fn vision_scope_cannot_authorize_observed_references_or_effects() {
        let good = request();
        good.validate().unwrap();
        for case in 0..5 {
            let mut bad = good.clone();
            match case {
                0 => bad.scope.creative_patterns = None,
                1 => bad.scope.target_ids.push("target".into()),
                2 => bad.scope.review_ids.push("review".into()),
                3 => bad.scope.edit_recipe_ids.push("edit".into()),
                _ => bad.scope.allowed_sample_strides.push(1),
            }
            assert!(bad.validate().is_err(), "{case}");
        }
    }
    #[test]
    fn old_text_envelopes_omit_vision_and_roundtrip_unchanged() {
        let mut request = request();
        request.vision = None;
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&encoded)
            .unwrap()
            .get("vision")
            .is_none());
        let decoded: AssistantWorkerRequest = serde_json::from_slice(&encoded).unwrap();
        assert!(decoded.vision.is_none());
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        let response = AssistantWorkerResponse {
            vision: None,
            version: request.version,
            attempt_id: request.attempt_id,
            runtime: request.runtime,
            model: request.model,
            proposal: AssistantProposal::NoAction {},
        };
        assert!(serde_json::to_value(response)
            .unwrap()
            .get("vision")
            .is_none());
    }
}

/// Engine-issued pixels from exactly one still frame. No path or effect authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantImageContext {
    pub image_id: String,
    pub project_id: crate::ProjectId,
    pub session_id: crate::SessionId,
    pub revision: crate::RevisionId,
    pub source_version: crate::SourceVersionId,
    pub source: crate::ArtifactIdentity,
    pub rgb: crate::ArtifactIdentity,
    pub width: u32,
    pub height: u32,
    pub preprocessing_version: u16,
    pub ffmpeg: crate::ArtifactIdentity,
    pub ffprobe: crate::ArtifactIdentity,
}
impl AssistantImageContext {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        for identity in [&self.source, &self.rgb, &self.ffmpeg, &self.ffprobe] {
            identity.validate()?;
        }
        if self.image_id.len() != 64
            || !self
                .image_id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || self.preprocessing_version != 1
            || !(1..=512).contains(&self.width)
            || !(1..=512).contains(&self.height)
            || self.rgb.byte_len != u64::from(self.width) * u64::from(self.height) * 3
        {
            return Err(ProtocolError::invalid(
                "invalid prepared assistant image context",
            ));
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &crate::Request) -> Result<(), ProtocolError> {
        self.validate()?;
        if request.project.as_ref() != Some(&self.project_id)
            || request.session.as_ref() != Some(&self.session_id)
            || request.expected_revision != Some(self.revision)
            || !matches!(&request.command, crate::Command::PrepareAssistantImage { source_version } if source_version == &self.source_version)
        {
            return Err(ProtocolError::invalid(
                "prepared image does not bind requested source/owner/revision",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod prepared_image_tests {
    use super::*;
    fn image() -> AssistantImageContext {
        let identity = |size| crate::ArtifactIdentity {
            artifact_id: crate::ArtifactId::new("prepared-fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: size,
        };
        AssistantImageContext {
            image_id: "b".repeat(64),
            project_id: crate::ProjectId::new("project").unwrap(),
            session_id: crate::SessionId::new("session").unwrap(),
            revision: crate::RevisionId::new(0),
            source_version: crate::SourceVersionId::new("source").unwrap(),
            source: identity(1),
            rgb: identity(12),
            width: 2,
            height: 2,
            preprocessing_version: 1,
            ffmpeg: identity(1),
            ffprobe: identity(1),
        }
    }
    fn start() -> AssistantStart {
        let mut profile = super::authenticated_tests::profile();
        profile.vision_projector = Some(image().source);
        AssistantStart {
            image: Some(image()),
            profile,
            user_request: "Create a theme".into(),
            creative_patterns: Some(crate::CreativePatternScope {
                axes: vec![crate::Axis::Stroke],
                duration: crate::ProjectTime::from_nanos(2_000_000_000),
                sample_hz: 8,
                max_amplitude: 0.1,
            }),
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
        }
    }
    #[test]
    fn prepared_image_requires_exclusive_creative_scope_and_registered_capability() {
        start().validate().unwrap();
        for case in 0..7 {
            let mut bad = start();
            match case {
                0 => bad.profile.vision_projector = None,
                1 => bad.image = None,
                2 => bad.creative_patterns = None,
                3 => bad.target_focus.push("target".into()),
                4 => bad.review_focus.push("review".into()),
                5 => bad.edit_focus.push("edit".into()),
                _ => bad.profile.vision_projector.as_mut().unwrap().byte_len = (2u64 << 30) + 1,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
    }
    #[test]
    fn prepared_image_binds_owner_source_revision_and_bounded_preprocessing() {
        let good = image();
        let mut request = crate::Request::new(
            crate::RequestId::new("prepare").unwrap(),
            crate::Command::PrepareAssistantImage {
                source_version: good.source_version.clone(),
            },
        );
        request.project = Some(good.project_id.clone());
        request.session = Some(good.session_id.clone());
        request.expected_revision = Some(good.revision);
        request.validate().unwrap();
        good.validate_for(&request).unwrap();
        for case in 0..10 {
            let mut bad = good.clone();
            match case {
                0 => bad.image_id = "B".repeat(64),
                1 => bad.width = 0,
                2 => bad.height = 513,
                3 => bad.rgb.byte_len += 1,
                4 => bad.preprocessing_version = 2,
                5 => bad.session_id = crate::SessionId::new("other").unwrap(),
                6 => bad.project_id = crate::ProjectId::new("other").unwrap(),
                7 => bad.revision = crate::RevisionId::new(1),
                8 => bad.source_version = crate::SourceVersionId::new("other").unwrap(),
                _ => bad.ffprobe.sha256 = "invalid".into(),
            }
            assert!(bad.validate_for(&request).is_err(), "case {case}");
        }
        request.expected_revision = None;
        assert!(request.validate().is_err());
        let mut raw = serde_json::to_value(good).unwrap();
        raw["path"] = "/untrusted/path".into();
        assert!(serde_json::from_value::<AssistantImageContext>(raw).is_err());
    }
    #[test]
    fn text_start_omits_new_fields_and_roundtrips_without_changing_bytes() {
        let mut spec = start();
        spec.image = None;
        spec.profile.vision_projector = None;
        let raw = serde_json::to_vec(&spec).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert!(value.get("image").is_none());
        assert!(value["profile"].get("vision_projector").is_none());
        let decoded: AssistantStart = serde_json::from_slice(&raw).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), raw);
    }
}
