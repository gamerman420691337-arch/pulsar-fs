//! Exact effective action-clock derivation through native or retained archival origins.
use super::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};

const MAX_ENTRIES: usize = 1_000_000;
const MAX_NODES: usize = 2048;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClockCarrierOrigin {
    /// Historical namespace only. Native proofs omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_manifest: Option<PackageObjectRef>,
    pub original_candidate: CandidateId,
    pub original_job: JobId,
    pub original_program: ArtifactIdentity,
    pub original_receipt: ArtifactIdentity,
    pub placement: SourcePlacement,
    #[serde(deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 128>")]
    pub retimings: Vec<RevisionId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActionClockOrigin {
    pub project_time: ProjectTime,
    pub carrier: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeActionAncestry {
    #[serde(deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 2048>")]
    pub carriers: Vec<ClockCarrierOrigin>,
    /// One selection per mapping, in exact elapsed-source order.
    #[serde(deserialize_with = "crate::source_clock_receipts::bounded::<_, _, 1000000>")]
    pub selections: Vec<ActionClockOrigin>,
}
struct Carrier {
    origin: ClockCarrierOrigin,
    mapper: SourceClockMapper,
    entries: usize,
    _memory: Reservation,
}
struct Derived {
    descriptor: ProgramDescriptor,
    track: Option<MotionTrack>,
    labels: Vec<usize>,
    _memory: Reservation,
}
struct Walker<'a> {
    db: &'a Connection,
    project: &'a ProjectId,
    source: &'a SourceVersionId,
    pool: &'a ResourcePool,
    memo: HashMap<String, Arc<Derived>>,
    active: HashSet<String>,
    carriers: Vec<Carrier>,
    archive: Option<stash_archived_lineage::Links<'a>>,
}
type ActionRevisionRow = (
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn mismatch() -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch,
        "native action clock lacks exact generation, edit, merge, restoration or retiming correspondence")
}
fn exhausted() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::ResourceExhausted,
        "native action-clock ancestry exceeds admission",
    )
}
fn descriptor(db: &Connection, reference: &str) -> PResult<ProgramDescriptor> {
    let digest = reference
        .strip_prefix("motion:")
        .filter(|s| s.len() == 64)
        .ok_or_else(mismatch)?;
    let raw:Option<Option<String>>=db.query_row("SELECT CASE WHEN typeof(descriptor)='text' AND length(CAST(descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN descriptor ELSE NULL END FROM motion_objects WHERE sha256=?1",[digest],|r|r.get(0)).optional().map_err(internal)?;
    let descriptor: ProgramDescriptor = decode(&raw.flatten().ok_or_else(mismatch)?)?;
    descriptor.validate().map_err(|_| mismatch())?;
    if descriptor.sha256 != digest {
        return Err(mismatch());
    }
    Ok(descriptor)
}
impl Walker<'_> {
    fn enter(&mut self, key: &str, depth: usize) -> PResult<Option<Arc<Derived>>> {
        if let Some(value) = self.memo.get(key) {
            return Ok(Some(value.clone()));
        }
        if depth >= 128 || self.memo.len() + self.active.len() >= MAX_NODES {
            return Err(exhausted());
        }
        if !self.active.insert(key.into()) {
            return Err(mismatch());
        }
        Ok(None)
    }
    fn finish(&mut self, key: String, value: Derived) -> Arc<Derived> {
        self.active.remove(&key);
        let value = Arc::new(value);
        self.memo.insert(key, value.clone());
        value
    }
    fn load(&self, reference: Option<String>) -> PResult<Derived> {
        let _metadata = self.pool.reserve_memory(256 * 1024)?;
        let descriptor = descriptor(self.db, &reference.ok_or_else(mismatch)?)?;
        let memory = self.pool.reserve_memory(
            descriptor
                .byte_len
                .checked_mul(8)
                .and_then(|n| n.checked_add(16 * 1024 * 1024))
                .ok_or_else(exhausted)?,
        )?;
        let program = motion_state::store_for(self.db)?
            .read(&descriptor)
            .map_err(|_| mismatch())?;
        let track = program.track(Axis::Stroke).cloned();
        if track
            .as_ref()
            .is_some_and(|t| t.actions().len() > MAX_ENTRIES)
        {
            return Err(exhausted());
        }
        Ok(Derived {
            descriptor,
            track,
            labels: vec![],
            _memory: memory,
        })
    }
    fn candidate(&mut self, id: &CandidateId, depth: usize) -> PResult<Arc<Derived>> {
        let key = format!("c:{id}");
        if let Some(value) = self.enter(&key, depth)? {
            return Ok(value);
        }
        let row:Option<(Option<String>,Option<String>)>=self.db.query_row("SELECT CASE WHEN job IS NULL THEN NULL WHEN typeof(job)='text' AND length(CAST(job AS BLOB)) BETWEEN 1 AND 128 THEN job ELSE '' END,CASE WHEN typeof(program)='text' AND length(CAST(program AS BLOB))=71 THEN program ELSE NULL END FROM candidates WHERE id=?1 AND project=?2",params![id.as_str(),self.project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
        let (job, reference) = row.ok_or_else(mismatch)?;
        let mut value = self.load(reference)?;
        let parent:Option<String>=self.db.query_row("SELECT CASE WHEN typeof(parent)='text' AND length(CAST(parent AS BLOB)) BETWEEN 1 AND 128 THEN parent ELSE '' END FROM candidate_rebases WHERE candidate=?1",[id.as_str()],|r|r.get(0)).optional().map_err(internal)?;
        let archived = if let Some(links) = self.archive.as_mut() {
            links.candidate(self.db, self.project, id.as_str())?
        } else {
            None
        };
        if let Some(link) = archived {
            if job.is_some() || parent.is_some() {
                return Err(mismatch());
            }
            match link {
                stash_archived_lineage::CandidateLink::Generation => {
                    let (origin, verified) = self
                        .archive
                        .as_ref()
                        .ok_or_else(mismatch)?
                        .generation_plan(self.db, self.project, id)?;
                    if origin.program.sha256 != value.descriptor.sha256
                        || origin.program.byte_len != value.descriptor.byte_len
                    {
                        return Err(mismatch());
                    }
                    if self.carriers.len() >= MAX_NODES {
                        return Err(exhausted());
                    }
                    let entries = verified.plan.samples.len();
                    let memory = self.pool.reserve_memory(
                        (entries as u64)
                            .checked_mul(512)
                            .and_then(|n| n.checked_add(8192))
                            .ok_or_else(exhausted)?,
                    )?;
                    let mapper = SourceClockMapper::new(
                        &verified.plan.placement.spec,
                        Some(&verified.plan),
                        MAX_ENTRIES,
                    )
                    .map_err(|_| mismatch())?;
                    let carrier = self.carriers.len();
                    self.carriers.push(Carrier {
                        origin: ClockCarrierOrigin {
                            origin_manifest: Some(origin.origin_manifest),
                            original_candidate: origin.candidate_id,
                            original_job: origin.job_id,
                            original_program: origin.program,
                            original_receipt: verified.receipt.clone(),
                            placement: verified.plan.placement.clone(),
                            retimings: vec![],
                        },
                        mapper,
                        entries,
                        _memory: memory,
                    });
                    value.labels =
                        vec![carrier; value.track.as_ref().map_or(0, |t| t.actions().len())];
                }
                stash_archived_lineage::CandidateLink::Authored(base) => {
                    let inherited = self.revision(rev_sql(base)?, depth + 1)?;
                    value.labels = edited_labels(&inherited, value.track.as_ref())?;
                }
                stash_archived_lineage::CandidateLink::Rebased(parent, _) => {
                    let inherited = self.candidate(&parent, depth + 1)?;
                    if value.descriptor != inherited.descriptor {
                        return Err(mismatch());
                    }
                    value.labels = inherited.labels.clone();
                }
            }
        } else if let Some(parent) = parent {
            let parent = CandidateId::new(parent).map_err(|_| mismatch())?;
            let bound:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM candidates WHERE id=?1 AND project=?2 AND job IS ?3)",params![parent.as_str(),self.project.as_str(),job],|r|r.get(0)).map_err(internal)?;
            if !bound {
                return Err(mismatch());
            }
            let inherited = self.candidate(&parent, depth + 1)?;
            if value.descriptor != inherited.descriptor {
                return Err(mismatch());
            }
            value.labels = inherited.labels.clone();
        } else if let Some(job) = job {
            let job = JobId::new(job).map_err(|_| mismatch())?;
            let manifest = source_clock_origins::manifest(
                self.db,
                self.project,
                self.source,
                &job,
                self.pool,
            )?;
            let origin = source_clock_origins::clocks(self.db, &manifest, id, self.pool)?;
            if origin.original_candidate != *id
                || origin.program.sha256 != value.descriptor.sha256
                || origin.program.byte_len != value.descriptor.byte_len
            {
                return Err(mismatch());
            }
            if self.carriers.len() >= MAX_NODES {
                return Err(exhausted());
            }
            let entries = origin.verified.plan.samples.len();
            let memory = self.pool.reserve_memory(
                (entries as u64)
                    .checked_mul(512)
                    .and_then(|n| n.checked_add(8192))
                    .ok_or_else(exhausted)?,
            )?;
            let mapper = SourceClockMapper::new(
                &origin.verified.plan.placement.spec,
                Some(&origin.verified.plan),
                MAX_ENTRIES,
            )
            .map_err(|_| mismatch())?;
            let carrier = self.carriers.len();
            self.carriers.push(Carrier {
                origin: ClockCarrierOrigin {
                    origin_manifest: None,
                    original_candidate: id.clone(),
                    original_job: job,
                    original_program: origin.program.clone(),
                    original_receipt: origin.verified.identity.clone(),
                    placement: origin.verified.plan.placement.clone(),
                    retimings: vec![],
                },
                mapper,
                entries,
                _memory: memory,
            });
            value.labels = vec![carrier; value.track.as_ref().map_or(0, |t| t.actions().len())];
        } else {
            let edit = edit_clock_origins::load(self.db, self.project, id, self.pool)?;
            let base = edit.base_revision;
            if value.track.as_ref() != edit.reconciled.program.track(Axis::Stroke) {
                return Err(mismatch());
            }
            drop(edit);
            let inherited = self.revision(rev_sql(base)?, depth + 1)?;
            value.labels = edited_labels(&inherited, value.track.as_ref())?;
        }
        Ok(self.finish(key, value))
    }
    fn revision(&mut self, id: i64, depth: usize) -> PResult<Arc<Derived>> {
        let key = format!("r:{id}");
        if let Some(value) = self.enter(&key, depth)? {
            return Ok(value);
        }
        let row:Option<ActionRevisionRow>=self.db.query_row("SELECT l.parent_revision,l.restored_from_revision,CASE WHEN l.candidate IS NULL THEN NULL WHEN typeof(l.candidate)='text' AND length(CAST(l.candidate AS BLOB)) BETWEEN 1 AND 128 THEN l.candidate ELSE '' END,CASE WHEN typeof(l.operation)='text' AND length(CAST(l.operation AS BLOB)) BETWEEN 1 AND 64 THEN l.operation ELSE NULL END,CASE WHEN typeof(l.contributions)='text' AND length(CAST(l.contributions AS BLOB)) BETWEEN 1 AND 1048576 THEN l.contributions ELSE NULL END,CASE WHEN typeof(r.program)='text' AND length(CAST(r.program AS BLOB))=71 THEN r.program ELSE NULL END FROM revision_lineage l JOIN revisions r ON r.project=l.project AND r.revision=l.revision WHERE l.project=?1 AND l.revision=?2",params![self.project.as_str(),id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(internal)?;
        let (parent, restored, candidate, operation, contributions, reference) =
            row.ok_or_else(mismatch)?;
        let operation = operation.ok_or_else(mismatch)?;
        let contributions = contributions.ok_or_else(mismatch)?;
        let _metadata = self.pool.reserve_memory(
            (contributions.len() as u64)
                .checked_mul(8)
                .and_then(|n| n.checked_add(8192))
                .ok_or_else(exhausted)?,
        )?;
        if let Some(archive) = self.archive.as_ref() {
            archive.check_revision(
                self.db,
                self.project,
                stash_archived_lineage::RevisionRow {
                    revision: id,
                    parent,
                    restored,
                    candidate: candidate.as_deref(),
                    operation: &operation,
                    contributions: &contributions,
                },
            )?;
        }
        let mut value = self.load(reference)?;
        match operation.as_str() {
            "create" if id == 0 => {
                if value
                    .track
                    .as_ref()
                    .is_some_and(|t| !t.actions().is_empty())
                {
                    return Err(mismatch());
                }
            }
            "candidate_commit" => {
                let candidate =
                    CandidateId::new(candidate.ok_or_else(mismatch)?).map_err(|_| mismatch())?;
                let inherited = self.candidate(&candidate, depth + 1)?;
                if value.descriptor != inherited.descriptor {
                    return Err(mismatch());
                }
                value.labels = inherited.labels.clone();
            }
            "candidate_merge" => {
                let base = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                let regions: Vec<(Axis, TimeRange)> = decode(&contributions)?;
                if regions.len() > 1024 {
                    return Err(exhausted());
                }
                if regions.iter().any(|(axis, _)| *axis == Axis::Stroke) {
                    let candidate = CandidateId::new(candidate.ok_or_else(mismatch)?)
                        .map_err(|_| mismatch())?;
                    let incoming = self.candidate(&candidate, depth + 1)?;
                    value.labels = merged_labels(&base, &incoming, value.track.as_ref(), &regions)?;
                } else {
                    if value.track != base.track {
                        return Err(mismatch());
                    }
                    value.labels = base.labels.clone();
                }
            }
            "undo" | "redo" => {
                let inherited = self.revision(restored.ok_or_else(mismatch)?, depth + 1)?;
                if value.descriptor != inherited.descriptor {
                    return Err(mismatch());
                }
                value.labels = inherited.labels.clone();
            }
            "edit" => {
                let base = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                value.labels = edited_labels(&base, value.track.as_ref())?;
            }
            "source_retime" => {
                let revision = RevisionId::new(u64::try_from(id).map_err(|_| mismatch())?);
                let local:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM motion_retime_receipts WHERE project=?1 AND revision=?2)",
                    params![self.project.as_str(),id],|r|r.get(0)).map_err(internal)?;
                let (base_revision, axes, from, to) = if local || self.archive.is_none() {
                    let receipt = motion_retime_receipts::load_verified(
                        self.db,
                        self.project,
                        revision,
                        self.pool,
                    )?;
                    (
                        receipt.base_revision,
                        receipt.axes,
                        receipt.from,
                        receipt.to,
                    )
                } else {
                    let receipt = self.archive.as_ref().ok_or_else(mismatch)?.retime(
                        self.db,
                        self.project,
                        revision,
                    )?;
                    (
                        receipt.base_revision,
                        receipt.origin_receipt.axes,
                        receipt.from,
                        receipt.to,
                    )
                };
                if parent != Some(rev_sql(base_revision)?) {
                    return Err(mismatch());
                }
                let base = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                if axes.contains(&Axis::Stroke) {
                    value.labels = self.retimed_labels(
                        &base,
                        value.track.as_ref(),
                        revision,
                        &from.spec,
                        &to.spec,
                    )?;
                } else {
                    if value.track != base.track {
                        return Err(mismatch());
                    }
                    value.labels = base.labels.clone();
                }
            }
            "protection"
            | "source_alignment"
            | "review_disposition"
            | "review_interval_disposition" => {
                let base = self.revision(parent.ok_or_else(mismatch)?, depth + 1)?;
                if value.track != base.track {
                    return Err(mismatch());
                }
                value.labels = base.labels.clone();
            }
            _ => return Err(mismatch()),
        }
        if value.track.as_ref().map_or(0, |t| t.actions().len()) != value.labels.len() {
            return Err(mismatch());
        }
        Ok(self.finish(key, value))
    }
    fn retimed_labels(
        &mut self,
        base: &Derived,
        after: Option<&MotionTrack>,
        revision: RevisionId,
        from: &SourcePlacementSpec,
        to: &SourcePlacementSpec,
    ) -> PResult<Vec<usize>> {
        let before = base.track.as_ref().ok_or_else(mismatch)?;
        let after = after.ok_or_else(mismatch)?;
        let mut replacements = BTreeMap::new();
        for carrier in base.labels.iter().copied().collect::<BTreeSet<_>>() {
            if self.carriers.len() >= MAX_NODES {
                return Err(exhausted());
            }
            let prior = self.carriers.get(carrier).ok_or_else(mismatch)?;
            if prior.origin.retimings.len() >= 128 {
                return Err(exhausted());
            }
            let memory = self.pool.reserve_memory(
                (prior.entries as u64)
                    .checked_add(before.actions().len() as u64)
                    .and_then(|n| n.checked_mul(512))
                    .and_then(|n| n.checked_add(8192))
                    .ok_or_else(exhausted)?,
            )?;
            let mut mapper = prior.mapper.clone();
            mapper
                .apply_retime(from, to, before)
                .map_err(|_| mismatch())?;
            let mut origin = prior.origin.clone();
            origin.retimings.push(revision);
            replacements.insert(carrier, self.carriers.len());
            self.carriers.push(Carrier {
                origin,
                mapper,
                entries: before.actions().len(),
                _memory: memory,
            });
        }
        let mut labels = Vec::with_capacity(base.labels.len());
        for (action, label) in before.actions().iter().zip(&base.labels) {
            let source = from
                .source_at_project(action.time())
                .map_err(|_| mismatch())?;
            let time = to
                .project_at_source(source)
                .and_then(|t| t.quantize_nanoseconds())
                .map_err(|_| mismatch())?
                .time;
            labels.push((
                time,
                *replacements.get(label).ok_or_else(mismatch)?,
                action.position(),
                action.evidence(),
            ));
        }
        labels.sort_by_key(|r| r.0);
        if labels.len() != after.actions().len()
            || labels
                .iter()
                .zip(after.actions())
                .any(|(r, a)| r.0 != a.time() || r.2 != a.position() || r.3 != a.evidence())
        {
            return Err(mismatch());
        }
        Ok(labels.into_iter().map(|r| r.1).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(times: &[i64]) -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            times
                .iter()
                .map(|t| {
                    MotionAction::new(
                        ProjectTime::from_nanos(*t),
                        NormalizedPosition::new(0.2).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()])
        .unwrap()
    }
    fn derived(program: &MotionProgram, labels: Vec<usize>, pool: &ResourcePool) -> Derived {
        let root = tempfile::tempdir().unwrap();
        let store =
            crate::motion_artifacts::ProgramStore::new(&root.path().join("objects")).unwrap();
        Derived {
            descriptor: store.publish(program).unwrap(),
            track: program.track(Axis::Stroke).cloned(),
            labels,
            _memory: pool.reserve_memory(8192).unwrap(),
        }
    }
    #[test]
    fn retained_merge_mask_matches_exact_production_inputs_even_when_values_are_equal() {
        let pool = ResourcePool::new(128 * 1024 * 1024, 1);
        let current = track(&[0, 100, 125, 200, 300]);
        let candidate = track(&[0, 50, 100, 150, 175, 200, 300]);
        let range =
            TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap();
        let merged = merge_candidate_with_action_origins(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range),
            &[],
        )
        .unwrap();
        let regions = candidate_merge_writable_regions(
            &current,
            &candidate,
            &[Axis::Stroke],
            Some(range),
            &[],
        )
        .unwrap();
        let base = derived(&current, vec![0; 5], &pool);
        let incoming = derived(&candidate, vec![1; 7], &pool);
        let labels = merged_labels(
            &base,
            &incoming,
            merged.program.track(Axis::Stroke),
            &regions,
        )
        .unwrap();
        assert_eq!(
            labels,
            merged.action_origins[&Axis::Stroke]
                .iter()
                .map(|l| match l.input {
                    CandidateMergeInput::Current => 0,
                    CandidateMergeInput::Candidate => 1,
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(labels, vec![0, 0, 1, 1, 0, 0]);
        let wrong = track(&[0, 100, 125, 200, 300]);
        assert!(merged_labels(&base, &incoming, wrong.track(Axis::Stroke), &regions).is_err());
        let overlapping = [(Axis::Stroke, range), (Axis::Stroke, range)];
        assert!(merged_labels(
            &base,
            &incoming,
            merged.program.track(Axis::Stroke),
            &overlapping
        )
        .is_err());
    }
    #[test]
    fn authored_new_times_require_one_neighbor_carrier_or_a_uniform_base() {
        let pool = ResourcePool::new(128 * 1024 * 1024, 1);
        let base = derived(&track(&[0, 100, 200, 300]), vec![0, 0, 1, 1], &pool);
        assert_eq!(
            edited_labels(
                &base,
                track(&[0, 50, 100, 200, 250, 300]).track(Axis::Stroke)
            )
            .unwrap(),
            vec![0, 0, 0, 1, 1, 1]
        );
        for times in [vec![150], vec![400]] {
            assert!(edited_labels(&base, track(&times).track(Axis::Stroke)).is_err());
        }
        let uniform = derived(&track(&[0, 100, 200]), vec![0; 3], &pool);
        assert_eq!(
            edited_labels(&uniform, track(&[50, 250]).track(Axis::Stroke)).unwrap(),
            vec![0, 0]
        );
    }
    #[test]
    fn effective_replacement_and_undo_redo_follow_stored_objects_not_history_parent() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("projects.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE revisions(project TEXT,revision INTEGER,program TEXT);CREATE TABLE motion_objects(sha256 TEXT PRIMARY KEY,descriptor TEXT);CREATE TABLE revision_lineage(project TEXT,revision INTEGER,parent_revision INTEGER,restored_from_revision INTEGER,candidate TEXT,operation TEXT,contributions TEXT);").unwrap();
        let pool = ResourcePool::new(256 * 1024 * 1024, 1);
        let project = ProjectId::new("project").unwrap();
        let source = SourceVersionId::new("source").unwrap();
        let first = derived(&track(&[0, 100, 200]), vec![0; 3], &pool);
        let second = derived(&track(&[0, 150, 300]), vec![1; 3], &pool);
        let first_ref = motion_state::register(&db, &first.descriptor).unwrap();
        let second_ref = motion_state::register(&db, &second.descriptor).unwrap();
        // Seed independently verified leaves. This fixture exercises only graph
        // composition; actual producer/receipt loading has native CLI coverage.
        let store = motion_state::store_for(&db).unwrap();
        assert_eq!(
            store.publish(&track(&[0, 100, 200])).unwrap(),
            first.descriptor
        );
        assert_eq!(
            store.publish(&track(&[0, 150, 300])).unwrap(),
            second.descriptor
        );
        let mut walker = Walker {
            db: &db,
            project: &project,
            source: &source,
            pool: &pool,
            memo: HashMap::new(),
            active: HashSet::new(),
            carriers: vec![],
            archive: None,
        };
        walker.memo.insert("c:first".into(), Arc::new(first));
        walker.memo.insert("c:second".into(), Arc::new(second));
        for (id, parent, restored, candidate, op, reference) in [
            (1, 0, None, Some("first"), "candidate_commit", &first_ref),
            (2, 1, None, Some("second"), "candidate_commit", &second_ref),
            (3, 2, Some(1), None, "undo", &first_ref),
            (4, 3, Some(2), None, "redo", &second_ref),
        ] {
            db.execute(
                "INSERT INTO revisions VALUES('project',?1,?2)",
                params![id, reference],
            )
            .unwrap();
            db.execute(
                "INSERT INTO revision_lineage VALUES('project',?1,?2,?3,?4,?5,'[]')",
                params![id, parent, restored, candidate, op],
            )
            .unwrap();
        }
        assert_eq!(walker.revision(2, 0).unwrap().labels, vec![1; 3]);
        assert_eq!(walker.revision(3, 0).unwrap().labels, vec![0; 3]);
        assert_eq!(walker.revision(4, 0).unwrap().labels, vec![1; 3]);
        db.execute(
            "INSERT INTO revisions VALUES('project',5,?1)",
            [&second_ref],
        )
        .unwrap();
        db.execute(
            "INSERT INTO revision_lineage VALUES('project',5,4,1,NULL,'undo','[]')",
            [],
        )
        .unwrap();
        assert!(walker.revision(5, 0).is_err());
    }
    #[test]
    fn recursive_and_missing_graph_nodes_refuse_under_the_same_admission() {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("projects.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE revisions(project TEXT,revision INTEGER,program TEXT);CREATE TABLE motion_objects(sha256 TEXT PRIMARY KEY,descriptor TEXT);CREATE TABLE revision_lineage(project TEXT,revision INTEGER,parent_revision INTEGER,restored_from_revision INTEGER,candidate TEXT,operation TEXT,contributions TEXT);").unwrap();
        let pool = ResourcePool::new(128 * 1024 * 1024, 1);
        let project = ProjectId::new("project").unwrap();
        let source = SourceVersionId::new("source").unwrap();
        let reference = motion_state::save(&db, &track(&[0, 100])).unwrap();
        db.execute("INSERT INTO revisions VALUES('project',1,?1)", [reference])
            .unwrap();
        db.execute(
            "INSERT INTO revision_lineage VALUES('project',1,1,NULL,NULL,'edit','[]')",
            [],
        )
        .unwrap();
        let mut walker = Walker {
            db: &db,
            project: &project,
            source: &source,
            pool: &pool,
            memo: HashMap::new(),
            active: HashSet::new(),
            carriers: vec![],
            archive: None,
        };
        assert_eq!(
            walker.revision(1, 0).err().unwrap().code,
            ErrorCode::DependencyMismatch
        );
        assert_eq!(
            walker.revision(2, 128).err().unwrap().code,
            ErrorCode::ResourceExhausted
        );
    }
}

fn edited_labels(base: &Derived, after: Option<&MotionTrack>) -> PResult<Vec<usize>> {
    let Some(after) = after else {
        return Ok(vec![]);
    };
    if after.actions().is_empty() {
        return Ok(vec![]);
    }
    let before = base.track.as_ref().ok_or_else(mismatch)?;
    if before.actions().len() != base.labels.len() || base.labels.is_empty() {
        return Err(mismatch());
    }
    let uniform = base.labels.iter().all(|l| *l == base.labels[0]);
    after.actions().iter().map(|a| {
        match before.actions().binary_search_by_key(&a.time(),|a|a.time()) {
            Ok(index)=>Ok(base.labels[index]),
            Err(index) if index>0 && index<base.labels.len() && base.labels[index-1]==base.labels[index]=>Ok(base.labels[index]),
            Err(_) if uniform=>Ok(base.labels[0]),
            _=>Err(ProtocolError::new(ErrorCode::DependencyMismatch,"new authored action crosses different source-clock carriers without explicit correspondence")),
        }
    }).collect()
}

fn merged_labels(
    base: &Derived,
    incoming: &Derived,
    after: Option<&MotionTrack>,
    regions: &[(Axis, TimeRange)],
) -> PResult<Vec<usize>> {
    let Some(after) = after else {
        return Ok(vec![]);
    };
    let mut stroke: Vec<_> = regions
        .iter()
        .filter(|(axis, _)| *axis == Axis::Stroke)
        .map(|(_, range)| *range)
        .collect();
    stroke.sort_by_key(|r| r.start());
    if stroke.iter().any(|r| r.start() < ProjectTime::ZERO)
        || stroke.windows(2).any(|p| p[0].end() > p[1].start())
    {
        return Err(mismatch());
    }
    after
        .actions()
        .iter()
        .map(|action| {
            let index = stroke.partition_point(|r| r.end() <= action.time());
            let origin = if stroke.get(index).is_some_and(|r| r.contains(action.time())) {
                incoming
            } else {
                base
            };
            let track = origin.track.as_ref().ok_or_else(mismatch)?;
            let index = track
                .actions()
                .binary_search_by_key(&action.time(), |a| a.time())
                .map_err(|_| mismatch())?;
            // The recorded mask selects the source; equality only verifies that
            // exact input index. Equal-valued competing inputs remain distinct.
            if track.actions().get(index) != Some(action) {
                return Err(mismatch());
            }
            origin.labels.get(index).copied().ok_or_else(mismatch)
        })
        .collect()
}

pub(super) struct VerifiedActionMapping {
    pub motion: SourceClockMotion,
    pub ancestry: NativeActionAncestry,
    _memory: Reservation,
}
impl VerifiedActionMapping {
    pub(super) fn into_parts(self) -> (SourceClockMotion, NativeActionAncestry, Reservation) {
        (self.motion, self.ancestry, self._memory)
    }
}

pub(super) fn map_native(
    db: &Connection,
    project: &ProjectId,
    source: &SourceVersionId,
    revision: RevisionId,
    current: &MotionTrack,
    pool: &ResourcePool,
) -> PResult<VerifiedActionMapping> {
    let mut walker = Walker {
        db,
        project,
        source,
        pool,
        memo: HashMap::new(),
        active: HashSet::new(),
        carriers: vec![],
        archive: None,
    };
    map_effective(&mut walker, revision, current)
}

/// Derive exact actions from retained imported identities. This does not relax
/// the selected-producer ancestry guard or authorize historical execution.
pub(super) fn map_archived(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    selected: &ArchivedGenerationOrigin,
    current: &MotionTrack,
    pool: &ResourcePool,
    store: &crate::project_package_import_storage::ImportStore,
) -> PResult<VerifiedActionMapping> {
    let source = &selected.source_version;
    let source_bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2 AND sha256=?3 AND bytes=?4)",
        params![project.as_str(),source.as_str(),selected.source_identity.sha256,
            i64::try_from(selected.source_identity.byte_len).map_err(internal)?],|r|r.get(0)).map_err(internal)?;
    if !source_bound {
        return Err(mismatch());
    }
    let selected = std::slice::from_ref(selected);
    let archive =
        stash_archived_lineage::Links::for_action_clocks(db, project, pool, store, selected)?;
    let mut walker = Walker {
        db,
        project,
        source,
        pool,
        memo: HashMap::new(),
        active: HashSet::new(),
        carriers: vec![],
        archive: Some(archive),
    };
    let mapped = map_effective(&mut walker, revision, current)?;
    selected_archived_retimings(&mapped.ancestry, &selected[0])?;
    Ok(mapped)
}
fn map_effective(
    walker: &mut Walker<'_>,
    revision: RevisionId,
    current: &MotionTrack,
) -> PResult<VerifiedActionMapping> {
    let pool = walker.pool;
    let derived = walker.revision(rev_sql(revision)?, 0)?;
    if derived.track.as_ref() != Some(current) {
        return Err(mismatch());
    }
    let mut entries = 0usize;
    for label in derived.labels.iter().copied().collect::<HashSet<_>>() {
        entries = entries
            .checked_add(walker.carriers.get(label).ok_or_else(mismatch)?.entries)
            .ok_or_else(exhausted)?;
        if entries > MAX_ENTRIES {
            return Err(exhausted());
        }
    }
    let memory = pool.reserve_memory(
        (entries as u64)
            .checked_mul(512)
            .and_then(|n| {
                (current.actions().len() as u64)
                    .checked_mul(1024)
                    .and_then(|m| n.checked_add(m))
            })
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(exhausted)?,
    )?;
    let mut compact = BTreeMap::new();
    let mut carriers = Vec::new();
    let mut origins = Vec::new();
    let mut selections = Vec::new();
    for (action, label) in current.actions().iter().zip(&derived.labels) {
        let index = if let Some(index) = compact.get(label) {
            *index
        } else {
            let carrier = walker.carriers.get(*label).ok_or_else(mismatch)?;
            let index = carriers.len();
            carriers.push(carrier.mapper.clone());
            origins.push(carrier.origin.clone());
            compact.insert(*label, index);
            index
        };
        selections.push(SourceClockSelection {
            project_time: action.time(),
            carrier: index,
        });
    }
    let mapped = map_track_to_selected_source_clocks(
        current,
        &carriers,
        &selections,
        MAX_ENTRIES,
        MAX_NODES,
    )
    .map_err(|e| ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string()))?;
    let selections = mapped
        .motion
        .mappings
        .iter()
        .zip(mapped.carriers)
        .map(|(m, carrier)| ActionClockOrigin {
            project_time: m.project_time,
            carrier,
        })
        .collect();
    Ok(VerifiedActionMapping {
        motion: mapped.motion,
        ancestry: NativeActionAncestry {
            carriers: origins,
            selections,
        },
        _memory: memory,
    })
}

/// The selected native anchor must contribute effective actions. Every carrier
/// was independently checked by the walker; its retiming chain is not global.
pub(super) fn selected_native_retimings(
    ancestry: &NativeActionAncestry,
    job: &JobId,
    selected: &source_clock_origins::NativeClocks,
) -> PResult<Vec<RevisionId>> {
    if !ancestry.carriers.iter().any(|carrier| {
        carrier.origin_manifest.is_none()
            && carrier.original_candidate == selected.original_candidate
            && carrier.original_job == *job
            && carrier.original_program == selected.program
            && carrier.original_receipt == selected.verified.identity
            && carrier.placement == selected.verified.plan.placement
    }) {
        return Err(mismatch());
    }
    let revisions = ancestry
        .carriers
        .iter()
        .flat_map(|carrier| carrier.retimings.iter().copied())
        .collect::<BTreeSet<_>>();
    if revisions.len() > MAX_NODES {
        return Err(exhausted());
    }
    Ok(revisions.into_iter().collect())
}

/// Catalogue of independently verified carrier chains, not one global retiming.
pub(super) fn selected_archived_retimings(
    ancestry: &NativeActionAncestry,
    selected: &ArchivedGenerationOrigin,
) -> PResult<Vec<RevisionId>> {
    if !ancestry.carriers.iter().any(|carrier| {
        carrier.origin_manifest.as_ref() == Some(&selected.origin_manifest)
            && carrier.original_candidate == selected.candidate_id
            && carrier.original_job == selected.job_id
            && carrier.original_program == selected.program
            && carrier.original_receipt == selected.worker_receipt
            && selected.settings.source_placement.as_ref() == Some(&carrier.placement)
    }) {
        return Err(mismatch());
    }
    let revisions = ancestry
        .carriers
        .iter()
        .flat_map(|carrier| carrier.retimings.iter().copied())
        .collect::<BTreeSet<_>>();
    if revisions.len() > 128 {
        return Err(exhausted());
    }
    Ok(revisions.into_iter().collect())
}

fn current_master_reference(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<String> {
    let head:(i64,Option<String>)=db.query_row(
        "SELECT revision,CASE WHEN typeof(program)='text' AND length(CAST(program AS BLOB))=71 THEN program ELSE NULL END FROM projects WHERE id=?1",
        [project.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(internal)?;
    if head.0 != rev_sql(revision)? {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "archived source-time derivation requires the current master revision",
        ));
    }
    let reference = head.1.ok_or_else(mismatch)?;
    if !reference.strip_prefix("motion:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(mismatch());
    }
    Ok(reference)
}

/// Explicit source-time publication replaces the single-clock derivation guard
/// only after complete effective-action correspondence and anchor contribution.
pub(super) fn archived_master(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    candidate: &CandidateId,
    selected: &ArchivedGenerationOrigin,
    pool: &ResourcePool,
    store: &crate::project_package_import_storage::ImportStore,
) -> PResult<ArchivedMasterDerivation> {
    let reference = current_master_reference(db, project, revision)?;
    let descriptor = motion_state::revision_descriptor(db, project, revision)?;
    if reference != format!("motion:{}", descriptor.sha256) {
        return Err(mismatch());
    }
    let _memory = pool.reserve_memory(
        descriptor
            .byte_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(exhausted)?,
    )?;
    let program = motion_state::store_for(db)?.read(&descriptor)?;
    let track = program.track(Axis::Stroke).ok_or_else(mismatch)?;
    let mapped = map_archived(db, project, revision, selected, track, pool, store)?;
    let retiming_revisions = selected_archived_retimings(&mapped.ancestry, selected)?;
    Ok(ArchivedMasterDerivation {
        revision,
        master: ArtifactIdentity {
            artifact_id: descriptor.artifact_id,
            sha256: descriptor.sha256,
            byte_len: descriptor.byte_len,
        },
        source_version: selected.source_version.clone(),
        anchor_candidate: candidate.clone(),
        retiming_revisions,
        publication_authority: false,
    })
}

#[cfg(test)]
mod archived_anchor_tests {
    use super::*;
    #[test]
    fn archived_master_head_refuses_unbounded_or_noncanonical_program_metadata() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,revision INTEGER,program);INSERT INTO projects VALUES('head-project',7,NULL);").unwrap();
        let project = ProjectId::new("head-project").unwrap();
        let revision = RevisionId::new(7);
        let valid = format!("motion:{}", "a".repeat(64));
        db.execute("UPDATE projects SET program=?1", [&valid])
            .unwrap();
        assert_eq!(
            current_master_reference(&db, &project, revision).unwrap(),
            valid
        );
        for (name,sql) in [
            ("oversized text","UPDATE projects SET program=CAST(zeroblob(8388608) AS TEXT)"),
            ("multibyte text","UPDATE projects SET program=replace(hex(zeroblob(71)),'00','é')"),
            ("oversized blob","UPDATE projects SET program=zeroblob(8388608)"),
            ("null","UPDATE projects SET program=NULL"),
            ("integer","UPDATE projects SET program=42"),
            ("real","UPDATE projects SET program=4.2"),
            ("empty","UPDATE projects SET program=''"),
            ("wrong prefix","UPDATE projects SET program='unknown'||lower(hex(zeroblob(32)))"),
            ("embedded nul","UPDATE projects SET program='motion:'||char(0)||substr(lower(hex(zeroblob(32))),2)"),
            ("uppercase digest","UPDATE projects SET program='motion:'||replace(lower(hex(zeroblob(32))),'0','A')"),
        ] {
            db.execute_batch(sql).unwrap();
            assert!(matches!(current_master_reference(&db,&project,revision),Err(ProtocolError {code:ErrorCode::DependencyMismatch,..})),"accepted or misclassified {name}");
            assert!(matches!(current_master_reference(&db,&project,RevisionId::new(6)),Err(ProtocolError {code:ErrorCode::RevisionConflict,..})),"stale revision precedence changed for {name}");
        }
    }
    fn fixture() -> (ArchivedGenerationOrigin, ClockCarrierOrigin) {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let mut settings = match request.operation {
            WorkerOperation::Generate { settings, .. } => settings,
            _ => panic!("generation fixture"),
        };
        let placement = SourcePlacement {
            placement_id: SourcePlacementId::new("catalogue-placement").unwrap(),
            created_revision: RevisionId::new(0),
            spec: SourcePlacementSpec {
                source_version: request.source.source_version.clone(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(1, 1).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                rate_numerator: 1,
                rate_denominator: 1,
            },
        };
        settings.source_placement = Some(placement.clone());
        let origin = ArchivedGenerationOrigin {
            origin_manifest: PackageObjectRef {
                sha256: "a".repeat(64),
                byte_len: 1,
            },
            project_id: request.project_id,
            candidate_id: CandidateId::new("catalogue-candidate").unwrap(),
            job_id: request.job_id,
            attempt_id: request.attempt_id,
            source_version: request.source.source_version,
            source_identity: request.source.identity,
            program: ArtifactIdentity {
                artifact_id: ArtifactId::new(format!("motion.{}", "b".repeat(64))).unwrap(),
                sha256: "b".repeat(64),
                byte_len: 1,
            },
            worker_manifest: PackageObjectRef {
                sha256: "c".repeat(64),
                byte_len: 1,
            },
            worker_receipt: ArtifactIdentity {
                artifact_id: ArtifactId::new("d".repeat(64)).unwrap(),
                sha256: "d".repeat(64),
                byte_len: 1,
            },
            preset: "fast".into(),
            settings: *settings,
            clock: None,
        };
        let carrier = ClockCarrierOrigin {
            origin_manifest: Some(origin.origin_manifest.clone()),
            original_candidate: origin.candidate_id.clone(),
            original_job: origin.job_id.clone(),
            original_program: origin.program.clone(),
            original_receipt: origin.worker_receipt.clone(),
            placement,
            retimings: vec![RevisionId::new(2)],
        };
        (origin, carrier)
    }
    #[test]
    fn archived_anchor_catalogue_is_exact_sorted_and_requires_effective_contribution() {
        // Conditional unit inputs only; no archive loading or hardware qualification.
        let (origin, carrier) = fixture();
        let mut sibling = carrier.clone();
        sibling.original_candidate = CandidateId::new("sibling").unwrap();
        sibling.retimings = vec![RevisionId::new(1), RevisionId::new(2)];
        let ancestry = NativeActionAncestry {
            carriers: vec![carrier.clone(), sibling.clone()],
            selections: vec![],
        };
        assert_eq!(
            selected_archived_retimings(&ancestry, &origin).unwrap(),
            vec![RevisionId::new(1), RevisionId::new(2)]
        );
        assert!(selected_archived_retimings(
            &NativeActionAncestry {
                carriers: vec![sibling],
                selections: vec![]
            },
            &origin
        )
        .is_err());
        for field in [
            "namespace",
            "candidate",
            "job",
            "program",
            "receipt",
            "placement",
        ] {
            let mut changed = carrier.clone();
            match field {
                "namespace" => changed.origin_manifest.as_mut().unwrap().sha256 = "e".repeat(64),
                "candidate" => changed.original_candidate = CandidateId::new("other").unwrap(),
                "job" => changed.original_job = JobId::new("other").unwrap(),
                "program" => changed.original_program.sha256 = "e".repeat(64),
                "receipt" => changed.original_receipt.sha256 = "e".repeat(64),
                _ => changed.placement.spec.rate_numerator = 2,
            }
            assert!(
                selected_archived_retimings(
                    &NativeActionAncestry {
                        carriers: vec![changed],
                        selections: vec![]
                    },
                    &origin
                )
                .is_err(),
                "{field}"
            );
        }
        let mut changed = carrier;
        changed.retimings = (1..=129).map(RevisionId::new).collect();
        assert_eq!(
            selected_archived_retimings(
                &NativeActionAncestry {
                    carriers: vec![changed],
                    selections: vec![]
                },
                &origin
            )
            .unwrap_err()
            .code,
            ErrorCode::ResourceExhausted
        );
    }
}
