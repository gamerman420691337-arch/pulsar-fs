//! Durable immutable placement definitions and revision/history membership.
use super::*;
pub(super) fn initialize(db: &mut Connection, previous_version: i64) -> Result<()> {
    if matches!(previous_version, 3..=20) {
        let tables:i64=db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('source_placement_definitions','source_placement_active','source_placement_history','source_placement_revisions')",[],|r|r.get(0))?;
        anyhow::ensure!(
            tables == 4,
            "placement database schema is incomplete; explicit recovery is required"
        );
        if previous_version >= 4 {
            let proof:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='stash_clock_proofs')",[],|r|r.get(0))?;
            anyhow::ensure!(
                proof,
                "timing proof database schema is incomplete; explicit recovery is required"
            );
            stash_clock_proofs::validate_layout(db)?;
        }
    }
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS source_placement_definitions(id TEXT PRIMARY KEY,project TEXT NOT NULL,source TEXT NOT NULL,descriptor TEXT NOT NULL,UNIQUE(project,id),FOREIGN KEY(project,source) REFERENCES sources(project,version));
 CREATE TABLE IF NOT EXISTS source_placement_active(project TEXT NOT NULL,id TEXT NOT NULL,PRIMARY KEY(project,id),FOREIGN KEY(project,id) REFERENCES source_placement_definitions(project,id));
 CREATE TABLE IF NOT EXISTS source_placement_history(project TEXT NOT NULL,position INTEGER NOT NULL,ids TEXT NOT NULL,PRIMARY KEY(project,position),FOREIGN KEY(project,position) REFERENCES edit_states(project,position) ON DELETE CASCADE);
 CREATE TABLE IF NOT EXISTS source_placement_revisions(project TEXT NOT NULL,revision INTEGER NOT NULL,ids TEXT NOT NULL,PRIMARY KEY(project,revision),FOREIGN KEY(project,revision) REFERENCES revisions(project,revision));
 ")?;
    stash_clock_proofs::initialize(&tx)?;
    motion_retime_receipts::initialize(&tx)?;
    review_interval_receipts::initialize(&tx)?;
    imported_candidate_intervals::initialize(&tx)?;
    // Imported restrictions and the schema-9 semantic barrier publish together
    // after checked archival recovery. Never downgrade an existing barrier.
    if previous_version < 8 {
        crate::inference_equivalence_reader::publish(&tx, 8)?;
    }
    tx.commit()?;
    Ok(())
}
fn ids(db: &Connection, p: &ProjectId) -> PResult<Vec<String>> {
    let mut stmt = db
        .prepare("SELECT id FROM source_placement_active WHERE project=?1 ORDER BY id")
        .map_err(internal)?;
    let rows = stmt
        .query_map([p.as_str()], |r| r.get::<_, String>(0))
        .map_err(internal)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(internal)
}
pub(super) fn active(
    db: &Connection,
    p: &ProjectId,
    id: &SourcePlacementId,
) -> PResult<SourcePlacement> {
    let row:Option<(String,String,i64)>=db.query_row("SELECT d.descriptor,d.source,p.revision FROM source_placement_active a JOIN source_placement_definitions d ON d.project=a.project AND d.id=a.id JOIN projects p ON p.id=a.project WHERE a.project=?1 AND a.id=?2",params![p.as_str(),id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let (raw, source, revision) = row.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "source placement is not active in this project",
        )
    })?;
    let placement: SourcePlacement = decode(&raw)?;
    if placement.spec.source_version.as_str() != source
        || placement.created_revision.value() > u64::try_from(revision).map_err(internal)?
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "placement source/revision binding mismatch",
        ));
    }
    placement.spec.validate().map_err(internal)?;
    if &placement.placement_id != id {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "placement definition identity mismatch",
        ));
    }
    Ok(placement)
}
/// Historical immutable definition in this project; active membership is not required.
pub(super) fn retained(
    db: &Connection,
    p: &ProjectId,
    id: &SourcePlacementId,
) -> PResult<SourcePlacement> {
    let row:Option<(String,String,i64)>=db.query_row("SELECT d.descriptor,d.source,p.revision FROM source_placement_definitions d JOIN projects p ON p.id=d.project WHERE d.project=?1 AND d.id=?2",params![p.as_str(),id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(internal)?;
    let (raw, source, revision) = row.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "source placement definition is not retained in this project",
        )
    })?;
    let value: SourcePlacement = decode(&raw)?;
    value.spec.validate().map_err(internal)?;
    if value.placement_id != *id
        || value.spec.source_version.as_str() != source
        || value.created_revision.value() > u64::try_from(revision).map_err(internal)?
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "retained placement binding mismatch",
        ));
    }
    Ok(value)
}
pub(super) fn snapshot(db: &Connection, p: &ProjectId) -> PResult<Vec<SourcePlacement>> {
    let keys = ids(db, p)?;
    if keys.len() > 128 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "active source placement limit exceeded",
        ));
    }
    keys.into_iter()
        .map(|key| active(db, p, &SourcePlacementId::new(key).map_err(internal)?))
        .collect()
}
pub(super) fn write(
    engine: &Engine,
    tx: &Transaction<'_>,
    request: &Request,
    session: &SessionId,
) -> PResult<ResponseBody> {
    let p = project(request)?;
    let state = project_state(tx, p)?;
    let next = state.revision.checked_next().map_err(internal)?;
    match &request.command {
        Command::SetSourcePlacement { spec, replace } => {
            spec.validate()
                .map_err(|e| ProtocolError::invalid(e.to_string()))?;
            source_kinds::require_media(tx, p, &spec.source_version)?;
            let available:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2 AND evicted=0)",params![p.as_str(),spec.source_version.as_str()],|r|r.get(0)).map_err(internal)?;
            if !available {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "source placement requires its retained source snapshot",
                ));
            }
            if let Some(old) = replace {
                active(tx, p, old)?;
            }
            let count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM source_placement_definitions WHERE project=?1",
                    [p.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if count >= 4096 || (replace.is_none() && ids(tx, p)?.len() >= 128) {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "source placement definition or active limit reached",
                ));
            }
            let placement = SourcePlacement {
                placement_id: id!(SourcePlacementId),
                created_revision: next,
                spec: spec.clone(),
            };
            tx.execute(
                "INSERT INTO source_placement_definitions VALUES(?1,?2,?3,?4)",
                params![
                    placement.placement_id.as_str(),
                    p.as_str(),
                    spec.source_version.as_str(),
                    encode(&placement)?
                ],
            )
            .map_err(internal)?;
            if let Some(old) = replace {
                tx.execute(
                    "DELETE FROM source_placement_active WHERE project=?1 AND id=?2",
                    params![p.as_str(), old.as_str()],
                )
                .map_err(internal)?;
            }
            tx.execute(
                "INSERT INTO source_placement_active VALUES(?1,?2)",
                params![p.as_str(), placement.placement_id.as_str()],
            )
            .map_err(internal)?;
        }
        Command::RemoveSourcePlacement { placement_id } => {
            active(tx, p, placement_id)?;
            tx.execute(
                "DELETE FROM source_placement_active WHERE project=?1 AND id=?2",
                params![p.as_str(), placement_id.as_str()],
            )
            .map_err(internal)?;
        }
        _ => return Err(ProtocolError::invalid("not a source placement edit")),
    }
    // Alignment changes record a revision and history entry without changing any
    // motion bytes, timestamps, protection or existing generated master.
    commit_program(
        tx,
        request,
        session,
        &state.program,
        "source_alignment",
        "Change source placement; motion unchanged",
        true,
    )?;
    let _ = engine;
    Ok(ResponseBody::Project((project_snapshot(tx, p)?).into()))
}
pub(super) fn capture_revision(
    tx: &Transaction<'_>,
    p: &ProjectId,
    revision: RevisionId,
) -> PResult<()> {
    tx.execute(
        "INSERT INTO source_placement_revisions VALUES(?1,?2,?3)",
        params![p.as_str(), rev_sql(revision)?, encode(&ids(tx, p)?)?],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn capture_history(tx: &Transaction<'_>, p: &ProjectId, position: i64) -> PResult<()> {
    tx.execute(
        "INSERT INTO source_placement_history VALUES(?1,?2,?3)",
        params![p.as_str(), position, encode(&ids(tx, p)?)?],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn restore_history(tx: &Transaction<'_>, p: &ProjectId, position: i64) -> PResult<()> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT ids FROM source_placement_history WHERE project=?1 AND position=?2",
            params![p.as_str(), position],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    let keys: Vec<String> = raw.map(|raw| decode(&raw)).transpose()?.unwrap_or_default();
    if keys.len() > 128 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "restored placement limit exceeded",
        ));
    }
    tx.execute(
        "DELETE FROM source_placement_active WHERE project=?1",
        [p.as_str()],
    )
    .map_err(internal)?;
    for key in keys {
        tx.execute(
            "INSERT INTO source_placement_active VALUES(?1,?2)",
            params![p.as_str(), key],
        )
        .map_err(internal)?;
    }
    Ok(())
}
pub(super) fn is_retained_source(db: &Connection, source: &SourceVersionId) -> PResult<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_placement_definitions WHERE source=?1)",
        [source.as_str()],
        |r| r.get(0),
    )
    .map_err(internal)
}
pub(super) fn has_definitions(db: &Connection, p: &ProjectId) -> PResult<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM source_placement_definitions WHERE project=?1)",
        [p.as_str()],
        |r| r.get(0),
    )
    .map_err(internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_schema_upgrade_preserves_rows_and_incomplete_schema3_refuses_repair() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE edit_states(project TEXT,position INTEGER,PRIMARY KEY(project,position));CREATE TABLE revisions(project TEXT,revision INTEGER,PRIMARY KEY(project,revision));INSERT INTO sources VALUES('p','v');INSERT INTO edit_states VALUES('p',0);INSERT INTO revisions VALUES('p',0);PRAGMA user_version=2;").unwrap();
        initialize(&mut db, 2).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            8
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sources WHERE project='p' AND version='v'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        db.execute("DROP TABLE source_placement_active", [])
            .unwrap();
        assert!(initialize(&mut db, 3).is_err());
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='source_placement_active'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn incomplete_schema4_or5_timing_proof_layout_refuses_automatic_repair() {
        for previous in [4, 5, 18] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch("CREATE TABLE sources(project TEXT,version TEXT,PRIMARY KEY(project,version));CREATE TABLE edit_states(project TEXT,position INTEGER,PRIMARY KEY(project,position));CREATE TABLE revisions(project TEXT,revision INTEGER,PRIMARY KEY(project,revision));CREATE TABLE exports(session TEXT,request TEXT,PRIMARY KEY(session,request));").unwrap();
            initialize(&mut db, 2).unwrap();
            db.execute_batch(&format!("PRAGMA user_version={previous};"))
                .unwrap();
            db.execute("DROP TABLE stash_clock_proofs", []).unwrap();
            assert!(initialize(&mut db, previous).is_err());
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                previous
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='stash_clock_proofs'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            db.execute_batch(
                "CREATE TABLE stash_clock_proofs(session TEXT,request TEXT,proof TEXT);",
            )
            .unwrap();
            assert!(initialize(&mut db, previous).is_err());
        }
    }
}
