//! Transactional authored retiming facts. Validation never qualifies media/devices.
use super::*;
const LIMIT: usize = 16 * 1024;
pub(crate) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS motion_retime_receipts(project TEXT NOT NULL,revision INTEGER NOT NULL,receipt TEXT NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(project,revision),FOREIGN KEY(project,revision) REFERENCES revisions(project,revision));")?;
    Ok(())
}
pub(crate) fn validate_layout(db: &Connection) -> Result<()> {
    let mut statement = db.prepare("PRAGMA table_info(motion_retime_receipts)")?;
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
                ("project".into(), "TEXT".into(), 1, 1),
                ("revision".into(), "INTEGER".into(), 1, 2),
                ("receipt".into(), "TEXT".into(), 1, 0),
                ("sha256".into(), "TEXT".into(), 1, 0)
            ],
        "retiming receipt database layout requires explicit recovery"
    );
    let mut statement = db.prepare("PRAGMA foreign_key_list(motion_retime_receipts)")?;
    let keys = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        keys == vec![
            (0, 0, "revisions".into(), "project".into(), "project".into()),
            (
                0,
                1,
                "revisions".into(),
                "revision".into(),
                "revision".into()
            )
        ],
        "retiming receipt database keys require explicit recovery"
    );
    Ok(())
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "retiming receipt does not bind the recorded edit, placements and motion",
    )
}
pub(crate) fn record(tx: &Transaction<'_>, receipt: &MotionRetimeReceipt) -> PResult<()> {
    receipt.validate()?;
    let raw = encode(receipt)?;
    if raw.len() > LIMIT {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "retiming receipt exceeds admission",
        ));
    }
    tx.execute(
        "INSERT INTO motion_retime_receipts VALUES(?1,?2,?3,?4)",
        params![
            receipt.project_id.as_str(),
            rev_sql(receipt.revision)?,
            raw,
            format!("{:x}", Sha256::digest(raw.as_bytes()))
        ],
    )
    .map_err(internal)?;
    Ok(())
}
fn identity(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<ArtifactIdentity> {
    let value = motion_state::revision_descriptor(db, project, revision)?;
    Ok(ArtifactIdentity {
        artifact_id: value.artifact_id,
        sha256: value.sha256,
        byte_len: value.byte_len,
    })
}
pub(crate) fn load_verified(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
    pool: &ResourcePool,
) -> PResult<MotionRetimeReceipt> {
    // SQL byte bounds apply before allocating attacker-modified TEXT/BLOB cells.
    let row:Option<(Option<String>,Option<String>)>=db.query_row("SELECT CASE WHEN typeof(receipt)='text' AND length(CAST(receipt AS BLOB)) BETWEEN 1 AND 16384 THEN receipt ELSE NULL END,CASE WHEN typeof(sha256)='text' AND length(CAST(sha256 AS BLOB))=64 THEN sha256 ELSE NULL END FROM motion_retime_receipts WHERE project=?1 AND revision=?2",params![project.as_str(),rev_sql(revision)?],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (raw, hash) = row.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "no structured retiming receipt is recorded for this revision",
        )
    })?;
    let raw = raw.ok_or_else(mismatch)?;
    let hash = hash.ok_or_else(mismatch)?;
    if format!("{:x}", Sha256::digest(raw.as_bytes())) != hash {
        return Err(mismatch());
    }
    let receipt: MotionRetimeReceipt = decode(&raw)?;
    receipt.validate()?;
    if receipt.project_id != *project || receipt.revision != revision {
        return Err(mismatch());
    }
    let expected:Option<(String,String,i64,String)>=db.query_row("SELECT r.actor,r.kind,l.parent_revision,l.operation FROM revisions r JOIN revision_lineage l ON l.project=r.project AND l.revision=r.revision WHERE r.project=?1 AND r.revision=?2",params![project.as_str(),rev_sql(revision)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(internal)?;
    if expected
        != Some((
            receipt.actor.clone(),
            "source_retime".into(),
            rev_sql(receipt.base_revision)?,
            "source_retime".into(),
        ))
        || actor(db, &receipt.session_id)? != receipt.actor
    {
        return Err(mismatch());
    }
    let request:Option<(String,Option<String>)>=db.query_row("SELECT CASE WHEN typeof(fingerprint)='text' AND length(CAST(fingerprint AS BLOB))=64 THEN fingerprint ELSE '' END,CASE WHEN typeof(response)='text' AND length(CAST(response AS BLOB)) BETWEEN 1 AND 1048576 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![receipt.session_id.as_str(),receipt.request_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (fingerprint, response) = request.ok_or_else(mismatch)?;
    let response: ResponseBody = decode(&response.ok_or_else(mismatch)?)?;
    let ResponseBody::Project(snapshot) = response else {
        return Err(mismatch());
    };
    if fingerprint != receipt.operation_sha256
        || snapshot.project_id != *project
        || snapshot.revision != revision
        || snapshot.motion.program.sha256 != receipt.after.sha256
        || snapshot.motion.program.byte_len != receipt.after.byte_len
        || snapshot.motion.program.artifact_id != receipt.after.artifact_id
        || !snapshot
            .placements
            .iter()
            .any(|placement| placement == &receipt.to)
    {
        return Err(mismatch());
    }
    for placement in [&receipt.from, &receipt.to] {
        let retained = source_placements::retained(db, project, &placement.placement_id)?;
        if retained != *placement {
            return Err(mismatch());
        }
    }
    if identity(db, project, receipt.base_revision)?.sha256 != receipt.before.sha256
        || identity(db, project, receipt.base_revision)?.byte_len != receipt.before.byte_len
        || identity(db, project, revision)?.sha256 != receipt.after.sha256
        || identity(db, project, revision)?.byte_len != receipt.after.byte_len
    {
        return Err(mismatch());
    }
    let needed = receipt
        .before
        .byte_len
        .checked_add(receipt.after.byte_len)
        .and_then(|v| v.checked_mul(8))
        .and_then(|v| v.checked_add(16 * 1024 * 1024))
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "retiming verification memory overflow",
            )
        })?;
    let _memory = pool.reserve_memory(needed)?;
    let store = motion_state::store_for(db)?;
    let before = store.read(&motion_state::revision_descriptor(
        db,
        project,
        receipt.base_revision,
    )?)?;
    let after = store.read(&motion_state::revision_descriptor(db, project, revision)?)?;
    if retime_motion_between_placements(
        &before,
        &receipt.from.spec,
        &receipt.to.spec,
        &receipt.axes,
    )
    .map_err(|_| mismatch())?
        != after
    {
        return Err(mismatch());
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_requires_composite_revision_key_and_foreign_key() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE revisions(project TEXT,revision INTEGER,PRIMARY KEY(project,revision));",
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        initialize(&tx).unwrap();
        tx.commit().unwrap();
        validate_layout(&db).unwrap();
        db.execute_batch("DROP TABLE motion_retime_receipts;CREATE TABLE motion_retime_receipts(project TEXT NOT NULL,revision INTEGER NOT NULL,receipt TEXT NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(project,revision));").unwrap();
        assert!(validate_layout(&db).is_err());
        db.execute_batch("DROP TABLE motion_retime_receipts;CREATE TABLE motion_retime_receipts(project TEXT NOT NULL,revision INTEGER NOT NULL,receipt TEXT NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(revision,project),FOREIGN KEY(project,revision) REFERENCES revisions(project,revision));").unwrap();
        assert!(validate_layout(&db).is_err());
    }
}
