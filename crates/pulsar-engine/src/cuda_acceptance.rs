//! Durable candidate/evidence membership. This does not authorize GPU launch.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

type PortableCudaMembers = (
    usize,
    std::collections::BTreeMap<AttemptId, (PackageCudaEvidence, WorkerCudaArtifacts)>,
);

const TABLE: &str = "cuda_candidate_evidence";
pub(crate) const CREATE: &str = "CREATE TABLE cuda_candidate_evidence(candidate TEXT PRIMARY KEY NOT NULL REFERENCES candidates(id),attempt TEXT UNIQUE NOT NULL REFERENCES cuda_attempt_intents(attempt),request_sha256 TEXT NOT NULL,artifacts TEXT NOT NULL)";
const MAX_RECORD: usize = 32768;
fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
/// Portable accepted-CUDA closure is still unimplemented. Never silently omit
/// durable role membership from a project archive.
fn layout(db: &Connection) -> Result<()> {
    let sql: Option<String> = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[TABLE],|r|r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(CREATE),
        "CUDA candidate evidence layout differs"
    );
    Ok(())
}
fn capacity(db: &Connection) -> Result<(i64, i64)> {
    let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM cuda_candidate_evidence WHERE typeof(candidate)!='text' OR length(CAST(candidate AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(request_sha256)!='text' OR length(CAST(request_sha256 AS BLOB))!=64 OR typeof(artifacts)!='text' OR length(CAST(artifacts AS BLOB)) NOT BETWEEN 1 AND 32768)",[],|r|r.get(0))?;
    ensure!(!invalid, "CUDA candidate evidence scalar admission");
    let (count,total):(i64,i64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(artifacts AS BLOB))),0) FROM cuda_candidate_evidence",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        count <= 4096 && total <= 64 * 1024 * 1024,
        "CUDA candidate evidence capacity exceeded"
    );
    Ok((count, total))
}
fn hash(request: &WorkerRequest) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(request)?)
    ))
}
/// Read only the selected project's accepted role mapping. Historical paths
/// remain parent-owned capture inputs, never destination import resolvers.
pub(crate) fn portable_members(
    db: &Connection,
    project: &ProjectId,
) -> Result<PortableCudaMembers> {
    let mut members = std::collections::BTreeMap::new();
    if !exists(db)? {
        return Ok((0, members));
    }
    layout(db)?;
    capacity(db)?;
    let mut bytes = 0usize;
    let mut statement = db.prepare("SELECT e.candidate,e.attempt,e.request_sha256,e.artifacts FROM cuda_candidate_evidence e JOIN candidates c ON c.id=e.candidate WHERE c.project=?1 ORDER BY e.attempt")?;
    let mut rows = statement.query([project.as_str()])?;
    while let Some(row) = rows.next()? {
        let candidate: String = row.get(0)?;
        let attempt: String = row.get(1)?;
        let digest: String = row.get(2)?;
        let raw: String = row.get(3)?;
        bytes = bytes
            .checked_add(candidate.len() + attempt.len() + digest.len() + raw.len() + 256)
            .context("portable CUDA metadata overflow")?;
        ensure!(
            members.len() < 4096 && bytes <= 64 * 1024 * 1024 + 4096 * 512,
            "portable CUDA metadata exceeds admission"
        );
        check_member(db, &candidate, &attempt, &digest, &raw)?;
        let artifacts: WorkerCudaArtifacts = serde_json::from_str(&raw)?;
        let evidence = PackageCudaEvidence {
            accepted_candidate: CandidateId::new(candidate)?,
            request_sha256: digest,
            receipt: artifacts.receipt.identity.clone(),
            trace: artifacts.trace.identity.clone(),
        };
        evidence.validate()?;
        ensure!(
            members
                .insert(AttemptId::new(attempt)?, (evidence, artifacts))
                .is_none(),
            "duplicate portable CUDA attempt membership"
        );
    }
    Ok((bytes, members))
}
fn check_member(
    db: &Connection,
    candidate: &str,
    attempt: &str,
    digest: &str,
    raw: &str,
) -> Result<()> {
    ensure!(
        raw.len() <= MAX_RECORD,
        "CUDA evidence record byte admission"
    );
    let evidence: WorkerCudaArtifacts = serde_json::from_str(raw)?;
    ensure!(
        evidence.receipt.path.file_name().and_then(|s| s.to_str()) == Some("cuda-inference.json")
            && evidence.trace.path.file_name().and_then(|s| s.to_str()) == Some("cuda-trace.json"),
        "CUDA accepted artifact roles differ"
    );
    ensure!(
        serde_json::to_string(&evidence)? == raw,
        "CUDA candidate evidence is not canonical"
    );
    let (request_raw,intent_digest):(String,String)=db.query_row("SELECT manifest,request_sha256 FROM cuda_attempt_intents WHERE attempt=?1 AND typeof(manifest)='text' AND length(CAST(manifest AS BLOB))<=1048576 AND typeof(request_sha256)='text' AND length(CAST(request_sha256 AS BLOB))=64",[attempt],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let request: WorkerRequest = serde_json::from_str(&request_raw)?;
    request.validate()?;
    ensure!(
        digest == intent_digest
            && digest == hash(&request)?
            && request.attempt_id.as_str() == attempt,
        "CUDA accepted request differs from intent"
    );
    let (project,job,revision,lineage):(String,String,i64,String)=db.query_row("SELECT project,job,base_revision,lineage FROM candidates WHERE id=?1 AND typeof(project)='text' AND length(CAST(project AS BLOB))<=128 AND typeof(job)='text' AND length(CAST(job AS BLOB))<=128 AND typeof(lineage)='text' AND length(CAST(lineage AS BLOB))<=1048576",[candidate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let lineage: Vec<ArtifactIdentity> = serde_json::from_str(&lineage)?;
    ensure!(lineage.len() <= 256, "CUDA accepted lineage capacity");
    for identity in &lineage {
        identity.validate()?;
    }
    ensure!(
        project == request.project_id.as_str()
            && job == request.job_id.as_str()
            && revision >= 0
            && revision as u64 == request.base_revision.value()
            && lineage.contains(&evidence.receipt.identity)
            && lineage.contains(&evidence.trace.identity),
        "CUDA candidate evidence scope or lineage differs"
    );
    let state:String=db.query_row("SELECT state FROM attempts WHERE id=?1 AND job=?2 AND typeof(state)='text' AND length(CAST(state AS BLOB))<=32",params![attempt,job],|r|r.get(0))?;
    ensure!(
        state == "completed",
        "CUDA evidence belongs to an uncompleted attempt"
    );
    let current:(String,String,Option<String>)=db.query_row("SELECT attempt,state,candidate FROM jobs WHERE id=?1 AND typeof(attempt)='text' AND length(CAST(attempt AS BLOB))<=128 AND typeof(state)='text' AND length(CAST(state AS BLOB))<=32 AND (candidate IS NULL OR (typeof(candidate)='text' AND length(CAST(candidate AS BLOB))<=128))",[&job],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if current.0 == attempt {
        ensure!(
            current.1 == "completed" && current.2.as_deref() == Some(candidate),
            "CUDA current job candidate differs"
        );
    }
    crate::cuda_result::verify_files(&request, &evidence)?;
    Ok(())
}
/// Read-only startup validation runs before writable SQLite/WAL initialization.
pub fn validate(db: &Connection, version: i64) -> Result<()> {
    let version = crate::live_cuda_evidence_db::base_version(db, version)?;
    if version == 0 {
        return Ok(());
    }
    crate::portable_cuda_db::validate(db, version)?;
    let present = exists(db)?;
    if present {
        layout(db)?;
        capacity(db)?;
    }
    if version < 16 || matches!(version, 18..=20) && !present {
        if present {
            ensure!(
                capacity(db)?.0 == 0,
                "CUDA accepted evidence requires schema-16 reader barrier"
            );
        }
        let accepted:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE state='completed' AND candidate IS NOT NULL AND json_type(manifest,'$.cuda_execution') IS NOT NULL AND json_type(manifest,'$.cuda_execution')!='null')",[],|r|r.get(0))?;
        ensure!(
            !accepted,
            "CUDA completed candidate lacks schema-16 evidence membership"
        );
        return Ok(());
    }
    ensure!(
        matches!(version, 16..=20) && present,
        "missing or unknown CUDA accepted evidence schema"
    );
    let mut statement=db.prepare("SELECT candidate,attempt,request_sha256,artifacts FROM cuda_candidate_evidence ORDER BY candidate")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        check_member(
            db,
            &row.get::<_, String>(0)?,
            &row.get::<_, String>(1)?,
            &row.get::<_, String>(2)?,
            &row.get::<_, String>(3)?,
        )?;
    }
    let missing:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE state='completed' AND json_type(manifest,'$.cuda_execution') IS NOT NULL AND json_type(manifest,'$.cuda_execution')!='null' AND NOT EXISTS(SELECT 1 FROM cuda_candidate_evidence WHERE attempt=attempts.id))",[],|r|r.get(0))?;
    ensure!(
        !missing,
        "completed CUDA attempt lacks accepted evidence membership"
    );
    Ok(())
}
/// Must share the candidate/job/attempt completion transaction. The caller has
/// already checked authorization, epoch, running intent and launch custody.
pub fn retain(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    candidate: &CandidateId,
    evidence: &WorkerCudaArtifacts,
) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_cuda_acceptance;")?;
    match retain_inner(tx, request, candidate, evidence) {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_cuda_acceptance;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_cuda_acceptance; RELEASE pulsar_cuda_acceptance;",
            )?;
            Err(error)
        }
    }
}
fn retain_inner(
    tx: &Transaction<'_>,
    request: &WorkerRequest,
    candidate: &CandidateId,
    evidence: &WorkerCudaArtifacts,
) -> Result<()> {
    request.validate()?;
    ensure!(
        request.cuda_execution.is_some(),
        "CPU candidate cannot retain CUDA evidence"
    );
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let version = crate::live_cuda_evidence_db::base_version(tx, version)?;
    ensure!(
        matches!(version, 15..=20),
        "CUDA evidence requires durable intent schema"
    );
    crate::portable_cuda_db::validate(tx, version)?;
    if !exists(tx)? {
        tx.execute_batch(CREATE)?;
    }
    layout(tx)?;
    let (count, total) = capacity(tx)?;
    let raw = serde_json::to_string(evidence)?;
    ensure!(
        raw.len() <= MAX_RECORD && count < 4096 && total + raw.len() as i64 <= 64 * 1024 * 1024,
        "CUDA accepted evidence capacity exhausted"
    );
    let digest = hash(request)?;
    check_member(
        tx,
        candidate.as_str(),
        request.attempt_id.as_str(),
        &digest,
        &raw,
    )?;
    tx.execute(
        "INSERT INTO cuda_candidate_evidence VALUES(?1,?2,?3,?4)",
        params![candidate.as_str(), request.attempt_id.as_str(), digest, raw],
    )?;
    if version < 16 {
        crate::portable_annotated_db::publish(tx, 16)?;
    }
    if version >= 18 {
        crate::portable_cuda_db::arm(tx)?;
    }
    validate(tx, version.max(16)).context("CUDA accepted evidence publication validation")
}

#[cfg(test)]
pub(crate) fn authored_evidence(root: &std::path::Path) -> (WorkerRequest, WorkerCudaArtifacts) {
    let request: WorkerRequest =
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap();
    authored_evidence_for(request, root)
}
#[cfg(test)]
pub(crate) fn authored_evidence_for(
    mut request: WorkerRequest,
    root: &std::path::Path,
) -> (WorkerRequest, WorkerCudaArtifacts) {
    request.output_dir = root.to_path_buf();
    let execution = request.cuda_execution.as_ref().unwrap();
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref().unwrap(),
        _ => unreachable!(),
    };
    let trace=serde_json::to_vec(&serde_json::json!([
        {"cat":"Node","name":"Conv_0_kernel_time","ph":"X","ts":11,"dur":2,"pid":1,"tid":2,"args":{"provider":"CUDAExecutionProvider","op_name":"Conv"}},
        {"cat":"Session","name":"model_run","ph":"X","ts":10,"dur":10,"pid":1,"tid":2}
    ])).unwrap();
    let checked =
        crate::inference_trace::validate_node_trace(&trace, InferenceProvider::Cuda, 1).unwrap();
    let receipt = serde_json::json!({"provider":"cuda","provider_evidence":"strict-cuda-node-trace",
        "device":{"device_uuid":execution.device_uuid,"pci_bus_address":execution.pci_bus_address,"device_ordinal":execution.device_ordinal,"visible_device_count":1,"total_bytes":25_313_280_000u64,"driver_version":13040},
        "successful_runs":1,"trace_path":root.join("cuda_model_authored.json"),"trace":checked,
        "worker_binding":{"attempt_id":request.attempt_id,"job_id":request.job_id,"request_sha256":hash(&request).unwrap(),"profile_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(profile).unwrap())),"model":profile.model,"runtime":profile.runtime,"owner_policy":execution.owner_policy}});
    std::fs::write(root.join("cuda-trace.json"), trace).unwrap();
    std::fs::write(
        root.join("cuda-inference.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    let evidence = WorkerCudaArtifacts {
        receipt: crate::worker::pin_artifact(&root.join("cuda-inference.json")).unwrap(),
        trace: crate::worker::pin_artifact(&root.join("cuda-trace.json")).unwrap(),
    };
    crate::cuda_result::verify_files(&request, &evidence).unwrap();
    (request, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database(request: &WorkerRequest, evidence: &WorkerCudaArtifacts) -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=14;CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT,project TEXT,base_revision INTEGER,source TEXT,manifest TEXT,state TEXT,candidate TEXT);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT REFERENCES jobs(id),manifest TEXT,state TEXT);CREATE TABLE candidates(id TEXT PRIMARY KEY,project TEXT,job TEXT,base_revision INTEGER,lineage TEXT);").unwrap();
        let tx = db.transaction().unwrap();
        let raw = serde_json::to_string(request).unwrap();
        tx.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,?5,?6,'queued',NULL)",
            params![
                request.job_id.as_str(),
                request.attempt_id.as_str(),
                request.project_id.as_str(),
                request.base_revision.value(),
                request.source.source_version.as_str(),
                raw
            ],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
        )
        .unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        crate::cuda_attempts::declare(&tx, request, &epoch).unwrap();
        tx.execute_batch("UPDATE jobs SET state='running';UPDATE attempts SET state='running';")
            .unwrap();
        crate::cuda_attempts::require_running(&tx, request, &epoch).unwrap();
        assert!(crate::cuda_attempts::require_running(
            &tx,
            request,
            &uuid::Uuid::new_v4().to_string()
        )
        .is_err());
        tx.execute(
            "INSERT INTO candidates VALUES('candidate',?1,?2,?3,?4)",
            params![
                request.project_id.as_str(),
                request.job_id.as_str(),
                request.base_revision.value(),
                serde_json::to_string(&vec![
                    evidence.receipt.identity.clone(),
                    evidence.trace.identity.clone()
                ])
                .unwrap()
            ],
        )
        .unwrap();
        tx.execute_batch("UPDATE jobs SET state='completed',candidate='candidate';UPDATE attempts SET state='completed';").unwrap();
        tx.commit().unwrap();
        db
    }
    #[test]
    fn evidence_membership_binds_intent_candidate_lineage_and_reader_barrier() {
        let root = tempfile::tempdir().unwrap();
        let (request, evidence) = authored_evidence(root.path());
        let mut db = database(&request, &evidence);
        assert!(validate(&db, 15).is_err());
        let tx = db.transaction().unwrap();
        retain(
            &tx,
            &request,
            &CandidateId::new("candidate").unwrap(),
            &evidence,
        )
        .unwrap();
        tx.commit().unwrap();
        validate(&db, 16).unwrap();
        crate::cuda_attempts::validate(&db, 16).unwrap();
        assert!(validate(&db, 15).is_err());
        for sql in ["UPDATE candidates SET lineage='[]'","UPDATE cuda_candidate_evidence SET request_sha256=printf('%064d',0)","UPDATE cuda_candidate_evidence SET artifacts=zeroblob(32769)","UPDATE cuda_candidate_evidence SET artifacts=json_set(artifacts,'$.trace.path','/outside')","UPDATE attempts SET state='running'","UPDATE jobs SET candidate=NULL","DELETE FROM cuda_candidate_evidence"] {
            let tx=db.transaction().unwrap();tx.execute_batch(sql).unwrap();assert!(validate(&tx,16).is_err(),"{sql}");tx.rollback().unwrap();
        }
        std::fs::write(&evidence.trace.path, b"[]").unwrap();
        assert!(validate(&db, 16).is_err());
    }
    #[test]
    fn archival_barrier_activation_retains_evidence_requirement_and_rolls_back_failure() {
        let root = tempfile::tempdir().unwrap();
        let (request, evidence) = authored_evidence(root.path());
        let mut db = database(&request, &evidence);
        {
            let tx = db.transaction().unwrap();
            crate::portable_cuda_db::arm(&tx).unwrap();
            tx.commit().unwrap();
        }
        let initial = crate::portable_cuda_db::validate(&db, 18).unwrap().unwrap();
        assert!(initial.cuda_intents && !initial.cuda_evidence && !initial.worker_fence);
        {
            let tx = db.transaction().unwrap();
            tx.execute("UPDATE candidates SET lineage='[]'", [])
                .unwrap();
            assert!(retain(
                &tx,
                &request,
                &CandidateId::new("candidate").unwrap(),
                &evidence
            )
            .is_err());
            assert!(!exists(&tx).unwrap());
            assert_eq!(
                crate::portable_cuda_db::validate(&tx, 18).unwrap().unwrap(),
                initial
            );
            tx.rollback().unwrap();
        }
        {
            let tx = db.transaction().unwrap();
            retain(
                &tx,
                &request,
                &CandidateId::new("candidate").unwrap(),
                &evidence,
            )
            .unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
        let activated = crate::portable_cuda_db::validate(&db, 18).unwrap().unwrap();
        assert!(activated.cuda_evidence && activated.cuda_intents && !activated.worker_fence);
        validate(&db, 18).unwrap();
        crate::cuda_attempts::validate(&db, 18).unwrap();
        let tx = db.transaction().unwrap();
        tx.execute("DROP TABLE cuda_candidate_evidence", [])
            .unwrap();
        assert!(validate(&tx, 18).is_err());
        assert!(crate::portable_cuda_db::arm(&tx).is_err());
        tx.rollback().unwrap();
        validate(&db, 18).unwrap();
    }
    #[test]
    fn failed_retention_rolls_back_layout_and_barrier_without_replacing_intent() {
        let root = tempfile::tempdir().unwrap();
        let (request, evidence) = authored_evidence(root.path());
        let mut db = database(&request, &evidence);
        db.execute("UPDATE candidates SET lineage='[]'", [])
            .unwrap();
        let tx = db.transaction().unwrap();
        assert!(retain(
            &tx,
            &request,
            &CandidateId::new("candidate").unwrap(),
            &evidence
        )
        .is_err());
        assert!(!exists(&tx).unwrap());
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            15
        );
        tx.commit().unwrap();
        crate::cuda_attempts::validate(&db, 15).unwrap();
    }
}
