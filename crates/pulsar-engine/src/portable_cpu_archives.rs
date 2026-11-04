//! Bounded archival CPU-image correspondence and conditional schema-39 reader barrier.
//! This module never opens a runtime path, adopts an owner policy, or creates
//! execution or acceptance authority.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::collections::{BTreeMap, BTreeSet};

type PResult<T> = std::result::Result<T, ProtocolError>;
type ImageKey = (String, String, u64);
const MAX_INDEX_BYTES: usize = 128 * 1024 * 1024;
const READER_NAME: &str = "portable_cpu_reader";
const READER_MARKER: &str = "CREATE TABLE portable_cpu_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 2 AND 38))";
const OUTER_VERSION: i64 = 39;

fn header(db: &Connection) -> Result<(i64, bool)> {
    let raw: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    ensure!(
        raw == 0 || (2..=OUTER_VERSION).contains(&raw),
        "unknown portable CPU reader version"
    );
    let temporary: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM temp.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    ensure!(!temporary, "portable CPU reader temporary object differs");
    let objects: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE OR tbl_name=?1 COLLATE NOCASE LIMIT 2)",
        [READER_NAME],
        |r| r.get(0),
    )?;
    if raw != OUTER_VERSION {
        ensure!(
            objects == 0,
            "portable CPU reader barrier downgraded or misplaced"
        );
        return Ok((raw, false));
    }
    ensure!(
        objects == 1,
        "portable CPU reader marker missing or associated objects differ"
    );
    let layout: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1 AND tbl_name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))=?2 AND sql=?3)",
        rusqlite::params![READER_NAME, READER_MARKER.len() as i64, READER_MARKER],
        |r| r.get(0),
    )?;
    ensure!(layout, "portable CPU reader layout differs");
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM main.portable_cpu_reader LIMIT 2)",
        [],
        |r| r.get(0),
    )?;
    ensure!(count == 1, "portable CPU reader singleton differs");
    let underlying: Option<i64> = db
        .query_row(
            "SELECT underlying_version FROM main.portable_cpu_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 2 AND 38",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok((
        underlying.context("portable CPU reader scalar admission differs")?,
        true,
    ))
}

pub(crate) fn version(db: &Connection) -> Result<i64> {
    Ok(header(db)?.0)
}

pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let (old, armed) = header(db)?;
    ensure!(
        (2..=38).contains(&new_version) && new_version >= old,
        "portable CPU reader publication would downgrade or exceed supported storage"
    );
    if new_version == old {
        return Ok(());
    }
    if armed {
        let changed = db.execute(
            "UPDATE main.portable_cpu_reader SET underlying_version=?1 WHERE singleton=1 AND underlying_version=?2",
            rusqlite::params![new_version, old],
        )?;
        ensure!(
            changed == 1,
            "portable CPU reader publication lost its retained header"
        );
    } else {
        db.pragma_update(
            Some(rusqlite::DatabaseName::Main),
            "user_version",
            new_version,
        )?;
    }
    Ok(())
}

pub(crate) fn arm(tx: &Transaction<'_>) -> Result<()> {
    let (old, armed) = header(tx)?;
    ensure!(
        (2..=38).contains(&old),
        "portable CPU reader requires supported initialized storage"
    );
    if !armed {
        tx.execute_batch(READER_MARKER)?;
        tx.execute(
            "INSERT INTO main.portable_cpu_reader(singleton,underlying_version) VALUES(1,?1)",
            [old],
        )?;
        tx.pragma_update(
            Some(rusqlite::DatabaseName::Main),
            "user_version",
            OUTER_VERSION,
        )?;
    }
    ensure!(
        header(tx)? == (old, true),
        "portable CPU reader arming did not preserve storage"
    );
    Ok(())
}

/// Detect stripped format-15 portable CPU reader markers before any recovery,
/// using SQL byte admission before JSON parsing.
pub(crate) fn validate_metadata(db: &Connection) -> Result<()> {
    let (_, armed) = header(db)?;
    for table in ["project_import_archives", "project_package_exports"] {
        let present: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !present {
            continue;
        }
        let sql = if table == "project_import_archives" {
            "SELECT EXISTS(SELECT 1 FROM main.project_import_archives WHERE CASE WHEN typeof(manifest)!='text' OR length(CAST(manifest AS BLOB)) NOT BETWEEN 1 AND 16777216 OR NOT json_valid(manifest) THEN 0 ELSE json_extract(manifest,'$.format_version')>=15 OR EXISTS(SELECT 1 FROM json_each(manifest,'$.imported_origins') e WHERE json_extract(e.value,'$.container.format_version')>=15) OR EXISTS(SELECT 1 FROM json_each(manifest,'$.generated_origins') e WHERE json_type(e.value,'$.cpu_runtime') IS NOT NULL AND json_type(e.value,'$.cpu_runtime')!='null') END)"
        } else {
            "SELECT EXISTS(SELECT 1 FROM main.project_package_exports WHERE CASE WHEN typeof(status)!='text' OR length(CAST(status AS BLOB)) NOT BETWEEN 1 AND 1048576 OR NOT json_valid(status) THEN 0 ELSE json_extract(status,'$.artifact.format_version')>=15 END)"
        };
        let required: bool = db.query_row(sql, [], |r| r.get(0))?;
        ensure!(
            !required || armed,
            "portable CPU metadata requires its schema-39 barrier"
        );
    }
    Ok(())
}

pub(crate) fn require_no_fallback(dependencies: &[ArtifactIdentity]) -> PResult<()> {
    if dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
    }) {
        return Err(ProtocolError::unsupported(
            "portable inference fallback history is not implemented",
        ));
    }
    Ok(())
}

fn has_cpu(dependencies: &[ArtifactIdentity]) -> bool {
    dependencies.iter().any(|identity| {
        identity
            .artifact_id
            .as_str()
            .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
    })
}

pub(crate) fn check_request(
    origin: &PackageGeneratedOrigin,
    request: &WorkerRequest,
) -> PResult<()> {
    require_no_fallback(&request.dependencies)?;
    match (
        origin.cpu_runtime.as_deref(),
        has_cpu(&request.dependencies),
    ) {
        (None, false) => Ok(()),
        (Some(image), true) => crate::cpu_runtime_db::check_image_declaration(request, image)
            .map_err(|_| {
                ProtocolError::invalid(
                    "archived CPU image differs from its original worker manifest",
                )
            }),
        (None, true) => crate::project_package_import_plan::require_portable_cpu_dependencies(
            &request.dependencies,
        ),
        (Some(_), false) => Err(ProtocolError::invalid(
            "archived CPU image membership is missing or unexpected",
        )),
    }
}

fn image_key(identity: &ArtifactIdentity) -> ImageKey {
    (
        identity.artifact_id.to_string(),
        identity.sha256.clone(),
        identity.byte_len,
    )
}

/// Callers already hold the package's admitted metadata memory. The additional
/// index stays below128MiB and never retains image bodies or worker requests.
#[derive(Default)]
pub(crate) struct CpuArchiveIndex {
    images: BTreeSet<ImageKey>,
    candidate_tags: BTreeSet<ImageKey>,
    declared_workers: BTreeSet<(String, u64)>,
    entries: usize,
    bytes: usize,
}
impl CpuArchiveIndex {
    fn charge(&mut self, bytes: usize) -> PResult<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= MAX_INDEX_BYTES)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "portable CPU correspondence index exceeds metadata admission",
                )
            })?;
        Ok(())
    }
    pub(crate) fn observe_manifest(&mut self, manifest: &PortableProjectManifest) -> PResult<()> {
        self.entries = self
            .entries
            .checked_add(manifest.candidates.len())
            .and_then(|n| n.checked_add(manifest.generated_origins.len()))
            .filter(|n| *n <= MAX_PACKAGE_ORIGIN_ENTRIES)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "portable CPU origin graph exceeds entry admission",
                )
            })?;
        for origin in &manifest.generated_origins {
            if let Some(image) = &origin.cpu_runtime {
                let identity = image.identity()?;
                if identity.byte_len > 128 * 1024 || origin.cuda_evidence.is_some() {
                    return Err(ProtocolError::invalid("invalid portable CPU image origin"));
                }
                let key = image_key(&identity);
                if !self.images.contains(&key) {
                    self.charge(384)?;
                    self.images.insert(key);
                }
                let worker = (origin.manifest.sha256.clone(), origin.manifest.byte_len);
                if !self.declared_workers.contains(&worker) {
                    self.charge(128)?;
                    self.declared_workers.insert(worker);
                }
            }
        }
        for candidate in &manifest.candidates {
            require_no_fallback(&candidate.lineage)?;
            for identity in candidate.lineage.iter().filter(|identity| {
                identity
                    .artifact_id
                    .as_str()
                    .starts_with(CPU_RUNTIME_IMAGE_ID_PREFIX)
            }) {
                identity.validate()?;
                let key = image_key(identity);
                if !self.candidate_tags.contains(&key) {
                    self.charge(384)?;
                    self.candidate_tags.insert(key);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn check_worker(
        &self,
        reference: &PackageObjectRef,
        request: &WorkerRequest,
    ) -> PResult<()> {
        require_no_fallback(&request.dependencies)?;
        if has_cpu(&request.dependencies)
            && !self
                .declared_workers
                .contains(&(reference.sha256.clone(), reference.byte_len))
        {
            return Err(ProtocolError::unsupported(
                "portable CPU worker lacks its exact declared origin",
            ));
        }
        Ok(())
    }
    pub(crate) fn finish(&self) -> PResult<()> {
        if !self.candidate_tags.is_subset(&self.images) {
            return Err(ProtocolError::unsupported(
                "portable CPU lineage lacks its exact image declaration",
            ));
        }
        Ok(())
    }
}

pub(crate) fn check_graph(
    manifest: &PortableProjectManifest,
    origins: &BTreeMap<String, PortableProjectManifest>,
) -> PResult<()> {
    let mut index = CpuArchiveIndex::default();
    for node in std::iter::once(manifest).chain(origins.values()) {
        index.observe_manifest(node)?;
    }
    index.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_reader39_preserves_underlying_versions_and_refuses_stripped_markers() {
        for base in 2..=38 {
            let mut db = Connection::open_in_memory().unwrap();
            db.pragma_update(Some(rusqlite::DatabaseName::Main), "user_version", base)
                .unwrap();
            assert_eq!(version(&db).unwrap(), base);
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            arm(&tx).unwrap();
            assert_eq!(version(&tx).unwrap(), base);
            assert_eq!(
                tx.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                39
            );
            tx.commit().unwrap();
            assert_eq!(version(&db).unwrap(), base);
            publish(&db, 38).unwrap();
            assert_eq!(version(&db).unwrap(), 38);
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                39
            );
        }
        for (table, row) in [
            ("project_import_archives", r#"{"format_version":15}"#),
            (
                "project_import_archives",
                r#"{"format_version":1,"imported_origins":[{"container":{"format_version":15}}]}"#,
            ),
            (
                "project_import_archives",
                r#"{"format_version":1,"generated_origins":[{"cpu_runtime":{}}]}"#,
            ),
            (
                "project_package_exports",
                r#"{"artifact":{"format_version":15}}"#,
            ),
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch(&format!(
                "PRAGMA user_version=25;CREATE TABLE {table}({col} TEXT);INSERT INTO {table} VALUES(?1)",
                col = if table == "project_import_archives" {
                    "manifest"
                } else {
                    "status"
                }
            ))
            .unwrap();
            db.execute(&format!("DELETE FROM {table}"), []).unwrap();
            db.execute(&format!("INSERT INTO {table} VALUES(?1)"), [row])
                .unwrap();
            assert!(validate_metadata(&db).is_err());
            let tx = db.transaction().unwrap();
            arm(&tx).unwrap();
            tx.commit().unwrap();
            validate_metadata(&db).unwrap();
            db.execute_batch("DROP TABLE portable_cpu_reader;PRAGMA user_version=25;")
                .unwrap();
            assert!(validate_metadata(&db).is_err());
        }
    }
}
