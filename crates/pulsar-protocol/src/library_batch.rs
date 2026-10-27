//! Explicit local intake and resumable generation, separate from publication.
use crate::*;
use std::{collections::BTreeSet, path::PathBuf};
pub const MAX_LIBRARY_BATCH_ITEMS: usize = 100;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationDirectoryIdentity {
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchInput {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Explicit audio publication subset; the synthesis candidate still contains six axes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_axes: Option<Vec<Axis>>,
    /// Existing output directory; each selected leaf is published without replacement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_directory: Option<PublicationDirectoryIdentity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchOptions {
    pub inputs: Vec<LibraryBatchInput>,
    pub recipe: LibraryBatchRecipe,
}
/// Audio remains wire-compatible with its original untagged recipe object.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LibraryBatchRecipe {
    Audio(AudioBundleRecipe),
    Visual(Box<LibraryVisualRecipe>),
}
impl From<AudioBundleRecipe> for LibraryBatchRecipe {
    fn from(recipe: AudioBundleRecipe) -> Self {
        Self::Audio(recipe)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryVisualKind {
    Visual,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryVisualRecipe {
    pub kind: LibraryVisualKind,
    pub preset: String,
    pub settings: GenerationSettings,
    pub model_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<ModelPackChoice>,
}
impl LibraryVisualRecipe {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(choice) = &self.model_pack {
            choice.validate()?;
            if self.model_path.is_some() {
                return Err(ProtocolError::invalid(
                    "Choose a file model or selected pack",
                ));
            }
            // The Engine fills the selected profile before execution-preset validation.
            self.settings.validate()?;
            if !["default", "fast", "maximum", "quality"].contains(&self.preset.as_str()) {
                return Err(ProtocolError::invalid("Unsupported visual preset"));
            }
        } else {
            self.settings
                .validate_execution_preset(&self.preset, self.model_path.is_some())?;
        }
        if let Some(path) = &self.model_path {
            let b = path.as_os_str().as_encoded_bytes();
            if !path.is_absolute()
                || b.len() > 32768
                || b.contains(&0)
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ProtocolError::invalid(
                    "Batch model requires bounded absolute path without traversal",
                ));
            }
        }
        Ok(())
    }
}
impl LibraryBatchRecipe {
    pub fn model_pack(&self) -> Option<&ModelPackChoice> {
        match self {
            Self::Visual(recipe) => recipe.model_pack.as_ref(),
            Self::Audio(_) => None,
        }
    }
}
impl LibraryBatchOptions {
    pub fn publication_schema(&self, cancelled: bool) -> u16 {
        if self.recipe.model_pack().is_some() {
            4
        } else if matches!(self.recipe, LibraryBatchRecipe::Visual(_)) {
            3
        } else if self
            .inputs
            .iter()
            .any(|input| input.publication_directory.is_some())
        {
            6
        } else if self
            .inputs
            .iter()
            .any(|input| input.publication_axes.is_some())
        {
            5
        } else if cancelled {
            2
        } else {
            1
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let LibraryBatchRecipe::Visual(recipe) = &self.recipe {
            recipe.validate()?;
        }
        if self.inputs.is_empty() || self.inputs.len() > MAX_LIBRARY_BATCH_ITEMS {
            return Err(ProtocolError::invalid(
                "Local batch requires 1..=100 selected sources",
            ));
        }
        let mut sources = BTreeSet::new();
        let mut destinations = BTreeSet::new();
        for item in &self.inputs {
            if item.publication_directory.is_some_and(|id| id.inode == 0)
                || (item.publication_directory.is_some()
                    && !matches!(self.recipe, LibraryBatchRecipe::Audio(_)))
            {
                return Err(ProtocolError::invalid(
                    "Existing publication directories require audio and positive inode identity",
                ));
            }
            if let Some(axes) = &item.publication_axes {
                if !matches!(self.recipe, LibraryBatchRecipe::Audio(_))
                    || !(valid_audio_publication_subset(axes)
                        || (item.publication_directory.is_some() && axes == &Axis::ALL))
                {
                    return Err(ProtocolError::invalid("Audio publication requires one through five distinct axes in neutral order"));
                }
            }
            for path in [&item.source, &item.destination] {
                let b = path.as_os_str().as_encoded_bytes();
                if !path.is_absolute()
                    || b.len() > 32768
                    || b.contains(&0)
                    || path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(ProtocolError::invalid(
                        "Batch paths require bounded absolute paths without parent traversal",
                    ));
                }
            }
            if item.source == item.destination
                || item.destination.file_name().is_none()
                || !sources.insert(&item.source)
                || !destinations.insert(&item.destination)
            {
                return Err(ProtocolError::invalid(
                    "Batch source/destination selections must be distinct and unique",
                ));
            }
        }
        if self.inputs.iter().any(|i| sources.contains(&i.destination)) {
            return Err(ProtocolError::invalid(
                "Batch destination conflicts with selected source",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryBatchPhase {
    Selected,
    ProjectCreated,
    SourceImported,
    Generating,
    CandidateReady,
    ReadyForPublication,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchEffect {
    pub request_id: RequestId,
    pub phase: LibraryBatchPhase,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchItem {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub source_sha256: String,
    pub source_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_axes: Option<Vec<Axis>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_directory: Option<PublicationDirectoryIdentity>,
    pub phase: LibraryBatchPhase,
    pub project_id: Option<ProjectId>,
    pub source_version: Option<SourceVersionId>,
    pub revision: RevisionId,
    pub job_id: Option<JobId>,
    pub candidate_id: Option<CandidateId>,
    pub effects: Vec<LibraryBatchEffect>,
    pub error: Option<ProtocolError>,
}
impl LibraryBatchItem {
    pub fn publication_axes(&self) -> &[Axis] {
        self.publication_axes.as_deref().unwrap_or(&Axis::ALL)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchReport {
    pub schema: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<Box<LibraryBatchMigration>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_batch_request_id: Option<Box<RequestId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<Box<ModelPackChoice>>,
    pub batch_request_id: RequestId,
    pub items: Vec<LibraryBatchItem>,
    pub pending_advance_request_id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation: Option<LibraryBatchCancellation>,
    pub generation_ready: bool,
    pub publication_completed: bool,
    pub qualified: bool,
}

impl LibraryBatchReport {
    pub fn is_visual(&self) -> bool {
        matches!(self.schema, 3 | 4)
    }
    pub fn validate_publication_shape(&self) -> Result<(), ProtocolError> {
        if self.schema != 6
            && self
                .items
                .iter()
                .any(|item| item.publication_directory.is_some())
        {
            return Err(ProtocolError::invalid(
                "Existing directory requires partial-audio schema",
            ));
        }
        let valid = match self.schema {
            1 | 2 => {
                self.model_pack.is_none()
                    && self.items.iter().all(|i| i.publication_axes.is_none())
                    && ((self.schema == 2) == self.cancellation.is_some())
            }
            3 | 4 => {
                (self.model_pack.is_some() == (self.schema == 4))
                    && self
                        .items
                        .iter()
                        .all(|i| i.publication_axes.as_deref() == Some(&[Axis::Stroke]))
            }
            5 => {
                self.model_pack.is_none()
                    && self.items.iter().any(|i| i.publication_axes.is_some())
                    && self.items.iter().all(|i| {
                        i.publication_axes
                            .as_deref()
                            .is_none_or(valid_audio_publication_subset)
                    })
            }
            6 => {
                self.model_pack.is_none()
                    && self
                        .items
                        .iter()
                        .any(|item| item.publication_directory.is_some())
                    && self.items.iter().all(|item| {
                        item.publication_directory.is_none_or(|id| id.inode > 0)
                            && item.publication_axes.as_deref().is_none_or(|axes| {
                                valid_audio_publication_subset(axes)
                                    || (item.publication_directory.is_some() && axes == Axis::ALL)
                            })
                    })
            }
            _ => false,
        };
        if !valid {
            return Err(ProtocolError::invalid(
                "Batch publication schema/axis shape differs",
            ));
        }
        Ok(())
    }
    pub fn report_sha256(&self) -> Result<String, ProtocolError> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("Batch report cannot be encoded"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn validate_migration(&self) -> Result<(), ProtocolError> {
        if self.successor_batch_request_id.as_deref() == Some(&self.batch_request_id)
            || (self.successor_batch_request_id.is_some()
                && (self.cancellation.is_some() || self.pending_advance_request_id.is_some()))
        {
            return Err(ProtocolError::invalid(
                "Migrated original report shape differs",
            ));
        }
        if let Some(m) = &self.migration {
            if m.parent_batch_request_id == self.batch_request_id
                || m.parent_report_sha256.len() != 64
                || !m
                    .parent_report_sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(ProtocolError::invalid("Batch migration lineage differs"));
            }
        }
        Ok(())
    }
}

pub fn valid_audio_publication_subset(axes: &[Axis]) -> bool {
    !axes.is_empty()
        && axes.len() < Axis::ALL.len()
        && Axis::ALL
            .into_iter()
            .filter(|axis| axes.contains(axis))
            .collect::<Vec<_>>()
            == axes
}

/// Explicit continuation lineage; accepted work remains on the original batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchMigration {
    pub parent_batch_request_id: RequestId,
    pub parent_report_sha256: String,
}

/// Durable stop policy and exact ordinary cancellation acknowledgements; not physical retirement proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchCancellation {
    pub request_id: RequestId,
    pub jobs: Vec<LibraryBatchCancelEffect>,
    pub resolved_generation_advance_request_id: Option<RequestId>,
    pub completed: bool,
    pub worker_retirement_verified: bool,
    pub error: Option<ProtocolError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchCancelEffect {
    pub request_id: RequestId,
    pub project_id: ProjectId,
    pub job_id: JobId,
    pub base_revision: RevisionId,
    pub acknowledgement: Option<JobSnapshot>,
}
fn is_false(value: &bool) -> bool {
    !*value
}

/// Replayable metadata release acknowledgement; no filesystem deletion or publication claim.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchReleaseReceipt {
    pub report: LibraryBatchReport,
    pub released_source_memberships: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_batch_request_id: Option<Box<RequestId>>,
    pub batch_request_id: RequestId,
    pub first_source: PathBuf,
    pub item_count: u16,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancellation_requested: bool,
    pub generation_ready: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryBatchCatalog {
    pub batches: Vec<LibraryBatchSummary>,
    pub qualified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> LibraryBatchOptions {
        let root = std::env::temp_dir().join("authored-local-batch");
        let recipe: AudioBundleRecipe=serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap();
        LibraryBatchOptions {
            inputs: vec![LibraryBatchInput {
                publication_directory: None,
                publication_axes: None,
                source: root.join("input.wav"),
                destination: root.join("output"),
            }],
            recipe: recipe.into(),
        }
    }
    #[test]
    fn selected_audio_axes_are_canonical_bounded_and_wire_distinct_from_visual() {
        let mut options = options();
        assert!(serde_json::to_value(&options).unwrap()["inputs"][0]
            .get("publication_axes")
            .is_none());
        options.inputs[0].publication_axes = Some(Axis::ALL[1..].to_vec());
        options.validate().unwrap();
        assert_eq!(options.publication_schema(false), 5);
        assert_eq!(options.publication_schema(true), 5);
        for axes in [
            vec![],
            Axis::ALL.to_vec(),
            vec![Axis::Roll, Axis::Roll],
            vec![Axis::Roll, Axis::Stroke],
        ] {
            options.inputs[0].publication_axes = Some(axes);
            assert!(options.validate().is_err());
        }
    }
    #[test]
    fn existing_directory_mode_requires_positive_identity_and_pinned_exports_require_revision() {
        let mut options = options();
        options.inputs[0].publication_directory = Some(PublicationDirectoryIdentity {
            device: 1,
            inode: 0,
        });
        assert!(options.validate().is_err());
        options.inputs[0]
            .publication_directory
            .as_mut()
            .unwrap()
            .inode = 2;
        options.inputs[0].publication_axes = Some(Axis::ALL.to_vec());
        options.validate().unwrap();
        assert_eq!(options.publication_schema(false), 6);
        let mut request = Request::new(
            RequestId::new("pinned-export").unwrap(),
            Command::ExportAxisPinned {
                path: std::path::PathBuf::from("/fixture/roll.funscript"),
                axis: Axis::Roll,
                directory_identity: PublicationDirectoryIdentity {
                    device: 1,
                    inode: 2,
                },
            },
        );
        request.project = Some(ProjectId::new("project").unwrap());
        assert!(request.validate().is_err());
        request.expected_revision = Some(RevisionId::new(1));
        request.validate().unwrap();
    }
    #[test]
    fn audio_wire_is_preserved_and_visual_recipes_require_explicit_checked_contracts() {
        let mut opts = options();
        let wire = serde_json::to_value(&opts).unwrap();
        assert!(wire["recipe"]["channels"].is_array());
        assert!(wire["recipe"].get("kind").is_none());
        let recipe = LibraryVisualRecipe {
            kind: LibraryVisualKind::Visual,
            preset: "default".into(),
            settings: GenerationSettings::default(),
            model_pack: None,
            model_path: None,
        };
        opts.recipe = LibraryBatchRecipe::Visual(Box::new(recipe.clone()));
        opts.validate().unwrap();
        let mut changed = recipe;
        changed.model_path = Some(std::env::temp_dir().join("detector.onnx"));
        assert!(changed.validate().is_err());
        let mut invalid = serde_json::to_value(&opts).unwrap();
        invalid["recipe"]["ignored_switch"] = true.into();
        assert!(serde_json::from_value::<LibraryBatchOptions>(invalid).is_err());
    }
    #[test]
    fn selected_pack_recipe_preserves_exact_fence_and_refuses_file_ambiguity() {
        let mut recipe = LibraryVisualRecipe {
            kind: LibraryVisualKind::Visual,
            preset: "maximum".into(),
            settings: GenerationSettings::default(),
            model_path: None,
            model_pack: Some(ModelPackChoice {
                manifest_sha256: "a".repeat(64),
                selection_revision: 7,
            }),
        };
        recipe.validate().unwrap();
        let wire = serde_json::to_value(&recipe).unwrap();
        let parsed: LibraryVisualRecipe = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(parsed.model_pack, recipe.model_pack);
        recipe.model_path = Some(std::env::temp_dir().join("model.onnx"));
        assert!(recipe.validate().is_err());
        recipe.model_path = None;
        recipe.model_pack.as_mut().unwrap().selection_revision = 0;
        assert!(recipe.validate().is_err());
        assert_eq!(wire["model_pack"]["selection_revision"], 7);
    }
    #[test]
    fn local_controls_are_global_but_only_status_is_readonly() {
        let id = RequestId::new("batch").unwrap();
        for command in [
            Command::LibraryBatchList,
            Command::LibraryBatchRelease {
                batch_request_id: id.clone(),
            },
            Command::LibraryBatchPrepare { options: options() },
            Command::LibraryBatchMigrate {
                batch_request_id: id.clone(),
                expected_report_sha256: "a".repeat(64),
                recipe: options().recipe,
            },
            Command::LibraryBatchAdvance {
                batch_request_id: id.clone(),
            },
            Command::LibraryBatchStatus {
                batch_request_id: id,
            },
        ] {
            assert!(!command.requires_project());
            assert_eq!(
                command.is_mutating(),
                !matches!(
                    command,
                    Command::LibraryBatchStatus { .. } | Command::LibraryBatchList
                )
            );
            Request::new(RequestId::new("request").unwrap(), command)
                .validate()
                .unwrap();
        }
    }
    #[test]
    fn intake_refuses_duplicate_crossed_relative_and_over_capacity_selections() {
        let mut opts = options();
        opts.validate().unwrap();
        opts.inputs.push(opts.inputs[0].clone());
        assert!(opts.validate().is_err());
        let mut opts = options();
        opts.inputs[0].destination = opts.inputs[0].source.clone();
        assert!(opts.validate().is_err());
        let mut opts = options();
        opts.inputs[0].source = PathBuf::from("relative.wav");
        assert!(opts.validate().is_err());
        let mut opts = options();
        opts.inputs = vec![];
        assert!(opts.validate().is_err());
        let mut opts = options();
        opts.inputs = vec![opts.inputs[0].clone(); MAX_LIBRARY_BATCH_ITEMS + 1];
        assert!(opts.validate().is_err());
        let mut wire = serde_json::to_value(options()).unwrap();
        wire["allow_overwrite"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LibraryBatchOptions>(wire).is_err());
    }
    #[test]
    fn pinned_import_requires_project_and_canonical_positive_identity() {
        let path = std::env::temp_dir().join("captured-input");
        let id = RequestId::new("request").unwrap();
        let project = ProjectId::new("project").unwrap();
        let valid = Command::ImportSourcePinned {
            path: path.clone(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        assert!(Request::new(id.clone(), valid.clone()).validate().is_err());
        Request::new(id.clone(), valid)
            .in_project(project.clone(), Some(RevisionId::new(0)))
            .validate()
            .unwrap();
        for (sha, bytes) in [
            ("A".repeat(64), 1),
            ("a".repeat(63), 1),
            ("a".repeat(64), 0),
        ] {
            assert!(Request::new(
                id.clone(),
                Command::ImportSourcePinned {
                    path: path.clone(),
                    sha256: sha,
                    byte_len: bytes
                }
            )
            .in_project(project.clone(), Some(RevisionId::new(0)))
            .validate()
            .is_err());
        }
    }
    #[test]
    fn migration_requires_exact_report_digest() {
        let mut command = Command::LibraryBatchMigrate {
            batch_request_id: RequestId::new("original").unwrap(),
            expected_report_sha256: "a".repeat(64),
            recipe: options().recipe,
        };
        let request = |command| Request::new(RequestId::new("migration").unwrap(), command);
        request(command.clone()).validate().unwrap();
        if let Command::LibraryBatchMigrate {
            expected_report_sha256,
            ..
        } = &mut command
        {
            *expected_report_sha256 = "A".repeat(64);
        }
        assert!(request(command).validate().is_err());
    }
}
