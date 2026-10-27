//! Setup journal metadata and single-use claims. This module performs no I/O to a device.
use super::*;
const TABLE: &str = "CREATE TABLE setup_effects(id TEXT PRIMARY KEY NOT NULL,route TEXT NOT NULL,intent TEXT NOT NULL,record TEXT NOT NULL,state TEXT NOT NULL CHECK(state IN ('prepared','claimed','acknowledged','unknown','cancelled')))";
const INDEX: &str =
    "CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'";

#[path = "device_setup_observations.rs"]
mod observations;

#[derive(Clone, Debug, PartialEq)]
pub enum SetupCompletion {
    Acknowledged(HandySetupAcknowledgment),
    Unknown,
}
fn is_false(value: &bool) -> bool {
    !*value
}

const INTENT_INDEX: &str = "CREATE UNIQUE INDEX setup_intent_identity ON setup_effects(intent)";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupEffectRecord {
    pub boot: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub hosting_request_id: RequestId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub registration: HandyRegistration,
    pub qualification: HandyQualificationBinding,
    pub master_sha256: String,
    pub script: ArtifactIdentity,
    pub hosted: ArtifactIdentity,
    /// Engine-derived opaque credential-route identity, never a credential or URL.
    pub route_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_namespace_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub acknowledgment_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_mode: Option<HandyDeviceMode>,
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl SetupEffectRecord {
    fn validate(&self) -> Result<()> {
        if let Some(mode) = &self.preflight_mode {
            mode.validate()?;
            ensure!(mode.mode == 1, "setup requires HSSP mode");
        }
        if let Some(namespace) = &self.route_namespace_sha256 {
            ensure!(digest_valid(namespace), "invalid route namespace");
        }
        DeviceSessionId::new(self.boot.as_str())?;
        DeviceSessionId::new(self.device_session.as_str())?;
        SessionId::new(self.actor_session.as_str())?;
        RequestId::new(self.request_id.as_str())?;
        RequestId::new(self.hosting_request_id.as_str())?;
        ensure!(
            self.request_id != self.hosting_request_id,
            "setup and hosting requests must differ"
        );
        ProjectId::new(self.project_id.as_str())?;
        self.registration.validate()?;
        self.qualification.validate()?;
        ensure!(
            !self.registration.disabled
                && self.boot != self.device_session
                && digest_valid(&self.master_sha256)
                && digest_valid(&self.route_sha256),
            "invalid setup subject"
        );
        for (value, limit) in [
            (&self.script, HANDY_STROKE_SCRIPT_MAX_BYTES),
            (&self.hosted, HANDY_HOSTED_SCRIPT_MAX_BYTES),
        ] {
            value.validate()?;
            ensure!(
                value.byte_len > 0
                    && value.byte_len <= limit
                    && value.artifact_id.as_str() == value.sha256,
                "invalid setup content identity"
            );
        }
        Ok(())
    }
}
fn intent_identity(record: &SetupEffectRecord) -> Result<String> {
    request_identity(&record.actor_session, &record.request_id)
}
fn request_identity(actor: &SessionId, request: &RequestId) -> Result<String> {
    SessionId::new(actor.as_str())?;
    RequestId::new(request.as_str())?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(actor, request))?)
    ))
}
fn check_namespace(
    db: &Connection,
    namespace: &str,
    device: &DeviceId,
    current_id: &str,
) -> Result<()> {
    ensure!(digest_valid(namespace), "invalid route namespace");
    let resolved = super::reconciliation::resolved_keys(db)?;
    let mut q=db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM setup_effects WHERE state!='cancelled' LIMIT 1025")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        ids.len() <= CAPACITY as usize,
        "setup journal capacity exceeded"
    );
    for id in ids {
        if resolved.contains(&(CustodyKind::Setup, id.clone())) {
            continue;
        }
        let value = row(db, &id)?;
        ensure!(
            value.record.route_namespace_sha256.as_deref() == Some(namespace),
            "legacy or changed route namespace has unresolved setup history"
        );
        ensure!(
            value.id == current_id || &value.record.registration.device_id != device,
            "logical device has unresolved setup history"
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupJournalState {
    Prepared,
    Claimed,
    Acknowledged,
    Unknown,
    Cancelled,
}
impl SetupJournalState {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "prepared" => Self::Prepared,
            "claimed" => Self::Claimed,
            "acknowledged" => Self::Acknowledged,
            "unknown" => Self::Unknown,
            "cancelled" => Self::Cancelled,
            _ => anyhow::bail!("invalid setup state"),
        })
    }
}
#[derive(Clone, Debug)]
pub struct SetupEffectObservation {
    pub id: String,
    pub record: SetupEffectRecord,
    pub state: SetupJournalState,
    pub acknowledgment: Option<HandySetupAcknowledgment>,
}
/// A metadata receipt, not permission to contact hardware. Clones still share one durable claim.
#[derive(Clone, Debug)]
pub struct PreparedSetupReceipt {
    id: String,
    boot: DeviceSessionId,
}
impl PreparedSetupReceipt {
    pub fn id(&self) -> &str {
        &self.id
    }
}
/// Returned only after the claim commits. Not cloneable or deserializable.
pub struct ClaimedSetupReceipt {
    id: String,
    boot: DeviceSessionId,
}
fn decode(
    id: String,
    route: String,
    intent: String,
    text: String,
    state: String,
) -> Result<SetupEffectObservation> {
    ensure!(
        digest_valid(&id)
            && digest_valid(&route)
            && text.len() <= RECORD_LIMIT
            && format!("{:x}", Sha256::digest(text.as_bytes())) == id,
        "setup journal integrity mismatch"
    );
    let record: SetupEffectRecord = serde_json::from_str(&text)?;
    record.validate()?;
    ensure!(
        record.route_sha256 == route && intent_identity(&record)? == intent,
        "setup route or request binding differs"
    );
    Ok(SetupEffectObservation {
        id,
        record,
        state: SetupJournalState::parse(&state)?,
        acknowledgment: None,
    })
}
pub(super) fn row(db: &Connection, id: &str) -> Result<SetupEffectObservation> {
    ensure!(digest_valid(id), "invalid setup receipt identity");
    let (route,intent,text,state):(String,String,String,String)=db.query_row("SELECT CASE WHEN length(route)=64 THEN route ELSE NULL END,CASE WHEN length(intent)=64 THEN intent ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END FROM setup_effects WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let mut value = decode(id.into(), route, intent, text, state)?;
    observations::read(db, &mut value)?;
    Ok(value)
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    for (name, kind, sql) in [
        ("setup_effects", "table", TABLE),
        (
            "setup_active_route",
            "index",
            if version >= 8 {
                super::reconciliation::SETUP_HISTORY_INDEX
            } else {
                INDEX
            },
        ),
        ("setup_intent_identity", "index", INTENT_INDEX),
    ] {
        let actual: Option<(String, String)> = db
            .query_row(
                "SELECT type,sql FROM sqlite_master WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if version < 2 {
            ensure!(
                actual.is_none(),
                "unexpected setup journal layout; original was not modified"
            );
        } else {
            ensure!(
                actual == Some((kind.into(), sql.into())),
                "setup journal layout differs; original was not modified"
            );
        }
    }
    observations::validate_schema(db, version)?;
    if version < 2 {
        return Ok(());
    }
    let triggers: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name='setup_effects'",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        triggers == 0,
        "unexpected setup journal trigger; original was not modified"
    );
    let count: u64 = db.query_row("SELECT count(*) FROM setup_effects", [], |r| r.get(0))?;
    ensure!(
        count <= CAPACITY,
        "setup journal exceeds capacity; original was not modified"
    );
    let mut q=db.prepare("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END,CASE WHEN length(route)=64 THEN route ELSE NULL END,CASE WHEN length(intent)=64 THEN intent ELSE NULL END,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(state)<=16 THEN state ELSE NULL END FROM setup_effects")?;
    let rows = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    for value in rows {
        let (id, route, intent, text, state) = value?;
        let mut value = decode(id, route, intent, text, state)?;
        observations::read(db, &mut value)?;
    }
    Ok(())
}
pub(super) fn upgrade(db: &mut Connection, version: u32) -> Result<()> {
    if version >= 3 {
        return Ok(());
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if version < 2 {
        tx.execute_batch(TABLE)?;
        tx.execute_batch(INDEX)?;
        tx.execute_batch(INTENT_INDEX)?;
    }
    observations::create(&tx)?;
    tx.execute_batch("PRAGMA user_version=3")?;
    tx.commit()?;
    Ok(())
}

fn owner(db: &Connection, boot: &DeviceSessionId) -> Result<()> {
    let value:String=db.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1",[],|r|r.get(0))?;
    ensure!(
        value == boot.as_str(),
        "setup journal belongs to a newer engine epoch"
    );
    Ok(())
}
impl DeviceEffectJournal {
    /// Read-only cross-store presence check. It does not change the boot owner or restore claims.
    pub(crate) fn verify_setup_records(
        state_dir: &Path,
        witnesses: &[(SessionId, RequestId, String)],
    ) -> Result<()> {
        if witnesses.is_empty() {
            return Ok(());
        }
        let path = state_dir.join("device-effects/journal.sqlite3");
        let _held = crate::artifacts::open_regular(&path).map_err(|_| {
            anyhow::anyhow!(
                "setup journal missing; restore retained effect history before starting"
            )
        })?;
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        ensure!(
            Self::validate_existing(&db)? >= 2,
            "setup journal history unavailable"
        );
        for (actor, request, id) in witnesses {
            let observed = row(&db, id)?;
            ensure!(
                &observed.record.actor_session == actor && &observed.record.request_id == request,
                "setup witness correspondence differs"
            );
        }
        Ok(())
    }
    /// Caller must independently check current authority and the native credential route.
    /// Preparing or reading this metadata never creates a dispatch claim.
    pub fn prepare_setup(&mut self, record: SetupEffectRecord) -> Result<PreparedSetupReceipt> {
        record.validate()?;
        ensure!(
            record.preflight_mode.is_some(),
            "new setup requires a captured mode generation"
        );
        ensure!(
            record.acknowledgment_required,
            "new setup must retain acknowledged observations"
        );
        let namespace = record
            .route_namespace_sha256
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("new setup requires a keyed route namespace"))?;
        ensure!(
            record.boot == self.boot,
            "setup belongs to a retired engine boot"
        );
        let text = serde_json::to_string(&record)?;
        ensure!(text.len() <= RECORD_LIMIT, "setup record exceeds limit");
        let id = format!("{:x}", Sha256::digest(text.as_bytes()));
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Setup, &id)?;
        check_namespace(&tx, namespace, &record.registration.device_id, &id)?;
        super::play::available(
            &tx,
            &record.route_sha256,
            namespace,
            &record.registration.device_id,
            None,
        )?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM setup_effects WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if exists {
            let previous = row(&tx, &id)?;
            ensure!(
                previous.state == SetupJournalState::Prepared,
                "setup already attempted or cancelled"
            );
        } else {
            let count: u64 =
                tx.query_row("SELECT count(*) FROM setup_effects", [], |r| r.get(0))?;
            ensure!(count < CAPACITY, "setup journal capacity reached");
            let occupied: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM setup_custody_routes WHERE route=?1)",
                [&record.route_sha256],
                |r| r.get(0),
            )?;
            ensure!(!occupied, "setup route has unresolved history");
            tx.execute(
                "INSERT INTO setup_effects VALUES(?1,?2,?3,?4,'prepared')",
                params![id, record.route_sha256, intent_identity(&record)?, text],
            )?;
        }
        super::reconciliation::update_setup_route(&tx, &id)?;
        tx.commit()?;
        Ok(PreparedSetupReceipt {
            id,
            boot: self.boot.clone(),
        })
    }
    /// Commit uncertainty before returning the sole transport-handoff token.
    pub fn claim_setup(&mut self, receipt: &PreparedSetupReceipt) -> Result<ClaimedSetupReceipt> {
        ensure!(receipt.boot == self.boot, "retired setup receipt");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Setup, &receipt.id)?;
        let value = row(&tx, &receipt.id)?;
        super::play::available(
            &tx,
            &value.record.route_sha256,
            value
                .record
                .route_namespace_sha256
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("setup namespace missing"))?,
            &value.record.registration.device_id,
            None,
        )?;
        ensure!(
            value.record.boot == self.boot && value.state == SetupJournalState::Prepared,
            "setup claim already consumed or retired"
        );
        tx.execute(
            "UPDATE setup_effects SET state='claimed' WHERE id=?1",
            [&receipt.id],
        )?;
        tx.commit()?;
        Ok(ClaimedSetupReceipt {
            id: receipt.id.clone(),
            boot: self.boot.clone(),
        })
    }
    /// Retry metadata only. Neither outcome establishes loaded-program or stopping proof.
    pub fn complete_setup(
        &mut self,
        claim: &ClaimedSetupReceipt,
        outcome: SetupCompletion,
    ) -> Result<()> {
        ensure!(claim.boot == self.boot, "retired setup claim");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        let value = row(&tx, &claim.id)?;
        ensure!(
            value.record.boot == self.boot,
            "setup completion boot differs"
        );
        let (state, name, acknowledgment) = match outcome {
            SetupCompletion::Acknowledged(value) => {
                value.validate()?;
                (SetupJournalState::Acknowledged, "acknowledged", Some(value))
            }
            SetupCompletion::Unknown => (SetupJournalState::Unknown, "unknown", None),
        };
        if super::reconciliation::resolved_keys(&tx)?
            .contains(&(CustodyKind::Setup, claim.id.clone()))
        {
            ensure!(
                value.state == state && value.acknowledgment == acknowledgment,
                "resolved setup outcome cannot advance"
            );
            return Ok(());
        }
        ensure!(
            value.state == SetupJournalState::Claimed
                || (value.state == state && value.acknowledgment == acknowledgment),
            "setup outcome differs; reconciliation required"
        );
        if let Some(observation) = &acknowledgment {
            observations::save(&tx, &claim.id, observation)?;
        }
        tx.execute(
            "UPDATE setup_effects SET state=?1 WHERE id=?2",
            params![name, claim.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Only a provably unclaimed preparation can release its route locally.
    pub fn cancel_setup_unclaimed(&mut self, id: &str) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        owner(&tx, &self.boot)?;
        super::reconciliation::require_unresolved(&tx, CustodyKind::Setup, id)?;
        let value = row(&tx, id)?;
        ensure!(
            matches!(
                value.state,
                SetupJournalState::Prepared | SetupJournalState::Cancelled
            ),
            "attempted setup cannot be cancelled locally"
        );
        tx.execute(
            "UPDATE setup_effects SET state='cancelled' WHERE id=?1",
            [id],
        )?;
        super::reconciliation::update_setup_route(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
    /// Historical observation only; it cannot reconstruct a receipt or claim.
    /// Resolve the original actor/request without source files, credentials or a receipt ID.
    /// Returns historical metadata only, never a reconstructible dispatch claim.
    pub fn setup_request_history(
        &self,
        actor: &SessionId,
        request: &RequestId,
    ) -> Result<Option<SetupEffectObservation>> {
        self.check_owner()?;
        let identity = request_identity(actor, request)?;
        let id:Option<String>=self.db.query_row("SELECT CASE WHEN length(id)=64 THEN id ELSE NULL END FROM setup_effects WHERE intent=?1",[identity],|r|r.get(0)).optional()?;
        id.map(|id| {
            let value = row(&self.db, &id)?;
            ensure!(
                &value.record.actor_session == actor && &value.record.request_id == request,
                "setup request correspondence differs"
            );
            Ok(value)
        })
        .transpose()
    }
    pub fn setup_history(&self, id: &str) -> Result<SetupEffectObservation> {
        self.check_owner()?;
        row(&self.db, id)
    }
    /// Includes prepared, claimed, acknowledged and unknown records across all boots.
    pub fn setup_route_reserved(&self, route_sha256: &str) -> Result<bool> {
        self.check_owner()?;
        ensure!(digest_valid(route_sha256), "invalid setup route identity");
        super::reconciliation::resolved_keys(&self.db)?;
        let id:Option<String>=self.db.query_row("SELECT CASE WHEN length(setup_id)=64 THEN setup_id ELSE NULL END FROM setup_custody_routes WHERE route=?1",[route_sha256],|r|r.get(0)).optional()?;
        if let Some(id) = id {
            let value = row(&self.db, &id)?;
            ensure!(
                value.record.route_sha256 == route_sha256,
                "setup route differs"
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn record(boot: &str, request: &str) -> SetupEffectRecord {
        let artifact = |value: char| {
            let sha = value.to_string().repeat(64);
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha.clone()).unwrap(),
                sha256: sha,
                byte_len: 40,
            }
        };
        SetupEffectRecord {
            boot: DeviceSessionId::new(boot).unwrap(),
            device_session: DeviceSessionId::new("device-session").unwrap(),
            actor_session: SessionId::new("synthetic-actor").unwrap(),
            request_id: RequestId::new(request).unwrap(),
            hosting_request_id: RequestId::new("hosted-original").unwrap(),
            project_id: ProjectId::new("synthetic-project").unwrap(),
            revision: RevisionId::new(1),
            registration: HandyRegistration {
                device_id: DeviceId::new("synthetic-device").unwrap(),
                revision: 1,
                settings: HandyRegistrationSettings {
                    label: "synthetic journal fixture; no device evidence".into(),
                    api_key_env: "FIXTURE_API".into(),
                    connection_key_env: "FIXTURE_ROUTE".into(),
                },
                disabled: false,
                binding_sha256: "a".repeat(64),
            },
            qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                revision: 2,
            },
            master_sha256: "c".repeat(64),
            script: artifact('d'),
            hosted: artifact('e'),
            route_sha256: "f".repeat(64),
            route_namespace_sha256: Some("1".repeat(64)),
            acknowledgment_required: true,
            preflight_mode: Some(HandyDeviceMode {
                mode: 1,
                mode_session_id: 10,
            }),
        }
    }
    fn open(path: &Path, boot: &str) -> DeviceEffectJournal {
        DeviceEffectJournal::open(path, DeviceSessionId::new(boot).unwrap()).unwrap()
    }
    #[test]
    fn setup_journal_single_claim_and_uncertain_or_acknowledged_routes_remain_reserved() {
        for outcome in [
            SetupCompletion::Acknowledged(crate::handy_wire::fixture_setup_acknowledgment()),
            SetupCompletion::Unknown,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut journal = open(dir.path(), "boot");
            let subject = record("boot", "original");
            let receipt = journal.prepare_setup(subject.clone()).unwrap();
            let repeated = journal.prepare_setup(subject.clone()).unwrap();
            assert_eq!(receipt.id(), repeated.id());
            let claim = journal.claim_setup(&receipt).unwrap();
            assert!(journal.claim_setup(&repeated).is_err());
            journal.complete_setup(&claim, outcome.clone()).unwrap();
            journal.complete_setup(&claim, outcome.clone()).unwrap();
            assert!(journal.cancel_setup_unclaimed(receipt.id()).is_err());
            assert!(journal.prepare_setup(subject.clone()).is_err());
            let mut alias = record("boot", "alias");
            alias.registration.device_id = DeviceId::new("another-logical-device").unwrap();
            assert!(journal.prepare_setup(alias).is_err());
            assert!(journal.setup_route_reserved(&subject.route_sha256).unwrap());
            let conflicting = if outcome == SetupCompletion::Unknown {
                SetupCompletion::Acknowledged(crate::handy_wire::fixture_setup_acknowledgment())
            } else {
                SetupCompletion::Unknown
            };
            assert!(journal.complete_setup(&claim, conflicting).is_err());
        }
    }
    #[test]
    fn setup_journal_reboot_fences_tokens_and_only_unclaimed_history_can_be_cancelled() {
        for claimed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut old = open(dir.path(), "old-boot");
            let receipt = old.prepare_setup(record("old-boot", "original")).unwrap();
            let claim = if claimed {
                Some(old.claim_setup(&receipt).unwrap())
            } else {
                None
            };
            let mut current = open(dir.path(), "new-boot");
            assert!(old.claim_setup(&receipt).is_err());
            assert!(current.claim_setup(&receipt).is_err());
            if let Some(claim) = claim {
                assert!(old
                    .complete_setup(
                        &claim,
                        SetupCompletion::Acknowledged(
                            crate::handy_wire::fixture_setup_acknowledgment()
                        )
                    )
                    .is_err());
                assert!(current
                    .complete_setup(
                        &claim,
                        SetupCompletion::Acknowledged(
                            crate::handy_wire::fixture_setup_acknowledgment()
                        )
                    )
                    .is_err());
            }
            assert_eq!(
                current.setup_history(receipt.id()).unwrap().state,
                if claimed {
                    SetupJournalState::Claimed
                } else {
                    SetupJournalState::Prepared
                }
            );
            assert!(current.prepare_setup(record("new-boot", "new")).is_err());
            if claimed {
                assert!(current.cancel_setup_unclaimed(receipt.id()).is_err());
            } else {
                current.cancel_setup_unclaimed(receipt.id()).unwrap();
                current.cancel_setup_unclaimed(receipt.id()).unwrap();
                let next = current.prepare_setup(record("new-boot", "new")).unwrap();
                current.claim_setup(&next).unwrap();
            }
        }
    }
    #[test]
    fn setup_journal_cancellation_and_failed_claim_never_issue_dispatch_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = open(dir.path(), "boot");
        let receipt = j.prepare_setup(record("boot", "original")).unwrap();
        j.db.execute_batch("CREATE TRIGGER fail_setup_claim BEFORE UPDATE ON setup_effects WHEN NEW.state='claimed' BEGIN SELECT RAISE(ABORT,'injected claim failure'); END;").unwrap();
        assert!(j.claim_setup(&receipt).is_err());
        assert_eq!(
            j.setup_history(receipt.id()).unwrap().state,
            SetupJournalState::Prepared
        );
        j.db.execute_batch("DROP TRIGGER fail_setup_claim").unwrap();
        j.cancel_setup_unclaimed(receipt.id()).unwrap();
        assert!(j.claim_setup(&receipt).is_err());
        assert!(!j.setup_route_reserved(&"f".repeat(64)).unwrap());
        assert!(j.prepare_setup(record("boot", "original")).is_err());
        let next = j.prepare_setup(record("boot", "second")).unwrap();
        let claim = j.claim_setup(&next).unwrap();
        j.db.execute_batch("CREATE TRIGGER fail_setup_ack BEFORE UPDATE ON setup_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'injected completion failure'); END;").unwrap();
        assert!(j
            .complete_setup(
                &claim,
                SetupCompletion::Acknowledged(crate::handy_wire::fixture_setup_acknowledgment())
            )
            .is_err());
        assert_eq!(
            j.setup_history(next.id()).unwrap().state,
            SetupJournalState::Claimed
        );
        assert!(j.claim_setup(&next).is_err());
        j.db.execute_batch("DROP TRIGGER fail_setup_ack").unwrap();
        j.complete_setup(
            &claim,
            SetupCompletion::Acknowledged(crate::handy_wire::fixture_setup_acknowledgment()),
        )
        .unwrap();
    }
    #[test]
    fn setup_journal_bad_subject_and_prepare_failure_leave_no_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = open(dir.path(), "boot");
        let mut bad = record("boot", "bad");
        bad.route_sha256 = "credential-name-not-a-route-digest".into();
        assert!(j.prepare_setup(bad).is_err());
        j.db.execute_batch("CREATE TRIGGER fail_setup_prepare BEFORE INSERT ON setup_effects BEGIN SELECT RAISE(ABORT,'injected prepare failure'); END;").unwrap();
        assert!(j.prepare_setup(record("boot", "original")).is_err());
        assert!(!j.setup_route_reserved(&"f".repeat(64)).unwrap());
        assert_eq!(
            j.db.query_row("SELECT count(*) FROM setup_effects", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn setup_journal_v1_upgrade_preserves_play_stop_rows_and_rolls_back_failed_creation() {
        let dir = tempfile::tempdir().unwrap();
        let j = open(dir.path(), "original");
        j.db.execute_batch(
            "DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; DROP TABLE play_operations; DROP TABLE clock_sync_effects; DROP TABLE setup_observations; DROP INDEX setup_active_route; DROP INDEX setup_intent_identity; DROP TABLE setup_effects; PRAGMA user_version=1;",
        )
        .unwrap();
        let legacy = EffectRecord {
            boot: DeviceSessionId::new("original").unwrap(),
            device_session: DeviceSessionId::new("session").unwrap(),
            sequence: 1,
            admission: PlaybackAdmission {
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(1),
                configuration: DeviceConfiguration {
                    device: DeviceId::new("device").unwrap(),
                    firmware: ArtifactId::new("firmware").unwrap(),
                    driver: ArtifactId::new("driver").unwrap(),
                    transport: ArtifactId::new("transport").unwrap(),
                    profile: ArtifactId::new("profile").unwrap(),
                },
                program_artifact: ArtifactId::new("program").unwrap(),
            },
            controller: None,
            qualification_record: ArtifactId::new("synthetic-qualification").unwrap(),
            stop: true,
            request_sha256: "a".repeat(64),
        };
        let text = serde_json::to_string(&legacy).unwrap();
        let id = format!("{:x}", Sha256::digest(text.as_bytes()));
        j.db.execute(
            "INSERT INTO effects VALUES(?1,'device',?2,'unknown')",
            params![id, text],
        )
        .unwrap();
        drop(j);
        let j = open(dir.path(), "next");
        assert_eq!(
            j.db.query_row("SELECT record FROM effects WHERE id=?1", [id], |r| r
                .get::<_, String>(0))
                .unwrap(),
            text
        );
        let history = j.history(&DeviceId::new("device").unwrap(), 1).unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].record.stop);
        assert_eq!(history[0].outcome, Some(DispatchOutcome::Unknown));
        assert_eq!(
            j.db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            12
        );
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE another(route TEXT); CREATE INDEX setup_active_route ON another(route); PRAGMA user_version=1;").unwrap();
        assert!(upgrade(&mut db, 1).is_err());
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='setup_effects'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn setup_journal_capacity_refuses_new_intents_without_discarding_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = open(dir.path(), "boot");
        let tx = j.db.transaction().unwrap();
        for index in 0..CAPACITY {
            let mut value = record("boot", &format!("fixture-{index}"));
            value.route_sha256 = format!("{:x}", Sha256::digest(index.to_le_bytes()));
            let text = serde_json::to_string(&value).unwrap();
            let id = format!("{:x}", Sha256::digest(text.as_bytes()));
            tx.execute(
                "INSERT INTO setup_effects VALUES(?1,?2,?3,?4,'prepared')",
                params![
                    id,
                    value.route_sha256,
                    intent_identity(&value).unwrap(),
                    text
                ],
            )
            .unwrap();
        }
        tx.execute_batch("INSERT INTO setup_custody_routes SELECT route,id FROM setup_effects WHERE state!='cancelled'").unwrap();
        tx.commit().unwrap();
        DeviceEffectJournal::validate_existing(&j.db).unwrap();
        let mut overflow = record("boot", "overflow");
        overflow.registration.device_id = DeviceId::new("overflow-device").unwrap();
        assert!(j
            .prepare_setup(overflow)
            .unwrap_err()
            .to_string()
            .contains("capacity"));
        assert_eq!(
            j.db.query_row("SELECT count(*) FROM setup_effects", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            CAPACITY
        );
    }
    #[test]
    fn setup_journal_corruption_refuses_startup_without_changing_owner_or_bytes() {
        for mutation in [
            "UPDATE setup_effects SET record='{}'",
            "UPDATE setup_effects SET route='bad'",
            "DROP INDEX setup_active_route",
            "PRAGMA user_version=13",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut j = open(dir.path(), "old");
            j.prepare_setup(record("old", "original")).unwrap();
            j.db.execute_batch(mutation).unwrap();
            drop(j);
            let path = dir.path().join("device-effects/journal.sqlite3");
            let before = std::fs::read(&path).unwrap();
            assert!(
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("new").unwrap())
                    .is_err()
            );
            assert_eq!(std::fs::read(path).unwrap(), before);
        }
    }
}

#[cfg(test)]
mod request_identity_tests {
    use super::*;
    #[test]
    fn setup_request_cannot_be_rebound_to_other_hosting_or_route_even_after_cancellation_or_restart(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut j =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let subject = super::tests::record("boot", "original");
        let receipt = j.prepare_setup(subject.clone()).unwrap();
        j.cancel_setup_unclaimed(receipt.id()).unwrap();
        let mut changed = subject.clone();
        changed.hosting_request_id = RequestId::new("other-hosting").unwrap();
        changed.route_sha256 = "a".repeat(64);
        assert!(j.prepare_setup(changed.clone()).is_err());
        drop(j);
        let mut j =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("next-boot").unwrap())
                .unwrap();
        changed.boot = DeviceSessionId::new("next-boot").unwrap();
        assert!(j.prepare_setup(changed).is_err());
        assert_eq!(
            j.setup_history(receipt.id()).unwrap().state,
            SetupJournalState::Cancelled
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn handy_setup_request_lookup_survives_restart_without_reclaiming_or_crossing_actors() {
        let dir = tempfile::tempdir().unwrap();
        let mut j =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let subject = super::tests::record("boot", "lost-request");
        let receipt = j.prepare_setup(subject.clone()).unwrap();
        j.claim_setup(&receipt).unwrap();
        drop(j);
        let j = DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("restart").unwrap())
            .unwrap();
        let found = j
            .setup_request_history(&subject.actor_session, &subject.request_id)
            .unwrap()
            .unwrap();
        assert_eq!(found.id, receipt.id());
        assert_eq!(found.state, SetupJournalState::Claimed);
        assert!(j
            .setup_request_history(&SessionId::new("other-actor").unwrap(), &subject.request_id)
            .unwrap()
            .is_none());
        assert!(j
            .setup_request_history(&subject.actor_session, &RequestId::new("absent").unwrap())
            .unwrap()
            .is_none());
    }
    #[test]
    fn handy_setup_namespace_rotation_and_legacy_active_history_cannot_bypass_reservations() {
        for legacy in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut j =
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap())
                    .unwrap();
            let mut first = super::tests::record("boot", "first");
            let id = if legacy {
                first.route_namespace_sha256 = None;
                let text = serde_json::to_string(&first).unwrap();
                assert!(!text.contains("route_namespace_sha256"));
                let id = format!("{:x}", Sha256::digest(text.as_bytes()));
                j.db.execute(
                    "INSERT INTO setup_effects VALUES(?1,?2,?3,?4,'prepared')",
                    params![
                        id,
                        first.route_sha256,
                        intent_identity(&first).unwrap(),
                        text
                    ],
                )
                .unwrap();
                reconciliation::update_setup_route(&j.db, &id).unwrap();
                id
            } else {
                j.prepare_setup(first.clone()).unwrap().id().to_owned()
            };
            let mut changed = super::tests::record("boot", "changed");
            changed.route_namespace_sha256 = Some("2".repeat(64));
            changed.route_sha256 = "a".repeat(64);
            changed.registration.device_id = DeviceId::new("different-device").unwrap();
            assert!(j
                .prepare_setup(changed.clone())
                .unwrap_err()
                .to_string()
                .contains("legacy or changed route namespace has unresolved setup history"));
            let found = j
                .setup_request_history(&first.actor_session, &first.request_id)
                .unwrap()
                .unwrap();
            assert_eq!(found.id, id);
            j.cancel_setup_unclaimed(&id).unwrap();
            j.prepare_setup(changed).unwrap();
        }
    }
}

#[cfg(test)]
mod logical_device_tests {
    use super::*;
    #[test]
    fn setup_logical_device_reservation_survives_connection_key_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut j =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("boot").unwrap()).unwrap();
        let first = super::tests::record("boot", "first");
        let receipt = j.prepare_setup(first).unwrap();
        j.claim_setup(&receipt).unwrap();
        let mut changed = super::tests::record("boot", "new-key");
        changed.route_sha256 = "a".repeat(64);
        let error = j.prepare_setup(changed).unwrap_err();
        assert!(error.to_string().contains("logical device"));
    }
}
