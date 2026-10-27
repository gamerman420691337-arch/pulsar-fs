//! Pure clone-import projection. Archive identities never become local authority.
//!
//! This module verifies canonical manifest identities, semantic namespace maps,
//! and bounded graph closure. It does NOT verify the complete container digest:
//! storage must reconstruct every ancestor from the exact manifest and payload
//! bytes before publishing a VerifiedImportBytes capability.
use pulsar_protocol::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

type PResult<T> = Result<T, ProtocolError>;

#[derive(Clone, Debug)]
pub(crate) struct CloneIdentityAssignment {
    pub project_id: ProjectId,
    /// Complete, one-to-one incoming candidate -> fresh local candidate map.
    pub candidates: BTreeMap<CandidateId, CandidateId>,
    pub placements: BTreeMap<SourcePlacementId, SourcePlacementId>,
}

#[derive(Clone, Debug)]
pub(crate) struct CloneImportPlan {
    pub manifest: PortableProjectManifest,
    pub origin_manifest: Vec<u8>,
    pub origin_manifest_ref: PackageObjectRef,
    pub identities: CloneIdentityAssignment,
    /// Exact stored source state. Runtime imported-trust overlays are separate.
    pub candidate_reviews: BTreeMap<CandidateId, PackageReviewState>,
}

/// Unbound declarations cannot become ordinary history. Complete format15 CPU
/// origins use the contextual image checker; fallback history remains refused.
pub(crate) fn require_portable_cpu_dependencies(dependencies: &[ArtifactIdentity]) -> PResult<()> {
    if dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    }) {
        return Err(ProtocolError::unsupported(
            "portable closed CPU runtime history is not implemented",
        ));
    }
    if dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
    }) {
        return Err(ProtocolError::unsupported(
            "portable inference fallback history is not implemented",
        ));
    }
    Ok(())
}

pub(crate) fn require_portable_cpu_manifest(manifest: &PortableProjectManifest) -> PResult<()> {
    for candidate in &manifest.candidates {
        crate::portable_cpu_archives::require_no_fallback(&candidate.lineage)?;
    }
    if manifest.imported_origins.is_empty() {
        crate::portable_cpu_archives::check_graph(manifest, &BTreeMap::new())?;
    }
    Ok(())
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidRequest, message)
}
fn exhausted(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::ResourceExhausted, message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn same<T: Serialize>(left: &T, right: &T) -> PResult<bool> {
    let left = serde_json::to_vec(left).map_err(|_| invalid("cannot encode archival metadata"))?;
    let right =
        serde_json::to_vec(right).map_err(|_| invalid("cannot encode archival metadata"))?;
    Ok(left == right)
}
fn checked_add(total: &mut usize, amount: usize) -> PResult<()> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| exhausted("origin count overflow"))?;
    if *total > MAX_PACKAGE_ORIGIN_ENTRIES {
        return Err(exhausted("origin graph exceeds the aggregate entry limit"));
    }
    Ok(())
}
fn logical_entries(manifest: &PortableProjectManifest) -> PResult<usize> {
    [
        1,
        manifest.revisions.len(),
        manifest.edit_states.len(),
        manifest.revision_lineage.len(),
        manifest.candidates.len(),
        manifest.authored_lineage.len(),
        manifest.sources.len(),
        manifest.legacy_cells.len(),
        manifest.generated_origins.len(),
        manifest.export_receipts.len(),
        manifest.objects.len(),
        manifest.imported_origins.len(),
        manifest
            .placements
            .as_ref()
            .map(|p| p.entry_count())
            .transpose()?
            .unwrap_or(0),
        manifest.retiming_receipts.len(),
    ]
    .into_iter()
    .try_fold(0usize, |total, count| total.checked_add(count))
    .ok_or_else(|| exhausted("origin logical count overflow"))
}
fn map_entries(manifest: &PortableProjectManifest) -> PResult<usize> {
    manifest
        .imported_origins
        .iter()
        .try_fold(0usize, |total, origin| {
            total
                .checked_add(origin.entry_count()?)
                .ok_or_else(|| exhausted("origin map count overflow"))
        })
}
fn bind_container(
    manifest: &PortableProjectManifest,
    container: &PackageArtifactDescriptor,
    bytes: &[u8],
) -> PResult<()> {
    container.validate()?;
    if container.format_version != manifest.format_version
        || container.manifest_sha256 != digest(bytes)
        || container.project_id != manifest.origin_project_id
        || container.captured_revision != manifest.captured_revision
        || container.captured_event_cursor != manifest.captured_event_cursor
        || u64::from(container.object_count) != manifest.objects.len() as u64
    {
        return Err(invalid(
            "original container metadata disagrees with its canonical manifest",
        ));
    }
    Ok(())
}

struct Index<'a> {
    history_zero: Option<&'a PackageEditState>,
    revisions: BTreeMap<RevisionId, &'a PackageRevision>,
    lineage: BTreeMap<RevisionId, &'a PackageRevisionLineage>,
    candidates: BTreeMap<&'a str, &'a PackageCandidate>,
    sources: BTreeMap<&'a str, &'a PackageSource>,
    actors: BTreeSet<&'a str>,
    placements: BTreeMap<&'a SourcePlacementId, &'a SourcePlacement>,
    placement_revisions: BTreeMap<RevisionId, &'a [SourcePlacementId]>,
    placement_zero: &'a [SourcePlacementId],
}
impl<'a> Index<'a> {
    fn new(manifest: &'a PortableProjectManifest) -> Self {
        Self {
            history_zero: manifest
                .edit_states
                .first()
                .filter(|state| state.position == 0),
            revisions: manifest.revisions.iter().map(|r| (r.revision, r)).collect(),
            lineage: manifest
                .revision_lineage
                .iter()
                .map(|r| (r.revision, r))
                .collect(),
            candidates: manifest
                .candidates
                .iter()
                .map(|c| (c.candidate_id.as_str(), c))
                .collect(),
            sources: manifest
                .sources
                .iter()
                .map(|s| (s.source_version.as_str(), s))
                .collect(),
            actors: manifest
                .revisions
                .iter()
                .map(|r| r.actor.as_str())
                .collect(),
            placements: manifest
                .placements
                .iter()
                .flat_map(|p| p.definitions.iter().map(|d| (&d.placement_id, d)))
                .collect(),
            placement_revisions: manifest
                .placements
                .iter()
                .flat_map(|p| {
                    p.revisions
                        .iter()
                        .map(|r| (r.revision, r.placements.as_slice()))
                })
                .collect(),
            placement_zero: manifest
                .placements
                .as_ref()
                .and_then(|p| p.history.iter().find(|r| r.position == 0))
                .map(|r| r.placements.as_slice())
                .unwrap_or(&[]),
        }
    }
}

/// A mapping claims preserved historical meaning, not just two existing IDs.
/// Mutable pin/eviction state, candidate rebasing, and subsequent commits do not
/// change the immutable program/evidence that the origin association describes.
fn validate_mapping(
    local: &Index<'_>,
    original: &Index<'_>,
    origin: &PackageArchiveOrigin,
) -> PResult<()> {
    // History suffixes can be replaced by branching, but no engine operation
    // edits position zero. Preserve its exact legacy absence/presence of a
    // source-revision link as well as motion and protection.
    let (Some(local_zero), Some(original_zero)) = (local.history_zero, original.history_zero)
    else {
        return Err(invalid(
            "origin history is missing its immutable initial state",
        ));
    };
    if !same(local_zero, original_zero)? {
        return Err(invalid(
            "origin mapping changed the immutable initial history state",
        ));
    }
    let revisions: BTreeMap<_, _> = origin
        .revisions
        .iter()
        .map(|m| (m.local, m.origin))
        .collect();
    let candidates: BTreeMap<_, _> = origin
        .candidates
        .iter()
        .map(|m| (m.local.as_str(), m.origin.as_str()))
        .collect();
    let placements: BTreeMap<_, _> = origin
        .placements
        .iter()
        .map(|m| (&m.local, &m.origin))
        .collect();
    let actors: BTreeMap<_, _> = origin
        .actors
        .iter()
        .map(|m| (m.local.as_str(), m.origin.as_str()))
        .collect();
    // Every original record survives clone. Extra local rows are later edits;
    // aliases may share candidate ancestry, but cannot silently omit originals.
    let target_revisions: BTreeSet<_> = revisions.values().copied().collect();
    let target_candidates: BTreeSet<_> = candidates.values().copied().collect();
    let target_sources: BTreeSet<_> = origin.sources.iter().map(|m| m.origin.as_str()).collect();
    let target_actors: BTreeSet<_> = actors.values().copied().collect();
    if target_revisions != original.revisions.keys().copied().collect()
        || target_candidates != original.candidates.keys().copied().collect()
        || target_sources != original.sources.keys().copied().collect()
        || target_actors != original.actors
        || origin
            .placements
            .iter()
            .map(|m| &m.origin)
            .collect::<BTreeSet<_>>()
            != original.placements.keys().copied().collect()
    {
        return Err(invalid(
            "origin mappings omit or invent an original logical identity",
        ));
    }
    let map_membership = |ids: &[SourcePlacementId]| -> PResult<BTreeSet<SourcePlacementId>> {
        ids.iter()
            .map(|id| {
                placements
                    .get(id)
                    .map(|v| (*v).clone())
                    .ok_or_else(|| invalid("placement membership is outside its origin namespace"))
            })
            .collect()
    };
    if local.placements.values().any(|d| {
        original.revisions.contains_key(&d.created_revision)
            && !placements.contains_key(&d.placement_id)
    }) {
        return Err(invalid(
            "unmapped placement cannot be grafted into an original revision",
        ));
    }
    if map_membership(local.placement_zero)? != original.placement_zero.iter().cloned().collect() {
        return Err(invalid("origin changed initial placement history"));
    }
    for map in &origin.placements {
        let l = local
            .placements
            .get(&map.local)
            .ok_or_else(|| invalid("missing mapped local placement"))?;
        let r = original
            .placements
            .get(&map.origin)
            .ok_or_else(|| invalid("missing mapped original placement"))?;
        if l.created_revision != r.created_revision || !same(&l.spec, &r.spec)? {
            return Err(invalid("mapped placement changed immutable alignment"));
        }
    }
    for map in &origin.actors {
        if !local.actors.contains(map.local.as_str())
            || !original.actors.contains(map.origin.as_str())
        {
            return Err(invalid("actor map references a missing archival label"));
        }
    }
    for map in &origin.revisions {
        if map.local != map.origin {
            return Err(invalid("clone revision ordinals must remain unchanged"));
        }
        let l_members = local
            .placement_revisions
            .get(&map.local)
            .copied()
            .unwrap_or(&[]);
        let r_members = original
            .placement_revisions
            .get(&map.origin)
            .copied()
            .unwrap_or(&[]);
        if map_membership(l_members)? != r_members.iter().cloned().collect() {
            return Err(invalid(
                "mapped revision changed historical placement membership",
            ));
        }
        let l = local
            .revisions
            .get(&map.local)
            .ok_or_else(|| invalid("missing mapped local revision"))?;
        let r = original
            .revisions
            .get(&map.origin)
            .ok_or_else(|| invalid("missing mapped origin revision"))?;
        if l.kind != r.kind
            || l.label != r.label
            || !same(&l.motion, &r.motion)?
            || !same(&l.protected, &r.protected)?
            || actors.get(l.actor.as_str()).copied() != Some(r.actor.as_str())
        {
            return Err(invalid(
                "mapped revision changed immutable historical content",
            ));
        }
        match (
            local.lineage.get(&map.local),
            original.lineage.get(&map.origin),
        ) {
            (None, None) => {}
            (Some(l), Some(r)) => {
                let mapped_candidate = l
                    .candidate
                    .as_ref()
                    .map(|c| {
                        candidates.get(c.as_str()).copied().ok_or_else(|| {
                            invalid("revision candidate is outside its origin namespace")
                        })
                    })
                    .transpose()?;
                let mut mapped_review = l.review.clone();
                for d in &mut mapped_review.decisions {
                    d.actor = actors
                        .get(d.actor.as_str())
                        .copied()
                        .ok_or_else(|| {
                            invalid("review decision actor is outside its original namespace")
                        })?
                        .to_owned();
                }
                if l.parent_revision != r.parent_revision
                    || l.restored_from_revision != r.restored_from_revision
                    || l.restored_history_position != r.restored_history_position
                    || l.operation != r.operation
                    || mapped_candidate != r.candidate.as_ref().map(|c| c.as_str())
                    || !same(&l.contributions, &r.contributions)?
                    || !same(&mapped_review, &r.review)?
                {
                    return Err(invalid(
                        "mapped revision changed immutable ancestry or review state",
                    ));
                }
            }
            _ => {
                return Err(invalid(
                    "mapped revision invented or discarded a historical lineage row",
                ))
            }
        }
    }
    for map in &origin.candidates {
        let l = local
            .candidates
            .get(map.local.as_str())
            .ok_or_else(|| invalid("missing mapped local candidate"))?;
        let r = original
            .candidates
            .get(map.origin.as_str())
            .ok_or_else(|| invalid("missing mapped original candidate"))?;
        if l.base_revision < r.base_revision
            || l.job_origin.is_some()
            || !same(&l.motion, &r.motion)?
            || !same(&l.lineage, &r.lineage)?
            || !same(&l.review, &r.review)?
        {
            return Err(invalid(
                "candidate origin changed program, evidence, or review meaning",
            ));
        }
        match (&l.rebased_from, &r.rebased_from) {
            (None, Some(_)) => {
                return Err(invalid("candidate origin discarded its rebase ancestry"))
            }
            (Some(parent), original) => {
                let matching = origin.candidates.iter().any(|m| {
                    m.local == *parent
                        && (m.origin == r.candidate_id || original.as_ref() == Some(&m.origin))
                });
                if !matching {
                    return Err(invalid(
                        "candidate origin changed rebase parent correspondence",
                    ));
                }
            }
            _ => {}
        }
        // A rebase/commit changes local admission history, never original bytes.
        // The local manifest validator separately checks actual commit edges.
    }
    for map in &origin.sources {
        let l = local
            .sources
            .get(map.local.as_str())
            .ok_or_else(|| invalid("missing mapped local source"))?;
        let r = original
            .sources
            .get(map.origin.as_str())
            .ok_or_else(|| invalid("missing mapped original source"))?;
        if map.local != map.origin
            || !same(&l.kind, &r.kind)?
            || !same(&l.identity, &r.identity)?
            || r.stash_associations
                .iter()
                .any(|a| !l.stash_associations.contains(a))
        {
            return Err(invalid(
                "source origin changed content identity or source kind",
            ));
        }
    }
    Ok(())
}

/// Native authored proof and imported archival ancestry are disjoint. Choosing
/// one by precedence would allow hostile metadata to shadow source review bits.
fn validate_classification(manifest: &PortableProjectManifest) -> PResult<()> {
    let imported: BTreeSet<_> = manifest
        .imported_origins
        .iter()
        .flat_map(|origin| origin.candidates.iter().map(|map| map.local.as_str()))
        .collect();
    if manifest
        .authored_lineage
        .iter()
        .any(|row| imported.contains(row.candidate_id.as_str()))
        || manifest
            .candidates
            .iter()
            .any(|row| row.job_origin.is_some() && imported.contains(row.candidate_id.as_str()))
    {
        return Err(invalid(
            "candidate cannot be both native proof and imported archival ancestry",
        ));
    }
    Ok(())
}

fn check_composed<K: Ord + Clone>(
    first: impl Iterator<Item = (K, K)>,
    second: impl Iterator<Item = (K, K)>,
    direct: impl Iterator<Item = (K, K)>,
) -> PResult<()> {
    let second: BTreeMap<_, _> = second.collect();
    let direct: BTreeMap<_, _> = direct.collect();
    let composed: BTreeMap<_, _> = first
        .filter_map(|(local, middle)| {
            second
                .get(&middle)
                .cloned()
                .map(|original| (local, original))
        })
        .collect();
    // Partial-map equality includes absence on either leg. Equal payloads,
    // absent intermediate edges, and direct-only local keys cannot invent
    // ancestry. Many-to-one aliases are valid when both paths preserve them.
    if direct != composed {
        return Err(invalid(
            "direct origin identity contradicts its transitive mapping",
        ));
    }
    Ok(())
}

/// Imported namespaces remain explicit through re-export. Equality of payloads
/// cannot establish equality of historical identities: both paths through each
/// namespace triangle must name the same original row.
fn validate_composition(
    manifest: &PortableProjectManifest,
    originals: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    let direct: BTreeMap<_, _> = manifest
        .imported_origins
        .iter()
        .map(|origin| (origin.manifest.sha256.as_str(), origin))
        .collect();
    for first in &manifest.imported_origins {
        let middle = originals
            .get(&first.manifest.sha256)
            .ok_or_else(|| invalid("required original manifest is missing"))?;
        for second in &middle.imported_origins {
            let target = direct
                .get(second.manifest.sha256.as_str())
                .ok_or_else(|| invalid("re-export discarded a transitive origin namespace"))?;
            check_composed(
                first.revisions.iter().map(|map| (map.local, map.origin)),
                second.revisions.iter().map(|map| (map.local, map.origin)),
                target.revisions.iter().map(|map| (map.local, map.origin)),
            )?;
            check_composed(
                first
                    .candidates
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                second
                    .candidates
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                target
                    .candidates
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
            )?;
            check_composed(
                first
                    .sources
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                second
                    .sources
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                target
                    .sources
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
            )?;
            check_composed(
                first.placements.iter().map(|map| (&map.local, &map.origin)),
                second
                    .placements
                    .iter()
                    .map(|map| (&map.local, &map.origin)),
                target
                    .placements
                    .iter()
                    .map(|map| (&map.local, &map.origin)),
            )?;
            check_composed(
                first
                    .actors
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                second
                    .actors
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
                target
                    .actors
                    .iter()
                    .map(|map| (map.local.as_str(), map.origin.as_str())),
            )?;
        }
    }
    Ok(())
}

/// Validate resolved canonical origin metadata, not complete container bytes.
/// The supplied map must contain exactly the reachable origin manifests.
pub(crate) fn validate_import_origins(
    manifest: &PortableProjectManifest,
    container: &PackageArtifactDescriptor,
    origins: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    let root_bytes = manifest.canonical_bytes()?;
    bind_container(manifest, container, &root_bytes)?;
    validate_retained_origin_graph(manifest, origins)
}

/// Check a retained clone projection without inventing a flat container identity.
pub(crate) fn validate_retained_origin_graph(
    manifest: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    if origins.len() > MAX_PACKAGE_IMPORTED_ORIGINS {
        return Err(exhausted("too many distinct origin manifests"));
    }
    require_portable_cpu_manifest(manifest)?;
    let root_bytes = manifest.canonical_bytes()?;
    let root_sha = digest(&root_bytes);
    let mut entries = 0usize;
    checked_add(&mut entries, logical_entries(manifest)?)?;
    checked_add(&mut entries, map_entries(manifest)?)?;
    let mut byte_count = 0u64;
    let mut canonical = BTreeMap::new();
    // Bound the full decoded aggregate before any edge-by-edge semantic work.
    for (sha, original) in origins {
        require_portable_cpu_manifest(original)?;
        checked_add(&mut entries, logical_entries(original)?)?;
        checked_add(&mut entries, map_entries(original)?)?;
        let bytes = original.canonical_bytes()?;
        byte_count = byte_count
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| exhausted("origin manifest byte count overflow"))?;
        if byte_count > MAX_PACKAGE_ORIGIN_MANIFEST_BYTES {
            return Err(exhausted(
                "origin manifests exceed the aggregate byte limit",
            ));
        }
        if sha != &digest(&bytes) || sha == &root_sha {
            return Err(invalid(
                "origin namespace does not identify its canonical original bytes",
            ));
        }
        canonical.insert(sha.as_str(), bytes.len() as u64);
    }
    crate::portable_cpu_archives::check_graph(manifest, origins)?;
    fn visit<'a>(
        sha: &'a str,
        node: &'a PortableProjectManifest,
        origins: &'a BTreeMap<String, PortableProjectManifest>,
        lengths: &BTreeMap<&str, u64>,
        active: &mut BTreeSet<&'a str>,
        depths: &mut BTreeMap<&'a str, usize>,
    ) -> PResult<usize> {
        if let Some(depth) = depths.get(sha) {
            return Ok(*depth);
        }
        if !active.insert(sha) {
            return Err(invalid("origin graph contains a cycle"));
        }
        if active.len() > MAX_PACKAGE_ORIGIN_DEPTH + 1 {
            return Err(exhausted("origin graph exceeds the depth limit"));
        }
        validate_classification(node)?;
        validate_composition(node, origins)?;
        let local_index = Index::new(node);
        let mut depth = 0usize;
        for edge in &node.imported_origins {
            let original = origins
                .get(&edge.manifest.sha256)
                .ok_or_else(|| invalid("required original manifest is missing"))?;
            let byte_len = lengths
                .get(edge.manifest.sha256.as_str())
                .ok_or_else(|| invalid("required original canonical bytes are missing"))?;
            if *byte_len != edge.manifest.byte_len || original.objects != edge.objects {
                return Err(invalid(
                    "original manifest length or object catalog differs from its namespace",
                ));
            }
            // canonical bytes were hashed above; the edge descriptor must bind
            // the same manifest and project, independently of flat object roles.
            edge.container.validate()?;
            if edge.container.format_version != original.format_version
                || edge.container.manifest_sha256 != edge.manifest.sha256
                || edge.container.project_id != original.origin_project_id
                || edge.container.captured_revision != original.captured_revision
                || edge.container.captured_event_cursor != original.captured_event_cursor
                || u64::from(edge.container.object_count) != original.objects.len() as u64
            {
                return Err(invalid(
                    "ancestor container metadata does not bind its original manifest",
                ));
            }
            validate_mapping(&local_index, &Index::new(original), edge)?;
            let child_depth = visit(
                &edge.manifest.sha256,
                original,
                origins,
                lengths,
                active,
                depths,
            )?;
            depth = depth.max(
                child_depth
                    .checked_add(1)
                    .ok_or_else(|| exhausted("origin graph depth overflow"))?,
            );
            if depth > MAX_PACKAGE_ORIGIN_DEPTH {
                return Err(exhausted("origin graph exceeds the depth limit"));
            }
        }
        active.remove(sha);
        depths.insert(sha, depth);
        Ok(depth)
    }
    let mut visited = BTreeMap::new();
    visit(
        &root_sha,
        manifest,
        origins,
        &canonical,
        &mut BTreeSet::new(),
        &mut visited,
    )?;
    if visited.len() != origins.len() + 1 {
        return Err(invalid(
            "resolved original manifests include unrelated graph nodes",
        ));
    }
    Ok(())
}

/// Native scopes come only from checked original worker evidence, indexed by
/// exact canonical manifest identity. None is proven whole-project scope.
pub(crate) type NativeCandidateIntervals = BTreeMap<(String, CandidateId), Option<TimeRange>>;

/// Resolve restrictions independently of job/execution authority. Graph
/// validation bounds recursion and binds every alias to byte-exact originals.
pub(crate) fn resolve_candidate_intervals(
    manifest: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
    native: &NativeCandidateIntervals,
) -> PResult<BTreeMap<CandidateId, Option<TimeRange>>> {
    validate_retained_origin_graph(manifest, origins)?;
    fn visit(
        namespace: &str,
        manifest: &PortableProjectManifest,
        candidate: &CandidateId,
        origins: &BTreeMap<String, PortableProjectManifest>,
        native: &NativeCandidateIntervals,
        memo: &mut NativeCandidateIntervals,
    ) -> PResult<Option<TimeRange>> {
        let key = (namespace.to_owned(), candidate.clone());
        if let Some(value) = memo.get(&key) {
            return Ok(*value);
        }
        let row = manifest
            .candidates
            .iter()
            .find(|c| &c.candidate_id == candidate)
            .ok_or_else(|| invalid("candidate scope references missing candidate"))?;
        let mut inherited: Option<Option<TimeRange>> = None;
        for edge in &manifest.imported_origins {
            if let Some(mapping) = edge.candidates.iter().find(|m| &m.local == candidate) {
                let original = origins
                    .get(&edge.manifest.sha256)
                    .ok_or_else(|| invalid("candidate scope origin missing"))?;
                let scope = visit(
                    &edge.manifest.sha256,
                    original,
                    &mapping.origin,
                    origins,
                    native,
                    memo,
                )?;
                if inherited.is_some_and(|previous| previous != scope) {
                    return Err(invalid("candidate ancestry disagrees on interval scope"));
                }
                inherited = Some(scope);
            }
        }
        let scope = if row.job_origin.is_some() {
            if inherited.is_some() {
                return Err(invalid("native scope cannot shadow imported ancestry"));
            }
            *native
                .get(&key)
                .ok_or_else(|| invalid("native candidate scope evidence missing"))?
        } else {
            inherited.unwrap_or(None)
        };
        // Older formats omitted restrictions; only their verified ancestry can
        // recover them. New declarations must neither widen nor narrow scope.
        if (manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CANDIDATE_INTERVALS
            || row.project_interval.is_some())
            && row.project_interval != scope
        {
            return Err(invalid("candidate interval differs from original evidence"));
        }
        memo.insert(key, scope);
        Ok(scope)
    }
    let sha = digest(&manifest.canonical_bytes()?);
    let mut memo = BTreeMap::new();
    manifest
        .candidates
        .iter()
        .map(|c| {
            Ok((
                c.candidate_id.clone(),
                visit(&sha, manifest, &c.candidate_id, origins, native, &mut memo)?,
            ))
        })
        .collect()
}

fn archival_actor(namespace: &str, actor: &str) -> String {
    format!("archive:{namespace}:{}", digest(actor.as_bytes()))
}

fn candidate_review(
    namespace: &str,
    manifest: &PortableProjectManifest,
    candidate: &CandidateId,
    origins: &BTreeMap<String, PortableProjectManifest>,
    memo: &mut BTreeMap<(String, CandidateId), PackageReviewState>,
) -> PResult<PackageReviewState> {
    let key = (namespace.to_owned(), candidate.clone());
    if let Some(review) = memo.get(&key) {
        return Ok(review.clone());
    }
    let index = manifest
        .candidates
        .binary_search_by(|c| c.candidate_id.cmp(candidate))
        .map_err(|_| invalid("candidate review references a missing candidate"))?;
    let review = if let Ok(index) = manifest
        .authored_lineage
        .binary_search_by(|c| c.candidate_id.cmp(candidate))
    {
        manifest.authored_lineage[index].review.clone()
    } else {
        let mut original_state = None;
        for origin in &manifest.imported_origins {
            if let Ok(index) = origin
                .candidates
                .binary_search_by(|m| m.local.cmp(candidate))
            {
                let original = origins
                    .get(&origin.manifest.sha256)
                    .ok_or_else(|| invalid("candidate review origin is missing"))?;
                let state = candidate_review(
                    &origin.manifest.sha256,
                    original,
                    &origin.candidates[index].origin,
                    origins,
                    memo,
                )?;
                if let Some(previous) = &original_state {
                    if !same(previous, &state)? {
                        return Err(invalid(
                            "candidate namespaces disagree on the original review state",
                        ));
                    }
                } else {
                    original_state = Some(state);
                }
            }
        }
        original_state.unwrap_or_else(|| PackageReviewState {
            decisions: vec![],
            flags: manifest.candidates[index].review.clone(),
            unknown: false,
            conservative: false,
        })
    };
    memo.insert(key, review.clone());
    Ok(review)
}

/// Translate a verified inert snapshot using engine-provided fresh identities.
/// No import edit/revision is fabricated. Current authority and event cursors
/// must be created separately by the engine publication transaction.
pub(crate) fn plan_clone_import(
    manifest: &PortableProjectManifest,
    container: &PackageArtifactDescriptor,
    origins: &BTreeMap<String, PortableProjectManifest>,
    ids: &CloneIdentityAssignment,
) -> PResult<CloneImportPlan> {
    validate_import_origins(manifest, container, origins)?;
    if ids.project_id == manifest.origin_project_id
        || origins
            .values()
            .any(|m| m.origin_project_id == ids.project_id)
    {
        return Err(invalid("clone requires a fresh project identity"));
    }
    let incoming: BTreeSet<_> = manifest
        .candidates
        .iter()
        .map(|c| c.candidate_id.clone())
        .collect();
    let assigned: BTreeSet<_> = ids.candidates.keys().cloned().collect();
    let targets: BTreeSet<_> = ids.candidates.values().cloned().collect();
    if incoming != assigned || targets.len() != assigned.len() || !targets.is_disjoint(&incoming) {
        return Err(invalid(
            "clone requires a complete bijective fresh candidate assignment",
        ));
    }
    let ancestor_candidates: BTreeSet<_> = origins
        .values()
        .flat_map(|m| m.candidates.iter().map(|c| &c.candidate_id))
        .collect();
    if targets.iter().any(|id| ancestor_candidates.contains(id)) {
        return Err(invalid(
            "clone candidate identity collides with archived ancestry",
        ));
    }
    let incoming_placements: BTreeSet<_> = manifest
        .placements
        .iter()
        .flat_map(|p| p.definitions.iter().map(|d| d.placement_id.clone()))
        .collect();
    let assigned_placements: BTreeSet<_> = ids.placements.keys().cloned().collect();
    let placement_targets: BTreeSet<_> = ids.placements.values().cloned().collect();
    let ancestor_placements: BTreeSet<_> = origins
        .values()
        .flat_map(|m| {
            m.placements
                .iter()
                .flat_map(|p| p.definitions.iter().map(|d| &d.placement_id))
        })
        .collect();
    if incoming_placements != assigned_placements
        || placement_targets.len() != assigned_placements.len()
        || !placement_targets.is_disjoint(&incoming_placements)
        || placement_targets
            .iter()
            .any(|id| ancestor_placements.contains(id))
    {
        return Err(invalid(
            "clone requires complete bijective fresh placement assignment",
        ));
    }
    let translate_placement = |id: &SourcePlacementId| -> PResult<SourcePlacementId> {
        ids.placements
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("placement assignment is incomplete"))
    };
    let origin_manifest = manifest.canonical_bytes()?;
    let origin_manifest_ref = PackageObjectRef {
        sha256: digest(&origin_manifest),
        byte_len: origin_manifest.len() as u64,
    };
    let actors: BTreeMap<_, _> = manifest
        .revisions
        .iter()
        .map(|r| {
            (
                r.actor.clone(),
                archival_actor(&origin_manifest_ref.sha256, &r.actor),
            )
        })
        .collect();
    let translate_candidate = |id: &CandidateId| -> PResult<CandidateId> {
        ids.candidates
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("candidate assignment is incomplete"))
    };
    let mut result = manifest.clone();
    result.retiming_receipts.clear();
    result.format_version = if result.placements.is_some() {
        PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
    } else {
        PROJECT_PACKAGE_FORMAT_VERSION_WITH_ORIGINS
    };
    result.origin_project_id = ids.project_id.clone();
    if let Some(closure) = &mut result.placements {
        for definition in &mut closure.definitions {
            definition.placement_id = translate_placement(&definition.placement_id)?;
        }
        closure
            .definitions
            .sort_by(|a, b| a.placement_id.cmp(&b.placement_id));
        let translate_members = |members: &mut Vec<SourcePlacementId>| -> PResult<()> {
            *members = members
                .iter()
                .map(translate_placement)
                .collect::<PResult<_>>()?;
            members.sort();
            Ok(())
        };
        translate_members(&mut closure.active)?;
        for row in &mut closure.revisions {
            translate_members(&mut row.placements)?;
        }
        for row in &mut closure.history {
            translate_members(&mut row.placements)?;
        }
    }
    for revision in &mut result.revisions {
        revision.actor = actors
            .get(&revision.actor)
            .cloned()
            .ok_or_else(|| invalid("actor assignment is incomplete"))?;
    }
    for row in &mut result.revision_lineage {
        for d in &mut row.review.decisions {
            d.actor = actors
                .get(d.actor.as_str())
                .cloned()
                .ok_or_else(|| invalid("review actor is absent from the origin projection"))?;
        }
        row.candidate = row
            .candidate
            .as_ref()
            .map(translate_candidate)
            .transpose()?;
    }
    let mut candidate_reviews = BTreeMap::new();
    let mut memo = BTreeMap::new();
    for candidate in &mut result.candidates {
        let fresh = translate_candidate(&candidate.candidate_id)?;
        let review = candidate_review(
            &origin_manifest_ref.sha256,
            manifest,
            &candidate.candidate_id,
            origins,
            &mut memo,
        )?;
        candidate_reviews.insert(fresh.clone(), review);
        candidate.candidate_id = fresh;
        candidate.rebased_from = candidate
            .rebased_from
            .as_ref()
            .map(translate_candidate)
            .transpose()?;
        candidate.job_origin = None;
    }
    result
        .candidates
        .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
    result.authored_lineage.clear();
    result.generated_origins.clear();
    for cell in &mut result.legacy_cells {
        cell.row_key = match cell.owner {
            PackageLegacyOwner::Projects => ids.project_id.to_string(),
            PackageLegacyOwner::Candidates => {
                let old = CandidateId::new(&cell.row_key)
                    .map_err(|_| invalid("invalid original legacy candidate key"))?;
                translate_candidate(&old)?.to_string()
            }
            PackageLegacyOwner::Revisions | PackageLegacyOwner::EditStates => {
                let (owner, suffix) = cell
                    .row_key
                    .rsplit_once(':')
                    .ok_or_else(|| invalid("invalid original legacy history key"))?;
                if owner != manifest.origin_project_id.as_str() {
                    return Err(invalid(
                        "legacy history key is outside the original project",
                    ));
                }
                format!("{}:{suffix}", ids.project_id)
            }
        };
    }
    result
        .legacy_cells
        .sort_by(|a, b| (a.owner, &a.row_key).cmp(&(b.owner, &b.row_key)));
    for origin in &mut result.imported_origins {
        for map in &mut origin.candidates {
            map.local = translate_candidate(&map.local)?;
        }
        origin.candidates.sort_by(|a, b| a.local.cmp(&b.local));
        for map in &mut origin.placements {
            map.local = translate_placement(&map.local)?;
        }
        origin.placements.sort_by(|a, b| a.local.cmp(&b.local));
        for map in &mut origin.actors {
            map.local = actors
                .get(&map.local)
                .cloned()
                .ok_or_else(|| invalid("archival actor map is outside incoming projection"))?;
        }
        origin.actors.sort_by(|a, b| a.local.cmp(&b.local));
    }
    let mut immediate = PackageArchiveOrigin {
        placements: manifest
            .placements
            .iter()
            .flat_map(|p| p.definitions.iter())
            .map(|d| {
                Ok(PackagePlacementOrigin {
                    local: translate_placement(&d.placement_id)?,
                    origin: d.placement_id.clone(),
                })
            })
            .collect::<PResult<_>>()?,
        manifest: origin_manifest_ref.clone(),
        container: container.clone(),
        objects: manifest.objects.clone(),
        revisions: manifest
            .revisions
            .iter()
            .map(|r| PackageRevisionOrigin {
                local: r.revision,
                origin: r.revision,
            })
            .collect(),
        candidates: manifest
            .candidates
            .iter()
            .map(|c| {
                Ok(PackageCandidateOrigin {
                    local: translate_candidate(&c.candidate_id)?,
                    origin: c.candidate_id.clone(),
                })
            })
            .collect::<PResult<_>>()?,
        sources: manifest
            .sources
            .iter()
            .map(|s| PackageSourceOrigin {
                local: s.source_version.clone(),
                origin: s.source_version.clone(),
            })
            .collect(),
        actors: actors
            .iter()
            .map(|(original, local)| PackageActorOrigin {
                local: local.clone(),
                origin: original.clone(),
            })
            .collect(),
    };
    immediate.candidates.sort_by(|a, b| a.local.cmp(&b.local));
    immediate.placements.sort_by(|a, b| a.local.cmp(&b.local));
    immediate.actors.sort_by(|a, b| a.local.cmp(&b.local));
    result.imported_origins.push(immediate);
    result
        .imported_origins
        .sort_by(|a, b| a.manifest.sha256.cmp(&b.manifest.sha256));
    if let Some(object) = result
        .objects
        .iter_mut()
        .find(|o| o.sha256 == origin_manifest_ref.sha256)
    {
        if object.byte_len != origin_manifest_ref.byte_len {
            return Err(invalid(
                "canonical manifest digest collides with an unequal object length",
            ));
        }
        object.roles.push(PackageObjectRole::ImportedManifest);
        object.roles.sort();
        object.roles.dedup();
    } else {
        result.objects.push(PackageObjectDescriptor {
            sha256: origin_manifest_ref.sha256.clone(),
            byte_len: origin_manifest_ref.byte_len,
            roles: vec![PackageObjectRole::ImportedManifest],
        });
        result.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    }
    result.format_version = result.required_format_version();
    result.counts = result.record_counts()?;
    result.validate()?;
    // Validate the newly composed graph too, not just the imported source.
    let mut composed = origins.clone();
    composed.insert(origin_manifest_ref.sha256.clone(), manifest.clone());
    let bytes = result.canonical_bytes()?;
    let synthetic_binding = PackageArtifactDescriptor {
        format_version: result.format_version,
        sha256: container.sha256.clone(),
        byte_len: container.byte_len,
        manifest_sha256: digest(&bytes),
        project_id: result.origin_project_id.clone(),
        captured_revision: result.captured_revision,
        captured_event_cursor: result.captured_event_cursor,
        object_count: result.objects.len() as u32,
        verification: container.verification,
    };
    // This internal binding is metadata-only; no new container identity or
    // verification receipt is returned or inferred from it.
    validate_import_origins(&result, &synthetic_binding, &composed)?;
    Ok(CloneImportPlan {
        manifest: result,
        origin_manifest,
        origin_manifest_ref,
        identities: ids.clone(),
        candidate_reviews,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PortableProjectManifest {
        let motion = ProgramDescriptor {
            artifact_id: ArtifactId::new("motion.empty").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 13,
            codec: MotionCodec::MotionProgramJsonV1,
            axes: vec![],
        };
        let mut manifest = PortableProjectManifest {
            placements: None,
            retiming_receipts: vec![],
            format_version: 1,
            origin_project_id: ProjectId::new("project:old").unwrap(),
            captured_revision: RevisionId::new(0),
            captured_event_cursor: 9,
            completeness: PackageCompleteness::default(),
            counts: PackageRecordCounts::default(),
            head: PackageProjectHead {
                name: "Original".into(),
                revision: RevisionId::new(0),
                history_cursor: 0,
                motion: motion.clone(),
                protected: vec![],
            },
            revisions: vec![PackageRevision {
                revision: RevisionId::new(0),
                actor: "original:actor".into(),
                kind: "create".into(),
                label: "".into(),
                motion: motion.clone(),
                protected: vec![],
            }],
            edit_states: vec![PackageEditState {
                position: 0,
                motion: motion.clone(),
                protected: vec![],
                source_revision: None,
            }],
            revision_lineage: vec![],
            candidates: vec![PackageCandidate {
                rebased_from: None,
                project_interval: None,
                candidate_id: CandidateId::new("candidate:old").unwrap(),
                base_revision: RevisionId::new(0),
                motion,
                job_origin: None,
                lineage: vec![],
                committed_revision: None,
                review: vec![],
            }],
            authored_lineage: vec![],
            sources: vec![],
            legacy_cells: vec![],
            generated_origins: vec![],
            export_receipts: vec![],
            imported_origins: vec![],
            objects: vec![PackageObjectDescriptor {
                sha256: "a".repeat(64),
                byte_len: 13,
                roles: vec![PackageObjectRole::MotionProgram],
            }],
        };
        manifest.counts = manifest.record_counts().unwrap();
        manifest
    }
    #[test]
    fn reserved_cpu_image_dependencies_refuse_while_legacy_identities_remain_portable() {
        let identity = |name: &str| ArtifactIdentity {
            artifact_id: ArtifactId::new(name).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        require_portable_cpu_dependencies(&[]).unwrap();
        require_portable_cpu_dependencies(&[
            identity("legacy-runtime"),
            identity("cpu-runtime-image"),
            identity("legacy-cpu-runtime-image-hint"),
        ])
        .unwrap();
        for name in [
            format!("{CPU_RUNTIME_IMAGE_ID_PREFIX}{}", "a".repeat(64)),
            format!("{CPU_RUNTIME_IMAGE_ID_PREFIX}malformed"),
        ] {
            let error = require_portable_cpu_dependencies(&[identity(&name)]).unwrap_err();
            assert_eq!(error.code, ErrorCode::Unsupported);
        }
    }

    #[test]
    fn fallback_selection_tags_refuse_in_dependencies_and_root_or_ancestor_lineage() {
        let identity = |name: &str| ArtifactIdentity {
            artifact_id: ArtifactId::new(name).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        require_portable_cpu_dependencies(&[
            identity("inference-fallback-selection"),
            identity("legacy-inference-fallback-selection-note"),
        ])
        .unwrap();
        for name in [
            format!("{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}", "a".repeat(64)),
            format!("{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}invalid"),
        ] {
            assert_eq!(
                require_portable_cpu_dependencies(&[identity(&name)])
                    .unwrap_err()
                    .code,
                ErrorCode::Unsupported
            );
        }
        let original = fixture();
        let mut tagged = original.clone();
        tagged.candidates[0].lineage.push(identity(&format!(
            "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
            "a".repeat(64)
        )));
        assert_eq!(
            validate_retained_origin_graph(&tagged, &BTreeMap::new())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let mut ancestors = BTreeMap::new();
        ancestors.insert("b".repeat(64), tagged);
        assert_eq!(
            validate_retained_origin_graph(&original, &ancestors)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn candidate_cpu_mode_cannot_be_flattened_out_of_root_or_ancestor_history() {
        let original = fixture();
        require_portable_cpu_manifest(&original).unwrap();
        let mut tagged = original.clone();
        tagged.candidates[0].lineage.push(ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "{CPU_RUNTIME_IMAGE_ID_PREFIX}{}",
                "a".repeat(64)
            ))
            .unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        });
        assert_eq!(
            require_portable_cpu_manifest(&tagged).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            validate_retained_origin_graph(&tagged, &BTreeMap::new())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let mut ancestors = BTreeMap::new();
        ancestors.insert("b".repeat(64), tagged);
        assert_eq!(
            validate_retained_origin_graph(&original, &ancestors)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn scope_declarations_cannot_invent_interval_authority() {
        let mut original = fixture();
        original.candidates[0].project_interval = Some(
            TimeRange::new(
                pulsar_core::ProjectTime::from_nanos(1),
                pulsar_core::ProjectTime::from_nanos(2),
            )
            .unwrap(),
        );
        original.format_version = original.required_format_version();
        original.validate().unwrap();
        assert!(resolve_candidate_intervals(
            &original,
            &BTreeMap::new(),
            &NativeCandidateIntervals::new()
        )
        .is_err());
    }
    #[test]
    fn whole_scope_remains_whole_through_successive_inert_clones() {
        let original = fixture();
        let first = plan_clone_import(
            &original,
            &descriptor(&original),
            &BTreeMap::new(),
            &ids(&original, "scope-one"),
        )
        .unwrap();
        let mut ancestors = BTreeMap::new();
        ancestors.insert(first.origin_manifest_ref.sha256.clone(), original);
        let second = plan_clone_import(
            &first.manifest,
            &descriptor(&first.manifest),
            &ancestors,
            &ids(&first.manifest, "scope-two"),
        )
        .unwrap();
        ancestors.insert(second.origin_manifest_ref.sha256.clone(), first.manifest);
        let scope = resolve_candidate_intervals(
            &second.manifest,
            &ancestors,
            &NativeCandidateIntervals::new(),
        )
        .unwrap();
        assert!(scope.values().all(Option::is_none));
    }
    fn descriptor(m: &PortableProjectManifest) -> PackageArtifactDescriptor {
        PackageArtifactDescriptor {
            format_version: m.format_version,
            sha256: "b".repeat(64),
            byte_len: 100_000,
            manifest_sha256: digest(&m.canonical_bytes().unwrap()),
            project_id: m.origin_project_id.clone(),
            captured_revision: m.captured_revision,
            captured_event_cursor: m.captured_event_cursor,
            object_count: m.objects.len() as u32,
            verification: PackageReadyVerification::AllDeclaredObjectsVerified,
        }
    }
    fn ids(m: &PortableProjectManifest, suffix: &str) -> CloneIdentityAssignment {
        CloneIdentityAssignment {
            project_id: ProjectId::new(format!("project.{suffix}")).unwrap(),
            placements: m
                .placements
                .iter()
                .flat_map(|p| p.definitions.iter())
                .enumerate()
                .map(|(i, d)| {
                    (
                        d.placement_id.clone(),
                        SourcePlacementId::new(format!("placement.{suffix}.{i}")).unwrap(),
                    )
                })
                .collect(),
            candidates: m
                .candidates
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    (
                        c.candidate_id.clone(),
                        CandidateId::new(format!("candidate.{suffix}.{i}")).unwrap(),
                    )
                })
                .collect(),
        }
    }
    fn clone_first() -> (PortableProjectManifest, CloneImportPlan) {
        let original = fixture();
        let plan = plan_clone_import(
            &original,
            &descriptor(&original),
            &BTreeMap::new(),
            &ids(&original, "one"),
        )
        .unwrap();
        (original, plan)
    }

    #[test]
    fn placement_closure_remaps_identity_preserves_history_and_rejects_forged_origins() {
        let mut original = fixture();
        let source = SourceVersionId::new("c".repeat(64)).unwrap();
        original.sources.push(PackageSource {
            source_version: source.clone(),
            kind: SourceKind::Media,
            label: "synthetic".into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("c".repeat(64)).unwrap(),
                sha256: "c".repeat(64),
                byte_len: 3,
            },
            pinned: true,
            evicted: false,
            stash_associations: vec![],
        });
        original.objects.push(PackageObjectDescriptor {
            sha256: "c".repeat(64),
            byte_len: 3,
            roles: vec![PackageObjectRole::SourceSnapshot],
        });
        let id = SourcePlacementId::new("placement.original").unwrap();
        original.placements = Some(PackagePlacementClosure {
            definitions: vec![SourcePlacement {
                placement_id: id.clone(),
                created_revision: RevisionId::new(0),
                spec: SourcePlacementSpec {
                    source_version: source,
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(2, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(1, 2).unwrap(),
                    project_anchor: ProjectTime::from_nanos(-3),
                    rate_numerator: -2,
                    rate_denominator: 13,
                },
            }],
            active: vec![id.clone()],
            revisions: vec![PackageRevisionPlacements {
                revision: RevisionId::new(0),
                placements: vec![id.clone()],
            }],
            history: vec![PackageHistoryPlacements {
                position: 0,
                placements: vec![id.clone()],
            }],
        });
        original.format_version = PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS;
        let binding = rebind(&mut original);
        let first = plan_clone_import(
            &original,
            &binding,
            &BTreeMap::new(),
            &ids(&original, "placement-one"),
        )
        .unwrap();
        let local = first.manifest.placements.as_ref().unwrap();
        assert_ne!(local.active[0], id);
        assert!(same(
            &local.definitions[0].spec,
            &original.placements.as_ref().unwrap().definitions[0].spec
        )
        .unwrap());
        let mut ancestors =
            BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original.clone())]);
        let second = plan_clone_import(
            &first.manifest,
            &descriptor(&first.manifest),
            &ancestors,
            &ids(&first.manifest, "placement-two"),
        )
        .unwrap();
        ancestors.insert(
            second.origin_manifest_ref.sha256.clone(),
            first.manifest.clone(),
        );
        let mut forged = second.manifest.clone();
        forged.placements.as_mut().unwrap().definitions[0]
            .spec
            .project_anchor = ProjectTime::from_nanos(999);
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "matching IDs cannot authorize changed alignment"
        );
        let mut forged = second.manifest.clone();
        forged
            .imported_origins
            .iter_mut()
            .find(|o| o.manifest.sha256 == first.origin_manifest_ref.sha256)
            .unwrap()
            .placements
            .clear();
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "placement ancestry cannot be omitted"
        );
        let mut incomplete = ids(&original, "incomplete");
        incomplete.placements.clear();
        assert!(plan_clone_import(
            &original,
            &descriptor(&original),
            &BTreeMap::new(),
            &incomplete
        )
        .is_err());
        let mut graft = second.manifest.clone();
        let mut extra = graft.placements.as_ref().unwrap().definitions[0].clone();
        extra.placement_id = SourcePlacementId::new("placement.grafted").unwrap();
        graft.placements.as_mut().unwrap().definitions.push(extra);
        graft
            .placements
            .as_mut()
            .unwrap()
            .definitions
            .sort_by(|a, b| a.placement_id.cmp(&b.placement_id));
        let binding = rebind(&mut graft);
        assert!(
            validate_import_origins(&graft, &binding, &ancestors).is_err(),
            "unused definitions cannot be assigned an original creation revision without ancestry"
        );
        let mut bad = original.clone();
        bad.format_version = 2;
        assert!(bad.validate().is_err());
        let mut bad = original.clone();
        bad.placements.as_mut().unwrap().active.clear();
        bad.counts = bad.record_counts().unwrap();
        assert!(bad.validate().is_err());
        let mut bad = original.clone();
        bad.sources[0].kind = SourceKind::GenerationInput;
        assert!(bad.validate().is_err());
        let mut bad = original;
        bad.placements.as_mut().unwrap().history[0].position = u64::MAX;
        assert!(bad.validate().is_err());
    }
    fn rebind(m: &mut PortableProjectManifest) -> PackageArtifactDescriptor {
        m.counts = m.record_counts().unwrap();
        descriptor(m)
    }

    #[test]
    fn clone_keeps_exact_inert_bytes_without_fabricated_revision_or_jobs() {
        let (original, plan) = clone_first();
        assert_eq!(plan.origin_manifest, original.canonical_bytes().unwrap());
        assert_eq!(plan.manifest.head.revision, original.head.revision);
        assert_eq!(plan.manifest.edit_states[0].source_revision, None);
        assert!(plan.manifest.revision_lineage.is_empty());
        assert!(plan.manifest.generated_origins.is_empty());
        assert!(plan.manifest.authored_lineage.is_empty());
        assert!(plan.manifest.candidates[0].job_origin.is_none());
        assert_ne!(
            plan.manifest.candidates[0].candidate_id,
            original.candidates[0].candidate_id
        );
        assert!(plan.manifest.revisions[0].actor.starts_with("archive:"));
        assert_eq!(plan.manifest.revisions[0].actor.len(), 137);
        assert_eq!(
            plan.manifest.imported_origins[0].actors[0].origin,
            "original:actor"
        );
    }

    #[test]
    fn fresh_assignment_must_be_complete_bijective_and_noncolliding() {
        let m = fixture();
        let mut assignment = ids(&m, "one");
        assignment.candidates.clear();
        assert!(plan_clone_import(&m, &descriptor(&m), &BTreeMap::new(), &assignment).is_err());
        assignment = ids(&m, "one");
        assignment.project_id = m.origin_project_id.clone();
        assert!(plan_clone_import(&m, &descriptor(&m), &BTreeMap::new(), &assignment).is_err());
        assignment = ids(&m, "one");
        assignment.candidates.insert(
            m.candidates[0].candidate_id.clone(),
            m.candidates[0].candidate_id.clone(),
        );
        assert!(plan_clone_import(&m, &descriptor(&m), &BTreeMap::new(), &assignment).is_err());
        let mut two = m.clone();
        let mut second = two.candidates[0].clone();
        second.candidate_id = CandidateId::new("candidate:z").unwrap();
        two.candidates.push(second);
        let d = rebind(&mut two);
        let mut assignment = ids(&two, "two");
        let first = assignment.candidates.values().next().unwrap().clone();
        for target in assignment.candidates.values_mut() {
            *target = first.clone();
        }
        assert!(plan_clone_import(&two, &d, &BTreeMap::new(), &assignment).is_err());
    }

    #[test]
    fn exact_history_legacy_keys_and_review_state_are_preserved() {
        let mut m = fixture();
        m.legacy_cells = vec![
            PackageLegacyCell {
                owner: PackageLegacyOwner::Projects,
                row_key: m.origin_project_id.to_string(),
                object: PackageObjectRef {
                    sha256: "a".repeat(64),
                    byte_len: 13,
                },
            },
            PackageLegacyCell {
                owner: PackageLegacyOwner::EditStates,
                row_key: format!("{}:99", m.origin_project_id),
                object: PackageObjectRef {
                    sha256: "a".repeat(64),
                    byte_len: 13,
                },
            },
            PackageLegacyCell {
                owner: PackageLegacyOwner::Candidates,
                row_key: m.candidates[0].candidate_id.to_string(),
                object: PackageObjectRef {
                    sha256: "a".repeat(64),
                    byte_len: 13,
                },
            },
        ];
        m.objects[0]
            .roles
            .push(PackageObjectRole::LegacyMotionBytes);
        let d = rebind(&mut m);
        let p = plan_clone_import(&m, &d, &BTreeMap::new(), &ids(&m, "legacy")).unwrap();
        assert_eq!(p.manifest.legacy_cells[0].row_key, "project.legacy");
        assert_eq!(p.manifest.legacy_cells[1].row_key, "project.legacy:99");
        assert_eq!(p.manifest.legacy_cells[2].row_key, "candidate.legacy.0");
        assert_eq!(p.manifest.legacy_cells[1].object, m.legacy_cells[1].object);
    }
    #[test]
    fn archival_review_decisions_remap_actors_and_reject_rewritten_rationale_or_attribution() {
        let mut original = fixture();
        let flag = ReviewFlag {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10)).unwrap(),
            reason: ReviewReason::new("fixture_review").unwrap(),
            evidence: EvidenceKind::Inferred,
            confidence: None,
        };
        let review = PackageReviewState {
            flags: vec![flag.clone()],
            decisions: vec![],
            unknown: false,
            conservative: false,
        };
        original.revision_lineage.push(PackageRevisionLineage {
            revision: RevisionId::new(0),
            parent_revision: None,
            restored_from_revision: None,
            restored_history_position: None,
            candidate: None,
            operation: "create".into(),
            contributions: vec![],
            review: review.clone(),
        });
        let mut revision = original.revisions[0].clone();
        revision.revision = RevisionId::new(1);
        revision.kind = "review_disposition".into();
        original.revisions.push(revision.clone());
        original.head.revision = RevisionId::new(1);
        original.captured_revision = RevisionId::new(1);
        let mut decided = review;
        decided.decisions.push(ReviewDecision {
            range: None,
            review_id: review_identity(&flag).unwrap(),
            actor: revision.actor,
            request_id: RequestId::new("fixture-review-request").unwrap(),
            base_revision: RevisionId::new(0),
            recorded_revision: RevisionId::new(1),
            disposition: ReviewDisposition::Resolved,
            note: "Synthetic declaration only".into(),
            recorded_at_unix_ms: 1,
        });
        original.revision_lineage.push(PackageRevisionLineage {
            revision: RevisionId::new(1),
            parent_revision: Some(RevisionId::new(0)),
            restored_from_revision: None,
            restored_history_position: None,
            candidate: None,
            operation: "review_disposition".into(),
            contributions: vec![],
            review: decided,
        });
        original.format_version = 4;
        let binding = rebind(&mut original);
        let first = plan_clone_import(
            &original,
            &binding,
            &BTreeMap::new(),
            &ids(&original, "review-first"),
        )
        .unwrap();
        assert_eq!(first.manifest.format_version, 4);
        assert_ne!(
            first.manifest.revision_lineage[1].review.decisions[0].actor,
            original.revision_lineage[1].review.decisions[0].actor
        );
        let ancestors =
            BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original.clone())]);
        let mut altered = first.manifest.clone();
        altered.revision_lineage[1].review.decisions[0].note = "Changed rationale".into();
        let binding = rebind(&mut altered);
        assert!(validate_import_origins(&altered, &binding, &ancestors).is_err());
        let mut altered = first.manifest.clone();
        altered.revision_lineage[1].review.decisions[0].actor = "unrelated-actor".into();
        assert!(altered.validate().is_err());
        let mut altered = original;
        altered.format_version = 3;
        assert!(
            altered.validate().is_err(),
            "older format cannot silently drop declarations"
        );
    }

    #[test]
    fn second_clone_composes_namespaces_after_new_local_revision_and_rebase_alias() {
        let (original, first) = clone_first();
        let mut edited = first.manifest.clone();
        let mut r = edited.revisions[0].clone();
        r.revision = RevisionId::new(1);
        r.actor = "local-live-actor".into();
        r.kind = "edit".into();
        edited.revisions.push(r.clone());
        edited.head.revision = r.revision;
        edited.head.history_cursor = 1;
        edited.captured_revision = r.revision;
        edited.captured_event_cursor += 3;
        edited.edit_states.push(PackageEditState {
            position: 1,
            motion: r.motion,
            protected: vec![],
            source_revision: Some(r.revision),
        });
        let mut alias = edited.candidates[0].clone();
        alias.rebased_from = Some(edited.candidates[0].candidate_id.clone());
        alias.candidate_id = CandidateId::new("candidate.rebased").unwrap();
        alias.base_revision = RevisionId::new(1);
        edited.candidates.push(alias.clone());
        edited
            .candidates
            .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        edited.imported_origins[0]
            .candidates
            .push(PackageCandidateOrigin {
                local: alias.candidate_id,
                origin: original.candidates[0].candidate_id.clone(),
            });
        edited.imported_origins[0]
            .candidates
            .sort_by(|a, b| a.local.cmp(&b.local));
        edited.format_version = edited.required_format_version();
        let d = rebind(&mut edited);
        let ancestors =
            BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original.clone())]);
        let second = plan_clone_import(&edited, &d, &ancestors, &ids(&edited, "two")).unwrap();
        assert_eq!(second.manifest.revisions.len(), 2);
        assert_eq!(second.manifest.candidates.len(), 2);
        assert_eq!(second.manifest.format_version, 9);
        let child = second
            .manifest
            .candidates
            .iter()
            .find(|c| c.rebased_from.is_some())
            .unwrap();
        assert!(second
            .manifest
            .candidates
            .iter()
            .any(|c| Some(&c.candidate_id) == child.rebased_from.as_ref()));
        assert_eq!(second.manifest.imported_origins.len(), 2);
        let ancestral = second
            .manifest
            .imported_origins
            .iter()
            .find(|o| o.manifest == first.origin_manifest_ref)
            .unwrap();
        assert_eq!(ancestral.candidates.len(), 2);
        assert_eq!(
            ancestral.candidates[0].origin,
            ancestral.candidates[1].origin
        );
        assert_eq!(ancestral.objects, original.objects);
        assert_eq!(
            ancestors[&first.origin_manifest_ref.sha256]
                .canonical_bytes()
                .unwrap(),
            first.origin_manifest
        );
        let mut all = ancestors;
        all.insert(second.origin_manifest_ref.sha256.clone(), edited);
        validate_import_origins(&second.manifest, &descriptor(&second.manifest), &all).unwrap();
        let mut dropped = second.manifest.clone();
        dropped
            .candidates
            .iter_mut()
            .find(|c| c.rebased_from.is_some())
            .unwrap()
            .rebased_from = None;
        assert!(validate_import_origins(&dropped, &descriptor(&dropped), &all).is_err());
    }

    #[test]
    fn unresolved_changed_or_unrelated_originals_are_rejected() {
        let (original, first) = clone_first();
        let d = descriptor(&first.manifest);
        assert!(validate_import_origins(&first.manifest, &d, &BTreeMap::new()).is_err());
        let mut ancestor = original.clone();
        ancestor.head.name = "rewritten".into();
        let mut map = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), ancestor)]);
        assert!(validate_import_origins(&first.manifest, &d, &map).is_err());
        map.insert(first.origin_manifest_ref.sha256.clone(), original.clone());
        let mut unrelated = original;
        unrelated.head.name = "unrelated".into();
        map.insert(digest(&unrelated.canonical_bytes().unwrap()), unrelated);
        assert!(validate_import_origins(&first.manifest, &d, &map).is_err());
    }

    #[test]
    fn origin_mapping_cannot_rewrite_revision_lineage_or_candidate_payload() {
        let (original, first) = clone_first();
        let map = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let mut changed = first.manifest.clone();
        changed.revisions[0].label = "fabricated".into();
        let d = rebind(&mut changed);
        assert!(validate_import_origins(&changed, &d, &map).is_err());
        let mut changed = first.manifest.clone();
        changed.imported_origins[0].candidates.clear();
        let d = rebind(&mut changed);
        assert!(validate_import_origins(&changed, &d, &map).is_err());
        let mut changed = first.manifest.clone();
        changed.imported_origins[0].actors[0].origin = "fabricated actor".into();
        let d = rebind(&mut changed);
        assert!(validate_import_origins(&changed, &d, &map).is_err());
    }

    #[test]
    fn original_catalog_and_capture_binding_must_match_exactly() {
        let (original, first) = clone_first();
        let map = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let mut changed = first.manifest.clone();
        changed.imported_origins[0].container.captured_event_cursor += 1;
        let d = rebind(&mut changed);
        assert!(validate_import_origins(&changed, &d, &map).is_err());
        let mut wrong = descriptor(&first.manifest);
        wrong.project_id = ProjectId::new("wrong").unwrap();
        assert!(validate_import_origins(&first.manifest, &wrong, &map).is_err());
    }

    #[test]
    fn metadata_validation_does_not_certify_full_container_digest() {
        let original = fixture();
        let mut descriptor = descriptor(&original);
        descriptor.sha256 = "c".repeat(64);
        // Intentionally accepted by this pure seam: storage independently
        // reconstructs the flat container bytes and must reject this digest.
        validate_import_origins(&original, &descriptor, &BTreeMap::new()).unwrap();
    }

    #[test]
    fn overlarge_resolved_graph_fails_before_decoding_all_nodes() {
        let m = fixture();
        let map = (0..=MAX_PACKAGE_IMPORTED_ORIGINS)
            .map(|n| (format!("{n:064x}"), m.clone()))
            .collect();
        assert_eq!(
            validate_import_origins(&m, &descriptor(&m), &map)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn contradictory_transitive_candidate_identity_is_rejected_even_for_equal_content() {
        let mut original = fixture();
        let mut other = original.candidates[0].clone();
        other.candidate_id = CandidateId::new("candidate:z").unwrap();
        original.candidates.push(other);
        let original_binding = rebind(&mut original);
        let first = plan_clone_import(
            &original,
            &original_binding,
            &BTreeMap::new(),
            &ids(&original, "first"),
        )
        .unwrap();
        let mut ancestors = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let second = plan_clone_import(
            &first.manifest,
            &descriptor(&first.manifest),
            &ancestors,
            &ids(&first.manifest, "second"),
        )
        .unwrap();
        ancestors.insert(second.origin_manifest_ref.sha256.clone(), first.manifest);
        let mut forged = second.manifest;
        let old = forged
            .imported_origins
            .iter_mut()
            .find(|edge| edge.manifest.sha256 == first.origin_manifest_ref.sha256)
            .unwrap();
        let one = old.candidates[0].origin.clone();
        old.candidates[0].origin = old.candidates[1].origin.clone();
        old.candidates[1].origin = one;
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "equal candidate payloads cannot justify contradictory identity ancestry"
        );
    }

    #[test]
    fn legacy_history_zero_cannot_change_under_an_unchanged_origin_revision() {
        let mut original = fixture();
        let mut revision = original.revisions[0].clone();
        revision.revision = RevisionId::new(1);
        original.revisions.push(revision.clone());
        original.edit_states.push(PackageEditState {
            position: 1,
            motion: revision.motion.clone(),
            protected: vec![],
            source_revision: Some(revision.revision),
        });
        original.head.revision = revision.revision;
        original.head.history_cursor = 1;
        original.captured_revision = revision.revision;
        let binding = rebind(&mut original);
        let first = plan_clone_import(
            &original,
            &binding,
            &BTreeMap::new(),
            &ids(&original, "history"),
        )
        .unwrap();
        let ancestors = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let mut forged = first.manifest;
        assert_eq!(forged.edit_states[0].source_revision, None);
        forged.edit_states[0].motion.artifact_id = ArtifactId::new("motion.forged").unwrap();
        forged.edit_states[0].motion.sha256 = "c".repeat(64);
        forged.objects.push(PackageObjectDescriptor {
            sha256: "c".repeat(64),
            byte_len: 13,
            roles: vec![PackageObjectRole::MotionProgram],
        });
        forged.objects.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "a legacy absent history link must not allow rewriting the undo baseline"
        );
    }

    #[test]
    fn imported_candidate_cannot_override_source_review_through_native_authored_classification() {
        let mut original = fixture();
        let receipt = ArtifactIdentity {
            artifact_id: ArtifactId::new("receipt.original").unwrap(),
            sha256: "d".repeat(64),
            byte_len: 8,
        };
        original.candidates[0].lineage = vec![receipt.clone()];
        original.authored_lineage.push(PackageAuthoredLineage {
            wasm_invocation: None,
            candidate_id: original.candidates[0].candidate_id.clone(),
            receipt,
            input: PackageObjectRef {
                sha256: "a".repeat(64),
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
        original.objects[0].roles.push(PackageObjectRole::EditInput);
        original.objects.push(PackageObjectDescriptor {
            sha256: "d".repeat(64),
            byte_len: 8,
            roles: vec![PackageObjectRole::EditReceipt],
        });
        let binding = rebind(&mut original);
        let first = plan_clone_import(
            &original,
            &binding,
            &BTreeMap::new(),
            &ids(&original, "classified"),
        )
        .unwrap();
        assert!(first.candidate_reviews.values().next().unwrap().unknown);
        let mut forged = first.manifest;
        let mut native = original.authored_lineage[0].clone();
        native.candidate_id = forged.candidates[0].candidate_id.clone();
        native.review.unknown = false;
        forged.authored_lineage.push(native);
        let ancestors = BTreeMap::from([(first.origin_manifest_ref.sha256, original)]);
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "native authored rows cannot shadow an imported candidate's original review state"
        );
    }

    #[test]
    fn absent_intermediate_ancestry_cannot_be_invented_by_a_later_clone() {
        let (original, first) = clone_first();
        let original_candidate = original.candidates[0].candidate_id.clone();
        let mut middle = first.manifest;
        // A new native candidate has equal values but no ancestry mapping to A.
        let native_id = CandidateId::new("candidate.native").unwrap();
        let mut native = middle.candidates[0].clone();
        native.candidate_id = native_id.clone();
        middle.candidates.push(native);
        middle
            .candidates
            .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        let middle_binding = rebind(&mut middle);
        let mut ancestors = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let second = plan_clone_import(
            &middle,
            &middle_binding,
            &ancestors,
            &ids(&middle, "missing-edge"),
        )
        .unwrap();
        let local_native = second.identities.candidates[&native_id].clone();
        ancestors.insert(second.origin_manifest_ref.sha256.clone(), middle);
        let mut forged = second.manifest;
        let direct = forged
            .imported_origins
            .iter_mut()
            .find(|origin| origin.manifest.sha256 == first.origin_manifest_ref.sha256)
            .unwrap();
        direct.candidates.push(PackageCandidateOrigin {
            local: local_native,
            origin: original_candidate,
        });
        direct.candidates.sort_by(|a, b| a.local.cmp(&b.local));
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "missing intermediate ancestry must remain absent, even for equal payloads"
        );
    }

    #[test]
    fn direct_only_ancestry_key_cannot_bypass_the_intermediate_namespace() {
        let (original, first) = clone_first();
        let original_candidate = original.candidates[0].candidate_id.clone();
        let mut ancestors = BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), original)]);
        let second = plan_clone_import(
            &first.manifest,
            &descriptor(&first.manifest),
            &ancestors,
            &ids(&first.manifest, "direct-only"),
        )
        .unwrap();
        ancestors.insert(second.origin_manifest_ref.sha256.clone(), first.manifest);
        let mut forged = second.manifest;
        let mut extra = forged.candidates[0].clone();
        extra.candidate_id = CandidateId::new("candidate.extra").unwrap();
        let extra_id = extra.candidate_id.clone();
        forged.candidates.push(extra);
        forged
            .candidates
            .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        let direct = forged
            .imported_origins
            .iter_mut()
            .find(|origin| origin.manifest.sha256 == first.origin_manifest_ref.sha256)
            .unwrap();
        direct.candidates.push(PackageCandidateOrigin {
            local: extra_id,
            origin: original_candidate,
        });
        direct.candidates.sort_by(|a, b| a.local.cmp(&b.local));
        let binding = rebind(&mut forged);
        assert!(
            validate_import_origins(&forged, &binding, &ancestors).is_err(),
            "a direct-only key must not invent ancestry missing from the intermediate namespace"
        );
    }
}
