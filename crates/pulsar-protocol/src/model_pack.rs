//! Offline byte-integrity contracts. Pack declarations never approve native
//! execution, redistribution, model quality or provider compatibility.
use crate::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelPackPreviewTime {
    Source {
        source_time: SourceTimestamp,
        time_basis: PreviewTimeBasis,
    },
    Placement {
        placement_id: SourcePlacementId,
        project_time: ProjectTime,
    },
}
impl ModelPackPreviewTime {
    pub fn validate(&self, context: &FrameContext) -> Result<(), ProtocolError> {
        match self {
            Self::Source {
                source_time,
                time_basis,
            } if *time_basis == PreviewTimeBasis::ElapsedFromFirstFrame
                && source_time.numerator() < 0 =>
            {
                Err(ProtocolError::invalid(
                    "elapsed preview time must be nonnegative",
                ))
            }
            Self::Placement { placement_id, .. } if placement_id != &context.source_placement => {
                Err(ProtocolError::invalid(
                    "pack preview context differs from requested placement",
                ))
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackPreviewResult {
    pub choice: ModelPackChoice,
    pub model: ArtifactIdentity,
    pub profile_sha256: String,
    pub preview: PreviewResult,
    pub qualified: bool,
}
impl ModelPackPreviewResult {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.choice.validate()?;
        self.model.validate()?;
        crate::validate_sha256(&self.profile_sha256)?;
        if self
            .preview
            .annotated
            .as_ref()
            .is_some_and(|e| e.model != self.model || e.profile.sha256 != self.profile_sha256)
        {
            return Err(ProtocolError::invalid(
                "annotated pack preview wrapper differs from model/profile evidence",
            ));
        }
        if self.qualified || !self.preview.matches_request(&self.preview.context) {
            return Err(ProtocolError::invalid(
                "invalid or falsely qualified model-pack preview",
            ));
        }
        Ok(())
    }
}

/// An optimistic selection fence. The host-local owner approves each change;
/// a pack import or a portable project never grants this authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackChoice {
    pub manifest_sha256: String,
    pub selection_revision: u64,
}
impl ModelPackChoice {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::validate_sha256(&self.manifest_sha256)?;
        if self.selection_revision == 0 || self.selection_revision > i64::MAX as u64 {
            return Err(ProtocolError::invalid(
                "invalid model-pack selection revision",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogBinding {
    pub bootstrap_sha256: String,
    pub catalog_revision: u64,
}
impl ModelCatalogBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::validate_sha256(&self.bootstrap_sha256)?;
        if self.catalog_revision == 0 || self.catalog_revision > i64::MAX as u64 {
            return Err(ProtocolError::invalid("invalid catalog approval binding"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackSelectionReport {
    pub schema_version: u16,
    pub selection_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_binding: Option<ModelCatalogBinding>,
    pub choice: Option<ModelPackChoice>,
    pub selected_by: Option<SessionId>,
    pub selection_request: Option<RequestId>,
    pub profile: Option<DeclaredInferenceProfile>,
    /// Explicit approval for future offline admission through the pinned runtime.
    pub native_execution_approved: bool,
    pub rights_verified: bool,
    pub compatible: bool,
    pub qualified: bool,
}
impl ModelPackSelectionReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != if self.catalog_binding.is_some() { 2 } else { 1 }
            || self.selection_revision > i64::MAX as u64
            || self.rights_verified
            || self.compatible
            || self.qualified
            || self.native_execution_approved != self.choice.is_some()
            || self.profile.is_some() != self.choice.is_some()
            || self.selected_by.is_some() != (self.selection_revision > 0)
            || self.selection_request.is_some() != (self.selection_revision > 0)
        {
            return Err(ProtocolError::invalid(
                "invalid model-pack selection report",
            ));
        }
        if let Some(binding) = &self.catalog_binding {
            binding.validate()?;
            if self.choice.is_none() {
                return Err(ProtocolError::invalid(
                    "catalog binding requires an active recorded choice",
                ));
            }
        }
        if let Some(choice) = &self.choice {
            choice.validate()?;
            if choice.selection_revision != self.selection_revision {
                return Err(ProtocolError::invalid(
                    "model-pack selection fence mismatch",
                ));
            }
        }
        if let Some(profile) = &self.profile {
            profile.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackAssetRole {
    Model,
    Runtime,
    Profile,
    License,
    Notice,
    NativeDependency,
    Fixture,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackAsset {
    pub path: String,
    pub role: PackAssetRole,
    pub byte_len: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackManifest {
    pub schema_version: u16,
    pub pack_id: ArtifactId,
    pub target_os: String,
    pub target_arch: String,
    pub ort_api_version: u16,
    pub labels: Vec<String>,
    pub assets: Vec<PackAsset>,
    pub license_path: String,
    /// Supplier text is a declaration, not independently verified rights.
    pub source_description: String,
}
pub const PACK_MANIFEST_LIMIT: usize = 128 * 1024;
pub const PACK_TOTAL_LIMIT: u64 = 512 * 1024 * 1024;
pub fn validate_pack_path(path: &str) -> Result<(), ProtocolError> {
    if path.is_empty()
        || path.len() > 240
        || path.split('/').next().is_some_and(|root| {
            root.eq_ignore_ascii_case("manifest.json")
                || root.eq_ignore_ascii_case("receipt.json")
                || root.eq_ignore_ascii_case(".pulsar-stage.json")
        })
    {
        return Err(ProtocolError::invalid("invalid/reserved pack asset path"));
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > 128
            || part.ends_with('.')
            || !part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(ProtocolError::invalid(
                "pack assets require portable relative ASCII paths",
            ));
        }
    }
    Ok(())
}

pub fn validate_stage_basename(name: &str) -> Result<(), ProtocolError> {
    if !name.starts_with(".pack-stage-")
        || name.len() > 128
        || name.len() == 12
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        return Err(ProtocolError::invalid("invalid staging basename"));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackDiscardOutcome {
    Removed,
    AbsentAfterIntent,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackDiscardReport {
    pub schema_version: u16,
    pub basename: String,
    pub manifest_sha256: String,
    pub outcome: ModelPackDiscardOutcome,
    pub planned_file_count: u32,
    pub planned_bytes: u64,
    pub native_execution_or_activation: bool,
}
impl ModelPackDiscardReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_stage_basename(&self.basename)?;
        crate::validate_sha256(&self.manifest_sha256)?;
        if self.schema_version != 1
            || self.native_execution_or_activation
            || self.planned_file_count > 67
            || self.planned_bytes > PACK_TOTAL_LIMIT + PACK_MANIFEST_LIMIT as u64 + 16384
        {
            return Err(ProtocolError::invalid("invalid staged discard receipt"));
        }
        Ok(())
    }
}
impl ModelPackManifest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || self.ort_api_version != 24
            || !matches!(self.target_os.as_str(), "linux" | "windows" | "macos")
            || !matches!(self.target_arch.as_str(), "x86_64" | "aarch64")
            || self.assets.is_empty()
            || self.assets.len() > 64
            || self.labels.is_empty()
            || self.labels.len() > 256
            || self.source_description.is_empty()
            || self.source_description.len() > 1024
            || self.source_description.chars().any(char::is_control)
        {
            return Err(ProtocolError::invalid(
                "unsupported or unbounded model-pack declaration",
            ));
        }
        let mut labels = BTreeSet::new();
        for label in &self.labels {
            if label.is_empty()
                || label.len() > 128
                || label.chars().any(char::is_control)
                || !labels.insert(label)
            {
                return Err(ProtocolError::invalid("invalid/duplicate pack label"));
            }
        }
        let mut paths = BTreeSet::new();
        let mut spelling = std::collections::BTreeMap::new();
        let mut total = 0u64;
        for asset in &self.assets {
            validate_pack_path(&asset.path)?;
            crate::validate_sha256(&asset.sha256)?;
            let mut prefix = String::new();
            for part in asset.path.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if let Some(previous) = spelling.insert(prefix.to_ascii_lowercase(), prefix.clone())
                {
                    if previous != prefix {
                        return Err(ProtocolError::invalid("ambiguous pack path casing"));
                    }
                }
            }
            if !paths.insert(asset.path.to_ascii_lowercase())
                || asset.byte_len == 0
                || asset.byte_len > 256 * 1024 * 1024
            {
                return Err(ProtocolError::invalid("duplicate or oversized pack asset"));
            }
            total = total
                .checked_add(asset.byte_len)
                .ok_or_else(|| ProtocolError::invalid("pack size overflow"))?;
        }
        if total > PACK_TOTAL_LIMIT {
            return Err(ProtocolError::invalid("pack exceeds total byte limit"));
        }
        for role in [
            PackAssetRole::Model,
            PackAssetRole::Runtime,
            PackAssetRole::Profile,
        ] {
            if self.assets.iter().filter(|a| a.role == role).count() != 1 {
                return Err(ProtocolError::invalid(
                    "pack requires exactly one model/runtime/profile",
                ));
            }
        }
        if !self
            .assets
            .iter()
            .any(|a| a.path == self.license_path && a.role == PackAssetRole::License)
        {
            return Err(ProtocolError::invalid(
                "pack license declaration is not a listed license asset",
            ));
        }
        for asset in &self.assets {
            for other in &self.assets {
                if other.path != asset.path
                    && other
                        .path
                        .to_ascii_lowercase()
                        .starts_with(&(asset.path.to_ascii_lowercase() + "/"))
                {
                    return Err(ProtocolError::invalid("pack file/directory path collision"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackReport {
    pub manifest_sha256: String,
    pub pack_id: ArtifactId,
    pub asset_count: u32,
    pub total_bytes: u64,
    pub declared_target_matches_host: bool,
    pub verified_at_unix_ms: u64,
    pub integrity_verified: bool,
    pub native_execution_approved: bool,
    pub rights_verified: bool,
    pub compatible: bool,
    pub qualified: bool,
    pub active: bool,
}
impl ModelPackReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::validate_sha256(&self.manifest_sha256)?;
        if !(1..=64).contains(&self.asset_count)
            || self.total_bytes == 0
            || self.total_bytes > PACK_TOTAL_LIMIT
            || self.verified_at_unix_ms == 0
            || !self.integrity_verified
            || self.native_execution_approved
            || self.rights_verified
            || self.compatible
            || self.qualified
            || self.active
        {
            return Err(ProtocolError::invalid(
                "model-pack integrity report cannot promote execution or qualification",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPackStageState {
    Locked,
    UnlockedRecognized,
    Unrecognized,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackStageObservation {
    pub basename: String,
    pub state: ModelPackStageState,
    pub manifest_sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackStagingReport {
    pub schema_version: u16,
    pub observed_at_unix_ms: u64,
    pub stages: Vec<ModelPackStageObservation>,
    pub observation_only: bool,
}
impl ModelPackStagingReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1
            || self.observed_at_unix_ms == 0
            || !self.observation_only
            || self.stages.len() > 128
        {
            return Err(ProtocolError::invalid(
                "invalid model-pack staging observation",
            ));
        }
        let mut names = BTreeSet::new();
        for stage in &self.stages {
            if !stage.basename.starts_with(".pack-stage-")
                || stage.basename.len() > 128
                || !stage
                    .basename
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
                || !names.insert(&stage.basename)
            {
                return Err(ProtocolError::invalid("invalid/duplicate staging basename"));
            }
            match (&stage.state, &stage.manifest_sha256) {
                (ModelPackStageState::UnlockedRecognized, Some(hash)) => {
                    crate::validate_sha256(hash)?
                }
                (ModelPackStageState::Locked | ModelPackStageState::Unrecognized, None) => {}
                _ => {
                    return Err(ProtocolError::invalid(
                        "staging identity lacks a verified marker/manifest",
                    ))
                }
            }
        }
        Ok(())
    }
}

/// Historical authenticated declarations relative to an explicitly approved local root.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogReport {
    pub schema_version: u16,
    pub bootstrap_sha256: String,
    pub current_root_sha256: String,
    pub catalog_revision: u64,
    pub root_version: u64,
    pub timestamp_version: u64,
    pub snapshot_version: u64,
    pub targets_version: u64,
    pub manifest_sha256s: Vec<String>,
    pub accepted_by: SessionId,
    pub accepted_request: RequestId,
    pub authenticated_at_acceptance: bool,
    pub current_publisher_trust: bool,
    pub native_execution_approved: bool,
    pub rights_verified: bool,
    pub compatible: bool,
    pub qualified: bool,
}
impl ModelCatalogReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.bootstrap_sha256)?;
        validate_sha256(&self.current_root_sha256)?;
        if self.schema_version != 1
            || !self.authenticated_at_acceptance
            || self.current_publisher_trust
            || self.native_execution_approved
            || self.rights_verified
            || self.compatible
            || self.qualified
            || [
                self.catalog_revision,
                self.root_version,
                self.timestamp_version,
                self.snapshot_version,
                self.targets_version,
            ]
            .iter()
            .any(|n| *n == 0 || *n > i64::MAX as u64)
            || self.manifest_sha256s.len() > 128
        {
            return Err(ProtocolError::invalid(
                "invalid or falsely authoritative catalog receipt",
            ));
        }
        let mut previous = None;
        for digest in &self.manifest_sha256s {
            validate_sha256(digest)?;
            if previous.is_some_and(|p: &String| p >= digest) {
                return Err(ProtocolError::invalid(
                    "catalog manifests must be unique and sorted",
                ));
            }
            previous = Some(digest);
        }
        Ok(())
    }
}

#[cfg(test)]
mod catalog_contract_tests {
    use super::*;
    #[test]
    fn catalog_reports_cannot_self_promote_and_refresh_is_explicit_global_mutation() {
        let mut report = ModelCatalogReport {
            schema_version: 1,
            bootstrap_sha256: "a".repeat(64),
            current_root_sha256: "b".repeat(64),
            catalog_revision: 1,
            root_version: 1,
            timestamp_version: 1,
            snapshot_version: 1,
            targets_version: 1,
            manifest_sha256s: vec!["c".repeat(64)],
            accepted_by: SessionId::new("fixture-catalog-owner").unwrap(),
            accepted_request: RequestId::new("fixture-catalog-request").unwrap(),
            authenticated_at_acceptance: true,
            current_publisher_trust: false,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
        };
        report.validate().unwrap();
        let pack = ModelPackReport {
            manifest_sha256: "c".repeat(64),
            pack_id: ArtifactId::new("association-fixture").unwrap(),
            asset_count: 4,
            total_bytes: 4,
            declared_target_matches_host: true,
            verified_at_unix_ms: 1,
            integrity_verified: true,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
            active: false,
        };
        let mut association = ModelPackCatalogReport {
            schema_version: 1,
            catalog: report.clone(),
            pack,
            checked_at_unix_ms: 2,
            metadata_expires_at_unix_ms: 3,
            listed_in_catalog: true,
            online_revocation_checked: false,
        };
        let check = Command::CheckModelPackCatalog {
            bootstrap_sha256: "a".repeat(64),
            manifest_sha256: "c".repeat(64),
            expected_catalog_revision: 1,
            owner_proof: "fixture-owner".into(),
        };
        association.validate_for(&check).unwrap();
        let mut wrong = check.clone();
        if let Command::CheckModelPackCatalog {
            expected_catalog_revision,
            ..
        } = &mut wrong
        {
            *expected_catalog_revision = 2;
        }
        assert!(association.validate_for(&wrong).is_err());
        wrong = check.clone();
        if let Command::CheckModelPackCatalog {
            manifest_sha256, ..
        } = &mut wrong
        {
            *manifest_sha256 = "d".repeat(64);
        }
        assert!(association.validate_for(&wrong).is_err());
        wrong = check.clone();
        if let Command::CheckModelPackCatalog {
            bootstrap_sha256, ..
        } = &mut wrong
        {
            *bootstrap_sha256 = "d".repeat(64);
        }
        assert!(association.validate_for(&wrong).is_err());
        association.online_revocation_checked = true;
        assert!(association.validate().is_err());
        association.online_revocation_checked = false;
        association.metadata_expires_at_unix_ms = 2;
        assert!(association.validate().is_err());
        association.metadata_expires_at_unix_ms = 3;
        association.catalog.manifest_sha256s.clear();
        assert!(association.validate().is_err());
        report.current_publisher_trust = true;
        assert!(report.validate().is_err());
        report.current_publisher_trust = false;
        report.manifest_sha256s.push("c".repeat(64));
        assert!(report.validate().is_err());
        let mut command = Command::RefreshModelCatalog {
            source: crate::test_host_path("/fixture/catalog").into(),
            bootstrap: crate::test_host_path("/fixture/root.json").into(),
            bootstrap_sha256: "a".repeat(64),
            expected_catalog_revision: 0,
            approve_bootstrap: true,
            owner_proof: "fixture-capability".into(),
        };
        assert!(!command.requires_project());
        assert!(!command.requires_revision());
        assert!(command.is_mutating());
        Request::new(
            RequestId::new("fixture-global-catalog").unwrap(),
            command.clone(),
        )
        .validate()
        .unwrap();
        if let Command::RefreshModelCatalog {
            approve_bootstrap, ..
        } = &mut command
        {
            *approve_bootstrap = false;
        }
        assert!(
            Request::new(RequestId::new("fixture-refused-catalog").unwrap(), command)
                .validate()
                .is_err()
        );
        let status = Command::ModelCatalogStatus {
            bootstrap_sha256: "a".repeat(64),
            owner_proof: "fixture-capability".into(),
        };
        assert!(!status.requires_project());
        assert!(!status.is_mutating());
    }
}

/// Point-in-time installed-byte association with one accepted offline catalog.
/// This is neither an execution approval nor proof of online revocation freshness.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackCatalogReport {
    pub schema_version: u16,
    pub catalog: ModelCatalogReport,
    pub pack: ModelPackReport,
    pub checked_at_unix_ms: u64,
    pub metadata_expires_at_unix_ms: u64,
    pub listed_in_catalog: bool,
    pub online_revocation_checked: bool,
}
impl ModelPackCatalogReport {
    pub fn validate_for(&self, command: &Command) -> Result<(), ProtocolError> {
        self.validate()?;
        if let Command::CheckModelPackCatalog {
            bootstrap_sha256,
            manifest_sha256,
            expected_catalog_revision,
            ..
        } = command
        {
            if &self.catalog.bootstrap_sha256 == bootstrap_sha256
                && &self.pack.manifest_sha256 == manifest_sha256
                && self.catalog.catalog_revision == *expected_catalog_revision
            {
                return Ok(());
            }
        }
        Err(ProtocolError::invalid(
            "installed catalog response differs from requested association",
        ))
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.catalog.validate()?;
        self.pack.validate()?;
        if self.schema_version != 1
            || !self.listed_in_catalog
            || self.online_revocation_checked
            || self.checked_at_unix_ms == 0
            || self.metadata_expires_at_unix_ms <= self.checked_at_unix_ms
            || self.pack.verified_at_unix_ms > self.checked_at_unix_ms
            || !self
                .catalog
                .manifest_sha256s
                .contains(&self.pack.manifest_sha256)
        {
            return Err(ProtocolError::invalid(
                "invalid or falsely authoritative installed catalog association",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod catalog_association_contract_tests {
    use super::*;
    #[test]
    fn installed_association_requires_exact_revision_and_remains_global_read_only() {
        let command = Command::CheckModelPackCatalog {
            bootstrap_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            expected_catalog_revision: 1,
            owner_proof: "fixture-owner".into(),
        };
        assert!(!command.requires_project());
        assert!(!command.requires_revision());
        assert!(!command.is_mutating());
        Request::new(
            RequestId::new("fixture-association").unwrap(),
            command.clone(),
        )
        .validate()
        .unwrap();
        let mut invalid = command;
        if let Command::CheckModelPackCatalog {
            expected_catalog_revision,
            ..
        } = &mut invalid
        {
            *expected_catalog_revision = 0;
        }
        assert!(
            Request::new(RequestId::new("fixture-association-zero").unwrap(), invalid)
                .validate()
                .is_err()
        );
    }
}

/// Withdrawal records cancellation intent, never worker termination or model qualification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPackWithdrawalReport {
    pub schema_version: u16,
    pub choice: ModelPackChoice,
    pub selection: ModelPackSelectionReport,
    pub cancelled_approved_jobs: u32,
    pub cancelled_unclassified_jobs: u32,
}
impl ModelPackWithdrawalReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.choice.validate()?;
        self.selection.validate()?;
        if self.schema_version != 1
            || self.selection.choice.as_ref() == Some(&self.choice)
            || self.choice.selection_revision > self.selection.selection_revision
            || self
                .cancelled_approved_jobs
                .checked_add(self.cancelled_unclassified_jobs)
                .is_none_or(|n| n > 1024)
        {
            return Err(ProtocolError::invalid(
                "invalid model-pack withdrawal receipt",
            ));
        }
        Ok(())
    }
    pub fn validate_reply(&self, request: &Request) -> Result<(), ProtocolError> {
        self.validate()?;
        let Command::WithdrawModelPackApproval {
            choice,
            expected_selection_revision,
            include_unclassified_generation,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::invalid(
                "withdrawal receipt for another operation",
            ));
        };
        if &self.choice != choice
            || (!include_unclassified_generation && self.cancelled_unclassified_jobs != 0)
            || self.selection.selection_revision
                != if choice.selection_revision == *expected_selection_revision {
                    expected_selection_revision.saturating_add(1)
                } else {
                    *expected_selection_revision
                }
            || (self.selection.selection_revision != *expected_selection_revision
                && (self.selection.choice.is_some()
                    || self.selection.selected_by.as_ref() != request.session.as_ref()
                    || self.selection.selection_request.as_ref() != Some(&request.request_id)))
        {
            return Err(ProtocolError::invalid(
                "withdrawal receipt differs from exact intent",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod withdrawal_contract_tests {
    use super::*;
    #[test]
    fn withdrawal_reply_binds_current_and_historical_fences_and_exact_scope() {
        let choice = ModelPackChoice {
            manifest_sha256: "a".repeat(64),
            selection_revision: 1,
        };
        let mut request = Request::new(
            RequestId::new("withdrawal").unwrap(),
            Command::WithdrawModelPackApproval {
                choice: choice.clone(),
                expected_selection_revision: 1,
                include_unclassified_generation: false,
                owner_proof: "fixture".into(),
            },
        )
        .with_session(SessionId::new("owner").unwrap());
        let mut report = ModelPackWithdrawalReport {
            schema_version: 1,
            choice,
            selection: ModelPackSelectionReport {
                schema_version: 1,
                selection_revision: 2,
                catalog_binding: None,
                choice: None,
                selected_by: request.session.clone(),
                selection_request: Some(request.request_id.clone()),
                profile: None,
                native_execution_approved: false,
                rights_verified: false,
                compatible: false,
                qualified: false,
            },
            cancelled_approved_jobs: 1,
            cancelled_unclassified_jobs: 0,
        };
        report.validate_reply(&request).unwrap();
        report.selection.selection_revision = 1;
        assert!(
            report.validate_reply(&request).is_err(),
            "current withdrawal must advance the fence"
        );
        report.selection.selection_revision = 2;
        report.selection.selection_request = Some(RequestId::new("other").unwrap());
        assert!(report.validate_reply(&request).is_err());
        report.selection.selection_request = Some(request.request_id.clone());
        report.cancelled_unclassified_jobs = 1;
        assert!(report.validate_reply(&request).is_err());
        report.cancelled_unclassified_jobs = 0;
        report.cancelled_approved_jobs = 1025;
        assert!(report.validate_reply(&request).is_err());
        report.cancelled_approved_jobs = 1;
        if let Command::WithdrawModelPackApproval {
            expected_selection_revision,
            ..
        } = &mut request.command
        {
            *expected_selection_revision = 2;
        }
        report.validate_reply(&request).unwrap();
        report.selection.selection_revision = 3;
        assert!(
            report.validate_reply(&request).is_err(),
            "historical withdrawal must preserve the newer fence"
        );
    }
}
