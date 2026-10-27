//! Reader barrier for inference equivalence decisions and transitions, nested
//! beneath any closed CPU runtime reader requirement.
//!
//! This module validates only the storage header. The registry separately
//! establishes which retained decisions and transitions require this barrier.
//! Arming never grants qualification, and no operation here removes the marker.
use anyhow::{ensure, Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction};

const NAME: &str = "inference_equivalence_reader";
const MARKER: &str = "CREATE TABLE inference_equivalence_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 34))";
const OUTER_VERSION: i64 = 35;

/// Returns the preserved version and whether the outer barrier is armed.
/// All queries are read-only and name the main database explicitly.
fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw = crate::cpu_runtime_reader::version(db)?;
    ensure!(
        raw == 0 || (2..=OUTER_VERSION).contains(&raw),
        "unknown inference equivalence reader version"
    );
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)",
        [NAME],
        |r| r.get(0),
    )?;
    ensure!(
        !temporary,
        "inference equivalence reader temporary object differs"
    );
    // A canonical marker has no associated index or trigger. Count at most two
    // objects before admitting its exact SQL, without allocating schema text.
    let objects: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)",
        [NAME],
        |r| r.get(0),
    )?;
    if raw != OUTER_VERSION {
        ensure!(
            objects == 0,
            "inference equivalence reader barrier downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        objects == 1,
        "inference equivalence reader marker missing or associated objects differ"
    );
    let layout: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",
        rusqlite::params![NAME, MARKER.len() as i64, MARKER],
        |r| r.get(0),
    )?;
    ensure!(layout, "inference equivalence reader layout differs");
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.inference_equivalence_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(count == 1, "inference equivalence reader singleton differs");
    let underlying: Option<i64> = db
        .query_row(
            "SELECT underlying_version FROM main.inference_equivalence_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 34",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok((
        underlying.context("inference equivalence reader scalar admission differs")?,
        true,
    ))
}

/// Read the previous outer header, preserving its own wrapper semantics.
pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

/// Publish an inner header monotonically without erasing an armed barrier.
pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=34).contains(&new_version) && new_version >= old,
        "inference equivalence reader publication would downgrade or exceed supported storage"
    );
    if new_version == old {
        return Ok(());
    }
    if armed {
        let changed = db.execute(
            "UPDATE main.inference_equivalence_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2",
            rusqlite::params![new_version, old],
        )?;
        ensure!(
            changed == 1,
            "inference equivalence reader publication lost its retained header"
        );
    } else {
        crate::cpu_runtime_reader::publish(db, new_version)?;
    }
    Ok(())
}

/// Arm in the same transaction as the decision or transition requiring it.
/// The caller must commit both; rolling back retains the prior header exactly.
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let (old, armed) = header(tx)?;
    ensure!(
        (2..=34).contains(&old),
        "inference equivalence requires supported initialized storage"
    );
    if !armed {
        tx.execute_batch(MARKER)?;
        tx.execute(
            "INSERT INTO main.inference_equivalence_reader(singleton,underlying_version) VALUES(1,?1)",
            [old],
        )?;
        crate::cpu_runtime_reader::publish(tx, OUTER_VERSION)?;
    }
    ensure!(
        header(tx)? == (old, true),
        "inference equivalence reader arming did not preserve storage"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(raw: i64) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(Some(rusqlite::DatabaseName::Main), "user_version", raw)
            .unwrap();
        db
    }

    fn raw(db: &Connection) -> i64 {
        db.query_row("PRAGMA main.user_version", [], |r| r.get(0))
            .unwrap()
    }

    fn marker_count(db: &Connection) -> i64 {
        db.query_row(
            "SELECT COUNT(*) FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE",
            [NAME],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn armed(old: i64) -> Connection {
        let mut db = db(old);
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        db
    }

    fn assert_read_only_rejection(db: &Connection) {
        let before = raw(db);
        let changes: i64 = db
            .query_row("SELECT total_changes()", [], |r| r.get(0))
            .unwrap();
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(version(db).is_err());
        assert_eq!(raw(db), before);
        assert_eq!(
            db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            changes
        );
    }

    #[test]
    fn every_supported_underlying_header_is_preserved_and_arming_is_idempotent() {
        for old in 2..=34 {
            let mut db = db(old);
            assert_eq!(version(&db).unwrap(), old);
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            arm(&tx).unwrap();
            assert_eq!(version(&tx).unwrap(), old);
            assert_eq!(raw(&tx), 35);
            tx.commit().unwrap();
            assert_eq!(version(&db).unwrap(), old);
            assert_eq!(raw(&db), 35);
            assert_eq!(marker_count(&db), 1);
        }
    }

    #[test]
    fn empty_storage_is_readable_but_cannot_arm_until_initialized() {
        let mut db = db(0);
        db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert_eq!(version(&db).unwrap(), 0);
        assert_eq!(marker_count(&db), 0);
        db.execute_batch("PRAGMA query_only=OFF").unwrap();
        let tx = db.transaction().unwrap();
        assert!(arm(&tx).is_err());
        tx.commit().unwrap();
        assert_eq!(raw(&db), 0);
        assert_eq!(marker_count(&db), 0);
        publish(&db, 2).unwrap();
        assert_eq!(version(&db).unwrap(), 2);
    }

    #[test]
    fn unsupported_raw_headers_refuse_read_only() {
        for invalid in [-1, 1, 36, i32::MAX.into()] {
            assert_read_only_rejection(&db(invalid));
        }
    }

    #[test]
    fn arming_and_inner_publication_roll_back_with_the_owner_transaction() {
        for old in 2..=34 {
            let mut db = db(old);
            {
                let tx = db.transaction().unwrap();
                arm(&tx).unwrap();
                publish(&tx, 34).unwrap();
                assert_eq!(raw(&tx), 35);
                tx.rollback().unwrap();
            }
            assert_eq!(raw(&db), old);
            assert_eq!(version(&db).unwrap(), old);
            assert_eq!(marker_count(&db), 0);
        }
        let mut db = armed(20);
        let tx = db.transaction().unwrap();
        publish(&tx, 34).unwrap();
        tx.rollback().unwrap();
        assert_eq!(raw(&db), 35);
        assert_eq!(version(&db).unwrap(), 20);
        assert_eq!(marker_count(&db), 1);
    }

    #[test]
    fn publication_advances_only_the_preserved_header_and_never_erases_marker() {
        let mut db = armed(2);
        for target in 2..=34 {
            publish(&db, target).unwrap();
            publish(&db, target).unwrap();
            assert_eq!(raw(&db), 35);
            assert_eq!(version(&db).unwrap(), target);
            assert_eq!(marker_count(&db), 1);
        }
        for invalid in [-1, 0, 1, 2, 33, 35, 36] {
            assert!(publish(&db, invalid).is_err());
            assert_eq!(raw(&db), 35);
            assert_eq!(version(&db).unwrap(), 34);
        }
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(raw(&db), 35);
        assert_eq!(version(&db).unwrap(), 34);
    }

    #[test]
    fn unarmed_publication_advances_the_actual_header_without_creating_marker() {
        let db = db(0);
        for target in 2..=34 {
            publish(&db, target).unwrap();
            assert_eq!(raw(&db), target);
            assert_eq!(version(&db).unwrap(), target);
            assert_eq!(marker_count(&db), 0);
        }
        for invalid in [-1, 0, 1, 2, 33, 35, 36] {
            assert!(publish(&db, invalid).is_err());
            assert_eq!(raw(&db), 34);
            assert_eq!(marker_count(&db), 0);
        }
    }

    #[test]
    fn stripped_marker_and_every_lowered_header_refuse_read_only() {
        let db = armed(20);
        db.execute_batch("DROP TABLE main.inference_equivalence_reader")
            .unwrap();
        assert_read_only_rejection(&db);
        for lower in std::iter::once(0).chain(2..=34) {
            let db = armed(20);
            db.pragma_update(Some(rusqlite::DatabaseName::Main), "user_version", lower)
                .unwrap();
            assert_read_only_rejection(&db);
        }
    }

    #[test]
    fn altered_layout_case_and_view_refuse_before_reading_marker_rows() {
        for sql in [
            "CREATE TABLE inference_equivalence_reader(singleton INTEGER,underlying_version INTEGER);INSERT INTO inference_equivalence_reader VALUES(1,20)",
            "CREATE TABLE Inference_Equivalence_Reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 34));INSERT INTO Inference_Equivalence_Reader VALUES(1,20)",
            "CREATE VIEW inference_equivalence_reader AS SELECT 1 AS singleton,20 AS underlying_version",
        ] {
            let db = db(35);
            db.execute_batch(sql).unwrap();
            assert_read_only_rejection(&db);
        }
    }

    #[test]
    fn missing_extra_wrong_singleton_and_invalid_scalar_rows_refuse_read_only() {
        for sql in [
            "DELETE FROM inference_equivalence_reader",
            "INSERT INTO inference_equivalence_reader VALUES(2,20)",
            "UPDATE inference_equivalence_reader SET singleton=2",
            "UPDATE inference_equivalence_reader SET underlying_version=1",
            "UPDATE inference_equivalence_reader SET underlying_version=35",
            "UPDATE inference_equivalence_reader SET underlying_version=20.5",
            "UPDATE inference_equivalence_reader SET underlying_version='invalid'",
            "UPDATE inference_equivalence_reader SET underlying_version=X'3230'",
        ] {
            let db = armed(20);
            db.execute_batch("PRAGMA ignore_check_constraints=ON")
                .unwrap();
            db.execute_batch(sql).unwrap();
            assert_read_only_rejection(&db);
        }
    }

    #[test]
    fn associated_triggers_and_indexes_are_rejected_before_publication() {
        for sql in [
            "CREATE TABLE unrelated(value INTEGER);CREATE TRIGGER redirect AFTER UPDATE ON inference_equivalence_reader BEGIN INSERT INTO unrelated VALUES(1);END",
            "CREATE INDEX extra_marker_index ON inference_equivalence_reader(underlying_version)",
            "CREATE TEMP TRIGGER temporary_redirect AFTER UPDATE ON main.inference_equivalence_reader BEGIN SELECT RAISE(IGNORE);END",
        ] {
            let db = armed(20);
            db.execute_batch(sql).unwrap();
            assert!(publish(&db, 21).is_err());
            assert_eq!(raw(&db), 35);
            let old: i64 = db.query_row("SELECT underlying_version FROM main.inference_equivalence_reader", [], |r| r.get(0)).unwrap();
            assert_eq!(old, 20);
            assert_read_only_rejection(&db);
        }
    }

    #[test]
    fn temporary_marker_shadowing_refuses_both_armed_and_unarmed_storage() {
        for armed_storage in [false, true] {
            let mut db = if armed_storage { armed(20) } else { db(20) };
            db.execute_batch("CREATE TEMP TABLE inference_equivalence_reader(singleton INTEGER,underlying_version INTEGER);INSERT INTO temp.inference_equivalence_reader VALUES(1,2)").unwrap();
            assert!(publish(&db, 21).is_err());
            let tx = db.transaction().unwrap();
            assert!(arm(&tx).is_err());
            tx.rollback().unwrap();
            assert_eq!(raw(&db), if armed_storage { 35 } else { 20 });
            assert_eq!(marker_count(&db), i64::from(armed_storage));
            assert_read_only_rejection(&db);
        }
    }

    #[test]
    fn attached_database_marker_is_neither_read_nor_modified() {
        let mut db = db(20);
        db.execute_batch("ATTACH DATABASE ':memory:' AS unrelated;CREATE TABLE unrelated.inference_equivalence_reader(singleton INTEGER,underlying_version INTEGER);INSERT INTO unrelated.inference_equivalence_reader VALUES(1,2);PRAGMA unrelated.user_version=2").unwrap();
        assert_eq!(version(&db).unwrap(), 20);
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        publish(&tx, 34).unwrap();
        tx.commit().unwrap();
        assert_eq!(raw(&db), 35);
        assert_eq!(version(&db).unwrap(), 34);
        let other: i64 = db
            .query_row(
                "SELECT underlying_version FROM unrelated.inference_equivalence_reader",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(other, 2);
        assert_eq!(
            db.query_row("PRAGMA unrelated.user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn closed_cpu_outer_reader_preserves_equivalence_in_either_activation_order() {
        for old in 2..=34 {
            for cpu_first in [false, true] {
                let mut db = db(old);
                let tx = db.transaction().unwrap();
                if cpu_first {
                    crate::cpu_runtime_reader::arm(&tx).unwrap();
                    arm(&tx).unwrap();
                } else {
                    arm(&tx).unwrap();
                    crate::cpu_runtime_reader::arm(&tx).unwrap();
                }
                assert_eq!(raw(&tx), 36);
                assert_eq!(crate::cpu_runtime_reader::version(&tx).unwrap(), 35);
                assert_eq!(version(&tx).unwrap(), old);
                publish(&tx, 34).unwrap();
                arm(&tx).unwrap();
                crate::cpu_runtime_reader::arm(&tx).unwrap();
                assert!(crate::cpu_runtime_reader::publish(&tx, 34).is_err());
                assert_eq!(version(&tx).unwrap(), 34);
                tx.commit().unwrap();
                assert_eq!(raw(&db), 36);
                assert_eq!(crate::cpu_runtime_reader::version(&db).unwrap(), 35);
                assert_eq!(marker_count(&db), 1);
            }
        }
    }

    #[test]
    fn nested_closed_cpu_activation_rolls_back_and_missing_inner_marker_refuses() {
        let mut db = armed(20);
        {
            let tx = db.transaction().unwrap();
            crate::cpu_runtime_reader::arm(&tx).unwrap();
            publish(&tx, 34).unwrap();
            tx.rollback().unwrap();
        }
        assert_eq!(raw(&db), 35);
        assert_eq!(version(&db).unwrap(), 20);
        let tx = db.transaction().unwrap();
        crate::cpu_runtime_reader::arm(&tx).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 36).unwrap(),
            20
        );
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 35).unwrap(),
            20
        );
        db.execute_batch("DROP TABLE main.inference_equivalence_reader")
            .unwrap();
        assert_read_only_rejection(&db);
        assert!(crate::live_cuda_evidence_db::base_version(&db, 36).is_err());
    }

    #[test]
    fn closed_cpu_reader_survives_portable_batch_and_underlying_storage_updates() {
        for equivalence in [false, true] {
            let mut db = db(18);
            db.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY)")
                .unwrap();
            let tx = db.transaction().unwrap();
            crate::portable_annotated_db::arm(&tx).unwrap();
            crate::portable_wasm_db::arm(&tx).unwrap();
            crate::wasm_archive_db::arm(&tx).unwrap();
            crate::library_batch_db::arm(&tx).unwrap();
            if equivalence {
                arm(&tx).unwrap();
            }
            crate::cpu_runtime_reader::arm(&tx).unwrap();
            crate::portable_annotated_db::publish(&tx, 20).unwrap();
            crate::library_batch_db::arm_partial_audio(&tx).unwrap();
            crate::library_batch_db::arm_cancel(&tx).unwrap();
            crate::library_batch_db::arm_pack(&tx).unwrap();
            crate::library_batch_db::capacity(&tx).unwrap();
            tx.commit().unwrap();
            assert_eq!(raw(&db), 36);
            assert_eq!(
                crate::cpu_runtime_reader::version(&db).unwrap(),
                if equivalence { 35 } else { 34 }
            );
            assert_eq!(version(&db).unwrap(), 34);
            assert_eq!(crate::library_batch_db::version(&db).unwrap(), 26);
            assert_eq!(crate::native_preparation_db::version(&db).unwrap(), 26);
            assert_eq!(crate::wasm_archive_db::version(&db).unwrap(), 25);
            assert_eq!(crate::portable_wasm_db::storage_version(&db).unwrap(), 24);
            assert_eq!(
                crate::portable_annotated_db::storage_version(&db).unwrap(),
                20
            );
            assert_eq!(
                crate::live_cuda_evidence_db::base_version(&db, 36).unwrap(),
                20
            );
            if equivalence {
                assert_eq!(
                    crate::live_cuda_evidence_db::base_version(&db, 35).unwrap(),
                    20
                );
            } else {
                assert!(crate::live_cuda_evidence_db::base_version(&db, 35).is_err());
            }
        }
    }

    #[test]
    fn supplied_closed_cpu_header_cannot_substitute_for_actual_header() {
        let db = armed(20);
        assert_eq!(raw(&db), 35);
        assert!(crate::live_cuda_evidence_db::base_version(&db, 36).is_err());
        assert_eq!(raw(&db), 35);
        assert_eq!(version(&db).unwrap(), 20);
    }
}
