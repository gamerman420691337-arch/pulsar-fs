//! Verified archival links for the publication ancestry walker. Original IDs
//! identify historical evidence; they never resolve jobs or authorize effects.
use super::*;
use crate::project_package_import_storage::ImportStore;
use std::collections::{BTreeMap, BTreeSet};

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "archived publication ancestry differs from retained evidence",
    )
}
pub(super) enum CandidateLink {
    Generation,
    Authored(RevisionId),
    Rebased(CandidateId, RevisionId),
}
/// Borrowed metadata from one local revision-lineage row.
pub(super) struct RevisionRow<'a> {
    pub revision: i64,
    pub parent: Option<i64>,
    pub restored: Option<i64>,
    pub candidate: Option<&'a str>,
    pub operation: &'a str,
    pub contributions: &'a str,
}
pub(super) struct Links<'a> {
    pub graph: archived_project_origins::VerifiedArchive,
    pool: &'a ResourcePool,
    store: &'a ImportStore,
    selected: &'a [ArchivedGenerationOrigin],
    verified: BTreeSet<String>,
    independent_clocks: bool,
}
impl<'a> Links<'a> {
    pub fn new(
        db: &Connection,
        project: &ProjectId,
        pool: &'a ResourcePool,
        store: &'a ImportStore,
        selected: &'a [ArchivedGenerationOrigin],
    ) -> PResult<Self> {
        Ok(Self {
            graph: archived_project_origins::load(db, project, None, pool, store)?,
            pool,
            store,
            selected,
            verified: BTreeSet::new(),
            independent_clocks: false,
        })
    }
    /// Used only by the exact action-clock walker. Every generated leaf must
    /// subsequently resolve one same-source producer and a verified sample plan.
    pub fn for_action_clocks(
        db: &Connection,
        project: &ProjectId,
        pool: &'a ResourcePool,
        store: &'a ImportStore,
        selected: &'a [ArchivedGenerationOrigin],
    ) -> PResult<Self> {
        let mut links = Self::new(db, project, pool, store, selected)?;
        links.independent_clocks = true;
        Ok(links)
    }
    pub fn check_revision(
        &self,
        db: &Connection,
        project: &ProjectId,
        row: RevisionRow<'_>,
    ) -> PResult<()> {
        let RevisionRow {
            revision,
            parent,
            restored,
            candidate,
            operation,
            contributions,
        } = row;
        let id = RevisionId::new(u64::try_from(revision).map_err(internal)?);
        if id > self.graph.root.captured_revision {
            return Ok(());
        }
        let declared = self
            .graph
            .root
            .revision_lineage
            .iter()
            .find(|r| r.revision == id)
            .ok_or_else(mismatch)?;
        let regions: Vec<(Axis, TimeRange)> = decode(contributions)?;
        if declared.parent_revision.map(rev_sql).transpose()? != parent
            || declared.restored_from_revision.map(rev_sql).transpose()? != restored
            || declared.candidate.as_ref().map(|c| c.as_str()) != candidate
            || declared.operation != operation
            || declared.contributions != regions
        {
            return Err(mismatch());
        }
        let motion = self
            .graph
            .root
            .revisions
            .iter()
            .find(|r| r.revision == id)
            .ok_or_else(mismatch)?;
        let reference:Option<String>=db.query_row("SELECT CASE WHEN typeof(program)='text' AND length(CAST(program AS BLOB))=71 THEN program ELSE NULL END FROM revisions WHERE project=?1 AND revision=?2",params![project.as_str(),revision],|r|r.get(0)).map_err(internal)?;
        if reference.as_deref() != Some(format!("motion:{}", motion.motion.sha256).as_str()) {
            return Err(mismatch());
        }
        Ok(())
    }
    pub fn candidate(
        &mut self,
        db: &Connection,
        project: &ProjectId,
        id: &str,
    ) -> PResult<Option<CandidateLink>> {
        let Some(anchor) = self
            .graph
            .root
            .candidates
            .iter()
            .find(|c| c.candidate_id.as_str() == id)
        else {
            return Ok(None);
        };
        let row:Option<(Option<String>,Option<String>,bool)>=db.query_row("SELECT CASE WHEN typeof(c.program)='text' AND length(CAST(c.program AS BLOB))=71 THEN c.program ELSE NULL END,CASE WHEN typeof(c.lineage)='text' AND length(CAST(c.lineage AS BLOB)) BETWEEN 1 AND 524288 THEN c.lineage ELSE NULL END,c.job IS NOT NULL FROM candidates c JOIN project_import_candidate_aliases a ON a.candidate=c.id AND a.project=c.project AND a.imported_candidate=c.id WHERE c.project=?1 AND c.id=?2 AND NOT EXISTS(SELECT 1 FROM candidate_rebases b WHERE b.candidate=c.id)",params![project.as_str(),id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
        let (program, lineage, job) = row.ok_or_else(mismatch)?;
        let lineage: Vec<ArtifactIdentity> = decode(&lineage.ok_or_else(mismatch)?)?;
        if job
            || program.as_deref() != Some(format!("motion:{}", anchor.motion.sha256).as_str())
            || lineage != anchor.lineage
        {
            return Err(mismatch());
        }
        if let Some(parent) = &anchor.rebased_from {
            let base: i64 = db
                .query_row(
                    "SELECT base_revision FROM candidates WHERE project=?1 AND id=?2",
                    params![project.as_str(), id],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if base != rev_sql(anchor.base_revision)? {
                return Err(mismatch());
            }
            return Ok(Some(CandidateLink::Rebased(
                parent.clone(),
                anchor.base_revision,
            )));
        }
        let mut map_entries = self.graph.root.revisions.len();
        let mut memory = vec![self.pool.reserve_memory(
            (map_entries as u64)
                .checked_mul(128)
                .and_then(|n| n.checked_add(4096))
                .ok_or_else(mismatch)?,
        )?];
        let map: BTreeMap<_, _> = self
            .graph
            .root
            .revisions
            .iter()
            .map(|r| (r.revision, r.revision))
            .collect();
        let mut pending = vec![(String::new(), anchor.candidate_id.clone(), map)];
        let mut visited = BTreeMap::new();
        let mut generation = false;
        let mut bases = BTreeSet::new();
        while let Some((namespace, id, map)) = pending.pop() {
            let key = (namespace.clone(), id.clone());
            if let Some(previous) = visited.get(&key) {
                if previous != &map {
                    return Err(mismatch());
                }
                continue;
            }
            visited.insert(key, map.clone());
            if visited.len() > 2048 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "archived ancestry namespace traversal exceeds admission",
                ));
            }
            let manifest = if namespace.is_empty() {
                &self.graph.root
            } else {
                self.graph.origins.get(&namespace).ok_or_else(mismatch)?
            };
            let candidate = manifest
                .candidates
                .iter()
                .find(|c| c.candidate_id == id)
                .ok_or_else(mismatch)?;
            if candidate.job_origin.is_some()
                || manifest
                    .authored_lineage
                    .iter()
                    .any(|a| a.candidate_id == id)
            {
                if namespace.is_empty() {
                    return Err(mismatch());
                }
                if self.verified.insert(namespace.clone()) {
                    let size = manifest
                        .objects
                        .iter()
                        .filter(|o| {
                            !o.roles
                                .iter()
                                .all(|r| *r == PackageObjectRole::SourceSnapshot)
                        })
                        .try_fold(0u64, |n, o| n.checked_add(o.byte_len))
                        .ok_or_else(mismatch)?;
                    let _memory = self.pool.reserve_memory(
                        size.checked_mul(8)
                            .and_then(|n| n.checked_add(16 * 1024 * 1024))
                            .ok_or_else(mismatch)?,
                    )?;
                    project_package_imports::validate_evidence(
                        manifest,
                        self.pool,
                        |r| {
                            archived_generation::read(
                                self.store,
                                &self.graph.root,
                                r,
                                None,
                                self.pool,
                            )
                        },
                        &mut || Ok(()),
                    )?;
                }
                if let Some(job) = &candidate.job_origin {
                    // Different generation clocks require their own explicit
                    // correspondence proof, even when source hashes coincide.
                    if !self.independent_clocks
                        && !self.selected.iter().any(|s| {
                            s.origin_manifest.sha256 == namespace
                                && s.candidate_id == id
                                && s.job_id == *job
                                && manifest.generated_origins.iter().any(|record| {
                                    record.manifest == s.worker_manifest
                                        && record.receipt.as_ref() == Some(&s.worker_receipt)
                                        && archived_generation::has_producer_receipt(
                                            candidate, record,
                                        )
                                })
                                && candidate.motion.sha256 == s.program.sha256
                                && candidate.motion.byte_len == s.program.byte_len
                        })
                    {
                        return Err(mismatch());
                    }
                    generation = true;
                } else {
                    let authored = manifest
                        .authored_lineage
                        .iter()
                        .find(|a| a.candidate_id == id)
                        .ok_or_else(mismatch)?;
                    let _memory = self.pool.reserve_memory(
                        authored
                            .receipt
                            .byte_len
                            .checked_mul(8)
                            .and_then(|n| n.checked_add(16 * 1024 * 1024))
                            .ok_or_else(mismatch)?,
                    )?;
                    let raw = archived_generation::read(
                        self.store,
                        &self.graph.root,
                        &PackageObjectRef {
                            sha256: authored.receipt.sha256.clone(),
                            byte_len: authored.receipt.byte_len,
                        },
                        Some(PackageObjectRole::EditReceipt),
                        self.pool,
                    )?;
                    // Full typed receipt correspondence was checked above.
                    #[derive(Deserialize)]
                    struct Base {
                        base_revision: RevisionId,
                    }
                    let base: Base = serde_json::from_slice(&raw).map_err(|_| mismatch())?;
                    bases.insert(*map.get(&base.base_revision).ok_or_else(mismatch)?);
                }
            }
            for edge in &manifest.imported_origins {
                if let Some(mapped) = edge.candidates.iter().find(|m| m.local == id) {
                    map_entries = map_entries
                        .checked_add(edge.revisions.len())
                        .ok_or_else(mismatch)?;
                    if map_entries > MAX_PACKAGE_ORIGIN_ENTRIES {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "archived revision map composition exceeds admission",
                        ));
                    }
                    memory.push(
                        self.pool.reserve_memory(
                            (edge.revisions.len() as u64)
                                .checked_mul(128)
                                .and_then(|n| n.checked_add(4096))
                                .ok_or_else(mismatch)?,
                        )?,
                    );
                    let composed = edge
                        .revisions
                        .iter()
                        .map(|m| {
                            map.get(&m.local)
                                .map(|local| (m.origin, *local))
                                .ok_or_else(mismatch)
                        })
                        .collect::<PResult<BTreeMap<_, _>>>()?;
                    pending.push((
                        edge.manifest.sha256.clone(),
                        mapped.origin.clone(),
                        composed,
                    ));
                }
            }
        }
        match (generation, bases.len()) {
            (true, 0) => Ok(Some(CandidateLink::Generation)),
            (false, 1) => Ok(Some(CandidateLink::Authored(
                *bases.first().expect("one checked base"),
            ))),
            _ => Err(mismatch()),
        }
    }
    /// Resolve the same explicitly selected historical producer through retained
    /// objects. Selection never grants authority to imported jobs or paths.
    pub fn generation_plan(
        &self,
        db: &Connection,
        project: &ProjectId,
        id: &CandidateId,
    ) -> PResult<(ArchivedGenerationOrigin, archived_generation::RetainedPlan)> {
        let report = archived_generation::load(db, project, id, self.pool, self.store)?;
        let selected = self
            .selected
            .iter()
            .map(encode)
            .collect::<PResult<Vec<_>>>()?;
        let mut matching = Vec::new();
        for origin in &report.origins {
            let permitted = if self.independent_clocks {
                let anchor = self.selected.first().ok_or_else(mismatch)?;
                // Preserve full selected-origin correspondence, including model,
                // preprocessing, attempt and sample metadata beyond carrier keys.
                if origin.origin_manifest == anchor.origin_manifest
                    && origin.candidate_id == anchor.candidate_id
                    && origin.job_id == anchor.job_id
                    && encode(origin)? != encode(anchor)?
                {
                    return Err(mismatch());
                }
                origin.source_version == anchor.source_version
                    && origin.source_identity == anchor.source_identity
                    && origin.clock.is_some()
            } else {
                selected.contains(&encode(origin)?)
            };
            if permitted {
                matching.push(origin);
            }
        }
        if matching.len() != 1 {
            return Err(mismatch());
        }
        let origin = matching[0].clone();
        let plan = archived_generation::retained_plan(db, project, &origin, self.pool, self.store)?;
        Ok((origin, plan))
    }
    pub fn retime(
        &self,
        db: &Connection,
        project: &ProjectId,
        revision: RevisionId,
    ) -> PResult<ArchivedMotionRetimeReceipt> {
        archived_motion_retimes::load_verified(db, project, revision, self.pool, self.store)
    }
}
