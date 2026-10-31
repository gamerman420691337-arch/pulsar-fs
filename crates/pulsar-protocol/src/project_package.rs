//! Portable editable-project logical closure, not an import authority or ready export.
//!
//! All identities here are archival. Original evidence is carried separately as
//! exact, digest-bound bytes; opaque worker manifests must never be executed.
//! Resolver paths, credentials, grants, and live jobs are deliberately absent.
//! The admission limits below are engineering defaults, not product guarantees.

use crate::{
    validate_review_decisions, validate_reviews, validate_sha256, ArtifactIdentity, AttemptId,
    Axis, CandidateId, ErrorCode, JobId, ProgramDescriptor, ProjectId, ProtectedRegion,
    ProtocolError, ReviewDecision, ReviewFlag, RevisionId, SourceKind, SourceVersionId, TimeRange,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub const PROJECT_PACKAGE_FORMAT_VERSION: u16 = 1;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_ORIGINS: u16 = 2;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS: u16 = 3;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_REVIEW_DECISIONS: u16 = 4;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_RETIMING_RECEIPTS: u16 = 5;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_REVIEW_INTERVALS: u16 = 6;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_CANDIDATE_INTERVALS: u16 = 7;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_STASH_ASSOCIATIONS: u16 = 8;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_REBASE_LINKS: u16 = 9;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_CUDA_EVIDENCE: u16 = 10;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_ANNOTATED_MODELS: u16 = 11;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_WASM_INVOCATIONS: u16 = 12;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_IMAGE_ORIGINS: u16 = 13;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS: u16 = 14;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY: u16 = 15;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY: u16 = 16;
pub const PROJECT_PACKAGE_FORMAT_VERSION_WITH_BROWSER_FALLBACK_HISTORY: u16 = 17;
/// Canonical history plus the original reports and bounded raw evidence bodies.
pub const MAX_PACKAGE_FALLBACK_HISTORY_BYTES: usize = 160 * 1024 * 1024;
pub const MAX_PROJECT_PACKAGE_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PROJECT_PACKAGE_ENTRIES: usize = 100_000;
pub const DEFAULT_MAX_PROJECT_PACKAGE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageObjectRole {
    MotionProgram,
    SourceSnapshot,
    EditInput,
    EditReceipt,
    WorkerReceipt,
    WorkerManifest,
    InferenceFallbackHistory,
    LegacyMotionBytes,
    DependencyEvidence,
    ExportReceipt,
    ImportedManifest,
    CudaExecutionReceipt,
    CudaNodeTrace,
    AnnotatedModel,
    WasmModule,
    WasmInput,
    BrowserInferenceFallbackHistory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageObjectRef {
    pub sha256: String,
    pub byte_len: u64,
}

impl PackageObjectRef {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.sha256)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageObjectDescriptor {
    pub sha256: String,
    pub byte_len: u64,
    pub roles: Vec<PackageObjectRole>,
}

impl PackageObjectDescriptor {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.sha256)?;
        if self.roles.is_empty() || !strictly_sorted(self.roles.iter()) {
            return invalid("package object roles must be nonempty, unique, and ordered");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageLogicalClosure {
    /// A coherent logical capture, not a proof that all referenced bytes exist.
    #[default]
    Captured,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceBytes {
    /// P1a never upgrades this into a self-contained/export-ready assertion.
    #[default]
    NeedsMaterialization,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageGenerationDependencies {
    /// Model/runtime identities may survive without redistributing their bytes.
    #[default]
    IdentitiesOnly,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCompleteness {
    pub logical_closure: PackageLogicalClosure,
    pub source_bytes: PackageSourceBytes,
    pub generation_dependencies: PackageGenerationDependencies,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRecordCounts {
    pub revisions: u32,
    pub edit_states: u32,
    pub revision_lineage: u32,
    pub candidates: u32,
    pub authored_lineage: u32,
    pub sources: u32,
    pub legacy_cells: u32,
    pub generated_origins: u32,
    pub export_receipts: u32,
    pub objects: u32,
    #[serde(default, skip_serializing_if = "package_origin_count_is_zero")]
    pub imported_origins: u32,
    #[serde(default, skip_serializing_if = "package_origin_count_is_zero")]
    pub placement_entries: u32,
    #[serde(default, skip_serializing_if = "package_origin_count_is_zero")]
    pub retiming_receipts: u32,
}

fn package_origin_count_is_zero(value: &u32) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageProjectHead {
    pub name: String,
    pub revision: RevisionId,
    pub history_cursor: u64,
    pub motion: ProgramDescriptor,
    pub protected: Vec<ProtectedRegion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRevision {
    pub revision: RevisionId,
    /// Historical attribution only, never a local authenticated actor.
    pub actor: String,
    pub kind: String,
    pub label: String,
    pub motion: ProgramDescriptor,
    pub protected: Vec<ProtectedRegion>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageEditState {
    pub position: u64,
    pub motion: ProgramDescriptor,
    pub protected: Vec<ProtectedRegion>,
    /// None preserves a genuinely absent legacy history-lineage row.
    pub source_revision: Option<RevisionId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageReviewState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<ReviewDecision>,
    pub flags: Vec<ReviewFlag>,
    pub unknown: bool,
    pub conservative: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRevisionLineage {
    pub revision: RevisionId,
    pub parent_revision: Option<RevisionId>,
    pub restored_from_revision: Option<RevisionId>,
    /// This historical position may have been removed by branch truncation.
    pub restored_history_position: Option<u64>,
    pub candidate: Option<CandidateId>,
    pub operation: String,
    pub contributions: Vec<(Axis, TimeRange)>,
    pub review: PackageReviewState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCandidate {
    /// Historical unchanged-motion rebase parent in this manifest's namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebased_from: Option<CandidateId>,
    /// Application restriction; archived jobs remain inert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_interval: Option<TimeRange>,
    pub candidate_id: CandidateId,
    pub base_revision: RevisionId,
    pub motion: ProgramDescriptor,
    /// Inert origin only. It cannot resume, cancel, or authorize work.
    pub job_origin: Option<JobId>,
    /// Exact compact identities; model/runtime bytes need not be bundled.
    pub lineage: Vec<ArtifactIdentity>,
    pub committed_revision: Option<RevisionId>,
    pub review: Vec<ReviewFlag>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageAuthoredLineage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_invocation: Option<crate::PackageWasmInvocation>,
    pub candidate_id: CandidateId,
    /// Original receipt identity, including its original (possibly older) base.
    pub receipt: ArtifactIdentity,
    pub input: PackageObjectRef,
    pub review: PackageReviewState,
    pub authored_count: u64,
    pub inherited_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSource {
    /// Original browser recording evidence; inert and bound to the exact snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_fallback_history: Option<PackageObjectRef>,
    pub source_version: SourceVersionId,
    pub kind: SourceKind,
    pub label: String,
    pub identity: ArtifactIdentity,
    pub pinned: bool,
    /// Captured catalog state, not permission to follow the mutable original.
    pub evicted: bool,
    /// Archival claims only. Never creates a destination-engine Stash association.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stash_associations: Vec<PackageStashAssociation>,
}
/// Historical remote identity plus exact inert capture receipt bytes. Host paths
/// in the receipt are never filesystem locators in the importing engine.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackageStashAssociation {
    pub endpoint: Option<String>,
    pub scene_id: String,
    pub file_id: String,
    pub server_path: String,
    pub capture_receipt: PackageObjectRef,
}
impl PackageStashAssociation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(endpoint) = &self.endpoint {
            text(endpoint, 4096, true)?;
            if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
                return invalid("archival Stash endpoint scheme");
            }
            if endpoint.contains(['@', '?', '#']) {
                return invalid(
                    "archival Stash endpoint cannot contain credentials, query or fragment",
                );
            }
        }
        text(&self.scene_id, 128, true)?;
        text(&self.file_id, 128, true)?;
        text(&self.server_path, 8192, true)?;
        self.capture_receipt.validate()?;
        if self.capture_receipt.byte_len == 0 || self.capture_receipt.byte_len > 64 * 1024 {
            return invalid("archival Stash capture receipt exceeds 64 KiB");
        }
        Ok(())
    }
    /// Called only on bounded, hash-verified inert bytes. Checks correspondence,
    /// not truth of the remote declaration or destination publication authority.
    pub fn check_capture(
        &self,
        source: &PackageSource,
        capture: &crate::StashSourceCapture,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        capture.identity.validate()?;
        if capture.source_version != source.source_version
            || capture.identity != source.identity
            || capture.endpoint != self.endpoint
            || capture.scene_id != self.scene_id
            || capture.file_id != self.file_id
            || capture.server_path != self.server_path
        {
            return invalid("archival Stash capture differs from its source/declaration");
        }
        for path in [&capture.local_source, &capture.script_destination] {
            let value = path
                .to_str()
                .ok_or_else(|| ProtocolError::invalid("archival Stash path is not UTF-8"))?;
            text(value, 4096, true)?;
            if !path.is_absolute() {
                return invalid("archival Stash capture path must be absolute inert metadata");
            }
        }
        if capture.local_source.with_extension("funscript") != capture.script_destination {
            return invalid("archival Stash destination is not the captured sibling");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageLegacyOwner {
    Projects,
    Revisions,
    EditStates,
    Candidates,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageLegacyCell {
    pub owner: PackageLegacyOwner,
    /// Original engine row identity, never SQL or a filesystem resolver path.
    pub row_key: String,
    pub object: PackageObjectRef,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCudaEvidence {
    /// Original accepted membership, never a destination job or device grant.
    pub accepted_candidate: CandidateId,
    pub request_sha256: String,
    pub receipt: ArtifactIdentity,
    pub trace: ArtifactIdentity,
}
impl PackageCudaEvidence {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.request_sha256)?;
        self.receipt.validate()?;
        self.trace.validate()?;
        if self.receipt.byte_len == 0
            || self.receipt.byte_len > 65536
            || self.trace.byte_len == 0
            || self.trace.byte_len > 8 * 1024 * 1024
        {
            return invalid("archived CUDA evidence exceeds role byte bounds");
        }
        Ok(())
    }
    /// CUDA completion appends these two identities after the general receipt.
    pub fn general_receipt<'a>(
        &self,
        lineage: &'a [ArtifactIdentity],
    ) -> Result<&'a ArtifactIdentity, ProtocolError> {
        self.validate()?;
        let index = lineage
            .len()
            .checked_sub(3)
            .ok_or_else(|| ProtocolError::invalid("archived CUDA lineage is incomplete"))?;
        if lineage[index + 1] != self.receipt || lineage[index + 2] != self.trace {
            return invalid("archived CUDA lineage role order differs");
        }
        Ok(&lineage[index])
    }
}

/// Inert approval record and exact normalized pixels. Neither grants execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageImageOrigin {
    pub record: ArtifactIdentity,
    pub rgb: ArtifactIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageGeneratedOrigin {
    /// Exact original job history, shared by its attempts. Never execution authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_history: Option<PackageObjectRef>,
    /// Original expected image only; never executable installation authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_runtime: Option<Box<crate::CpuRuntimeImage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_origin: Option<PackageImageOrigin>,
    /// Retained original model bytes; never executable installation authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_model: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_evidence: Option<PackageCudaEvidence>,
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub base_revision: RevisionId,
    pub source_version: SourceVersionId,
    /// Exact historical bytes. Imported data must not become a WorkerRequest.
    pub manifest: PackageObjectRef,
    pub receipt: Option<ArtifactIdentity>,
    /// Exact raw program identity named by the original receipt, not a rewritten canonical object.
    pub program: Option<ArtifactIdentity>,
    /// Historical status, not an executable queue state.
    pub state: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageExportState {
    Pending,
    Complete,
    FailedOrUnknown,
}

/// Inert historical export scope. DeviceMotion describes the whole proposal,
/// not an invented single axis or a physical qualification assertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageExportScope {
    Axis(Axis),
    DeviceMotion,
}
impl Serialize for PackageExportScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Axis(axis) => axis.as_str(),
            Self::DeviceMotion => "device-motion",
        })
    }
}
impl<'de> Deserialize<'de> for PackageExportScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "device-motion" {
            return Ok(Self::DeviceMotion);
        }
        Axis::deserialize(serde::de::value::StrDeserializer::<D::Error>::new(&value))
            .map(Self::Axis)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageExportReceipt {
    pub revision: RevisionId,
    /// Historical wire name retained for byte-compatible single-axis receipts.
    pub axis: PackageExportScope,
    pub sha256: String,
    pub state: PackageExportState,
    /// Optional private archival text; never an output or extraction path.
    pub archival_path: Option<String>,
}

/// Strict logical snapshot. No live authority, paths to resolve, or executable
/// requests may be added here as a convenience for a future importer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableProjectManifest {
    pub format_version: u16,
    pub origin_project_id: ProjectId,
    pub captured_revision: RevisionId,
    /// Historical capture cursor, never a reconnect cursor in the destination.
    pub captured_event_cursor: u64,
    pub completeness: PackageCompleteness,
    pub counts: PackageRecordCounts,
    pub head: PackageProjectHead,
    pub revisions: Vec<PackageRevision>,
    pub edit_states: Vec<PackageEditState>,
    /// Absent legacy rows stay absent; numeric adjacency is not ancestry.
    pub revision_lineage: Vec<PackageRevisionLineage>,
    pub candidates: Vec<PackageCandidate>,
    pub authored_lineage: Vec<PackageAuthoredLineage>,
    pub sources: Vec<PackageSource>,
    pub legacy_cells: Vec<PackageLegacyCell>,
    pub generated_origins: Vec<PackageGeneratedOrigin>,
    pub export_receipts: Vec<PackageExportReceipt>,
    /// SHA-256 order, one payload per digest even if it serves several roles.
    pub objects: Vec<PackageObjectDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imported_origins: Vec<crate::PackageArchiveOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placements: Option<crate::PackagePlacementClosure>,
    /// Original namespace facts only. Clone imports retain them in hashed origin
    /// manifests instead of rewriting them into destination-authored receipts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retiming_receipts: Vec<crate::MotionRetimeReceipt>,
}

impl PortableProjectManifest {
    /// Computes counts; capture must explicitly install these before validation.
    pub fn record_counts(&self) -> Result<PackageRecordCounts, ProtocolError> {
        let lengths = [
            self.revisions.len(),
            self.edit_states.len(),
            self.revision_lineage.len(),
            self.candidates.len(),
            self.authored_lineage.len(),
            self.sources.len(),
            self.legacy_cells.len(),
            self.generated_origins.len(),
            self.export_receipts.len(),
            self.objects.len(),
            self.imported_origins.len(),
            self.placements
                .as_ref()
                .map(|p| p.entry_count())
                .transpose()?
                .unwrap_or(0),
            self.retiming_receipts.len(),
        ];
        let total = lengths
            .iter()
            .try_fold(1usize, |total, n| total.checked_add(*n))
            .ok_or_else(|| resource("package entry count overflow"))?;
        if total > MAX_PROJECT_PACKAGE_ENTRIES {
            return Err(resource(
                "package exceeds the logical/object entry admission limit",
            ));
        }
        Ok(PackageRecordCounts {
            revisions: lengths[0] as u32,
            edit_states: lengths[1] as u32,
            revision_lineage: lengths[2] as u32,
            candidates: lengths[3] as u32,
            authored_lineage: lengths[4] as u32,
            sources: lengths[5] as u32,
            legacy_cells: lengths[6] as u32,
            generated_origins: lengths[7] as u32,
            export_receipts: lengths[8] as u32,
            objects: lengths[9] as u32,
            imported_origins: lengths[10] as u32,
            placement_entries: lengths[11] as u32,
            retiming_receipts: lengths[12] as u32,
        })
    }

    /// Validates bounded logical consistency, not possession or truth of bytes.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.canonical_bytes().map(|_| ())
    }

    /// Field order and array order are canonical. Reordering is never silent.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        let bytes = bounded_json(self, MAX_PROJECT_PACKAGE_MANIFEST_BYTES)?;
        self.validate_structure()?;
        Ok(bytes)
    }

    pub fn required_format_version(&self) -> u16 {
        if self
            .sources
            .iter()
            .any(|s| s.browser_fallback_history.is_some())
            || self.objects.iter().any(|o| {
                o.roles
                    .contains(&PackageObjectRole::BrowserInferenceFallbackHistory)
            })
            || self.imported_origins.iter().any(|o| {
                o.container.format_version
                    >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_BROWSER_FALLBACK_HISTORY
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_BROWSER_FALLBACK_HISTORY;
        }
        if self
            .generated_origins
            .iter()
            .any(|origin| origin.fallback_history.is_some())
            || self.objects.iter().any(|object| {
                object
                    .roles
                    .contains(&PackageObjectRole::InferenceFallbackHistory)
            })
            || self.imported_origins.iter().any(|origin| {
                origin.container.format_version
                    >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY;
        }
        if self
            .generated_origins
            .iter()
            .any(|origin| origin.cpu_runtime.is_some())
            || self.imported_origins.iter().any(|origin| {
                origin.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY;
        }
        if self
            .export_receipts
            .iter()
            .any(|r| matches!(r.axis, PackageExportScope::DeviceMotion))
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS;
        }
        if self
            .generated_origins
            .iter()
            .any(|o| o.image_origin.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_IMAGE_ORIGINS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_IMAGE_ORIGINS;
        }
        if self
            .authored_lineage
            .iter()
            .any(|l| l.wasm_invocation.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_WASM_INVOCATIONS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_WASM_INVOCATIONS;
        }
        if self
            .generated_origins
            .iter()
            .any(|o| o.annotated_model.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_ANNOTATED_MODELS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_ANNOTATED_MODELS;
        }
        if self
            .generated_origins
            .iter()
            .any(|o| o.cuda_evidence.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CUDA_EVIDENCE
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_CUDA_EVIDENCE;
        }
        if self.candidates.iter().any(|c| c.rebased_from.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_REBASE_LINKS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_REBASE_LINKS;
        }
        if self
            .sources
            .iter()
            .any(|s| !s.stash_associations.is_empty())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_STASH_ASSOCIATIONS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_STASH_ASSOCIATIONS;
        }
        if self.candidates.iter().any(|c| c.project_interval.is_some())
            || self.imported_origins.iter().any(|o| {
                o.container.format_version
                    >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CANDIDATE_INTERVALS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_CANDIDATE_INTERVALS;
        }
        if self
            .revision_lineage
            .iter()
            .any(|l| l.review.decisions.iter().any(|d| d.range.is_some()))
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_REVIEW_INTERVALS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_REVIEW_INTERVALS;
        }
        if !self.retiming_receipts.is_empty()
            || self.imported_origins.iter().any(|o| {
                o.container.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_RETIMING_RECEIPTS
            })
        {
            return PROJECT_PACKAGE_FORMAT_VERSION_WITH_RETIMING_RECEIPTS;
        }
        if self
            .revision_lineage
            .iter()
            .any(|l| !l.review.decisions.is_empty())
            || self
                .authored_lineage
                .iter()
                .any(|l| !l.review.decisions.is_empty())
        {
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_REVIEW_DECISIONS
        } else if self.placements.is_some() {
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
        } else if self.imported_origins.is_empty() {
            PROJECT_PACKAGE_FORMAT_VERSION
        } else {
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_ORIGINS
        }
    }
    fn validate_structure(&self) -> Result<(), ProtocolError> {
        if self.format_version != self.required_format_version() {
            return Err(ProtocolError::unsupported(
                "unsupported portable project format",
            ));
        }
        if let Some(placements) = &self.placements {
            placements.validate(self)?;
        }
        self.validate_retiming_receipts()?;
        if self.record_counts()? != self.counts {
            return invalid("package declared record counts do not match its closure");
        }
        if self.head.revision != self.captured_revision {
            return invalid("package head differs from captured revision");
        }
        if self
            .revisions
            .last()
            .is_none_or(|r| r.revision != self.captured_revision)
        {
            return invalid("captured head must be the maximum persisted revision");
        }
        if self
            .edit_states
            .iter()
            .enumerate()
            .any(|(position, state)| state.position != position as u64)
        {
            return invalid("retained history positions must be contiguous from zero");
        }
        text(&self.head.name, 256, true)?;
        if !strictly_sorted(self.revisions.iter().map(|r| &r.revision))
            || !strictly_sorted(self.edit_states.iter().map(|s| s.position))
            || !strictly_sorted(self.revision_lineage.iter().map(|r| &r.revision))
            || !strictly_sorted(self.candidates.iter().map(|c| c.candidate_id.as_str()))
            || !strictly_sorted(
                self.authored_lineage
                    .iter()
                    .map(|c| c.candidate_id.as_str()),
            )
            || !strictly_sorted(self.sources.iter().map(|s| s.source_version.as_str()))
            || !strictly_sorted(
                self.legacy_cells
                    .iter()
                    .map(|c| (c.owner, c.row_key.as_str())),
            )
            || !strictly_sorted(
                self.generated_origins
                    .iter()
                    .map(|g| (g.job_id.as_str(), g.attempt_id.as_str())),
            )
            || !strictly_sorted(self.objects.iter().map(|o| o.sha256.as_str()))
        {
            return invalid("package records must have unique identities in canonical order");
        }
        let revisions: BTreeMap<_, _> = self.revisions.iter().map(|r| (r.revision, r)).collect();
        let states: BTreeMap<_, _> = self.edit_states.iter().map(|s| (s.position, s)).collect();
        let candidates: BTreeMap<_, _> = self
            .candidates
            .iter()
            .map(|c| (c.candidate_id.as_str(), c))
            .collect();
        let sources: BTreeSet<_> = self
            .sources
            .iter()
            .map(|s| s.source_version.as_str())
            .collect();
        let mut job_receipts: BTreeMap<&str, BTreeSet<(&str, &str, u64)>> = BTreeMap::new();
        for origin in &self.generated_origins {
            if let Some(history) = &origin.fallback_history {
                history.validate()?;
                if history.byte_len == 0
                    || history.byte_len > MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64
                {
                    return invalid("fallback history object exceeds portable admission");
                }
            }
            if let Some(image) = &origin.cpu_runtime {
                image.validate()?;
                if origin.cuda_evidence.is_some() {
                    return invalid("CPU image and CUDA completion cannot share one origin");
                }
            }
            if let Some(model) = &origin.annotated_model {
                model.validate()?;
                if model.byte_len == 0 || model.byte_len > 256 * 1024 * 1024 {
                    return invalid("annotated model bytes exceed portable admission");
                }
                if origin.receipt.is_some() && origin.state != "completed" {
                    return invalid("annotated receipt belongs to a noncompleted origin");
                }
                if origin.state == "completed"
                    && (origin.receipt.is_none()
                        || !self
                            .candidates
                            .iter()
                            .any(|c| c.job_origin.as_ref() == Some(&origin.job_id)))
                {
                    return invalid("completed annotated origin lacks candidate and receipt");
                }
            }
            if let Some(receipt) = &origin.receipt {
                job_receipts
                    .entry(origin.job_id.as_str())
                    .or_default()
                    .insert((
                        receipt.artifact_id.as_str(),
                        receipt.sha256.as_str(),
                        receipt.byte_len,
                    ));
            }
        }
        let head_revision = revisions
            .get(&self.head.revision)
            .ok_or_else(|| ProtocolError::invalid("package head revision is missing"))?;
        let head_state = states
            .get(&self.head.history_cursor)
            .ok_or_else(|| ProtocolError::invalid("package head history state is missing"))?;
        if !same(&self.head.motion, &head_revision.motion)?
            || !same(&self.head.protected, &head_revision.protected)?
            || !same(&self.head.motion, &head_state.motion)?
            || !same(&self.head.protected, &head_state.protected)?
        {
            return invalid("package head disagrees with its revision or retained history state");
        }
        crate::validate_imported_origin_shapes(self)?;
        let mut required = RequiredObjects::default();
        for origin in &self.imported_origins {
            required.reference(&origin.manifest, PackageObjectRole::ImportedManifest)?;
            for object in &origin.objects {
                for role in &object.roles {
                    required.reference(
                        &PackageObjectRef {
                            sha256: object.sha256.clone(),
                            byte_len: object.byte_len,
                        },
                        *role,
                    )?;
                }
            }
        }
        required.motion(&self.head.motion)?;
        protection(&self.head.protected)?;
        for revision in &self.revisions {
            text(&revision.actor, 256, true)?;
            text(&revision.kind, 128, true)?;
            text(&revision.label, 4096, false)?;
            required.motion(&revision.motion)?;
            protection(&revision.protected)?;
        }
        for state in &self.edit_states {
            required.motion(&state.motion)?;
            protection(&state.protected)?;
            if let Some(revision) = state.source_revision {
                let associated = revisions.get(&revision).ok_or_else(|| {
                    ProtocolError::invalid("retained history references a missing revision")
                })?;
                if !same(&state.motion, &associated.motion)?
                    || state.protected != associated.protected
                {
                    return invalid("retained history content differs from its source revision");
                }
            }
        }
        for lineage in &self.revision_lineage {
            if !revisions.contains_key(&lineage.revision) {
                return invalid("lineage references a missing revision");
            }
            for parent in [lineage.parent_revision, lineage.restored_from_revision]
                .into_iter()
                .flatten()
            {
                if !revisions.contains_key(&parent) || parent >= lineage.revision {
                    return invalid("lineage must reference an existing earlier revision");
                }
            }
            if let Some(candidate) = &lineage.candidate {
                let candidate = candidates.get(candidate.as_str()).ok_or_else(|| {
                    ProtocolError::invalid("revision lineage references a missing candidate")
                })?;
                if candidate.committed_revision != Some(lineage.revision) {
                    return invalid("revision lineage disagrees with its candidate commit");
                }
            }
            text(&lineage.operation, 128, true)?;
            bounded_entries(lineage.contributions.len())?;
            // TimeRange already enforces checked, nonempty interval construction.
            review(&lineage.review)?;
            for d in &lineage.review.decisions {
                let r = revisions
                    .get(&d.recorded_revision)
                    .ok_or_else(|| ProtocolError::invalid("review decision revision is absent"))?;
                if d.recorded_revision > lineage.revision
                    || r.actor != d.actor.as_str()
                    || r.kind
                        != if d.range.is_some() {
                            "review_interval_disposition"
                        } else {
                            "review_disposition"
                        }
                    || !revisions.contains_key(&d.base_revision)
                {
                    return invalid("review decision actor/revision binding is inconsistent");
                }
            }
        }
        for candidate in &self.candidates {
            if let Some(parent) = &candidate.rebased_from {
                let parent = candidates
                    .get(parent.as_str())
                    .ok_or_else(|| ProtocolError::invalid("portable rebase parent is absent"))?;
                if parent.candidate_id == candidate.candidate_id
                    || parent.base_revision > candidate.base_revision
                    || !same(&parent.motion, &candidate.motion)?
                    || parent.lineage != candidate.lineage
                    || parent.job_origin != candidate.job_origin
                    || !same(&parent.review, &candidate.review)?
                    || parent.project_interval != candidate.project_interval
                {
                    return invalid("portable rebase changed motion or evidence");
                }
                let mut current = candidate;
                let mut seen = std::collections::BTreeSet::new();
                while let Some(parent) = &current.rebased_from {
                    if !seen.insert(current.candidate_id.as_str()) {
                        return invalid("portable rebase ancestry is cyclic");
                    }
                    if seen.len() >= 128 {
                        return Err(resource("portable rebase ancestry exceeds its bound"));
                    }
                    current = candidates.get(parent.as_str()).ok_or_else(|| {
                        ProtocolError::invalid("portable rebase parent is absent")
                    })?;
                }
            }
            if candidate
                .project_interval
                .is_some_and(|r| r.start() < crate::ProjectTime::ZERO || r.start() >= r.end())
            {
                return invalid("invalid candidate project interval");
            }
            if !revisions.contains_key(&candidate.base_revision)
                || candidate
                    .committed_revision
                    .is_some_and(|r| !revisions.contains_key(&r) || r <= candidate.base_revision)
            {
                return invalid("candidate references a missing base or committed revision");
            }
            if let Some(job) = &candidate.job_origin {
                let matched = job_receipts.get(job.as_str()).is_some_and(|receipts| {
                    candidate.lineage.iter().any(|identity| {
                        receipts.contains(&(
                            identity.artifact_id.as_str(),
                            identity.sha256.as_str(),
                            identity.byte_len,
                        ))
                    })
                });
                if !matched {
                    return invalid(
                        "generated candidate is missing its exact archived worker receipt",
                    );
                }
            }
            bounded_entries(candidate.lineage.len())?;
            for identity in &candidate.lineage {
                identity.validate()?;
            }
            validate_reviews(&candidate.review)?;
            required.motion(&candidate.motion)?;
        }
        for authored in &self.authored_lineage {
            if !candidates.contains_key(authored.candidate_id.as_str()) {
                return invalid("authored lineage references a missing candidate");
            }
            if authored
                .authored_count
                .checked_add(authored.inherited_count)
                .is_none()
            {
                return invalid("authored lineage counts overflow");
            }
            review(&authored.review)?;
            if !authored.review.decisions.is_empty() {
                return invalid(
                    "uncommitted authored candidates cannot carry accepted review decisions",
                );
            }
            if let Some(invocation) = &authored.wasm_invocation {
                invocation.validate()?;
                let origin = candidates
                    .get(invocation.origin_candidate_id.as_str())
                    .ok_or_else(|| ProtocolError::invalid("WASM original candidate is absent"))?;
                let base = self
                    .revisions
                    .iter()
                    .find(|r| r.revision == invocation.base_revision)
                    .ok_or_else(|| ProtocolError::invalid("WASM original base is absent"))?;
                let candidate = candidates.get(authored.candidate_id.as_str()).unwrap();
                if invocation.receipt.edit_receipt != authored.receipt
                    || invocation.receipt.output_sha256 != authored.input.sha256
                    || invocation.receipt.output_byte_len != authored.input.byte_len
                    || invocation.receipt.base_program != base.motion
                    || invocation.receipt.result_program != candidate.motion
                    || invocation.receipt.result_program != origin.motion
                {
                    return invalid("WASM archived association differs from authored lineage");
                }
                required.identity(&invocation.manifest.artifact, PackageObjectRole::WasmModule)?;
                required.reference(&invocation.input, PackageObjectRole::WasmInput)?;
            }
            required.identity(&authored.receipt, PackageObjectRole::EditReceipt)?;
            required.reference(&authored.input, PackageObjectRole::EditInput)?;
        }
        for source in &self.sources {
            if let Some(history) = &source.browser_fallback_history {
                if source.kind != SourceKind::LiveRecording
                    || history.byte_len == 0
                    || history.byte_len > MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64
                {
                    return invalid(
                        "browser fallback history requires a bounded live recording source",
                    );
                }
                required.reference(history, PackageObjectRole::BrowserInferenceFallbackHistory)?;
            }
            text(&source.label, 4096, false)?;
            required.identity(&source.identity, PackageObjectRole::SourceSnapshot)?;
            if source.kind != SourceKind::Media && !source.stash_associations.is_empty() {
                return invalid("Stash association requires a media source");
            }
            if source.stash_associations.len() > 16
                || !strictly_sorted(
                    source
                        .stash_associations
                        .iter()
                        .map(|a| a.capture_receipt.sha256.as_str()),
                )
            {
                return invalid(
                    "source Stash associations must be bounded and sorted by unique receipt digest",
                );
            }
            for association in &source.stash_associations {
                association.validate()?;
                required.reference(
                    &association.capture_receipt,
                    PackageObjectRole::DependencyEvidence,
                )?;
            }
        }
        for cell in &self.legacy_cells {
            text(&cell.row_key, 512, true)?;
            let present = match cell.owner {
                PackageLegacyOwner::Projects => cell.row_key == self.origin_project_id.as_str(),
                PackageLegacyOwner::Revisions => {
                    legacy_position(&cell.row_key, &self.origin_project_id)
                        .is_some_and(|revision| revisions.contains_key(&RevisionId::new(revision)))
                }
                // Exact original cells survive truncation of the retained history.
                PackageLegacyOwner::EditStates => {
                    legacy_position(&cell.row_key, &self.origin_project_id).is_some()
                }
                PackageLegacyOwner::Candidates => candidates.contains_key(cell.row_key.as_str()),
            };
            if !present {
                return invalid("legacy recovery cell is outside the captured logical closure");
            }
            required.reference(&cell.object, PackageObjectRole::LegacyMotionBytes)?;
        }
        for origin in &self.generated_origins {
            if !revisions.contains_key(&origin.base_revision)
                || !sources.contains(origin.source_version.as_str())
            {
                return invalid("generation origin references a missing revision or source");
            }
            if !(origin.state == "assistant_paused" && origin.fallback_history.is_some())
                && !matches!(
                    origin.state.as_str(),
                    "queued" | "running" | "interrupted" | "completed" | "failed" | "cancelled"
                )
            {
                return invalid("unsupported archival generation state");
            }
            required.reference(&origin.manifest, PackageObjectRole::WorkerManifest)?;
            if let Some(history) = &origin.fallback_history {
                required.reference(history, PackageObjectRole::InferenceFallbackHistory)?;
            }
            if let Some(image) = &origin.image_origin {
                if image.record.byte_len == 0
                    || image.record.byte_len > 128 * 1024
                    || image.rgb.byte_len == 0
                    || image.rgb.byte_len > 512 * 512 * 3
                {
                    return invalid("image origin exceeds portable admission");
                }
                required.identity(&image.record, PackageObjectRole::DependencyEvidence)?;
                required.identity(&image.rgb, PackageObjectRole::DependencyEvidence)?;
            }
            if let Some(model) = &origin.annotated_model {
                required.identity(model, PackageObjectRole::AnnotatedModel)?;
            }
            if origin.receipt.is_some() != origin.program.is_some() {
                return invalid("archived worker receipt and exact raw program must be paired");
            }
            if let Some(program) = &origin.program {
                required.identity(program, PackageObjectRole::DependencyEvidence)?;
            }
            if let Some(receipt) = &origin.receipt {
                required.identity(receipt, PackageObjectRole::WorkerReceipt)?;
            }
            if let Some(cuda) = &origin.cuda_evidence {
                cuda.validate()?;
                let candidate = candidates
                    .get(cuda.accepted_candidate.as_str())
                    .ok_or_else(|| {
                        ProtocolError::invalid("archived CUDA accepted candidate absent")
                    })?;
                if origin.state != "completed"
                    || candidate.job_origin.as_ref() != Some(&origin.job_id)
                    || candidate.base_revision != origin.base_revision
                    || origin.manifest.sha256 != cuda.request_sha256
                    || origin.receipt.as_ref() != Some(cuda.general_receipt(&candidate.lineage)?)
                {
                    return invalid("archived CUDA membership differs from generation origin");
                }
                required.identity(&cuda.receipt, PackageObjectRole::CudaExecutionReceipt)?;
                required.identity(&cuda.trace, PackageObjectRole::CudaNodeTrace)?;
            }
        }
        let mut previous: Option<Vec<u8>> = None;
        for receipt in &self.export_receipts {
            if !revisions.contains_key(&receipt.revision) {
                return invalid("export receipt references a missing revision");
            }
            validate_sha256(&receipt.sha256)?;
            if let Some(path) = &receipt.archival_path {
                text(path, 4096, false)?;
            }
            let key = bounded_json(receipt, 16 * 1024)?;
            if previous.as_ref().is_some_and(|p| p > &key) {
                return invalid("export receipts must be ordered by canonical record bytes");
            }
            previous = Some(key);
        }
        let mut total_bytes = 0u64;
        for object in &self.objects {
            object.validate()?;
            total_bytes = total_bytes
                .checked_add(object.byte_len)
                .ok_or_else(|| resource("package object byte total overflow"))?;
            let expected = required
                .0
                .remove(&object.sha256)
                .ok_or_else(|| ProtocolError::invalid("unreferenced object in package closure"))?;
            if object.byte_len != expected.0
                || object.roles.iter().copied().collect::<BTreeSet<_>>() != expected.1
            {
                return invalid("package object length or roles differ from logical references");
            }
        }
        if !required.0.is_empty() {
            return invalid("package is missing a required object descriptor");
        }
        Ok(())
    }

    fn validate_retiming_receipts(&self) -> Result<(), ProtocolError> {
        if !strictly_sorted(self.retiming_receipts.iter().map(|r| r.revision)) {
            return invalid("retiming receipts must have unique ascending revisions");
        }
        let revisions: BTreeMap<_, _> = self.revisions.iter().map(|r| (r.revision, r)).collect();
        let lineage: BTreeMap<_, _> = self
            .revision_lineage
            .iter()
            .map(|r| (r.revision, r))
            .collect();
        let placements: BTreeMap<_, _> = self
            .placements
            .iter()
            .flat_map(|p| p.definitions.iter().map(|d| (&d.placement_id, d)))
            .collect();
        for receipt in &self.retiming_receipts {
            receipt.validate()?;
            let before = revisions
                .get(&receipt.base_revision)
                .ok_or_else(|| ProtocolError::invalid("retiming base revision missing"))?;
            let after = revisions
                .get(&receipt.revision)
                .ok_or_else(|| ProtocolError::invalid("retiming result revision missing"))?;
            let link = lineage
                .get(&receipt.revision)
                .ok_or_else(|| ProtocolError::invalid("retiming lineage missing"))?;
            let matches = |motion: &ProgramDescriptor, identity: &ArtifactIdentity| {
                motion.artifact_id == identity.artifact_id
                    && motion.sha256 == identity.sha256
                    && motion.byte_len == identity.byte_len
            };
            if self
                .imported_origins
                .iter()
                .any(|o| receipt.revision <= o.container.captured_revision)
                || receipt.project_id != self.origin_project_id
                || after.actor != receipt.actor
                || after.kind != "source_retime"
                || link.operation != "source_retime"
                || link.parent_revision != Some(receipt.base_revision)
                || !matches(&before.motion, &receipt.before)
                || !matches(&after.motion, &receipt.after)
                || placements.get(&receipt.from.placement_id).copied() != Some(&receipt.from)
                || placements.get(&receipt.to.placement_id).copied() != Some(&receipt.to)
                || !self
                    .placements
                    .iter()
                    .flat_map(|p| p.revisions.iter())
                    .any(|r| {
                        r.revision == receipt.revision
                            && r.placements.contains(&receipt.to.placement_id)
                    })
            {
                return invalid("retiming receipt differs from its original namespace bindings");
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct RequiredObjects(BTreeMap<String, (u64, BTreeSet<PackageObjectRole>)>);
impl RequiredObjects {
    fn reference(
        &mut self,
        object: &PackageObjectRef,
        role: PackageObjectRole,
    ) -> Result<(), ProtocolError> {
        object.validate()?;
        if let Some((length, roles)) = self.0.get_mut(&object.sha256) {
            if *length != object.byte_len {
                return invalid("one object digest has inconsistent lengths");
            }
            roles.insert(role);
        } else {
            self.0.insert(
                object.sha256.clone(),
                (object.byte_len, BTreeSet::from([role])),
            );
        }
        Ok(())
    }
    fn identity(
        &mut self,
        identity: &ArtifactIdentity,
        role: PackageObjectRole,
    ) -> Result<(), ProtocolError> {
        identity.validate()?;
        self.reference(
            &PackageObjectRef {
                sha256: identity.sha256.clone(),
                byte_len: identity.byte_len,
            },
            role,
        )
    }
    fn motion(&mut self, motion: &ProgramDescriptor) -> Result<(), ProtocolError> {
        motion.validate()?;
        self.reference(
            &PackageObjectRef {
                sha256: motion.sha256.clone(),
                byte_len: motion.byte_len,
            },
            PackageObjectRole::MotionProgram,
        )
    }
}

fn strictly_sorted<T: PartialOrd>(values: impl IntoIterator<Item = T>) -> bool {
    let mut previous = None;
    for value in values {
        if previous.as_ref().is_some_and(|p| p >= &value) {
            return false;
        }
        previous = Some(value);
    }
    true
}
fn bounded_entries(n: usize) -> Result<(), ProtocolError> {
    if n > MAX_PROJECT_PACKAGE_ENTRIES {
        return Err(resource("package nested record limit exceeded"));
    }
    Ok(())
}
fn protection(regions: &[ProtectedRegion]) -> Result<(), ProtocolError> {
    if regions.len() > 1024 {
        return invalid("package protection limit exceeded");
    }
    Ok(())
}
fn review(state: &PackageReviewState) -> Result<(), ProtocolError> {
    validate_review_decisions(&state.flags, &state.decisions)
}
fn text(value: &str, limit: usize, nonempty: bool) -> Result<(), ProtocolError> {
    if value.len() > limit || value.contains('\0') || (nonempty && value.is_empty()) {
        return invalid("package text field exceeds its bounded contract");
    }
    Ok(())
}
fn legacy_position(key: &str, project: &ProjectId) -> Option<u64> {
    let (owner, suffix) = key.rsplit_once(':')?;
    if owner != project.as_str() {
        return None;
    }
    let value = suffix.parse::<u64>().ok()?;
    (suffix == value.to_string()).then_some(value)
}
fn same<T: Serialize>(left: &T, right: &T) -> Result<bool, ProtocolError> {
    Ok(bounded_json(left, MAX_PROJECT_PACKAGE_MANIFEST_BYTES)?
        == bounded_json(right, MAX_PROJECT_PACKAGE_MANIFEST_BYTES)?)
}
fn invalid<T>(message: &str) -> Result<T, ProtocolError> {
    Err(ProtocolError::invalid(message))
}
fn resource(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::ResourceExhausted, message)
}
fn bounded_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, ProtocolError> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > self.limit)
            {
                return Err(io::Error::other("package JSON admission limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| resource("package canonical JSON exceeds its admission limit"))?;
    Ok(writer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactId, MotionCodec};
    const EMPTY_SHA: &str = "ac11c4570c1e4918245c0ca34c2b951091b867a5aef252556ae323d06df61cf8";

    fn fixture() -> PortableProjectManifest {
        let motion = ProgramDescriptor {
            artifact_id: ArtifactId::new(format!("motion.{EMPTY_SHA}")).unwrap(),
            sha256: EMPTY_SHA.into(),
            byte_len: 13,
            codec: MotionCodec::MotionProgramJsonV1,
            axes: vec![],
        };
        let mut m = PortableProjectManifest {
            placements: None,
            retiming_receipts: vec![],
            imported_origins: vec![],
            format_version: 1,
            origin_project_id: ProjectId::new("p").unwrap(),
            captured_revision: RevisionId::new(0),
            captured_event_cursor: 0,
            completeness: PackageCompleteness::default(),
            counts: PackageRecordCounts::default(),
            head: PackageProjectHead {
                name: "P".into(),
                revision: RevisionId::new(0),
                history_cursor: 0,
                motion: motion.clone(),
                protected: vec![],
            },
            revisions: vec![PackageRevision {
                revision: RevisionId::new(0),
                actor: "actor".into(),
                kind: "create".into(),
                label: "".into(),
                motion: motion.clone(),
                protected: vec![],
            }],
            edit_states: vec![PackageEditState {
                position: 0,
                motion,
                protected: vec![],
                source_revision: None,
            }],
            revision_lineage: vec![],
            candidates: vec![],
            authored_lineage: vec![],
            sources: vec![],
            legacy_cells: vec![],
            generated_origins: vec![],
            export_receipts: vec![],
            objects: vec![PackageObjectDescriptor {
                sha256: EMPTY_SHA.into(),
                byte_len: 13,
                roles: vec![PackageObjectRole::MotionProgram],
            }],
        };
        m.counts = m.record_counts().unwrap();
        m
    }
    #[test]
    fn browser_history_requires_format17_and_exact_live_source_role() {
        let mut m = generated_fixture();
        let legacy = m.canonical_bytes().unwrap();
        assert!(!String::from_utf8(legacy.clone())
            .unwrap()
            .contains("browser_fallback_history"));
        let reference = PackageObjectRef {
            sha256: "e".repeat(64),
            byte_len: 100,
        };
        m.sources[0].kind = SourceKind::LiveRecording;
        m.sources[0].browser_fallback_history = Some(reference.clone());
        m.objects.push(PackageObjectDescriptor {
            sha256: reference.sha256,
            byte_len: reference.byte_len,
            roles: vec![PackageObjectRole::BrowserInferenceFallbackHistory],
        });
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        assert_eq!(m.format_version, 17);
        m.validate().unwrap();
        let raw = m.canonical_bytes().unwrap();
        let decoded: PortableProjectManifest = serde_json::from_slice(&raw).unwrap();
        assert_eq!(decoded.canonical_bytes().unwrap(), raw);
        for fault in ["format", "kind", "role", "length"] {
            let mut bad = m.clone();
            match fault {
                "format" => bad.format_version = 16,
                "kind" => bad.sources[0].kind = SourceKind::Media,
                "role" => {
                    bad.objects.last_mut().unwrap().roles =
                        vec![PackageObjectRole::DependencyEvidence]
                }
                _ => {
                    bad.sources[0]
                        .browser_fallback_history
                        .as_mut()
                        .unwrap()
                        .byte_len += 1
                }
            };
            assert!(bad.validate().is_err(), "{fault}");
        }
    }

    #[test]
    fn sparse_legacy_closure_and_exact_canonical_roundtrip() {
        let m = fixture();
        let bytes = m.canonical_bytes().unwrap();
        let decoded: PortableProjectManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.canonical_bytes().unwrap(), bytes);
        assert!(String::from_utf8(bytes)
            .unwrap()
            .contains("\"source_bytes\":\"needs_materialization\""));
    }
    #[test]
    fn missing_declared_record_or_required_object_fails() {
        let mut m = fixture();
        m.edit_states.clear();
        assert!(m.validate().is_err());
        let mut m = fixture();
        m.objects.clear();
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn duplicate_object_and_mismatched_roles_fail() {
        let mut m = fixture();
        m.objects.push(m.objects[0].clone());
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
        let mut m = fixture();
        m.objects[0].roles.push(PackageObjectRole::WorkerReceipt);
        assert!(m.validate().is_err());
    }
    #[test]
    fn authority_and_paths_are_not_manifest_fields() {
        for key in [
            "sessions",
            "grants",
            "auth_token",
            "worker_request",
            "sql",
            "destination_path",
        ] {
            let mut json = serde_json::to_value(fixture()).unwrap();
            json[key] = serde_json::json!([]);
            assert!(
                serde_json::from_value::<PortableProjectManifest>(json).is_err(),
                "{key}"
            );
        }
        let mut json = serde_json::to_value(fixture()).unwrap();
        json["objects"][0]["path"] = serde_json::json!("/etc/passwd");
        assert!(serde_json::from_value::<PortableProjectManifest>(json).is_err());
    }
    #[test]
    fn unknown_version_and_false_readiness_fail() {
        let mut m = fixture();
        m.format_version = 2;
        assert_eq!(m.validate().unwrap_err().code, ErrorCode::Unsupported);
        let mut json = serde_json::to_value(fixture()).unwrap();
        json["completeness"]["source_bytes"] = serde_json::json!("verified");
        assert!(serde_json::from_value::<PortableProjectManifest>(json).is_err());
    }
    #[test]
    fn exact_legacy_cell_reference_is_required_and_scoped() {
        let mut m = fixture();
        m.legacy_cells.push(PackageLegacyCell {
            owner: PackageLegacyOwner::Projects,
            row_key: "p".into(),
            object: PackageObjectRef {
                sha256: EMPTY_SHA.into(),
                byte_len: 13,
            },
        });
        m.objects[0]
            .roles
            .push(PackageObjectRole::LegacyMotionBytes);
        m.counts = m.record_counts().unwrap();
        m.validate().unwrap();
        m.legacy_cells[0].row_key = "other-project".into();
        assert!(m.validate().is_err());
    }
    #[test]
    fn broken_edges_fail_but_truncated_restoration_position_is_historical() {
        let mut m = fixture();
        let mut r = m.revisions[0].clone();
        r.revision = RevisionId::new(1);
        m.revisions.push(r);
        m.captured_revision = RevisionId::new(1);
        m.head.revision = RevisionId::new(1);
        m.revision_lineage.push(PackageRevisionLineage {
            revision: RevisionId::new(1),
            parent_revision: Some(RevisionId::new(0)),
            restored_from_revision: Some(RevisionId::new(0)),
            restored_history_position: Some(99),
            candidate: None,
            operation: "undo".into(),
            contributions: vec![],
            review: PackageReviewState {
                decisions: vec![],
                flags: vec![],
                unknown: true,
                conservative: true,
            },
        });
        m.counts = m.record_counts().unwrap();
        m.validate().unwrap();
        m.revision_lineage[0].parent_revision = Some(RevisionId::new(7));
        assert!(m.validate().is_err());
    }
    #[test]
    fn missing_authored_evidence_is_not_an_identity_only_dependency() {
        let mut m = fixture();
        let id = CandidateId::new("c").unwrap();
        m.candidates.push(PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: id.clone(),
            base_revision: RevisionId::new(0),
            motion: m.head.motion.clone(),
            job_origin: None,
            lineage: vec![],
            committed_revision: None,
            review: vec![],
        });
        m.authored_lineage.push(PackageAuthoredLineage {
            wasm_invocation: None,
            candidate_id: id,
            receipt: ArtifactIdentity {
                artifact_id: ArtifactId::new("receipt").unwrap(),
                sha256: "0".repeat(64),
                byte_len: 9,
            },
            input: PackageObjectRef {
                sha256: EMPTY_SHA.into(),
                byte_len: 13,
            },
            review: PackageReviewState {
                decisions: vec![],
                flags: vec![],
                unknown: true,
                conservative: false,
            },
            authored_count: 1,
            inherited_count: 0,
        });
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn current_head_must_not_have_future_persisted_revisions() {
        let mut m = fixture();
        let mut r = m.revisions[0].clone();
        r.revision = RevisionId::new(1);
        m.revisions.push(r);
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn retained_history_cannot_claim_unrelated_revision_content() {
        let mut m = fixture();
        let old = m.head.motion.clone();
        let mut new = old.clone();
        new.sha256 = "1".repeat(64);
        new.artifact_id = ArtifactId::new(format!("motion.{}", new.sha256)).unwrap();
        let mut r = m.revisions[0].clone();
        r.revision = RevisionId::new(1);
        r.motion = new.clone();
        m.revisions.push(r);
        m.captured_revision = RevisionId::new(1);
        m.head.revision = RevisionId::new(1);
        m.head.motion = new.clone();
        m.edit_states[0].motion = new;
        m.edit_states.push(PackageEditState {
            position: 1,
            motion: old,
            protected: vec![],
            source_revision: Some(RevisionId::new(1)),
        });
        m.objects.push(PackageObjectDescriptor {
            sha256: "1".repeat(64),
            byte_len: 13,
            roles: vec![PackageObjectRole::MotionProgram],
        });
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn truncated_recovery_retains_exact_colon_underscore_project_ownership() {
        let mut m = fixture();
        m.origin_project_id = ProjectId::new("p_:nested").unwrap();
        m.legacy_cells.push(PackageLegacyCell {
            owner: PackageLegacyOwner::EditStates,
            row_key: "p_:nested:99".into(),
            object: PackageObjectRef {
                sha256: EMPTY_SHA.into(),
                byte_len: 13,
            },
        });
        m.objects[0]
            .roles
            .push(PackageObjectRole::LegacyMotionBytes);
        m.counts = m.record_counts().unwrap();
        m.validate().unwrap();
        for key in [
            "pX:nested:99",
            "p_:nested_extra:99",
            "p_:nested:099",
            "p_:nested:+99",
            "p_:nested:18446744073709551616",
            "p_:nested:other:99",
        ] {
            m.legacy_cells[0].row_key = key.into();
            assert!(m.validate().is_err(), "{key}");
        }
    }
    #[test]
    fn image_origin_requires_format_thirteen_and_both_exact_evidence_roles() {
        let mut m = generated_fixture();
        let record = ArtifactIdentity {
            artifact_id: ArtifactId::new("image-origin").unwrap(),
            sha256: "6".repeat(64),
            byte_len: 2048,
        };
        let rgb = ArtifactIdentity {
            artifact_id: ArtifactId::new("image-pixels").unwrap(),
            sha256: "7".repeat(64),
            byte_len: 3,
        };
        m.generated_origins[0].image_origin = Some(PackageImageOrigin {
            record: record.clone(),
            rgb: rgb.clone(),
        });
        for identity in [&record, &rgb] {
            m.objects.push(PackageObjectDescriptor {
                sha256: identity.sha256.clone(),
                byte_len: identity.byte_len,
                roles: vec![PackageObjectRole::DependencyEvidence],
            });
        }
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        assert_eq!(m.format_version, 13);
        m.validate().unwrap();
        let mut old = m.clone();
        old.format_version = 12;
        assert!(old.validate().is_err());
        for identity in [&record, &rgb] {
            let mut bad = m.clone();
            bad.objects
                .iter_mut()
                .find(|o| o.sha256 == identity.sha256)
                .unwrap()
                .roles = vec![PackageObjectRole::SourceSnapshot];
            assert!(bad.validate().is_err());
        }
        let mut missing = m.clone();
        missing.generated_origins[0].image_origin = None;
        missing.format_version = missing.required_format_version();
        assert!(missing.validate().is_err());
        let mut oversized = m;
        oversized.generated_origins[0]
            .image_origin
            .as_mut()
            .unwrap()
            .rgb
            .byte_len = 512 * 512 * 3 + 1;
        assert!(oversized.validate().is_err());
    }
    #[test]
    fn annotated_model_requires_exact_format_and_payload_role() {
        let mut m = generated_fixture();
        let model = ArtifactIdentity {
            artifact_id: ArtifactId::new("6".repeat(64)).unwrap(),
            sha256: "6".repeat(64),
            byte_len: 9,
        };
        m.generated_origins[0].annotated_model = Some(model.clone());
        m.objects.push(PackageObjectDescriptor {
            sha256: model.sha256,
            byte_len: model.byte_len,
            roles: vec![PackageObjectRole::AnnotatedModel],
        });
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        assert_eq!(m.format_version, 11);
        m.validate().unwrap();
        let mut bad = m.clone();
        bad.format_version = 10;
        assert!(bad.validate().is_err());
        let mut bad = m.clone();
        bad.generated_origins[0].annotated_model = None;
        assert!(bad.validate().is_err());
        let mut bad = m.clone();
        bad.objects
            .retain(|o| !o.roles.contains(&PackageObjectRole::AnnotatedModel));
        bad.counts = bad.record_counts().unwrap();
        assert!(bad.validate().is_err());
        let mut bad = m.clone();
        bad.generated_origins[0].state = "failed".into();
        assert!(bad.validate().is_err());
        let mut bad = m.clone();
        bad.generated_origins[0]
            .annotated_model
            .as_mut()
            .unwrap()
            .byte_len = 256 * 1024 * 1024 + 1;
        assert!(bad.validate().is_err());
        let mut original = generated_fixture();
        let json = serde_json::to_value(&original.generated_origins[0]).unwrap();
        assert!(json.get("annotated_model").is_none());
        original.format_version = original.required_format_version();
        assert!(original.format_version < 11);
    }
    #[test]
    fn fallback_history_requires_format16_exact_role_and_bounded_object() {
        let legacy = generated_fixture();
        assert!(serde_json::to_value(&legacy.generated_origins[0])
            .unwrap()
            .get("fallback_history")
            .is_none());
        let history = PackageObjectRef {
            sha256: "7".repeat(64),
            byte_len: 100,
        };
        let mut current = legacy.clone();
        current.generated_origins[0].fallback_history = Some(history.clone());
        assert_eq!(
            current.required_format_version(),
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY
        );
        current.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY;
        assert!(current.validate().is_err());
        current.objects.push(PackageObjectDescriptor {
            sha256: history.sha256.clone(),
            byte_len: history.byte_len,
            roles: vec![PackageObjectRole::InferenceFallbackHistory],
        });
        current.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        current.counts = current.record_counts().unwrap();
        current.validate().unwrap();
        let bytes = current.canonical_bytes().unwrap();
        let restored: PortableProjectManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.canonical_bytes().unwrap(), bytes);
        for version in [1, 14, 15] {
            let mut bad = current.clone();
            bad.format_version = version;
            assert!(bad.validate().is_err());
        }
        let mut bad = current.clone();
        bad.objects
            .iter_mut()
            .find(|o| o.sha256 == history.sha256)
            .unwrap()
            .roles = vec![PackageObjectRole::DependencyEvidence];
        assert!(bad.validate().is_err());
        for size in [0, MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64 + 1] {
            let mut bad = current.clone();
            bad.generated_origins[0]
                .fallback_history
                .as_mut()
                .unwrap()
                .byte_len = size;
            bad.objects
                .iter_mut()
                .find(|o| o.sha256 == history.sha256)
                .unwrap()
                .byte_len = size;
            assert!(bad.validate().is_err());
        }
        let mut paused = current.clone();
        paused.generated_origins[0].state = "assistant_paused".into();
        paused.generated_origins[0].receipt = None;
        paused.generated_origins[0].program = None;
        // Remove the candidate and its otherwise-required worker receipt role.
        paused.candidates.clear();
        paused
            .objects
            .retain(|object| object.sha256 != "2".repeat(64));
        for object in &mut paused.objects {
            object
                .roles
                .retain(|role| *role != PackageObjectRole::DependencyEvidence);
        }
        paused.counts = paused.record_counts().unwrap();
        paused.validate().unwrap();
        let mut legacy_paused = legacy;
        legacy_paused.generated_origins[0].state = "assistant_paused".into();
        assert!(legacy_paused.validate().is_err());
        let mut header = vec![0; crate::PACKAGE_IMPORT_HEADER_BYTES];
        header[..8].copy_from_slice(b"PULSPKG\0");
        header[12..20].copy_from_slice(&1u64.to_le_bytes());
        header[8..10]
            .copy_from_slice(&PROJECT_PACKAGE_FORMAT_VERSION_WITH_FALLBACK_HISTORY.to_le_bytes());
        assert_eq!(crate::package_format_version(&header).unwrap(), 16);
        header[8..10].copy_from_slice(
            &(PROJECT_PACKAGE_FORMAT_VERSION_WITH_BROWSER_FALLBACK_HISTORY + 1).to_le_bytes(),
        );
        assert_eq!(
            crate::package_format_version(&header).unwrap_err().code,
            crate::ErrorCode::Unsupported
        );
    }

    #[test]
    fn cpu_image_history_requires_format15_and_preserves_legacy_bytes() {
        let legacy = generated_fixture();
        let value = serde_json::to_value(&legacy.generated_origins[0]).unwrap();
        assert!(value.get("cpu_runtime").is_none());
        let identity = |n: u8| ArtifactIdentity {
            artifact_id: crate::ArtifactId::new(format!("{n:064x}")).unwrap(),
            sha256: format!("{n:064x}"),
            byte_len: 1,
        };
        let image = crate::CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries: crate::CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(n, alias)| crate::CapturedNativeLibrary {
                    identity: identity(n as u8 + 1),
                    names: vec![(*alias).into()],
                })
                .collect(),
        };
        let mut current = legacy;
        current.generated_origins[0].cpu_runtime = Some(Box::new(image));
        assert_eq!(
            current.required_format_version(),
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        current.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY;
        current.validate().unwrap();
        let bytes = current.canonical_bytes().unwrap();
        let restored: PortableProjectManifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.canonical_bytes().unwrap(), bytes);
        current.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS;
        assert!(current.validate().is_err());
        current.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY;
        current.generated_origins[0]
            .cpu_runtime
            .as_mut()
            .unwrap()
            .libraries[0]
            .names
            .push("libc.so.6".into());
        assert!(current.validate().is_err());
    }
    fn generated_fixture() -> PortableProjectManifest {
        let mut m = fixture();
        let job = JobId::new("j").unwrap();
        let source = SourceVersionId::new("s").unwrap();
        let receipt = ArtifactIdentity {
            artifact_id: ArtifactId::new("receipt").unwrap(),
            sha256: "2".repeat(64),
            byte_len: 9,
        };
        m.candidates.push(PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: CandidateId::new("c").unwrap(),
            base_revision: RevisionId::new(0),
            motion: m.head.motion.clone(),
            job_origin: Some(job.clone()),
            lineage: vec![receipt],
            committed_revision: None,
            review: vec![],
        });
        m.sources.push(PackageSource {
            browser_fallback_history: None,
            source_version: source.clone(),
            kind: SourceKind::Media,
            label: "source".into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("source").unwrap(),
                sha256: "3".repeat(64),
                byte_len: 9,
            },
            pinned: false,
            evicted: false,
            stash_associations: vec![],
        });
        m.generated_origins.push(PackageGeneratedOrigin {
            fallback_history: None,
            cpu_runtime: None,
            image_origin: None,
            annotated_model: None,
            cuda_evidence: None,
            job_id: job,
            attempt_id: AttemptId::new("attempt").unwrap(),
            base_revision: RevisionId::new(0),
            source_version: source,
            manifest: PackageObjectRef {
                sha256: "4".repeat(64),
                byte_len: 9,
            },
            receipt: None,
            program: None,
            state: "completed".into(),
        });
        for (sha, role) in [
            ("3", PackageObjectRole::SourceSnapshot),
            ("4", PackageObjectRole::WorkerManifest),
        ] {
            m.objects.push(PackageObjectDescriptor {
                sha256: sha.repeat(64),
                byte_len: 9,
                roles: vec![role],
            });
        }
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
        m.generated_origins[0].receipt = Some(m.candidates[0].lineage[0].clone());
        m.generated_origins[0].program = Some(ArtifactIdentity {
            artifact_id: m.head.motion.artifact_id.clone(),
            sha256: m.head.motion.sha256.clone(),
            byte_len: m.head.motion.byte_len,
        });
        m.objects
            .iter_mut()
            .find(|o| o.sha256 == EMPTY_SHA)
            .unwrap()
            .roles
            .push(PackageObjectRole::DependencyEvidence);
        m.objects.push(PackageObjectDescriptor {
            sha256: "2".repeat(64),
            byte_len: 9,
            roles: vec![PackageObjectRole::WorkerReceipt],
        });
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.counts = m.record_counts().unwrap();
        m.validate().unwrap();
        m
    }
    #[test]
    fn generated_candidate_cannot_drop_its_exact_worker_receipt() {
        let mut m = generated_fixture();
        let receipt = m.generated_origins[0].receipt.take();
        let raw_program = m.generated_origins[0].program.take();
        assert!(m.validate().is_err());
        m.generated_origins[0].receipt = receipt;
        m.generated_origins[0].program = raw_program;
        let program = m.generated_origins[0].program.take();
        assert!(m.validate().is_err());
        m.generated_origins[0].program = program;
        m.generated_origins[0].receipt.as_mut().unwrap().artifact_id =
            ArtifactId::new("different-origin-receipt").unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn archived_cuda_roles_bind_membership_and_preserve_general_receipt() {
        let mut m = generated_fixture();
        let identity = |name: &str, digit: &str| ArtifactIdentity {
            artifact_id: ArtifactId::new(name).unwrap(),
            sha256: digit.repeat(64),
            byte_len: 9,
        };
        let receipt = identity("cuda-receipt", "5");
        let trace = identity("cuda-trace", "6");
        m.candidates[0]
            .lineage
            .extend([receipt.clone(), trace.clone()]);
        m.generated_origins[0].cuda_evidence = Some(PackageCudaEvidence {
            accepted_candidate: m.candidates[0].candidate_id.clone(),
            request_sha256: m.generated_origins[0].manifest.sha256.clone(),
            receipt: receipt.clone(),
            trace: trace.clone(),
        });
        for (identity, role) in [
            (receipt, PackageObjectRole::CudaExecutionReceipt),
            (trace, PackageObjectRole::CudaNodeTrace),
        ] {
            m.objects.push(PackageObjectDescriptor {
                sha256: identity.sha256,
                byte_len: identity.byte_len,
                roles: vec![role],
            });
        }
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        assert_eq!(m.format_version, 10);
        m.validate().unwrap();
        let mut invalid = m.clone();
        invalid.format_version = 9;
        assert!(invalid.validate().is_err());
        let mut invalid = m.clone();
        invalid.candidates[0].lineage.swap(1, 2);
        assert!(invalid.validate().is_err());
        let mut invalid = m.clone();
        invalid.generated_origins[0]
            .cuda_evidence
            .as_mut()
            .unwrap()
            .accepted_candidate = CandidateId::new("absent").unwrap();
        assert!(invalid.validate().is_err());
        let mut invalid = m.clone();
        invalid.generated_origins[0]
            .cuda_evidence
            .as_mut()
            .unwrap()
            .request_sha256 = "7".repeat(64);
        assert!(invalid.validate().is_err());
        let mut invalid = m.clone();
        invalid.generated_origins[0].state = "running".into();
        assert!(invalid.validate().is_err());
        for digest in ["5", "6"] {
            let mut invalid = m.clone();
            invalid
                .objects
                .iter_mut()
                .find(|o| o.sha256 == digest.repeat(64))
                .unwrap()
                .roles = vec![PackageObjectRole::DependencyEvidence];
            assert!(invalid.validate().is_err());
        }
        let mut invalid = m.clone();
        invalid.generated_origins[0]
            .cuda_evidence
            .as_mut()
            .unwrap()
            .trace
            .byte_len = 8 * 1024 * 1024 + 1;
        assert!(invalid.validate().is_err());
        let model = identity("annotated-model", "8");
        m.generated_origins[0].annotated_model = Some(model.clone());
        m.objects.push(PackageObjectDescriptor {
            sha256: model.sha256,
            byte_len: model.byte_len,
            roles: vec![PackageObjectRole::AnnotatedModel],
        });
        m.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        assert_eq!(m.format_version, 11);
        m.validate().unwrap();
        let mut invalid = m.clone();
        invalid.format_version = 10;
        assert!(invalid.validate().is_err());
        for role in [
            PackageObjectRole::AnnotatedModel,
            PackageObjectRole::CudaExecutionReceipt,
            PackageObjectRole::CudaNodeTrace,
        ] {
            let mut invalid = m.clone();
            invalid
                .objects
                .iter_mut()
                .find(|o| o.roles.contains(&role))
                .unwrap()
                .roles = vec![PackageObjectRole::DependencyEvidence];
            assert!(invalid.validate().is_err(), "{role:?}");
        }
        let mut invalid = m;
        invalid.candidates[0].lineage.swap(1, 2);
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn retained_history_positions_cannot_have_holes() {
        let mut m = fixture();
        let mut state = m.edit_states[0].clone();
        state.position = 2;
        m.edit_states.push(state);
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn candidate_commit_and_present_revision_lineage_must_agree() {
        let mut m = fixture();
        let candidate = CandidateId::new("c").unwrap();
        let mut r = m.revisions[0].clone();
        r.revision = RevisionId::new(1);
        m.revisions.push(r);
        m.captured_revision = RevisionId::new(1);
        m.head.revision = RevisionId::new(1);
        m.candidates.push(PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: candidate.clone(),
            base_revision: RevisionId::new(0),
            motion: m.head.motion.clone(),
            job_origin: None,
            lineage: vec![],
            committed_revision: None,
            review: vec![],
        });
        m.revision_lineage.push(PackageRevisionLineage {
            revision: RevisionId::new(1),
            parent_revision: Some(RevisionId::new(0)),
            restored_from_revision: None,
            restored_history_position: None,
            candidate: Some(candidate),
            operation: "candidate_commit".into(),
            contributions: vec![],
            review: PackageReviewState {
                decisions: vec![],
                flags: vec![],
                unknown: false,
                conservative: false,
            },
        });
        m.counts = m.record_counts().unwrap();
        assert!(m.validate().is_err());
        m.candidates[0].committed_revision = Some(RevisionId::new(0));
        assert!(m.validate().is_err());
        m.candidates[0].committed_revision = Some(RevisionId::new(1));
        m.validate().unwrap();
    }
    #[test]
    fn stash_association_requires_new_reader_exact_payload_and_explicit_bounds() {
        let source = PackageSource {
            browser_fallback_history: None,
            source_version: SourceVersionId::new("source").unwrap(),
            kind: SourceKind::Media,
            label: "source".into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("source").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 3,
            },
            pinned: true,
            evicted: false,
            stash_associations: vec![],
        };
        let mut association = PackageStashAssociation {
            endpoint: Some("http://localhost:9999/graphql".into()),
            scene_id: "scene".into(),
            file_id: "file".into(),
            server_path: "/stash/source.mp4".into(),
            capture_receipt: PackageObjectRef {
                sha256: "b".repeat(64),
                byte_len: 123,
            },
        };
        let mut capture = crate::StashSourceCapture {
            endpoint: association.endpoint.clone(),
            source_version: source.source_version.clone(),
            identity: source.identity.clone(),
            scene_id: association.scene_id.clone(),
            file_id: association.file_id.clone(),
            server_path: association.server_path.clone(),
            local_source: crate::test_host_path("/inert/source.mp4").into(),
            script_destination: crate::test_host_path("/inert/source.funscript").into(),
        };
        association.check_capture(&source, &capture).unwrap();
        capture.file_id = "different".into();
        assert!(association.check_capture(&source, &capture).is_err());
        association.capture_receipt.byte_len = 65537;
        assert!(association.validate().is_err());
        association.capture_receipt.byte_len = 123;
        association.endpoint = Some("http://user:password@localhost/graphql".into());
        assert!(association.validate().is_err());
        association.endpoint = None;
        let mut m = fixture();
        let mut declared = source;
        declared.stash_associations.push(association);
        m.sources.push(declared);
        assert_eq!(m.required_format_version(), 8);
        let legacy = serde_json::to_value(&m.sources[0]).unwrap();
        let mut omitted = legacy;
        omitted
            .as_object_mut()
            .unwrap()
            .remove("stash_associations");
        assert!(serde_json::from_value::<PackageSource>(omitted)
            .unwrap()
            .stash_associations
            .is_empty());
    }
    #[test]
    fn rebase_links_require_unchanged_evidence_acyclic_ancestry_and_new_format() {
        let mut m = fixture();
        let parent = PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: CandidateId::new("parent").unwrap(),
            base_revision: m.head.revision,
            motion: m.head.motion.clone(),
            job_origin: None,
            lineage: vec![],
            committed_revision: None,
            review: vec![],
        };
        let mut child = parent.clone();
        child.candidate_id = CandidateId::new("child").unwrap();
        child.rebased_from = Some(parent.candidate_id.clone());
        m.candidates = vec![child, parent];
        m.format_version = m.required_format_version();
        m.counts = m.record_counts().unwrap();
        m.validate().unwrap();
        assert_eq!(m.format_version, 9);
        let mut old = m.clone();
        old.format_version = 8;
        assert!(old.validate().is_err());
        let mut missing = m.clone();
        missing.candidates[0].rebased_from = Some(CandidateId::new("missing").unwrap());
        assert!(missing.validate().is_err());
        let mut cycle = m.clone();
        cycle.candidates[1].rebased_from = Some(cycle.candidates[0].candidate_id.clone());
        assert!(cycle.validate().is_err());
        let mut changed = m.clone();
        changed.candidates[0].project_interval = Some(
            TimeRange::new(crate::ProjectTime::ZERO, crate::ProjectTime::from_nanos(1)).unwrap(),
        );
        assert!(changed.validate().is_err());
    }
    #[test]
    fn manifest_json_is_bounded_before_growing_past_limit() {
        assert!(bounded_json(&"0123456789", 5).is_err());
        let mut m = fixture();
        m.head.name = "x".repeat(257);
        assert!(m.validate().is_err());
    }
    #[test]
    fn device_export_scope_preserves_legacy_axis_json_and_refuses_unknown() {
        for axis in Axis::ALL {
            let old = serde_json::to_vec(&axis).unwrap();
            assert_eq!(
                serde_json::to_vec(&PackageExportScope::Axis(axis)).unwrap(),
                old
            );
            assert_eq!(
                serde_json::from_slice::<PackageExportScope>(&old).unwrap(),
                PackageExportScope::Axis(axis)
            );
        }
        assert_eq!(
            serde_json::from_str::<PackageExportScope>("\"device-motion\"").unwrap(),
            PackageExportScope::DeviceMotion
        );
        for invalid in ["\"all\"", "\"device_motion\"", "null", "{}", "0"] {
            assert!(serde_json::from_str::<PackageExportScope>(invalid).is_err());
        }
    }
    #[test]
    fn device_export_scope_requires_v14_without_promoting_export_state() {
        for state in [
            PackageExportState::Pending,
            PackageExportState::Complete,
            PackageExportState::FailedOrUnknown,
        ] {
            let mut m = fixture();
            m.export_receipts.push(PackageExportReceipt {
                revision: RevisionId::new(0),
                axis: PackageExportScope::DeviceMotion,
                sha256: "a".repeat(64),
                state,
                archival_path: Some("/untrusted/never-resolve-proposal.json".into()),
            });
            m.counts = m.record_counts().unwrap();
            assert_eq!(
                m.required_format_version(),
                PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS
            );
            for version in 1..=13 {
                m.format_version = version;
                assert!(m.validate().is_err());
            }
            m.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_DEVICE_EXPORTS;
            let bytes = m.canonical_bytes().unwrap();
            let decoded: PortableProjectManifest = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(decoded.canonical_bytes().unwrap(), bytes);
            assert_eq!(
                serde_json::to_value(decoded.export_receipts[0].state).unwrap(),
                serde_json::to_value(state).unwrap()
            );
            m.export_receipts[0].axis = PackageExportScope::Axis(Axis::Stroke);
            m.format_version = 1;
            m.validate().unwrap();
            assert_eq!(m.required_format_version(), 1);
        }
    }
}
