//! Versioned, bounded response history stored atomically with setup completion.
use super::*;
const TABLE: &str = "CREATE TABLE setup_observations(setup_id TEXT PRIMARY KEY NOT NULL,sha256 TEXT NOT NULL,observation TEXT NOT NULL)";
const LIMIT: usize = 2048;
fn digest(id: &str, text: &str) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(id, text))?)
    ))
}
pub(super) fn create(db: &Connection) -> Result<()> {
    db.execute_batch(TABLE)?;
    Ok(())
}
pub(super) fn validate_schema(db: &Connection, version: u32) -> Result<()> {
    let actual: Option<(String, String)> = db
        .query_row(
            "SELECT type,sql FROM sqlite_master WHERE name='setup_observations'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if version < 3 {
        ensure!(
            actual.is_none(),
            "unexpected setup observations before migration"
        );
        return Ok(());
    }
    ensure!(
        actual == Some(("table".into(), TABLE.into())),
        "setup observation layout differs"
    );
    let triggers: u64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND tbl_name='setup_observations'",
        [],
        |r| r.get(0),
    )?;
    ensure!(triggers == 0, "unexpected setup observation trigger");
    let count: u64 = db.query_row("SELECT count(*) FROM setup_observations", [], |r| r.get(0))?;
    ensure!(count <= CAPACITY, "setup observations exceed capacity");
    let orphans: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM setup_observations o LEFT JOIN setup_effects e ON e.id=o.setup_id WHERE e.id IS NULL)", [], |r| r.get(0))?;
    ensure!(!orphans, "setup observation has no matching intent");
    Ok(())
}
pub(super) fn read(db: &Connection, value: &mut SetupEffectObservation) -> Result<()> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let raw: Option<(String, String)> = if version >= 3 {
        db.query_row("SELECT CASE WHEN length(sha256)=64 THEN sha256 ELSE NULL END,CASE WHEN length(CAST(observation AS BLOB))<=2048 THEN observation ELSE NULL END FROM setup_observations WHERE setup_id=?1", [&value.id], |r| Ok((r.get(0)?,r.get(1)?))).optional()?
    } else {
        None
    };
    if let Some((sha, text)) = raw {
        ensure!(
            value.state == SetupJournalState::Acknowledged && digest(&value.id, &text)? == sha,
            "setup observation binding differs"
        );
        let observed: HandySetupAcknowledgment = serde_json::from_str(&text)?;
        observed.validate()?;
        value.acknowledgment = Some(observed);
    }
    ensure!(
        !value.record.acknowledgment_required
            || value.state != SetupJournalState::Acknowledged
            || value.acknowledgment.is_some(),
        "required setup observation is missing"
    );
    Ok(())
}
pub(super) fn save(db: &Connection, id: &str, value: &HandySetupAcknowledgment) -> Result<()> {
    value.validate()?;
    let text = serde_json::to_string(value)?;
    ensure!(text.len() <= LIMIT, "setup observation exceeds limit");
    let changed = db.execute(
        "INSERT INTO setup_observations VALUES(?1,?2,?3) ON CONFLICT(setup_id) DO NOTHING",
        params![id, digest(id, &text)?, text],
    )?;
    if changed == 0 {
        let (sha, prior): (String, String) = db.query_row(
            "SELECT sha256,observation FROM setup_observations WHERE setup_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            prior == text && sha == digest(id, &text)?,
            "setup observation already differs"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(path: &Path, boot: &str) -> DeviceEffectJournal {
        DeviceEffectJournal::open(path, DeviceSessionId::new(boot).unwrap()).unwrap()
    }
    #[test]
    fn setup_observation_is_atomic_immutable_and_survives_restart_as_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = open(dir.path(), "boot");
        let prepared = j
            .prepare_setup(super::super::tests::record("boot", "original"))
            .unwrap();
        let claim = j.claim_setup(&prepared).unwrap();
        let ack = crate::handy_wire::fixture_setup_acknowledgment();
        j.db.execute_batch("CREATE TRIGGER fail_observation BEFORE INSERT ON setup_observations BEGIN SELECT RAISE(ABORT,'injected observation failure'); END;").unwrap();
        assert!(j
            .complete_setup(&claim, SetupCompletion::Acknowledged(ack.clone()))
            .is_err());
        let observed = j.setup_history(prepared.id()).unwrap();
        assert_eq!(observed.state, SetupJournalState::Claimed);
        assert!(observed.acknowledgment.is_none());
        j.db.execute_batch("DROP TRIGGER fail_observation; CREATE TRIGGER fail_ack_state BEFORE UPDATE ON setup_effects WHEN NEW.state='acknowledged' BEGIN SELECT RAISE(ABORT,'injected state failure'); END;").unwrap();
        assert!(j
            .complete_setup(&claim, SetupCompletion::Acknowledged(ack.clone()))
            .is_err());
        assert_eq!(
            j.db.query_row("SELECT count(*) FROM setup_observations", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        j.db.execute_batch("DROP TRIGGER fail_ack_state").unwrap();
        j.complete_setup(&claim, SetupCompletion::Acknowledged(ack.clone()))
            .unwrap();
        j.complete_setup(&claim, SetupCompletion::Acknowledged(ack.clone()))
            .unwrap();
        let mut changed = ack.clone();
        changed.stream_id += 1;
        assert!(j
            .complete_setup(&claim, SetupCompletion::Acknowledged(changed))
            .is_err());
        drop(j);
        let j = open(dir.path(), "next-boot");
        let observed = j.setup_history(prepared.id()).unwrap();
        assert_eq!(observed.acknowledgment, Some(ack));
        assert_eq!(observed.record.boot.as_str(), "boot");
        assert!(j
            .setup_route_reserved(&observed.record.route_sha256)
            .unwrap());
    }
    #[test]
    fn setup_observation_v2_upgrade_preserves_legacy_ack_without_fabricating_response() {
        let dir = tempfile::tempdir().unwrap();
        let j = open(dir.path(), "old");
        j.db.execute_batch(
            "DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; DROP TABLE custody_resolution_members; DROP TABLE custody_resolution_census; DROP TABLE custody_resolutions; DROP TABLE setup_custody_routes; DROP TABLE clock_sync_custody_routes; DROP INDEX setup_active_route; CREATE UNIQUE INDEX setup_active_route ON setup_effects(route) WHERE state!='cancelled'; DROP TABLE emergency_stop_observations; DROP TABLE emergency_stop_census; DROP TABLE play_operations; DROP TABLE clock_sync_effects; DROP TABLE setup_observations; PRAGMA user_version=2;",
        )
        .unwrap();
        let mut legacy = super::super::tests::record("old", "legacy");
        legacy.acknowledgment_required = false;
        legacy.preflight_mode = None;
        let text = serde_json::to_string(&legacy).unwrap();
        let id = format!("{:x}", Sha256::digest(text.as_bytes()));
        j.db.execute(
            "INSERT INTO setup_effects VALUES(?1,?2,?3,?4,'acknowledged')",
            params![
                id,
                legacy.route_sha256,
                intent_identity(&legacy).unwrap(),
                text
            ],
        )
        .unwrap();
        drop(j);
        let j = open(dir.path(), "next");
        let history = j.setup_history(&id).unwrap();
        assert_eq!(history.state, SetupJournalState::Acknowledged);
        assert!(history.acknowledgment.is_none());
        assert_eq!(
            j.db.query_row("SELECT record FROM setup_effects WHERE id=?1", [&id], |r| r
                .get::<_, String>(0))
                .unwrap(),
            text
        );
    }
    #[test]
    fn setup_observation_missing_or_corrupt_data_refuses_restart_without_modifying_journal() {
        for mutation in [
            "DELETE FROM setup_observations",
            "UPDATE setup_observations SET sha256=printf('%064d',0)",
            "UPDATE setup_observations SET observation='{}'",
            "UPDATE setup_observations SET setup_id=printf('%064d',0)",
            "UPDATE setup_effects SET state='unknown'",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut j = open(dir.path(), "boot");
            let p = j
                .prepare_setup(super::super::tests::record("boot", "original"))
                .unwrap();
            let c = j.claim_setup(&p).unwrap();
            j.complete_setup(
                &c,
                SetupCompletion::Acknowledged(crate::handy_wire::fixture_setup_acknowledgment()),
            )
            .unwrap();
            j.db.execute_batch(mutation).unwrap();
            drop(j);
            let path = dir.path().join("device-effects/journal.sqlite3");
            let bytes = std::fs::read(&path).unwrap();
            assert!(
                DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("next").unwrap())
                    .is_err()
            );
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
}
