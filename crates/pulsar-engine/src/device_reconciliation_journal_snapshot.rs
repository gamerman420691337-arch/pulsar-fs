//! Bounded original custody subjects. A snapshot never grants native authority.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MEMBER_LIMIT: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CustodyKind {
    Setup,
    ClockSync,
    Play,
    Effect,
}
impl CustodyKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::ClockSync => "clock_sync",
            Self::Play => "play",
            Self::Effect => "effect",
        }
    }
    pub(super) fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "setup" => Self::Setup,
            "clock_sync" => Self::ClockSync,
            "play" => Self::Play,
            "effect" => Self::Effect,
            _ => anyhow::bail!("invalid custody member kind"),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
impl State {
    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "prepared" => Self::Prepared,
            "claimed" => Self::Claimed,
            "acknowledged" => Self::Acknowledged,
            "unknown" => Self::Unknown,
            "cancelled" => Self::Cancelled,
            _ => anyhow::bail!("invalid custody state"),
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
    fn advances_to(self, after: Self) -> bool {
        self == after
            || matches!(
                (self, after),
                (
                    Self::Prepared,
                    Self::Claimed | Self::Acknowledged | Self::Unknown
                ) | (Self::Claimed, Self::Acknowledged | Self::Unknown)
            )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    boot: DeviceSessionId,
    revision: RevisionId,
    route: Option<String>,
    namespace: Option<String>,
    program: ArtifactId,
    qualification: ArtifactId,
    qualification_revision: Option<u64>,
    admission_sha256: Option<String>,
    stop: bool,
    setup: Option<String>,
    play: Option<String>,
    effect: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CustodyMember {
    pub kind: CustodyKind,
    pub id: String,
    pub project: ProjectId,
    pub device: DeviceId,
    pub device_session: DeviceSessionId,
    pub record_sha256: String,
    pub observation_sha256: String,
    subject: Subject,
    state: State,
    emergency: bool,
    effect_record: Option<EffectRecord>,
}
impl CustodyMember {
    pub(super) fn key(&self) -> (CustodyKind, String) {
        (self.kind, self.id.clone())
    }
    fn immutable_eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.id == other.id
            && self.project == other.project
            && self.device == other.device
            && self.device_session == other.device_session
            && self.record_sha256 == other.record_sha256
            && self.subject == other.subject
    }
    fn advances_to(&self, other: &Self) -> bool {
        self.immutable_eq(other)
            && self.effect_record == other.effect_record
            && self.state.advances_to(other.state)
            && (self.state != other.state
                || self.observation_sha256 == other.observation_sha256
                || (self.kind == CustodyKind::Effect
                    && !self.emergency
                    && other.emergency
                    && matches!(self.state, State::Acknowledged | State::Unknown)))
            && (!self.emergency || other.emergency)
    }
    fn stop_matches(&self, source: &Self) -> bool {
        self.kind == CustodyKind::Effect
            && self.subject.stop
            && self
                .effect_record
                .as_ref()
                .is_some_and(|record| super::super::emergency_stop::stop_shape(record).is_ok())
            && matches!(
                source.kind,
                CustodyKind::Setup | CustodyKind::Play | CustodyKind::Effect
            )
            && self.project == source.project
            && self.device == source.device
            && self.device_session == source.device_session
            && self.subject.boot == source.subject.boot
            && self.subject.revision == source.subject.revision
            && self.subject.program == source.subject.program
            && self.subject.qualification == source.subject.qualification
            && (source.subject.admission_sha256.is_none()
                || self.subject.admission_sha256 == source.subject.admission_sha256)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CustodySnapshot {
    pub device: DeviceId,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
    pub members: Vec<CustodyMember>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::handy_hsp_records::bounded_vec::<_, crate::device_effect_journal::HspStreamMembership, 64>"
    )]
    pub hsp_streams: Vec<crate::device_effect_journal::HspStreamMembership>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::handy_hsp_records::bounded_vec::<_, crate::device_effect_journal::HspPreparationMembership, 64>"
    )]
    pub hsp_preparations: Vec<crate::device_effect_journal::HspPreparationMembership>,
}
impl CustodySnapshot {
    pub(crate) fn id(&self) -> Result<String> {
        self.validate()?;
        let subjects = self
            .members
            .iter()
            .map(|m| (m.kind, &m.id, &m.record_sha256))
            .collect::<Vec<_>>();
        let legacy = (
            &self.device,
            &self.route_sha256,
            &self.route_namespace_sha256,
            subjects,
        );
        let base_id = if self.hsp_streams.is_empty() {
            digest(&serde_json::to_vec(&legacy)?)
        } else {
            let hsp = self
                .hsp_streams
                .iter()
                .map(|m| m.identity_sha256())
                .collect::<Result<Vec<_>>>()?;
            digest(&serde_json::to_vec(&(
                "custody-snapshot-hsp-v1",
                legacy,
                hsp,
            ))?)
        };
        if self.hsp_preparations.is_empty() {
            return Ok(base_id);
        }
        Ok(digest(&serde_json::to_vec(&(
            "custody-snapshot-hsp-preparation-v1",
            base_id,
            self.hsp_preparations
                .iter()
                .map(|m| (&m.id, &m.linked_stream_id))
                .collect::<Vec<_>>(),
        ))?))
    }
    pub(crate) fn counts(&self) -> HandyCustodyCounts {
        let mut n = HandyCustodyCounts {
            setup: 0,
            clock_sync: 0,
            play: 0,
            effects: 0,
            hsp_streams: self.hsp_streams.len() as u32,
            hsp_preparations: self.hsp_preparations.len() as u32,
        };
        for m in &self.members {
            match m.kind {
                CustodyKind::Setup => n.setup += 1,
                CustodyKind::ClockSync => n.clock_sync += 1,
                CustodyKind::Play => n.play += 1,
                CustodyKind::Effect => n.effects += 1,
            }
        }
        n
    }
    pub(crate) fn projects(&self) -> Vec<ProjectId> {
        let mut values = self
            .members
            .iter()
            .map(|m| m.project.clone())
            .collect::<Vec<_>>();
        values.extend(
            self.hsp_streams
                .iter()
                .map(|m| m.record.binding.identity.live.project.clone()),
        );
        values.extend(
            self.hsp_preparations
                .iter()
                .map(|m| m.record.project_id.clone()),
        );
        values.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        values.dedup();
        values
    }
    pub(crate) fn devices(&self) -> Vec<DeviceId> {
        let mut values = self
            .members
            .iter()
            .map(|m| m.device.clone())
            .collect::<Vec<_>>();
        values.push(self.device.clone());
        values.extend(
            self.hsp_streams
                .iter()
                .map(|m| m.record.registration.device_id.clone()),
        );
        values.extend(
            self.hsp_preparations
                .iter()
                .map(|m| m.record.registration.device_id.clone()),
        );
        values.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        values.dedup();
        values
    }
    pub(crate) fn historical_qualifications(&self) -> Result<Vec<HandyQualificationBinding>> {
        self.validate()?;
        let mut bindings = BTreeMap::new();
        for member in &self.members {
            if let Some(revision) = member.subject.qualification_revision {
                bindings.insert(
                    (member.subject.qualification.to_string(), revision),
                    HandyQualificationBinding {
                        dossier_id: member.subject.qualification.clone(),
                        revision,
                    },
                );
            } else if member.kind == CustodyKind::Effect {
                let mut matches = BTreeMap::new();
                for source in &self.members {
                    if matches!(source.kind, CustodyKind::Setup | CustodyKind::Play)
                        && member.project == source.project
                        && member.device == source.device
                        && member.device_session == source.device_session
                        && member.subject.boot == source.subject.boot
                        && member.subject.revision == source.subject.revision
                        && member.subject.program == source.subject.program
                        && member.subject.qualification == source.subject.qualification
                        && (source.subject.admission_sha256.is_none()
                            || member.subject.admission_sha256 == source.subject.admission_sha256)
                    {
                        let revision = source.subject.qualification_revision.ok_or_else(|| {
                            anyhow::anyhow!("original qualification revision missing")
                        })?;
                        matches.insert(
                            (source.subject.qualification.to_string(), revision),
                            HandyQualificationBinding {
                                dossier_id: source.subject.qualification.clone(),
                                revision,
                            },
                        );
                    }
                }
                ensure!(
                    matches.len() == 1,
                    "generic effect qualification binding is missing or ambiguous"
                );
                bindings.extend(matches);
            } else {
                anyhow::bail!("custody qualification binding is missing");
            }
        }
        for stream in &self.hsp_streams {
            let link = stream.qualification_link.as_ref().ok_or_else(|| {
                anyhow::anyhow!("historical HSP stream lacks original qualification link")
            })?;
            link.validate(&stream.record)?;
            bindings.insert(
                (
                    link.qualification.dossier_id.to_string(),
                    link.qualification.revision,
                ),
                link.qualification.clone(),
            );
        }
        for preparation in &self.hsp_preparations {
            bindings.insert(
                (
                    preparation.record.qualification.dossier_id.to_string(),
                    preparation.record.qualification.revision,
                ),
                preparation.record.qualification.clone(),
            );
        }
        Ok(bindings.into_values().collect())
    }
    pub(crate) fn hsp_stream_ids(&self) -> Vec<&str> {
        self.hsp_streams.iter().map(|m| m.id.as_str()).collect()
    }
    pub(crate) fn validate_transport(&self, transport: &HandyQualificationTransport) -> Result<()> {
        ensure!(
            match transport {
                HandyQualificationTransport::Hssp =>
                    self.hsp_streams.is_empty() && self.hsp_preparations.is_empty(),
                HandyQualificationTransport::Hsp =>
                    self.members.is_empty()
                        && (!self.hsp_streams.is_empty() || !self.hsp_preparations.is_empty()),
            },
            "recovery qualification transport does not match exact custody"
        );
        Ok(())
    }
    pub(super) fn validate(&self) -> Result<()> {
        DeviceId::new(self.device.as_str())?;
        validate_sha256(&self.route_sha256)?;
        validate_sha256(&self.route_namespace_sha256)?;
        ensure!(
            self.members.len() <= MEMBER_LIMIT,
            "custody snapshot exceeds bound"
        );
        ensure!(
            self.members.windows(2).all(|w| w[0].key() < w[1].key()),
            "custody members are duplicated or unsorted"
        );
        ensure!(
            self.hsp_streams.len() <= 64 && self.hsp_streams.windows(2).all(|w| w[0].id < w[1].id),
            "HSP custody stream membership exceeds bound or differs"
        );
        ensure!(
            self.hsp_preparations.len() <= 64
                && self.hsp_preparations.windows(2).all(|w| w[0].id < w[1].id),
            "HSP preparation custody exceeds bound or differs"
        );
        for preparation in &self.hsp_preparations {
            preparation.validate()?;
            ensure!(
                preparation.state != HandyClockSyncState::Cancelled
                    && preparation.record.route_sha256 == self.route_sha256
                    && preparation.record.route_namespace_sha256 == self.route_namespace_sha256,
                "HSP preparation snapshot target differs"
            );
            if let Some(child) = &preparation.linked_stream_id {
                ensure!(
                    self.hsp_streams.iter().any(|s| &s.id == child),
                    "HSP preparation snapshot lacks exact linked child"
                );
            }
        }
        for stream in &self.hsp_streams {
            stream.validate()?;
            ensure!(
                (stream.state != "cancelled"
                    || self
                        .hsp_preparations
                        .iter()
                        .any(|p| p.linked_stream_id.as_ref() == Some(&stream.id)
                            && !p.custody_transferred))
                    && stream.record.route_sha256 == self.route_sha256
                    && stream.record.route_namespace_sha256 == self.route_namespace_sha256,
                "HSP custody original route differs"
            );
        }
        for member in &self.members {
            validate_sha256(&member.id)?;
            validate_sha256(&member.record_sha256)?;
            validate_sha256(&member.observation_sha256)?;
            ensure!(
                member.id == member.record_sha256 && member.state != State::Cancelled,
                "invalid custody member identity or state"
            );
            ensure!(
                member.subject.route.as_deref() == Some(&self.route_sha256)
                    && member.subject.namespace.as_deref() == Some(&self.route_namespace_sha256),
                "custody member route differs"
            );
            if let Some(id) = &member.subject.setup {
                ensure!(
                    self.members
                        .iter()
                        .any(|m| m.kind == CustodyKind::Setup && &m.id == id),
                    "custody snapshot lacks setup parent"
                );
            }
            if let Some(id) = &member.subject.play {
                ensure!(
                    self.members
                        .iter()
                        .any(|m| m.kind == CustodyKind::Play && &m.id == id),
                    "custody snapshot lacks play parent"
                );
            }
            if let Some(id) = &member.subject.effect {
                ensure!(
                    self.members
                        .iter()
                        .any(|m| m.kind == CustodyKind::Effect && &m.id == id),
                    "custody snapshot lacks linked effect"
                );
            }
            if member.kind == CustodyKind::Effect {
                ensure!(
                    self.members.iter().any(|m| m.kind == CustodyKind::Setup
                        && member.project == m.project
                        && member.device == m.device
                        && member.device_session == m.device_session
                        && member.subject.boot == m.subject.boot
                        && member.subject.revision == m.subject.revision
                        && member.subject.program == m.subject.program
                        && member.subject.qualification == m.subject.qualification),
                    "custody effect lacks a matching original setup"
                );
            }
        }
        ensure!(
            self.projects().len() <= 128 && self.devices().len() <= 128,
            "custody project/device scope exceeds bound"
        );
        Ok(())
    }
    pub(crate) fn extends_with_stops(&self, after: &Self) -> Result<()> {
        self.validate()?;
        after.validate()?;
        ensure!(
            self.device == after.device
                && self.route_sha256 == after.route_sha256
                && self.route_namespace_sha256 == after.route_namespace_sha256,
            "custody extension changed target route"
        );
        ensure!(
            self.hsp_preparations.len() == after.hsp_preparations.len(),
            "HSP recovery preparation set changed"
        );
        for (before, next) in self.hsp_preparations.iter().zip(&after.hsp_preparations) {
            before.extends(next)?;
        }
        ensure!(
            self.hsp_streams.len() == after.hsp_streams.len(),
            "HSP recovery stream set changed"
        );
        for (before, next) in self.hsp_streams.iter().zip(&after.hsp_streams) {
            before.extends_with_stops(next)?;
        }
        let original = self
            .members
            .iter()
            .map(|m| (m.key(), m))
            .collect::<BTreeMap<_, _>>();
        let current = after
            .members
            .iter()
            .map(|m| (m.key(), m))
            .collect::<BTreeMap<_, _>>();
        for (key, member) in &original {
            ensure!(
                current.get(key).is_some_and(|m| member.advances_to(m)),
                "original custody changed or disappeared"
            );
        }
        for (key, member) in current {
            if !original.contains_key(&key) {
                ensure!(
                    self.members
                        .iter()
                        .any(|source| member.stop_matches(source)),
                    "custody extension contains unrelated work"
                );
            }
        }
        Ok(())
    }
    pub(crate) fn added_stop_records(&self, after: &Self) -> Result<Vec<EffectRecord>> {
        self.extends_with_stops(after)?;
        let original = self
            .members
            .iter()
            .map(CustodyMember::key)
            .collect::<BTreeSet<_>>();
        after
            .members
            .iter()
            .filter(|m| !original.contains(&m.key()))
            .map(|m| {
                let record = m
                    .effect_record
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("added stop lacks original effect metadata"))?;
                ensure!(
                    m.kind == CustodyKind::Effect && record.stop,
                    "extension is not a stop effect"
                );
                Ok(record)
            })
            .collect()
    }
}

pub(super) struct Catalog {
    pub(super) members: BTreeMap<(CustodyKind, String), CustodyMember>,
    inactive: BTreeSet<(CustodyKind, String)>,
}
fn ids(db: &Connection, table: &str) -> Result<Vec<String>> {
    let mut q = db.prepare(&format!(
        "SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM {table} LIMIT 1025"
    ))?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        rows.len() <= CAPACITY as usize,
        "custody source exceeds capacity"
    );
    Ok(rows)
}
fn raw_state(db: &Connection, table: &str, id: &str) -> Result<State> {
    let name: String = db.query_row(
        &format!(
            "SELECT CASE WHEN length(state)<=16 THEN state ELSE NULL END FROM {table} WHERE id=?1"
        ),
        [id],
        |r| r.get(0),
    )?;
    State::parse(&name)
}
fn member(
    kind: CustodyKind,
    id: String,
    scope: (ProjectId, DeviceId, DeviceSessionId),
    subject: Subject,
    state: State,
    observation_sha256: String,
    emergency: bool,
) -> CustodyMember {
    let (project, device, device_session) = scope;
    CustodyMember {
        kind,
        record_sha256: id.clone(),
        id,
        project,
        device,
        device_session,
        subject,
        state,
        observation_sha256,
        emergency,
        effect_record: None,
    }
}
impl Catalog {
    pub(super) fn load(db: &Connection) -> Result<Self> {
        let mut members = BTreeMap::new();
        let mut setups = BTreeMap::new();
        let mut inactive = BTreeSet::new();
        for id in ids(db, "setup_effects")? {
            let value = super::super::setup::row(db, &id)?;
            let record = &value.record;
            let state = raw_state(db, "setup_effects", &id)?;
            let subject = Subject {
                boot: record.boot.clone(),
                revision: record.revision,
                route: Some(record.route_sha256.clone()),
                namespace: record.route_namespace_sha256.clone(),
                program: record.hosted.artifact_id.clone(),
                qualification: record.qualification.dossier_id.clone(),
                qualification_revision: Some(record.qualification.revision),
                admission_sha256: None,
                stop: false,
                setup: None,
                play: None,
                effect: None,
            };
            let m = member(
                CustodyKind::Setup,
                id.clone(),
                (
                    record.project_id.clone(),
                    record.registration.device_id.clone(),
                    record.device_session.clone(),
                ),
                subject,
                state,
                digest(&serde_json::to_vec(&(state.name(), &value.acknowledgment))?),
                false,
            );
            if state == State::Cancelled {
                inactive.insert(m.key());
            }
            members.insert(m.key(), m);
            setups.insert(id, value);
        }
        for id in ids(db, "clock_sync_effects")? {
            let value = super::super::clock_sync::row(db, &id)?;
            let record = &value.record;
            super::super::clock_sync::parent(db, record)?;
            let setup = setups
                .get(record.setup_record_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("synchronization setup absent"))?;
            let c = &record.original_controller;
            let state = raw_state(db, "clock_sync_effects", &id)?;
            let subject = Subject {
                boot: c.engine_epoch.clone(),
                revision: c.revision,
                route: Some(record.route_sha256.clone()),
                namespace: Some(record.route_namespace_sha256.clone()),
                program: setup.record.hosted.artifact_id.clone(),
                qualification: record.preparation_qualification.dossier_id.clone(),
                qualification_revision: Some(record.preparation_qualification.revision),
                admission_sha256: None,
                stop: false,
                setup: Some(record.setup_record_id.to_string()),
                play: None,
                effect: None,
            };
            let m = member(
                CustodyKind::ClockSync,
                id,
                (
                    c.project_id.clone(),
                    c.device_id.clone(),
                    c.device_session.clone(),
                ),
                subject,
                state,
                digest(&serde_json::to_vec(&(state.name(), &value.acknowledgment))?),
                false,
            );
            if state == State::Cancelled {
                inactive.insert(m.key());
            }
            members.insert(m.key(), m);
        }
        let mut linked = BTreeMap::new();
        for id in ids(db, "play_operations")? {
            let value = super::super::play::row(db, &id)?;
            let record = &value.record;
            let setup = setups
                .get(record.setup_record_id.as_str())
                .ok_or_else(|| anyhow::anyhow!("play setup absent"))?;
            let effect = &record.effect;
            let state = raw_state(db, "play_operations", &id)?;
            let effect_id = digest(&serde_json::to_vec(effect)?);
            ensure!(
                linked
                    .insert(effect_id.clone(), (id.clone(), state))
                    .is_none(),
                "effect linked to multiple play operations"
            );
            let subject = Subject {
                boot: effect.boot.clone(),
                revision: effect.admission.revision,
                route: Some(setup.record.route_sha256.clone()),
                namespace: setup.record.route_namespace_sha256.clone(),
                program: effect.admission.program_artifact.clone(),
                qualification: effect.qualification_record.clone(),
                qualification_revision: Some(record.qualification.revision),
                admission_sha256: Some(digest(&serde_json::to_vec(&effect.admission)?)),
                stop: false,
                setup: Some(record.setup_record_id.to_string()),
                play: None,
                effect: Some(effect_id),
            };
            let m = member(
                CustodyKind::Play,
                id,
                (
                    effect.admission.project.clone(),
                    effect.admission.configuration.device.clone(),
                    effect.device_session.clone(),
                ),
                subject,
                state,
                digest(&serde_json::to_vec(state.name())?),
                false,
            );
            if state == State::Cancelled {
                inactive.insert(m.key());
            }
            members.insert(m.key(), m);
        }
        for id in ids(db, "effects")? {
            let (device,text,outcome):(String,String,String)=db.query_row("SELECT CASE WHEN length(CAST(device AS BLOB))<=128 THEN device ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(outcome)<=16 THEN outcome ELSE NULL END FROM effects WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            ensure!(
                digest(text.as_bytes()) == id,
                "custody effect integrity differs"
            );
            let record: EffectRecord = serde_json::from_str(&text)?;
            ensure!(
                record.sequence > 0 && record.admission.configuration.device.as_str() == device,
                "custody effect subject differs"
            );
            validate_sha256(&record.request_sha256)?;
            let state = State::parse(&outcome)?;
            ensure!(
                matches!(
                    state,
                    State::Prepared | State::Acknowledged | State::Unknown
                ),
                "invalid generic effect state"
            );
            let matching = setups
                .values()
                .filter(|s| {
                    s.record.boot == record.boot
                        && s.record.device_session == record.device_session
                        && s.record.project_id == record.admission.project
                        && s.record.revision == record.admission.revision
                        && s.record.registration.device_id == record.admission.configuration.device
                        && s.record.hosted.artifact_id == record.admission.program_artifact
                        && s.record.qualification.dossier_id == record.qualification_record
                })
                .collect::<Vec<_>>();
            let route = matching
                .first()
                .map(|s| s.record.route_sha256.clone())
                .filter(|route| matching.iter().all(|s| s.record.route_sha256 == *route));
            let namespace = matching
                .first()
                .and_then(|s| s.record.route_namespace_sha256.clone())
                .filter(|namespace| {
                    matching
                        .iter()
                        .all(|s| s.record.route_namespace_sha256.as_ref() == Some(namespace))
                });
            let marker:Option<(String,String)>=db.query_row("SELECT CASE WHEN length(outcome)<=16 THEN outcome ELSE NULL END,CASE WHEN length(provenance_sha256)=64 THEN provenance_sha256 ELSE NULL END FROM emergency_stop_observations WHERE effect_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let parent_play = linked.get(&id);
            if parent_play.is_some_and(|(_, state)| *state == State::Cancelled) {
                ensure!(
                    state == State::Prepared && !record.stop && marker.is_none(),
                    "cancelled play has attempted generic effect history"
                );
                inactive.insert((CustodyKind::Effect, id.clone()));
            }
            let subject = Subject {
                boot: record.boot.clone(),
                revision: record.admission.revision,
                route,
                namespace,
                program: record.admission.program_artifact.clone(),
                qualification: record.qualification_record.clone(),
                qualification_revision: None,
                admission_sha256: Some(digest(&serde_json::to_vec(&record.admission)?)),
                stop: record.stop,
                setup: None,
                play: parent_play.map(|(id, _)| id.clone()),
                effect: None,
            };
            let mut m = member(
                CustodyKind::Effect,
                id,
                (
                    record.admission.project.clone(),
                    record.admission.configuration.device.clone(),
                    record.device_session.clone(),
                ),
                subject,
                state,
                digest(&serde_json::to_vec(&(state.name(), &marker))?),
                marker.is_some(),
            );
            m.effect_record = Some(record);
            members.insert(m.key(), m);
        }
        ensure!(
            members.len() <= MEMBER_LIMIT,
            "custody catalog exceeds bound"
        );
        Ok(Self { members, inactive })
    }
    /// A new driver cannot treat unresolved custody in an older credential-key
    /// namespace as unrelated merely because its current route digest differs.
    pub(super) fn require_unresolved_namespace(
        &self,
        namespace: &str,
        resolved: &BTreeSet<(CustodyKind, String)>,
    ) -> Result<()> {
        validate_sha256(namespace)?;
        for (key, member) in &self.members {
            if !self.inactive.contains(key) && !resolved.contains(key) {
                ensure!(
                    member.subject.namespace.as_deref() == Some(namespace),
                    "unresolved legacy custody uses an unknown or different route namespace"
                );
            }
        }
        Ok(())
    }

    pub(super) fn snapshot(
        &self,
        device: &DeviceId,
        route: &str,
        namespace: &str,
        resolved: &BTreeSet<(CustodyKind, String)>,
    ) -> Result<CustodySnapshot> {
        validate_sha256(route)?;
        validate_sha256(namespace)?;
        let mut members = Vec::new();
        for (key, m) in &self.members {
            if self.inactive.contains(key) || resolved.contains(key) {
                continue;
            }
            if m.kind == CustodyKind::Effect {
                ensure!(
                    m.subject.route.is_some() && m.subject.namespace.is_some(),
                    "legacy effect routing is unknown or ambiguous"
                );
            }
            if &m.device == device || m.subject.route.as_deref() == Some(route) {
                ensure!(
                    m.subject.route.as_deref() == Some(route)
                        && m.subject.namespace.as_deref() == Some(namespace),
                    "selected custody uses a different route or namespace"
                );
                members.push(m.clone());
            }
        }
        let snapshot = CustodySnapshot {
            device: device.clone(),
            route_sha256: route.into(),
            route_namespace_sha256: namespace.into(),
            members,
            hsp_streams: Vec::new(),
            hsp_preparations: Vec::new(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub(super) fn current(&self, key: &(CustodyKind, String)) -> Result<&CustodyMember> {
        self.members
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("resolved custody member is missing"))
    }
    pub(super) fn active_effect_ids(
        &self,
        device: &DeviceId,
        resolved: &BTreeSet<(CustodyKind, String)>,
    ) -> Vec<String> {
        self.members
            .iter()
            .filter(|(key, m)| {
                m.kind == CustodyKind::Effect
                    && &m.device == device
                    && !self.inactive.contains(*key)
                    && !resolved.contains(*key)
            })
            .map(|(_, m)| m.id.clone())
            .collect()
    }
    pub(super) fn effect_uses_resolved_setup(
        &self,
        record: &EffectRecord,
        resolved: &BTreeSet<(CustodyKind, String)>,
    ) -> bool {
        let matches = self
            .members
            .values()
            .filter(|m| {
                m.kind == CustodyKind::Setup
                    && m.project == record.admission.project
                    && m.device == record.admission.configuration.device
                    && m.device_session == record.device_session
                    && m.subject.boot == record.boot
                    && m.subject.revision == record.admission.revision
                    && m.subject.program == record.admission.program_artifact
                    && m.subject.qualification == record.qualification_record
            })
            .collect::<Vec<_>>();
        !matches.is_empty()
            && matches
                .iter()
                .all(|m| resolved.contains(&m.key()) || self.inactive.contains(&m.key()))
    }
}
