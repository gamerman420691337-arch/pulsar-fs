//! Application restrictions for inert imported proposals, separate from jobs.
use super::*;

pub(super) fn initialize(db: &Connection) -> PResult<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS imported_candidate_intervals(candidate TEXT PRIMARY KEY REFERENCES candidates(id),project TEXT NOT NULL REFERENCES projects(id),scope TEXT NOT NULL);").map_err(internal)?;
    validate_layout(db).map_err(internal)
}
pub(super) fn validate_layout(db: &Connection) -> Result<()> {
    let mut statement = db.prepare("PRAGMA table_info(imported_candidate_intervals)")?;
    let columns = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        columns
            == vec![
                ("candidate".into(), "TEXT".into(), 0, 1),
                ("project".into(), "TEXT".into(), 1, 0),
                ("scope".into(), "TEXT".into(), 1, 0)
            ],
        "imported interval database schema is incomplete; explicit recovery is required"
    );
    Ok(())
}
pub(super) fn load(
    db: &Connection,
    id: &CandidateId,
    project: &ProjectId,
) -> PResult<Option<TimeRange>> {
    let raw:Option<Option<String>>=db.query_row("SELECT CASE WHEN s.project=a.project AND typeof(s.scope)='text' AND length(CAST(s.scope AS BLOB)) BETWEEN 1 AND 4096 THEN s.scope ELSE NULL END FROM project_import_candidate_aliases a LEFT JOIN imported_candidate_intervals s ON s.candidate=a.candidate WHERE a.candidate=?1 AND a.project=?2",params![id.as_str(),project.as_str()],|r|r.get(0)).optional().map_err(internal)?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let raw = raw.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "imported candidate scope is unresolved; verified archival recovery is required",
        )
    })?;
    let scope: Option<TimeRange> = decode(&raw)?;
    if scope.is_some_and(|r| r.start() < ProjectTime::ZERO || r.start() >= r.end()) {
        return Err(ProtocolError::invalid(
            "invalid stored imported candidate interval",
        ));
    }
    Ok(scope)
}
pub(super) fn insert(
    db: &Connection,
    id: &CandidateId,
    project: &ProjectId,
    scope: Option<TimeRange>,
) -> PResult<()> {
    db.execute(
        "INSERT INTO imported_candidate_intervals(candidate,project,scope) VALUES(?1,?2,?3)",
        params![id.as_str(), project.as_str(), encode(&scope)?],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn rebase(db: &Connection, old: &CandidateId, new: &CandidateId) -> PResult<()> {
    db.execute("INSERT INTO imported_candidate_intervals(candidate,project,scope) SELECT ?2,project,scope FROM imported_candidate_intervals WHERE candidate=?1",params![old.as_str(),new.as_str()]).map_err(internal)?;
    Ok(())
}

/// Recover legacy alias restrictions from retained checked originals. Policy
/// publication is one transaction; original archives and project motion remain
/// untouched. Failure leaves every missing row unresolved and application closed.
pub(super) fn recover(
    db: &mut Connection,
    pool: &ResourcePool,
    store: &crate::project_package_import_storage::ImportStore,
) -> PResult<()> {
    use std::collections::BTreeMap;
    let missing:i64=db.query_row("SELECT COUNT(*) FROM project_import_candidate_aliases a LEFT JOIN imported_candidate_intervals s ON s.candidate=a.candidate WHERE s.candidate IS NULL",[],|r|r.get(0)).map_err(internal)?;
    if missing == 0 {
        preserve_reader_barrier(db)?;
        return Ok(());
    }
    if missing < 0 || missing as u64 > MAX_PROJECT_PACKAGE_ENTRIES as u64 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "legacy interval recovery alias count exceeds bound",
        ));
    }
    let _rows_memory = pool.reserve_memory(128 * 1024 * 1024)?;
    let aliases: Vec<(String, String, String, String)> = {
        let mut statement=db.prepare("SELECT a.project,a.candidate,a.imported_candidate,c.program FROM project_import_candidate_aliases a JOIN candidates c ON c.id=a.candidate AND c.project=a.project LEFT JOIN imported_candidate_intervals s ON s.candidate=a.candidate WHERE s.candidate IS NULL AND typeof(a.project)='text' AND length(CAST(a.project AS BLOB))<=128 AND typeof(a.candidate)='text' AND length(CAST(a.candidate AS BLOB))<=128 AND typeof(a.imported_candidate)='text' AND length(CAST(a.imported_candidate AS BLOB))<=128 AND typeof(c.program)='text' AND length(CAST(c.program AS BLOB))<=71 AND length(CAST(c.lineage AS BLOB))<=524288 AND c.job IS NULL ORDER BY a.project,a.candidate LIMIT 100001").map_err(internal)?;
        let rows = statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(internal)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(internal)?
    };
    if missing != aliases.len() as i64 {
        return Err(ProtocolError::invalid(
            "legacy scope aliases are malformed or exceed bounds",
        ));
    }
    if aliases.len() > MAX_PROJECT_PACKAGE_ENTRIES {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "legacy interval recovery alias count exceeds bound",
        ));
    }
    let mut recovered = Vec::new();
    let mut offset = 0;
    while offset < aliases.len() {
        let project = &aliases[offset].0;
        let end = offset
            + aliases[offset..]
                .iter()
                .take_while(|a| &a.0 == project)
                .count();
        let _root_memory = pool.reserve_memory(144 * 1024 * 1024)?;
        let row:Option<(Option<String>,Option<String>,Option<String>)>=db.query_row("SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 16777216 THEN manifest ELSE NULL END,CASE WHEN typeof(origin_map)='text' AND length(CAST(origin_map AS BLOB)) BETWEEN 1 AND 4096 THEN origin_map ELSE NULL END,CASE WHEN typeof(package)='text' AND length(CAST(package AS BLOB)) BETWEEN 1 AND 4096 THEN package ELSE NULL END FROM project_import_archives WHERE project=?1",[project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
        let (raw, map, package) =
            row.ok_or_else(|| ProtocolError::invalid("legacy scope archive missing"))?;
        let raw = raw
            .ok_or_else(|| ProtocolError::invalid("legacy scope archive exceeds metadata bound"))?;
        let root: PortableProjectManifest = decode(&raw)?;
        root.validate()?;
        if root.origin_project_id.as_str() != project || root.canonical_bytes()? != raw.as_bytes() {
            return Err(ProtocolError::invalid("legacy scope root identity changed"));
        }
        let map: ArtifactIdentity = decode(
            &map.ok_or_else(|| ProtocolError::invalid("legacy scope identity map missing"))?,
        )?;
        map.validate()?;
        let package: PackageUploadDeclaration =
            decode(&package.ok_or_else(|| {
                ProtocolError::invalid("legacy scope container binding missing")
            })?)?;
        package.validate()?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct IdentityMap {
            project_id: ProjectId,
            candidates: BTreeMap<CandidateId, CandidateId>,
            #[serde(default)]
            placements: BTreeMap<SourcePlacementId, SourcePlacementId>,
        }
        let _map_memory = pool.reserve_memory(
            map.byte_len
                .checked_mul(8)
                .and_then(|n| n.checked_add(16 * 1024 * 1024))
                .ok_or_else(|| ProtocolError::invalid("legacy scope memory overflow"))?,
        )?;
        let data = store.read_retained_evidence(
            &PackageObjectRef {
                sha256: map.sha256,
                byte_len: map.byte_len,
            },
            MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
        )?;
        let identity: IdentityMap = serde_json::from_slice(&data).map_err(internal)?;
        let edges: Vec<_> = root
            .imported_origins
            .iter()
            .filter(|e| {
                e.container.sha256 == package.sha256
                    && e.container.byte_len == package.byte_len
                    && e.container.format_version == package.format_version
            })
            .collect();
        if edges.len() != 1
            || identity.project_id.as_str() != project
            || identity.candidates
                != edges[0]
                    .candidates
                    .iter()
                    .map(|m| (m.origin.clone(), m.local.clone()))
                    .collect()
            || identity.placements
                != edges[0]
                    .placements
                    .iter()
                    .map(|m| (m.origin.clone(), m.local.clone()))
                    .collect()
        {
            return Err(ProtocolError::invalid(
                "legacy scope original alias binding changed",
            ));
        }
        let mut origins = BTreeMap::new();
        let mut holds = Vec::new();
        let mut total = 0u64;
        for object in root
            .objects
            .iter()
            .filter(|o| o.roles.contains(&PackageObjectRole::ImportedManifest))
        {
            total = total
                .checked_add(object.byte_len)
                .ok_or_else(|| ProtocolError::invalid("legacy origin bytes overflow"))?;
            if total > MAX_PACKAGE_ORIGIN_MANIFEST_BYTES
                || origins.len() >= MAX_PACKAGE_IMPORTED_ORIGINS
            {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "legacy scope origin graph exceeds bounds",
                ));
            }
            holds.push(
                pool.reserve_memory(
                    object
                        .byte_len
                        .checked_mul(8)
                        .and_then(|n| n.checked_add(16 * 1024 * 1024))
                        .ok_or_else(|| ProtocolError::invalid("legacy scope memory overflow"))?,
                )?,
            );
            let data = store.read_retained_evidence(
                &PackageObjectRef {
                    sha256: object.sha256.clone(),
                    byte_len: object.byte_len,
                },
                MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
            )?;
            let original: PortableProjectManifest =
                serde_json::from_slice(&data).map_err(internal)?;
            if original.canonical_bytes()? != data {
                return Err(ProtocolError::invalid(
                    "legacy scope original canonical bytes changed",
                ));
            }
            origins.insert(object.sha256.clone(), original);
        }
        crate::project_package_import_plan::validate_retained_origin_graph(&root, &origins)?;
        let mut native = crate::project_package_import_plan::NativeCandidateIntervals::new();
        for manifest in std::iter::once(&root).chain(origins.values()) {
            let namespace = format!("{:x}", Sha256::digest(manifest.canonical_bytes()?));
            for origin in &manifest.generated_origins {
                let _request_memory = pool.reserve_memory(8 * PROGRAM_LIMIT as u64)?;
                if !root.objects.iter().any(|o| {
                    o.sha256 == origin.manifest.sha256
                        && o.byte_len == origin.manifest.byte_len
                        && o.roles.contains(&PackageObjectRole::WorkerManifest)
                }) {
                    return Err(ProtocolError::invalid(
                        "legacy scope worker object absent from verified closure",
                    ));
                }
                let data = store.read_retained_evidence(&origin.manifest, PROGRAM_LIMIT as u64)?;
                let request: WorkerRequest = serde_json::from_slice(&data).map_err(internal)?;
                let scope =
                    project_package_imports::checked_worker_scope(manifest, origin, &request)?;
                for candidate in manifest
                    .candidates
                    .iter()
                    .filter(|c| c.job_origin.as_ref() == Some(&origin.job_id))
                {
                    native.insert((namespace.clone(), candidate.candidate_id.clone()), scope);
                }
            }
        }
        let scopes = crate::project_package_import_plan::resolve_candidate_intervals(
            &root, &origins, &native,
        )?;
        for (_, local, original, program) in &aliases[offset..end] {
            let original = CandidateId::new(original).map_err(internal)?;
            let candidate = root
                .candidates
                .iter()
                .find(|c| c.candidate_id == original)
                .ok_or_else(|| ProtocolError::invalid("legacy scope original candidate missing"))?;
            let _lineage_memory = pool.reserve_memory(8 * PROGRAM_LIMIT as u64)?;
            let lineage:Option<String>=db.query_row("SELECT CASE WHEN typeof(lineage)='text' AND length(CAST(lineage AS BLOB))<=524288 THEN lineage ELSE NULL END FROM candidates WHERE id=?1",[local],|r|r.get(0)).map_err(internal)?;
            let actual: Vec<ArtifactIdentity> =
                decode(&lineage.ok_or_else(|| {
                    ProtocolError::invalid("legacy scope lineage exceeds bounds")
                })?)?;
            if !identity.candidates.values().any(|id| id == &original)
                || program != &format!("motion:{}", candidate.motion.sha256)
                || actual != candidate.lineage
            {
                return Err(ProtocolError::invalid(
                    "legacy scope local proposal differs from bound original",
                ));
            }
            recovered.push((
                CandidateId::new(local).map_err(internal)?,
                root.origin_project_id.clone(),
                *scopes
                    .get(&original)
                    .ok_or_else(|| ProtocolError::invalid("legacy scope resolution missing"))?,
            ));
        }
        offset = end;
    }
    let tx = db.transaction().map_err(internal)?;
    for (candidate, project, scope) in recovered {
        insert(&tx, &candidate, &project, scope)?;
    }
    preserve_reader_barrier(&tx)?;
    tx.commit().map_err(internal)?;
    Ok(())
}

fn preserve_reader_barrier(db: &Connection) -> PResult<()> {
    if crate::inference_equivalence_reader::version(db).map_err(internal)? < 9 {
        crate::inference_equivalence_reader::publish(db, 9).map_err(internal)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, ProjectId, CandidateId) {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY);CREATE TABLE candidates(id TEXT PRIMARY KEY);CREATE TABLE project_import_candidate_aliases(candidate TEXT PRIMARY KEY,project TEXT,imported_candidate TEXT,review_state TEXT);INSERT INTO projects VALUES('p');INSERT INTO candidates VALUES('c');").unwrap();
        initialize(&db).unwrap();
        (
            db,
            ProjectId::new("p").unwrap(),
            CandidateId::new("c").unwrap(),
        )
    }
    #[test]
    fn legacy_alias_requires_resolution_and_explicit_whole_scope_is_distinct() {
        let (db, project, candidate) = fixture();
        assert_eq!(load(&db, &candidate, &project).unwrap(), None);
        db.execute(
            "INSERT INTO project_import_candidate_aliases VALUES('c','p','c','{}')",
            [],
        )
        .unwrap();
        assert_eq!(
            load(&db, &candidate, &project).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        insert(&db, &candidate, &project, None).unwrap();
        assert_eq!(load(&db, &candidate, &project).unwrap(), None);
    }
    #[test]
    fn interval_rebase_preserves_policy_and_corrupt_metadata_refuses() {
        let (db, project, candidate) = fixture();
        db.execute(
            "INSERT INTO project_import_candidate_aliases VALUES('c','p','c','{}')",
            [],
        )
        .unwrap();
        let interval = TimeRange::new(
            ProjectTime::from_nanos(1_000_000_000),
            ProjectTime::from_nanos(6_000_000_000),
        )
        .unwrap();
        insert(&db, &candidate, &project, Some(interval)).unwrap();
        db.execute_batch("INSERT INTO candidates VALUES('next');INSERT INTO project_import_candidate_aliases VALUES('next','p','c','{}');").unwrap();
        let next = CandidateId::new("next").unwrap();
        rebase(&db, &candidate, &next).unwrap();
        assert_eq!(load(&db, &next, &project).unwrap(), Some(interval));
        db.execute(
            "UPDATE imported_candidate_intervals SET scope=?1 WHERE candidate='next'",
            ["x".repeat(4097)],
        )
        .unwrap();
        assert_eq!(
            load(&db, &next, &project).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        db.execute("INSERT INTO projects VALUES('another')", [])
            .unwrap();
        db.execute("UPDATE imported_candidate_intervals SET scope='null',project='another' WHERE candidate='next'",[]).unwrap();
        assert_eq!(
            load(&db, &next, &project).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }
}
