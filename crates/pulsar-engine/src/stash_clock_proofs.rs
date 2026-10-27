//! Private timing evidence retained before publication dispatch.
use super::*;
use std::io;
pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS stash_clock_proofs(session TEXT NOT NULL,request TEXT NOT NULL,proof TEXT NOT NULL,PRIMARY KEY(session,request),FOREIGN KEY(session,request) REFERENCES exports(session,request));")?;
    Ok(())
}
pub(super) fn validate_layout(db: &Connection) -> Result<()> {
    let mut stmt = db.prepare("PRAGMA table_info(stash_clock_proofs)")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let columns = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        columns
            == vec![
                ("session".into(), "TEXT".into(), 1, 1),
                ("request".into(), "TEXT".into(), 1, 2),
                ("proof".into(), "TEXT".into(), 1, 0)
            ],
        "timing proof database columns are incomplete; explicit recovery is required"
    );
    let mut stmt = db.prepare("PRAGMA foreign_key_list(stash_clock_proofs)")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let keys = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        keys == vec![
            (0, 0, "exports".into(), "session".into(), "session".into()),
            (0, 1, "exports".into(), "request".into(), "request".into())
        ],
        "timing proof database keys are incomplete; explicit recovery is required"
    );
    Ok(())
}
#[derive(Serialize)]
pub(super) struct Context<'a> {
    pub publication_request: &'a RequestId,
    pub source: &'a StashSourceCapture,
    pub project: &'a ProjectId,
    pub revision: RevisionId,
    pub candidate: &'a CandidateId,
    pub job: &'a JobId,
    pub original_candidate: &'a CandidateId,
    pub original_receipt: &'a ArtifactIdentity,
    pub original_program: &'a ArtifactIdentity,
    pub master_sha256: &'a str,
    pub script: &'a ArtifactIdentity,
    pub placement: &'a SourcePlacement,
    pub retimings: &'a [MotionRetimeReceipt],
    #[serde(skip)]
    pub piecewise: bool,
    pub action_ancestry: &'a stash_action_ancestry::NativeActionAncestry,
}
#[derive(Serialize)]
struct Proof<'a, C: Serialize> {
    schema_version: u32,
    clock: &'static str,
    quantization: &'static str,
    context: C,
    mappings: &'a [SourceClockActionMapping],
    reused_sample_clocks: u64,
    release_qualified: bool,
}
#[derive(Serialize)]
pub(super) struct ArchivedContext<'a> {
    pub publication_request: &'a RequestId,
    pub source: &'a StashSourceCapture,
    pub project: &'a ProjectId,
    pub revision: RevisionId,
    pub candidate: &'a CandidateId,
    /// Historical identity only; archived_context explicitly denies a live job.
    pub job: &'a JobId,
    pub master_sha256: &'a str,
    pub script: &'a ArtifactIdentity,
    pub archived_context: &'a ArchivedPublicationContext,
    pub native_retimings: &'a [MotionRetimeReceipt],
    pub archived_retimings: &'a [ArchivedMotionRetimeReceipt],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_ancestry: Option<&'a stash_action_ancestry::NativeActionAncestry>,
}
struct BoundedBytes {
    bytes: Vec<u8>,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > OUTPUT_LIMIT as usize)
        {
            return Err(io::Error::other("timing proof exceeds admission"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn prepare(
    engine: &Engine,
    context: Context<'_>,
    mapped: &SourceClockMotion,
) -> PResult<StashTimingProof> {
    let master = context.master_sha256.to_owned();
    let script_bytes = context.script.byte_len;
    let version = if context.piecewise { 6 } else { 4 };
    let proof=Proof {schema_version:version,clock:"elapsed from original first decoded source frame",quantization:"exact source rational -> floor nanoseconds; script nearest millisecond, timestamp collisions refused",context,mappings:&mapped.mappings,reused_sample_clocks:mapped.reused_sample_clocks,release_qualified:false};
    retain(
        engine,
        &proof,
        master,
        script_bytes,
        mapped.mappings.len() as u64,
        mapped.reused_sample_clocks,
        None,
    )
}
pub(super) fn prepare_archived(
    engine: &Engine,
    context: ArchivedContext<'_>,
    mapped: Option<&SourceClockMotion>,
) -> PResult<StashTimingProof> {
    context.archived_context.validate()?;
    let metadata = context.archived_context.clone();
    let master = context.master_sha256.to_owned();
    let bytes = context.script.byte_len;
    if metadata.sampled_clocks_verified != mapped.is_some() {
        return Err(ProtocolError::invalid(
            "archival clock proof mode differs from checked mappings",
        ));
    }
    let mappings = mapped.map_or(&[][..], |m| m.mappings.as_slice());
    let reused = mapped.map_or(0, |m| m.reused_sample_clocks);
    if mapped.is_some() != context.action_ancestry.is_some() {
        return Err(ProtocolError::invalid(
            "archival action ancestry differs from mapped proof mode",
        ));
    }
    let proof=Proof {schema_version:if context.action_ancestry.is_some_and(|a|a.carriers.len()>1){7}else if mapped.is_some(){5}else{3},clock:if mapped.is_some(){"elapsed from original first decoded source frame; retained sampled clocks"}else{"original unplaced elapsed-source convention; sampled clocks unavailable"},quantization:"placed: exact source rational -> floor nanoseconds -> nearest script millisecond; unplaced: original project-time convention; collisions refused",context,mappings,reused_sample_clocks:reused,release_qualified:false};
    retain(
        engine,
        &proof,
        master,
        bytes,
        mappings.len() as u64,
        reused,
        Some(metadata),
    )
}
fn retain(
    engine: &Engine,
    proof: &impl Serialize,
    master: String,
    script_bytes: u64,
    mapped_actions: u64,
    reused_sample_clocks: u64,
    archived_context: Option<ArchivedPublicationContext>,
) -> PResult<StashTimingProof> {
    let mut writer = BoundedBytes { bytes: Vec::new() };
    serde_json::to_writer(&mut writer, &proof).map_err(|_| {
        ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "timing proof serialization exceeds admitted bytes",
        )
    })?;
    let dir = engine.config.state_dir.join("source-clock-proofs");
    artifacts::private_directory(&dir).map_err(unavailable)?;
    let needed = engine
        .pool
        .reserved_storage()
        .checked_add(writer.bytes.len() as u64)
        .and_then(|n| n.checked_add(script_bytes))
        .and_then(|n| n.checked_add(16 * 1024 * 1024))
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "timing proof storage overflow",
            )
        })?;
    if fs2::available_space(&dir).map_err(unavailable)? < needed {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "insufficient timing proof storage",
        ));
    }
    let hash = format!("{:x}", Sha256::digest(&writer.bytes));
    let path = dir.join(format!("{hash}.json"));
    if path.try_exists().map_err(unavailable)? {
        let (found, size) = artifacts::digest_file(&path, OUTPUT_LIMIT).map_err(unavailable)?;
        if found != hash || size != writer.bytes.len() as u64 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "retained timing proof identity mismatch",
            ));
        }
    } else {
        artifacts::publish_export(&path, &writer.bytes).map_err(unavailable)?;
    }
    Ok(StashTimingProof {
        archived_context,
        identity: ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
            sha256: hash,
            byte_len: writer.bytes.len() as u64,
        },
        path,
        mapped_actions,
        reused_sample_clocks,
        master_sha256: master,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_reference_survives_pending_unknown_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("proofs.sqlite3");
        let db = Connection::open(&file).unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE exports(session TEXT,request TEXT,state TEXT,PRIMARY KEY(session,request));").unwrap();
        let mut db = db;
        let tx = db.transaction().unwrap();
        initialize(&tx).unwrap();
        assert!(tx
            .execute(
                "INSERT INTO stash_clock_proofs VALUES('s','r','retained-proof')",
                []
            )
            .is_err());
        tx.execute("INSERT INTO exports VALUES('s','r','pending')", [])
            .unwrap();
        tx.execute(
            "INSERT INTO stash_clock_proofs VALUES('s','r','retained-proof')",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
        db.execute(
            "UPDATE exports SET state='failed_or_unknown' WHERE request='r'",
            [],
        )
        .unwrap();
        drop(db);
        let db = Connection::open(file).unwrap();
        let row:(String,String)=db.query_row("SELECT e.state,p.proof FROM exports e JOIN stash_clock_proofs p USING(session,request)",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(row, ("failed_or_unknown".into(), "retained-proof".into()));
    }
    #[test]
    fn timing_serialization_refuses_over_admission_without_growing_buffer() {
        let mut writer = BoundedBytes {
            bytes: vec![0; OUTPUT_LIMIT as usize],
        };
        assert!(writer.write(&[1]).is_err());
        assert_eq!(writer.bytes.len(), OUTPUT_LIMIT as usize);
    }
}
