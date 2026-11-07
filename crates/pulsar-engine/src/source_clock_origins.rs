//! Bounded native generation origins. No authority, effects or master mutation.
use super::*;
use crate::resources::Reservation;
use crate::source_clock_receipts;
use std::fs::File;
use std::io::Read;

pub(super) struct NativeManifest {
    request: WorkerRequest,
    _memory: Reservation,
}
impl NativeManifest {
    pub(super) fn request(&self) -> &WorkerRequest {
        &self.request
    }
}
pub(super) struct NativeClocks {
    pub original_candidate: CandidateId,
    pub program: ArtifactIdentity,
    pub verified: crate::source_clock_receipts::VerifiedClocks,
    _memory: Reservation,
}
pub(super) struct NativeOrigin {
    pub original_candidate: CandidateId,
    pub program: ArtifactIdentity,
    pub verified: crate::source_clock_receipts::VerifiedOrigin,
    _memory: Reservation,
}
struct AdmittedProgramReader {
    file: File,
    _memory: Reservation,
}
impl Read for AdmittedProgramReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(bytes)
    }
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "native clock origin differs from the retained project/source/job/candidate",
    )
}

pub(super) fn manifest(
    db: &Connection,
    project: &ProjectId,
    source: &SourceVersionId,
    job: &JobId,
    pool: &ResourcePool,
) -> PResult<NativeManifest> {
    let raw: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 1048576 THEN manifest ELSE NULL END FROM jobs WHERE id=?1 AND project=?2 AND source=?3",
        params![job.as_str(),project.as_str(),source.as_str()], |r| r.get(0),
    ).optional().map_err(internal)?;
    let raw = raw.flatten().ok_or_else(mismatch)?;
    let memory = pool.reserve_memory(
        (raw.len() as u64)
            .checked_mul(8)
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(mismatch)?,
    )?;
    let request: WorkerRequest = decode(&raw)?;
    request.validate()?;
    if request.project_id != *project
        || request.source.source_version != *source
        || request.job_id != *job
        || !matches!(&request.operation, WorkerOperation::Generate { .. })
    {
        return Err(mismatch());
    }
    let bound: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE project=?1 AND version=?2 AND sha256=?3 AND bytes=?4)",
        params![project.as_str(),source.as_str(),request.source.identity.sha256,
            i64::try_from(request.source.identity.byte_len).map_err(|_| mismatch())?], |r| r.get(0),
    ).map_err(internal)?;
    if !bound {
        return Err(mismatch());
    }
    Ok(NativeManifest {
        request,
        _memory: memory,
    })
}

pub(super) fn clocks(
    db: &Connection,
    manifest: &NativeManifest,
    candidate: &CandidateId,
    pool: &ResourcePool,
) -> PResult<NativeClocks> {
    let origin = generation_origin(db, manifest, candidate, pool)?;
    Ok(NativeClocks {
        original_candidate: origin.original_candidate,
        program: origin.program,
        verified: crate::source_clock_receipts::VerifiedClocks {
            plan: origin.verified.plan.ok_or_else(mismatch)?,
            identity: origin.verified.identity,
        },
        _memory: origin._memory,
    })
}

pub(super) fn generation_origin(
    db: &Connection,
    manifest: &NativeManifest,
    candidate: &CandidateId,
    pool: &ResourcePool,
) -> PResult<NativeOrigin> {
    let origin = manifest.request();
    let original = stash_lineage::original_generation_candidate(
        db,
        &origin.project_id,
        candidate,
        &origin.job_id,
    )?;
    let row: (Option<String>, Option<String>) = db.query_row(
        "SELECT CASE WHEN typeof(lineage)='text' AND length(CAST(lineage AS BLOB)) BETWEEN 1 AND 1048576 THEN lineage ELSE NULL END,CASE WHEN typeof(program)='text' AND length(CAST(program AS BLOB))=71 THEN program ELSE NULL END FROM candidates WHERE id=?1 AND project=?2 AND job=?3",
        params![original.as_str(),origin.project_id.as_str(),origin.job_id.as_str()],
        |r| Ok((r.get(0)?,r.get(1)?)),
    ).map_err(internal)?;
    let raw = row.0.ok_or_else(mismatch)?;
    let _lineage_memory = pool.reserve_memory(
        (raw.len() as u64)
            .checked_mul(8)
            .and_then(|n| n.checked_add(8192 + 16 * 1024 * 8))
            .ok_or_else(mismatch)?,
    )?;
    let retained: Vec<ArtifactIdentity> = decode(&raw)?;
    let reference = row.1.ok_or_else(mismatch)?;
    let hash = reference.strip_prefix("motion:").ok_or_else(mismatch)?;
    let raw_descriptor: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN typeof(descriptor)='text' AND length(CAST(descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN descriptor ELSE NULL END FROM motion_objects WHERE sha256=?1",
        [hash], |r| r.get(0),
    ).optional().map_err(internal)?;
    let descriptor: ProgramDescriptor = decode(&raw_descriptor.flatten().ok_or_else(mismatch)?)?;
    descriptor.validate()?;
    if descriptor.sha256 != hash {
        return Err(mismatch());
    }
    let receipt =
        artifacts::open_regular(&origin.output_dir.join("receipt.json")).map_err(|_| mismatch())?;
    let size = receipt.metadata().map_err(|_| mismatch())?.len();
    if size == 0
        || size
            > origin
                .budget
                .output_bytes
                .min(source_clock_receipts::MAX_RECEIPT_BYTES)
    {
        return Err(mismatch());
    }
    let needed = size
        .checked_add(descriptor.byte_len)
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(mismatch)?;
    let memory = pool.reserve_memory(needed)?;
    // Authenticate the actual CAS motion too, not just its SQL descriptor.
    motion_state::store_for(db)?.read(&descriptor)?;
    let program = ArtifactIdentity {
        artifact_id: descriptor.artifact_id,
        sha256: descriptor.sha256,
        byte_len: descriptor.byte_len,
    };
    let verified = source_clock_receipts::load_origin_readers(
        origin,
        &retained,
        &program,
        receipt,
        size,
        |raw, _| {
            let file = artifacts::open_regular(&origin.output_dir.join("program.json"))
                .map_err(|_| mismatch())?;
            if file.metadata().map_err(|_| mismatch())?.len() != raw.byte_len {
                return Err(mismatch());
            }
            let memory = pool.reserve_memory(
                raw.byte_len
                    .checked_mul(8)
                    .and_then(|n| n.checked_add(8192))
                    .ok_or_else(mismatch)?,
            )?;
            Ok(Box::new(AdmittedProgramReader {
                file,
                _memory: memory,
            }) as Box<dyn Read>)
        },
    )?;
    Ok(NativeOrigin {
        original_candidate: original,
        program,
        verified,
        _memory: memory,
    })
}

#[cfg(test)]
pub(crate) fn verify_native_fixture(
    directory: &Path,
    request: &WorkerRequest,
    original: &MotionProgram,
    plan: &PlacedGenerationPlan,
    retained: &[ArtifactIdentity],
    receipt_identity: &ArtifactIdentity,
    canonical: &ArtifactIdentity,
) {
    // Native origins bind bounded SQL metadata and actual CAS bytes to the
    // same immutable producer whose receipt/sample clocks were verified above.
    let db = rusqlite::Connection::open(directory.join("clock-origins.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TABLE jobs(id TEXT,project TEXT,source TEXT,manifest TEXT);
        CREATE TABLE sources(project TEXT,version TEXT,sha256 TEXT,bytes INTEGER);
        CREATE TABLE candidates(id TEXT,project TEXT,job TEXT,lineage TEXT,program TEXT);
        CREATE TABLE candidate_rebases(candidate TEXT,parent TEXT);
        CREATE TABLE motion_objects(sha256 TEXT PRIMARY KEY,descriptor TEXT NOT NULL);",
    )
    .unwrap();
    let reference = motion_state::save(&db, original).unwrap();
    let candidate = CandidateId::new("native-clock-origin").unwrap();
    db.execute(
        "INSERT INTO sources VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            request.project_id.as_str(),
            request.source.source_version.as_str(),
            request.source.identity.sha256,
            request.source.identity.byte_len as i64
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO jobs VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            request.job_id.as_str(),
            request.project_id.as_str(),
            request.source.source_version.as_str(),
            serde_json::to_string(request).unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO candidates VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            candidate.as_str(),
            request.project_id.as_str(),
            request.job_id.as_str(),
            serde_json::to_string(retained).unwrap(),
            reference
        ],
    )
    .unwrap();
    let pool = crate::resources::ResourcePool::new(64 * 1024 * 1024, 1);
    let native_manifest = self::manifest(
        &db,
        &request.project_id,
        &request.source.source_version,
        &request.job_id,
        &pool,
    )
    .unwrap();
    {
        let clocks = self::clocks(&db, &native_manifest, &candidate, &pool).unwrap();
        assert_eq!(clocks.original_candidate, candidate);
        assert_eq!(clocks.program.sha256, canonical.sha256);
        assert_eq!(clocks.verified.plan, *plan);
        assert_eq!(&clocks.verified.identity, receipt_identity);
    }
    let too_small = crate::resources::ResourcePool::new(1024 * 1024, 1);
    assert_eq!(
        self::clocks(&db, &native_manifest, &candidate, &too_small)
            .err()
            .unwrap()
            .code,
        ErrorCode::ResourceExhausted
    );
    db.execute("UPDATE sources SET sha256=?1", ["0".repeat(64)])
        .unwrap();
    assert!(self::manifest(
        &db,
        &request.project_id,
        &request.source.source_version,
        &request.job_id,
        &pool
    )
    .is_err());
    db.execute(
        "UPDATE sources SET sha256=?1",
        [&request.source.identity.sha256],
    )
    .unwrap();
    db.execute("UPDATE jobs SET manifest=?1", [" ".repeat(1_048_577)])
        .unwrap();
    assert!(self::manifest(
        &db,
        &request.project_id,
        &request.source.source_version,
        &request.job_id,
        &pool
    )
    .is_err());
    db.execute(
        "UPDATE jobs SET manifest=?1",
        [serde_json::to_string(request).unwrap()],
    )
    .unwrap();
    let object = directory.join("motion-objects").join(&canonical.sha256);
    let backing = object.with_extension("saved");
    std::fs::rename(&object, &backing).unwrap();
    std::fs::write(&object, b"{}").unwrap();
    assert!(self::clocks(&db, &native_manifest, &candidate, &pool).is_err());
    std::fs::remove_file(&object).unwrap();
    std::fs::rename(&backing, &object).unwrap();
    assert!(self::clocks(&db, &native_manifest, &candidate, &pool).is_ok());
    let descriptor: String = db
        .query_row("SELECT descriptor FROM motion_objects", [], |r| r.get(0))
        .unwrap();
    db.execute(
        "UPDATE motion_objects SET descriptor=?1",
        [" ".repeat(16385)],
    )
    .unwrap();
    assert!(self::clocks(&db, &native_manifest, &candidate, &pool).is_err());
    db.execute("UPDATE motion_objects SET descriptor=?1", [descriptor])
        .unwrap();
    db.execute("UPDATE candidates SET lineage=?1", [" ".repeat(1048577)])
        .unwrap();
    assert!(self::clocks(&db, &native_manifest, &candidate, &pool).is_err());
    db.execute(
        "UPDATE candidates SET lineage=?1",
        [serde_json::to_string(retained).unwrap()],
    )
    .unwrap();
    assert!(self::clocks(&db, &native_manifest, &candidate, &pool).is_ok());
}
