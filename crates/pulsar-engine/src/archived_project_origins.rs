//! Shared read-only verification of retained clone namespaces. Paths in opaque
//! records stay inert; only engine-owned CAS evidence is opened by identity.
use super::*;
use crate::project_package_import_storage::ImportStore;
use std::collections::BTreeMap;
type ArchiveRow = (i64, Option<String>, Option<String>, Option<String>);

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "retained archive namespace or identity-map correspondence changed",
    )
}
fn absent() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        "no retained archive corresponds to this project/revision",
    )
}
fn memory(pool: &ResourcePool, bytes: u64) -> PResult<Reservation> {
    let needed = bytes
        .checked_mul(8)
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(mismatch)?;
    pool.reserve_memory(needed)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginMap {
    project_id: ProjectId,
    candidates: BTreeMap<CandidateId, CandidateId>,
    #[serde(default)]
    placements: BTreeMap<SourcePlacementId, SourcePlacementId>,
}
pub(super) struct VerifiedArchive {
    pub root: PortableProjectManifest,
    pub origins: BTreeMap<String, PortableProjectManifest>,
    pub immediate_manifest: String,
    _memory: Vec<Reservation>,
}
pub(super) fn load(
    db: &Connection,
    project: &ProjectId,
    revision_limit: Option<RevisionId>,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<VerifiedArchive> {
    // Bound SQLite cells before Rust allocations; retain metadata admission
    // until graph checks and the small response are complete.
    let mut retained_memory = vec![memory(pool, MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64)?];
    let row:Option<ArchiveRow>=db.query_row(
        "SELECT captured_revision,CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 16777216 THEN manifest ELSE NULL END,CASE WHEN typeof(origin_map)='text' AND length(CAST(origin_map AS BLOB)) BETWEEN 1 AND 4096 THEN origin_map ELSE NULL END,CASE WHEN typeof(package)='text' AND length(CAST(package AS BLOB)) BETWEEN 1 AND 4096 THEN package ELSE NULL END FROM project_import_archives WHERE project=?1",
        [project.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(internal)?;
    let (captured, raw, map, package) = row.ok_or_else(absent)?;
    if revision_limit.is_some_and(|revision| rev_sql(revision).is_ok_and(|value| value > captured))
    {
        return Err(absent());
    }
    let raw = raw.ok_or_else(mismatch)?;
    let manifest: PortableProjectManifest = decode(&raw)?;
    manifest.validate()?;
    if manifest.origin_project_id != *project
        || rev_sql(manifest.captured_revision)? != captured
        || manifest.canonical_bytes()? != raw.as_bytes()
    {
        return Err(mismatch());
    }
    let map: ArtifactIdentity = decode(&map.ok_or_else(mismatch)?)?;
    map.validate()?;
    let package: PackageUploadDeclaration = decode(&package.ok_or_else(mismatch)?)?;
    package.validate()?;
    retained_memory.push(memory(pool, map.byte_len)?);
    let map_bytes = store.read_retained_evidence(
        &PackageObjectRef {
            sha256: map.sha256.clone(),
            byte_len: map.byte_len,
        },
        MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
    )?;
    let identity_map: OriginMap = serde_json::from_slice(&map_bytes).map_err(|_| mismatch())?;
    let immediate: Vec<_> = manifest
        .imported_origins
        .iter()
        .filter(|o| {
            o.container.sha256 == package.sha256
                && o.container.byte_len == package.byte_len
                && o.container.format_version == package.format_version
        })
        .collect();
    if immediate.len() != 1 || identity_map.project_id != *project {
        return Err(mismatch());
    }
    let immediate = immediate[0];
    if identity_map.candidates
        != immediate
            .candidates
            .iter()
            .map(|m| (m.origin.clone(), m.local.clone()))
            .collect()
        || identity_map.placements
            != immediate
                .placements
                .iter()
                .map(|m| (m.origin.clone(), m.local.clone()))
                .collect()
    {
        return Err(mismatch());
    }

    let mut origins = BTreeMap::new();

    let mut total_bytes = 0u64;
    for object in manifest
        .objects
        .iter()
        .filter(|o| o.roles.contains(&PackageObjectRole::ImportedManifest))
    {
        if origins.len() >= MAX_PACKAGE_IMPORTED_ORIGINS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "archival origin count exceeds admission",
            ));
        }
        total_bytes = total_bytes
            .checked_add(object.byte_len)
            .ok_or_else(mismatch)?;
        if total_bytes > MAX_PACKAGE_ORIGIN_MANIFEST_BYTES {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "archival origin bytes exceed admission",
            ));
        }
        retained_memory.push(memory(pool, object.byte_len)?);
        let data = store.read_retained_evidence(
            &PackageObjectRef {
                sha256: object.sha256.clone(),
                byte_len: object.byte_len,
            },
            MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
        )?;
        let origin: PortableProjectManifest =
            serde_json::from_slice(&data).map_err(|_| mismatch())?;
        if origin.canonical_bytes()? != data {
            return Err(mismatch());
        }
        origins.insert(object.sha256.clone(), origin);
    }
    crate::project_package_import_plan::validate_retained_origin_graph(&manifest, &origins)?;
    Ok(VerifiedArchive {
        immediate_manifest: immediate.manifest.sha256.clone(),
        root: manifest,
        origins,
        _memory: retained_memory,
    })
}
