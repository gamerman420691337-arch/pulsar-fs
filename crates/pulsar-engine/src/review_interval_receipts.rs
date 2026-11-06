//! Transactional interval-review intent. No motion or qualification authority.
use super::*;
const LIMIT: usize = 4096;
pub(crate) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS review_interval_receipts(project TEXT NOT NULL,revision INTEGER NOT NULL,receipt TEXT NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(project,revision),FOREIGN KEY(project,revision) REFERENCES revisions(project,revision));")?;
    Ok(())
}
pub(crate) fn validate_layout(db: &Connection) -> Result<()> {
    let mut statement = db.prepare("PRAGMA table_info(review_interval_receipts)")?;
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
        "interval review receipt database layout requires explicit recovery"
    );
    let mut statement = db.prepare("PRAGMA foreign_key_list(review_interval_receipts)")?;
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
        "interval review receipt database keys require explicit recovery"
    );
    Ok(())
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "interval review intent differs from its committed receipt",
    )
}
pub(super) fn record(tx: &Transaction<'_>, project: &ProjectId, d: &ReviewDecision) -> PResult<()> {
    if d.range.is_none() {
        return Err(mismatch());
    }
    let raw = encode(d)?;
    if raw.len() > LIMIT {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "interval review receipt exceeds admission",
        ));
    }
    tx.execute(
        "INSERT INTO review_interval_receipts VALUES(?1,?2,?3,?4)",
        params![
            project.as_str(),
            rev_sql(d.recorded_revision)?,
            raw,
            format!("{:x}", Sha256::digest(raw.as_bytes()))
        ],
    )
    .map_err(internal)?;
    Ok(())
}
pub(super) fn require(db: &Connection, project: &ProjectId, d: &ReviewDecision) -> PResult<()> {
    let row:Option<(Option<String>,String)>=db.query_row("SELECT CASE WHEN typeof(receipt)='text' AND length(CAST(receipt AS BLOB)) BETWEEN 1 AND 4096 THEN receipt ELSE NULL END,CASE WHEN typeof(sha256)='text' AND length(CAST(sha256 AS BLOB))=64 THEN sha256 ELSE '' END FROM review_interval_receipts WHERE project=?1 AND revision=?2",params![project.as_str(),rev_sql(d.recorded_revision)?],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    let (raw, sha) = row.ok_or_else(mismatch)?;
    let raw = raw.ok_or_else(mismatch)?;
    if encode(d)? != raw || format!("{:x}", Sha256::digest(raw.as_bytes())) != sha {
        return Err(mismatch());
    }
    Ok(())
}
