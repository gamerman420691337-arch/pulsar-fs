//! Bounded HSP history and single-use durable claims. History never restores
//! runtime authority, retries a request, or proves a device physically stopped.
use super::*;
use crate::handy_hsp_records::{
    HspEffectRecord, HspEffectRecordKind, HspOutcomeRecord, HspStreamRecord,
    HSP_EFFECT_RECORD_LIMIT, HSP_STREAM_RECORD_LIMIT,
};
use crate::handy_hsp_runtime::{HspCompletionRecord, HspEffect};
use std::collections::{BTreeMap, BTreeSet};
#[path = "device_hsp_preparation_journal.rs"]
pub(super) mod preparation;
#[path = "device_hsp_recovery_journal.rs"]
pub(super) mod recovery;
pub use preparation::{
    ClaimedHspPreparationReceipt, HspPreparationMembership, HspPreparationObservation,
    PreparedHspPreparationReceipt,
};
pub use recovery::{HspEmergencyReceipt, HspResolvedReceipt, HspStreamMembership};

#[cfg(test)]
#[path = "device_hsp_journal_tests.rs"]
mod tests;

const STREAM_CAPACITY: u64 = 64;
const ORDINARY_CAPACITY: u64 = 65_536;
const ORDINARY_BYTES_LIMIT: u64 = 128 * 1024 * 1024;
const OUTCOME_LIMIT: usize = 2048;
// Every prepared operation reserves both its immutable first outcome and one
// late annotation. Each stream independently reserves one full-size Stop.
const OUTCOME_RESERVATION: u64 = (2 * OUTCOME_LIMIT) as u64;
const STREAMS: &str = "CREATE TABLE hsp_streams(id TEXT PRIMARY KEY NOT NULL,intent TEXT NOT NULL,route TEXT NOT NULL,namespace TEXT NOT NULL,device TEXT NOT NULL,record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','custody','cancelled')))";
const INTENTS: &str = "CREATE UNIQUE INDEX hsp_stream_intent ON hsp_streams(intent)";
const EFFECTS: &str = "CREATE TABLE hsp_effects(id TEXT PRIMARY KEY NOT NULL,stream TEXT NOT NULL,sequence TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('setup','append','play','stop')),record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','claimed','acknowledged','unknown','cancelled')),outcome TEXT,outcome_sha256 TEXT,late TEXT,late_sha256 TEXT,lifecycle_sha256 TEXT NOT NULL)";
const SEQUENCES: &str = "CREATE UNIQUE INDEX hsp_effect_sequence ON hsp_effects(stream,sequence)";
const STOPS: &str = "CREATE UNIQUE INDEX hsp_stream_stop ON hsp_effects(stream) WHERE kind='stop'";
const COUNTERS: &str = "CREATE TABLE hsp_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),streams INTEGER NOT NULL,ordinary INTEGER NOT NULL,stops INTEGER NOT NULL,stream_bytes INTEGER NOT NULL,ordinary_bytes INTEGER NOT NULL,stop_bytes INTEGER NOT NULL)";
const OBJECTS: [(&str, &str, &str); 6] = [
    ("hsp_streams", "table", STREAMS),
    ("hsp_stream_intent", "index", INTENTS),
    ("hsp_effects", "table", EFFECTS),
    ("hsp_effect_sequence", "index", SEQUENCES),
    ("hsp_stream_stop", "index", STOPS),
    ("hsp_census", "table", COUNTERS),
];
const PROVENANCE_MARKER: &str = "CREATE TABLE hsp_provenance_format(singleton INTEGER PRIMARY KEY CHECK(singleton=1),format INTEGER NOT NULL CHECK(format=1))";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspJournalState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
impl HspJournalState {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "prepared" => Self::Prepared,
            "claimed" => Self::Claimed,
            "acknowledged" => Self::Acknowledged,
            "unknown" => Self::Unknown,
            "cancelled" => Self::Cancelled,
            _ => anyhow::bail!("invalid HSP effect state"),
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Claimed => "claimed",
            Self::Acknowledged => "acknowledged",
            Self::Unknown => "unknown",
            Self::Cancelled => "cancelled",
        }
    }
    fn attempted(self) -> bool {
        matches!(self, Self::Claimed | Self::Acknowledged | Self::Unknown)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspStreamJournalState {
    Prepared,
    Custody,
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct HspStreamObservation {
    pub id: String,
    pub record: HspStreamRecord,
    pub state: HspStreamJournalState,
    pub effect_ids: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct HspEffectObservation {
    pub id: String,
    pub record: HspEffectRecord,
    pub state: HspJournalState,
    pub outcome: Option<HspOutcomeRecord>,
    pub late_observation: Option<HspOutcomeRecord>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspStreamReceipt {
    id: String,
    boot: DeviceSessionId,
}
impl HspStreamReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedHspReceipt {
    id: String,
    stream_id: String,
    boot: DeviceSessionId,
}
impl PreparedHspReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimedHspReceipt {
    id: String,
    stream_id: String,
    boot: DeviceSessionId,
}
impl ClaimedHspReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}
/// Exact proof that this original operation was durably cancelled before any
/// native claim. This can retire that same runtime operation, never authorize
/// another operation or establish physical stopping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelledHspReceipt {
    id: String,
    stream_id: String,
    boot: DeviceSessionId,
    effect: HspEffect,
}
impl CancelledHspReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
    pub(crate) fn matches_effect(&self, effect: &HspEffect) -> bool {
        self.effect == *effect
    }
}
#[derive(Default, Debug, PartialEq, Eq)]
struct Census {
    streams: u64,
    ordinary: u64,
    stops: u64,
    stream_bytes: u64,
    ordinary_bytes: u64,
    stop_bytes: u64,
}
struct Catalog {
    links: BTreeMap<String, crate::handy_hsp_records::HspQualificationLinkRecord>,
    emergencies: BTreeMap<String, crate::handy_hsp_records::HspEmergencyRecord>,
    resolved: BTreeSet<String>,
    streams: BTreeMap<String, HspStreamObservation>,
    effects: BTreeMap<String, HspEffectObservation>,
    census: Census,
}
/// Validated history cached only for the exact SQLite connection state. The
/// cache is invalidated by external writes, internal writes outside this module,
/// schema changes, failed transactions, and restarts. It is never authority.
#[derive(Default)]
pub(super) struct ValidationCache {
    inner: std::cell::RefCell<Option<ValidationCacheEntry>>,
}
struct ValidationCacheEntry {
    stamp: (u64, u64, u64),
    catalog: Catalog,
}
fn stamp(db: &Connection) -> Result<(u64, u64, u64)> {
    Ok((
        db.query_row("PRAGMA data_version", [], |r| r.get(0))?,
        db.query_row("SELECT total_changes()", [], |r| r.get(0))?,
        db.query_row("PRAGMA schema_version", [], |r| r.get(0))?,
    ))
}
impl ValidationCache {
    fn take(&self, db: &Connection) -> Result<Catalog> {
        let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?;
        schema(db, version)?;
        let current = stamp(db)?;
        if let Some(prior) = self.inner.borrow_mut().take() {
            if prior.stamp == current {
                ensure!(
                    census(db)? == prior.catalog.census,
                    "HSP cached census differs"
                );
                recovery::validate_census(db, &prior.catalog)?;
                return Ok(prior.catalog);
            }
        }
        catalog(db)
    }
    fn keep(&self, stamp: (u64, u64, u64), value: Catalog) {
        *self.inner.borrow_mut() = Some(ValidationCacheEntry {
            stamp,
            catalog: value,
        });
    }
}
fn kind_name(kind: HspEffectRecordKind) -> &'static str {
    match kind {
        HspEffectRecordKind::Setup => "setup",
        HspEffectRecordKind::Append => "append",
        HspEffectRecordKind::Play => "play",
        HspEffectRecordKind::Stop => "stop",
    }
}
fn digest<T: Serialize>(domain: &str, value: &T) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(domain, value))?)
    ))
}
fn lifecycle(id: &str, state: &str, outcome: Option<&str>, late: Option<&str>) -> Result<String> {
    digest(
        "pulsar.hsp.journal.lifecycle.v1",
        &(id, state, outcome, late),
    )
}
fn outcome_text(outcome: &HspOutcomeRecord) -> Result<String> {
    outcome.validate()?;
    let text = serde_json::to_string(outcome)?;
    ensure!(text.len() <= OUTCOME_LIMIT, "HSP outcome exceeds bound");
    Ok(text)
}
fn outcome_hash(id: &str, text: &str, late: bool) -> Result<String> {
    digest("pulsar.hsp.journal.observation.v1", &(id, late, text))
}
fn checked_outcome(
    id: &str,
    text: Option<String>,
    hash: Option<String>,
    late: bool,
    effect: &HspEffectRecord,
) -> Result<Option<HspOutcomeRecord>> {
    match (text, hash) {
        (None, None) => Ok(None),
        (Some(text), Some(hash)) => {
            validate_sha256(&hash)?;
            ensure!(
                outcome_hash(id, &text, late)? == hash,
                "HSP observation integrity differs"
            );
            let value: HspOutcomeRecord = serde_json::from_str(&text)?;
            value.validate_for_effect(effect)?;
            ensure!(
                outcome_text(&value)? == text,
                "noncanonical HSP observation"
            );
            Ok(Some(value))
        }
        _ => anyhow::bail!("incomplete HSP observation"),
    }
}
fn owner(db: &Connection, boot: &DeviceSessionId) -> Result<()> {
    let current: String = db.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1", [], |r| r.get(0))?;
    ensure!(current == boot.as_str(), "retired HSP journal owner");
    Ok(())
}
fn schema(db: &Connection, version: u32) -> Result<()> {
    if version >= 10 {
        ensure!(
            matches!(version, 10..=13)
                && db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? == version,
            "HSP journal version changed"
        );
    }
    let temporary: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_temp_master WHERE lower(name) GLOB 'hsp_*' OR lower(tbl_name) GLOB 'hsp_*'",
        [],
        |r| r.get(0),
    )?;
    ensure!(temporary == 0, "unexpected temporary HSP journal object");
    let count: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE (lower(name) GLOB 'hsp_*' OR lower(tbl_name) GLOB 'hsp_*') AND sql IS NOT NULL", [], |r| r.get(0))?;
    ensure!(
        count
            == if version >= 10 {
                (OBJECTS.len()
                    + if version >= 11 {
                        recovery::OBJECTS.len()
                    } else {
                        0
                    }
                    + if version >= 12 {
                        preparation::OBJECTS.len()
                    } else {
                        0
                    }
                    + usize::from(version >= 13)) as u64
            } else {
                0
            },
        "unexpected HSP journal objects"
    );
    for (name, kind, sql) in OBJECTS {
        let actual: Option<(String, String)> = db.query_row("SELECT CASE WHEN length(type)<=16 THEN type ELSE NULL END,CASE WHEN length(CAST(sql AS BLOB))<=16384 THEN sql ELSE NULL END FROM sqlite_master WHERE name=?1", [name], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        ensure!(
            actual
                == if version >= 10 {
                    Some((kind.into(), sql.into()))
                } else {
                    None
                },
            "HSP journal schema differs"
        );
    }
    for (name, kind, sql) in recovery::OBJECTS {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        ensure!(
            actual
                == if version >= 11 {
                    Some((kind.into(), sql.into()))
                } else {
                    None
                },
            "HSP recovery schema differs"
        );
    }
    for (name, kind, sql) in preparation::OBJECTS {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        ensure!(
            actual
                == if version >= 12 {
                    Some((kind.into(), sql.into()))
                } else {
                    None
                },
            "HSP preparation schema differs"
        );
    }
    let marker: Option<(String, String)> = db
        .query_row(
            "SELECT type,sql FROM sqlite_master WHERE name='hsp_provenance_format'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    ensure!(
        marker
            == if version >= 13 {
                Some(("table".into(), PROVENANCE_MARKER.into()))
            } else {
                None
            },
        "HSP provenance schema marker differs"
    );
    if version >= 13 {
        let mut statement =
            db.prepare("SELECT singleton,format FROM hsp_provenance_format LIMIT 2")?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(rows == [(1, 1)], "HSP provenance format marker differs");
    }
    Ok(())
}

/// Called only within the original immediate write transaction, before its
/// first extended effect. Opening or reading a legacy journal never upgrades it.
fn require_provenance_schema(db: &Connection) -> Result<()> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    schema(db, version)?;
    ensure!(
        matches!(version, 12 | 13),
        "HSP provenance requires a current base journal"
    );
    if version == 12 {
        db.execute_batch(PROVENANCE_MARKER)?;
        db.execute_batch("INSERT INTO hsp_provenance_format VALUES(1,1);PRAGMA user_version=13;")?;
        schema(db, 13)?;
    }
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 12 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    schema(&tx, version)?;
    if version < 10 {
        for (_, _, sql) in OBJECTS {
            tx.execute_batch(sql)?;
        }
        tx.execute_batch("INSERT INTO hsp_census VALUES(1,0,0,0,0,0,0);")?;
    }
    if version < 11 {
        for (_, _, sql) in recovery::OBJECTS {
            tx.execute_batch(sql)?;
        }
        tx.execute_batch("INSERT INTO hsp_recovery_census VALUES(1,0,0,0,0);")?;
    }
    for (_, _, sql) in preparation::OBJECTS {
        tx.execute_batch(sql)?;
    }
    tx.execute_batch(
        "INSERT INTO hsp_preparation_census VALUES(1,0,0,0,0);PRAGMA user_version=12;",
    )?;
    validate_schema(&tx, 12)?;
    tx.commit()?;
    Ok(())
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    schema(db, version)?;
    if version >= 10 {
        catalog(db)?;
    }
    if version >= 12 {
        preparation::validate_catalog(db)?;
    }
    Ok(())
}
fn census(db: &Connection) -> Result<Census> {
    ensure!(
        db.query_row("SELECT count(*) FROM hsp_census", [], |r| r
            .get::<_, u64>(0))?
            == 1,
        "HSP census membership differs"
    );
    let value = db.query_row("SELECT streams,ordinary,stops,stream_bytes,ordinary_bytes,stop_bytes FROM hsp_census WHERE singleton=1", [], |r| Ok(Census { streams:r.get(0)?,ordinary:r.get(1)?,stops:r.get(2)?,stream_bytes:r.get(3)?,ordinary_bytes:r.get(4)?,stop_bytes:r.get(5)? }))?;
    ensure!(
        value.streams <= STREAM_CAPACITY
            && value.ordinary <= ORDINARY_CAPACITY
            && value.stops <= value.streams
            && value.stream_bytes <= STREAM_CAPACITY * HSP_STREAM_RECORD_LIMIT as u64
            && value.ordinary_bytes <= ORDINARY_BYTES_LIMIT
            && value.stop_bytes
                <= STREAM_CAPACITY * (HSP_EFFECT_RECORD_LIMIT as u64 + OUTCOME_RESERVATION),
        "HSP census exceeds capacity"
    );
    Ok(value)
}
fn save_census(db: &Connection, value: &Census) -> Result<()> {
    ensure!(db.execute("UPDATE hsp_census SET streams=?1,ordinary=?2,stops=?3,stream_bytes=?4,ordinary_bytes=?5,stop_bytes=?6 WHERE singleton=1", params![value.streams,value.ordinary,value.stops,value.stream_bytes,value.ordinary_bytes,value.stop_bytes])? == 1,"HSP census missing");
    Ok(())
}
fn catalog(db: &Connection) -> Result<Catalog> {
    let all = load_catalog(db)?;
    for stream in all
        .streams
        .values()
        .filter(|s| s.state != HspStreamJournalState::Cancelled && !all.resolved.contains(&s.id))
    {
        let parent = preparation::parent_for_stream(db, &stream.id)?;
        preparation::ensure_route_available(
            db,
            &stream.record.route_sha256,
            &stream.record.route_namespace_sha256,
            &stream.record.registration.device_id,
            parent.as_deref(),
        )?;
        super::reconciliation::ensure_no_legacy_custody(
            db,
            &stream.record.route_sha256,
            &stream.record.route_namespace_sha256,
            &stream.record.registration.device_id,
        )?;
    }
    Ok(all)
}
fn load_catalog(db: &Connection) -> Result<Catalog> {
    let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?;
    schema(db, version)?;
    let stored = census(db)?;
    // Count and byte census precede every row allocation. SQL CASE also bounds
    // individual strings, including corrupt rows inserted outside this API.
    let counts: (u64,u64,u64) = db.query_row("SELECT (SELECT count(*) FROM hsp_streams),(SELECT count(*) FROM hsp_effects),(SELECT coalesce(sum(length(CAST(record AS BLOB))),0) FROM hsp_effects)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(
        counts.0 <= STREAM_CAPACITY
            && counts.1 <= ORDINARY_CAPACITY + STREAM_CAPACITY
            && counts.2 <= ORDINARY_BYTES_LIMIT + STREAM_CAPACITY * HSP_EFFECT_RECORD_LIMIT as u64,
        "HSP journal exceeds capacity"
    );
    let invalid:u64=db.query_row("SELECT count(*) FROM hsp_effects WHERE (outcome IS NOT NULL AND (typeof(outcome)!='text' OR length(CAST(outcome AS BLOB))>2048)) OR (late IS NOT NULL AND (typeof(late)!='text' OR length(CAST(late AS BLOB))>2048))",[],|r|r.get(0))?;
    ensure!(invalid == 0, "HSP observation exceeds bound");
    let mut result = Catalog {
        links: BTreeMap::new(),
        emergencies: BTreeMap::new(),
        resolved: BTreeSet::new(),
        streams: BTreeMap::new(),
        effects: BTreeMap::new(),
        census: Census::default(),
    };
    let mut q = db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END,CASE WHEN length(intent)=64 THEN intent ELSE NULL END,CASE WHEN length(route)=64 THEN route ELSE NULL END,CASE WHEN length(namespace)=64 THEN namespace ELSE NULL END,CASE WHEN length(CAST(device AS BLOB)) BETWEEN 1 AND 128 THEN device ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END FROM hsp_streams")?;
    let rows = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (id, intent, route, namespace, device, text, state) = row?;
        let record = HspStreamRecord::from_json(text.as_bytes())?;
        ensure!(
            record.id()? == id
                && record.request_identity()? == intent
                && record.route_sha256 == route
                && record.route_namespace_sha256 == namespace
                && record.registration.device_id.as_str() == device
                && record.to_json()? == text.as_bytes(),
            "HSP stream identity or membership differs"
        );
        let state = match state.as_str() {
            "prepared" => HspStreamJournalState::Prepared,
            "custody" => HspStreamJournalState::Custody,
            "cancelled" => HspStreamJournalState::Cancelled,
            _ => anyhow::bail!("invalid HSP stream state"),
        };
        result.census.streams += 1;
        result.census.stream_bytes += text.len() as u64;
        result.streams.insert(
            id.clone(),
            HspStreamObservation {
                id,
                record,
                state,
                effect_ids: Vec::new(),
            },
        );
    }
    let mut q = db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END,CASE WHEN length(stream)=64 THEN stream ELSE NULL END,CASE WHEN length(sequence)=16 THEN sequence ELSE NULL END,CASE WHEN length(kind)<=8 THEN kind ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=65536 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END,CASE WHEN outcome IS NULL OR length(CAST(outcome AS BLOB))<=2048 THEN outcome ELSE NULL END,CASE WHEN outcome_sha256 IS NULL OR length(outcome_sha256)=64 THEN outcome_sha256 ELSE 'invalid' END,CASE WHEN late IS NULL OR length(CAST(late AS BLOB))<=2048 THEN late ELSE NULL END,CASE WHEN late_sha256 IS NULL OR length(late_sha256)=64 THEN late_sha256 ELSE 'invalid' END,CASE WHEN length(lifecycle_sha256)=64 THEN lifecycle_sha256 ELSE NULL END FROM hsp_effects ORDER BY stream,sequence")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let stream: String = row.get(1)?;
        let sequence: String = row.get(2)?;
        let kind: String = row.get(3)?;
        let text: String = row.get(4)?;
        let state: String = row.get(5)?;
        let raw: Option<String> = row.get(6)?;
        let raw_hash: Option<String> = row.get(7)?;
        let late: Option<String> = row.get(8)?;
        let late_hash: Option<String> = row.get(9)?;
        let row_hash: String = row.get(10)?;
        ensure!(
            lifecycle(&id, &state, raw.as_deref(), late.as_deref())? == row_hash,
            "HSP lifecycle integrity differs"
        );
        let parent = result
            .streams
            .get_mut(&stream)
            .ok_or_else(|| anyhow::anyhow!("orphan HSP effect"))?;
        let record = HspEffectRecord::from_json(text.as_bytes(), &parent.record)?;
        ensure!(
            version >= 13 || !record.requires_provenance_schema(),
            "HSP source or clock provenance requires journal version 13"
        );
        ensure!(
            record.id()? == id
                && record.stream_id == stream
                && sequence == format!("{:016x}", record.sequence)
                && kind == kind_name(record.kind)
                && record.to_json()? == text.as_bytes(),
            "HSP effect identity or membership differs"
        );
        let state = HspJournalState::parse(&state)?;
        let outcome = checked_outcome(&id, raw, raw_hash, false, &record)?;
        let late_observation = checked_outcome(&id, late, late_hash, true, &record)?;
        ensure!(
            matches!(
                (&outcome, state),
                (
                    None,
                    HspJournalState::Prepared
                        | HspJournalState::Claimed
                        | HspJournalState::Cancelled
                ) | (Some(HspOutcomeRecord::Unknown), HspJournalState::Unknown)
                    | (
                        Some(HspOutcomeRecord::Acknowledged(_)),
                        HspJournalState::Acknowledged
                    )
            ),
            "HSP outcome state differs"
        );
        ensure!(
            late_observation.is_none() || state == HspJournalState::Unknown,
            "HSP late observation requires original uncertainty"
        );
        let bytes = text.len() as u64 + OUTCOME_RESERVATION;
        if record.kind == HspEffectRecordKind::Stop {
            result.census.stops += 1;
            result.census.stop_bytes += bytes;
        } else {
            result.census.ordinary += 1;
            result.census.ordinary_bytes += bytes;
        }
        parent.effect_ids.push(id.clone());
        result.effects.insert(
            id.clone(),
            HspEffectObservation {
                id,
                record,
                state,
                outcome,
                late_observation,
            },
        );
    }
    ensure!(
        result.census == stored,
        "HSP census differs from retained records"
    );
    recovery::load(db, &mut result)?;
    for stream in result.streams.values() {
        validate_history(stream, &result.effects)?;
        for other in result
            .streams
            .values()
            .filter(|other| other.id != stream.id)
        {
            ensure!(
                other.record.binding != stream.record.binding,
                "HSP runtime binding has multiple original stream parents"
            );
        }
        if stream.state != HspStreamJournalState::Cancelled && !result.resolved.contains(&stream.id)
        {
            stream_available(&result, &stream.record, Some(&stream.id))?;
        }
    }
    Ok(result)
}
fn validate_history(
    stream: &HspStreamObservation,
    effects: &BTreeMap<String, HspEffectObservation>,
) -> Result<()> {
    let mut prior: Option<&HspEffectObservation> = None;
    let mut attempted = false;
    let mut all_cancelled = !stream.effect_ids.is_empty();
    let mut playing = false;
    let mut appended = false;
    let mut capacity = None;
    let mut tail = None;
    let mut chunk_sequence = 0u64;
    let mut total_points = 0u64;
    let mut previous_sample: Option<&crate::handy_hsp_records::HspSampleRecord> = None;
    let mut previous_source: Option<&crate::handy_hsp_records::HspSourceCursorRecord> = None;
    let mut source_epochs = BTreeSet::new();
    let mut source_attempts = BTreeSet::new();
    let mut clock_observations: BTreeMap<
        (String, u64),
        &crate::handy_hsp_records::HspClockObservationRecord,
    > = BTreeMap::new();
    let mut clock_owner: Option<&crate::handy_hsp_records::HspClockObservationRecord> = None;
    for id in &stream.effect_ids {
        let effect = effects
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("missing HSP effect membership"))?;
        let stop = effect.record.kind == HspEffectRecordKind::Stop;
        if let Some(clock) = effect
            .record
            .original_facts
            .as_ref()
            .and_then(|facts| facts.clock_observation.as_ref())
        {
            if let Some(original) = clock_owner {
                ensure!(
                    clock.owner_id == original.owner_id
                        && clock.original_server_mapping.domain
                            == original.original_server_mapping.domain
                        && clock.session_until_ns == original.session_until_ns
                        && clock.original_device_sync == original.original_device_sync
                        && clock.device_clock == original.device_clock
                        && clock.maximum_total_error_ns == original.maximum_total_error_ns,
                    "HSP clock refresh changed original owner or synchronization basis"
                );
            } else {
                clock_owner = Some(clock);
            }
            let key = (clock.owner_id.to_string(), clock.generation);
            if let Some(original) = clock_observations.get(&key) {
                ensure!(
                    clock.same_observation(original),
                    "HSP clock generation changed immutable observation"
                );
            } else {
                clock_observations.insert(key, clock);
            }
        } else if !stop {
            ensure!(
                clock_owner.is_none(),
                "HSP ordinary effect lost original clock provenance"
            );
        }
        ensure!(
            effect.record.sequence > prior.map_or(0, |p| p.record.sequence),
            "HSP sequence regressed"
        );
        if let Some(previous) = prior {
            ensure!(
                previous.record.kind != HspEffectRecordKind::Stop
                    && !matches!(
                        previous.state,
                        HspJournalState::Prepared | HspJournalState::Claimed
                    ),
                "HSP operation overlaps unsettled work"
            );
            if !stop {
                ensure!(
                    previous.state == HspJournalState::Acknowledged
                        && previous.record.sequence.checked_add(1) == Some(effect.record.sequence),
                    "HSP continuation follows uncertainty or skipped work"
                );
            }
        } else if !stop {
            ensure!(
                effect.record.kind == HspEffectRecordKind::Setup && effect.record.sequence == 1,
                "HSP stream must begin with original setup"
            );
        }
        match effect.record.kind {
            HspEffectRecordKind::Setup => ensure!(prior.is_none(), "duplicate HSP setup"),
            HspEffectRecordKind::Append => {
                ensure!(
                    prior.is_some() && effect.record.expected_running == playing,
                    "HSP append phase differs"
                );
                let chunk = effect
                    .record
                    .chunk
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing HSP append chunk"))?;
                ensure!(
                    chunk.sequence == chunk_sequence + 1
                        && u64::from(chunk.tail_point_stream_index) + 1
                            == total_points + chunk.samples.len() as u64,
                    "HSP original append sequence or cumulative tail differs"
                );
                if let (Some(before), Some(after)) = (previous_sample, chunk.samples.first()) {
                    let same_source = previous_source == chunk.source_cursor.as_ref();
                    if !same_source {
                        let old = previous_source.ok_or_else(|| {
                            anyhow::anyhow!(
                                "legacy HSP source cannot silently gain a successor cursor"
                            )
                        })?;
                        let new = chunk.source_cursor.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("HSP source cursor provenance was removed")
                        })?;
                        ensure!(playing && old.identity.epoch != new.identity.epoch && old.attempt_id != new.attempt_id
                            && after.source_sequence == 1
                            && source_epochs.insert(new.identity.epoch.to_string())
                            && source_attempts.insert(new.attempt_id.to_string()),
                            "HSP successor requires a new playing cursor and its first original sequence");
                    }
                    ensure!(
                        after.time_ms > before.time_ms
                            && after.captured_at_ns >= before.captured_at_ns
                            && after.accepted_at_ns >= before.accepted_at_ns
                            && (!same_source
                                || (after.source_sequence > before.source_sequence
                                    && after.continuous_from_previous)),
                        "HSP append continuity differs across original chunks"
                    );
                } else if let Some(source) = &chunk.source_cursor {
                    ensure!(
                        source.identity == stream.record.binding.identity.live,
                        "initial HSP cursor differs from stream origin"
                    );
                    source_epochs.insert(source.identity.epoch.to_string());
                    source_attempts.insert(source.attempt_id.to_string());
                }
                chunk_sequence = chunk.sequence;
                total_points += chunk.samples.len() as u64;
                previous_sample = chunk.samples.last();
                previous_source = chunk.source_cursor.as_ref();
                appended = true;
            }
            HspEffectRecordKind::Play => {
                ensure!(appended && !playing, "HSP play ordering differs");
                playing = true;
            }
            HspEffectRecordKind::Stop => {}
        }
        for observation in [&effect.outcome, &effect.late_observation] {
            if let Some(HspOutcomeRecord::Acknowledged(value)) = observation {
                if !stop {
                    ensure!(
                        capacity.is_none_or(|v| v == value.max_points),
                        "HSP device capacity changed"
                    );
                    capacity = Some(value.max_points);
                    if effect.record.kind == HspEffectRecordKind::Play {
                        ensure!(
                            tail == Some(value.tail_point_stream_index),
                            "HSP play tail differs from original append history"
                        );
                    }
                }
                if effect.record.kind == HspEffectRecordKind::Append {
                    tail = Some(value.tail_point_stream_index);
                }
            }
        }
        attempted |= effect.state.attempted();
        all_cancelled &= effect.state == HspJournalState::Cancelled;
        prior = Some(effect);
    }
    let expected = if attempted {
        HspStreamJournalState::Custody
    } else if all_cancelled {
        HspStreamJournalState::Cancelled
    } else {
        HspStreamJournalState::Prepared
    };
    ensure!(
        stream.state == expected
            || (stream.effect_ids.is_empty() && stream.state == HspStreamJournalState::Cancelled),
        "HSP stream custody differs from effect membership"
    );
    Ok(())
}
fn stream_available(
    catalog: &Catalog,
    record: &HspStreamRecord,
    allowed: Option<&str>,
) -> Result<()> {
    for stream in catalog.streams.values() {
        if stream.state == HspStreamJournalState::Cancelled
            || catalog.resolved.contains(&stream.id)
            || allowed == Some(stream.id.as_str())
        {
            continue;
        }
        ensure!(
            stream.record.route_namespace_sha256 == record.route_namespace_sha256,
            "HSP namespace changed with unresolved custody"
        );
        ensure!(
            stream.record.route_sha256 != record.route_sha256
                && stream.record.registration.device_id != record.registration.device_id,
            "HSP route or device has unresolved custody"
        );
    }
    Ok(())
}
/// Registration mutation cannot ignore HSP reservations or unresolved claims.
pub(super) fn has_device_custody(db: &Connection, device: &DeviceId) -> Result<bool> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 10 {
        schema(db, version)?;
        return Ok(false);
    }
    let all = catalog(db)?;
    Ok(all.streams.values().any(|stream| {
        stream.state != HspStreamJournalState::Cancelled
            && !all.resolved.contains(&stream.id)
            && &stream.record.registration.device_id == device
    }))
}
pub(super) fn ensure_legacy_route_available(
    db: &Connection,
    route: &str,
    namespace: &str,
    device: &DeviceId,
) -> Result<()> {
    validate_sha256(route)?;
    validate_sha256(namespace)?;
    preparation::ensure_route_available(db, route, namespace, device, None)?;
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 10 {
        schema(db, version)?;
        return Ok(());
    }
    let all = catalog(db)?;
    for stream in all
        .streams
        .values()
        .filter(|s| s.state != HspStreamJournalState::Cancelled && !all.resolved.contains(&s.id))
    {
        ensure!(
            stream.record.route_namespace_sha256 == namespace,
            "legacy route blocked by unresolved foreign HSP namespace"
        );
        ensure!(
            stream.record.route_sha256 != route && &stream.record.registration.device_id != device,
            "legacy route blocked by unresolved HSP custody"
        );
    }
    Ok(())
}
fn prepare_stream(
    db: &Connection,
    boot: &DeviceSessionId,
    record: &HspStreamRecord,
    all: &mut Catalog,
    allowed_preparation: Option<&str>,
) -> Result<HspStreamReceipt> {
    owner(db, boot)?;
    record.validate()?;
    preparation::ensure_route_available(
        db,
        &record.route_sha256,
        &record.route_namespace_sha256,
        &record.registration.device_id,
        allowed_preparation,
    )?;
    ensure!(&record.boot == boot, "retired HSP stream intent");
    let id = record.id()?;
    ensure!(
        !all.resolved.contains(&id),
        "resolved HSP stream cannot be prepared again"
    );
    if let Some(prior) = all.streams.get(&id) {
        ensure!(
            prior.record.to_json()? == record.to_json()?,
            "HSP stream replay differs"
        );
        return Ok(HspStreamReceipt {
            id,
            boot: boot.clone(),
        });
    }
    let request_identity = record.request_identity()?;
    for stream in all.streams.values() {
        ensure!(
            stream.record.request_identity()? != request_identity,
            "HSP request identity already bound to different original stream"
        );
        ensure!(
            stream.record.binding != record.binding,
            "HSP runtime binding already belongs to another original stream"
        );
    }
    stream_available(all, record, None)?;
    super::reconciliation::ensure_no_legacy_custody(
        db,
        &record.route_sha256,
        &record.route_namespace_sha256,
        &record.registration.device_id,
    )?;
    ensure!(
        all.census.streams < STREAM_CAPACITY,
        "HSP stream capacity reached"
    );
    let text = String::from_utf8(record.to_json()?)?;
    db.execute(
        "INSERT INTO hsp_streams VALUES(?1,?2,?3,?4,?5,?6,'prepared')",
        params![
            id,
            record.request_identity()?,
            record.route_sha256,
            record.route_namespace_sha256,
            record.registration.device_id.as_str(),
            text
        ],
    )?;
    all.census.streams += 1;
    all.census.stream_bytes += text.len() as u64;
    save_census(db, &all.census)?;
    all.streams.insert(
        id.clone(),
        HspStreamObservation {
            id: id.clone(),
            record: record.clone(),
            state: HspStreamJournalState::Prepared,
            effect_ids: Vec::new(),
        },
    );
    Ok(HspStreamReceipt {
        id,
        boot: boot.clone(),
    })
}
fn prepare_effect(
    db: &Connection,
    boot: &DeviceSessionId,
    receipt: &HspStreamReceipt,
    effect: &HspEffect,
    all: &mut Catalog,
) -> Result<PreparedHspReceipt> {
    owner(db, boot)?;
    ensure!(
        !all.resolved.contains(&receipt.id),
        "resolved HSP stream cannot gain effects"
    );
    ensure!(&receipt.boot == boot, "retired HSP stream receipt");
    let stream = all
        .streams
        .get(&receipt.id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
    ensure!(&stream.record.boot == boot, "retired HSP stream");
    let record = HspEffectRecord::capture(&receipt.id, &stream.record, effect)?;
    if record.requires_provenance_schema() {
        require_provenance_schema(db)?;
    }
    let id = record.id()?;
    let result = PreparedHspReceipt {
        id: id.clone(),
        stream_id: receipt.id.clone(),
        boot: boot.clone(),
    };
    if let Some(prior) = all.effects.get(&id) {
        ensure!(
            prior.record.to_json()? == record.to_json()?,
            "HSP effect replay differs"
        );
        return Ok(result);
    }
    ensure!(
        stream.state != HspStreamJournalState::Cancelled,
        "cancelled HSP stream cannot regain authority"
    );
    stream_available(all, &stream.record, Some(&receipt.id))?;
    super::reconciliation::ensure_no_legacy_custody(
        db,
        &stream.record.route_sha256,
        &stream.record.route_namespace_sha256,
        &stream.record.registration.device_id,
    )?;
    let text = String::from_utf8(record.to_json()?)?;
    let cost = text.len() as u64 + OUTCOME_RESERVATION;
    if record.kind == HspEffectRecordKind::Stop {
        ensure!(
            !stream
                .effect_ids
                .iter()
                .any(|id| all.effects[id].record.kind == HspEffectRecordKind::Stop),
            "HSP stop already retained"
        );
    }
    reserve_effect(&mut all.census, record.kind, cost)?;
    let hash = lifecycle(&id, "prepared", None, None)?;
    db.execute(
        "INSERT INTO hsp_effects VALUES(?1,?2,?3,?4,?5,'prepared',NULL,NULL,NULL,NULL,?6)",
        params![
            id,
            receipt.id,
            format!("{:016x}", record.sequence),
            kind_name(record.kind),
            text,
            hash
        ],
    )?;
    save_census(db, &all.census)?;
    all.effects.insert(
        id.clone(),
        HspEffectObservation {
            id: id.clone(),
            record,
            state: HspJournalState::Prepared,
            outcome: None,
            late_observation: None,
        },
    );
    let stream = all
        .streams
        .get_mut(&receipt.id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
    stream.effect_ids.push(id);
    validate_history(stream, &all.effects)?;
    Ok(result)
}
fn reserve_effect(census: &mut Census, kind: HspEffectRecordKind, cost: u64) -> Result<()> {
    ensure!(
        cost <= HSP_EFFECT_RECORD_LIMIT as u64 + OUTCOME_RESERVATION,
        "HSP effect reserve exceeds bound"
    );
    if kind == HspEffectRecordKind::Stop {
        ensure!(census.stops < census.streams, "HSP Stop reserve exhausted");
        census.stops += 1;
        census.stop_bytes += cost;
    } else {
        ensure!(
            census.ordinary < ORDINARY_CAPACITY
                && census
                    .ordinary_bytes
                    .checked_add(cost)
                    .is_some_and(|n| n <= ORDINARY_BYTES_LIMIT),
            "HSP ordinary capacity reached; Stop reserve retained"
        );
        census.ordinary += 1;
        census.ordinary_bytes += cost;
    }
    Ok(())
}
fn set_effect(
    db: &Connection,
    id: &str,
    state: HspJournalState,
    outcome: Option<&HspOutcomeRecord>,
    late: Option<&HspOutcomeRecord>,
) -> Result<()> {
    let text = outcome.map(outcome_text).transpose()?;
    let late = late.map(outcome_text).transpose()?;
    let sha = text
        .as_deref()
        .map(|t| outcome_hash(id, t, false))
        .transpose()?;
    let late_sha = late
        .as_deref()
        .map(|t| outcome_hash(id, t, true))
        .transpose()?;
    let hash = lifecycle(id, state.name(), text.as_deref(), late.as_deref())?;
    ensure!(db.execute("UPDATE hsp_effects SET state=?1,outcome=?2,outcome_sha256=?3,late=?4,late_sha256=?5,lifecycle_sha256=?6 WHERE id=?7",params![state.name(),text,sha,late,late_sha,hash,id])?==1,"missing HSP effect");
    Ok(())
}
impl DeviceEffectJournal {
    /// Native pump spelling; all HSP writes refuse writer contention promptly.
    pub fn try_claim_hsp(
        &mut self,
        receipt: &PreparedHspReceipt,
        effect: &HspEffect,
    ) -> Result<ClaimedHspReceipt> {
        self.claim_hsp(receipt, effect)
    }
    pub fn try_complete_hsp(
        &mut self,
        receipt: &ClaimedHspReceipt,
        completion: &HspCompletionRecord,
    ) -> Result<HspEffectObservation> {
        self.complete_hsp(receipt, completion)
    }
    pub fn try_prepare_hsp_effect(
        &mut self,
        receipt: &HspStreamReceipt,
        effect: &HspEffect,
    ) -> Result<PreparedHspReceipt> {
        self.prepare_hsp_effect(receipt, effect)
    }
    pub fn try_prepare_hsp_stream_effect(
        &mut self,
        record: &HspStreamRecord,
        effect: &HspEffect,
    ) -> Result<(HspStreamReceipt, PreparedHspReceipt)> {
        self.prepare_hsp_stream_effect(record, effect)
    }
    fn hsp_transaction<T>(
        &mut self,
        operation: impl FnOnce(&Connection, &DeviceSessionId, &mut Catalog) -> Result<T>,
    ) -> Result<T> {
        self.without_writer_wait(|journal| {
            let tx = journal
                .db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            owner(&tx, &journal.boot)?;
            let mut all = journal.hsp_cache.take(&tx)?;
            let value = operation(&tx, &journal.boot, &mut all)?;
            ensure!(census(&tx)? == all.census, "HSP transaction census differs");
            recovery::validate_census(&tx, &all)?;
            let version = stamp(&tx)?;
            tx.commit()?;
            journal.hsp_cache.keep(version, all);
            Ok(value)
        })
    }
    fn hsp_read<T>(&self, operation: impl FnOnce(&Catalog) -> Result<T>) -> Result<T> {
        let tx = self.db.unchecked_transaction()?;
        let initial_version = stamp(&tx)?;
        owner(&tx, &self.boot)?;
        let all = self.hsp_cache.take(&tx)?;
        let value = operation(&all)?;
        let version = stamp(&tx)?;
        ensure!(
            version == initial_version,
            "HSP read snapshot changed during validation"
        );
        tx.commit()?;
        self.hsp_cache.keep(version, all);
        Ok(value)
    }
    pub fn prepare_hsp_stream(&mut self, record: &HspStreamRecord) -> Result<HspStreamReceipt> {
        self.hsp_transaction(|db, boot, all| prepare_stream(db, boot, record, all, None))
    }
    pub fn prepare_hsp_stream_effect(
        &mut self,
        record: &HspStreamRecord,
        effect: &HspEffect,
    ) -> Result<(HspStreamReceipt, PreparedHspReceipt)> {
        self.hsp_transaction(|db, boot, all| {
            let stream = prepare_stream(db, boot, record, all, None)?;
            let effect = prepare_effect(db, boot, &stream, effect, all)?;
            Ok((stream, effect))
        })
    }
    pub fn prepare_hsp_effect(
        &mut self,
        receipt: &HspStreamReceipt,
        effect: &HspEffect,
    ) -> Result<PreparedHspReceipt> {
        self.hsp_transaction(|db, boot, all| prepare_effect(db, boot, receipt, effect, all))
    }
    /// Read-only cross-store witnesses. Absence before the first native prepare
    /// is legitimate; records returned here are history, never new receipts.
    pub(crate) fn retained_hsp_stream_records(
        state_dir: &std::path::Path,
    ) -> Result<
        Vec<(
            HspStreamRecord,
            Option<crate::handy_hsp_records::HspQualificationLinkRecord>,
        )>,
    > {
        let path = state_dir.join("device-effects/journal.sqlite3");
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let _held = crate::artifacts::open_regular(&path)?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        let tx = db.unchecked_transaction()?;
        let version = Self::validate_existing(&tx)?;
        if version < 10 {
            tx.commit()?;
            return Ok(Vec::new());
        }
        let count: u64 = tx.query_row("SELECT count(*) FROM hsp_streams", [], |row| row.get(0))?;
        ensure!(
            count <= STREAM_CAPACITY,
            "HSP stream witnesses exceed bound"
        );
        let records = {
            let mut query = tx.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END, CASE WHEN length(intent)=64 THEN intent ELSE NULL END, CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END FROM hsp_streams")?;
            let mut records = Vec::new();
            for row in query.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })? {
                let (id, intent, raw) = row?;
                let record = HspStreamRecord::from_json(raw.as_bytes())?;
                ensure!(
                    record.id()? == id
                        && record.request_identity()? == intent
                        && record.to_json()? == raw.as_bytes(),
                    "HSP stream witness identity differs"
                );
                let link = if version >= 11 {
                    let raw: Option<(String,String)> = tx.query_row(
                        "SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END, CASE WHEN length(CAST(record AS BLOB))<=4096 THEN record ELSE NULL END FROM hsp_qualification_links WHERE stream=?1",
                        [&id], |row| Ok((row.get(0)?,row.get(1)?))).optional()?;
                    raw.map(|(id,raw)| -> Result<crate::handy_hsp_records::HspQualificationLinkRecord> {
                        let link: crate::handy_hsp_records::HspQualificationLinkRecord = serde_json::from_str(&raw)?;
                        link.validate(&record)?;
                        ensure!(link.id()? == id && serde_json::to_string(&link)? == raw,
                            "HSP source witness qualification link differs");
                        Ok(link)
                    }).transpose()?
                } else {
                    None
                };
                records.push((record, link));
            }
            records
        };
        tx.commit()?;
        Ok(records)
    }
    pub fn hsp_stream(&self, id: &str) -> Result<HspStreamObservation> {
        validate_sha256(id)?;
        self.hsp_read(|all| {
            all.streams
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))
        })
    }
    pub fn hsp_effect(&self, id: &str) -> Result<HspEffectObservation> {
        validate_sha256(id)?;
        self.hsp_read(|all| {
            all.effects
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))
        })
    }
    pub fn claim_hsp(
        &mut self,
        receipt: &PreparedHspReceipt,
        effect: &HspEffect,
    ) -> Result<ClaimedHspReceipt> {
        self.hsp_transaction(|db, boot, all| {
            ensure!(&receipt.boot == boot, "retired HSP effect receipt");
            ensure!(
                !all.resolved.contains(&receipt.stream_id),
                "resolved HSP effect cannot regain claim"
            );
            ensure!(
                !all.emergencies.contains_key(&receipt.stream_id),
                "HSP emergency history forbids another durable claim"
            );
            let prior = all
                .effects
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?;
            let stream = all
                .streams
                .get(&receipt.stream_id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            ensure!(
                prior.record.stream_id == receipt.stream_id
                    && &stream.record.boot == boot
                    && prior.state == HspJournalState::Prepared,
                "HSP claim already consumed or retired"
            );
            prior.record.matches_effect(&stream.record, effect)?;
            stream_available(all, &stream.record, Some(&receipt.stream_id))?;
            super::reconciliation::ensure_no_legacy_custody(
                db,
                &stream.record.route_sha256,
                &stream.record.route_namespace_sha256,
                &stream.record.registration.device_id,
            )?;
            if prior.record.kind == HspEffectRecordKind::Setup {
                preparation::transfer_at_setup_claim(db, &stream.record, &prior.record)?;
            }
            set_effect(db, &receipt.id, HspJournalState::Claimed, None, None)?;
            db.execute(
                "UPDATE hsp_streams SET state='custody' WHERE id=?1",
                [&receipt.stream_id],
            )?;
            all.effects
                .get_mut(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?
                .state = HspJournalState::Claimed;
            let stream = all
                .streams
                .get_mut(&receipt.stream_id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            stream.state = HspStreamJournalState::Custody;
            validate_history(stream, &all.effects)?;
            Ok(ClaimedHspReceipt {
                id: receipt.id.clone(),
                stream_id: receipt.stream_id.clone(),
                boot: boot.clone(),
            })
        })
    }
    pub fn complete_hsp(
        &mut self,
        receipt: &ClaimedHspReceipt,
        completion: &HspCompletionRecord,
    ) -> Result<HspEffectObservation> {
        self.hsp_transaction(|db, boot, all| {
            ensure!(&receipt.boot == boot, "retired HSP claim");
            ensure!(
                !all.resolved.contains(&receipt.stream_id),
                "resolved HSP observations are immutable"
            );
            let prior = all
                .effects
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?;
            let stream = all
                .streams
                .get(&receipt.stream_id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            ensure!(
                prior.record.stream_id == receipt.stream_id && &stream.record.boot == boot,
                "HSP completion membership differs"
            );
            prior
                .record
                .matches_effect(&stream.record, &completion.effect)?;
            let outcome = HspOutcomeRecord::capture(&completion.outcome)?;
            outcome.validate_for_effect(&prior.record)?;
            let late = completion
                .late_observation
                .as_ref()
                .map(HspOutcomeRecord::capture)
                .transpose()?;
            if let Some(late) = &late {
                late.validate_for_effect(&prior.record)?;
            }
            ensure!(
                late.is_none() || outcome == HspOutcomeRecord::Unknown,
                "late HSP observation cannot replace acknowledgment"
            );
            if prior.state != HspJournalState::Claimed {
                ensure!(
                    prior.outcome.as_ref() == Some(&outcome),
                    "HSP first completion is immutable"
                );
                if prior.late_observation.is_some() {
                    ensure!(
                        prior.late_observation == late || late.is_none(),
                        "HSP late annotation is immutable"
                    );
                }
            } else {
                ensure!(
                    prior.outcome.is_none(),
                    "claimed HSP effect already has outcome"
                );
            }
            ensure!(
                matches!(
                    prior.state,
                    HspJournalState::Claimed
                        | HspJournalState::Acknowledged
                        | HspJournalState::Unknown
                ),
                "unclaimed HSP effect cannot complete"
            );
            let retained_late = prior.late_observation.as_ref().or(late.as_ref()).cloned();
            let state = if outcome == HspOutcomeRecord::Unknown {
                HspJournalState::Unknown
            } else {
                HspJournalState::Acknowledged
            };
            set_effect(
                db,
                &receipt.id,
                state,
                Some(&outcome),
                retained_late.as_ref(),
            )?;
            let result = HspEffectObservation {
                id: prior.id.clone(),
                record: prior.record.clone(),
                state,
                outcome: Some(outcome),
                late_observation: retained_late,
            };
            all.effects.insert(receipt.id.clone(), result.clone());
            validate_history(stream, &all.effects)?;
            Ok(result)
        })
    }
    /// Release only a stream that has never handed any operation to transport.
    pub fn cancel_hsp_stream(&mut self, receipt: &HspStreamReceipt) -> Result<()> {
        self.hsp_transaction(|db, boot, all| {
            ensure!(&receipt.boot == boot, "retired HSP stream receipt");
            ensure!(
                !all.resolved.contains(&receipt.id),
                "resolved HSP stream cannot be cancelled"
            );
            let stream = all
                .streams
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            ensure!(
                &stream.record.boot == boot
                    && stream
                        .effect_ids
                        .iter()
                        .all(|id| !all.effects[id].state.attempted()),
                "attempted HSP stream cannot be released locally"
            );
            for id in &stream.effect_ids {
                set_effect(db, id, HspJournalState::Cancelled, None, None)?;
                all.effects
                    .get_mut(id)
                    .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?
                    .state = HspJournalState::Cancelled;
            }
            db.execute(
                "UPDATE hsp_streams SET state='cancelled' WHERE id=?1",
                [&receipt.id],
            )?;
            let stream = all
                .streams
                .get_mut(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            stream.state = HspStreamJournalState::Cancelled;
            validate_history(stream, &all.effects)?;
            Ok(())
        })
    }
    /// Cancel the exact definitely-unsent operation and return sealed proof for
    /// the runtime. A transport rejection by itself cannot mint this receipt.
    pub fn try_cancel_hsp_effect(
        &mut self,
        receipt: &PreparedHspReceipt,
        effect: &HspEffect,
    ) -> Result<CancelledHspReceipt> {
        self.hsp_transaction(|db, boot, all| {
            ensure!(&receipt.boot == boot, "retired HSP effect receipt");
            ensure!(
                !all.resolved.contains(&receipt.stream_id),
                "resolved HSP effect cannot regain claim"
            );
            let prior = all
                .effects
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?;
            let stream = all
                .streams
                .get(&receipt.stream_id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
            ensure!(
                prior.record.stream_id == receipt.stream_id && &stream.record.boot == boot,
                "HSP cancellation membership differs"
            );
            prior.record.matches_effect(&stream.record, effect)?;
            cancel_prepared_effect(db, boot, receipt, all)?;
            Ok(CancelledHspReceipt {
                id: receipt.id.clone(),
                stream_id: receipt.stream_id.clone(),
                boot: boot.clone(),
                effect: effect.clone(),
            })
        })
    }
    pub fn cancel_hsp_effect(&mut self, receipt: &PreparedHspReceipt) -> Result<()> {
        self.hsp_transaction(|db, boot, all| cancel_prepared_effect(db, boot, receipt, all))
    }
}
fn cancel_prepared_effect(
    db: &Connection,
    boot: &DeviceSessionId,
    receipt: &PreparedHspReceipt,
    all: &mut Catalog,
) -> Result<()> {
    ensure!(&receipt.boot == boot, "retired HSP effect receipt");
    ensure!(
        !all.resolved.contains(&receipt.stream_id),
        "resolved HSP effect cannot be cancelled"
    );
    let prior = all
        .effects
        .get(&receipt.id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?;
    ensure!(
        prior.record.stream_id == receipt.stream_id
            && matches!(
                prior.state,
                HspJournalState::Prepared | HspJournalState::Cancelled
            ),
        "attempted HSP effect cannot be cancelled locally"
    );
    set_effect(db, &receipt.id, HspJournalState::Cancelled, None, None)?;
    all.effects
        .get_mut(&receipt.id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?
        .state = HspJournalState::Cancelled;
    let stream = all
        .streams
        .get_mut(&receipt.stream_id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
    if stream
        .effect_ids
        .iter()
        .all(|id| all.effects[id].state == HspJournalState::Cancelled)
    {
        db.execute(
            "UPDATE hsp_streams SET state='cancelled' WHERE id=?1",
            [&receipt.stream_id],
        )?;
        stream.state = HspStreamJournalState::Cancelled;
    }
    validate_history(stream, &all.effects)?;
    Ok(())
}
