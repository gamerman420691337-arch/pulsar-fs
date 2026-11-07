//! Reader barrier for inert portable CUDA facts, separate from active admission.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
const TABLE: &str = "portable_cuda_reader_state";
const CREATE: &str = "CREATE TABLE portable_cuda_reader_state(id INTEGER PRIMARY KEY CHECK(id=1),worker_fence INTEGER NOT NULL CHECK(worker_fence IN (0,1)),cuda_intents INTEGER NOT NULL CHECK(cuda_intents IN (0,1)),cuda_evidence INTEGER NOT NULL CHECK(cuda_evidence IN (0,1)))";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub worker_fence: bool,
    pub cuda_intents: bool,
    pub cuda_evidence: bool,
}
fn exists(db: &Connection, table: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )?)
}
fn present(db: &Connection) -> Result<State> {
    Ok(State {
        worker_fence: exists(db, "worker_fence_binding")?,
        cuda_intents: exists(db, "cuda_attempt_intents")?,
        cuda_evidence: exists(db, "cuda_candidate_evidence")?,
    })
}
fn read(db: &Connection) -> Result<State> {
    let sql: Option<String> = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[TABLE],|r|r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(CREATE),
        "portable CUDA reader-state layout differs"
    );
    let count: i64 = db.query_row("SELECT COUNT(*) FROM portable_cuda_reader_state", [], |r| {
        r.get(0)
    })?;
    ensure!(count == 1, "portable CUDA reader state must have one row");
    let row: (i64,i64,i64) = db.query_row("SELECT worker_fence,cuda_intents,cuda_evidence FROM portable_cuda_reader_state WHERE id=1 AND typeof(id)='integer' AND typeof(worker_fence)='integer' AND typeof(cuda_intents)='integer' AND typeof(cuda_evidence)='integer' AND worker_fence IN (0,1) AND cuda_intents IN (0,1) AND cuda_evidence IN (0,1)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    Ok(State {
        worker_fence: row.0 == 1,
        cuda_intents: row.1 == 1,
        cuda_evidence: row.2 == 1,
    })
}
/// This checks required-table presence, not their individual layouts or contents.
/// Their owning validators remain mandatory, including for active CUDA jobs.
pub(crate) fn validate(db: &Connection, version: i64) -> Result<Option<State>> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    if version < 18 {
        ensure!(
            !exists(db, TABLE)?,
            "portable CUDA reader state requires schema-18 barrier"
        );
        return Ok(None);
    }
    ensure!(
        matches!(version, 18..=20) && exists(db, TABLE)?,
        "missing or unknown portable CUDA reader state"
    );
    let state = read(db)?;
    ensure!(
        state == present(db)?,
        "portable CUDA required active-table presence differs"
    );
    if matches!(version, 19 | 20) {
        ensure!(
            state.worker_fence && state.cuda_intents && state.cuda_evidence,
            "schema-19/20 ownership requires all active ownership tables"
        );
    }
    Ok(Some(state))
}
/// Shares publication's transaction. Required active tables can be added later,
/// but this marker cannot be refreshed to forget an existing requirement.
pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_portable_cuda;")?;
    match arm_inner(tx) {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_portable_cuda;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch("ROLLBACK TO pulsar_portable_cuda; RELEASE pulsar_portable_cuda;")?;
            Err(error)
        }
    }
}
fn arm_inner(tx: &Transaction<'_>) -> Result<()> {
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = crate::live_cuda_evidence_db::base_version(tx, version)?;
    ensure!(
        (2..=20).contains(&version),
        "unsupported portable CUDA barrier writer"
    );
    let state = present(tx)?;
    if version >= 18 {
        let old = read(tx)?;
        ensure!(
            (!old.worker_fence || state.worker_fence)
                && (!old.cuda_intents || state.cuda_intents)
                && (!old.cuda_evidence || state.cuda_evidence),
            "portable CUDA cannot discard an active-table requirement"
        );
        tx.execute("UPDATE portable_cuda_reader_state SET worker_fence=?1,cuda_intents=?2,cuda_evidence=?3 WHERE id=1",params![state.worker_fence,state.cuda_intents,state.cuda_evidence])?;
    } else {
        ensure!(
            !exists(tx, TABLE)?,
            "portable CUDA marker exists below its barrier"
        );
        tx.execute_batch(CREATE)?;
        tx.execute(
            "INSERT INTO portable_cuda_reader_state VALUES(1,?1,?2,?3)",
            params![state.worker_fence, state.cuda_intents, state.cuda_evidence],
        )?;
        crate::portable_annotated_db::publish(tx, 18)?;
    }
    validate(tx, version.max(18))?;
    Ok(())
}
/// Detect both direct CUDA membership and format-10 ancestor declarations before
/// writable startup. SQL byte limits precede JSON parsing and Rust allocation.
pub(crate) fn validate_metadata(db: &Connection, version: i64) -> Result<()> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    validate(db, version)?;
    let mut cuda = false;
    if exists(db, "project_import_archives")? {
        let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM project_import_archives WHERE typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest))",[],|r|r.get(0))?;
        if version >= 18 {
            ensure!(!invalid, "portable archive metadata admission");
        }
        cuda=db.query_row("SELECT EXISTS(SELECT 1 FROM project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=10 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=10) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.generated_origins') e WHERE json_type(e.value,'$.cuda_evidence') IS NOT NULL AND json_type(e.value,'$.cuda_evidence')!='null') END)",[],|r|r.get(0))?;
    }
    if exists(db, "project_package_exports")? {
        let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM project_package_exports WHERE typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status))",[],|r|r.get(0))?;
        if version >= 18 {
            ensure!(!invalid, "portable export metadata admission");
        }
        cuda |= db.query_row("SELECT EXISTS(SELECT 1 FROM project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=10 END)",[],|r|r.get::<_,bool>(0))?;
    }
    ensure!(
        !cuda || matches!(version, 18..=20),
        "portable CUDA facts require schema-18 reader barrier"
    );
    Ok(())
}

/// Revalidate inert CUDA anchors from retained namespaces before writable
/// startup. Original request paths are never filesystem resolvers here.
pub(crate) fn validate_archived_bytes(
    db: &Connection,
    version: i64,
    store: &crate::project_package_import_storage::ImportStore,
    pool: &crate::resources::ResourcePool,
) -> Result<()> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    use pulsar_protocol::*;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    validate_metadata(db, version)?;
    if !matches!(version, 18..=20) || !exists(db, "project_import_archives")? {
        return Ok(());
    }
    let mut statement = db.prepare("SELECT CASE WHEN typeof(project)='text' AND length(CAST(project AS BLOB)) BETWEEN 1 AND 128 THEN project ELSE NULL END,manifest FROM project_import_archives WHERE json_extract(manifest,'$.format_version')>=10 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=10) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.generated_origins') e WHERE json_type(e.value,'$.cuda_evidence') IS NOT NULL AND json_type(e.value,'$.cuda_evidence')!='null')")?;
    let mut rows = statement.query([])?;
    let mut total = 0u64;
    let mut count = 0usize;
    while let Some(row) = rows.next()? {
        count += 1;
        ensure!(count <= 4096, "portable CUDA archive row capacity exceeded");
        let project: String = row.get(0)?;
        let raw: String = row.get(1)?;
        total = total
            .checked_add(raw.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("portable CUDA archive byte overflow"))?;
        ensure!(
            total <= MAX_PACKAGE_ORIGIN_MANIFEST_BYTES,
            "portable CUDA archive metadata exceeds aggregate bounds"
        );
        let _root_memory = pool.reserve_memory(raw.len() as u64 * 8 + 16 * 1024 * 1024)?;
        let root: PortableProjectManifest = serde_json::from_str(&raw)?;
        ensure!(
            root.origin_project_id.as_str() == project && root.canonical_bytes()? == raw.as_bytes(),
            "portable CUDA root archive binding differs"
        );
        let mut origins = BTreeMap::new();
        let mut holds = Vec::new();
        let mut origin_bytes = 0u64;
        for object in root
            .objects
            .iter()
            .filter(|o| o.roles.contains(&PackageObjectRole::ImportedManifest))
        {
            origin_bytes = origin_bytes
                .checked_add(object.byte_len)
                .ok_or_else(|| anyhow::anyhow!("portable CUDA origin byte overflow"))?;
            ensure!(
                origin_bytes <= MAX_PACKAGE_ORIGIN_MANIFEST_BYTES
                    && origins.len() < MAX_PACKAGE_IMPORTED_ORIGINS,
                "portable CUDA retained origin graph exceeds admission"
            );
            holds.push(
                pool.reserve_memory(
                    object
                        .byte_len
                        .checked_mul(8)
                        .and_then(|v| v.checked_add(16 * 1024 * 1024))
                        .ok_or_else(|| anyhow::anyhow!("portable CUDA origin memory overflow"))?,
                )?,
            );
            let bytes = store.read_retained_object(
                object,
                PackageObjectRole::ImportedManifest,
                MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
            )?;
            let original: PortableProjectManifest = serde_json::from_slice(&bytes)?;
            ensure!(
                original.canonical_bytes()? == bytes,
                "portable CUDA original manifest is not canonical"
            );
            ensure!(
                origins.insert(object.sha256.clone(), original).is_none(),
                "duplicate portable CUDA original namespace"
            );
        }
        crate::project_package_import_plan::validate_retained_origin_graph(&root, &origins)?;
        let reference = |identity: &ArtifactIdentity| PackageObjectRef {
            sha256: identity.sha256.clone(),
            byte_len: identity.byte_len,
        };
        let read = |reference: &PackageObjectRef,
                    role: PackageObjectRole,
                    limit: u64|
         -> Result<Vec<u8>> {
            let object = root
                .objects
                .iter()
                .find(|o| o.sha256 == reference.sha256 && o.byte_len == reference.byte_len)
                .ok_or_else(|| {
                    anyhow::anyhow!("portable CUDA anchor absent from retained catalog")
                })?;
            Ok(store.read_retained_object(object, role, limit)?)
        };
        for manifest in std::iter::once(&root).chain(origins.values()) {
            if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_ANNOTATED_MODELS {
                crate::authority::project_package_imports::validate_evidence(
                    manifest,
                    pool,
                    |reference| {
                        let object = root
                            .objects
                            .iter()
                            .find(|o| {
                                o.sha256 == reference.sha256 && o.byte_len == reference.byte_len
                            })
                            .ok_or_else(|| {
                                ProtocolError::invalid("annotated retained object absent")
                            })?;
                        let role = *object.roles.first().ok_or_else(|| {
                            ProtocolError::invalid("annotated retained role absent")
                        })?;
                        let limit = if object.roles.contains(&PackageObjectRole::AnnotatedModel) {
                            256 * 1024 * 1024
                        } else {
                            128 * 1024 * 1024
                        };
                        store.read_retained_object(object, role, limit)
                    },
                    &mut || Ok(()),
                )?;
            }
            for origin in &manifest.generated_origins {
                let request_bytes = read(
                    &origin.manifest,
                    PackageObjectRole::WorkerManifest,
                    1024 * 1024,
                )?;
                let request: WorkerRequest = serde_json::from_slice(&request_bytes)?;
                crate::authority::project_package_imports::checked_worker_scope(
                    manifest, origin, &request,
                )?;
                let completed_cuda =
                    request.cuda_execution.is_some() && origin.state == "completed";
                ensure!(
                    origin.cuda_evidence.is_some() == completed_cuda,
                    "portable CUDA retained completion membership differs"
                );
                if let Some(cuda) = &origin.cuda_evidence {
                    let _memory = pool.reserve_memory(128 * 1024 * 1024)?;
                    ensure!(
                        format!("{:x}", Sha256::digest(serde_json::to_vec(&request)?))
                            == cuda.request_sha256,
                        "portable CUDA retained canonical request differs"
                    );
                    let receipt = read(
                        &reference(&cuda.receipt),
                        PackageObjectRole::CudaExecutionReceipt,
                        65536,
                    )?;
                    let trace = read(
                        &reference(&cuda.trace),
                        PackageObjectRole::CudaNodeTrace,
                        8 * 1024 * 1024,
                    )?;
                    crate::cuda_result::verify_bytes(&request, &receipt, &trace)?;
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA user_version=11;").unwrap();
        db
    }
    #[test]
    fn archival_barrier_is_transactional_and_does_not_create_active_tables() {
        let mut db = fixture();
        {
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            assert_eq!(
                validate(&tx, 18).unwrap(),
                Some(State {
                    worker_fence: false,
                    cuda_intents: false,
                    cuda_evidence: false
                })
            );
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert!(!exists(&db, TABLE).unwrap());
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        validate_metadata(&db, 18).unwrap();
        assert!(validate(&db, 17).is_err());
    }
    #[test]
    fn activation_adds_requirements_and_missing_tables_cannot_be_forgotten() {
        let mut db = fixture();
        {
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
        }
        {
            let tx = db.transaction().unwrap();
            tx.execute_batch("CREATE TABLE worker_fence_binding(scope TEXT);")
                .unwrap();
            assert!(validate(&tx, 18).is_err());
            arm(&tx).unwrap();
            tx.commit().unwrap();
        }
        assert!(validate(&db, 18).unwrap().unwrap().worker_fence);
        let tx = db.transaction().unwrap();
        tx.execute_batch("DROP TABLE worker_fence_binding;")
            .unwrap();
        assert!(validate(&tx, 18).is_err());
        assert!(arm(&tx).is_err());
        drop(tx);
        assert!(validate(&db, 18).unwrap().unwrap().worker_fence);
    }
    #[test]
    fn legacy_invalid_metadata_does_not_arm_or_trigger_cuda_barrier() {
        let mut db = fixture();
        db.execute_batch("CREATE TABLE project_import_archives(manifest TEXT); INSERT INTO project_import_archives VALUES('not json'); CREATE TABLE project_package_exports(status TEXT); INSERT INTO project_package_exports VALUES('not json');").unwrap();
        validate_metadata(&db, 11).unwrap();
        let tx = db.transaction().unwrap();
        arm(&tx).unwrap();
        tx.commit().unwrap();
        assert!(validate_metadata(&db, 18).is_err());
    }
    #[test]
    fn metadata_downgrade_and_corrupt_marker_refuse() {
        let mut db = fixture();
        db.execute_batch("CREATE TABLE project_import_archives(manifest TEXT); INSERT INTO project_import_archives VALUES('{\"format_version\":10}');").unwrap();
        assert!(validate_metadata(&db, 11).is_err());
        {
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
        }
        validate_metadata(&db, 18).unwrap();
        db.execute_batch("DELETE FROM portable_cuda_reader_state;")
            .unwrap();
        assert!(validate_metadata(&db, 18).is_err());
    }
}
