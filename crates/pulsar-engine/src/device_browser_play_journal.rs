//! Versioned membership for immutable browser start decisions. Never live authority.
use super::*;

const MEMBERS: &str = "CREATE TABLE play_reference_members(operation_id TEXT PRIMARY KEY NOT NULL,decision_sha256 TEXT,membership_sha256 TEXT NOT NULL)";
const CENSUS: &str = "CREATE TABLE play_reference_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),operation_count INTEGER NOT NULL CHECK(operation_count BETWEEN 0 AND 1024))";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn decision_digest(record: &PlayOperationRecord) -> Result<Option<String>> {
    record
        .browser_start
        .as_ref()
        .map(|decision| {
            decision.validate()?;
            Ok(digest(&serde_json::to_vec(decision)?))
        })
        .transpose()
}
fn membership_digest(id: &str, decision: Option<&str>) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&(
        "play-reference-v1",
        id,
        decision,
    ))?))
}

fn census(db: &Connection) -> Result<u64> {
    let mut q =
        db.prepare("SELECT singleton,operation_count FROM play_reference_census LIMIT 2")?;
    let values = q
        .query_map([], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        values.len() == 1 && values[0].0 == 1 && values[0].1 <= CAPACITY,
        "play reference census is missing or invalid"
    );
    let (operations, members): (u64, u64) = db.query_row(
        "SELECT (SELECT count(*) FROM play_operations),(SELECT count(*) FROM play_reference_members)",
        [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    ensure!(
        operations == values[0].1 && members == values[0].1,
        "play reference operation or membership is missing"
    );
    Ok(operations)
}

/// Validate exact membership on every read, not just at process startup.
pub(super) fn check_row(db: &Connection, id: &str, record: &PlayOperationRecord) -> Result<()> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 9 {
        ensure!(
            record.browser_start.is_none(),
            "browser play requires journal version 9"
        );
        return Ok(());
    }
    ensure!(
        matches!(version, 9..=13),
        "unsupported play reference journal version"
    );
    census(db)?;
    let (decision, membership): (Option<String>, String) = db.query_row(
        "SELECT CASE WHEN decision_sha256 IS NULL OR length(decision_sha256)=64 THEN decision_sha256 ELSE 'invalid' END,CASE WHEN length(membership_sha256)=64 THEN membership_sha256 ELSE NULL END FROM play_reference_members WHERE operation_id=?1",
        [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let expected = decision_digest(record)?;
    ensure!(
        decision == expected && membership == membership_digest(id, expected.as_deref())?,
        "play reference membership differs"
    );
    Ok(())
}

/// Called in the same transaction as the effect and original play insertion.
pub(super) fn insert(db: &Connection, id: &str, record: &PlayOperationRecord) -> Result<()> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        matches!(version, 9..=13),
        "play reference journal unavailable"
    );
    let decision = decision_digest(record)?;
    db.execute("INSERT INTO play_reference_members(operation_id,decision_sha256,membership_sha256) VALUES(?1,?2,?3)",
        params![id,decision,membership_digest(id, decision.as_deref())?])?;
    ensure!(db.execute("UPDATE play_reference_census SET operation_count=operation_count+1 WHERE singleton=1 AND operation_count<1024", [])? == 1,
        "play reference census cannot admit operation");
    check_row(db, id, record)
}

pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, sql) in [
        ("play_reference_members", MEMBERS),
        ("play_reference_census", CENSUS),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if version < 9 {
            ensure!(
                actual.is_none(),
                "unexpected browser play journal before migration"
            );
        } else {
            ensure!(
                actual == Some(("table".into(), sql.into())),
                "browser play journal layout differs"
            );
        }
    }
    if version < 9 {
        return Ok(());
    }
    let triggers: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('play_reference_members','play_reference_census')",
        [], |r| r.get(0))?;
    ensure!(triggers == 0, "unexpected browser play journal trigger");
    let count = census(db)?;
    let mut q = db.prepare("SELECT CASE WHEN length(operation_id)=64 THEN operation_id ELSE NULL END FROM play_reference_members")?;
    let mut checked = 0u64;
    for id in q.query_map([], |r| r.get::<_, String>(0))? {
        super::play::row(db, &id?)?;
        checked += 1;
    }
    ensure!(checked == count, "play reference census differs");
    Ok(())
}

pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 9 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // Earlier schema owners have already upgraded to v8. Validate their rows
    // before changing the format or adding reference membership.
    ensure!(
        DeviceEffectJournal::validate_existing(&tx)? == 8,
        "browser play migration requires intact version 8"
    );
    let mut originals = Vec::new();
    {
        let mut q = tx.prepare("SELECT id FROM play_operations LIMIT 1025")?;
        for id in q.query_map([], |r| r.get::<_, String>(0))? {
            let id = id?;
            let record = super::play::row(&tx, &id)?.record;
            ensure!(
                record.browser_start.is_none(),
                "legacy browser decision cannot be adopted"
            );
            originals.push((id, record));
        }
    }
    ensure!(
        originals.len() <= CAPACITY as usize,
        "play reference migration exceeds capacity"
    );
    tx.execute_batch(MEMBERS)?;
    tx.execute_batch(CENSUS)?;
    tx.execute(
        "INSERT INTO play_reference_census VALUES(1,?1)",
        [originals.len() as u64],
    )?;
    for (id, record) in &originals {
        let decision = decision_digest(record)?;
        tx.execute(
            "INSERT INTO play_reference_members VALUES(?1,?2,?3)",
            params![id, decision, membership_digest(id, decision.as_deref())?],
        )?;
    }
    tx.execute_batch("PRAGMA user_version=9")?;
    validate_schema(&tx, 9)?;
    tx.commit()?;
    Ok(())
}
