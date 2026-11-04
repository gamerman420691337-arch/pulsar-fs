//! Durable revision ancestry and unresolved review snapshots. No equality-based origin inference.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct ReviewState {
    pub(super) flags: Vec<ReviewFlag>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) decisions: Vec<ReviewDecision>,
    pub(super) unknown: bool,
    pub(super) conservative: bool,
}
impl ReviewState {
    fn empty() -> Self {
        Self {
            flags: vec![],
            decisions: vec![],
            unknown: false,
            conservative: false,
        }
    }
}

pub(super) fn initialize(db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS revision_lineage(
            project TEXT NOT NULL, revision INTEGER NOT NULL,
            parent_revision INTEGER, restored_from_revision INTEGER, restored_history_position INTEGER,
            candidate TEXT, operation TEXT NOT NULL, contributions TEXT NOT NULL, review_state TEXT NOT NULL,
            PRIMARY KEY(project,revision), FOREIGN KEY(project,revision) REFERENCES revisions(project,revision),
            FOREIGN KEY(project,parent_revision) REFERENCES revisions(project,revision),
            FOREIGN KEY(project,restored_from_revision) REFERENCES revisions(project,revision),
            FOREIGN KEY(candidate) REFERENCES candidates(id));
         CREATE TABLE IF NOT EXISTS history_lineage(
            project TEXT NOT NULL, position INTEGER NOT NULL, revision INTEGER NOT NULL,
            PRIMARY KEY(project,position),
            FOREIGN KEY(project,position) REFERENCES edit_states(project,position) ON DELETE CASCADE,
            FOREIGN KEY(project,revision) REFERENCES revisions(project,revision));"
    )?;
    let projects = {
        let mut statement = tx.prepare("SELECT id,revision,history_cursor FROM projects WHERE NOT EXISTS(SELECT 1 FROM project_import_archives WHERE project_import_archives.project=projects.id) AND NOT EXISTS(
            SELECT 1 FROM revision_lineage WHERE revision_lineage.project=projects.id AND revision_lineage.revision=projects.revision)")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (project, revision, cursor) in projects {
        // Only an existing explicit candidate->committed revision link is reused.
        // We never guess an older undo/redo ancestry from equal program bytes.
        let reviews = {
            let mut statement = tx.prepare(
                "SELECT review FROM candidates WHERE project=?1 AND committed_revision=?2",
            )?;
            let rows =
                statement.query_map(params![project, revision], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut state = ReviewState {
            flags: vec![],
            decisions: vec![],
            unknown: true,
            conservative: true,
        };
        for raw in reviews {
            state.flags.extend(decode::<Vec<ReviewFlag>>(&raw)?);
        }
        validate_reviews(&state.flags)?;
        anyhow::ensure!(
            encode(&state)?.len() <= PROGRAM_LIMIT,
            "legacy review snapshot exceeds 512 KiB"
        );
        tx.execute("INSERT INTO revision_lineage VALUES(?1,?2,NULL,NULL,NULL,NULL,'legacy_unknown','[]',?3)",
            params![project,revision,encode(&state)?])?;
        tx.execute(
            "INSERT OR IGNORE INTO history_lineage VALUES(?1,?2,?3)",
            params![project, cursor, revision],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub(super) fn state(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<ReviewState> {
    let raw: Option<String> = db
        .query_row(
            "SELECT review_state FROM revision_lineage WHERE project=?1 AND revision=?2",
            params![project.as_str(), rev_sql(revision)?],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let mut state: ReviewState = match raw {
        Some(raw) => decode(&raw)?,
        None => ReviewState {
            flags: vec![],
            decisions: vec![],
            unknown: true,
            conservative: true,
        },
    };
    if project_package_imports::imported_revision(db, project, revision)? {
        state.unknown = true;
        state.conservative = true;
        state.decisions.clear();
    }
    validate_review_decisions(&state.flags, &state.decisions)?;
    for d in &state.decisions {
        if d.range.is_some() {
            review_interval_receipts::require(db, project, d)?;
        }
        if d.recorded_revision > revision {
            return Err(ProtocolError::invalid(
                "review decision follows the queried revision",
            ));
        }
        let origin:Option<(String,String,i64,String)>=db.query_row("SELECT r.actor,r.kind,l.parent_revision,l.operation FROM revisions r JOIN revision_lineage l ON l.project=r.project AND l.revision=r.revision WHERE r.project=?1 AND r.revision=?2",params![project.as_str(),rev_sql(d.recorded_revision)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(internal)?;
        if origin
            != Some((
                d.actor.clone(),
                if d.range.is_some() {
                    "review_interval_disposition".into()
                } else {
                    "review_disposition".into()
                },
                rev_sql(d.base_revision)?,
                if d.range.is_some() {
                    "review_interval_disposition".into()
                } else {
                    "review_disposition".into()
                },
            ))
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "review decision attribution/revision binding is inconsistent",
            ));
        }
    }
    Ok(state)
}

pub(super) fn created(tx: &Transaction<'_>, project: &ProjectId) -> PResult<()> {
    tx.execute(
        "INSERT INTO revision_lineage VALUES(?1,0,NULL,NULL,NULL,NULL,'create','[]',?2)",
        params![project.as_str(), encode(&ReviewState::empty())?],
    )
    .map_err(internal)?;
    tx.execute(
        "INSERT INTO history_lineage VALUES(?1,0,0)",
        [project.as_str()],
    )
    .map_err(internal)?;
    Ok(())
}

pub(super) fn coverage(program: &MotionProgram) -> PResult<Vec<(Axis, TimeRange)>> {
    let mut output = Vec::new();
    for track in program.tracks() {
        let mut spans: Vec<_> = track
            .span()
            .map_err(internal)?
            .into_iter()
            .chain(track.gaps().iter().copied())
            .collect();
        spans.sort_by_key(|span| span.start());
        let mut merged: Vec<TimeRange> = Vec::new();
        for span in spans {
            if let Some(last) = merged.last_mut() {
                if span.start() <= last.end() {
                    *last = TimeRange::new(last.start(), last.end().max(span.end()))
                        .map_err(internal)?;
                    continue;
                }
            }
            merged.push(span);
        }
        output.extend(merged.into_iter().map(|span| (track.axis(), span)));
    }
    Ok(output)
}

pub(super) fn clipped(
    flags: &[ReviewFlag],
    regions: &[(Axis, TimeRange)],
) -> PResult<Vec<ReviewFlag>> {
    let mut output = Vec::new();
    for flag in flags {
        for (axis, region) in regions {
            if flag.axis.is_some_and(|selected| selected != *axis) || !flag.range.overlaps(*region)
            {
                continue;
            }
            if output.len() >= 1024 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "review contribution snapshot exceeds 1024 flags",
                ));
            }
            output.push(ReviewFlag {
                axis: Some(*axis),
                range: TimeRange::new(
                    flag.range.start().max(region.start()),
                    flag.range.end().min(region.end()),
                )
                .map_err(internal)?,
                reason: flag.reason.clone(),
                evidence: flag.evidence,
                confidence: flag.confidence.clone(),
            });
        }
    }
    validate_reviews(&output)?;
    Ok(output)
}

/// Called inside the same transaction as the revision and history transition.
pub(super) fn record(
    tx: &Transaction<'_>,
    request: &Request,
    revision: RevisionId,
    parent: RevisionId,
    before: &MotionProgram,
    program: &MotionProgram,
    append: bool,
) -> PResult<()> {
    let project = project(request)?;
    let prior = state(tx, project, parent)?;
    let mut next = prior.clone();
    let mut candidate_id = None;
    let mut restored_from = None;
    let mut restored_position = None;
    let mut contributions = Vec::new();
    let operation = match &request.command {
        Command::CommitCandidate { candidate_id: id } => {
            let candidate = candidate_state(tx, id, project)?;
            next = candidate_review_state(tx, id)?.unwrap_or(ReviewState {
                flags: candidate.review,
                decisions: vec![],
                unknown: false,
                conservative: false,
            });
            candidate_id = Some(id.as_str());
            contributions = coverage(program)?;
            "candidate_commit"
        }
        Command::MergeCandidate {
            candidate_id: id,
            axes,
            range,
        } => {
            let candidate = candidate_state(tx, id, project)?;
            let raw: String = tx
                .query_row(
                    "SELECT protected FROM revisions WHERE project=?1 AND revision=?2",
                    params![project.as_str(), rev_sql(parent)?],
                    |row| row.get(0),
                )
                .map_err(internal)?;
            let protected: Vec<ProtectedRegion> = decode(&raw)?;
            contributions = pulsar_core::candidate_merge_writable_regions(
                before,
                &candidate.program,
                axes,
                *range,
                &protected,
            )
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
            next.flags = clipped(&prior.flags, &coverage(program)?)?;
            next.flags
                .extend(clipped(&candidate.review, &contributions)?);
            // Previous flags may now cover unchanged or replaced samples; they
            // remain visibly conservative until explicit resolution exists.
            next.conservative = true;
            next.unknown |= candidate_review_state(tx, id)?.is_some_and(|state| state.unknown);
            candidate_id = Some(id.as_str());
            "candidate_merge"
        }
        Command::Undo | Command::Redo => {
            let cursor: i64 = tx
                .query_row(
                    "SELECT history_cursor FROM projects WHERE id=?1",
                    [project.as_str()],
                    |row| row.get(0),
                )
                .map_err(internal)?;
            let delta = if matches!(&request.command, Command::Undo) {
                -1
            } else {
                1
            };
            let target = cursor
                .checked_add(delta)
                .ok_or_else(|| ProtocolError::invalid("history lineage position overflow"))?;
            restored_position = Some(target);
            let source: Option<i64> = tx
                .query_row(
                    "SELECT revision FROM history_lineage WHERE project=?1 AND position=?2",
                    params![project.as_str(), target],
                    |row| row.get(0),
                )
                .optional()
                .map_err(internal)?;
            if let Some(source) = source {
                restored_from = Some(source);
                next = state(
                    tx,
                    project,
                    RevisionId::new(u64::try_from(source).map_err(internal)?),
                )?;
            } else {
                next.flags = clipped(&prior.flags, &coverage(program)?)?;
                next.unknown = true;
                next.conservative = true;
            }
            if delta < 0 {
                "undo"
            } else {
                "redo"
            }
        }
        Command::RetimeMotion { .. } => {
            // Original evidence flags remain immutable in their original clock.
            // A new review marker covers the changed clock; old decisions retire.
            for (axis, range) in coverage(program)? {
                next.flags.push(ReviewFlag {
                    axis: Some(axis),
                    range,
                    reason: ReviewReason::new(
                        "existing_motion_explicitly_retimed_review_required",
                    )?,
                    evidence: EvidenceKind::Inferred,
                    confidence: None,
                });
            }
            next.unknown = true;
            next.conservative = true;
            "source_retime"
        }
        Command::SetSourcePlacement { .. } | Command::RemoveSourcePlacement { .. } => {
            for (axis, range) in coverage(program)? {
                let flag = ReviewFlag {
                    axis: Some(axis),
                    range,
                    reason: ReviewReason::new("source_alignment_changed_motion_not_retimed")?,
                    evidence: EvidenceKind::Inferred,
                    confidence: None,
                };
                if !next.flags.iter().any(|existing| {
                    existing.axis == flag.axis
                        && existing.range == flag.range
                        && existing.reason.as_str() == flag.reason.as_str()
                }) {
                    next.flags.push(flag);
                }
            }
            next.conservative = true;
            "source_alignment"
        }
        Command::SetReviewDisposition {
            review_id,
            disposition,
            note,
        }
        | Command::SetReviewIntervalDisposition {
            review_id,
            disposition,
            note,
            ..
        } => {
            if !prior
                .flags
                .iter()
                .any(|flag| review_identity(flag).ok().as_deref() == Some(review_id.as_str()))
            {
                return Err(ProtocolError::new(
                    ErrorCode::NotFound,
                    "review evidence ID is not present at this revision",
                ));
            }
            let range =
                if let Command::SetReviewIntervalDisposition { range, .. } = &request.command {
                    Some(*range)
                } else {
                    None
                };
            if range.is_none() {
                next.decisions.retain(|d| d.review_id != *review_id);
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(internal)?
                .as_millis();
            next.decisions.push(ReviewDecision {
                range,
                review_id: review_id.clone(),
                actor: actor(tx, request.session.as_ref().expect("authenticated"))?,
                request_id: request.request_id.clone(),
                base_revision: parent,
                recorded_revision: revision,
                disposition: *disposition,
                note: note.clone(),
                recorded_at_unix_ms: u64::try_from(now).map_err(internal)?,
            });
            next.decisions.sort_by(|a, b| {
                (&a.review_id, a.recorded_revision).cmp(&(&b.review_id, b.recorded_revision))
            });
            if range.is_some() {
                "review_interval_disposition"
            } else {
                "review_disposition"
            }
        }
        Command::SetProtectedRegions { .. } => "protection",
        _ => {
            next.flags = clipped(&prior.flags, &coverage(program)?)?;
            next.conservative |= !next.flags.is_empty();
            "edit"
        }
    };
    if !matches!(
        &request.command,
        Command::SetReviewDisposition { .. }
            | Command::SetReviewIntervalDisposition { .. }
            | Command::SetProtectedRegions { .. }
            | Command::Undo
            | Command::Redo
    ) {
        next.decisions.clear();
    }
    if matches!(
        &request.command,
        Command::CommitCandidate { .. } | Command::MergeCandidate { .. }
    ) {
        let retained: Vec<_> = prior
            .flags
            .iter()
            .filter(|flag| flag.reason.as_str() == "source_alignment_changed_motion_not_retimed")
            .cloned()
            .collect();
        for flag in clipped(&retained, &coverage(program)?)? {
            if !next.flags.iter().any(|existing| {
                existing.axis == flag.axis
                    && existing.range == flag.range
                    && existing.reason.as_str() == flag.reason.as_str()
            }) {
                next.flags.push(flag);
                next.conservative = true;
            }
        }
    }
    validate_review_decisions(&next.flags, &next.decisions)
        .map_err(|error| ProtocolError::new(ErrorCode::ResourceExhausted, error.to_string()))?;
    if matches!(
        request.command,
        Command::SetReviewIntervalDisposition { .. }
    ) {
        let d = next
            .decisions
            .iter()
            .find(|d| d.recorded_revision == revision)
            .ok_or_else(|| internal("interval decision missing"))?;
        review_interval_receipts::record(tx, project, d)?;
    }
    let encoded = encode(&next)?;
    if encoded.len() > PROGRAM_LIMIT {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "revision review snapshot exceeds 512 KiB",
        ));
    }
    tx.execute(
        "INSERT INTO revision_lineage VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            project.as_str(),
            rev_sql(revision)?,
            rev_sql(parent)?,
            restored_from,
            restored_position,
            candidate_id,
            operation,
            encode(&contributions)?,
            encoded
        ],
    )
    .map_err(internal)?;
    if append {
        let position: i64 = tx
            .query_row(
                "SELECT history_cursor FROM projects WHERE id=?1",
                [project.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        tx.execute(
            "INSERT INTO history_lineage VALUES(?1,?2,?3)",
            params![project.as_str(), position, rev_sql(revision)?],
        )
        .map_err(internal)?;
    }
    Ok(())
}

pub(super) fn candidate_review_state(
    db: &Connection,
    candidate: &CandidateId,
) -> PResult<Option<ReviewState>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT review_state FROM edit_candidate_lineage WHERE candidate=?1",
            [candidate.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let state: Option<ReviewState> = raw.map(|raw| decode(&raw)).transpose()?;
    let mut state = state;
    if let Some(state) = &mut state {
        state.decisions.clear();
        validate_reviews(&state.flags)?;
    }
    if state.is_none() {
        return project_package_imports::candidate_review(db, candidate);
    }
    Ok(state)
}

impl ReviewState {
    pub(super) fn unresolved(&self) -> PResult<Vec<ReviewFlag>> {
        validate_review_decisions(&self.flags, &self.decisions)?;
        let mut result = Vec::new();
        for f in &self.flags {
            let id = review_identity(f)?;
            let decisions: Vec<_> = self
                .decisions
                .iter()
                .filter(|d| d.review_id == id)
                .cloned()
                .collect();
            for range in review_unresolved_ranges(f, &decisions)? {
                let mut flag = f.clone();
                flag.range = range;
                result.push(flag);
            }
        }
        Ok(result)
    }
}
