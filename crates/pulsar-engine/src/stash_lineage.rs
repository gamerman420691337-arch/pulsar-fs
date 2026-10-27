//! Stash publication follows authored revision/candidate links, never byte equality.
use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
struct Derivation {
    anchored: bool,
    retimes: Vec<RevisionId>,
}
#[derive(PartialEq, Eq)]
struct GenerationClock {
    source: ArtifactIdentity,
    placement: Option<SourcePlacementSpec>,
    raw_frame_pair: Option<RawFramePair>,
}

// Source identity alone cannot establish the project/source correspondence of
// another job. Preserve exact declared domains and rates, not placement IDs or
// generation presets. Historical paths remain metadata and are never opened.
fn generation_clock(
    db: &Connection,
    project: &ProjectId,
    source: &SourceVersionId,
    job: &JobId,
    pool: &ResourcePool,
) -> PResult<GenerationClock> {
    let raw: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN manifest ELSE NULL END FROM jobs WHERE id=?1 AND project=?2 AND source=?3",
        params![job.as_str(),project.as_str(),source.as_str()], |row| row.get(0),
    ).optional().map_err(internal)?;
    let raw = raw.flatten().ok_or_else(mismatch)?;
    let _memory = pool.reserve_memory(
        (raw.len() as u64)
            .checked_mul(8)
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(mismatch)?,
    )?;
    let manifest: WorkerRequest = decode(&raw)?;
    manifest.validate()?;
    if manifest.job_id != *job
        || manifest.project_id != *project
        || manifest.source.source_version != *source
    {
        return Err(mismatch());
    }
    let WorkerOperation::Generate { settings, .. } = manifest.operation else {
        return Err(mismatch());
    };
    Ok(GenerationClock {
        source: manifest.source.identity,
        placement: settings.source_placement.map(|placement| placement.spec),
        raw_frame_pair: settings.raw_frame_pair,
    })
}

struct Walker<'a> {
    db: &'a Connection,
    project: &'a ProjectId,
    source: &'a SourceVersionId,
    anchor: &'a CandidateId,
    pool: Option<&'a ResourcePool>,
    memo: HashMap<String, Derivation>,
    active: HashSet<String>,
    archive: Option<&'a mut stash_archived_lineage::Links<'a>>,
    generation_clock: Option<GenerationClock>,
    verified_origins: HashSet<String>,
}
type RevisionLineageRow = (Option<i64>, Option<i64>, Option<String>, String, String);

fn mismatch() -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch,
        "Stash publication requires recorded derivation from the selected captured-source generation candidate; unrelated or unknown source lineage is refused")
}
#[cfg(test)]
pub(super) fn require_source_derivation(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    source: &SourceVersionId,
    candidate: &CandidateId,
    job: &JobId,
) -> PResult<()> {
    let bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM candidates c JOIN jobs j ON c.job=j.id WHERE c.id=?1 AND c.project=?2 AND j.id=?3 AND j.project=c.project AND j.source=?4)",
        params![candidate.as_str(),project.as_str(),job.as_str(),source.as_str()],|r|r.get(0)).map_err(internal)?;
    if !bound {
        return Err(mismatch());
    }
    let mut walker = Walker {
        db,
        project,
        source,
        anchor: candidate,
        pool: None,
        memo: HashMap::new(),
        active: HashSet::new(),
        archive: None,
        generation_clock: None,
        verified_origins: HashSet::new(),
    };
    if !walker.revision(rev_sql(revision)?, 0)?.anchored {
        return Err(mismatch());
    }
    Ok(())
}
pub(super) fn verified_source_retimings(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    source: &SourceVersionId,
    candidate: &CandidateId,
    job: &JobId,
    pool: &ResourcePool,
) -> PResult<Vec<RevisionId>> {
    let bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM candidates c JOIN jobs j ON c.job=j.id WHERE c.id=?1 AND c.project=?2 AND j.id=?3 AND j.project=c.project AND j.source=?4)",params![candidate.as_str(),project.as_str(),job.as_str(),source.as_str()],|r|r.get(0)).map_err(internal)?;
    if !bound {
        return Err(mismatch());
    }
    let origin_clock = generation_clock(db, project, source, job, pool)?;
    let mut walker = Walker {
        db,
        project,
        source,
        anchor: candidate,
        pool: Some(pool),
        memo: HashMap::new(),
        active: HashSet::new(),
        archive: None,
        generation_clock: Some(origin_clock),
        verified_origins: HashSet::new(),
    };
    let value = walker.revision(rev_sql(revision)?, 0)?;
    if !value.anchored {
        return Err(mismatch());
    }
    Ok(value.retimes)
}
/// Verify the current master through retained imported links and local edits.
/// This returns ancestry facts only; publication must recheck current authority,
/// source/destination binding and migrate originating clocks separately.
pub(super) fn verified_archived_master(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    report: &ArchivedGenerationReport,
    pool: &ResourcePool,
    store: &crate::project_package_import_storage::ImportStore,
) -> PResult<ArchivedMasterDerivation> {
    report.validate()?;
    if report.project_id != *project {
        return Err(mismatch());
    }
    let source = &report.origins.first().ok_or_else(mismatch)?.source_version;
    if report.origins.iter().any(|o| o.source_version != *source) {
        return Err(mismatch());
    }
    let head: i64 = db
        .query_row(
            "SELECT revision FROM projects WHERE id=?1",
            [project.as_str()],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if head != rev_sql(revision)? {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "archived ancestry inspection requires the current master revision",
        ));
    }
    let row:(Option<String>,Option<String>,Option<String>)=db.query_row("SELECT CASE WHEN typeof(p.program)='text' AND length(CAST(p.program AS BLOB))=71 THEN p.program ELSE NULL END,CASE WHEN typeof(r.program)='text' AND length(CAST(r.program AS BLOB))=71 THEN r.program ELSE NULL END,CASE WHEN typeof(o.descriptor)='text' AND length(CAST(o.descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN o.descriptor ELSE NULL END FROM projects p JOIN revisions r ON r.project=p.id AND r.revision=p.revision LEFT JOIN motion_objects o ON o.sha256=substr(r.program,8) WHERE p.id=?1",[project.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(internal)?;
    let (current, recorded, raw) = row;
    let descriptor: ProgramDescriptor = decode(&raw.ok_or_else(mismatch)?)?;
    descriptor.validate()?;
    if current != recorded
        || current.as_deref() != Some(format!("motion:{}", descriptor.sha256).as_str())
    {
        return Err(mismatch());
    }
    let _memory = pool.reserve_memory(
        descriptor
            .byte_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(mismatch)?,
    )?;
    motion_state::store_for(db)?.read(&descriptor)?;
    let mut links = stash_archived_lineage::Links::new(db, project, pool, store, &report.origins)?;
    let mut walker = Walker {
        db,
        project,
        source,
        anchor: &report.candidate_id,
        pool: Some(pool),
        memo: HashMap::new(),
        active: HashSet::new(),
        archive: Some(&mut links),
        generation_clock: None,
        verified_origins: HashSet::new(),
    };
    let derived = walker.revision(head, 0)?;
    if !derived.anchored {
        return Err(mismatch());
    }
    Ok(ArchivedMasterDerivation {
        revision,
        master: ArtifactIdentity {
            artifact_id: descriptor.artifact_id,
            sha256: descriptor.sha256,
            byte_len: descriptor.byte_len,
        },
        source_version: source.clone(),
        anchor_candidate: report.candidate_id.clone(),
        retiming_revisions: derived.retimes,
        publication_authority: false,
    })
}
/// Original generation bytes precede any rebased candidate's merged motion.
/// Follow only recorded, same-project/same-job rebase links under a hard bound.
pub(super) fn original_generation_candidate(
    db: &Connection,
    project: &ProjectId,
    candidate: &CandidateId,
    job: &JobId,
) -> PResult<CandidateId> {
    let mut current = candidate.clone();
    let mut seen = HashSet::new();
    for _ in 0..128 {
        if !seen.insert(current.to_string()) {
            return Err(mismatch());
        }
        let bound: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM candidates WHERE id=?1 AND project=?2 AND job=?3)",
                params![current.as_str(), project.as_str(), job.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if !bound {
            return Err(mismatch());
        }
        let parent:Option<String>=db.query_row("SELECT CASE WHEN typeof(parent)='text' AND length(CAST(parent AS BLOB)) BETWEEN 1 AND 128 THEN parent ELSE '' END FROM candidate_rebases WHERE candidate=?1",[current.as_str()],|r|r.get(0)).optional().map_err(internal)?;
        match parent {
            None => return Ok(current),
            Some(parent) => current = CandidateId::new(parent).map_err(internal)?,
        }
    }
    Err(ProtocolError::new(
        ErrorCode::ResourceExhausted,
        "originating generation rebase traversal limit reached",
    ))
}
impl Walker<'_> {
    fn enter(&mut self, key: &str, depth: usize) -> PResult<Option<Derivation>> {
        if let Some(value) = self.memo.get(key) {
            return Ok(Some(value.clone()));
        }
        if depth >= 128 || self.memo.len() + self.active.len() >= 2048 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Stash source lineage traversal limit reached",
            ));
        }
        if !self.active.insert(key.into()) {
            return Err(mismatch());
        }
        Ok(None)
    }
    fn finish(&mut self, key: String, value: Derivation) -> Derivation {
        self.active.remove(&key);
        self.memo.insert(key, value.clone());
        value
    }
    fn revision(&mut self, revision: i64, depth: usize) -> PResult<Derivation> {
        let key = format!("r:{revision}");
        if let Some(value) = self.enter(&key, depth)? {
            return Ok(value);
        }
        let row:Option<RevisionLineageRow>=self.db.query_row(
            "SELECT parent_revision,restored_from_revision,CASE WHEN candidate IS NULL THEN NULL WHEN typeof(candidate)='text' AND length(CAST(candidate AS BLOB)) BETWEEN 1 AND 128 THEN candidate ELSE '' END,CASE WHEN typeof(operation)='text' AND length(CAST(operation AS BLOB)) BETWEEN 1 AND 64 THEN operation ELSE '' END,CASE WHEN typeof(contributions)='text' AND length(CAST(contributions AS BLOB)) BETWEEN 1 AND 1048576 THEN contributions ELSE '' END FROM revision_lineage WHERE project=?1 AND revision=?2",
            params![self.project.as_str(),revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(internal)?;
        let (parent, restored, candidate, operation, contributions) = row.ok_or_else(mismatch)?;
        let _row_memory = self
            .pool
            .map(|p| {
                p.reserve_memory(
                    (contributions.len() as u64)
                        .checked_mul(8)
                        .and_then(|n| n.checked_add(8192))
                        .ok_or_else(mismatch)?,
                )
            })
            .transpose()?;
        if let Some(archive) = self.archive.as_ref() {
            archive.check_revision(
                self.db,
                self.project,
                stash_archived_lineage::RevisionRow {
                    revision,
                    parent,
                    restored,
                    candidate: candidate.as_deref(),
                    operation: &operation,
                    contributions: &contributions,
                },
            )?;
        }
        let value = match operation.as_str() {
            "create" if revision == 0 => Derivation::default(),
            "candidate_commit" => {
                self.candidate(candidate.as_deref().ok_or_else(mismatch)?, depth + 1)?
            }
            "candidate_merge" => {
                let mut value = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                let regions: Vec<(Axis, TimeRange)> = decode(&contributions)?;
                if regions.iter().any(|(axis, _)| *axis == Axis::Stroke) {
                    let contributed =
                        self.candidate(candidate.as_deref().ok_or_else(mismatch)?, depth + 1)?;
                    if value.retimes != contributed.retimes {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "mixed stroke clock histories require an explicit correspondence proof",
                        ));
                    }
                    value.anchored |= contributed.anchored;
                }
                value
            }
            "source_retime" => {
                let pool = self.pool.ok_or_else(mismatch)?;
                let id = RevisionId::new(u64::try_from(revision).map_err(internal)?);
                let native:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM motion_retime_receipts WHERE project=?1 AND revision=?2)",params![self.project.as_str(),revision],|r|r.get(0)).map_err(internal)?;
                let (base, stroke) = if native || self.archive.is_none() {
                    let receipt =
                        motion_retime_receipts::load_verified(self.db, self.project, id, pool)?;
                    (receipt.base_revision, receipt.axes.contains(&Axis::Stroke))
                } else {
                    let receipt = self.archive.as_ref().ok_or_else(mismatch)?.retime(
                        self.db,
                        self.project,
                        id,
                    )?;
                    (
                        receipt.base_revision,
                        receipt.origin_receipt.axes.contains(&Axis::Stroke),
                    )
                };
                if parent != Some(rev_sql(base)?) {
                    return Err(mismatch());
                }
                let mut value = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                if stroke {
                    if value.retimes.len() >= 128 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "retiming clock chain exceeds admission",
                        ));
                    }
                    value.retimes.push(id);
                }
                value
            }
            "undo" | "redo" => self.revision(restored.ok_or_else(mismatch)?, depth + 1)?,
            "edit"
            | "protection"
            | "source_alignment"
            | "review_disposition"
            | "review_interval_disposition" => {
                self.revision(parent.ok_or_else(mismatch)?, depth + 1)?
            }
            _ => return Err(mismatch()),
        };
        Ok(self.finish(key, value))
    }
    fn candidate(&mut self, candidate: &str, depth: usize) -> PResult<Derivation> {
        let key = format!("c:{candidate}");
        if let Some(value) = self.enter(&key, depth)? {
            return Ok(value);
        }
        let row:Option<(Option<String>,Option<String>)>=self.db.query_row(
            "SELECT CASE WHEN c.job IS NULL THEN NULL WHEN typeof(c.job)='text' AND length(CAST(c.job AS BLOB)) BETWEEN 1 AND 128 THEN c.job ELSE '' END,CASE WHEN j.source IS NULL THEN NULL WHEN typeof(j.source)='text' AND length(CAST(j.source AS BLOB)) BETWEEN 1 AND 128 THEN j.source ELSE '' END FROM candidates c LEFT JOIN jobs j ON c.job=j.id AND j.project=c.project WHERE c.id=?1 AND c.project=?2",
            params![candidate,self.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
        let (job, source) = row.ok_or_else(mismatch)?;
        let value = if job.is_some() {
            if self.archive.is_some() {
                return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"combining live and archived generation clocks requires an explicit correspondence proof"));
            }
            if source.as_deref() != Some(self.source.as_str()) {
                return Err(mismatch());
            }
            if let Some(expected) = &self.generation_clock {
                let job =
                    JobId::new(job.as_deref().ok_or_else(mismatch)?).map_err(|_| mismatch())?;
                if generation_clock(
                    self.db,
                    self.project,
                    self.source,
                    &job,
                    self.pool.ok_or_else(mismatch)?,
                )? != *expected
                {
                    return Err(ProtocolError::new(ErrorCode::DependencyMismatch,
                        "different generation clocks require an explicit piecewise correspondence proof"));
                }
                // Clock declarations are not proof of a contributor's producer
                // receipt or original motion. Authenticate every effective native
                // origin, including rebased aliases, before accepting the graph.
                let selected = CandidateId::new(candidate).map_err(|_| mismatch())?;
                let original =
                    original_generation_candidate(self.db, self.project, &selected, &job)?;
                if !self.verified_origins.contains(original.as_str()) {
                    let pool = self.pool.ok_or_else(mismatch)?;
                    let manifest = source_clock_origins::manifest(
                        self.db,
                        self.project,
                        self.source,
                        &job,
                        pool,
                    )?;
                    let origin = source_clock_origins::generation_origin(
                        self.db, &manifest, &selected, pool,
                    )?;
                    if let Some(plan) = &origin.verified.plan {
                        SourceClockMapper::new(
                            &plan.placement.spec,
                            Some(plan),
                            plan.samples.len(),
                        )
                        .map_err(|e| {
                            ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string())
                        })?;
                    }
                    self.verified_origins.insert(original.to_string());
                }
            }
            let parent:Option<String>=self.db.query_row("SELECT CASE WHEN typeof(parent)='text' AND length(CAST(parent AS BLOB)) BETWEEN 1 AND 128 THEN parent ELSE '' END FROM candidate_rebases WHERE candidate=?1",[candidate],|r|r.get(0)).optional().map_err(internal)?;
            let has_parent = parent.is_some();
            let inherited = parent
                .map(|parent| self.candidate(&parent, depth + 1))
                .transpose()?
                .unwrap_or_default();
            if has_parent && self.pool.is_some() {
                let base: i64 = self
                    .db
                    .query_row(
                        "SELECT base_revision FROM candidates WHERE id=?1 AND project=?2",
                        params![candidate, self.project.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                if self.revision(base, depth + 1)?.retimes != inherited.retimes {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "rebased generation has mixed clock histories without correspondence proof",
                    ));
                }
            }
            Derivation {
                anchored: candidate == self.anchor.as_str() || inherited.anchored,
                retimes: inherited.retimes,
            }
        } else if self.archive.is_some()
            && !self.archive.as_ref().is_some_and(|a| {
                a.graph
                    .root
                    .candidates
                    .iter()
                    .any(|c| c.candidate_id.as_str() == candidate)
            })
            && self
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM candidate_rebases WHERE candidate=?1)",
                    [candidate],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(internal)?
        {
            let parent:String=self.db.query_row("SELECT CASE WHEN typeof(parent)='text' AND length(CAST(parent AS BLOB)) BETWEEN 1 AND 128 THEN parent ELSE '' END FROM candidate_rebases WHERE candidate=?1",[candidate],|r|r.get(0)).map_err(internal)?;
            let inherited = self.candidate(&parent, depth + 1)?;
            let base: i64 = self
                .db
                .query_row(
                    "SELECT base_revision FROM candidates WHERE id=?1 AND project=?2",
                    params![candidate, self.project.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            let base = self.revision(base, depth + 1)?;
            if base.retimes != inherited.retimes {
                return Err(mismatch());
            }
            Derivation {
                anchored: inherited.anchored || base.anchored,
                retimes: inherited.retimes,
            }
        } else if let Some(link) = self
            .archive
            .as_mut()
            .map(|a| a.candidate(self.db, self.project, candidate))
            .transpose()?
            .flatten()
        {
            match link {
                stash_archived_lineage::CandidateLink::Generation => Derivation {
                    anchored: true,
                    retimes: vec![],
                },
                stash_archived_lineage::CandidateLink::Authored(base) => {
                    self.revision(rev_sql(base)?, depth + 1)?
                }
                stash_archived_lineage::CandidateLink::Rebased(parent, base) => {
                    let inherited = self.candidate(parent.as_str(), depth + 1)?;
                    let base = self.revision(rev_sql(base)?, depth + 1)?;
                    if inherited.retimes != base.retimes {
                        return Err(mismatch());
                    }
                    Derivation {
                        anchored: inherited.anchored || base.anchored,
                        retimes: inherited.retimes,
                    }
                }
            }
        } else {
            if let Some(pool) = self.pool {
                let candidate = CandidateId::new(candidate).map_err(|_| mismatch())?;
                let origin = edit_clock_origins::load(self.db, self.project, &candidate, pool)?;
                let base = origin.base_revision;
                // Verified motion/interval buffers need not accumulate along
                // the ancestry recursion. Future action composition consumes
                // these facts while their explicit reservation is live.
                drop(origin);
                let value = self.revision(rev_sql(base)?, depth + 1)?;
                return Ok(self.finish(key, value));
            }
            // Rebased edit candidates copy the original immutable receipt. Follow
            // its original upload base, not the newer candidate base revision.
            let bases = {
                let mut statement=self.db.prepare("SELECT DISTINCT u.base_revision FROM edit_candidate_lineage e JOIN edit_upload_receipts u ON u.receipt=e.receipt AND u.project=?2 WHERE e.candidate=?1 LIMIT 2").map_err(internal)?;
                let rows = statement
                    .query_map(params![candidate, self.project.as_str()], |r| {
                        r.get::<_, i64>(0)
                    })
                    .map_err(internal)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(internal)?
            };
            if bases.len() != 1 {
                return Err(mismatch());
            }
            self.revision(bases[0], depth + 1)?
        };
        Ok(self.finish(key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE candidates(id TEXT,project TEXT,job TEXT);CREATE TABLE jobs(id TEXT,project TEXT,source TEXT);
            CREATE TABLE revision_lineage(project TEXT,revision INTEGER,parent_revision INTEGER,restored_from_revision INTEGER,candidate TEXT,operation TEXT,contributions TEXT);
            CREATE TABLE candidate_rebases(candidate TEXT,parent TEXT);CREATE TABLE edit_candidate_lineage(candidate TEXT,receipt TEXT);CREATE TABLE edit_upload_receipts(project TEXT,base_revision INTEGER,receipt TEXT);
            INSERT INTO jobs VALUES('job','project','source'),('other-job','project','foreign-source');
            INSERT INTO candidates VALUES('root','project','job'),('foreign','project','other-job'),('imported','project',NULL),('edited','project',NULL),('rebased-edit','project',NULL);
            INSERT INTO edit_candidate_lineage VALUES('edited','immutable-receipt'),('rebased-edit','immutable-receipt');
            INSERT INTO edit_upload_receipts VALUES('project',1,'immutable-receipt');
            INSERT INTO revision_lineage VALUES('project',0,NULL,NULL,NULL,'create','[]'),('project',1,0,NULL,'root','candidate_commit','[]');").unwrap();
        db
    }
    fn revision(
        db: &Connection,
        id: i64,
        parent: i64,
        restored: Option<i64>,
        candidate: Option<&str>,
        operation: &str,
        stroke: bool,
    ) {
        let regions = if stroke {
            vec![(
                Axis::Stroke,
                TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000_000)).unwrap(),
            )]
        } else {
            vec![]
        };
        db.execute(
            "INSERT INTO revision_lineage VALUES('project',?1,?2,?3,?4,?5,?6)",
            params![
                id,
                parent,
                restored,
                candidate,
                operation,
                encode(&regions).unwrap()
            ],
        )
        .unwrap();
    }
    fn check(db: &Connection, revision: u64) -> PResult<()> {
        require_source_derivation(
            db,
            &ProjectId::new("project").unwrap(),
            RevisionId::new(revision),
            &SourceVersionId::new("source").unwrap(),
            &CandidateId::new("root").unwrap(),
            &JobId::new("job").unwrap(),
        )
    }
    #[test]
    fn edits_protection_and_immutable_edit_candidates_keep_the_original_source() {
        let db = fixture();
        check(&db, 1).unwrap();
        revision(&db, 2, 1, None, None, "edit", false);
        revision(&db, 3, 2, None, None, "protection", false);
        revision(&db, 4, 3, None, Some("edited"), "candidate_commit", false);
        revision(
            &db,
            5,
            4,
            None,
            Some("rebased-edit"),
            "candidate_commit",
            false,
        );
        for id in 2..=5 {
            check(&db, id).unwrap();
        }
        db.execute(
            "INSERT INTO candidates VALUES('rebased-generation','project','job')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO candidate_rebases VALUES('rebased-generation','root')",
            [],
        )
        .unwrap();
        revision(
            &db,
            6,
            5,
            None,
            Some("rebased-generation"),
            "candidate_commit",
            false,
        );
        check(&db, 6).unwrap();
    }
    #[test]
    fn merges_use_only_recorded_stroke_contributions_and_reject_foreign_sources() {
        let db = fixture();
        revision(&db, 2, 1, None, Some("edited"), "candidate_merge", true);
        check(&db, 2).unwrap();
        revision(&db, 3, 2, None, Some("foreign"), "candidate_merge", true);
        assert_eq!(
            check(&db, 3).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        revision(&db, 4, 2, None, Some("foreign"), "candidate_merge", false);
        check(&db, 4).unwrap();
        revision(&db, 5, 2, None, Some("imported"), "candidate_merge", true);
        assert!(check(&db, 5).is_err());
    }
    #[test]
    fn replacement_and_restoration_follow_effective_revision_instead_of_history_parent() {
        let db = fixture();
        revision(&db, 2, 1, None, Some("imported"), "candidate_commit", false);
        assert!(check(&db, 2).is_err());
        revision(&db, 3, 2, Some(1), None, "undo", false);
        check(&db, 3).unwrap();
        revision(&db, 4, 3, Some(2), None, "redo", false);
        assert!(check(&db, 4).is_err());
        revision(&db, 5, 1, Some(0), None, "undo", false);
        assert!(check(&db, 5).is_err());
    }
    #[test]
    fn missing_ambiguous_cross_project_and_cyclic_links_fail_closed() {
        let db = fixture();
        revision(&db, 2, 1, None, None, "legacy_unknown", false);
        assert!(check(&db, 2).is_err());
        revision(&db, 6, 1, None, None, "source_retime", false);
        assert!(check(&db, 6).is_err());
        revision(&db, 3, 3, None, None, "edit", false);
        assert!(check(&db, 3).is_err());
        db.execute(
            "INSERT INTO edit_upload_receipts VALUES('project',0,'immutable-receipt')",
            [],
        )
        .unwrap();
        revision(&db, 4, 1, None, Some("edited"), "candidate_commit", false);
        assert!(check(&db, 4).is_err());
        db.execute(
            "UPDATE candidates SET project='foreign-project' WHERE id='root'",
            [],
        )
        .unwrap();
        assert!(check(&db, 1).is_err());
    }
    #[test]
    fn deep_lineage_is_bounded_without_claiming_a_successful_derivation() {
        let db = fixture();
        for id in 2..=130 {
            revision(&db, id, id - 1, None, None, "edit", false);
        }
        assert_eq!(
            check(&db, 130).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn mixed_verified_clock_histories_refuse_stroke_merges_and_generation_rebases() {
        let db = fixture();
        db.execute_batch("ALTER TABLE candidates ADD COLUMN base_revision INTEGER;UPDATE candidates SET base_revision=0;INSERT INTO candidates VALUES('rebased-generation','project','job',1);INSERT INTO candidate_rebases VALUES('rebased-generation','root');").unwrap();
        revision(&db, 2, 1, None, Some("root"), "candidate_merge", true);
        revision(&db, 3, 1, None, Some("root"), "candidate_merge", false);
        let project = ProjectId::new("project").unwrap();
        let source = SourceVersionId::new("source").unwrap();
        let anchor = CandidateId::new("root").unwrap();
        let pool = ResourcePool::new(16 * 1024 * 1024, 1);
        let mut walker = Walker {
            db: &db,
            project: &project,
            source: &source,
            anchor: &anchor,
            pool: Some(&pool),
            memo: HashMap::new(),
            active: HashSet::new(),
            archive: None,
            generation_clock: None,
            verified_origins: HashSet::new(),
        };
        // Seed the independently verified prefix; these branches test composition,
        // while receipt loading/recomputation has authenticated engine coverage.
        walker.memo.insert(
            "r:1".into(),
            Derivation {
                anchored: true,
                retimes: vec![RevisionId::new(1)],
            },
        );
        assert_eq!(
            walker.revision(2, 0).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert_eq!(
            walker.revision(3, 0).unwrap().retimes,
            vec![RevisionId::new(1)]
        );
        assert_eq!(
            walker.candidate("rebased-generation", 0).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }
    #[test]
    fn live_generation_clock_requires_bounded_typed_origin_metadata() {
        let db = fixture();
        db.execute("ALTER TABLE jobs ADD COLUMN manifest TEXT", [])
            .unwrap();
        let project = ProjectId::new("project").unwrap();
        let source = SourceVersionId::new("source").unwrap();
        let job = JobId::new("job").unwrap();
        let pool = ResourcePool::new(16 * 1024 * 1024, 1);
        assert!(generation_clock(&db, &project, &source, &job, &pool).is_err());
        for raw in ["{}".to_string(), " ".repeat(1048577)] {
            db.execute("UPDATE jobs SET manifest=?1 WHERE id='job'", [raw])
                .unwrap();
            assert!(generation_clock(&db, &project, &source, &job, &pool).is_err());
        }
        assert!(generation_clock(
            &db,
            &project,
            &source,
            &JobId::new("missing").unwrap(),
            &pool
        )
        .is_err());
    }
    #[test]
    fn originating_bytes_follow_only_bounded_same_job_rebase_links() {
        let db = fixture();
        let p = ProjectId::new("project").unwrap();
        let j = JobId::new("job").unwrap();
        let c = CandidateId::new("rebased-generation").unwrap();
        db.execute(
            "INSERT INTO candidates VALUES('rebased-generation','project','job')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO candidate_rebases VALUES('rebased-generation','root')",
            [],
        )
        .unwrap();
        assert_eq!(
            original_generation_candidate(&db, &p, &c, &j)
                .unwrap()
                .as_str(),
            "root"
        );
        db.execute(
            "UPDATE candidate_rebases SET parent='foreign' WHERE candidate='rebased-generation'",
            [],
        )
        .unwrap();
        assert!(original_generation_candidate(&db, &p, &c, &j).is_err());
        db.execute("UPDATE candidate_rebases SET parent='rebased-generation' WHERE candidate='rebased-generation'",[]).unwrap();
        assert!(original_generation_candidate(&db, &p, &c, &j).is_err());
        for n in 0..128 {
            db.execute(
                "INSERT INTO candidates VALUES(?1,'project','job')",
                [format!("chain-{n}")],
            )
            .unwrap();
            db.execute(
                "INSERT INTO candidate_rebases VALUES(?1,?2)",
                params![
                    format!("chain-{n}"),
                    if n == 127 {
                        "root".into()
                    } else {
                        format!("chain-{}", n + 1)
                    }
                ],
            )
            .unwrap();
        }
        assert_eq!(
            original_generation_candidate(&db, &p, &CandidateId::new("chain-0").unwrap(), &j)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
    }
}
