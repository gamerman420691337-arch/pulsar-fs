//! Inert namespaces for exact original manifests. These mappings never adopt
//! original sessions, permissions, jobs, or attempts as local authority.
use crate::{
    CandidateId, ErrorCode, PackageArtifactDescriptor, PackageObjectDescriptor, PackageObjectRef,
    PortableProjectManifest, ProtocolError, RevisionId, SourcePlacementId, SourceVersionId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_PACKAGE_IMPORTED_ORIGINS: usize = 64;
pub const MAX_PACKAGE_ORIGIN_DEPTH: usize = 64;
pub const MAX_PACKAGE_ORIGIN_MANIFEST_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PACKAGE_ORIGIN_ENTRIES: usize = 500_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRevisionOrigin {
    pub local: RevisionId,
    pub origin: RevisionId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCandidateOrigin {
    pub local: CandidateId,
    pub origin: CandidateId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSourceOrigin {
    pub local: SourceVersionId,
    pub origin: SourceVersionId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePlacementOrigin {
    pub local: SourcePlacementId,
    pub origin: SourcePlacementId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageActorOrigin {
    pub local: String,
    pub origin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageArchiveOrigin {
    /// Exact original canonical manifest bytes; SHA-256 is this namespace.
    pub manifest: PackageObjectRef,
    /// Original complete container identity, not a nested container payload.
    pub container: PackageArtifactDescriptor,
    /// Exact original catalog. Every declared payload remains required.
    pub objects: Vec<PackageObjectDescriptor>,
    pub revisions: Vec<PackageRevisionOrigin>,
    pub candidates: Vec<PackageCandidateOrigin>,
    pub sources: Vec<PackageSourceOrigin>,
    pub actors: Vec<PackageActorOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<PackagePlacementOrigin>,
}
impl PackageArchiveOrigin {
    /// Catalog and mapping entries are charged even when another namespace
    /// repeats them. The graph validator additionally counts decoded records.
    pub fn entry_count(&self) -> Result<usize, ProtocolError> {
        [
            self.objects.len(),
            self.revisions.len(),
            self.candidates.len(),
            self.sources.len(),
            self.actors.len(),
            self.placements.len(),
        ]
        .into_iter()
        .try_fold(0usize, |sum, count| sum.checked_add(count))
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::ResourceExhausted, "origin entry count overflow")
        })
    }
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidRequest, message)
}
fn sorted<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let mut previous = None;
    for item in items {
        if previous.as_ref().is_some_and(|value| value >= &item) {
            return false;
        }
        previous = Some(item);
    }
    true
}
fn actor(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0')
}

/// Checks only bounded shape and references to the current projection. A
/// successful return is not original-byte verification. The importer must
/// resolve all original manifests, check exact catalogs and mappings, reject
/// cycles, and enforce aggregate bounds over the distinct ancestor graph.
pub fn validate_imported_origin_shapes(
    manifest: &PortableProjectManifest,
) -> Result<(), ProtocolError> {
    if manifest.imported_origins.len() > MAX_PACKAGE_IMPORTED_ORIGINS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "too many direct origin manifests",
        ));
    }
    if !sorted(
        manifest
            .imported_origins
            .iter()
            .map(|origin| origin.manifest.sha256.as_str()),
    ) {
        return Err(invalid(
            "origin namespaces must be unique and SHA-256 ordered",
        ));
    }
    let revisions: BTreeSet<_> = manifest
        .revisions
        .iter()
        .map(|revision| revision.revision)
        .collect();
    let candidates: BTreeSet<_> = manifest
        .candidates
        .iter()
        .map(|candidate| candidate.candidate_id.as_str())
        .collect();
    let sources: BTreeSet<_> = manifest
        .sources
        .iter()
        .map(|source| source.source_version.as_str())
        .collect();
    let actors: BTreeSet<_> = manifest
        .revisions
        .iter()
        .map(|revision| revision.actor.as_str())
        .collect();
    let placements: BTreeSet<_> = manifest
        .placements
        .iter()
        .flat_map(|p| p.definitions.iter().map(|d| &d.placement_id))
        .collect();
    let mut entries = 0usize;
    let mut bytes = 0u64;
    for origin in &manifest.imported_origins {
        origin.manifest.validate()?;
        origin.container.validate()?;
        if origin.manifest.byte_len == 0
            || origin.manifest.byte_len > crate::MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64
            || origin.manifest.sha256 != origin.container.manifest_sha256
            || u64::from(origin.container.object_count) != origin.objects.len() as u64
        {
            return Err(invalid(
                "origin manifest differs from its container binding",
            ));
        }
        entries = entries
            .checked_add(origin.entry_count()?)
            .ok_or_else(|| invalid("origin entry count overflow"))?;
        bytes = bytes
            .checked_add(origin.manifest.byte_len)
            .ok_or_else(|| invalid("origin byte count overflow"))?;
        if entries > MAX_PACKAGE_ORIGIN_ENTRIES || bytes > MAX_PACKAGE_ORIGIN_MANIFEST_BYTES {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "origin metadata exceeds its bound",
            ));
        }
        if !sorted(origin.objects.iter().map(|object| object.sha256.as_str()))
            || !sorted(origin.revisions.iter().map(|map| map.local))
            || !sorted(origin.candidates.iter().map(|map| map.local.as_str()))
            || !sorted(origin.sources.iter().map(|map| map.local.as_str()))
            || !sorted(origin.actors.iter().map(|map| map.local.as_str()))
            || !sorted(origin.placements.iter().map(|map| &map.local))
        {
            return Err(invalid(
                "origin catalogs and local mapping keys must be unique and ordered",
            ));
        }
        for object in &origin.objects {
            object.validate()?;
        }
        if origin
            .revisions
            .iter()
            .any(|map| !revisions.contains(&map.local))
            || origin
                .candidates
                .iter()
                .any(|map| !candidates.contains(map.local.as_str()))
            || origin
                .sources
                .iter()
                .any(|map| !sources.contains(map.local.as_str()))
            || origin
                .placements
                .iter()
                .any(|map| !placements.contains(&map.local))
            || origin.actors.iter().any(|map| {
                !actors.contains(map.local.as_str()) || !actor(&map.local) || !actor(&map.origin)
            })
        {
            return Err(invalid(
                "origin mapping references a missing local projection record",
            ));
        }
    }
    Ok(())
}

/// Checked historical generation facts, never a live job or publication grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedGenerationOrigin {
    pub origin_manifest: PackageObjectRef,
    pub project_id: crate::ProjectId,
    pub candidate_id: CandidateId,
    pub job_id: crate::JobId,
    pub attempt_id: crate::AttemptId,
    pub source_version: SourceVersionId,
    pub source_identity: crate::ArtifactIdentity,
    pub program: crate::ArtifactIdentity,
    pub worker_manifest: PackageObjectRef,
    pub worker_receipt: crate::ArtifactIdentity,
    pub preset: String,
    pub settings: crate::GenerationSettings,
    pub clock: Option<ArchivedGenerationClock>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedGenerationClock {
    pub receipt: crate::ArtifactIdentity,
    pub sample_count: u64,
    pub source_origin: crate::SourceTimestamp,
    pub project_trim: crate::TimeRange,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedGenerationReport {
    pub project_id: crate::ProjectId,
    pub candidate_id: CandidateId,
    pub origins: Vec<ArchivedGenerationOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub master_derivation: Option<ArchivedMasterDerivation>,
    pub archival: bool,
    pub active_job: bool,
    pub publication_authority: bool,
    pub release_qualified: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedMasterDerivation {
    pub revision: RevisionId,
    pub master: crate::ArtifactIdentity,
    pub source_version: SourceVersionId,
    pub anchor_candidate: CandidateId,
    pub retiming_revisions: Vec<RevisionId>,
    pub publication_authority: bool,
}
impl ArchivedGenerationReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !self.archival
            || self.active_job
            || self.publication_authority
            || self.release_qualified
            || self.origins.is_empty()
            || self.origins.len() > 16
        {
            return Err(ProtocolError::invalid(
                "archived generation facts cannot assert live or publication authority",
            ));
        }
        let mut previous = None;
        for origin in &self.origins {
            origin.origin_manifest.validate()?;
            origin.worker_manifest.validate()?;
            origin.source_identity.validate()?;
            origin.program.validate()?;
            origin.worker_receipt.validate()?;
            origin.settings.validate()?;
            if previous.is_some_and(|key: (&str, &str)| {
                key >= (
                    origin.origin_manifest.sha256.as_str(),
                    origin.candidate_id.as_str(),
                )
            }) {
                return Err(ProtocolError::invalid(
                    "archived generation origins must be unique and sorted",
                ));
            }
            previous = Some((
                origin.origin_manifest.sha256.as_str(),
                origin.candidate_id.as_str(),
            ));
            if origin.source_version.as_str() != origin.source_identity.sha256
                || origin.worker_receipt.artifact_id.as_str() != origin.worker_receipt.sha256
                || origin.program.artifact_id.as_str()
                    != format!("motion.{}", origin.program.sha256)
            {
                return Err(ProtocolError::invalid(
                    "archived generation artifact correspondence",
                ));
            }
            match (&origin.settings.source_placement, &origin.clock) {
                (None, None) => {}
                (Some(_), Some(clock))
                    if clock.receipt == origin.worker_receipt
                        && clock.sample_count > 0
                        && clock.sample_count <= 1_000_000 => {}
                _ => {
                    return Err(ProtocolError::invalid(
                        "archived generation clock declaration differs from checked receipt",
                    ))
                }
            }
        }
        if let Some(master) = &self.master_derivation {
            master.master.validate()?;
            if master.publication_authority
                || master.anchor_candidate != self.candidate_id
                || self
                    .origins
                    .iter()
                    .any(|o| o.source_version != master.source_version)
                || master.master.artifact_id.as_str() != format!("motion.{}", master.master.sha256)
                || master.retiming_revisions.len() > 128
                || master
                    .retiming_revisions
                    .iter()
                    .any(|r| *r > master.revision)
                || master.retiming_revisions.windows(2).any(|r| r[0] >= r[1])
            {
                return Err(ProtocolError::invalid(
                    "archived master derivation correspondence",
                ));
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("archived generation encoding"))?
            .len()
            > 768 * 1024
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "archived generation report exceeds bounded control response",
            ));
        }
        Ok(())
    }
}
