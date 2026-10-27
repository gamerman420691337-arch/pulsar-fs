//! Immutable HSP admission links, retrospective emergency observations and exact
//! recovery membership. These records never recreate native stopping authority.
use super::*;
use crate::handy_hsp_records::{HspEmergencyRecord, HspQualificationLinkRecord};
use std::collections::BTreeSet;

const LINKS: &str = "CREATE TABLE hsp_qualification_links(stream TEXT PRIMARY KEY NOT NULL,id TEXT NOT NULL UNIQUE,record TEXT NOT NULL)";
const EMERGENCIES: &str = "CREATE TABLE hsp_emergency_observations(id TEXT PRIMARY KEY NOT NULL,stream TEXT NOT NULL UNIQUE,effect_id TEXT NOT NULL,record TEXT NOT NULL)";
const RESOLUTIONS: &str = "CREATE TABLE hsp_resolution_members(stream TEXT PRIMARY KEY NOT NULL,resolution TEXT NOT NULL,observation_sha256 TEXT NOT NULL,membership_sha256 TEXT NOT NULL)";
const CENSUS: &str = "CREATE TABLE hsp_recovery_census(singleton INTEGER PRIMARY KEY CHECK(singleton=1),links INTEGER NOT NULL CHECK(links BETWEEN 0 AND 64),emergencies INTEGER NOT NULL CHECK(emergencies BETWEEN 0 AND 64),emergency_bytes INTEGER NOT NULL CHECK(emergency_bytes BETWEEN 0 AND 1048576),resolutions INTEGER NOT NULL CHECK(resolutions BETWEEN 0 AND 64))";
pub(super) const OBJECTS: [(&str, &str, &str); 4] = [
    ("hsp_qualification_links", "table", LINKS),
    ("hsp_emergency_observations", "table", EMERGENCIES),
    ("hsp_resolution_members", "table", RESOLUTIONS),
    ("hsp_recovery_census", "table", CENSUS),
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspEffectMembership {
    pub id: String,
    pub sequence: u64,
    pub kind: HspEffectRecordKind,
    pub state: String,
    pub outcome_sha256: Option<String>,
    pub late_sha256: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspEmergencyMembership {
    pub id: String,
    pub effect_id: String,
    pub sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspStreamMembership {
    pub id: String,
    pub record: HspStreamRecord,
    pub qualification_link: Option<HspQualificationLinkRecord>,
    pub state: String,
    #[serde(
        deserialize_with = "crate::handy_hsp_records::bounded_vec::<_, HspEffectMembership, 65537>"
    )]
    pub effects: Vec<HspEffectMembership>,
    pub emergency: Option<HspEmergencyMembership>,
}
impl HspStreamMembership {
    pub fn stream_id(&self) -> &str {
        &self.id
    }
    pub fn validate(&self) -> Result<()> {
        self.record.validate()?;
        ensure!(
            self.id == self.record.id()?,
            "HSP membership stream differs"
        );
        ensure!(
            matches!(self.state.as_str(), "prepared" | "custody" | "cancelled"),
            "invalid HSP membership state"
        );
        if let Some(link) = &self.qualification_link {
            link.validate(&self.record)?;
        }
        ensure!(
            self.effects.len() <= ORDINARY_CAPACITY as usize + 1,
            "HSP effect membership exceeds bound"
        );
        ensure!(
            self.effects
                .windows(2)
                .all(|w| w[0].sequence < w[1].sequence),
            "HSP membership effect order differs"
        );
        let mut ids = BTreeSet::new();
        for effect in &self.effects {
            validate_sha256(&effect.id)?;
            ensure!(
                effect.sequence > 0 && ids.insert(&effect.id),
                "HSP membership duplicate effect"
            );
            let state = HspJournalState::parse(&effect.state)?;
            for hash in effect
                .outcome_sha256
                .iter()
                .chain(effect.late_sha256.iter())
            {
                validate_sha256(hash)?;
            }
            ensure!(
                effect.outcome_sha256.is_some()
                    == matches!(
                        state,
                        HspJournalState::Acknowledged | HspJournalState::Unknown
                    )
                    && (effect.late_sha256.is_none() || state == HspJournalState::Unknown),
                "HSP membership observation differs"
            );
        }
        if let Some(emergency) = &self.emergency {
            validate_sha256(&emergency.id)?;
            validate_sha256(&emergency.effect_id)?;
            ensure!(emergency.sequence > 0, "invalid HSP emergency sequence");
        }
        Ok(())
    }
    pub fn observation_sha256(&self) -> Result<String> {
        self.validate()?;
        digest("pulsar.hsp.custody.observation.v1", self)
    }
    pub fn identity_sha256(&self) -> Result<String> {
        self.validate()?;
        digest(
            "pulsar.hsp.custody.identity.v1",
            &(
                &self.id,
                &self.qualification_link,
                self.effects
                    .iter()
                    .map(|e| (&e.id, e.sequence, e.kind))
                    .collect::<Vec<_>>(),
                &self.emergency,
            ),
        )
    }
    pub fn extends_with_stops(&self, after: &Self) -> Result<()> {
        self.validate()?;
        after.validate()?;
        ensure!(
            self.id == after.id
                && self.record == after.record
                && self.qualification_link == after.qualification_link,
            "HSP recovery original stream or qualification changed"
        );
        ensure!(
            self.state == after.state
                || (self.state == "prepared"
                    && matches!(after.state.as_str(), "custody" | "cancelled")),
            "HSP custody state regressed"
        );
        for original in &self.effects {
            let current = after
                .effects
                .iter()
                .find(|e| e.id == original.id)
                .ok_or_else(|| anyhow::anyhow!("HSP original effect disappeared"))?;
            ensure!(
                original.sequence == current.sequence && original.kind == current.kind,
                "HSP original effect changed"
            );
            let before = HspJournalState::parse(&original.state)?;
            let next = HspJournalState::parse(&current.state)?;
            ensure!(
                before == next
                    || matches!(
                        (before, next),
                        (
                            HspJournalState::Prepared,
                            HspJournalState::Claimed
                                | HspJournalState::Acknowledged
                                | HspJournalState::Unknown
                                | HspJournalState::Cancelled
                        ) | (
                            HspJournalState::Claimed,
                            HspJournalState::Acknowledged | HspJournalState::Unknown
                        )
                    ),
                "HSP recovery effect state changed"
            );
            ensure!(
                original.late_sha256 == current.late_sha256,
                "HSP recovery gained a late observation"
            );
            if before == next || original.outcome_sha256.is_some() {
                ensure!(
                    original.outcome_sha256 == current.outcome_sha256
                        && original.late_sha256 == current.late_sha256,
                    "HSP recovery original observation changed"
                );
            }
        }
        for current in &after.effects {
            if !self.effects.iter().any(|e| e.id == current.id) {
                ensure!(
                    current.kind == HspEffectRecordKind::Stop,
                    "HSP recovery gained ordinary work"
                );
            }
        }
        if let Some(original) = &self.emergency {
            ensure!(
                after.emergency.as_ref() == Some(original),
                "HSP emergency observation changed"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspEmergencyReceipt {
    id: String,
    stream_id: String,
}
impl HspEmergencyReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
}
/// A current verified committed resolution. It carries no transport authority.
#[derive(Clone, Debug)]
pub struct HspResolvedReceipt {
    device: DeviceId,
    route_sha256: String,
    route_namespace_sha256: String,
    resolution_id: String,
    memberships: Vec<HspStreamMembership>,
    preparations: Vec<HspPreparationMembership>,
}
impl HspResolvedReceipt {
    pub fn matches_target(&self, device: &DeviceId, route: &str, namespace: &str) -> bool {
        &self.device == device
            && self.route_sha256 == route
            && self.route_namespace_sha256 == namespace
    }
    pub fn contains_exact_preparations(&self, expected: &[HspPreparationMembership]) -> bool {
        let mut ids = BTreeSet::new();
        expected.len() <= self.preparations.len()
            && expected
                .iter()
                .all(|m| ids.insert(&m.id) && self.preparations.iter().any(|p| p == m))
    }
    pub fn resolution_id(&self) -> &str {
        &self.resolution_id
    }
    pub fn stream_ids(&self) -> Vec<&str> {
        self.memberships.iter().map(|m| m.id.as_str()).collect()
    }
    pub fn matches_memberships(&self, expected: &[HspStreamMembership]) -> bool {
        self.memberships == expected
    }
    pub fn contains_exact_memberships(&self, expected: &[HspStreamMembership]) -> bool {
        let mut ids = BTreeSet::new();
        expected.len() <= self.memberships.len()
            && expected
                .iter()
                .all(|m| ids.insert(&m.id) && self.memberships.iter().any(|current| current == m))
    }
}
fn membership(all: &Catalog, id: &str) -> Result<HspStreamMembership> {
    let stream = all
        .streams
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("missing HSP stream"))?;
    let effects = stream
        .effect_ids
        .iter()
        .map(|id| {
            let e = all
                .effects
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("missing HSP effect"))?;
            Ok(HspEffectMembership {
                id: id.clone(),
                sequence: e.record.sequence,
                kind: e.record.kind,
                state: e.state.name().into(),
                outcome_sha256: e
                    .outcome
                    .as_ref()
                    .map(|o| digest("pulsar.hsp.membership.outcome.v1", o))
                    .transpose()?,
                late_sha256: e
                    .late_observation
                    .as_ref()
                    .map(|o| digest("pulsar.hsp.membership.late.v1", o))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let emergency = all
        .emergencies
        .get(id)
        .map(|record| -> Result<HspEmergencyMembership> {
            Ok(HspEmergencyMembership {
                id: record.id()?,
                effect_id: record.effect.id()?,
                sequence: record.effect.sequence,
            })
        })
        .transpose()?;
    let value = HspStreamMembership {
        id: id.into(),
        record: stream.record.clone(),
        qualification_link: all.links.get(id).cloned(),
        state: match stream.state {
            HspStreamJournalState::Prepared => "prepared",
            HspStreamJournalState::Custody => "custody",
            HspStreamJournalState::Cancelled => "cancelled",
        }
        .into(),
        effects,
        emergency,
    };
    value.validate()?;
    Ok(value)
}
fn membership_hash(resolution: &str, value: &HspStreamMembership) -> Result<String> {
    digest(
        "pulsar.hsp.resolution.membership.v1",
        &(resolution, &value.id, value.observation_sha256()?),
    )
}
pub(super) fn validate_census(db: &Connection, all: &Catalog) -> Result<()> {
    if db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? < 11 {
        return Ok(());
    }
    let values:Vec<(u64,u64,u64,u64,u64)>=db.prepare("SELECT singleton,links,emergencies,emergency_bytes,resolutions FROM hsp_recovery_census LIMIT 2")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
    for emergency in all.emergencies.values() {
        validate_emergency(all, emergency)?;
    }
    let bytes = all
        .emergencies
        .values()
        .map(|r| serde_json::to_vec(r).map(|v| v.len() as u64))
        .collect::<serde_json::Result<Vec<_>>>()?
        .iter()
        .sum::<u64>();
    ensure!(
        values
            == vec![(
                1,
                all.links.len() as u64,
                all.emergencies.len() as u64,
                bytes,
                all.resolved.len() as u64
            )],
        "HSP recovery census differs"
    );
    ensure!(
        all.links.len() <= 64
            && all.emergencies.len() <= 64
            && all.resolved.len() <= 64
            && bytes <= 1_048_576,
        "HSP recovery capacity exceeded"
    );
    Ok(())
}
pub(super) fn load(db: &Connection, all: &mut Catalog) -> Result<()> {
    if db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? < 11 {
        return Ok(());
    }
    let mut q=db.prepare("SELECT CASE WHEN length(stream)=64 THEN stream ELSE NULL END,CASE WHEN length(id)=64 THEN id ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=4096 THEN record ELSE NULL END FROM hsp_qualification_links LIMIT 65")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (stream, id, text) = row?;
        let value: HspQualificationLinkRecord = serde_json::from_str(&text)?;
        let parent = all
            .streams
            .get(&stream)
            .ok_or_else(|| anyhow::anyhow!("orphan HSP qualification link"))?;
        value.validate(&parent.record)?;
        ensure!(
            value.stream_id == stream
                && value.id()? == id
                && serde_json::to_string(&value)? == text,
            "HSP qualification link integrity differs"
        );
        all.links.insert(stream, value);
    }
    let mut q=db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END,CASE WHEN length(stream)=64 THEN stream ELSE NULL END,CASE WHEN length(effect_id)=64 THEN effect_id ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=16384 THEN record ELSE NULL END FROM hsp_emergency_observations LIMIT 65")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, stream, effect_id, text) = row?;
        let value: HspEmergencyRecord = serde_json::from_str(&text)?;
        validate_emergency(all, &value)?;
        ensure!(
            value.id()? == id
                && value.stream_id == stream
                && value.effect.id()? == effect_id
                && serde_json::to_string(&value)? == text,
            "HSP emergency integrity differs"
        );
        all.emergencies.insert(stream, value);
    }
    let mut q=db.prepare("SELECT CASE WHEN length(stream)=64 THEN stream ELSE NULL END,CASE WHEN length(resolution)=64 THEN resolution ELSE NULL END,CASE WHEN length(observation_sha256)=64 THEN observation_sha256 ELSE NULL END,CASE WHEN length(membership_sha256)=64 THEN membership_sha256 ELSE NULL END FROM hsp_resolution_members LIMIT 65")?;
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (stream, resolution, observation, hash) = row?;
        let value = membership(all, &stream)?;
        let text:String=db.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM custody_resolutions WHERE id=?1",[&resolution],|r|r.get(0))?;
        let record: super::super::reconciliation::CustodyResolutionRecord =
            serde_json::from_str(&text)?;
        ensure!(
            record.id()? == resolution
                && record.snapshot.hsp_streams.iter().any(|m| m == &value)
                && observation == value.observation_sha256()?
                && hash == membership_hash(&resolution, &value)?,
            "HSP resolution original membership differs"
        );
        all.resolved.insert(stream);
    }
    validate_census(db, all)
}
fn validate_emergency(all: &Catalog, value: &HspEmergencyRecord) -> Result<()> {
    let stream = all
        .streams
        .get(&value.stream_id)
        .ok_or_else(|| anyhow::anyhow!("orphan HSP emergency observation"))?;
    value.validate(&stream.record)?;
    ensure!(
        stream.state == HspStreamJournalState::Custody,
        "HSP emergency lacks original attempted stream"
    );
    for id in &stream.effect_ids {
        let effect = &all.effects[id];
        if effect.record.kind == HspEffectRecordKind::Stop {
            ensure!(
                effect.record == value.effect
                    && matches!(
                        effect.state,
                        HspJournalState::Prepared | HspJournalState::Cancelled
                    ),
                "HSP emergency overlaps a durable Stop attempt"
            );
        } else {
            ensure!(
                effect.record.sequence < value.effect.sequence,
                "HSP emergency precedes original ordinary operation"
            );
        }
    }
    Ok(())
}
pub(super) fn insert_link(
    db: &Connection,
    all: &mut Catalog,
    link: &HspQualificationLinkRecord,
) -> Result<()> {
    let stream = all
        .streams
        .get(&link.stream_id)
        .ok_or_else(|| anyhow::anyhow!("missing linked HSP stream"))?;
    link.validate(&stream.record)?;
    ensure!(
        !all.links.contains_key(&link.stream_id),
        "HSP admission already linked"
    );
    db.execute(
        "INSERT INTO hsp_qualification_links VALUES(?1,?2,?3)",
        params![link.stream_id, link.id()?, serde_json::to_string(link)?],
    )?;
    db.execute(
        "UPDATE hsp_recovery_census SET links=links+1 WHERE singleton=1",
        [],
    )?;
    all.links.insert(link.stream_id.clone(), link.clone());
    Ok(())
}
pub(crate) fn selected_memberships(
    db: &Connection,
    device: &DeviceId,
    route: &str,
    namespace: &str,
) -> Result<Vec<HspStreamMembership>> {
    let all = load_catalog(db)?;
    let mut result = Vec::new();
    for stream in all.streams.values() {
        if all.resolved.contains(&stream.id)
            || (stream.state == HspStreamJournalState::Cancelled
                && !super::preparation::include_cancelled_child(db, &stream.id)?)
        {
            continue;
        }

        ensure!(
            stream.record.route_namespace_sha256 == namespace,
            "unresolved HSP custody uses foreign namespace"
        );
        if &stream.record.registration.device_id == device || stream.record.route_sha256 == route {
            ensure!(
                stream.record.route_sha256 == route,
                "selected HSP custody uses another route"
            );
            result.push(membership(&all, &stream.id)?);
        }
    }
    Ok(result)
}
pub(crate) fn verify_resolution_members(
    db: &Connection,
    resolution: &str,
    expected: &[HspStreamMembership],
) -> Result<()> {
    if db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? < 11 {
        ensure!(expected.is_empty(), "HSP resolution requires v11");
        return Ok(());
    }
    let all = if expected.is_empty() {
        None
    } else {
        Some(load_catalog(db)?)
    };
    for value in expected {
        ensure!(
            membership(all.as_ref().expect("nonempty selection"), &value.id)? == *value,
            "resolved HSP history changed"
        );
        let (id,observation,hash):(String,String,String)=db.query_row("SELECT resolution,observation_sha256,membership_sha256 FROM hsp_resolution_members WHERE stream=?1",[&value.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        ensure!(
            id == resolution
                && observation == value.observation_sha256()?
                && hash == membership_hash(resolution, value)?,
            "HSP resolution membership differs"
        );
    }
    let count: usize = db.query_row(
        "SELECT count(*) FROM hsp_resolution_members WHERE resolution=?1",
        [resolution],
        |r| r.get(0),
    )?;
    ensure!(
        count == expected.len(),
        "HSP resolution membership census differs"
    );
    Ok(())
}
pub(crate) fn insert_resolution(
    db: &Connection,
    resolution: &str,
    values: &[HspStreamMembership],
) -> Result<()> {
    let all = if values.is_empty() {
        None
    } else {
        Some(load_catalog(db)?)
    };
    for value in values {
        ensure!(
            membership(all.as_ref().expect("nonempty selection"), &value.id)? == *value,
            "HSP resolution subject changed"
        );
        db.execute(
            "INSERT INTO hsp_resolution_members VALUES(?1,?2,?3,?4)",
            params![
                value.id,
                resolution,
                value.observation_sha256()?,
                membership_hash(resolution, value)?
            ],
        )?;
        db.execute(
            "UPDATE hsp_recovery_census SET resolutions=resolutions+1 WHERE singleton=1",
            [],
        )?;
    }
    Ok(())
}
impl DeviceEffectJournal {
    pub fn prepare_hsp_stream_effect_linked(
        &mut self,
        record: &HspStreamRecord,
        effect: &HspEffect,
        link: &HspQualificationLinkRecord,
    ) -> Result<(HspStreamReceipt, PreparedHspReceipt)> {
        self.hsp_transaction(|db, boot, all| {
            link.validate(record)?;
            let id = record.id()?;
            let fresh = !all.streams.contains_key(&id);
            if !fresh {
                ensure!(
                    all.links.get(&id) == Some(link),
                    "historical unlinked HSP stream cannot gain a guessed admission"
                );
            }
            let stream = prepare_stream(db, boot, record, all, None)?;
            if fresh {
                insert_link(db, all, link)?;
            }
            let effect = prepare_effect(db, boot, &stream, effect, all)?;
            Ok((stream, effect))
        })
    }
    pub fn persist_hsp_emergency(
        &mut self,
        stream: &HspStreamRecord,
        observation: &crate::handy_hsp_dispatch::HspEmergencyStopObservation,
    ) -> Result<HspEmergencyReceipt> {
        let value = HspEmergencyRecord::capture(stream, observation)?;
        self.persist_hsp_emergency_record(&value)
    }
    fn persist_hsp_emergency_record(
        &mut self,
        value: &HspEmergencyRecord,
    ) -> Result<HspEmergencyReceipt> {
        self.hsp_transaction(|db,boot,all| {
            let id=value.id()?;
            if let Some(prior)=all.emergencies.get(&value.stream_id) {
                ensure!(prior==value,"HSP emergency original observation is immutable");
                return Ok(HspEmergencyReceipt {id,stream_id:value.stream_id.clone()});
            }
            let stream=all.streams.get(&value.stream_id).ok_or_else(||anyhow::anyhow!("missing HSP emergency stream"))?;
            ensure!(&stream.record.boot==boot && !all.resolved.contains(&value.stream_id),"retired or resolved HSP emergency history");
            validate_emergency(all,value)?;
            let text=serde_json::to_string(&value)?;
            db.execute("INSERT INTO hsp_emergency_observations VALUES(?1,?2,?3,?4)",params![id,value.stream_id,value.effect.id()?,text])?;
            db.execute("UPDATE hsp_recovery_census SET emergencies=emergencies+1,emergency_bytes=emergency_bytes+?1 WHERE singleton=1",[text.len() as u64])?;
            all.emergencies.insert(value.stream_id.clone(),value.clone());
            Ok(HspEmergencyReceipt {id,stream_id:value.stream_id.clone()})
        })
    }
    pub fn hsp_emergency_for_stream(&self, id: &str) -> Result<Option<HspEmergencyRecord>> {
        self.hsp_read(|all| {
            ensure!(all.streams.contains_key(id), "missing HSP stream");
            Ok(all.emergencies.get(id).cloned())
        })
    }
    pub fn hsp_stream_membership(&self, id: &str) -> Result<HspStreamMembership> {
        self.hsp_read(|all| membership(all, id))
    }
    pub fn hsp_resolution_receipt(
        &self,
        resolution: &str,
        expected: &[HspStreamMembership],
    ) -> Result<HspResolvedReceipt> {
        self.check_owner()?;
        validate_sha256(resolution)?;
        ensure!(
            expected.len() <= 64 && expected.windows(2).all(|w| w[0].id < w[1].id),
            "HSP resolved receipt selection invalid"
        );
        let tx = self.db.unchecked_transaction()?;
        owner(&tx, &self.boot)?;
        super::super::reconciliation::validate_schema(&tx, 11)?;
        verify_resolution_members(&tx, resolution, expected)?;
        let text:String=tx.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM custody_resolutions WHERE id=?1",[resolution],|r|r.get(0))?;
        let record: super::super::reconciliation::CustodyResolutionRecord =
            serde_json::from_str(&text)?;
        ensure!(
            record.id()? == resolution
                && (!record.snapshot.hsp_streams.is_empty()
                    || !record.snapshot.hsp_preparations.is_empty()),
            "HSP resolution identity differs or lacks HSP custody"
        );
        tx.commit()?;
        Ok(HspResolvedReceipt {
            device: record.snapshot.device,
            route_sha256: record.snapshot.route_sha256,
            route_namespace_sha256: record.snapshot.route_namespace_sha256,
            resolution_id: resolution.into(),
            memberships: expected.to_vec(),
            preparations: record.snapshot.hsp_preparations,
        })
    }
}

#[cfg(test)]
pub(super) fn persist_authored_emergency(
    journal: &mut DeviceEffectJournal,
    value: &HspEmergencyRecord,
) -> Result<HspEmergencyReceipt> {
    journal.persist_hsp_emergency_record(value)
}
