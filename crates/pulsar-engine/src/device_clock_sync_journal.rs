//! Durable, independent synchronization custody; no device I/O and no restored dispatch claims.
use super::*;
#[cfg(test)]
#[path = "device_clock_sync_journal_tests.rs"]
mod tests;
const TABLE:&str="CREATE TABLE clock_sync_effects(id TEXT PRIMARY KEY NOT NULL,route TEXT NOT NULL,intent TEXT NOT NULL,record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','claimed','acknowledged','unknown','cancelled')),observation TEXT,observation_sha256 TEXT)";
const ROUTE:&str="CREATE UNIQUE INDEX clock_sync_active_route ON clock_sync_effects(route) WHERE state IN ('prepared','claimed','unknown')";
const INTENT: &str = "CREATE UNIQUE INDEX clock_sync_intent ON clock_sync_effects(intent)";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSyncEffectRecord {
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub original_controller: HandyControllerLease,
    pub registration: HandyRegistration,
    pub preparation_qualification: HandyQualificationBinding,
    pub setup_record_id: ArtifactId,
    pub user_offset_before: HandyUserTimeOffset,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
}
impl ClockSyncEffectRecord {
    fn validate(&self) -> Result<()> {
        self.original_controller.validate()?;
        self.registration.validate()?;
        self.preparation_qualification.validate()?;
        self.user_offset_before.validate()?;
        for value in [
            &self.route_sha256,
            &self.route_namespace_sha256,
            self.setup_record_id.as_str(),
        ] {
            validate_sha256(value)?;
        }
        ensure!(
            self.original_controller.state == HandyControllerState::Active
                && !self.registration.disabled
                && self.registration.device_id == self.original_controller.device_id
                && self.request_id != self.original_controller.acquisition_request_id,
            "invalid synchronization subject"
        );
        Ok(())
    }
    fn intent(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                &self.actor_session,
                &self.request_id
            ))?)
        ))
    }
}
#[derive(Clone, Debug)]
pub struct ClockSyncEffectObservation {
    pub id: String,
    pub record: ClockSyncEffectRecord,
    pub state: HandyClockSyncState,
    pub acknowledgment: Option<HandyClockSyncAcknowledgment>,
}
#[derive(Clone, Debug)]
pub struct PreparedClockSyncReceipt {
    id: String,
    boot: DeviceSessionId,
}
impl PreparedClockSyncReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
}
pub struct ClaimedClockSyncReceipt {
    id: String,
    boot: DeviceSessionId,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClockSyncCompletion {
    Acknowledged(HandyClockSyncAcknowledgment),
    Unknown,
}
fn state(s: &str) -> Result<HandyClockSyncState> {
    Ok(match s {
        "prepared" => HandyClockSyncState::Prepared,
        "claimed" => HandyClockSyncState::Claimed,
        "acknowledged" => HandyClockSyncState::Acknowledged,
        "unknown" => HandyClockSyncState::Unknown,
        "cancelled" => HandyClockSyncState::Cancelled,
        _ => anyhow::bail!("invalid synchronization state"),
    })
}
fn observation_digest(id: &str, text: &str) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(id, text))?)
    ))
}
pub(super) fn row(db: &Connection, id: &str) -> Result<ClockSyncEffectObservation> {
    validate_sha256(id)?;
    let (route,intent,text,name,observation,sha):(String,String,String,String,Option<String>,Option<String>)=db.query_row("SELECT CASE WHEN length(route)=64 THEN route ELSE NULL END,CASE WHEN length(intent)=64 THEN intent ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(CAST(state AS BLOB))<=16 THEN state ELSE NULL END,CASE WHEN observation IS NULL OR length(CAST(observation AS BLOB))<=1024 THEN observation ELSE 'invalid' END,CASE WHEN observation_sha256 IS NULL OR length(observation_sha256)=64 THEN observation_sha256 ELSE 'invalid' END FROM clock_sync_effects WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    ensure!(
        format!("{:x}", Sha256::digest(text.as_bytes())) == id,
        "synchronization record integrity differs"
    );
    let record: ClockSyncEffectRecord = serde_json::from_str(&text)?;
    record.validate()?;
    ensure!(
        record.route_sha256 == route && record.intent()? == intent,
        "synchronization route or intent differs"
    );
    let state = state(&name)?;
    let acknowledgment = match (observation, sha) {
        (Some(text), Some(sha)) => {
            ensure!(
                state == HandyClockSyncState::Acknowledged && observation_digest(id, &text)? == sha,
                "synchronization observation integrity differs"
            );
            let a: HandyClockSyncAcknowledgment = serde_json::from_str(&text)?;
            a.validate()?;
            Some(a)
        }
        (None, None) => {
            ensure!(
                state != HandyClockSyncState::Acknowledged,
                "missing synchronization acknowledgment"
            );
            None
        }
        _ => anyhow::bail!("incomplete synchronization observation"),
    };
    Ok(ClockSyncEffectObservation {
        id: id.into(),
        record,
        state,
        acknowledgment,
    })
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, kind, sql) in [
        ("clock_sync_effects", "table", TABLE),
        (
            "clock_sync_active_route",
            "index",
            if version >= 8 {
                super::reconciliation::SYNC_HISTORY_INDEX
            } else {
                ROUTE
            },
        ),
        ("clock_sync_intent", "index", INTENT),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if version < 4 {
            ensure!(
                actual.is_none(),
                "unexpected synchronization journal before migration"
            );
        } else {
            ensure!(
                actual == Some((kind.into(), sql.into())),
                "synchronization journal schema differs"
            );
        }
    }
    if version < 4 {
        return Ok(());
    }
    let count: u64 = db.query_row("SELECT count(*) FROM clock_sync_effects", [], |r| r.get(0))?;
    ensure!(
        count <= CAPACITY,
        "synchronization journal capacity exceeded"
    );
    let triggers: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name='clock_sync_effects'",
        [],
        |r| r.get(0),
    )?;
    ensure!(triggers == 0, "unexpected synchronization journal trigger");
    let mut q =
        db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM clock_sync_effects")?;
    for id in q.query_map([], |r| r.get::<_, String>(0))? {
        let value = row(db, &id?)?;
        parent(db, &value.record)?;
    }
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 4 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for sql in [TABLE, ROUTE, INTENT, "PRAGMA user_version=4"] {
        tx.execute_batch(sql)?;
    }
    tx.commit()?;
    Ok(())
}
fn owner(db: &Connection, boot: &DeviceSessionId) -> Result<()> {
    let value:String=db.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1",[],|r|r.get(0))?;
    ensure!(
        value == boot.as_str(),
        "retired synchronization journal owner"
    );
    Ok(())
}
pub(super) fn parent(db: &Connection, record: &ClockSyncEffectRecord) -> Result<()> {
    let value = super::setup::row(db, record.setup_record_id.as_str())?;
    let c = &record.original_controller;
    ensure!(
        value.state == SetupJournalState::Acknowledged
            && value.acknowledgment.is_some()
            && value.record.actor_session == record.actor_session
            && value.record.boot == c.engine_epoch
            && value.record.device_session == c.device_session
            && value.record.project_id == c.project_id
            && value.record.revision == c.revision
            && value.record.registration == record.registration
            && value.record.qualification == record.preparation_qualification
            && value.record.route_sha256 == record.route_sha256
            && value.record.route_namespace_sha256.as_ref() == Some(&record.route_namespace_sha256),
        "synchronization setup parent differs"
    );
    Ok(())
}
fn available(
    db: &Connection,
    route: &str,
    namespace: &str,
    device: &DeviceId,
    allowed: Option<&str>,
) -> Result<()> {
    validate_sha256(route)?;
    validate_sha256(namespace)?;
    let resolved = super::reconciliation::resolved_keys(db)?;
    if let Some(id) = allowed {
        ensure!(
            !resolved.contains(&(CustodyKind::ClockSync, id.into())),
            "resolved synchronization cannot regain authority"
        );
    }
    let mut q=db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM clock_sync_effects WHERE state IN ('prepared','claimed','unknown') LIMIT 1025")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        ids.len() <= CAPACITY as usize,
        "synchronization capacity exceeded"
    );
    for id in ids {
        if resolved.contains(&(CustodyKind::ClockSync, id.clone())) {
            continue;
        }
        let v = row(db, &id)?;
        ensure!(
            v.record.route_namespace_sha256 == namespace,
            "synchronization namespace changed with unresolved history"
        );
        ensure!(
            allowed == Some(id.as_str())
                || (v.record.route_sha256 != route && v.record.registration.device_id != *device),
            "device route has unresolved synchronization"
        );
    }
    Ok(())
}
impl DeviceEffectJournal {
    pub(crate) fn clock_sync_route_available(
        &self,
        route: &str,
        namespace: &str,
        device: &DeviceId,
        claim: Option<&ClaimedClockSyncReceipt>,
    ) -> Result<()> {
        self.check_owner()?;
        super::hsp::ensure_legacy_route_available(&self.db, route, namespace, device)?;
        if let Some(c) = claim {
            let value = row(&self.db, &c.id)?;
            ensure!(
                c.boot == self.boot
                    && value.state == HandyClockSyncState::Claimed
                    && value.record.route_sha256 == route
                    && value.record.route_namespace_sha256 == namespace
                    && value.record.registration.device_id == *device,
                "retired synchronization claim"
            );
        }
        available(
            &self.db,
            route,
            namespace,
            device,
            claim.map(|c| c.id.as_str()),
        )
    }
    pub fn clock_sync_request_history(
        &self,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<Option<ClockSyncEffectObservation>> {
        self.check_owner()?;
        let intent = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(actor, request))?)
        );
        let id:Option<String>=self.db.query_row("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM clock_sync_effects WHERE intent=?1",[intent],|r|r.get(0)).optional()?;
        id.map(|id| row(&self.db, &id)).transpose()
    }
    pub fn prepare_clock_sync(
        &mut self,
        record: ClockSyncEffectRecord,
    ) -> Result<PreparedClockSyncReceipt> {
        record.validate()?;
        ensure!(
            record.original_controller.engine_epoch == self.boot,
            "retired synchronization intent"
        );
        let text = serde_json::to_string(&record)?;
        ensure!(
            text.len() <= RECORD_LIMIT,
            "synchronization record exceeds bound"
        );
        let id = format!("{:x}", Sha256::digest(text.as_bytes()));
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::ClockSync, &id)?;
        super::reconciliation::require_unresolved(
            &tx,
            CustodyKind::Setup,
            record.setup_record_id.as_str(),
        )?;
        parent(&tx, &record)?;
        super::play::available(
            &tx,
            &record.route_sha256,
            &record.route_namespace_sha256,
            &record.registration.device_id,
            None,
        )?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM clock_sync_effects WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if exists {
            row(&tx, &id)?;
        } else {
            available(
                &tx,
                &record.route_sha256,
                &record.route_namespace_sha256,
                &record.registration.device_id,
                None,
            )?;
            let count: u64 =
                tx.query_row("SELECT count(*) FROM clock_sync_effects", [], |r| r.get(0))?;
            ensure!(count < CAPACITY, "synchronization capacity reached");
            tx.execute(
                "INSERT INTO clock_sync_effects VALUES(?1,?2,?3,?4,'prepared',NULL,NULL)",
                params![id, record.route_sha256, record.intent()?, text],
            )?;
        }
        super::reconciliation::update_sync_route(&tx, &id)?;
        tx.commit()?;
        Ok(PreparedClockSyncReceipt {
            id,
            boot: self.boot.clone(),
        })
    }
    pub fn claim_clock_sync(
        &mut self,
        p: &PreparedClockSyncReceipt,
    ) -> Result<ClaimedClockSyncReceipt> {
        ensure!(p.boot == self.boot, "retired synchronization receipt");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::ClockSync, &p.id)?;
        let v = row(&tx, &p.id)?;
        super::reconciliation::require_unresolved(
            &tx,
            CustodyKind::Setup,
            v.record.setup_record_id.as_str(),
        )?;
        super::play::available(
            &tx,
            &v.record.route_sha256,
            &v.record.route_namespace_sha256,
            &v.record.registration.device_id,
            None,
        )?;
        ensure!(
            v.record.original_controller.engine_epoch == self.boot
                && v.state == HandyClockSyncState::Prepared,
            "synchronization claim already consumed"
        );
        tx.execute(
            "UPDATE clock_sync_effects SET state='claimed' WHERE id=?1",
            [&p.id],
        )?;
        tx.commit()?;
        Ok(ClaimedClockSyncReceipt {
            id: p.id.clone(),
            boot: self.boot.clone(),
        })
    }
    pub fn complete_clock_sync(
        &mut self,
        c: &ClaimedClockSyncReceipt,
        outcome: ClockSyncCompletion,
    ) -> Result<()> {
        ensure!(c.boot == self.boot, "retired synchronization claim");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        let prior = row(&tx, &c.id)?;
        let (state, name, ack) = match outcome {
            ClockSyncCompletion::Acknowledged(a) => {
                a.validate()?;
                (HandyClockSyncState::Acknowledged, "acknowledged", Some(a))
            }
            ClockSyncCompletion::Unknown => (HandyClockSyncState::Unknown, "unknown", None),
        };
        if super::reconciliation::resolved_keys(&tx)?
            .contains(&(CustodyKind::ClockSync, c.id.clone()))
        {
            ensure!(
                prior.state == state && prior.acknowledgment == ack,
                "resolved synchronization outcome cannot advance"
            );
            return Ok(());
        }
        ensure!(
            prior.state == HandyClockSyncState::Claimed
                || (prior.state == state && prior.acknowledgment == ack),
            "changed synchronization completion requires reconciliation"
        );
        let text = ack.as_ref().map(serde_json::to_string).transpose()?;
        ensure!(
            text.as_ref().is_none_or(|s| s.len() <= 1024),
            "synchronization observation exceeds bound"
        );
        let sha = text
            .as_ref()
            .map(|s| observation_digest(&c.id, s))
            .transpose()?;
        tx.execute("UPDATE clock_sync_effects SET state=?1,observation=?2,observation_sha256=?3 WHERE id=?4",params![name,text,sha,c.id])?;
        super::reconciliation::update_sync_route(&tx, &c.id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn cancel_clock_sync_unclaimed(&mut self, id: &str) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::ClockSync, id)?;
        let v = row(&tx, id)?;
        ensure!(
            matches!(
                v.state,
                HandyClockSyncState::Prepared | HandyClockSyncState::Cancelled
            ),
            "attempted synchronization cannot be cancelled locally"
        );
        tx.execute(
            "UPDATE clock_sync_effects SET state='cancelled' WHERE id=?1",
            [id],
        )?;
        super::reconciliation::update_sync_route(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn verify_clock_sync_records(
        state_dir: &Path,
        witnesses: &[(SessionId, RequestId, String)],
    ) -> Result<()> {
        if witnesses.is_empty() {
            return Ok(());
        }
        let path = state_dir.join("device-effects/journal.sqlite3");
        let _held = crate::artifacts::open_regular(&path)?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        ensure!(
            Self::validate_existing(&db)? >= 4,
            "synchronization journal history unavailable"
        );
        for (actor, request, id) in witnesses {
            let v = row(&db, id)?;
            ensure!(
                &v.record.actor_session == actor && &v.record.request_id == request,
                "synchronization witness differs"
            );
        }
        Ok(())
    }
}
