//! Retrospective emergency-stop observations. This path never issues a receipt or claim.
use super::*;

const TABLE: &str = "CREATE TABLE emergency_stop_observations(effect_id TEXT PRIMARY KEY NOT NULL,outcome TEXT NOT NULL CHECK(outcome IN ('acknowledged','unknown')),provenance_sha256 TEXT NOT NULL)";
const CENSUS: &str = "CREATE TABLE emergency_stop_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),observation_count INTEGER NOT NULL CHECK(observation_count BETWEEN 0 AND 1024))";
const PROVENANCE: &str = "emergency-stop-without-confirmed-prewrite-v1";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn outcome_name(outcome: DispatchOutcome) -> &'static str {
    match outcome {
        DispatchOutcome::Acknowledged => "acknowledged",
        DispatchOutcome::Unknown => "unknown",
    }
}
fn provenance(id: &str, outcome: &str) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&(PROVENANCE, id, outcome))?))
}
pub(super) fn stop_shape(record: &EffectRecord) -> Result<()> {
    let expected = digest(&serde_json::to_vec(&(
        "PUT",
        "/hssp/stop",
        Option::<&[u8]>::None,
    ))?);
    ensure!(
        record.stop && record.sequence > 0 && record.request_sha256 == expected,
        "emergency history requires an exact bodyless HSSP stop"
    );
    Ok(())
}
fn census(db: &Connection) -> Result<()> {
    let values = {
        let mut query =
            db.prepare("SELECT singleton,observation_count FROM emergency_stop_census LIMIT 2")?;
        let rows = query.query_map([], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    ensure!(
        values.len() == 1 && values[0].0 == 1 && values[0].1 <= CAPACITY,
        "emergency stop census missing or invalid"
    );
    let count: u64 = db.query_row(
        "SELECT count(*) FROM emergency_stop_observations",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count == values[0].1,
        "emergency stop observation is missing or unregistered"
    );
    Ok(())
}
fn row(db: &Connection, id: &str) -> Result<bool> {
    ensure!(valid_digest(id), "invalid emergency stop effect identity");
    let marker: Option<(String, String)> = db.query_row(
        "SELECT CASE WHEN length(outcome)<=16 THEN outcome ELSE NULL END,CASE WHEN length(provenance_sha256)=64 THEN provenance_sha256 ELSE NULL END FROM emergency_stop_observations WHERE effect_id=?1",
        [id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    let Some((outcome, sha)) = marker else {
        return Ok(false);
    };
    ensure!(
        matches!(outcome.as_str(), "acknowledged" | "unknown") && sha == provenance(id, &outcome)?,
        "emergency stop provenance differs"
    );
    let (device, text, recorded): (String,String,String) = db.query_row(
        "SELECT CASE WHEN length(CAST(device AS BLOB))<=128 THEN device ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(outcome)<=16 THEN outcome ELSE NULL END FROM effects WHERE id=?1",
        [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    )?;
    ensure!(
        digest(text.as_bytes()) == id && recorded == outcome,
        "emergency stop effect correspondence differs"
    );
    let record: EffectRecord = serde_json::from_str(&text)?;
    stop_shape(&record)?;
    ensure!(
        record.admission.configuration.device.as_str() == device,
        "emergency stop device differs"
    );
    ensure!(
        !super::play::linked(db, id)?,
        "emergency stop cannot be a play operation"
    );
    Ok(true)
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, expected) in [
        ("emergency_stop_observations", TABLE),
        ("emergency_stop_census", CENSUS),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if version < 7 {
            ensure!(
                actual.is_none(),
                "unexpected emergency stop journal before migration"
            );
        } else {
            ensure!(
                actual == Some(("table".into(), expected.into())),
                "emergency stop journal layout differs"
            );
        }
    }
    if version < 7 {
        return Ok(());
    }
    let triggers: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('emergency_stop_observations','emergency_stop_census')",[],|r|r.get(0))?;
    ensure!(triggers == 0, "unexpected emergency stop journal trigger");
    census(db)?;
    let mut query = db.prepare("SELECT CASE WHEN length(effect_id)=64 THEN effect_id ELSE NULL END FROM emergency_stop_observations")?;
    for id in query.query_map([], |r| r.get::<_, String>(0))? {
        ensure!(row(db, &id?)?, "emergency stop marker disappeared");
    }
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 7 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for sql in [
        TABLE,
        CENSUS,
        "INSERT INTO emergency_stop_census VALUES(1,0)",
        "PRAGMA user_version=7",
    ] {
        tx.execute_batch(sql)?;
    }
    tx.commit()?;
    Ok(())
}
impl DeviceEffectJournal {
    /// Known provenance only; absence does not prove that no emergency attempt occurred.
    pub(crate) fn emergency_stop_recorded(&self, id: &str) -> Result<bool> {
        self.check_owner()?;
        census(&self.db)?;
        row(&self.db, id)
    }
    /// Persist an already observed stop and its provenance together. This never
    /// returns a dispatch receipt or changes runtime or physical authority.
    pub(crate) fn try_record_emergency_stop(
        &mut self,
        intent: &EffectIntent,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        self.without_writer_wait(|journal| {
            journal.check_owner()?;
            stop_shape(&intent.record)?;
            let tx = journal.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            validate_schema(&tx,7)?;
            let id=intent.record_id()?;
            if super::reconciliation::resolved_keys(&tx)?.contains(&(CustodyKind::Effect,id.clone())) {
                ensure!(row(&tx,&id)?,"resolved effect cannot acquire new emergency provenance");
                let receipt=PreparedEffectReceipt {id,boot:journal.boot.clone()};
                Self::complete_in_transaction(&tx,&receipt,outcome)?;
                return Ok(());
            }
            let receipt = Self::prepare_in_transaction(&tx,&journal.boot,intent)?;
            ensure!(!super::play::linked(&tx,&receipt.id)?, "emergency stop cannot be a play operation");
            let name = outcome_name(outcome);
            let added = tx.execute("INSERT INTO emergency_stop_observations(effect_id,outcome,provenance_sha256) VALUES(?1,?2,?3) ON CONFLICT(effect_id) DO NOTHING",params![receipt.id,name,provenance(&receipt.id,name)?])?;
            if added == 1 {
                ensure!(tx.execute("UPDATE emergency_stop_census SET observation_count=observation_count+1 WHERE singleton=1 AND observation_count<1024",[])? == 1,"emergency stop census capacity exceeded");
            } else {
                ensure!(row(&tx,&receipt.id)?, "emergency stop marker missing");
            }
            Self::complete_in_transaction(&tx,&receipt,outcome)?;
            validate_schema(&tx,7)?;
            tx.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "device_emergency_stop_journal_tests.rs"]
mod tests;
