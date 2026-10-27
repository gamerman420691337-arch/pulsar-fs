//! Original play-operation custody. History cannot recreate native authority.
use super::*;

const PLAY_RECORD_LIMIT: usize = 32768;
const TABLE: &str = "CREATE TABLE play_operations(id TEXT PRIMARY KEY NOT NULL,actor TEXT NOT NULL,request TEXT NOT NULL,effect_id TEXT NOT NULL,record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','claimed','acknowledged','unknown','cancelled')))";
const REQUEST: &str =
    "CREATE UNIQUE INDEX play_operation_request ON play_operations(actor,request)";
const EFFECT: &str = "CREATE UNIQUE INDEX play_operation_effect ON play_operations(effect_id)";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayClockRecord {
    pub domain: ArtifactId,
    pub monotonic_ns: u64,
    pub valid_until_ns: u64,
    pub estimated_server_ms: u64,
    pub uncertainty_ns: u64,
}
impl From<&crate::playback_runtime::PlaybackClock> for PlayClockRecord {
    fn from(clock: &crate::playback_runtime::PlaybackClock) -> Self {
        Self {
            domain: clock.domain.clone(),
            monotonic_ns: clock.monotonic_ns,
            valid_until_ns: clock.valid_until_ns,
            estimated_server_ms: clock.estimated_server_ms,
            uncertainty_ns: clock.uncertainty_ns,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayOperationRecord {
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub request_fingerprint: String,
    pub original_controller: HandyControllerLease,
    pub qualification: HandyQualificationBinding,
    pub setup_record_id: ArtifactId,
    pub clock_request_id: RequestId,
    pub clock: PlayClockRecord,
    pub start_time_ms: u64,
    pub effect: EffectRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_start: Option<HandyBrowserStartDecision>,
}
fn digest(text: &[u8]) -> String {
    format!("{:x}", Sha256::digest(text))
}
fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl PlayOperationRecord {
    pub fn validate(&self) -> Result<()> {
        self.original_controller.validate()?;
        self.qualification.validate()?;
        let l = &self.original_controller;
        ensure!(
            l.state == HandyControllerState::Active && l.armed && !l.physical_stop_pending,
            "play intent requires an originally armed controller"
        );
        ensure!(
            is_digest(&self.request_fingerprint)
                && is_digest(self.setup_record_id.as_str())
                && is_digest(&self.effect.request_sha256),
            "invalid play identity digest"
        );
        ensure!(
            !self.effect.stop
                && self.effect.sequence > 0
                && self.effect.boot == l.engine_epoch
                && self.effect.device_session == l.device_session
                && self.effect.controller.as_ref() == Some(&l.controller_id)
                && self.effect.admission.configuration.device == l.device_id
                && self.effect.admission.project == l.project_id
                && self.effect.admission.revision == l.revision
                && self.effect.qualification_record == self.qualification.dossier_id,
            "play effect and original controller differ"
        );
        ensure!(
            self.clock.monotonic_ns <= self.clock.valid_until_ns
                && self.clock.monotonic_ns < l.deadline_boottime_ns
                && self.clock.estimated_server_ms < (1u64 << 53)
                && self.start_time_ms < (1u64 << 53)
                && self.clock_request_id != self.request_id,
            "invalid original play timing metadata"
        );
        ensure!(
            self.original_controller.preparation_id.as_str()
                == digest(&serde_json::to_vec(&(
                    &self.actor_session,
                    self.original_controller.engine_epoch.as_str(),
                    &self.original_controller.preparation_request_id,
                    &self.setup_record_id,
                ))?),
            "play preparation does not name its original setup"
        );
        if let Some(decision) = &self.browser_start {
            decision.validate()?;
            ensure!(
                decision.subject.origin == HandyEvidenceOrigin::Physical
                    && decision.subject.device_qualification == self.qualification
                    && decision.subject.project_id == l.project_id
                    && decision.subject.revision == l.revision
                    && decision.subject.script_actor_session == self.actor_session
                    && decision.timing_binding.engine_epoch.as_str() == l.engine_epoch.as_str()
                    && decision.reference_boottime_ns == self.clock.monotonic_ns
                    && decision.alignment.reference_boottime_ns == self.clock.monotonic_ns
                    && decision.alignment.start_time_ms == self.start_time_ms
                    && decision.alignment.total_error_ns == self.clock.uncertainty_ns
                    && self.clock.valid_until_ns <= decision.alignment.valid_until_boottime_ns
                    && self.clock.valid_until_ns <= l.deadline_boottime_ns,
                "browser start decision and original play timing differ"
            );
            let request = crate::handy_wire::EncodedRequest::hssp_play(
                self.start_time_ms,
                self.clock.estimated_server_ms,
            )
            .map_err(|_| anyhow::anyhow!("invalid browser play wire reference"))?;
            ensure!(
                self.effect.request_sha256
                    == digest(&serde_json::to_vec(&(
                        request.method(),
                        request.path(),
                        request.body(),
                    ))?),
                "browser play does not preserve its original phase/server pair"
            );
        }
        Ok(())
    }
    pub fn id(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() <= PLAY_RECORD_LIMIT,
            "play record exceeds limit"
        );
        Ok(digest(&bytes))
    }
    fn effect_id(&self) -> Result<String> {
        Ok(digest(&serde_json::to_vec(&self.effect)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayOperationState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
impl PlayOperationState {
    fn parse(state: &str) -> Result<Self> {
        Ok(match state {
            "prepared" => Self::Prepared,
            "claimed" => Self::Claimed,
            "acknowledged" => Self::Acknowledged,
            "unknown" => Self::Unknown,
            "cancelled" => Self::Cancelled,
            _ => anyhow::bail!("invalid play operation state"),
        })
    }
    fn effect_outcome(self) -> &'static str {
        match self {
            Self::Acknowledged => "acknowledged",
            Self::Unknown => "unknown",
            _ => "prepared",
        }
    }
}
#[derive(Clone, Debug)]
pub struct PlayOperationObservation {
    pub id: String,
    pub record: PlayOperationRecord,
    pub state: PlayOperationState,
}
impl PlayOperationObservation {
    pub fn status(&self) -> Result<HandyPlaybackStatus> {
        let state = match self.state {
            PlayOperationState::Prepared => HandyPlaybackOperationState::Prepared,
            PlayOperationState::Claimed => HandyPlaybackOperationState::Claimed,
            PlayOperationState::Acknowledged => HandyPlaybackOperationState::Acknowledged,
            PlayOperationState::Unknown => HandyPlaybackOperationState::Unknown,
            PlayOperationState::Cancelled => HandyPlaybackOperationState::Cancelled,
        };
        let value = HandyPlaybackStatus {
            project_id: self.record.original_controller.project_id.clone(),
            play_request_id: self.record.request_id.clone(),
            history: Some(HandyPlaybackHistory {
                record_id: ArtifactId::new(self.id.clone())?,
                original_controller: self.record.original_controller.clone(),
                qualification: self.record.qualification.clone(),
                setup_record_id: self.record.setup_record_id.clone(),
                clock_request_id: self.record.clock_request_id.clone(),
                start_time_ms: self.record.start_time_ms,
                browser_start: self.record.browser_start.clone(),
                state,
            }),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn check_witness(&self, expected: &HandyPlaybackStatus) -> Result<()> {
        expected.validate()?;
        let h = expected
            .history
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("play witness has no history"))?;
        let mut actual = self.status()?;
        let current = actual.history.as_mut().expect("journal history");
        ensure!(
            h.state == current.state
                || h.state == HandyPlaybackOperationState::Prepared
                || (h.state == HandyPlaybackOperationState::Claimed
                    && matches!(
                        current.state,
                        HandyPlaybackOperationState::Acknowledged
                            | HandyPlaybackOperationState::Unknown
                    )),
            "play witness claims a future or conflicting disposition"
        );
        current.state = h.state;
        ensure!(
            serde_json::to_vec(&actual)? == serde_json::to_vec(expected)?,
            "play witness subject differs"
        );
        Ok(())
    }
}
/// Captured under current runtime ownership, with no database I/O.
pub struct PlayIntent {
    record: PlayOperationRecord,
    effect: EffectIntent,
}
/// No effect receipt is exposed until its durable claim succeeds.
pub struct PreparedPlayReceipt {
    id: String,
    boot: DeviceSessionId,
}
pub struct ClaimedPlayReceipt {
    id: String,
    effect: PreparedEffectReceipt,
}
impl ClaimedPlayReceipt {
    pub fn effect_receipt(&self) -> &PreparedEffectReceipt {
        &self.effect
    }
    pub fn operation_id(&self) -> &str {
        &self.id
    }
}
impl EffectReceiptBinding {
    pub fn capture_play_intent(
        &self,
        runtime: &HandyPlaybackRuntime,
        effect: &PlaybackEffect,
        record: PlayOperationRecord,
    ) -> Result<PlayIntent> {
        record.validate()?;
        ensure!(
            effect.is_referenced_start() == record.browser_start.is_some(),
            "play reference mode and browser decision differ"
        );
        ensure!(
            effect.referenced_media_rate_error_ppm()
                == record
                    .browser_start
                    .as_ref()
                    .map(|decision| decision.subject.timing.media_rate_error_ppm),
            "play reference media progression envelope differs"
        );
        let intent = self.capture_intent(runtime, effect)?;
        ensure!(
            serde_json::to_vec(&intent.record)? == serde_json::to_vec(&record.effect)?,
            "play record describes another effect"
        );
        let clock = effect
            .retained_clock()
            .ok_or_else(|| anyhow::anyhow!("play clock missing"))?;
        ensure!(
            record.clock == PlayClockRecord::from(clock),
            "play clock metadata differs"
        );
        let body: serde_json::Value = serde_json::from_slice(
            effect
                .request()
                .body()
                .ok_or_else(|| anyhow::anyhow!("play body missing"))?,
        )?;
        ensure!(
            effect.request().path() == "/hssp/play"
                && effect.request().method() == "PUT"
                && body.get("start_time").and_then(|v| v.as_u64()) == Some(record.start_time_ms),
            "play start offset differs"
        );
        record.id()?;
        Ok(PlayIntent {
            record,
            effect: intent,
        })
    }
}

pub(super) fn linked(db: &Connection, effect: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM play_operations WHERE effect_id=?1)",
        [effect],
        |r| r.get(0),
    )?)
}
/// Acknowledged play still owns stopping uncertainty. Only Cancelled is known unsent.
pub(super) fn available(
    db: &Connection,
    route: &str,
    namespace: &str,
    device: &DeviceId,
    allowed: Option<&str>,
) -> Result<()> {
    ensure!(
        is_digest(route) && is_digest(namespace),
        "invalid play route identity"
    );
    super::hsp::ensure_legacy_route_available(db, route, namespace, device)?;
    let resolved = super::reconciliation::resolved_keys(db)?;
    if let Some(id) = allowed {
        ensure!(
            !resolved.contains(&(CustodyKind::Play, id.into())),
            "resolved play cannot regain authority"
        );
    }
    let mut q = db.prepare("SELECT id FROM play_operations WHERE state!='cancelled' LIMIT 1025")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        ids.len() <= CAPACITY as usize,
        "play route capacity exceeded"
    );
    for id in ids {
        if resolved.contains(&(CustodyKind::Play, id.clone())) {
            continue;
        }
        let value = row(db, &id)?;
        let setup = super::setup::row(db, value.record.setup_record_id.as_str())?;
        ensure!(
            setup.record.route_namespace_sha256.as_deref() == Some(namespace),
            "unresolved play belongs to another route namespace"
        );
        ensure!(
            allowed == Some(id.as_str())
                || (setup.record.route_sha256 != route
                    && value.record.original_controller.device_id != *device),
            "device or route has unresolved play custody"
        );
    }
    Ok(())
}
fn owner(db: &Connection, boot: &DeviceSessionId) -> Result<()> {
    let current:String=db.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB))<=128 THEN boot ELSE NULL END FROM owner WHERE singleton=1",[],|r|r.get(0))?;
    ensure!(current == boot.as_str(), "retired play journal owner");
    Ok(())
}
fn parent(db: &Connection, record: &PlayOperationRecord) -> Result<()> {
    let setup = super::setup::row(db, record.setup_record_id.as_str())?;
    let l = &record.original_controller;
    ensure!(
        setup.state == SetupJournalState::Acknowledged
            && setup.acknowledgment.is_some()
            && setup.record.preflight_mode.is_some()
            && setup.record.route_namespace_sha256.is_some()
            && setup.record.actor_session == record.actor_session
            && setup.record.boot == l.engine_epoch
            && setup.record.device_session == l.device_session
            && setup.record.project_id == l.project_id
            && setup.record.revision == l.revision
            && setup.record.registration.device_id == l.device_id
            && setup.record.qualification == record.qualification
            && setup.record.hosted.artifact_id == record.effect.admission.program_artifact,
        "play setup parent differs"
    );
    if let Some(decision) = &record.browser_start {
        ensure!(
            setup.record.master_sha256 == decision.subject.master_sha256
                && setup.record.script == decision.subject.script,
            "browser start source/script correspondence differs from setup"
        );
    }
    Ok(())
}
pub(super) fn row(db: &Connection, id: &str) -> Result<PlayOperationObservation> {
    ensure!(is_digest(id), "invalid play operation ID");
    let (actor,request,effect_id,raw,state): (String,String,String,String,String) = db.query_row(
        "SELECT CASE WHEN length(CAST(actor AS BLOB))<=128 THEN actor ELSE NULL END,CASE WHEN length(CAST(request AS BLOB))<=128 THEN request ELSE NULL END,CASE WHEN length(effect_id)=64 THEN effect_id ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=32768 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END FROM play_operations WHERE id=?1", [id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    ensure!(
        digest(raw.as_bytes()) == id,
        "play record integrity mismatch"
    );
    let record: PlayOperationRecord = serde_json::from_str(&raw)?;
    record.validate()?;
    super::browser_play::check_row(db, id, &record)?;
    parent(db, &record)?;
    ensure!(
        record.actor_session.as_str() == actor
            && record.request_id.as_str() == request
            && record.effect_id()? == effect_id,
        "play index correspondence differs"
    );
    let state = PlayOperationState::parse(&state)?;
    let (device,effect,outcome):(String,String,String)=db.query_row(
        "SELECT CASE WHEN length(CAST(device AS BLOB))<=128 THEN device ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(outcome)<=16 THEN outcome ELSE NULL END FROM effects WHERE id=?1",[&effect_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(
        device == record.effect.admission.configuration.device.as_str()
            && digest(effect.as_bytes()) == effect_id
            && effect == serde_json::to_string(&record.effect)?
            && outcome == state.effect_outcome(),
        "play effect history correspondence differs"
    );
    Ok(PlayOperationObservation {
        id: id.into(),
        record,
        state,
    })
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, sql) in [
        ("play_operations", TABLE),
        ("play_operation_request", REQUEST),
        ("play_operation_effect", EFFECT),
    ] {
        let actual: Option<String> = db
            .query_row("SELECT sql FROM sqlite_master WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()?;
        if version < 5 {
            ensure!(actual.is_none(), "unexpected play journal schema");
        } else {
            ensure!(
                actual.as_deref() == Some(sql),
                "play journal schema differs"
            );
        }
    }
    if version < 5 {
        return Ok(());
    }
    let count: u64 = db.query_row("SELECT count(*) FROM play_operations", [], |r| r.get(0))?;
    ensure!(count <= CAPACITY, "play operation capacity exceeded");
    let triggers: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name='play_operations'",
        [],
        |r| r.get(0),
    )?;
    ensure!(triggers == 0, "unexpected play operation trigger");
    let mut q =
        db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM play_operations")?;
    for id in q.query_map([], |r| r.get::<_, String>(0))? {
        row(db, &id?)?;
    }
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 6 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if version < 5 {
        for sql in [TABLE, REQUEST, EFFECT] {
            tx.execute_batch(sql)?;
        }
    }
    // v6 readers enforce play custody before setup/synchronization mutation.
    tx.execute_batch("PRAGMA user_version=6")?;
    tx.commit()?;
    Ok(())
}

impl DeviceEffectJournal {
    pub fn verify_play_statuses(
        state_dir: &Path,
        witnesses: &[(SessionId, HandyPlaybackStatus)],
    ) -> Result<()> {
        if witnesses.is_empty() {
            return Ok(());
        }
        ensure!(witnesses.len() <= 4096, "play witness capacity exceeded");
        let path = state_dir.join("device-effects/journal.sqlite3");
        let _held = crate::artifacts::open_regular(&path)?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        ensure!(
            Self::validate_existing(&db)? >= 5,
            "play journal unavailable"
        );
        for (actor, witness) in witnesses {
            witness.validate()?;
            let h = witness
                .history
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing play witness"))?;
            let observed = row(&db, h.record_id.as_str())?;
            ensure!(
                &observed.record.actor_session == actor,
                "play witness actor differs"
            );
            observed.check_witness(witness)?;
        }
        Ok(())
    }
    pub fn play_route_available(
        &self,
        route: &str,
        namespace: &str,
        device: &DeviceId,
        allowed: Option<&str>,
    ) -> Result<()> {
        self.check_owner()?;
        available(&self.db, route, namespace, device, allowed)?;
        let resolved = super::reconciliation::resolved_keys(&self.db)?;
        let mut q=self.db.prepare("SELECT id FROM effects e WHERE NOT EXISTS(SELECT 1 FROM play_operations p WHERE p.effect_id=e.id) LIMIT 1025")?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(
            ids.len() <= CAPACITY as usize,
            "legacy effect capacity exceeded"
        );
        let legacy = ids
            .iter()
            .any(|id| !resolved.contains(&(CustodyKind::Effect, id.clone())));
        ensure!(
            !legacy,
            "legacy physical-effect uncertainty requires reconciliation"
        );
        Ok(())
    }
    /// Writes the exact effect and original operation together. A legacy effect
    /// cannot be adopted: its prepared row may already have been dispatched.
    pub fn prepare_play(&mut self, intent: &PlayIntent) -> Result<PreparedPlayReceipt> {
        self.check_owner()?;
        let record = &intent.record;
        ensure!(record.effect.boot == self.boot, "retired play intent");
        let id = record.id()?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Play, &id)?;
        super::reconciliation::require_unresolved(
            &tx,
            CustodyKind::Setup,
            record.setup_record_id.as_str(),
        )?;
        parent(&tx, record)?;
        let setup = super::setup::row(&tx, record.setup_record_id.as_str())?;
        available(
            &tx,
            &setup.record.route_sha256,
            setup
                .record
                .route_namespace_sha256
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("play namespace missing"))?,
            &record.original_controller.device_id,
            Some(&id),
        )?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT id FROM play_operations WHERE actor=?1 AND request=?2",
                params![record.actor_session.as_str(), record.request_id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            ensure!(prior == id, "play request already names another intent");
            ensure!(
                row(&tx, &id)?.state == PlayOperationState::Prepared,
                "play request already claimed or terminal"
            );
        } else {
            let effect_id = record.effect_id()?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM effects WHERE id=?1)",
                [&effect_id],
                |r| r.get(0),
            )?;
            ensure!(!exists, "existing effect cannot become an unclaimed play");
            let count: u64 =
                tx.query_row("SELECT count(*) FROM play_operations", [], |r| r.get(0))?;
            ensure!(count < CAPACITY, "play operation capacity reached");
            let effect = Self::prepare_in_transaction(&tx, &self.boot, &intent.effect)?;
            ensure!(effect.id == effect_id, "play receipt differs");
            tx.execute("INSERT INTO play_operations(id,actor,request,effect_id,record,state) VALUES(?1,?2,?3,?4,?5,'prepared')",
                params![id,record.actor_session.as_str(),record.request_id.as_str(),effect_id,serde_json::to_string(record)?])?;
            super::browser_play::insert(&tx, &id, record)?;
        }
        tx.commit()?;
        Ok(PreparedPlayReceipt {
            id,
            boot: self.boot.clone(),
        })
    }
    pub fn try_prepare_play(&mut self, intent: &PlayIntent) -> Result<PreparedPlayReceipt> {
        self.without_writer_wait(|journal| journal.prepare_play(intent))
    }
    pub fn claim_play(&mut self, prepared: PreparedPlayReceipt) -> Result<ClaimedPlayReceipt> {
        self.check_owner()?;
        ensure!(prepared.boot == self.boot, "retired play receipt");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Play, &prepared.id)?;
        let observed = row(&tx, &prepared.id)?;
        let original_setup = super::setup::row(&tx, observed.record.setup_record_id.as_str())?;
        available(
            &tx,
            &original_setup.record.route_sha256,
            original_setup
                .record
                .route_namespace_sha256
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("play route namespace is missing"))?,
            &observed.record.original_controller.device_id,
            Some(&prepared.id),
        )?;
        super::reconciliation::require_unresolved(
            &tx,
            CustodyKind::Setup,
            observed.record.setup_record_id.as_str(),
        )?;
        super::reconciliation::require_unresolved(
            &tx,
            CustodyKind::Effect,
            &observed.record.effect_id()?,
        )?;
        ensure!(
            observed.record.effect.boot == self.boot
                && observed.state == PlayOperationState::Prepared,
            "play operation cannot be claimed"
        );
        ensure!(
            tx.execute(
                "UPDATE play_operations SET state='claimed' WHERE id=?1 AND state='prepared'",
                [&prepared.id]
            )? == 1,
            "play claim lost"
        );
        tx.commit()?;
        Ok(ClaimedPlayReceipt {
            id: prepared.id,
            effect: PreparedEffectReceipt {
                id: observed.record.effect_id()?,
                boot: self.boot.clone(),
            },
        })
    }
    pub fn try_claim_play(&mut self, prepared: PreparedPlayReceipt) -> Result<ClaimedPlayReceipt> {
        self.without_writer_wait(|journal| journal.claim_play(prepared))
    }
    pub fn complete_play_operation(
        &mut self,
        claimed: &ClaimedPlayReceipt,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        self.check_owner()?;
        ensure!(claimed.effect.boot == self.boot, "retired play completion");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        let observed = row(&tx, &claimed.id)?;
        let (name, state) = match outcome {
            DispatchOutcome::Acknowledged => ("acknowledged", PlayOperationState::Acknowledged),
            DispatchOutcome::Unknown => ("unknown", PlayOperationState::Unknown),
        };
        if super::reconciliation::resolved_keys(&tx)?
            .contains(&(CustodyKind::Play, claimed.id.clone()))
        {
            ensure!(
                observed.state == state && observed.record.effect_id()? == claimed.effect.id,
                "resolved play outcome cannot advance"
            );
            return Ok(());
        }
        ensure!(
            observed.record.effect_id()? == claimed.effect.id
                && (observed.state == PlayOperationState::Claimed || observed.state == state),
            "play outcome changed or claim missing"
        );
        Self::complete_in_transaction(&tx, &claimed.effect, outcome)?;
        tx.execute(
            "UPDATE play_operations SET state=?1 WHERE id=?2",
            params![name, claimed.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn try_complete_play_operation(
        &mut self,
        claimed: &ClaimedPlayReceipt,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        self.without_writer_wait(|journal| journal.complete_play_operation(claimed, outcome))
    }
    pub fn cancel_unclaimed_play(
        &mut self,
        actor: &SessionId,
        request: &RequestId,
        id: &str,
    ) -> Result<()> {
        self.check_owner()?;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Play, id)?;
        let observed = row(&tx, id)?;
        ensure!(
            &observed.record.actor_session == actor && &observed.record.request_id == request,
            "play cancellation selection differs"
        );
        ensure!(
            matches!(
                observed.state,
                PlayOperationState::Prepared | PlayOperationState::Cancelled
            ),
            "attempted play cannot be cancelled as unsent"
        );
        tx.execute(
            "UPDATE play_operations SET state='cancelled' WHERE id=?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn play_request_history(
        &self,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<Option<PlayOperationObservation>> {
        self.check_owner()?;
        let id:Option<String>=self.db.query_row("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM play_operations WHERE actor=?1 AND request=?2",params![actor.as_str(),request.as_str()],|r|r.get(0)).optional()?;
        id.map(|id| row(&self.db, &id)).transpose()
    }
    /// Read-only startup check for the future main-request witness. Absence
    /// refuses; this method never creates a replacement journal or native queue.
    pub fn verify_play_records(
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
            Self::validate_existing(&db)? >= 5,
            "play journal history unavailable"
        );
        ensure!(
            witnesses.len() <= CAPACITY as usize,
            "play witness capacity exceeded"
        );
        for (actor, request, id) in witnesses {
            let value = row(&db, id)?;
            ensure!(
                &value.record.actor_session == actor && &value.record.request_id == request,
                "play witness differs"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "device_play_journal_tests.rs"]
mod tests;
