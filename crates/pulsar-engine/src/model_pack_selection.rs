//! Durable host-owner choice, separate from inert supplier integrity receipts.
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use rusqlite::{Connection, OptionalExtension, Transaction};

pub(crate) fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS model_pack_selection(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL, report TEXT NOT NULL);")?;
    Ok(())
}
pub(crate) fn validate_layout(db: &Connection) -> Result<()> {
    let names = db
        .prepare("PRAGMA table_info(model_pack_selection)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        names == ["singleton", "revision", "report"],
        "model-pack selection layout mismatch"
    );
    status(db)?;
    Ok(())
}
pub(crate) fn status(db: &Connection) -> Result<ModelPackSelectionReport> {
    let invalid: i64 = db.query_row(
        "SELECT COUNT(*) FROM model_pack_selection WHERE singleton!=1",
        [],
        |r| r.get(0),
    )?;
    ensure!(invalid == 0, "invalid model-pack selection singleton");
    let row: Option<(i64, String)> = db.query_row(
        "SELECT revision,CASE WHEN length(CAST(report AS BLOB))<=1048576 THEN report ELSE NULL END FROM model_pack_selection WHERE singleton=1", [],
        |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let report = if let Some((revision, raw)) = row {
        let report: ModelPackSelectionReport = serde_json::from_str(&raw)?;
        ensure!(
            revision > 0 && revision as u64 == report.selection_revision,
            "selection storage fence mismatch"
        );
        report
    } else {
        ModelPackSelectionReport {
            schema_version: 1,
            catalog_binding: None,
            selection_revision: 0,
            choice: None,
            profile: None,
            selected_by: None,
            selection_request: None,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
        }
    };
    report.validate()?;
    ensure!(
        report.catalog_binding.is_none() || crate::inference_equivalence_reader::version(db)? >= 14,
        "catalog approval lacks reader barrier"
    );
    Ok(report)
}
#[cfg(test)]
pub(crate) fn select(
    tx: &Transaction<'_>,
    expected: u64,
    selected: Option<(String, DeclaredInferenceProfile)>,
    session: &SessionId,
    request: &RequestId,
) -> Result<ModelPackSelectionReport, ProtocolError> {
    select_bound(tx, expected, selected, None, session, request)
}
pub(crate) fn select_bound(
    tx: &Transaction<'_>,
    expected: u64,
    selected: Option<(String, DeclaredInferenceProfile)>,
    binding: Option<ModelCatalogBinding>,
    session: &SessionId,
    request: &RequestId,
) -> Result<ModelPackSelectionReport, ProtocolError> {
    let current =
        status(tx).map_err(|e| ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string()))?;
    if current.selection_revision != expected {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "model-pack selection changed; refresh the exact choice",
        ));
    }
    let revision = expected
        .checked_add(1)
        .filter(|r| *r <= i64::MAX as u64)
        .ok_or_else(|| ProtocolError::invalid("selection revision exhausted"))?;
    let (choice, profile) = match selected {
        Some((sha, profile)) => (
            Some(ModelPackChoice {
                manifest_sha256: sha,
                selection_revision: revision,
            }),
            Some(profile),
        ),
        None => (None, None),
    };
    let report = ModelPackSelectionReport {
        schema_version: if binding.is_some() { 2 } else { 1 },
        catalog_binding: binding.clone(),
        selection_revision: revision,
        native_execution_approved: choice.is_some(),
        choice,
        profile,
        selected_by: Some(session.clone()),
        selection_request: Some(request.clone()),
        rights_verified: false,
        compatible: false,
        qualified: false,
    };
    report.validate()?;
    let storage_error = |e: rusqlite::Error| ProtocolError::new(ErrorCode::Internal, e.to_string());
    if crate::inference_equivalence_reader::version(tx)
        .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?
        < 12
    {
        crate::inference_equivalence_reader::publish(tx, 12)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?;
    }
    if binding.is_some()
        && crate::inference_equivalence_reader::version(tx)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?
            < 14
    {
        crate::inference_equivalence_reader::publish(tx, 14)
            .map_err(|e| ProtocolError::new(ErrorCode::Internal, e.to_string()))?;
    }
    tx.execute("INSERT INTO model_pack_selection(singleton,revision,report) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,report=excluded.report",
        rusqlite::params![revision as i64, serde_json::to_string(&report).map_err(|e| ProtocolError::new(ErrorCode::Internal,e.to_string()))?]).map_err(storage_error)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_publication_preserves_equivalence_outer_reader() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        db.execute_batch("PRAGMA user_version=8;").unwrap();
        let tx = db.transaction().unwrap();
        crate::inference_equivalence_reader::arm(&tx).unwrap();
        let report = select(
            &tx,
            0,
            None,
            &SessionId::new("owner").unwrap(),
            &RequestId::new("clear-selection").unwrap(),
        )
        .unwrap();
        assert_eq!(report.selection_revision, 1);
        assert!(report.choice.is_none());
        assert!(!report.native_execution_approved && !report.qualified);
        assert_eq!(
            crate::inference_equivalence_reader::version(&tx).unwrap(),
            12
        );
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            35
        );
        tx.commit().unwrap();
        assert_eq!(status(&db).unwrap().selection_revision, 1);
    }
    #[test]
    fn catalog_binding_is_atomic_reader_fenced_and_explicitly_cleared_by_deactivation() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        db.execute_batch("PRAGMA user_version=13;").unwrap();
        let id = ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture-model").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: id.clone(),
            runtime: id,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 134217728,
            minimum_output_bytes: 1048576,
            cpu_threads: 2,
            cuda_resources: None,
        };
        let binding = ModelCatalogBinding {
            bootstrap_sha256: "b".repeat(64),
            catalog_revision: 2,
        };
        let session = SessionId::new("bound-owner").unwrap();
        let request = RequestId::new("bound-select").unwrap();
        {
            let tx = db.transaction().unwrap();
            let r = select_bound(
                &tx,
                0,
                Some(("c".repeat(64), profile.clone())),
                Some(binding.clone()),
                &session,
                &request,
            )
            .unwrap();
            assert_eq!(r.schema_version, 2);
            assert_eq!(r.catalog_binding, Some(binding.clone()));
        }
        assert_eq!(status(&db).unwrap().selection_revision, 0);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            13
        );
        let tx = db.transaction().unwrap();
        select_bound(
            &tx,
            0,
            Some(("c".repeat(64), profile)),
            Some(binding.clone()),
            &session,
            &request,
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(status(&db).unwrap().catalog_binding, Some(binding));
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        let tx = db.transaction().unwrap();
        let off = select(&tx, 1, None, &session, &request).unwrap();
        assert!(off.catalog_binding.is_none());
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        db.execute(
            "UPDATE model_pack_selection SET report=zeroblob(1048577)",
            [],
        )
        .unwrap();
        assert!(status(&db).is_err());
    }
    #[test]
    fn selection_changes_preserve_a_newer_pack_batch_reader_barrier() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        db.execute_batch("PRAGMA user_version=13;").unwrap();
        let tx = db.transaction().unwrap();
        select(
            &tx,
            0,
            None,
            &SessionId::new("fixture-owner").unwrap(),
            &RequestId::new("fixture-deactivation").unwrap(),
        )
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            13
        );
    }
    #[test]
    fn deactivation_is_revision_fenced_durable_and_advances_reader_barrier() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=11;").unwrap();
        initialize(&db).unwrap();
        assert_eq!(status(&db).unwrap().selection_revision, 0);
        let session = SessionId::new("owner-session").unwrap();
        let request = RequestId::new("deactivate-request").unwrap();
        let tx = db.transaction().unwrap();
        let report = select(&tx, 0, None, &session, &request).unwrap();
        assert_eq!(report.selection_revision, 1);
        assert!(!report.native_execution_approved);
        tx.commit().unwrap();
        validate_layout(&db).unwrap();
        assert_eq!(status(&db).unwrap().selected_by, Some(session.clone()));
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            12
        );
        let tx = db.transaction().unwrap();
        assert_eq!(
            select(&tx, 0, None, &session, &request).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        drop(tx);
        assert_eq!(status(&db).unwrap().selection_revision, 1);
        db.execute("UPDATE model_pack_selection SET revision=2", [])
            .unwrap();
        assert!(validate_layout(&db).is_err());
    }
}
