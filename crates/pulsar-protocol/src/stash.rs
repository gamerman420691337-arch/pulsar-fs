//! Fixed Stash operations. Credentials are resolved by the engine, never sent
//! by a client or included in retained server responses.
use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashConnection {
    pub endpoint: String,
    /// Environment variable NAME only; the engine must already have its value.
    pub credential_env: Option<String>,
}
impl StashConnection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.endpoint.is_empty()
            || self.endpoint.len() > 4096
            || self.endpoint.chars().any(char::is_control)
        {
            return Err(ProtocolError::invalid("invalid Stash endpoint"));
        }
        if let Some(name) = &self.credential_env {
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(ProtocolError::invalid(
                    "Stash credential must be an environment variable name",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashVideoFile {
    pub id: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashScene {
    pub id: String,
    pub title: String,
    pub interactive: bool,
    pub files: Vec<StashVideoFile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashDiscovery {
    pub scenes: Vec<StashScene>,
    pub reported_count: u64,
    /// This is paginated discovery, not a transactional snapshot of Stash.
    pub pagination_complete: bool,
}

/// Explicit server POSIX prefix to host folder mapping. Discovery never infers
/// that a Stash server path belongs to the engine's filesystem namespace.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashRootMapping {
    pub server_root: String,
    pub local_root: std::path::PathBuf,
}
impl StashRootMapping {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let root = self.server_root.trim_end_matches('/');
        if self.server_root.len() > 4096
            || !self.server_root.starts_with('/')
            || self.server_root.contains('\\')
            || self.server_root.chars().any(char::is_control)
            || root
                .split('/')
                .skip(1)
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !self.local_root.is_absolute()
            || self.local_root.as_os_str().len() > 4096
            || self.local_root.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(ProtocolError::invalid(
                "Stash mapping requires normalized absolute server POSIX and local roots",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashFileStatus {
    ReadyForSnapshot,
    AlreadyInteractive,
    AmbiguousFiles,
    Unmapped,
    AmbiguousMapping,
    UnsafePath,
    UnavailableSource,
    ExistingScript,
    UnsupportedPlatform,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashFilePlan {
    pub scene_id: String,
    pub file_id: Option<String>,
    pub status: StashFileStatus,
    pub local_source: Option<std::path::PathBuf>,
    pub script_destination: Option<std::path::PathBuf>,
    pub observed_source_bytes: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashPlan {
    pub discovery: StashDiscovery,
    pub files: Vec<StashFilePlan>,
    /// Planning observes mutable filesystem state. Generation must independently
    /// snapshot inputs and publish with no-clobber semantics, then reconcile.
    pub planning_only: bool,
}

/// Engine-captured source identity and the separately named server/host paths.
/// This authorizes neither overwriting a script nor a Stash scan/tag effect.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashSourceCapture {
    /// Missing on legacy captures; publication must re-capture to bind an origin.
    #[serde(default)]
    pub endpoint: Option<String>,
    pub source_version: SourceVersionId,
    pub identity: ArtifactIdentity,
    pub scene_id: String,
    pub file_id: String,
    pub server_path: String,
    pub local_source: std::path::PathBuf,
    pub script_destination: std::path::PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashScriptExport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_proof: Option<StashTimingProof>,
    #[serde(default)]
    pub publication_request_id: Option<RequestId>,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub candidate_id: CandidateId,
    pub job_id: JobId,
    pub source: StashSourceCapture,
    pub path: std::path::PathBuf,
    pub identity: ArtifactIdentity,
    pub unresolved_review_count: u32,
    pub review_state_unknown: bool,
    pub release_qualified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashTimingProof {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_context: Option<ArchivedPublicationContext>,
    pub identity: ArtifactIdentity,
    pub path: std::path::PathBuf,
    pub mapped_actions: u64,
    pub reused_sample_clocks: u64,
    pub master_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedPublicationContext {
    pub project_id: ProjectId,
    pub origin: crate::ArchivedGenerationOrigin,
    pub master: crate::ArchivedMasterDerivation,
    pub source_time: bool,
    pub sampled_clocks_verified: bool,
    pub archival: bool,
    pub active_job: bool,
    pub release_qualified: bool,
}
impl ArchivedPublicationContext {
    pub fn validate(&self) -> Result<(), crate::ProtocolError> {
        if !self.archival
            || self.active_job
            || self.release_qualified
            || self.sampled_clocks_verified != self.origin.clock.is_some()
            || self.source_time != self.sampled_clocks_verified
            || (!self.source_time && !self.master.retiming_revisions.is_empty())
        {
            return Err(crate::ProtocolError::invalid(
                "archival publication cannot activate jobs or assert unavailable sampled clocks",
            ));
        }
        crate::ArchivedGenerationReport {
            project_id: self.project_id.clone(),
            candidate_id: self.master.anchor_candidate.clone(),
            origins: vec![self.origin.clone()],
            master_derivation: Some(self.master.clone()),
            archival: true,
            active_job: false,
            publication_authority: false,
            release_qualified: false,
        }
        .validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashPublicationDispatch {
    Pending,
    FailedOrUnknown,
    Complete,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashArtifactObservation {
    Verified,
    Unverified,
    NotRecorded,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashPublicationObservation {
    pub publication_request_id: RequestId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub dispatch: StashPublicationDispatch,
    pub path: std::path::PathBuf,
    pub expected_script_sha256: String,
    pub timing_proof: Option<StashTimingProof>,
    pub proof_observation: StashArtifactObservation,
    pub source_and_script_observation: StashArtifactObservation,
    pub observed_at_unix_ms: u64,
    pub completed_receipt: Option<StashScriptExport>,
    pub release_qualified: bool,
}

/// Scan completion and scene attachment are separate observations.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashScanState {
    DispatchUnknown,
    Submitted,
    Ready,
    Running,
    Stopping,
    Cancelled,
    Failed,
    JobMissing,
    FinishedUnattached,
    VerifiedLinked,
}
/// Point-in-time attachment observation; it does not recover a remote job identity.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashAttachmentState {
    ObservedLinked,
    Unattached,
    IdentityChanged,
    LocalPublicationUnverified,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashAttachmentObservation {
    pub state: StashAttachmentState,
    pub observed_at_unix_ms: u64,
    pub remote_script_hash_verified: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashScanReceipt {
    pub request_id: RequestId,
    pub publication_request_id: RequestId,
    pub project_id: ProjectId,
    pub publication: StashScriptExport,
    pub remote_job_id: Option<String>,
    pub state: StashScanState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<StashAttachmentObservation>,
    pub release_qualified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StashTagState {
    CreationUnknown,
    Resolved,
    AttachUnknown,
    AttachedUnverified,
    VerifiedAdded,
    TagMissing,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashTagReceipt {
    pub request_id: RequestId,
    pub scan_request_id: RequestId,
    pub project_id: ProjectId,
    pub publication: StashScriptExport,
    pub remote_job_id: String,
    pub tag_name: String,
    pub tag_id: Option<String>,
    pub state: StashTagState,
    pub add_dispatched: bool,
    pub release_qualified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchTag {
    pub name: String,
    pub create: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchAnnotatedView {
    pub scene_id: String,
    pub file_id: String,
    pub view: AnnotatedGenerationView,
}
/// Batch templates omit the single-source view; every catalog entry resolves a
/// complete ordinary generation setting before dispatch.
pub fn validate_stash_batch_settings(
    settings: &GenerationSettings,
    views: &[StashBatchAnnotatedView],
) -> Result<(), ProtocolError> {
    if settings.annotated_view.is_some() {
        return Err(ProtocolError::invalid(
            "Stash batches require a per-source annotated view catalog",
        ));
    }
    if views.is_empty() {
        return settings.validate();
    }
    if views.len() > 100
        || settings.source_placement.is_some()
        || settings.project_interval.is_some()
    {
        return Err(ProtocolError::invalid(
            "annotated batch views require at most 100 unplaced full-source entries",
        ));
    }
    let mut keys = std::collections::BTreeSet::new();
    for entry in views {
        if [&entry.scene_id, &entry.file_id].iter().any(|id| {
            id.is_empty()
                || id.len() > 256
                || id.trim() != id.as_str()
                || id.chars().any(char::is_control)
        }) || !keys.insert((&entry.scene_id, &entry.file_id))
        {
            return Err(ProtocolError::invalid(
                "annotated batch scene/file keys must be bounded and unique",
            ));
        }
        let mut resolved = settings.clone();
        resolved.annotated_view = Some(entry.view.clone());
        resolved.validate()?;
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_plans: Vec<StashBatchSourcePlan>,
    pub connection: StashConnection,
    pub mappings: Vec<StashRootMapping>,
    pub limit: u32,
    pub preset: String,
    pub settings: GenerationSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotated_views: Vec<StashBatchAnnotatedView>,
    pub model_path: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<ModelPackChoice>,
    pub scan: bool,
    pub tag: Option<StashBatchTag>,
}
impl StashBatchOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.connection.validate()?;
        validate_stash_batch_source_plans(
            &self.settings,
            &self.annotated_views,
            &self.source_plans,
        )?;
        if !self.annotated_views.is_empty()
            && (self.model_path.is_none() && self.model_pack.is_none())
        {
            return Err(ProtocolError::invalid(
                "annotated Stash batches require an explicit file model or selected pack",
            ));
        }
        if self.limit == 0
            || self.limit > 100
            || self.mappings.is_empty()
            || self.mappings.len() > 16
        {
            return Err(ProtocolError::invalid(
                "Stash batch requires 1..=100 scenes and 1..=16 mappings",
            ));
        }
        for mapping in &self.mappings {
            mapping.validate()?;
        }
        if !matches!(
            self.preset.as_str(),
            "default" | "fast" | "maximum" | "quality"
        ) {
            return Err(ProtocolError::unsupported("Stash batch preset"));
        }
        if let Some(choice) = &self.model_pack {
            choice.validate()?;
            if self.model_path.is_some() {
                return Err(ProtocolError::invalid(
                    "Stash pack choice conflicts with a file model",
                ));
            }
        }
        if self.settings.inference_profile.is_some()
            && self.model_path.is_none()
            && self.model_pack.is_none()
        {
            return Err(ProtocolError::invalid(
                "Stash inference profile requires an explicit model or pack",
            ));
        }
        if let Some(tag) = &self.tag {
            if !self.scan
                || tag.name.is_empty()
                || tag.name.len() > 256
                || tag.name.trim() != tag.name
                || tag.name.chars().any(char::is_control)
            {
                return Err(ProtocolError::invalid(
                    "batch tag requires scan and an exact nonempty trimmed tag name",
                ));
            }
        }
        Ok(())
    }
}
/// Full replacement generation intent. None explicitly selects no custom model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchGeneration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_plans: Vec<StashBatchSourcePlan>,
    pub preset: String,
    pub settings: GenerationSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotated_views: Vec<StashBatchAnnotatedView>,
    #[serde(deserialize_with = "required_model_path")]
    pub model_path: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<ModelPackChoice>,
}
fn required_model_path<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<std::path::PathBuf>, D::Error> {
    Option::<std::path::PathBuf>::deserialize(deserializer)
}
impl StashBatchGeneration {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_stash_batch_source_plans(
            &self.settings,
            &self.annotated_views,
            &self.source_plans,
        )?;
        if !self.annotated_views.is_empty()
            && (self.model_path.is_none() && self.model_pack.is_none())
        {
            return Err(ProtocolError::invalid(
                "annotated Stash migration requires an explicit file model or selected pack",
            ));
        }
        if !matches!(
            self.preset.as_str(),
            "default" | "fast" | "maximum" | "quality"
        ) {
            return Err(ProtocolError::unsupported("Stash migration preset"));
        }
        if self
            .model_path
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(ProtocolError::invalid(
                "migration model path must be absolute",
            ));
        }
        if let Some(choice) = &self.model_pack {
            choice.validate()?;
            if self.model_path.is_some() {
                return Err(ProtocolError::invalid(
                    "migration pack choice conflicts with a file model",
                ));
            }
        }
        if self.settings.inference_profile.is_some()
            && self.model_path.is_none()
            && self.model_pack.is_none()
        {
            return Err(ProtocolError::invalid(
                "migration inference profile requires a model or pack",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchMigrationReceipt {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub superseded_placement_requests: Vec<RequestId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reconciled_placement_requests: Vec<RequestId>,
    pub request_id: RequestId,
    pub previous_ledger_sha256: String,
    pub previous_options_sha256: String,
    pub current_options_sha256: String,
    pub previous_worker_sha256: String,
    pub current_worker_sha256: String,
    pub previous_model_sha256: Option<String>,
    pub current_model_sha256: Option<String>,
    pub superseded_generation_requests: Vec<RequestId>,
    pub reconciled_generation_requests: Vec<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_model_pack: Option<ModelPackChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_model_pack: Option<ModelPackChoice>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchEffect {
    pub phase: String,
    pub request_id: RequestId,
    pub expected_revision: Option<RevisionId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchItem {
    pub scene_id: String,
    pub file_id: Option<String>,
    pub state: String,
    pub eligibility: StashFileStatus,
    pub project_id: Option<ProjectId>,
    pub source_version: Option<SourceVersionId>,
    pub job_id: Option<JobId>,
    pub candidate_id: Option<CandidateId>,
    pub requests: Vec<StashBatchEffect>,
    pub export: Option<StashScriptExport>,
    pub scan: Option<StashScanReceipt>,
    pub tag: Option<StashTagReceipt>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchReport {
    pub schema_version: u16,
    pub batch_request_id: RequestId,
    pub state: String,
    pub item_count: u32,
    pub offset: u32,
    pub has_more: bool,
    pub pagination_complete: bool,
    pub items: Vec<StashBatchItem>,
    pub release_qualified: bool,
    pub stash_mutations_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrations: Vec<StashBatchMigrationReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<ModelPackChoice>,
}
impl StashBatchReport {
    pub fn has_failures(&self) -> bool {
        self.items.iter().any(|item| item.error.is_some())
            || matches!(self.state.as_str(), "needs_attention" | "cancelled")
    }
}

#[cfg(test)]
mod source_clock_wire_tests {
    use super::*;
    #[test]
    fn migration_requires_explicit_model_selection_and_valid_generation() {
        let mut value =
            serde_json::json!({"preset":"fast","settings":GenerationSettings::default()});
        assert!(serde_json::from_value::<StashBatchGeneration>(value.clone()).is_err());
        value["model_path"] = serde_json::Value::Null;
        serde_json::from_value::<StashBatchGeneration>(value.clone())
            .unwrap()
            .validate()
            .unwrap();
        value["model_path"] = serde_json::json!("relative.onnx");
        assert!(serde_json::from_value::<StashBatchGeneration>(value)
            .unwrap()
            .validate()
            .is_err());
    }
    #[test]
    fn legacy_publication_omits_default_clock_intent_and_explicit_intent_is_distinct() {
        let old = serde_json::json!({"operation":"stash_export_script","arguments":{"connection":{"endpoint":"http://localhost:9999/graphql","credential_env":null},"source_version":"source","scene_id":"scene","file_id":"file","candidate_id":"candidate","job_id":"job","owner_proof":"fixture"}});
        let command: Command = serde_json::from_value(old.clone()).unwrap();
        assert!(matches!(
            command,
            Command::StashExportScript {
                source_time: false,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(&command).unwrap(), old);
        let mut explicit = old.clone();
        explicit["arguments"]["source_time"] = serde_json::json!(true);
        let command: Command = serde_json::from_value(explicit.clone()).unwrap();
        assert!(matches!(
            command,
            Command::StashExportScript {
                source_time: true,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(command).unwrap(), explicit);
    }
}

#[cfg(test)]
mod tier_name_tests {
    use super::*;
    #[test]
    fn batch_and_migration_preserve_tier_names_and_defer_model_capture() {
        let mut migration = StashBatchGeneration {
            source_plans: vec![],
            preset: "maximum".into(),
            settings: GenerationSettings::default(),
            annotated_views: vec![],
            model_path: None,
            model_pack: None,
        };
        let mut batch = StashBatchOptions {
            source_plans: vec![],
            connection: StashConnection {
                endpoint: "http://127.0.0.1:9999/graphql".into(),
                credential_env: None,
            },
            mappings: vec![StashRootMapping {
                server_root: "/server".into(),
                local_root: crate::test_host_path("/local").into(),
            }],
            limit: 1,
            preset: "maximum".into(),
            settings: GenerationSettings::default(),
            annotated_views: vec![],
            model_path: None,
            model_pack: None,
            scan: false,
            tag: None,
        };
        for preset in ["fast", "default", "maximum", "quality"] {
            batch.preset = preset.into();
            batch.validate().unwrap();
            let encoded = serde_json::to_vec(&batch).unwrap();
            let restored: StashBatchOptions = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(restored.preset, preset);
            migration.preset = preset.into();
            migration.validate().unwrap();
            let encoded = serde_json::to_vec(&migration).unwrap();
            let restored: StashBatchGeneration = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(restored.preset, preset);
        }
        migration.preset = "economy".into();
        assert_eq!(
            migration.validate().unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}

#[cfg(test)]
mod explicit_multifile_request_tests {
    use super::*;

    fn request(allow: Option<bool>, expected: Option<&str>) -> Request {
        Request::new(
            RequestId::new("explicit-stash-source").unwrap(),
            Command::StashImportSource {
                connection: StashConnection {
                    endpoint: "http://127.0.0.1:9999/graphql".into(),
                    credential_env: None,
                },
                scene_id: "scene".into(),
                file_id: "file".into(),
                expected_server_path: expected.map(str::to_owned),
                expected_source_version: None,
                allow_multifile_selection: allow,
                mappings: vec![StashRootMapping {
                    server_root: "/server".into(),
                    local_root: crate::test_host_path("/local").into(),
                }],
                owner_proof: "owner".into(),
            },
        )
        .in_project(ProjectId::new("project").unwrap(), Some(RevisionId::new(0)))
        .with_session(SessionId::new("session").unwrap())
        .with_auth_token("synthetic-token".into())
    }

    #[allow(dead_code)]
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct LegacyImportArguments {
        connection: serde_json::Value,
        scene_id: String,
        file_id: String,
        #[serde(default)]
        expected_server_path: Option<String>,
        #[serde(default)]
        expected_source_version: Option<serde_json::Value>,
        mappings: Vec<serde_json::Value>,
        owner_proof: String,
    }

    #[test]
    fn explicit_multifile_wire_is_opt_in_and_requires_expected_path() {
        let legacy = request(None, None);
        legacy.validate().unwrap();
        let value = serde_json::to_value(&legacy.command).unwrap();
        assert!(value["arguments"]
            .get("allow_multifile_selection")
            .is_none());
        assert!(
            serde_json::from_value::<LegacyImportArguments>(value["arguments"].clone()).is_ok()
        );
        let encoded = serde_json::to_vec(&legacy).unwrap();
        let decoded: Request = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        assert!(request(Some(true), None).validate().is_err());
        assert!(request(Some(false), Some("/server/video.mp4"))
            .validate()
            .is_err());
        let explicit = request(Some(true), Some("/server/video.mp4"));
        explicit.validate().unwrap();
        let value = serde_json::to_value(&explicit.command).unwrap();
        assert_eq!(value["arguments"]["allow_multifile_selection"], true);
        assert!(
            serde_json::from_value::<LegacyImportArguments>(value["arguments"].clone()).is_err()
        );
    }
}
