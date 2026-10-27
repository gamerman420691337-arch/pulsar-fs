//! Read-only historical retiming facts through verified clone namespaces.
use super::*;
use crate::project_package_import_storage::ImportStore;

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "archival retiming facts or destination correspondence changed",
    )
}
fn absent() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::NotFound,
        "no preserved archival retiming receipt corresponds to this revision",
    )
}
fn memory(pool: &ResourcePool, bytes: u64) -> PResult<Reservation> {
    let needed = bytes
        .checked_mul(8)
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "archival retiming memory overflow",
            )
        })?;
    pool.reserve_memory(needed)
}

fn placement(
    db: &Connection,
    project: &ProjectId,
    id: &SourcePlacementId,
) -> PResult<SourcePlacement> {
    let row:Option<(Option<String>,Option<String>,i64)>=db.query_row("SELECT CASE WHEN typeof(d.descriptor)='text' AND length(CAST(d.descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN d.descriptor ELSE NULL END,CASE WHEN typeof(d.source)='text' AND length(CAST(d.source AS BLOB)) BETWEEN 1 AND 128 THEN d.source ELSE NULL END,p.revision FROM source_placement_definitions d JOIN projects p ON p.id=d.project WHERE d.project=?1 AND d.id=?2",params![project.as_str(),id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let (raw, source, head) = row.ok_or_else(mismatch)?;
    let result: SourcePlacement = decode(&raw.ok_or_else(mismatch)?)?;
    result.spec.validate().map_err(|_| mismatch())?;
    if result.placement_id != *id
        || result.spec.source_version.as_str() != source.ok_or_else(mismatch)?
        || rev_sql(result.created_revision)? > head
    {
        return Err(mismatch());
    }
    Ok(result)
}
fn motion_descriptor(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<ProgramDescriptor> {
    let row:Option<(String,Option<String>)>=db.query_row("SELECT CASE WHEN typeof(r.program)='text' AND length(CAST(r.program AS BLOB))=71 THEN r.program ELSE '' END,CASE WHEN typeof(o.descriptor)='text' AND length(CAST(o.descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN o.descriptor ELSE NULL END FROM revisions r LEFT JOIN motion_objects o ON o.sha256=substr(r.program,8) WHERE r.project=?1 AND r.revision=?2",params![project.as_str(),rev_sql(revision)?],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (reference, raw) = row.ok_or_else(mismatch)?;
    let result: ProgramDescriptor = decode(&raw.ok_or_else(mismatch)?)?;
    result.validate()?;
    if reference != format!("motion:{}", result.sha256) {
        return Err(mismatch());
    }
    Ok(result)
}

pub(super) fn load_verified(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    pool: &ResourcePool,
    store: &ImportStore,
) -> PResult<ArchivedMotionRetimeReceipt> {
    let graph = archived_project_origins::load(db, project, Some(revision), pool, store)?;
    let manifest = &graph.root;
    let origins = &graph.origins;
    let mut selected = None;
    for edge in &manifest.imported_origins {
        let Some(mapped) = edge.revisions.iter().find(|m| m.local == revision) else {
            continue;
        };
        let origin = origins.get(&edge.manifest.sha256).ok_or_else(mismatch)?;
        let Some(receipt) = origin
            .retiming_receipts
            .iter()
            .find(|r| r.revision == mapped.origin)
        else {
            continue;
        };
        if selected.is_some() {
            return Err(mismatch());
        }
        selected = Some((edge, receipt));
    }
    let (edge, receipt) = selected.ok_or_else(absent)?;
    let base = edge
        .revisions
        .iter()
        .find(|m| m.origin == receipt.base_revision)
        .ok_or_else(mismatch)?
        .local;
    let from = edge
        .placements
        .iter()
        .find(|m| m.origin == receipt.from.placement_id)
        .ok_or_else(mismatch)?;
    let to = edge
        .placements
        .iter()
        .find(|m| m.origin == receipt.to.placement_id)
        .ok_or_else(mismatch)?;
    let actor = edge
        .actors
        .iter()
        .find(|m| m.origin == receipt.actor)
        .ok_or_else(mismatch)?
        .local
        .clone();
    let from = placement(db, project, &from.local)?;
    let to = placement(db, project, &to.local)?;
    let recorded:Option<(String,String,i64,String)>=db.query_row("SELECT CASE WHEN typeof(r.actor)='text' AND length(CAST(r.actor AS BLOB)) BETWEEN 1 AND 256 THEN r.actor ELSE '' END,CASE WHEN typeof(r.kind)='text' AND length(CAST(r.kind AS BLOB)) BETWEEN 1 AND 64 THEN r.kind ELSE '' END,l.parent_revision,CASE WHEN typeof(l.operation)='text' AND length(CAST(l.operation AS BLOB)) BETWEEN 1 AND 64 THEN l.operation ELSE '' END FROM revisions r JOIN revision_lineage l ON l.project=r.project AND l.revision=r.revision WHERE r.project=?1 AND r.revision=?2",params![project.as_str(),rev_sql(revision)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(internal)?;
    if recorded
        != Some((
            actor.clone(),
            "source_retime".into(),
            rev_sql(base)?,
            "source_retime".into(),
        ))
        || from.created_revision != receipt.from.created_revision
        || to.created_revision != receipt.to.created_revision
        || from.spec != receipt.from.spec
        || to.spec != receipt.to.spec
    {
        return Err(mismatch());
    }
    let members:Option<Option<String>>=db.query_row("SELECT CASE WHEN typeof(ids)='text' AND length(CAST(ids AS BLOB)) BETWEEN 1 AND 1048576 THEN ids ELSE NULL END FROM source_placement_revisions WHERE project=?1 AND revision=?2",params![project.as_str(),rev_sql(revision)?],|r|r.get(0)).optional().map_err(internal)?;
    let members: Vec<SourcePlacementId> = decode(&members.flatten().ok_or_else(mismatch)?)?;
    if !members.contains(&to.placement_id) {
        return Err(mismatch());
    }
    let before = motion_descriptor(db, project, base)?;
    let after = motion_descriptor(db, project, revision)?;
    if before.sha256 != receipt.before.sha256
        || before.byte_len != receipt.before.byte_len
        || before.artifact_id != receipt.before.artifact_id
        || after.sha256 != receipt.after.sha256
        || after.byte_len != receipt.after.byte_len
        || after.artifact_id != receipt.after.artifact_id
    {
        return Err(mismatch());
    }
    let bytes = before
        .byte_len
        .checked_add(after.byte_len)
        .ok_or_else(mismatch)?;
    let _motion_memory = memory(pool, bytes)?;
    let motion = motion_state::store_for(db)?;
    let before = motion.read(&before)?;
    let after = motion.read(&after)?;
    if retime_motion_between_placements(&before, &from.spec, &to.spec, &receipt.axes)
        .map_err(|_| mismatch())?
        != after
    {
        return Err(mismatch());
    }
    let result = ArchivedMotionRetimeReceipt {
        schema_version: 1,
        project_id: project.clone(),
        base_revision: base,
        revision,
        from,
        to,
        local_actor: actor,
        origin_manifest: edge.manifest.clone(),
        origin_receipt: receipt.clone(),
        historical_attribution_only: true,
    };
    result.validate()?;
    Ok(result)
}
