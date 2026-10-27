//! Original HSP synchronization custody and exact preparation-to-stream handoff.
//! Journal history never restores a native clock or permits a repeated GET.
use super::*;
use crate::handy_hsp_preparation_records::HspPreparationRecord;
use crate::handy_hsp_records::HspQualificationLinkRecord;

const LIMIT: usize = 64;
const PREPARATIONS:&str="CREATE TABLE hsp_preparations(id TEXT PRIMARY KEY NOT NULL,intent TEXT NOT NULL,record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','claimed','acknowledged','unknown','cancelled')),acknowledgment TEXT,lifecycle_sha256 TEXT NOT NULL)";
const INTENTS: &str = "CREATE UNIQUE INDEX hsp_preparation_intent ON hsp_preparations(intent)";
const LINKS:&str="CREATE TABLE hsp_preparation_links(preparation TEXT PRIMARY KEY NOT NULL,stream TEXT NOT NULL UNIQUE,record TEXT NOT NULL,transferred INTEGER NOT NULL CHECK(transferred IN (0,1)),lifecycle_sha256 TEXT NOT NULL)";
const RESOLUTIONS:&str="CREATE TABLE hsp_preparation_resolutions(preparation TEXT PRIMARY KEY NOT NULL,resolution TEXT NOT NULL,membership_sha256 TEXT NOT NULL)";
const CENSUS:&str="CREATE TABLE hsp_preparation_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),preparations INTEGER NOT NULL CHECK(preparations BETWEEN 0 AND 64),record_bytes INTEGER NOT NULL CHECK(record_bytes BETWEEN 0 AND 1048576),links INTEGER NOT NULL CHECK(links BETWEEN 0 AND 64),resolutions INTEGER NOT NULL CHECK(resolutions BETWEEN 0 AND 64))";
pub(super) const OBJECTS: [(&str, &str, &str); 5] = [
    ("hsp_preparations", "table", PREPARATIONS),
    ("hsp_preparation_intent", "index", INTENTS),
    ("hsp_preparation_links", "table", LINKS),
    ("hsp_preparation_resolutions", "table", RESOLUTIONS),
    ("hsp_preparation_census", "table", CENSUS),
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedHspPreparationReceipt {
    id: String,
    boot: DeviceSessionId,
}
impl PreparedHspPreparationReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimedHspPreparationReceipt {
    id: String,
    boot: DeviceSessionId,
}
impl ClaimedHspPreparationReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
}
#[derive(Clone, Debug)]
pub struct HspPreparationObservation {
    pub id: String,
    pub record: HspPreparationRecord,
    pub state: HandyClockSyncState,
    pub acknowledgment: Option<HandyClockSyncAcknowledgment>,
    pub linked_stream_id: Option<String>,
    pub custody_transferred: bool,
    pub resolved: bool,
}
impl HspPreparationObservation {
    pub fn history(&self) -> std::result::Result<HandyHspPreparationHistory, ProtocolError> {
        let value = HandyHspPreparationHistory {
            record_id: ArtifactId::new(self.id.clone())
                .map_err(|_| ProtocolError::invalid("invalid HSP preparation history identity"))?,
            engine_epoch: self.record.boot.clone(),
            device_session: self.record.device_session.clone(),
            revision: self.record.revision,
            device_id: self.record.registration.device_id.clone(),
            registration_revision: self.record.registration.revision,
            qualification: self.record.qualification.clone(),
            subject_sha256: self.record.subject_sha256.clone(),
            profile_sha256: self.record.profile_sha256.clone(),
            deadline_boottime_ns: self.record.deadline_boottime_ns,
            state: self.state,
            acknowledgment: self.acknowledgment.clone(),
            linked_stream_id: self
                .linked_stream_id
                .as_ref()
                .map(|id| {
                    ArtifactId::new(id.clone())
                        .map_err(|_| ProtocolError::invalid("invalid HSP child stream history"))
                })
                .transpose()?,
            custody_transferred: self.custody_transferred,
            resolved: self.resolved,
        };
        value.validate()?;
        Ok(value)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspPreparationMembership {
    pub id: String,
    pub record: HspPreparationRecord,
    pub state: HandyClockSyncState,
    pub acknowledgment: Option<HandyClockSyncAcknowledgment>,
    pub linked_stream_id: Option<String>,
    pub custody_transferred: bool,
}
impl HspPreparationMembership {
    pub fn validate(&self) -> Result<()> {
        self.record.validate()?;
        ensure!(
            self.id == self.record.id()?,
            "HSP preparation membership identity differs"
        );
        check_ack(&self.record, self.state, self.acknowledgment.as_ref())?;
        if let Some(id) = &self.linked_stream_id {
            validate_sha256(id)?;
            ensure!(
                self.state == HandyClockSyncState::Acknowledged,
                "unacknowledged HSP preparation has child link"
            );
        }
        ensure!(
            !self.custody_transferred || self.linked_stream_id.is_some(),
            "HSP preparation transfer lacks child"
        );
        Ok(())
    }
    pub fn observation_sha256(&self) -> Result<String> {
        self.validate()?;
        digest("pulsar.hsp.preparation.membership.v1", self)
    }
    pub fn extends(&self, after: &Self) -> Result<()> {
        self.validate()?;
        after.validate()?;
        ensure!(
            self.id == after.id
                && self.record == after.record
                && self.linked_stream_id == after.linked_stream_id
                && (!self.custody_transferred || after.custody_transferred),
            "HSP recovery preparation identity or handoff changed"
        );
        ensure!(
            self.state == after.state
                || matches!(
                    (self.state, after.state),
                    (
                        HandyClockSyncState::Prepared,
                        HandyClockSyncState::Claimed
                            | HandyClockSyncState::Acknowledged
                            | HandyClockSyncState::Unknown
                            | HandyClockSyncState::Cancelled
                    ) | (
                        HandyClockSyncState::Claimed,
                        HandyClockSyncState::Acknowledged | HandyClockSyncState::Unknown
                    )
                ),
            "HSP original synchronization uncertainty changed"
        );
        if self.acknowledgment.is_some() || self.state == after.state {
            ensure!(
                self.acknowledgment == after.acknowledgment,
                "HSP synchronization observation changed"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamLink {
    preparation: String,
    stream: String,
    qualification_link_sha256: String,
}
struct PreparationCatalog {
    rows: BTreeMap<String, HspPreparationObservation>,
    resolutions: BTreeMap<String, String>,
}
fn state_name(state: HandyClockSyncState) -> &'static str {
    match state {
        HandyClockSyncState::Prepared => "prepared",
        HandyClockSyncState::Claimed => "claimed",
        HandyClockSyncState::Acknowledged => "acknowledged",
        HandyClockSyncState::Unknown => "unknown",
        HandyClockSyncState::Cancelled => "cancelled",
    }
}
fn parse_state(state: &str) -> Result<HandyClockSyncState> {
    Ok(match state {
        "prepared" => HandyClockSyncState::Prepared,
        "claimed" => HandyClockSyncState::Claimed,
        "acknowledged" => HandyClockSyncState::Acknowledged,
        "unknown" => HandyClockSyncState::Unknown,
        "cancelled" => HandyClockSyncState::Cancelled,
        _ => anyhow::bail!("invalid HSP preparation state"),
    })
}
fn lifecycle(
    id: &str,
    state: HandyClockSyncState,
    ack: Option<&HandyClockSyncAcknowledgment>,
) -> Result<String> {
    digest(
        "pulsar.hsp.preparation.lifecycle.v1",
        &(id, state_name(state), ack),
    )
}
fn link_lifecycle(link: &StreamLink, transferred: bool) -> Result<String> {
    digest("pulsar.hsp.preparation.handoff.v1", &(link, transferred))
}
fn membership(value: &HspPreparationObservation) -> HspPreparationMembership {
    HspPreparationMembership {
        id: value.id.clone(),
        record: value.record.clone(),
        state: value.state,
        acknowledgment: value.acknowledgment.clone(),
        linked_stream_id: value.linked_stream_id.clone(),
        custody_transferred: value.custody_transferred,
    }
}
fn check_ack(
    record: &HspPreparationRecord,
    state: HandyClockSyncState,
    ack: Option<&HandyClockSyncAcknowledgment>,
) -> Result<()> {
    ensure!(
        (state == HandyClockSyncState::Acknowledged) == ack.is_some(),
        "HSP synchronization state and acknowledgment differ"
    );
    if let Some(ack) = ack {
        ack.validate()?;
        ensure!(
            ack.sent_boottime_ns >= record.started_boottime_ns
                && ack.received_boottime_ns < record.deadline_boottime_ns,
            "HSP synchronization acknowledgment exceeds original deadline"
        );
    }
    Ok(())
}
fn existing(db: &Connection) -> Result<bool> {
    Ok(db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? >= 12)
}
fn read_row(db: &Connection, id: &str) -> Result<HspPreparationObservation> {
    validate_sha256(id)?;
    let (intent,text,state,ack,hash):(String,String,String,Option<String>,String)=db.query_row("SELECT CASE WHEN length(intent)=64 THEN intent ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=16384 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END,CASE WHEN acknowledgment IS NULL OR length(CAST(acknowledgment AS BLOB))<=2048 THEN acknowledgment ELSE NULL END,CASE WHEN length(lifecycle_sha256)=64 THEN lifecycle_sha256 ELSE NULL END FROM hsp_preparations WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let record = HspPreparationRecord::from_json(text.as_bytes())?;
    let state = parse_state(&state)?;
    let acknowledgment = ack
        .as_deref()
        .map(serde_json::from_str::<HandyClockSyncAcknowledgment>)
        .transpose()?;
    check_ack(&record, state, acknowledgment.as_ref())?;
    ensure!(
        record.id()? == id
            && record.intent()? == intent
            && record.to_json()? == text.as_bytes()
            && lifecycle(id, state, acknowledgment.as_ref())? == hash,
        "HSP preparation original history integrity differs"
    );
    ensure!(
        ack == acknowledgment
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
        "HSP preparation acknowledgment is not canonical"
    );
    Ok(HspPreparationObservation {
        id: id.into(),
        record,
        state,
        acknowledgment,
        linked_stream_id: None,
        custody_transferred: false,
        resolved: false,
    })
}
fn check_link(
    record: &HspPreparationRecord,
    stream: &HspStreamRecord,
    link: &HspQualificationLinkRecord,
) -> Result<()> {
    link.validate(stream)?;
    ensure!(
        record.boot == stream.boot
            && record.actor_session == stream.actor_session
            && record.request_id != stream.request_id
            && record.project_id == stream.binding.identity.live.project
            && record.revision == stream.binding.identity.live.revision
            && record.device_session == stream.binding.session
            && record.registration == stream.registration
            && record.device == stream.device
            && record.route_sha256 == stream.route_sha256
            && record.route_namespace_sha256 == stream.route_namespace_sha256
            && record.qualification == link.qualification
            && record.subject_sha256 == link.subject_sha256
            && record.profile_sha256 == link.profile_sha256,
        "HSP child stream differs from exact native preparation"
    );
    Ok(())
}
fn load(db: &Connection) -> Result<PreparationCatalog> {
    let mut all = PreparationCatalog {
        rows: BTreeMap::new(),
        resolutions: BTreeMap::new(),
    };
    let version = db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?;
    schema(db, version)?;
    if version < 12 {
        return Ok(all);
    }
    let census:Vec<(u64,u64,u64,u64,u64)>=db.prepare("SELECT singleton,preparations,record_bytes,links,resolutions FROM hsp_preparation_census LIMIT 2")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
    ensure!(
        census.len() == 1
            && census[0].0 == 1
            && census[0].1 <= 64
            && census[0].2 <= 1_048_576
            && census[0].3 <= 64
            && census[0].4 <= 64,
        "HSP preparation census exceeds bound"
    );
    let counts:(u64,u64,u64,u64)=db.query_row("SELECT (SELECT count(*) FROM hsp_preparations),(SELECT coalesce(sum(length(CAST(record AS BLOB))),0) FROM hsp_preparations),(SELECT count(*) FROM hsp_preparation_links),(SELECT count(*) FROM hsp_preparation_resolutions)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    ensure!(
        counts == (census[0].1, census[0].2, census[0].3, census[0].4),
        "HSP preparation census differs from original membership"
    );
    let mut q = db.prepare(
        "SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM hsp_preparations LIMIT 65",
    )?;
    for id in q.query_map([], |r| r.get::<_, String>(0))? {
        let id = id?;
        all.rows.insert(id.clone(), read_row(db, &id)?);
    }
    let mut q=db.prepare("SELECT CASE WHEN length(preparation)=64 THEN preparation ELSE NULL END,CASE WHEN length(stream)=64 THEN stream ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=4096 THEN record ELSE NULL END,transferred,CASE WHEN length(lifecycle_sha256)=64 THEN lifecycle_sha256 ELSE NULL END FROM hsp_preparation_links LIMIT 65")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, u8>(3)?,
            r.get::<_, String>(4)?,
        ))
    })? {
        let (prep, stream, text, transferred, hash) = row?;
        let link: StreamLink = serde_json::from_str(&text)?;
        ensure!(
            transferred <= 1
                && link.preparation == prep
                && link.stream == stream
                && serde_json::to_string(&link)? == text
                && link_lifecycle(&link, transferred == 1)? == hash,
            "HSP handoff integrity differs"
        );
        let parent = all
            .rows
            .get_mut(&prep)
            .ok_or_else(|| anyhow::anyhow!("orphan HSP preparation link"))?;
        ensure!(
            parent.state == HandyClockSyncState::Acknowledged,
            "HSP handoff lacks original acknowledged synchronization"
        );
        let stream_text:String=db.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END FROM hsp_streams WHERE id=?1",[&stream],|r|r.get(0))?;
        let stream_record = HspStreamRecord::from_json(stream_text.as_bytes())?;
        ensure!(
            stream_record.id()? == stream,
            "HSP linked stream identity differs"
        );
        let (qualification_id,qualification_text):(String,String)=db.query_row("SELECT id,CASE WHEN length(CAST(record AS BLOB))<=4096 THEN record ELSE NULL END FROM hsp_qualification_links WHERE stream=?1",[&stream],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let qualification: HspQualificationLinkRecord = serde_json::from_str(&qualification_text)?;
        check_link(&parent.record, &stream_record, &qualification)?;
        ensure!(
            qualification.id()? == qualification_id
                && qualification_id == link.qualification_link_sha256,
            "HSP linked qualification receipt differs"
        );
        let child_state:String=db.query_row("SELECT state FROM hsp_effects WHERE stream=?1 AND kind='setup' AND sequence='0000000000000001'",[&stream],|r|r.get(0))?;
        ensure!(
            (transferred == 1)
                == matches!(child_state.as_str(), "claimed" | "acknowledged" | "unknown"),
            "HSP custody transfer differs from first child claim"
        );
        parent.linked_stream_id = Some(stream);
        parent.custody_transferred = transferred == 1;
    }
    let mut q=db.prepare("SELECT CASE WHEN length(preparation)=64 THEN preparation ELSE NULL END,CASE WHEN length(resolution)=64 THEN resolution ELSE NULL END,CASE WHEN length(membership_sha256)=64 THEN membership_sha256 ELSE NULL END FROM hsp_preparation_resolutions LIMIT 65")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (prep, resolution, hash) = row?;
        let value = all
            .rows
            .get(&prep)
            .ok_or_else(|| anyhow::anyhow!("resolved HSP preparation missing"))?;
        let current = membership(value);
        let text:String=db.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM custody_resolutions WHERE id=?1",[&resolution],|r|r.get(0))?;
        let record: super::super::reconciliation::CustodyResolutionRecord =
            serde_json::from_str(&text)?;
        ensure!(
            record.id()? == resolution
                && record
                    .snapshot
                    .hsp_preparations
                    .iter()
                    .any(|m| m == &current)
                && resolution_digest(&resolution, &current)? == hash,
            "HSP preparation resolution differs"
        );
        all.resolutions.insert(prep, resolution);
    }
    for row in all.rows.values_mut() {
        row.resolved = all.resolutions.contains_key(&row.id);
        if row.custody_transferred {
            let child_resolution: Option<String> = db
                .query_row(
                    "SELECT resolution FROM hsp_resolution_members WHERE stream=?1",
                    [row.linked_stream_id.as_ref().expect("validated link")],
                    |r| r.get(0),
                )
                .optional()?;
            ensure!(
                child_resolution.as_ref() == all.resolutions.get(&row.id),
                "HSP parent and claimed child resolved independently"
            );
        }
    }
    Ok(all)
}
fn resolution_digest(resolution: &str, value: &HspPreparationMembership) -> Result<String> {
    digest("pulsar.hsp.preparation.resolution.v1", &(resolution, value))
}
pub(crate) fn validate_catalog(db: &Connection) -> Result<()> {
    load(db)?;
    Ok(())
}
fn available(
    all: &PreparationCatalog,
    route: &str,
    namespace: &str,
    device: &DeviceId,
    allowed: Option<&str>,
) -> Result<()> {
    for row in all.rows.values().filter(|r| {
        r.state != HandyClockSyncState::Cancelled && !r.resolved && !r.custody_transferred
    }) {
        ensure!(
            row.record.route_namespace_sha256 == namespace,
            "HSP preparation namespace changed with unresolved custody"
        );
        ensure!(
            allowed == Some(row.id.as_str())
                || (row.record.route_sha256 != route
                    && &row.record.registration.device_id != device),
            "HSP preparation retains route or device custody"
        );
    }
    Ok(())
}
pub(crate) fn ensure_route_available(
    db: &Connection,
    route: &str,
    namespace: &str,
    device: &DeviceId,
    allowed: Option<&str>,
) -> Result<()> {
    available(&load(db)?, route, namespace, device, allowed)
}
pub(crate) fn parent_for_stream(db: &Connection, stream: &str) -> Result<Option<String>> {
    if !existing(db)? {
        return Ok(None);
    }
    Ok(db
        .query_row(
            "SELECT preparation FROM hsp_preparation_links WHERE stream=?1",
            [stream],
            |r| r.get(0),
        )
        .optional()?)
}
pub(crate) fn include_cancelled_child(db: &Connection, stream: &str) -> Result<bool> {
    Ok(load(db)?.rows.values().any(|r| {
        !r.resolved
            && r.state != HandyClockSyncState::Cancelled
            && r.linked_stream_id.as_deref() == Some(stream)
    }))
}
pub(crate) fn has_device_custody(db: &Connection, device: &DeviceId) -> Result<bool> {
    Ok(load(db)?.rows.values().any(|r| {
        r.state != HandyClockSyncState::Cancelled
            && !r.resolved
            && !r.custody_transferred
            && &r.record.registration.device_id == device
    }))
}
pub(crate) fn selected_memberships(
    db: &Connection,
    device: &DeviceId,
    route: &str,
    namespace: &str,
) -> Result<Vec<HspPreparationMembership>> {
    let all = load(db)?;
    let mut selected = Vec::new();
    for row in all
        .rows
        .values()
        .filter(|r| r.state != HandyClockSyncState::Cancelled && !r.resolved)
    {
        ensure!(
            row.record.route_namespace_sha256 == namespace,
            "HSP preparation custody uses a foreign namespace"
        );
        if &row.record.registration.device_id == device || row.record.route_sha256 == route {
            ensure!(
                row.record.route_sha256 == route,
                "HSP preparation selected device uses another route"
            );
            selected.push(membership(row));
        }
    }
    Ok(selected)
}
fn set_state(
    db: &Connection,
    id: &str,
    state: HandyClockSyncState,
    ack: Option<&HandyClockSyncAcknowledgment>,
) -> Result<()> {
    ensure!(db.execute("UPDATE hsp_preparations SET state=?1,acknowledgment=?2,lifecycle_sha256=?3 WHERE id=?4",params![state_name(state),ack.map(serde_json::to_string).transpose()?,lifecycle(id,state,ack)?,id])?==1,"HSP preparation disappeared");
    Ok(())
}
pub(crate) fn retire_claims(db: &mut Connection, new_boot: &DeviceSessionId) -> Result<()> {
    let all = load(db)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for row in all.rows.values().filter(|r| {
        r.state == HandyClockSyncState::Claimed && !r.resolved && &r.record.boot != new_boot
    }) {
        set_state(&tx, &row.id, HandyClockSyncState::Unknown, None)?;
    }
    tx.commit()?;
    Ok(())
}
pub(crate) fn transfer_at_setup_claim(
    db: &Connection,
    stream: &HspStreamRecord,
    effect: &HspEffectRecord,
) -> Result<()> {
    let Some(parent) = parent_for_stream(db, &stream.id()?)? else {
        return Ok(());
    };
    ensure!(
        effect.kind == HspEffectRecordKind::Setup && effect.sequence == 1,
        "HSP preparation only transfers at original child Setup"
    );
    let all = load(db)?;
    let row = all
        .rows
        .get(&parent)
        .ok_or_else(|| anyhow::anyhow!("missing HSP preparation parent"))?;
    ensure!(
        row.state == HandyClockSyncState::Acknowledged
            && !row.resolved
            && !row.custody_transferred
            && row.record.boot == stream.boot,
        "retired or consumed HSP preparation handoff"
    );
    let text: String = db.query_row(
        "SELECT record FROM hsp_preparation_links WHERE preparation=?1",
        [&parent],
        |r| r.get(0),
    )?;
    let link: StreamLink = serde_json::from_str(&text)?;
    ensure!(db.execute("UPDATE hsp_preparation_links SET transferred=1,lifecycle_sha256=?1 WHERE preparation=?2 AND transferred=0",params![link_lifecycle(&link,true)?,parent])?==1,"HSP handoff already consumed");
    Ok(())
}
pub(crate) fn verify_resolution_members(
    db: &Connection,
    resolution: &str,
    values: &[HspPreparationMembership],
) -> Result<()> {
    if !existing(db)? {
        ensure!(values.is_empty(), "HSP preparation recovery requires v12");
        return Ok(());
    }
    let all = load(db)?;
    for value in values {
        ensure!(
            all.rows.get(&value.id).map(membership).as_ref() == Some(value)
                && all
                    .resolutions
                    .get(&value.id)
                    .is_some_and(|r| r == resolution),
            "HSP resolved preparation membership differs"
        );
    }
    let count: usize = db.query_row(
        "SELECT count(*) FROM hsp_preparation_resolutions WHERE resolution=?1",
        [resolution],
        |r| r.get(0),
    )?;
    ensure!(
        count == values.len(),
        "HSP preparation resolution membership count differs"
    );
    Ok(())
}
pub(crate) fn insert_resolution(
    db: &Connection,
    resolution: &str,
    values: &[HspPreparationMembership],
) -> Result<()> {
    // Caller already compared the exact current aggregate in this Immediate
    // transaction. Both parent and stream memberships publish before validation.
    for value in values {
        value.validate()?;
        db.execute(
            "INSERT INTO hsp_preparation_resolutions VALUES(?1,?2,?3)",
            params![value.id, resolution, resolution_digest(resolution, value)?],
        )?;
        db.execute(
            "UPDATE hsp_preparation_census SET resolutions=resolutions+1 WHERE singleton=1",
            [],
        )?;
    }
    Ok(())
}
impl DeviceEffectJournal {
    fn hsp_preparation_transaction<T>(
        &mut self,
        operation: impl FnOnce(&Connection, &DeviceSessionId, &PreparationCatalog) -> Result<T>,
    ) -> Result<T> {
        self.without_writer_wait(|journal| {
            let tx = journal
                .db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            owner(&tx, &journal.boot)?;
            let all = load(&tx)?;
            let result = operation(&tx, &journal.boot, &all)?;
            load(&tx)?;
            tx.commit()?;
            Ok(result)
        })
    }
    pub fn prepare_hsp_preparation(
        &mut self,
        record: &HspPreparationRecord,
    ) -> Result<PreparedHspPreparationReceipt> {
        record.validate()?;
        self.hsp_preparation_transaction(|db,boot,all| {
            ensure!(&record.boot==boot,"retired HSP preparation request");let id=record.id()?;
            if let Some(prior)=all.rows.get(&id) {ensure!(prior.record==*record && !prior.resolved,"HSP preparation replay changed or resolved");return Ok(PreparedHspPreparationReceipt {id,boot:boot.clone()});}
            ensure!(all.rows.len()<LIMIT,"HSP preparation capacity reached");
            available(all,&record.route_sha256,&record.route_namespace_sha256,&record.registration.device_id,None)?;
            super::ensure_legacy_route_available(db,&record.route_sha256,&record.route_namespace_sha256,&record.registration.device_id)?;
            super::super::reconciliation::ensure_no_legacy_custody(db,&record.route_sha256,&record.route_namespace_sha256,&record.registration.device_id)?;
            let text=String::from_utf8(record.to_json()?)?;
            db.execute("INSERT INTO hsp_preparations VALUES(?1,?2,?3,'prepared',NULL,?4)",params![id,record.intent()?,text,lifecycle(&id,HandyClockSyncState::Prepared,None)?])?;
            db.execute("UPDATE hsp_preparation_census SET preparations=preparations+1,record_bytes=record_bytes+?1 WHERE singleton=1",[text.len() as u64])?;
            Ok(PreparedHspPreparationReceipt {id,boot:boot.clone()})
        })
    }
    pub fn claim_hsp_preparation(
        &mut self,
        receipt: &PreparedHspPreparationReceipt,
    ) -> Result<ClaimedHspPreparationReceipt> {
        self.hsp_preparation_transaction(|db, boot, all| {
            let row = all
                .rows
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP preparation"))?;
            ensure!(
                &receipt.boot == boot
                    && &row.record.boot == boot
                    && row.state == HandyClockSyncState::Prepared
                    && !row.resolved,
                "HSP synchronization claim is retired or consumed"
            );
            available(
                all,
                &row.record.route_sha256,
                &row.record.route_namespace_sha256,
                &row.record.registration.device_id,
                Some(&receipt.id),
            )?;
            let streams = load_catalog(db)?;
            for stream in streams.streams.values().filter(|s| {
                s.state != HspStreamJournalState::Cancelled && !streams.resolved.contains(&s.id)
            }) {
                ensure!(
                    stream.record.route_namespace_sha256 == row.record.route_namespace_sha256
                        && stream.record.route_sha256 != row.record.route_sha256
                        && stream.record.registration.device_id
                            != row.record.registration.device_id,
                    "HSP stream blocks preparation claim"
                );
            }
            super::super::reconciliation::ensure_no_legacy_custody(
                db,
                &row.record.route_sha256,
                &row.record.route_namespace_sha256,
                &row.record.registration.device_id,
            )?;
            set_state(db, &receipt.id, HandyClockSyncState::Claimed, None)?;
            Ok(ClaimedHspPreparationReceipt {
                id: receipt.id.clone(),
                boot: boot.clone(),
            })
        })
    }
    pub fn complete_hsp_preparation(
        &mut self,
        receipt: &ClaimedHspPreparationReceipt,
        outcome: ClockSyncCompletion,
    ) -> Result<HspPreparationObservation> {
        self.hsp_preparation_transaction(|db, boot, all| {
            let row = all
                .rows
                .get(&receipt.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP preparation"))?;
            ensure!(
                &receipt.boot == boot && &row.record.boot == boot && !row.resolved,
                "retired HSP synchronization completion"
            );
            let (state, ack) = match outcome {
                ClockSyncCompletion::Acknowledged(ack) => {
                    (HandyClockSyncState::Acknowledged, Some(ack))
                }
                ClockSyncCompletion::Unknown => (HandyClockSyncState::Unknown, None),
            };
            check_ack(&row.record, state, ack.as_ref())?;
            if row.state != HandyClockSyncState::Claimed {
                ensure!(
                    row.state == state && row.acknowledgment == ack,
                    "original HSP synchronization outcome is immutable"
                );
            }
            set_state(db, &receipt.id, state, ack.as_ref())?;
            Ok(load(db)?
                .rows
                .remove(&receipt.id)
                .expect("updated original preparation"))
        })
    }
    /// Joined worker failure can retain ambiguity without reconstructing a claim.
    /// Exact original Ack/Unknown observations remain immutable metadata.
    pub fn mark_hsp_preparation_unknown_original(
        &mut self,
        id: &str,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<HspPreparationObservation> {
        self.hsp_preparation_transaction(|db, _, all| {
            let row = all
                .rows
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP preparation"))?;
            ensure!(
                &row.record.actor_session == actor && &row.record.request_id == request,
                "HSP original failure subject differs"
            );
            if matches!(
                row.state,
                HandyClockSyncState::Acknowledged | HandyClockSyncState::Unknown
            ) {
                return Ok(row.clone());
            }
            ensure!(
                row.state == HandyClockSyncState::Claimed && !row.resolved,
                "unclaimed HSP preparation cannot become an attempted outcome"
            );
            set_state(db, id, HandyClockSyncState::Unknown, None)?;
            Ok(load(db)?
                .rows
                .remove(id)
                .expect("retained original unknown"))
        })
    }
    pub fn cancel_hsp_preparation(
        &mut self,
        receipt: &PreparedHspPreparationReceipt,
    ) -> Result<HspPreparationObservation> {
        let row = self.hsp_preparation_observation(&receipt.id)?;
        ensure!(
            row.record.boot == receipt.boot,
            "HSP cancellation receipt changed"
        );
        self.cancel_hsp_preparation_original(
            &receipt.id,
            &row.record.actor_session,
            &row.record.request_id,
        )
    }
    pub fn cancel_hsp_preparation_original(
        &mut self,
        id: &str,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<HspPreparationObservation> {
        self.hsp_preparation_transaction(|db, _, all| {
            let row = all
                .rows
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP preparation"))?;
            ensure!(
                &row.record.actor_session == actor
                    && &row.record.request_id == request
                    && !row.resolved
                    && row.linked_stream_id.is_none()
                    && matches!(
                        row.state,
                        HandyClockSyncState::Prepared | HandyClockSyncState::Cancelled
                    ),
                "attempted HSP preparation cannot be cancelled as unsent"
            );
            set_state(db, id, HandyClockSyncState::Cancelled, None)?;
            Ok(load(db)?
                .rows
                .remove(id)
                .expect("cancelled original preparation"))
        })
    }
    pub fn hsp_preparation_observation(&self, id: &str) -> Result<HspPreparationObservation> {
        let tx = self.db.unchecked_transaction()?;
        owner(&tx, &self.boot)?;
        let value = load(&tx)?
            .rows
            .remove(id)
            .ok_or_else(|| anyhow::anyhow!("missing HSP preparation"))?;
        tx.commit()?;
        Ok(value)
    }
    pub fn hsp_preparation_membership(&self, id: &str) -> Result<HspPreparationMembership> {
        Ok(membership(&self.hsp_preparation_observation(id)?))
    }
    pub fn hsp_preparation_history(
        &self,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<Option<HspPreparationObservation>> {
        let tx = self.db.unchecked_transaction()?;
        owner(&tx, &self.boot)?;
        let result = load(&tx)?
            .rows
            .into_values()
            .find(|r| &r.record.actor_session == actor && &r.record.request_id == request);
        tx.commit()?;
        Ok(result)
    }
    pub fn prepare_hsp_stream_from_preparation(
        &mut self,
        parent: &PreparedHspPreparationReceipt,
        record: &HspStreamRecord,
        effect: &HspEffect,
        qualification: &HspQualificationLinkRecord,
    ) -> Result<(HspStreamReceipt, PreparedHspReceipt)> {
        self.hsp_transaction(|db, boot, all| {
            let preparations = load(db)?;
            let row = preparations
                .rows
                .get(&parent.id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP preparation parent"))?;
            ensure!(
                &parent.boot == boot
                    && &row.record.boot == boot
                    && row.state == HandyClockSyncState::Acknowledged
                    && !row.resolved
                    && !row.custody_transferred,
                "HSP preparation cannot hand off again"
            );
            check_link(&row.record, record, qualification)?;
            let stream_id = record.id()?;
            ensure!(
                row.linked_stream_id
                    .as_ref()
                    .is_none_or(|id| id == &stream_id),
                "HSP preparation already names another child"
            );
            ensure!(
                effect.kind() == crate::handy_hsp_runtime::HspEffectKind::Setup
                    && effect.sequence() == 1,
                "HSP handoff must begin with original Setup"
            );
            let fresh = !all.streams.contains_key(&stream_id);
            ensure!(
                fresh || all.links.get(&stream_id) == Some(qualification),
                "HSP handoff qualification differs"
            );
            let stream = prepare_stream(db, boot, record, all, Some(parent.id()))?;
            if fresh {
                recovery::insert_link(db, all, qualification)?;
            }
            let prepared = prepare_effect(db, boot, &stream, effect, all)?;
            if row.linked_stream_id.is_none() {
                let link = StreamLink {
                    preparation: parent.id.clone(),
                    stream: stream_id,
                    qualification_link_sha256: qualification.id()?,
                };
                db.execute(
                    "INSERT INTO hsp_preparation_links VALUES(?1,?2,?3,0,?4)",
                    params![
                        link.preparation,
                        link.stream,
                        serde_json::to_string(&link)?,
                        link_lifecycle(&link, false)?
                    ],
                )?;
                db.execute(
                    "UPDATE hsp_preparation_census SET links=links+1 WHERE singleton=1",
                    [],
                )?;
            }
            load(db)?;
            Ok((stream, prepared))
        })
    }
}

impl DeviceEffectJournal {
    pub(crate) fn verify_hsp_preparation_records(
        state_dir: &Path,
        witnesses: &[(SessionId, RequestId, String)],
    ) -> Result<()> {
        ensure!(
            witnesses.len() <= 2048,
            "HSP preparation witnesses exceed bound"
        );
        if witnesses.is_empty() {
            return Ok(());
        }
        let path = state_dir.join("device-effects/journal.sqlite3");
        let _held = crate::artifacts::open_regular(&path)?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        let tx = db.unchecked_transaction()?;
        ensure!(
            Self::validate_existing(&tx)? >= 12,
            "HSP preparation journal unavailable"
        );
        let all = load(&tx)?;
        for (actor, request, id) in witnesses {
            let row = all
                .rows
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("HSP preparation witness history missing"))?;
            ensure!(
                &row.record.actor_session == actor && &row.record.request_id == request,
                "HSP preparation witness actor/request differs"
            );
        }
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
#[path = "device_hsp_preparation_journal_tests.rs"]
mod tests;
