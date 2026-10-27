//! Durable effect metadata. Recovery never replays a command or restores a
//! controller, admission, headless approval, clock mapping or physical proof.
use crate::playback_runtime::{DispatchOutcome, HandyPlaybackRuntime, PlaybackEffect};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, path::Path, time::Duration};

const RECORD_LIMIT: usize = 8192;
const CAPACITY: u64 = 1024;

#[path = "device_hsp_journal.rs"]
mod hsp;
pub use hsp::{
    CancelledHspReceipt, ClaimedHspPreparationReceipt, ClaimedHspReceipt, HspEffectObservation,
    HspEmergencyReceipt, HspJournalState, HspPreparationMembership, HspPreparationObservation,
    HspResolvedReceipt, HspStreamJournalState, HspStreamMembership, HspStreamObservation,
    HspStreamReceipt, PreparedHspPreparationReceipt, PreparedHspReceipt,
};
#[cfg(test)]
#[path = "device_hsp_journal_native_tests.rs"]
mod hsp_native;

#[path = "device_emergency_stop_journal.rs"]
mod emergency_stop;

#[path = "device_reconciliation_journal.rs"]
mod reconciliation;
pub(crate) use reconciliation::{CustodyKind, CustodyResolutionRecord, CustodySnapshot};

#[path = "device_browser_play_journal.rs"]
mod browser_play;
#[path = "device_play_journal.rs"]
mod play;
pub use play::{
    ClaimedPlayReceipt, PlayClockRecord, PlayIntent, PlayOperationObservation, PlayOperationRecord,
    PlayOperationState, PreparedPlayReceipt,
};

#[path = "device_clock_sync_journal.rs"]
mod clock_sync;
pub use clock_sync::{
    ClaimedClockSyncReceipt, ClockSyncCompletion, ClockSyncEffectObservation,
    ClockSyncEffectRecord, PreparedClockSyncReceipt,
};

#[path = "device_setup_journal.rs"]
mod setup;
pub use setup::{
    ClaimedSetupReceipt, PreparedSetupReceipt, SetupCompletion, SetupEffectObservation,
    SetupEffectRecord, SetupJournalState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedEffectReceipt {
    id: String,
    boot: DeviceSessionId,
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    #[test]
    fn execution_writes_refuse_writer_contention_promptly_and_restore_other_callers_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DeviceEffectJournal::open(
            dir.path(),
            DeviceSessionId::new("nonblocking-boot").unwrap(),
        )
        .unwrap();
        journal
            .db
            .busy_timeout(Duration::from_millis(1750))
            .unwrap();
        let writer = Connection::open(dir.path().join("device-effects/journal.sqlite3")).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = std::time::Instant::now();
        assert!(journal
            .without_writer_wait(|journal| {
                journal
                    .db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?
                    .commit()?;
                Ok(())
            })
            .is_err());
        assert!(begin.elapsed() < Duration::from_millis(750));
        let timeout: u64 = journal
            .db
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 1750);
        writer.execute_batch("COMMIT").unwrap();
        journal
            .without_writer_wait(|journal| {
                journal
                    .db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?
                    .commit()?;
                Ok(())
            })
            .unwrap();
        let timeout: u64 = journal
            .db
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 1750);
    }
}

/// A current effect captured without database I/O. Recording it later does not
/// grant dispatch permission: the final native claim still checks live state.
#[derive(Clone)]
pub(crate) struct EffectIntent {
    record: EffectRecord,
    requires_play_record: bool,
}
impl EffectIntent {
    pub(crate) fn record_id(&self) -> Result<String> {
        let bytes = serde_json::to_vec(&self.record)?;
        ensure!(
            bytes.len() <= RECORD_LIMIT,
            "device effect record exceeds limit"
        );
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    pub boot: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub sequence: u64,
    pub admission: PlaybackAdmission,
    pub controller: Option<ClientId>,
    pub qualification_record: ArtifactId,
    pub stop: bool,
    pub request_sha256: String,
}
#[derive(Clone, Debug)]
pub struct EffectObservation {
    pub id: String,
    pub record: EffectRecord,
    pub outcome: Option<DispatchOutcome>,
}

pub struct DeviceEffectJournal {
    db: Connection,
    boot: DeviceSessionId,
    hsp_cache: hsp::ValidationCache,
}
/// Immutable receipt correspondence only, not live ownership or dispatch authority.
/// Contains no database handle; callback observation may use it without journal I/O.
#[derive(Clone)]
pub struct EffectReceiptBinding {
    boot: DeviceSessionId,
}
impl EffectReceiptBinding {
    /// Immutable correspondence only. This neither reads ownership nor grants a claim.
    pub(crate) fn validate_intent(
        &self,
        effect: &PlaybackEffect,
        intent: &EffectIntent,
    ) -> Result<()> {
        ensure!(intent.record.boot == self.boot, "retired effect intent");
        ensure!(
            intent.requires_play_record == effect.is_referenced_start(),
            "device intent reference mode differs"
        );
        ensure!(
            serde_json::to_vec(&intent.record)?
                == serde_json::to_vec(&self.record_for_effect(effect)?)?,
            "device intent does not match the effect"
        );
        intent.record_id()?;
        Ok(())
    }
    pub(crate) fn capture_intent(
        &self,
        runtime: &HandyPlaybackRuntime,
        effect: &PlaybackEffect,
    ) -> Result<EffectIntent> {
        ensure!(
            runtime.effect_is_current(effect),
            "retired effect cannot be prepared"
        );
        Ok(EffectIntent {
            record: self.record_for_effect(effect)?,
            requires_play_record: effect.is_referenced_start(),
        })
    }
    /// Immutable metadata only; this does not confer dispatch authority.
    pub fn record_for_effect(&self, effect: &PlaybackEffect) -> Result<EffectRecord> {
        Ok(EffectRecord {
            boot: self.boot.clone(),
            device_session: effect.session_id().clone(),
            sequence: effect.sequence(),
            admission: effect.admission().clone(),
            controller: effect.controller().cloned(),
            qualification_record: effect.qualification_record().clone(),
            stop: effect.is_stop(),
            request_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(
                    effect.request().method(),
                    effect.request().path(),
                    effect.request().body()
                ))?)
            ),
        })
    }

    /// Check immutable callback correspondence before changing runtime state.
    /// Completion separately checks persisted integrity and the current boot owner.
    pub(crate) fn validate_effect_receipt(
        &self,
        effect: &PlaybackEffect,
        receipt: &PreparedEffectReceipt,
    ) -> Result<()> {
        ensure!(receipt.boot == self.boot, "retired effect receipt");
        let bytes = serde_json::to_vec(&self.record_for_effect(effect)?)?;
        ensure!(
            receipt.id == format!("{:x}", Sha256::digest(bytes)),
            "device receipt does not match the completed effect"
        );
        Ok(())
    }
}
impl DeviceEffectJournal {
    /// Complete existence query, independent of history pagination. Absence is not physical stop evidence.
    pub(crate) fn has_device_effects(&self, device: &DeviceId) -> Result<bool> {
        self.check_owner()?;
        let resolved = reconciliation::resolved_keys(&self.db)?;
        Ok(hsp::preparation::has_device_custody(&self.db, device)?
            || hsp::has_device_custody(&self.db, device)?
            || !reconciliation::active_effect_ids(&self.db, device, &resolved)?.is_empty())
    }

    pub fn receipt_binding(&self) -> EffectReceiptBinding {
        EffectReceiptBinding {
            boot: self.boot.clone(),
        }
    }
    pub(crate) fn validate_effect_receipt(
        &self,
        effect: &PlaybackEffect,
        receipt: &PreparedEffectReceipt,
    ) -> Result<()> {
        self.receipt_binding()
            .validate_effect_receipt(effect, receipt)
    }
    /// The owning engine supplies a fresh, globally unique boot epoch and
    /// holds its instance lock. Opening a new epoch fences older writers.
    pub fn open(state_dir: &Path, boot: DeviceSessionId) -> Result<Self> {
        let dir = state_dir.join("device-effects");
        crate::artifacts::private_directory(&dir)?;
        let path = dir.join("journal.sqlite3");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        match options.open(&path) {
            Ok(file) => {
                file.sync_all()?;
                crate::artifacts::sync_dir(&dir)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let file = crate::artifacts::open_regular(&path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let metadata = file.metadata()?;
                    ensure!(
                        metadata.nlink() == 1
                            && metadata.uid() == unsafe { libc::geteuid() }
                            && metadata.mode() & 0o777 == 0o600,
                        "device journal must be private and owned by the engine user"
                    );
                }
            }
            Err(e) => return Err(e.into()),
        }
        // Refuse unsupported metadata before writable SQLite can checkpoint WAL on close.
        {
            let probe =
                Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            probe.busy_timeout(Duration::from_secs(2))?;
            Self::validate_existing(&probe)?;
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(2))?;
        let version = Self::validate_existing(&db)?;
        if version == 0 {
            let tx = db.transaction()?;
            tx.execute_batch("CREATE TABLE owner(singleton INTEGER PRIMARY KEY CHECK(singleton=1),boot TEXT NOT NULL); CREATE TABLE effects(id TEXT PRIMARY KEY,device TEXT NOT NULL,record TEXT NOT NULL,outcome TEXT NOT NULL CHECK(outcome IN ('prepared','acknowledged','unknown'))); PRAGMA user_version=1;")?;
            tx.execute(
                "INSERT INTO owner(singleton,boot) VALUES(1,?1)",
                [boot.as_str()],
            )?;
            tx.commit()?;
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=100; PRAGMA journal_size_limit=4194304;")?;
        setup::upgrade(&mut db, version)?;
        clock_sync::upgrade(&mut db, version)?;
        play::upgrade(&mut db, version)?;
        emergency_stop::upgrade(&mut db, version)?;
        reconciliation::upgrade(&mut db, version)?;
        browser_play::upgrade(&mut db, version)?;
        hsp::upgrade(&mut db, version)?;
        hsp::preparation::retire_claims(&mut db, &boot)?;
        db.execute("INSERT INTO owner(singleton,boot) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET boot=excluded.boot",[boot.as_str()])?;
        Ok(Self {
            db,
            boot,
            hsp_cache: hsp::ValidationCache::default(),
        })
    }
    fn validate_existing(db: &Connection) -> Result<u32> {
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(
            version <= 13,
            "device journal requires an explicit migration; original was not modified"
        );
        if version == 0 {
            let tables: u64 = db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                tables == 0,
                "unversioned device journal is not empty; original was not modified"
            );
            setup::validate_schema(db, version)?;
            clock_sync::validate_schema(db, version)?;
            emergency_stop::validate_schema(db, version)?;
            reconciliation::validate_schema(db, version)?;
            browser_play::validate_schema(db, version)?;
            hsp::validate_schema(db, version)?;
            return Ok(version);
        }
        for (table, wanted) in [
            ("owner", vec![("singleton", 1), ("boot", 0)]),
            (
                "effects",
                vec![("id", 1), ("device", 0), ("record", 0), ("outcome", 0)],
            ),
        ] {
            let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
            let actual = statement
                .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(
                actual
                    == wanted
                        .into_iter()
                        .map(|(name, key)| (name.to_string(), key))
                        .collect::<Vec<_>>(),
                "device journal layout mismatch; original was not modified"
            );
        }
        let owners: Vec<(i64, String)> = {
            let mut statement = db.prepare("SELECT singleton,CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner")?;
            let rows = statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        ensure!(
            owners.len() == 1
                && owners[0].0 == 1
                && DeviceSessionId::new(owners[0].1.clone()).is_ok(),
            "invalid journal owner; original was not modified"
        );
        setup::validate_schema(db, version)?;
        clock_sync::validate_schema(db, version)?;
        play::validate_schema(db, version)?;
        emergency_stop::validate_schema(db, version)?;
        reconciliation::validate_schema(db, version)?;
        browser_play::validate_schema(db, version)?;
        hsp::validate_schema(db, version)?;
        Ok(version)
    }
    /// A known foreign epoch is false; unavailable or malformed metadata is an error.
    pub(crate) fn current_owner(&self) -> Result<bool> {
        let current: String =
            self.db
                .query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1", [], |r| r.get(0))?;
        DeviceSessionId::new(current.clone())?;
        Ok(current == self.boot.as_str())
    }
    /// Stop fallback may observe a replacement, but must not wait for metadata.
    /// An unavailable read is not proof that this epoch still owns the journal.
    pub(crate) fn try_current_owner(&mut self) -> Result<bool> {
        self.without_writer_wait(|journal| journal.current_owner())
    }
    fn check_owner(&self) -> Result<()> {
        ensure!(
            self.current_owner()?,
            "device journal belongs to a newer engine epoch"
        );
        Ok(())
    }
    pub fn prepare(
        &mut self,
        runtime: &HandyPlaybackRuntime,
        effect: &PlaybackEffect,
    ) -> Result<PreparedEffectReceipt> {
        let intent = self.receipt_binding().capture_intent(runtime, effect)?;
        self.prepare_intent(&intent)
    }
    /// No runtime/control lock is needed during SQLite writes. A retired intent
    /// may be retained as conservative history, but cannot pass a later claim.
    pub(crate) fn prepare_intent(
        &mut self,
        intent: &EffectIntent,
    ) -> Result<PreparedEffectReceipt> {
        ensure!(
            !intent.requires_play_record,
            "referenced play requires its browser decision and original operation"
        );
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let receipt = Self::prepare_in_transaction(&tx, &self.boot, intent)?;
        ensure!(
            !play::linked(&tx, &receipt.id)?,
            "play effect requires its original operation"
        );
        tx.commit()?;
        Ok(receipt)
    }
    fn prepare_in_transaction(
        tx: &rusqlite::Transaction<'_>,
        boot: &DeviceSessionId,
        intent: &EffectIntent,
    ) -> Result<PreparedEffectReceipt> {
        let record = &intent.record;
        ensure!(&record.boot == boot, "retired effect intent");
        let bytes = serde_json::to_vec(&record)?;
        ensure!(
            bytes.len() <= RECORD_LIMIT,
            "device effect record exceeds limit"
        );
        let id = format!("{:x}", Sha256::digest(&bytes));
        reconciliation::effect_active(tx, &id, record)?;
        let text = String::from_utf8(bytes)?;
        let owner: String =
            tx.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(
            owner == boot.as_str(),
            "device journal belongs to a newer engine epoch"
        );
        let existing: Option<String> = tx
            .query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END FROM effects WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(existing) = existing {
            ensure!(existing == text, "device effect identity conflict");
        } else {
            let count: u64 = tx.query_row("SELECT count(*) FROM effects", [], |r| r.get(0))?;
            ensure!(count < CAPACITY, "device journal capacity reached");
            tx.execute(
                "INSERT INTO effects(id,device,record,outcome) VALUES(?1,?2,?3,'prepared')",
                params![id, record.admission.configuration.device.as_str(), text],
            )?;
        }
        Ok(PreparedEffectReceipt {
            id,
            boot: boot.clone(),
        })
    }
    /// The execution pump never waits for another SQLite writer while owning
    /// the shared journal. Preserve the normal timeout for unrelated callers.
    fn without_writer_wait<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let prior: u64 = self.db.query_row("PRAGMA busy_timeout", [], |r| r.get(0))?;
        self.db.busy_timeout(Duration::ZERO)?;
        let result = operation(self);
        self.db.busy_timeout(Duration::from_millis(prior))?;
        result
    }
    pub(crate) fn try_prepare_intent(
        &mut self,
        intent: &EffectIntent,
    ) -> Result<PreparedEffectReceipt> {
        self.without_writer_wait(|journal| journal.prepare_intent(intent))
    }
    /// Retain a rejected stop's unresolved identity without inventing a claim,
    /// transport outcome, emergency-attempt provenance or dispatch receipt.
    pub(crate) fn try_register_rejected_stop(&mut self, intent: &EffectIntent) -> Result<()> {
        self.without_writer_wait(|journal| {
            journal.check_owner()?;
            emergency_stop::stop_shape(&intent.record)?;
            let tx = journal
                .db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let receipt = Self::prepare_in_transaction(&tx, &journal.boot, intent)?;
            ensure!(
                !play::linked(&tx, &receipt.id)?,
                "rejected stop cannot be a play operation"
            );
            tx.commit()?;
            Ok(())
        })
    }
    pub(crate) fn try_complete(
        &mut self,
        receipt: &PreparedEffectReceipt,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        self.without_writer_wait(|journal| journal.complete(receipt, outcome))
    }
    /// Record a transport observation. Neither outcome is physical stop proof.
    pub fn complete(
        &mut self,
        receipt: &PreparedEffectReceipt,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        self.check_owner()?;
        ensure!(receipt.boot == self.boot, "retired effect receipt");
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        ensure!(
            !play::linked(&tx, &receipt.id)?,
            "play outcome requires its original operation"
        );
        Self::complete_in_transaction(&tx, receipt, outcome)?;
        tx.commit()?;
        Ok(())
    }
    fn complete_in_transaction(
        tx: &rusqlite::Transaction<'_>,
        receipt: &PreparedEffectReceipt,
        outcome: DispatchOutcome,
    ) -> Result<()> {
        let name = match outcome {
            DispatchOutcome::Acknowledged => "acknowledged",
            DispatchOutcome::Unknown => "unknown",
        };
        let owner: String =
            tx.query_row("SELECT CASE WHEN length(CAST(boot AS BLOB)) BETWEEN 1 AND 128 THEN boot ELSE NULL END FROM owner WHERE singleton=1", [], |r| r.get(0))?;
        ensure!(owner == receipt.boot.as_str(), "retired engine epoch");
        let record: String=tx.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END FROM effects WHERE id=?1",[&receipt.id],|r|r.get(0))?;
        ensure!(
            format!("{:x}", Sha256::digest(record.as_bytes())) == receipt.id,
            "device journal integrity mismatch"
        );
        let subject: EffectRecord = serde_json::from_str(&record)?;
        ensure!(
            subject.boot == receipt.boot,
            "device completion belongs to another engine epoch"
        );
        let prior: String = tx.query_row(
            "SELECT CASE WHEN length(CAST(outcome AS BLOB))<=16 THEN outcome ELSE NULL END FROM effects WHERE id=?1",
            [&receipt.id],
            |r| r.get(0),
        )?;
        ensure!(
            prior == "prepared" || prior == name,
            "changed device effect outcome requires reconciliation"
        );
        if reconciliation::resolved_keys(tx)?.contains(&(CustodyKind::Effect, receipt.id.clone())) {
            ensure!(prior == name, "resolved effect outcome cannot advance");
            return Ok(());
        }
        tx.execute(
            "UPDATE effects SET outcome=?1 WHERE id=?2",
            params![name, receipt.id],
        )?;
        Ok(())
    }
    pub fn history(&self, device: &DeviceId, limit: u32) -> Result<Vec<EffectObservation>> {
        self.check_owner()?;
        ensure!(
            (1..=128).contains(&limit),
            "device history requires limit 1..=128"
        );
        let mut statement=self.db.prepare("SELECT id,CASE WHEN length(CAST(record AS BLOB))<=8192 THEN record ELSE NULL END,CASE WHEN length(CAST(outcome AS BLOB))<=16 THEN outcome ELSE NULL END FROM effects WHERE device=?1 ORDER BY rowid DESC LIMIT ?2")?;
        let rows = statement.query_map(params![device.as_str(), limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut observations = vec![];
        for row in rows {
            let (id, text, outcome) = row?;
            ensure!(
                text.len() <= RECORD_LIMIT
                    && format!("{:x}", Sha256::digest(text.as_bytes())) == id,
                "device journal integrity mismatch"
            );
            let record: EffectRecord = serde_json::from_str(&text)?;
            ensure!(
                &record.admission.configuration.device == device
                    && record.sequence > 0
                    && record.request_sha256.len() == 64
                    && record
                        .request_sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "invalid device journal record"
            );
            let outcome = match outcome.as_str() {
                "prepared" => None,
                "acknowledged" => Some(DispatchOutcome::Acknowledged),
                "unknown" => Some(DispatchOutcome::Unknown),
                _ => anyhow::bail!("invalid device journal outcome"),
            };
            observations.push(EffectObservation {
                id,
                record,
                outcome,
            });
        }
        Ok(observations)
    }
}
