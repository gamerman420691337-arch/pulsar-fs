//! Host-local scope binding and the older-reader barrier for tracked workers.
use anyhow::{ensure, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

const CREATE: &str = "CREATE TABLE worker_fence_binding(scope TEXT PRIMARY KEY NOT NULL)";
pub(crate) const CREATE_STAGING: &str = "CREATE TABLE worker_fence_binding(scope TEXT PRIMARY KEY NOT NULL,minimum_ledger_version INTEGER NOT NULL CHECK(minimum_ledger_version=3))";
pub(crate) const CREATE_CREATION: &str = "CREATE TABLE worker_fence_binding(scope TEXT PRIMARY KEY NOT NULL,minimum_ledger_version INTEGER NOT NULL CHECK(minimum_ledger_version=4))";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) scope: String,
    pub(crate) minimum_ledger_version: u32,
}

pub(crate) fn scope(db: &Connection, version: i64) -> Result<Option<String>> {
    Ok(binding(db, version)?.map(|value| value.scope))
}

pub(crate) fn binding(db: &Connection, version: i64) -> Result<Option<Binding>> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    let present: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='worker_fence_binding')",
        [], |r| r.get(0),
    )?;
    crate::portable_cuda_db::validate(db, version)?;
    if version == 18 && !present {
        return Ok(None);
    }
    if version < 17 {
        ensure!(
            !present,
            "worker fence binding requires schema-17 reader barrier"
        );
        return Ok(None);
    }
    ensure!(
        matches!(version, 17..=20) && present,
        "missing or unknown worker fence binding"
    );
    let sql: Option<String> = db.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='worker_fence_binding' AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",
        [], |r| r.get(0),
    ).optional()?;
    ensure!(
        matches!(
            sql.as_deref(),
            Some(CREATE) | Some(CREATE_STAGING) | Some(CREATE_CREATION)
        ),
        "worker fence binding layout differs"
    );
    let required_layout = match version {
        20 => CREATE_CREATION,
        19 => CREATE_STAGING,
        _ => CREATE,
    };
    ensure!(
        sql.as_deref() == Some(required_layout),
        "worker binding requires its exact schema and mandatory ledger requirement"
    );
    let count: i64 = db.query_row("SELECT COUNT(*) FROM worker_fence_binding", [], |r| {
        r.get(0)
    })?;
    ensure!(count == 1, "worker fence binding must have one scope");
    let value: String = db.query_row(
        "SELECT scope FROM worker_fence_binding WHERE typeof(scope)='text' AND length(CAST(scope AS BLOB))=36",
        [], |r| r.get(0),
    )?;
    ensure!(
        uuid::Uuid::parse_str(&value)?.to_string() == value,
        "worker fence scope is not canonical"
    );
    let minimum_ledger_version = if version >= 19 {
        let required = if version == 20 { 4 } else { 3 };
        db.query_row("SELECT minimum_ledger_version FROM worker_fence_binding WHERE typeof(minimum_ledger_version)='integer' AND minimum_ledger_version=?1", [required], |r| r.get::<_, u32>(0))?
    } else {
        2
    };
    Ok(Some(Binding {
        scope: value,
        minimum_ledger_version,
    }))
}

/// Shares the job transaction, or a fresh authorized preview transaction.
pub(crate) fn arm(tx: &Transaction<'_>, expected: &str) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_worker_fence;")?;
    match arm_inner(tx, expected) {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_worker_fence;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch("ROLLBACK TO pulsar_worker_fence; RELEASE pulsar_worker_fence;")?;
            Err(error)
        }
    }
}
fn arm_inner(tx: &Transaction<'_>, expected: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(expected)?.to_string() == expected,
        "invalid worker fence scope"
    );
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = crate::live_cuda_evidence_db::base_version(tx, version)?;
    if let Some(current) = scope(tx, version)? {
        ensure!(
            current == expected,
            "worker fence scope differs from engine custody"
        );
        return Ok(());
    }
    // Schema 17 also preserves the exact CUDA intent/evidence readers' layouts.
    for (table, create) in [
        ("cuda_attempt_intents", crate::cuda_attempts::CREATE),
        ("cuda_candidate_evidence", crate::cuda_acceptance::CREATE),
    ] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            tx.execute_batch(create)?;
        }
    }
    tx.execute_batch(CREATE)?;
    tx.execute("INSERT INTO worker_fence_binding VALUES(?1)", [expected])?;
    if version < 18 {
        crate::portable_annotated_db::publish(tx, 17)?;
    } else {
        crate::portable_cuda_db::arm(tx)?;
    }
    crate::cuda_attempts::validate(tx, version.max(17))?;
    crate::cuda_acceptance::validate(tx, version.max(17))?;
    ensure!(
        scope(tx, version.max(17))?.as_deref() == Some(expected),
        "worker fence publication binding differs"
    );
    Ok(())
}

/// Commit the staging reader requirement after format 3 is durably enabled.
/// Its savepoint composes with the existing worker/CUDA barriers.
pub(crate) fn arm_staging(tx: &Transaction<'_>, expected: &str) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_staging_requirement;")?;
    let result = (|| -> Result<()> {
        arm(tx, expected)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let prior =
            binding(tx, version)?.ok_or_else(|| anyhow::anyhow!("worker binding missing"))?;
        ensure!(prior.scope == expected, "staging requirement scope differs");
        if prior.minimum_ledger_version >= 3 {
            return Ok(());
        }
        tx.execute_batch("ALTER TABLE worker_fence_binding RENAME TO worker_fence_binding_prior;")?;
        tx.execute_batch(CREATE_STAGING)?;
        tx.execute("INSERT INTO worker_fence_binding VALUES(?1,3)", [expected])?;
        tx.execute_batch("DROP TABLE worker_fence_binding_prior;")?;
        crate::portable_cuda_db::arm(tx)?;
        crate::portable_annotated_db::publish(tx, 19)?;
        let version = 19;
        crate::cuda_attempts::validate(tx, version)?;
        crate::cuda_acceptance::validate(tx, version)?;
        ensure!(
            binding(tx, version)?
                == Some(Binding {
                    scope: expected.into(),
                    minimum_ledger_version: 3
                }),
            "staging requirement publication differs"
        );
        Ok(())
    })();
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_staging_requirement;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_staging_requirement; RELEASE pulsar_staging_requirement;",
            )?;
            Err(error)
        }
    }
}

/// Reader prerequisite for durable cgroup creation. Production admission must
/// publish format 4 first and commit this requirement before any creation intent.
pub(crate) fn arm_creation(tx: &Transaction<'_>, expected: &str) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_creation_requirement;")?;
    let result = (|| -> Result<()> {
        arm_staging(tx, expected)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let prior =
            binding(tx, version)?.ok_or_else(|| anyhow::anyhow!("worker binding missing"))?;
        ensure!(
            prior.scope == expected,
            "creation requirement scope differs"
        );
        if prior.minimum_ledger_version == 4 {
            return Ok(());
        }
        tx.execute_batch("ALTER TABLE worker_fence_binding RENAME TO worker_fence_binding_prior;")?;
        tx.execute_batch(CREATE_CREATION)?;
        tx.execute("INSERT INTO worker_fence_binding VALUES(?1,4)", [expected])?;
        tx.execute_batch("DROP TABLE worker_fence_binding_prior;")?;
        crate::portable_cuda_db::arm(tx)?;
        crate::portable_annotated_db::publish(tx, 20)?;
        crate::cuda_attempts::validate(tx, 20)?;
        crate::cuda_acceptance::validate(tx, 20)?;
        ensure!(
            binding(tx, 20)?
                == Some(Binding {
                    scope: expected.into(),
                    minimum_ledger_version: 4
                }),
            "creation requirement publication differs"
        );
        Ok(())
    })();
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_creation_requirement;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_creation_requirement; RELEASE pulsar_creation_requirement;",
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_requirement_is_transactional_and_cannot_be_downgraded_by_other_writers() {
        for starting in [14, 17, 18, 19] {
            let mut db = fixture();
            let expected = uuid::Uuid::new_v4().to_string();
            if starting >= 17 {
                let tx = db.transaction().unwrap();
                arm(&tx, &expected).unwrap();
                if starting >= 18 {
                    crate::portable_cuda_db::arm(&tx).unwrap();
                }
                if starting >= 19 {
                    arm_staging(&tx, &expected).unwrap();
                }
                tx.commit().unwrap();
            }
            {
                let tx = db.transaction().unwrap();
                arm_creation(&tx, &expected).unwrap();
                assert_eq!(
                    binding(&tx, 20).unwrap(),
                    Some(Binding {
                        scope: expected.clone(),
                        minimum_ledger_version: 4
                    })
                );
            }
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                starting
            );
            let tx = db.transaction().unwrap();
            arm_creation(&tx, &expected).unwrap();
            arm_creation(&tx, &expected).unwrap();
            arm_staging(&tx, &expected).unwrap();
            arm(&tx, &expected).unwrap();
            crate::portable_cuda_db::arm(&tx).unwrap();
            assert!(arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).is_err());
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                20
            );
            assert_eq!(
                binding(&tx, 20).unwrap(),
                Some(Binding {
                    scope: expected,
                    minimum_ledger_version: 4
                })
            );
            let marker = crate::portable_cuda_db::validate(&tx, 20).unwrap().unwrap();
            assert!(marker.worker_fence && marker.cuda_intents && marker.cuda_evidence);
            tx.commit().unwrap();
            assert_eq!(binding(&db, 20).unwrap().unwrap().minimum_ledger_version, 4);
        }
    }

    #[test]
    fn creation_requirement_rejects_lost_columns_layouts_rows_and_downgrades() {
        for case in [
            "future-minimum",
            "staging-minimum",
            "lost-column",
            "changed-layout",
            "missing-scope",
            "missing-binding",
            "missing-cuda-marker",
            "schema19-downgrade",
            "schema18-downgrade",
        ] {
            let mut db = fixture();
            let tx = db.transaction().unwrap();
            arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
            tx.commit().unwrap();
            match case {
                "future-minimum"=>db.execute_batch("PRAGMA ignore_check_constraints=ON;UPDATE worker_fence_binding SET minimum_ledger_version=5;").unwrap(),
                "staging-minimum"=>db.execute_batch("PRAGMA ignore_check_constraints=ON;UPDATE worker_fence_binding SET minimum_ledger_version=3;").unwrap(),
                "lost-column"=>db.execute_batch("ALTER TABLE worker_fence_binding DROP COLUMN minimum_ledger_version;").unwrap(),
                "changed-layout"=>db.execute_batch("ALTER TABLE worker_fence_binding ADD COLUMN ignored INTEGER;").unwrap(),
                "missing-binding"=>db.execute_batch("DROP TABLE worker_fence_binding;").unwrap(),
                "missing-cuda-marker"=>db.execute_batch("DROP TABLE portable_cuda_reader_state;").unwrap(),
                "schema19-downgrade"=>db.execute_batch("PRAGMA user_version=19;").unwrap(),
                "schema18-downgrade"=>db.execute_batch("PRAGMA user_version=18;").unwrap(),
                _=>db.execute_batch("DELETE FROM worker_fence_binding;").unwrap(),
            }
            let version = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert!(binding(&db, version).is_err(), "{case}");
            let tx = db.transaction().unwrap();
            assert!(
                arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).is_err(),
                "{case}"
            );
        }
    }

    #[test]
    fn creation_schema_retains_archived_cuda_metadata_validation() {
        let mut db = fixture();
        let tx = db.transaction().unwrap();
        arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
        tx.execute_batch("CREATE TABLE project_import_archives(project TEXT,manifest TEXT);INSERT INTO project_import_archives VALUES('authored','not-json');").unwrap();
        assert!(crate::portable_cuda_db::validate_metadata(&tx, 20).is_err());
    }

    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "PRAGMA foreign_keys=ON;PRAGMA user_version=14;
        CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT,manifest TEXT,state TEXT,candidate TEXT);
        CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT,manifest TEXT,state TEXT);
        CREATE TABLE candidates(id TEXT PRIMARY KEY);",
        )
        .unwrap();
        db
    }
    #[test]
    fn creation_activation_preserves_portable_annotated_barrier() {
        let mut db = fixture();
        let tx = db.transaction().unwrap();
        crate::portable_cuda_db::arm(&tx).unwrap();
        crate::portable_annotated_db::arm(&tx).unwrap();
        let scope = uuid::Uuid::new_v4().to_string();
        arm_creation(&tx, &scope).unwrap();
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            24
        );
        assert_eq!(
            crate::portable_annotated_db::storage_version(&tx).unwrap(),
            20
        );
        assert_eq!(binding(&tx, 24).unwrap().unwrap().minimum_ledger_version, 4);
        crate::cuda_attempts::validate(&tx, 24).unwrap();
        crate::cuda_acceptance::validate(&tx, 24).unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn staging_requirement_is_transactional_idempotent_and_preserves_cuda_requirements() {
        for archival in [false, true] {
            let mut db = fixture();
            let expected = uuid::Uuid::new_v4().to_string();
            {
                let tx = db.transaction().unwrap();
                if archival {
                    crate::portable_cuda_db::arm(&tx).unwrap();
                }
                arm_staging(&tx, &expected).unwrap();
                arm_staging(&tx, &expected).unwrap();
                arm(&tx, &expected).unwrap();
                let version = 19;
                assert_eq!(
                    binding(&tx, version).unwrap(),
                    Some(Binding {
                        scope: expected.clone(),
                        minimum_ledger_version: 3
                    })
                );
                assert_eq!(
                    tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                        .unwrap(),
                    version
                );
                if archival {
                    let marker = crate::portable_cuda_db::validate(&tx, 19).unwrap().unwrap();
                    assert!(marker.worker_fence && marker.cuda_intents && marker.cuda_evidence);
                }
                assert!(arm_staging(&tx, &uuid::Uuid::new_v4().to_string()).is_err());
            }
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                14
            );
            assert!(binding(&db, 14).unwrap().is_none());
            let tx = db.transaction().unwrap();
            arm(&tx, &expected).unwrap();
            tx.commit().unwrap();
            {
                let tx = db.transaction().unwrap();
                arm_staging(&tx, &expected).unwrap();
            }
            assert_eq!(binding(&db, 17).unwrap().unwrap().minimum_ledger_version, 2);
            let tx = db.transaction().unwrap();
            arm_staging(&tx, &expected).unwrap();
            tx.commit().unwrap();
            assert_eq!(binding(&db, 19).unwrap().unwrap().minimum_ledger_version, 3);
        }
    }
    #[test]
    fn changed_or_unknown_staging_requirement_refuses_instead_of_downgrading() {
        for case in [
            "future-version",
            "downgraded-minimum",
            "changed-layout",
            "missing-scope",
            "missing-binding",
            "missing-cuda-marker",
            "downgraded-schema",
        ] {
            let mut db = fixture();
            let tx = db.transaction().unwrap();
            arm_staging(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
            tx.commit().unwrap();
            match case {
                "future-version"=>db.execute_batch("PRAGMA ignore_check_constraints=ON;UPDATE worker_fence_binding SET minimum_ledger_version=4;").unwrap(),
                "downgraded-minimum"=>db.execute_batch("PRAGMA ignore_check_constraints=ON;UPDATE worker_fence_binding SET minimum_ledger_version=2;").unwrap(),
                "changed-layout"=>db.execute_batch("ALTER TABLE worker_fence_binding ADD COLUMN ignored INTEGER;").unwrap(),
                "missing-binding"=>db.execute_batch("DROP TABLE worker_fence_binding;").unwrap(),
                "missing-cuda-marker"=>db.execute_batch("DROP TABLE portable_cuda_reader_state;").unwrap(),
                "downgraded-schema"=>db.execute_batch("PRAGMA user_version=18;").unwrap(),
                _=>db.execute_batch("DELETE FROM worker_fence_binding;").unwrap(),
            }
            let version = db
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert!(binding(&db, version).is_err(), "{case}");
            assert!(scope(&db, version).is_err(), "{case}");
        }
    }
    #[test]
    fn lost_staging_requirement_column_cannot_be_accepted_as_legacy_binding() {
        let mut db = fixture();
        let tx = db.transaction().unwrap();
        arm_staging(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
        tx.commit().unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        db.execute_batch("ALTER TABLE worker_fence_binding DROP COLUMN minimum_ledger_version;")
            .unwrap();
        assert!(
            binding(&db, version).is_err(),
            "lost staging requirement was silently accepted as legacy format-2 authority"
        );
    }
    #[test]
    fn staging_requirement_retains_schema19_during_cuda_rearming_and_archive_validation() {
        let mut db = fixture();
        let tx = db.transaction().unwrap();
        let expected = uuid::Uuid::new_v4().to_string();
        arm_staging(&tx, &expected).unwrap();
        crate::portable_cuda_db::arm(&tx).unwrap();
        arm(&tx, &expected).unwrap();
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
        crate::cuda_attempts::validate(&tx, 19).unwrap();
        crate::cuda_acceptance::validate(&tx, 19).unwrap();
        assert_eq!(binding(&tx, 19).unwrap().unwrap().minimum_ledger_version, 3);
        tx.execute_batch("CREATE TABLE project_import_archives(project TEXT,manifest TEXT);INSERT INTO project_import_archives VALUES('authored','not-json');").unwrap();
        assert!(
            crate::portable_cuda_db::validate_metadata(&tx, 19).is_err(),
            "schema 19 skipped malformed archive admission"
        );
    }
    #[test]
    fn archival_reader_barrier_survives_worker_binding_and_retains_requirements() {
        let mut db = fixture();
        {
            let tx = db.transaction().unwrap();
            crate::portable_cuda_db::arm(&tx).unwrap();
            assert!(scope(&tx, 18).unwrap().is_none());
            let expected = uuid::Uuid::new_v4().to_string();
            arm(&tx, &expected).unwrap();
            assert_eq!(scope(&tx, 18).unwrap().as_deref(), Some(expected.as_str()));
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                18
            );
            let marker = crate::portable_cuda_db::validate(&tx, 18).unwrap().unwrap();
            assert!(marker.worker_fence && marker.cuda_intents && marker.cuda_evidence);
            tx.commit().unwrap();
        }
        db.execute_batch("DROP TABLE worker_fence_binding;")
            .unwrap();
        assert!(scope(&db, 18).is_err());
    }
    #[test]
    fn binding_and_barrier_roll_back_with_caller_then_bind_idempotently() {
        let mut db = fixture();
        let uuid = uuid::Uuid::new_v4().to_string();
        {
            let tx = db.transaction().unwrap();
            arm(&tx, &uuid).unwrap();
            assert_eq!(scope(&tx, 17).unwrap().as_deref(), Some(uuid.as_str()));
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        assert_eq!(scope(&db, 14).unwrap(), None);
        let tx = db.transaction().unwrap();
        arm(&tx, &uuid).unwrap();
        arm(&tx, &uuid).unwrap();
        assert!(arm(&tx, &uuid::Uuid::new_v4().to_string()).is_err());
        tx.commit().unwrap();
        assert_eq!(scope(&db, 17).unwrap().as_deref(), Some(uuid.as_str()));
        assert!(scope(&db, 16).is_err());
        crate::cuda_attempts::validate(&db, 17).unwrap();
        crate::cuda_acceptance::validate(&db, 17).unwrap();
    }
    #[test]
    fn failed_arm_preserves_caller_transaction_without_partial_binding() {
        let mut db = fixture();
        db.execute_batch("CREATE TABLE cuda_attempt_intents(bad TEXT);")
            .unwrap();
        let tx = db.transaction().unwrap();
        tx.execute_batch(
            "CREATE TABLE caller_note(value INTEGER);INSERT INTO caller_note VALUES(7);",
        )
        .unwrap();
        assert!(arm(&tx, &uuid::Uuid::new_v4().to_string()).is_err());
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        assert_eq!(scope(&db, 14).unwrap(), None);
        assert_eq!(
            db.query_row("SELECT value FROM caller_note", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='cuda_candidate_evidence'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn missing_altered_multiple_or_oversized_scopes_refuse() {
        for case in ["missing", "layout", "multiple", "oversized", "noncanonical"] {
            let mut db = fixture();
            let tx = db.transaction().unwrap();
            arm(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
            tx.commit().unwrap();
            match case {
                "missing"=>db.execute_batch("DROP TABLE worker_fence_binding").unwrap(),
                "layout"=>db.execute_batch("DROP TABLE worker_fence_binding;CREATE TABLE worker_fence_binding(scope TEXT)").unwrap(),
                "multiple"=>{db.execute("INSERT INTO worker_fence_binding VALUES(?1)",[uuid::Uuid::new_v4().to_string()]).unwrap();},
                "oversized"=>{db.execute("UPDATE worker_fence_binding SET scope=?1",["x".repeat(1024*1024)]).unwrap();},
                _=>{db.execute("UPDATE worker_fence_binding SET scope=?1",[uuid::Uuid::new_v4().to_string().to_uppercase()]).unwrap();},
            }
            assert!(scope(&db, 17).is_err(), "{case}");
        }
    }
}
