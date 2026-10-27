//! Immutable custody resolution. Records are historical, never executable stopping proof.
use super::*;
use std::collections::BTreeSet;

#[path = "device_reconciliation_journal_snapshot.rs"]
mod snapshot;
use snapshot::{Catalog, MEMBER_LIMIT};
pub(crate) use snapshot::{CustodyKind, CustodyMember, CustodySnapshot};
const RESOLUTION_LIMIT: usize = 16 * 1024 * 1024;
const TABLE:&str="CREATE TABLE custody_resolutions(id TEXT PRIMARY KEY NOT NULL,actor TEXT NOT NULL,request TEXT NOT NULL,record TEXT NOT NULL)";
const REQUEST_INDEX: &str =
    "CREATE UNIQUE INDEX custody_resolution_request ON custody_resolutions(actor,request)";
const MEMBERS:&str="CREATE TABLE custody_resolution_members(kind TEXT NOT NULL CHECK(kind IN ('setup','clock_sync','play','effect')),id TEXT NOT NULL,resolution TEXT NOT NULL,record_sha256 TEXT NOT NULL,observation_sha256 TEXT NOT NULL,membership_sha256 TEXT NOT NULL,PRIMARY KEY(kind,id))";
const CENSUS:&str="CREATE TABLE custody_resolution_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),resolution_count INTEGER NOT NULL CHECK(resolution_count BETWEEN 0 AND 1024),member_count INTEGER NOT NULL CHECK(member_count BETWEEN 0 AND 4096))";
const SETUP_ROUTES:&str="CREATE TABLE setup_custody_routes(route TEXT PRIMARY KEY NOT NULL,setup_id TEXT NOT NULL UNIQUE)";
const SYNC_ROUTES:&str="CREATE TABLE clock_sync_custody_routes(route TEXT PRIMARY KEY NOT NULL,sync_id TEXT NOT NULL UNIQUE)";
pub(super) const SETUP_HISTORY_INDEX: &str =
    "CREATE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'";
pub(super) const SYNC_HISTORY_INDEX:&str="CREATE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown')";
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CustodyResolutionRecord {
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub request_fingerprint: String,
    pub plan: HandyRecoveryPlan,
    pub basis: CustodySnapshot,
    pub snapshot: CustodySnapshot,
    pub evidence: HandyRecoveryEvidence,
}
impl CustodyResolutionRecord {
    fn validate(&self) -> Result<()> {
        SessionId::new(self.actor_session.as_str())?;
        RequestId::new(self.request_id.as_str())?;
        validate_sha256(&self.request_fingerprint)?;
        self.plan.validate()?;
        self.basis.validate()?;
        self.snapshot.validate()?;
        self.basis
            .validate_transport(&self.plan.contract.transport)?;
        self.snapshot
            .validate_transport(&self.plan.contract.transport)?;
        self.basis.extends_with_stops(&self.snapshot)?;
        self.basis.historical_qualifications()?;
        self.snapshot.historical_qualifications()?;
        ensure!(
            self.plan.plan_id == self.plan.identity()?
                && self.plan.snapshot_id.as_str() == self.basis.id()?
                && self.basis.device == self.plan.selection.device_id
                && self.basis.route_sha256 == self.plan.route_sha256
                && self.basis.route_namespace_sha256 == self.plan.route_namespace_sha256
                && self.plan.counts == self.basis.counts()
                && self.plan.projects == self.basis.projects()
                && self.plan.devices == self.basis.devices(),
            "resolution plan and original custody differ"
        );
        self.evidence.validate(&self.plan.contract)?;
        Ok(())
    }
    pub(crate) fn id(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() <= RESOLUTION_LIMIT,
            "custody resolution exceeds record bound"
        );
        Ok(digest(&bytes))
    }
}
fn membership_digest(resolution: &str, member: &CustodyMember) -> Result<String> {
    Ok(digest(&serde_json::to_vec(&(
        "custody-membership-v1",
        resolution,
        member.kind,
        &member.id,
        &member.record_sha256,
        &member.observation_sha256,
    ))?))
}
fn census(db: &Connection) -> Result<(usize, usize)> {
    let mut q = db.prepare(
        "SELECT singleton,resolution_count,member_count FROM custody_resolution_census LIMIT 2",
    )?;
    let values = q
        .query_map([], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        values.len() == 1
            && values[0].0 == 1
            && values[0].1 <= CAPACITY
            && values[0].2 <= MEMBER_LIMIT as u64,
        "custody resolution census is missing or invalid"
    );
    let (records,members):(u64,u64)=db.query_row("SELECT (SELECT count(*) FROM custody_resolutions),(SELECT count(*) FROM custody_resolution_members)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        records == values[0].1 && members == values[0].2,
        "custody resolution or membership is missing"
    );
    Ok((records as usize, members as usize))
}
fn record_row(db: &Connection, id: &str, catalog: &Catalog) -> Result<CustodyResolutionRecord> {
    validate_sha256(id)?;
    let (actor,request,text):(String,String,String)=db.query_row("SELECT CASE WHEN length(CAST(actor AS BLOB))<=128 THEN actor ELSE NULL END,CASE WHEN length(CAST(request AS BLOB))<=128 THEN request ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM custody_resolutions WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(
        digest(text.as_bytes()) == id,
        "custody resolution integrity differs"
    );
    let record: CustodyResolutionRecord = serde_json::from_str(&text)?;
    record.validate()?;
    ensure!(
        record.id()? == id
            && record.actor_session.as_str() == actor
            && record.request_id.as_str() == request,
        "custody resolution request differs"
    );
    for member in &record.snapshot.members {
        ensure!(
            catalog.current(&member.key())? == member,
            "resolved original custody facts changed"
        );
        let (resolution,record_sha,observation_sha,sha):(String,String,String,String)=db.query_row("SELECT CASE WHEN length(resolution)=64 THEN resolution ELSE NULL END,CASE WHEN length(record_sha256)=64 THEN record_sha256 ELSE NULL END,CASE WHEN length(observation_sha256)=64 THEN observation_sha256 ELSE NULL END,CASE WHEN length(membership_sha256)=64 THEN membership_sha256 ELSE NULL END FROM custody_resolution_members WHERE kind=?1 AND id=?2",params![member.kind.name(),member.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        ensure!(
            resolution == id
                && record_sha == member.record_sha256
                && observation_sha == member.observation_sha256
                && sha == membership_digest(id, member)?,
            "custody resolution membership differs"
        );
    }
    super::hsp::recovery::verify_resolution_members(db, id, &record.snapshot.hsp_streams)?;
    super::hsp::preparation::verify_resolution_members(db, id, &record.snapshot.hsp_preparations)?;
    let count: usize = db.query_row(
        "SELECT count(*) FROM custody_resolution_members WHERE resolution=?1",
        [id],
        |r| r.get(0),
    )?;
    ensure!(
        count == record.snapshot.members.len(),
        "custody resolution contains unexpected membership"
    );
    Ok(record)
}
pub(super) fn resolved_keys(db: &Connection) -> Result<BTreeSet<(CustodyKind, String)>> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 8 {
        return Ok(BTreeSet::new());
    }
    validate_schema(db, version)?;
    raw_keys(db)
}
fn raw_keys(db: &Connection) -> Result<BTreeSet<(CustodyKind, String)>> {
    let mut q = db.prepare("SELECT kind,id FROM custody_resolution_members LIMIT 4097")?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        rows.len() <= MEMBER_LIMIT,
        "custody membership capacity exceeded"
    );
    rows.into_iter()
        .map(|(kind, id)| {
            validate_sha256(&id)?;
            Ok((CustodyKind::parse(&kind)?, id))
        })
        .collect()
}
pub(super) fn require_unresolved(db: &Connection, kind: CustodyKind, id: &str) -> Result<()> {
    ensure!(
        !resolved_keys(db)?.contains(&(kind, id.to_owned())),
        "resolved custody cannot regain a claim"
    );
    Ok(())
}
pub(super) fn effect_active(db: &Connection, id: &str, record: &EffectRecord) -> Result<()> {
    let resolved = resolved_keys(db)?;
    ensure!(
        !resolved.contains(&(CustodyKind::Effect, id.to_owned())),
        "resolved effect cannot be prepared again"
    );
    if !resolved.is_empty() {
        ensure!(
            !Catalog::load(db)?.effect_uses_resolved_setup(record, &resolved),
            "effect uses only resolved setup parents"
        );
    }
    Ok(())
}
pub(super) fn active_effect_ids(
    db: &Connection,
    device: &DeviceId,
    resolved: &BTreeSet<(CustodyKind, String)>,
) -> Result<Vec<String>> {
    Ok(Catalog::load(db)?.active_effect_ids(device, resolved))
}
fn route_rows(db: &Connection, table: &str, id_column: &str) -> Result<BTreeSet<(String, String)>> {
    let mut q=db.prepare(&format!("SELECT CASE WHEN length(route)=64 THEN route ELSE NULL END,CASE WHEN length({id_column})=64 THEN {id_column} ELSE NULL END FROM {table} LIMIT 1025"))?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        rows.len() <= CAPACITY as usize,
        "custody route cache exceeds bound"
    );
    Ok(rows.into_iter().collect())
}
pub(super) fn validate_routes(db: &Connection) -> Result<()> {
    for (table, id_column, source, kind, predicate) in [
        (
            "setup_custody_routes",
            "setup_id",
            "setup_effects",
            "setup",
            "state!='cancelled'",
        ),
        (
            "clock_sync_custody_routes",
            "sync_id",
            "clock_sync_effects",
            "clock_sync",
            "state IN ('prepared','claimed','unknown')",
        ),
    ] {
        let actual = route_rows(db, table, id_column)?;
        let mut q=db.prepare(&format!("SELECT route,id FROM {source} s WHERE {predicate} AND NOT EXISTS(SELECT 1 FROM custody_resolution_members m WHERE m.kind='{kind}' AND m.id=s.id) LIMIT 1025"))?;
        let expected = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;
        ensure!(
            expected.len() <= CAPACITY as usize && actual == expected,
            "custody route cache differs from unresolved history"
        );
    }
    Ok(())
}
pub(super) fn update_setup_route(db: &Connection, id: &str) -> Result<()> {
    db.execute("DELETE FROM setup_custody_routes WHERE setup_id=?1", [id])?;
    db.execute("INSERT INTO setup_custody_routes(route,setup_id) SELECT route,id FROM setup_effects s WHERE id=?1 AND state!='cancelled' AND NOT EXISTS(SELECT 1 FROM custody_resolution_members m WHERE m.kind='setup' AND m.id=s.id)",[id])?;
    Ok(())
}
pub(super) fn update_sync_route(db: &Connection, id: &str) -> Result<()> {
    db.execute(
        "DELETE FROM clock_sync_custody_routes WHERE sync_id=?1",
        [id],
    )?;
    db.execute("INSERT INTO clock_sync_custody_routes(route,sync_id) SELECT route,id FROM clock_sync_effects s WHERE id=?1 AND state IN ('prepared','claimed','unknown') AND NOT EXISTS(SELECT 1 FROM custody_resolution_members m WHERE m.kind='clock_sync' AND m.id=s.id)",[id])?;
    Ok(())
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, kind, sql) in [
        ("custody_resolutions", "table", TABLE),
        ("custody_resolution_request", "index", REQUEST_INDEX),
        ("custody_resolution_members", "table", MEMBERS),
        ("custody_resolution_census", "table", CENSUS),
        ("setup_custody_routes", "table", SETUP_ROUTES),
        ("clock_sync_custody_routes", "table", SYNC_ROUTES),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if version < 8 {
            ensure!(
                actual.is_none(),
                "unexpected reconciliation journal before migration"
            );
        } else {
            ensure!(
                actual == Some((kind.into(), sql.into())),
                "reconciliation journal layout differs"
            );
        }
    }
    if version < 8 {
        return Ok(());
    }
    let triggers:u64=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('custody_resolutions','custody_resolution_members','custody_resolution_census','setup_custody_routes','clock_sync_custody_routes')",[],|r|r.get(0))?;
    ensure!(triggers == 0, "unexpected reconciliation journal trigger");
    let (count, total) = census(db)?;
    let catalog = Catalog::load(db)?;
    let mut observed = 0usize;
    let mut keys = BTreeSet::new();
    let mut q = db
        .prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM custody_resolutions")?;
    for id in q.query_map([], |r| r.get::<_, String>(0))? {
        let record = record_row(db, &id?, &catalog)?;
        for m in record.snapshot.members {
            ensure!(keys.insert(m.key()), "custody member is covered twice");
        }
        observed += 1;
    }
    ensure!(
        observed == count && keys.len() == total && keys == raw_keys(db)?,
        "custody membership census differs"
    );
    validate_routes(db)?;
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 8 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for sql in [TABLE,REQUEST_INDEX,MEMBERS,CENSUS,SETUP_ROUTES,SYNC_ROUTES,"INSERT INTO custody_resolution_census VALUES(1,0,0)","DROP INDEX setup_active_route",SETUP_HISTORY_INDEX,"DROP INDEX clock_sync_active_route",SYNC_HISTORY_INDEX,"INSERT INTO setup_custody_routes SELECT route,id FROM setup_effects WHERE state!='cancelled'","INSERT INTO clock_sync_custody_routes SELECT route,id FROM clock_sync_effects WHERE state IN ('prepared','claimed','unknown')","PRAGMA user_version=8"] {tx.execute_batch(sql)?;}
    validate_schema(&tx, 8)?;
    tx.commit()?;
    Ok(())
}
/// HSP admission checks legacy custody inside its own immediate transaction.
/// This deliberately bypasses the HSP guard on the public legacy snapshot path.
pub(super) fn ensure_no_legacy_custody(
    db: &Connection,
    route: &str,
    namespace: &str,
    device: &DeviceId,
) -> Result<()> {
    let resolved = resolved_keys(db)?;
    let catalog = Catalog::load(db)?;
    catalog.require_unresolved_namespace(namespace, &resolved)?;
    ensure!(
        catalog
            .snapshot(device, route, namespace, &resolved)?
            .members
            .is_empty(),
        "device or route retains unresolved legacy custody"
    );
    Ok(())
}

impl DeviceEffectJournal {
    pub(crate) fn custody_snapshot(
        &self,
        device: &DeviceId,
        route: &str,
        namespace: &str,
    ) -> Result<CustodySnapshot> {
        self.check_owner()?;
        let tx = self.db.unchecked_transaction()?;
        let owner: String =
            tx.query_row("SELECT boot FROM owner WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(
            owner == self.boot.as_str(),
            "custody snapshot owner changed"
        );
        Self::validate_existing(&tx)?;
        let resolved = raw_keys(&tx)?;
        let mut snapshot = Catalog::load(&tx)?.snapshot(device, route, namespace, &resolved)?;
        snapshot.hsp_streams =
            super::hsp::recovery::selected_memberships(&tx, device, route, namespace)?;
        snapshot.hsp_preparations =
            super::hsp::preparation::selected_memberships(&tx, device, route, namespace)?;
        snapshot.validate()?;
        tx.commit()?;
        Ok(snapshot)
    }
    pub(crate) fn is_resolved(&self, kind: CustodyKind, id: &str) -> Result<bool> {
        self.check_owner()?;
        validate_sha256(id)?;
        Ok(resolved_keys(&self.db)?.contains(&(kind, id.to_owned())))
    }
    pub(crate) fn record_resolution(&mut self, record: &CustodyResolutionRecord) -> Result<String> {
        record.validate()?;
        self.check_owner()?;
        let id = record.id()?;
        let text = serde_json::to_string(record)?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let owner: String =
            tx.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(
            owner == self.boot.as_str(),
            "resolution journal owner changed"
        );
        validate_schema(&tx, 8)?;
        let catalog = Catalog::load(&tx)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT id FROM custody_resolutions WHERE actor=?1 AND request=?2",
                params![record.actor_session.as_str(), record.request_id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            let previous = record_row(&tx, &prior, &catalog)?;
            ensure!(
                prior == id && serde_json::to_string(&previous)? == text,
                "resolution request already names different custody"
            );
            return Ok(id);
        }
        ensure!(
            record.plan.selection.engine_epoch == self.boot,
            "resolution belongs to a retired Engine epoch"
        );
        let resolved = raw_keys(&tx)?;
        let mut current = catalog.snapshot(
            &record.snapshot.device,
            &record.snapshot.route_sha256,
            &record.snapshot.route_namespace_sha256,
            &resolved,
        )?;
        current.hsp_streams = super::hsp::recovery::selected_memberships(
            &tx,
            &record.snapshot.device,
            &record.snapshot.route_sha256,
            &record.snapshot.route_namespace_sha256,
        )?;
        current.hsp_preparations = super::hsp::preparation::selected_memberships(
            &tx,
            &record.snapshot.device,
            &record.snapshot.route_sha256,
            &record.snapshot.route_namespace_sha256,
        )?;
        ensure!(
            current == record.snapshot,
            "custody changed after final observation"
        );
        record.basis.extends_with_stops(&current)?;
        let (records, members) = census(&tx)?;
        ensure!(
            records < CAPACITY as usize && members + current.members.len() <= MEMBER_LIMIT,
            "resolution journal capacity exceeded"
        );
        tx.execute(
            "INSERT INTO custody_resolutions VALUES(?1,?2,?3,?4)",
            params![
                id,
                record.actor_session.as_str(),
                record.request_id.as_str(),
                text
            ],
        )?;
        for member in &current.members {
            tx.execute(
                "INSERT INTO custody_resolution_members VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    member.kind.name(),
                    member.id,
                    id,
                    member.record_sha256,
                    member.observation_sha256,
                    membership_digest(&id, member)?
                ],
            )?;
            match member.kind {
                CustodyKind::Setup => update_setup_route(&tx, &member.id)?,
                CustodyKind::ClockSync => update_sync_route(&tx, &member.id)?,
                _ => {}
            }
        }
        super::hsp::recovery::insert_resolution(&tx, &id, &current.hsp_streams)?;
        super::hsp::preparation::insert_resolution(&tx, &id, &current.hsp_preparations)?;
        ensure!(tx.execute("UPDATE custody_resolution_census SET resolution_count=resolution_count+1,member_count=member_count+?1 WHERE singleton=1",[current.members.len() as u64])?==1,"resolution census disappeared");
        validate_schema(&tx, 8)?;
        tx.commit()?;
        Ok(id)
    }
    pub(crate) fn resolution_history(
        &self,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<Option<(String, CustodyResolutionRecord)>> {
        self.check_owner()?;
        validate_schema(&self.db, 8)?;
        let id: Option<String> = self
            .db
            .query_row(
                "SELECT id FROM custody_resolutions WHERE actor=?1 AND request=?2",
                params![actor.as_str(), request.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| {
            let record = record_row(&self.db, &id, &Catalog::load(&self.db)?)?;
            Ok((id, record))
        })
        .transpose()
    }
    pub(crate) fn verify_resolution(
        state_dir: &Path,
        actor: &SessionId,
        request: &RequestId,
        id: &str,
    ) -> Result<CustodyResolutionRecord> {
        let path = state_dir.join("device-effects/journal.sqlite3");
        let _held = crate::artifacts::open_regular(&path)?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        ensure!(
            Self::validate_existing(&db)? >= 8,
            "reconciliation journal unavailable"
        );
        let record = record_row(&db, id, &Catalog::load(&db)?)?;
        ensure!(
            &record.actor_session == actor && &record.request_id == request,
            "resolution witness actor/request differs"
        );
        Ok(record)
    }
}

#[cfg(test)]
#[path = "device_reconciliation_journal_tests.rs"]
mod tests;
