//! Verified native values-only edit origins for source-clock composition.
use super::*;
use std::io::Read;

const RECEIPT_LIMIT: u64 = 128 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    format: String,
    kernel: String,
    lease_id: TransferId,
    project_id: ProjectId,
    base_revision: RevisionId,
    base_program: ProgramDescriptor,
    upload_sha256: String,
    upload_byte_len: u64,
    result_program: ProgramDescriptor,
    authored_provenance: pulsar_core::ProvenanceRef,
    authored_ranges: Vec<(Axis, TimeRange)>,
    inherited_ranges: Vec<(Axis, TimeRange)>,
    actor_id: String,
    label: String,
}

/// Reservations remain live with the reconciled motion and interval facts.
pub(super) struct VerifiedEditOrigin {
    pub base_revision: RevisionId,
    pub reconciled: pulsar_core::ReconciledEdit,
    _memory: Reservation,
    _receipt_memory: Reservation,
}

fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "native edit origin differs from its receipt, upload, base or candidate motion",
    )
}

fn read_exact_identity(path: &Path, identity: &ArtifactIdentity, limit: u64) -> PResult<Vec<u8>> {
    identity.validate().map_err(|_| mismatch())?;
    if identity.byte_len == 0 || identity.byte_len > limit {
        return Err(mismatch());
    }
    let file = artifacts::open_regular(path).map_err(|_| mismatch())?;
    if file.metadata().map_err(|_| mismatch())?.len() != identity.byte_len {
        return Err(mismatch());
    }
    let mut bytes = Vec::new();
    file.take(identity.byte_len.checked_add(1).ok_or_else(mismatch)?)
        .read_to_end(&mut bytes)
        .map_err(|_| mismatch())?;
    if bytes.len() as u64 != identity.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != identity.sha256
    {
        return Err(mismatch());
    }
    Ok(bytes)
}

fn descriptor(db: &Connection, reference: &str) -> PResult<ProgramDescriptor> {
    let digest = reference
        .strip_prefix("motion:")
        .filter(|s| s.len() == 64)
        .ok_or_else(mismatch)?;
    let raw: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN typeof(descriptor)='text' AND length(CAST(descriptor AS BLOB)) BETWEEN 1 AND 16384 THEN descriptor ELSE NULL END FROM motion_objects WHERE sha256=?1",
        [digest],|r|r.get(0)).optional().map_err(internal)?;
    let descriptor: ProgramDescriptor = decode(&raw.flatten().ok_or_else(mismatch)?)?;
    descriptor.validate().map_err(|_| mismatch())?;
    if descriptor.sha256 != digest {
        return Err(mismatch());
    }
    Ok(descriptor)
}

pub(super) fn load(
    db: &Connection,
    project: &ProjectId,
    candidate: &CandidateId,
    pool: &ResourcePool,
) -> PResult<VerifiedEditOrigin> {
    let row: Option<(Option<String>,Option<String>,i64,i64)> = db.query_row(
        "SELECT CASE WHEN typeof(e.receipt)='text' AND length(CAST(e.receipt AS BLOB)) BETWEEN 1 AND 1024 THEN e.receipt ELSE NULL END,CASE WHEN typeof(c.program)='text' AND length(CAST(c.program AS BLOB))=71 THEN c.program ELSE NULL END,e.authored_count,e.inherited_count FROM edit_candidate_lineage e JOIN candidates c ON c.id=e.candidate WHERE e.candidate=?1 AND c.project=?2 AND c.job IS NULL",
        params![candidate.as_str(),project.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
        .optional().map_err(internal)?;
    let (identity, reference, authored_count, inherited_count) = row.ok_or_else(mismatch)?;
    let identity = identity.ok_or_else(mismatch)?;
    let _metadata_memory = pool.reserve_memory(256 * 1024)?;
    let receipt_identity: ArtifactIdentity = decode(&identity)?;
    receipt_identity.validate().map_err(|_| mismatch())?;
    if receipt_identity.artifact_id.as_str() != format!("edit-receipt.{}", receipt_identity.sha256)
        || receipt_identity.byte_len == 0
        || receipt_identity.byte_len > RECEIPT_LIMIT
    {
        return Err(mismatch());
    }
    let receipt_memory = pool.reserve_memory(
        receipt_identity
            .byte_len
            .checked_mul(8)
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(mismatch)?,
    )?;
    let root = Path::new(db.path().ok_or_else(mismatch)?)
        .parent()
        .ok_or_else(mismatch)?;
    let bytes = read_exact_identity(
        &root.join("edit-receipts").join(&receipt_identity.sha256),
        &receipt_identity,
        RECEIPT_LIMIT,
    )?;
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| mismatch())?;
    receipt.base_program.validate().map_err(|_| mismatch())?;
    receipt.result_program.validate().map_err(|_| mismatch())?;
    if receipt.format != "pulsar-edit-receipt-v1"
        || receipt.kernel != pulsar_core::EDIT_VALUES_KERNEL_VERSION
        || receipt.project_id != *project
        || receipt.actor_id.is_empty()
        || receipt.actor_id.len() > 256
        || receipt.label.len() > 256
        || i64::try_from(receipt.authored_ranges.len()).ok() != Some(authored_count)
        || i64::try_from(receipt.inherited_ranges.len()).ok() != Some(inherited_count)
        || receipt.authored_provenance.as_str() != format!("edit:{}", receipt.lease_id.as_str())
    {
        return Err(mismatch());
    }
    let binding:Option<(i64,Option<String>)> = db.query_row(
        "SELECT base_revision,CASE WHEN typeof(session)='text' AND length(CAST(session AS BLOB)) BETWEEN 1 AND 128 THEN session ELSE NULL END FROM edit_upload_receipts WHERE lease_id=?1 AND project=?2 AND receipt=?3",
        params![receipt.lease_id.as_str(),project.as_str(),identity],|r|Ok((r.get(0)?,r.get(1)?)))
        .optional().map_err(internal)?;
    let (base, session) = binding.ok_or_else(mismatch)?;
    let session = SessionId::new(session.ok_or_else(mismatch)?).map_err(|_| mismatch())?;
    let recorded_actor:Option<Option<String>>=db.query_row(
        "SELECT CASE WHEN typeof(actor)='text' AND length(CAST(actor AS BLOB)) BETWEEN 1 AND 256 THEN actor ELSE NULL END FROM sessions WHERE id=?1",
        [session.as_str()],|r|r.get(0)).optional().map_err(internal)?;
    if base != rev_sql(receipt.base_revision)?
        || recorded_actor.flatten().as_deref() != Some(receipt.actor_id.as_str())
    {
        return Err(mismatch());
    }
    let base_ref: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN typeof(program)='text' AND length(CAST(program AS BLOB))=71 THEN program ELSE NULL END FROM revisions WHERE project=?1 AND revision=?2",
        params![project.as_str(),base],|r|r.get(0)).optional().map_err(internal)?;
    let before = descriptor(db, &base_ref.flatten().ok_or_else(mismatch)?)?;
    let after = descriptor(db, &reference.ok_or_else(mismatch)?)?;
    if before != receipt.base_program || after != receipt.result_program {
        return Err(mismatch());
    }
    let input_identity = ArtifactIdentity {
        artifact_id: ArtifactId::new(format!("edit-input.{}", receipt.upload_sha256))
            .map_err(|_| mismatch())?,
        sha256: receipt.upload_sha256.clone(),
        byte_len: receipt.upload_byte_len,
    };
    input_identity.validate().map_err(|_| mismatch())?;
    if input_identity.byte_len == 0
        || input_identity.byte_len > crate::motion_artifacts::MAX_PROGRAM_BYTES
    {
        return Err(mismatch());
    }
    let needed = input_identity
        .byte_len
        .checked_add(before.byte_len)
        .and_then(|n| n.checked_add(after.byte_len))
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(mismatch)?;
    let memory = pool.reserve_memory(needed)?;
    let input = read_exact_identity(
        &root.join("edit-inputs").join(&input_identity.sha256),
        &input_identity,
        crate::motion_artifacts::MAX_PROGRAM_BYTES,
    )?;
    let values: pulsar_core::EditValuesProgram =
        serde_json::from_slice(&input).map_err(|_| mismatch())?;
    let store = motion_state::store_for(db)?;
    let original = store.read(&before).map_err(|_| mismatch())?;
    let result = store.read(&after).map_err(|_| mismatch())?;
    let reconciled =
        pulsar_core::reconcile_edit_values(&original, &values, &receipt.authored_provenance)
            .map_err(|_| mismatch())?;
    if reconciled.program != result
        || reconciled.authored_ranges != receipt.authored_ranges
        || reconciled.inherited_ranges != receipt.inherited_ranges
    {
        return Err(mismatch());
    }
    Ok(VerifiedEditOrigin {
        base_revision: receipt.base_revision,
        reconciled,
        _memory: memory,
        _receipt_memory: receipt_memory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Connection, ProjectId, CandidateId) {
        let root = tempfile::tempdir().unwrap();
        let db = Connection::open(root.path().join("projects.sqlite3")).unwrap();
        db.execute_batch("CREATE TABLE sessions(id TEXT,actor TEXT);INSERT INTO sessions VALUES('session','author');
            CREATE TABLE revisions(project TEXT,revision INTEGER,program TEXT);
            CREATE TABLE candidates(id TEXT,project TEXT,program TEXT,job TEXT);
            CREATE TABLE motion_objects(sha256 TEXT PRIMARY KEY,descriptor TEXT);
            CREATE TABLE edit_candidate_lineage(candidate TEXT,receipt TEXT,authored_count INTEGER,inherited_count INTEGER);
            CREATE TABLE edit_upload_receipts(lease_id TEXT,project TEXT,receipt TEXT,base_revision INTEGER,session TEXT);").unwrap();
        let before = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            [0, 10, 20]
                .into_iter()
                .map(|t| {
                    MotionAction::new(
                        ProjectTime::from_nanos(t),
                        NormalizedPosition::new(0.2).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()])
        .unwrap();
        let values:pulsar_core::EditValuesProgram=serde_json::from_value(serde_json::json!({"tracks":[{"axis":"stroke","actions":[{"time":0,"position":0.4},{"time":10,"position":0.2},{"time":20,"position":0.2}],"gaps":[]}]})).unwrap();
        let provenance = pulsar_core::ProvenanceRef::new("edit:lease").unwrap();
        let reconciled = pulsar_core::reconcile_edit_values(&before, &values, &provenance).unwrap();
        let store = motion_state::store_for(&db).unwrap();
        let base = store.publish(&before).unwrap();
        let after = store.publish(&reconciled.program).unwrap();
        let base_ref = motion_state::register(&db, &base).unwrap();
        let result_ref = motion_state::register(&db, &after).unwrap();
        db.execute("INSERT INTO revisions VALUES('project',1,?1)", [base_ref])
            .unwrap();
        db.execute(
            "INSERT INTO candidates VALUES('edited','project',?1,NULL)",
            [result_ref],
        )
        .unwrap();
        let input = serde_json::to_vec(&values).unwrap();
        let input_sha = format!("{:x}", Sha256::digest(&input));
        fs::create_dir(root.path().join("edit-inputs")).unwrap();
        fs::write(root.path().join("edit-inputs").join(&input_sha), &input).unwrap();
        let raw=serde_json::to_vec(&serde_json::json!({"format":"pulsar-edit-receipt-v1","kernel":pulsar_core::EDIT_VALUES_KERNEL_VERSION,"lease_id":"lease","project_id":"project","base_revision":1,"base_program":base,"upload_sha256":input_sha,"upload_byte_len":input.len(),"result_program":after,"authored_provenance":provenance,"authored_ranges":reconciled.authored_ranges,"inherited_ranges":reconciled.inherited_ranges,"actor_id":"author","label":"fixture"})).unwrap();
        let sha = format!("{:x}", Sha256::digest(&raw));
        fs::create_dir(root.path().join("edit-receipts")).unwrap();
        fs::write(root.path().join("edit-receipts").join(&sha), &raw).unwrap();
        let id = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-receipt.{sha}")).unwrap(),
            sha256: sha,
            byte_len: raw.len() as u64,
        };
        let id = encode(&id).unwrap();
        db.execute(
            "INSERT INTO edit_candidate_lineage VALUES('edited',?1,?2,?3)",
            params![
                id,
                reconciled.authored_ranges.len() as i64,
                reconciled.inherited_ranges.len() as i64
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO edit_upload_receipts VALUES('lease','project',?1,1,'session')",
            [id],
        )
        .unwrap();
        (
            root,
            db,
            ProjectId::new("project").unwrap(),
            CandidateId::new("edited").unwrap(),
        )
    }
    #[test]
    fn native_edit_origin_replays_values_and_refuses_low_memory_or_metadata_changes() {
        let (_root, db, p, c) = fixture();
        let pool = ResourcePool::new(128 * 1024 * 1024, 1);
        let origin = load(&db, &p, &c, &pool).unwrap();
        assert_eq!(origin.base_revision, RevisionId::new(1));
        assert_eq!(
            origin
                .reconciled
                .program
                .track(Axis::Stroke)
                .unwrap()
                .actions()[0]
                .position()
                .value(),
            0.4
        );
        drop(origin);
        assert_eq!(
            load(&db, &p, &c, &ResourcePool::new(1024, 1))
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceExhausted
        );
        db.execute(
            "UPDATE edit_candidate_lineage SET authored_count=authored_count+1",
            [],
        )
        .unwrap();
        assert!(load(&db, &p, &c, &pool).is_err());
        db.execute(
            "UPDATE edit_candidate_lineage SET receipt=?1",
            [" ".repeat(1025)],
        )
        .unwrap();
        assert!(load(&db, &p, &c, &pool).is_err());
    }
    #[test]
    fn rehashed_interval_claims_do_not_replace_edit_recomputation() {
        let (root, db, p, c) = fixture();
        let pool = ResourcePool::new(128 * 1024 * 1024, 1);
        let original: String = db
            .query_row("SELECT receipt FROM edit_candidate_lineage", [], |r| {
                r.get(0)
            })
            .unwrap();
        let id: ArtifactIdentity = decode(&original).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join("edit-receipts").join(id.sha256)).unwrap(),
        )
        .unwrap();
        value["authored_ranges"][0][1]["start"] = serde_json::json!(1);
        let raw = serde_json::to_vec(&value).unwrap();
        let sha = format!("{:x}", Sha256::digest(&raw));
        fs::write(root.path().join("edit-receipts").join(&sha), &raw).unwrap();
        let forged = encode(&ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-receipt.{sha}")).unwrap(),
            sha256: sha,
            byte_len: raw.len() as u64,
        })
        .unwrap();
        db.execute("UPDATE edit_candidate_lineage SET receipt=?1", [&forged])
            .unwrap();
        db.execute("UPDATE edit_upload_receipts SET receipt=?1", [&forged])
            .unwrap();
        assert_eq!(
            load(&db, &p, &c, &pool).err().unwrap().code,
            ErrorCode::DependencyMismatch
        );
        db.execute("UPDATE edit_candidate_lineage SET receipt=?1", [&original])
            .unwrap();
        db.execute("UPDATE edit_upload_receipts SET receipt=?1", [&original])
            .unwrap();
        assert!(load(&db, &p, &c, &pool).is_ok());
    }
}
