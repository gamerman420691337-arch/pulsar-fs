//! Capture provenance is distinct from imported media. Stored bytes establish
//! historical consistency, never a current lease or permission to launch.
use crate::{
    live_cuda_evidence::{EvidenceAdmission, LiveCudaEvidence},
    resources::ResourcePool,
};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
const TABLE: &str = "live_cuda_capture_evidence";
const CREATE:&str="CREATE TABLE live_cuda_capture_evidence(attempt TEXT PRIMARY KEY NOT NULL,project TEXT NOT NULL REFERENCES projects(id),epoch TEXT UNIQUE NOT NULL,source TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),owner TEXT NOT NULL REFERENCES sessions(id),origin BLOB NOT NULL,manifest BLOB NOT NULL,summary BLOB NOT NULL,receipt BLOB NOT NULL,trace BLOB NOT NULL)";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveCudaOrigin {
    schema_version: u16,
    engine_epoch: AttemptId,
    identity: LiveIdentity,
    owner: SessionId,
    start_request: RequestId,
    options: LiveCaptureOptions,
}
impl LiveCudaOrigin {
    pub(crate) fn new(
        owner: &SessionId,
        report: &LiveCaptureReport,
        start_request: &RequestId,
    ) -> Result<Self> {
        report.validate()?;
        ensure!(
            report.active
                && report.received_frames == 0
                && report.inference.as_ref().is_some_and(|i| i.state
                    == LiveCaptureInferenceState::Starting
                    && i.completed == 0),
            "CUDA origin requires initial admitted capture"
        );
        let origin = Self {
            schema_version: 1,
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
            owner: owner.clone(),
            start_request: start_request.clone(),
            options: report.options.clone(),
        };
        origin.bytes()?;
        Ok(origin)
    }
    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        self.options.validate()?;
        ensure!(
            self.schema_version == 1
                && self
                    .options
                    .inference
                    .as_ref()
                    .is_some_and(|i| i.profile.provider == InferenceProvider::Cuda),
            "CUDA capture origin schema/provider differs"
        );
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() <= 1048576,
            "CUDA capture origin exceeds admission"
        );
        Ok(bytes)
    }
    fn membership(
        &self,
        db: &Connection,
        evidence: &LiveCudaEvidence,
        current: bool,
    ) -> Result<()> {
        let response:Option<String>=db.query_row("SELECT response FROM requests WHERE session=?1 AND request=?2 AND typeof(response)='text' AND length(CAST(response AS BLOB))<=1048576",params![self.owner.as_str(),self.start_request.as_str()],|r|r.get(0)).optional()?;
        let response = response.ok_or_else(|| {
            anyhow::anyhow!("CUDA capture initial admission receipt missing or oversized")
        })?;
        let body: ResponseBody = serde_json::from_str(&response)?;
        ensure!(
            serde_json::to_string(&body)? == response,
            "CUDA capture admission receipt is not canonical"
        );
        let ResponseBody::LiveCapture(report) = body else {
            anyhow::bail!("CUDA capture admission receipt kind differs");
        };
        ensure!(
            Self::new(&self.owner, &report, &self.start_request)?.bytes()? == self.bytes()?,
            "CUDA capture origin differs from initial admission receipt"
        );
        let stream = &evidence.manifest().stream;
        ensure!(
            self.identity == stream.identity
                && self.options.clock(self.identity.epoch.clone()) == stream.clock
                && crate::inference_profiles::profile_identity(
                    &self.options.inference.as_ref().unwrap().profile
                )?
                .sha256
                    == crate::inference_profiles::profile_identity(&stream.profile)?.sha256,
            "CUDA capture origin differs from native scope"
        );
        let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM projects p JOIN sessions s ON s.id=?1 WHERE p.id=?2 AND typeof(p.revision)='integer' AND p.revision>=?3 AND (?4=0 OR p.revision=?3))",params![self.owner.as_str(),self.identity.project.as_str(),i64::try_from(self.identity.revision.value())?,current],|r|r.get(0))?;
        ensure!(
            present,
            "CUDA capture owner/project/revision membership differs"
        );
        Ok(())
    }
}
pub(crate) fn exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [TABLE],
        |r| r.get(0),
    )?)
}
pub(crate) fn shape(db: &Connection) -> Result<(i64, i64)> {
    let sql:Option<String>=db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096",[TABLE],|r|r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(CREATE),
        "live CUDA capture evidence layout differs"
    );
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM live_cuda_capture_evidence LIMIT 257)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= 256,
        "live CUDA capture evidence row capacity exceeded"
    );
    let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM live_cuda_capture_evidence WHERE typeof(attempt)!='text' OR length(CAST(attempt AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(project)!='text' OR length(CAST(project AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(epoch)!='text' OR length(CAST(epoch AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(source)!='text' OR length(CAST(source AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(owner)!='text' OR length(CAST(owner AS BLOB)) NOT BETWEEN 1 AND 128 OR typeof(revision)!='integer' OR revision<0 OR typeof(origin)!='blob' OR length(origin) NOT BETWEEN 1 AND 1048576 OR typeof(manifest)!='blob' OR length(manifest) NOT BETWEEN 1 AND 1048576 OR typeof(summary)!='blob' OR length(summary) NOT BETWEEN 1 AND 4096 OR typeof(receipt)!='blob' OR length(receipt) NOT BETWEEN 1 AND 65536 OR typeof(trace)!='blob' OR length(trace) NOT BETWEEN 1 AND 8388608)",[],|r|r.get(0))?;
    ensure!(
        !invalid,
        "live CUDA capture evidence scalar or byte admission differs"
    );
    let total:i64=db.query_row("SELECT COALESCE(SUM(length(origin)+length(manifest)+length(summary)+length(receipt)+length(trace)),0) FROM live_cuda_capture_evidence",[],|r|r.get(0))?;
    ensure!(
        total <= 256 * 1024 * 1024,
        "live CUDA capture evidence aggregate capacity exceeded"
    );
    Ok((count, total))
}
pub(crate) fn validate(db: &Connection, pool: &ResourcePool) -> Result<()> {
    if !exists(db)? {
        return Ok(());
    }
    shape(db)?;
    let mut query=db.prepare("SELECT attempt,project,epoch,source,revision,owner,origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence ORDER BY attempt")?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        read_row(db, row, pool)?;
    }
    Ok(())
}
fn read_row(
    db: &Connection,
    row: &rusqlite::Row<'_>,
    pool: &ResourcePool,
) -> Result<(LiveCudaOrigin, LiveCudaEvidence)> {
    let admission = EvidenceAdmission::reserve(pool)?;
    let origin_bytes: Vec<u8> = row.get(6)?;
    let origin: LiveCudaOrigin = serde_json::from_slice(&origin_bytes)?;
    ensure!(
        origin.bytes()? == origin_bytes,
        "CUDA capture origin is not canonical"
    );
    let evidence = admission.read(row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)?;
    let stream = &evidence.manifest().stream;
    ensure!(
        row.get::<_, String>(0)? == stream.attempt_id.as_str()
            && row.get::<_, String>(1)? == stream.identity.project.as_str()
            && row.get::<_, String>(2)? == stream.identity.epoch.as_str()
            && row.get::<_, String>(3)? == stream.identity.source.as_str()
            && row.get::<_, i64>(4)? == i64::try_from(stream.identity.revision.value())?
            && row.get::<_, String>(5)? == origin.owner.as_str(),
        "CUDA capture indexed scope differs"
    );
    origin.membership(db, &evidence, false)?;
    Ok((origin, evidence))
}
/// Bounds SQL values before loading originals, then revalidates historical
/// correspondence. Caller holds current project authorization and revision.
/// Scalar-only index read. Original blobs are validated by inspection/download.
pub(crate) fn list(
    db: &Connection,
    project: &ProjectId,
    after: Option<&AttemptId>,
    limit: u16,
) -> Result<LiveCudaEvidencePage> {
    ensure!(
        (1..=16).contains(&limit),
        "retained CUDA listing limit must be between 1 and 16"
    );
    let mut page = LiveCudaEvidencePage {
        project: project.clone(),
        after: after.cloned(),
        limit,
        entries: Vec::new(),
        next_after: None,
    };
    if !exists(db)? {
        return Ok(page);
    }
    shape(db)?;
    let mut query=db.prepare("SELECT epoch,source,revision FROM live_cuda_capture_evidence WHERE project=?1 AND (?2 IS NULL OR epoch COLLATE BINARY>?2) ORDER BY epoch COLLATE BINARY LIMIT ?3")?;
    let mut rows = query.query(params![
        project.as_str(),
        after.map(|id| id.as_str()),
        i64::from(limit) + 1
    ])?;
    while let Some(row) = rows.next()? {
        let entry = LiveIdentity {
            project: project.clone(),
            epoch: AttemptId::new(row.get::<_, String>(0)?)?,
            source: SourceVersionId::new(row.get::<_, String>(1)?)?,
            revision: RevisionId::new(u64::try_from(row.get::<_, i64>(2)?)?),
        };
        if page.entries.len() == usize::from(limit) {
            page.next_after = page.entries.last().map(|e| e.epoch.clone());
            break;
        }
        page.entries.push(entry);
    }
    page.validate()?;
    Ok(page)
}
pub(crate) fn inspect(
    db: &Connection,
    pool: &ResourcePool,
    project: &ProjectId,
    epoch: &AttemptId,
) -> Result<Option<RetainedLiveCudaEvidence>> {
    let Some((origin, evidence)) = load(db, pool, project, epoch)? else {
        return Ok(None);
    };
    let report = RetainedLiveCudaEvidence {
        schema_version: 1,
        engine_epoch: origin.engine_epoch,
        identity: origin.identity,
        owner: origin.owner,
        start_request: origin.start_request,
        evidence: evidence.report_record()?,
    };
    report.validate()?;
    Ok(Some(report))
}
fn load(
    db: &Connection,
    pool: &ResourcePool,
    project: &ProjectId,
    epoch: &AttemptId,
) -> Result<Option<(LiveCudaOrigin, LiveCudaEvidence)>> {
    if !exists(db)? {
        return Ok(None);
    }
    shape(db)?;
    let mut query=db.prepare("SELECT attempt,project,epoch,source,revision,owner,origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence WHERE project=?1 AND epoch=?2")?;
    let mut rows = query.query(params![project.as_str(), epoch.as_str()])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(read_row(db, row, pool)?))
}

/// Keeps original bytes and evidence memory admission alive through lease retries.
pub(crate) struct EvidenceDownload {
    origin: Vec<u8>,
    evidence: LiveCudaEvidence,
    part: LiveCudaEvidencePart,
}
impl EvidenceDownload {
    pub(crate) fn bytes(&self) -> &[u8] {
        match self.part {
            LiveCudaEvidencePart::Origin => &self.origin,
            LiveCudaEvidencePart::Manifest => self.evidence.manifest_bytes(),
            LiveCudaEvidencePart::Summary => self.evidence.summary_bytes(),
            LiveCudaEvidencePart::Receipt => self.evidence.receipt(),
            LiveCudaEvidencePart::Trace => self.evidence.trace(),
        }
    }
    pub(crate) fn identity(&self) -> Result<ArtifactIdentity> {
        let bytes = self.bytes();
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len: bytes.len() as u64,
        })
    }
}
pub(crate) fn download(
    db: &Connection,
    pool: &ResourcePool,
    project: &ProjectId,
    epoch: &AttemptId,
    part: LiveCudaEvidencePart,
) -> Result<Option<EvidenceDownload>> {
    let Some((origin, evidence)) = load(db, pool, project, epoch)? else {
        return Ok(None);
    };
    let result = EvidenceDownload {
        origin: origin.bytes()?,
        evidence,
        part,
    };
    ensure!(
        !result.bytes().is_empty() && result.bytes().len() as u64 <= part.maximum_bytes(),
        "retained evidence part admission differs"
    );
    Ok(Some(result))
}
/// Deletes only the checked retained row in the caller's transaction. Existing
/// admitted downloads own immutable bytes until their independent retirement.
pub(crate) fn remove(
    tx: &Transaction<'_>,
    pool: &ResourcePool,
    project: &ProjectId,
    epoch: &AttemptId,
    expected_manifest_sha256: &str,
) -> Result<Option<LiveCudaEvidenceRemoval>> {
    validate_sha256(expected_manifest_sha256)?;
    let Some((origin, evidence)) = load(tx, pool, project, epoch)? else {
        return Ok(None);
    };
    let record = evidence.report_record()?;
    ensure!(
        record.manifest.sha256 == expected_manifest_sha256,
        "retained CUDA manifest changed or differs from removal intent"
    );
    let receipt = LiveCudaEvidenceRemoval {
        schema_version: 1,
        project_id: project.clone(),
        epoch: epoch.clone(),
        manifest: record.manifest,
        removed_original_bytes: evidence.byte_len() + origin.bytes()?.len() as u64,
        physical_erasure: false,
    };
    receipt.validate()?;
    ensure!(
        tx.execute(
            "DELETE FROM live_cuda_capture_evidence WHERE project=?1 AND epoch=?2",
            params![project.as_str(), epoch.as_str()]
        )? == 1,
        "retained CUDA removal row changed"
    );
    Ok(Some(receipt))
}
/// Scope is minted only by the public capture registry; the caller supplies
/// current authorization, opaque worker binding and the original lease checks.
pub(crate) fn retain(
    tx: &Transaction<'_>,
    origin: &LiveCudaOrigin,
    evidence: &LiveCudaEvidence,
) -> Result<()> {
    tx.execute_batch("SAVEPOINT pulsar_live_cuda_capture;")?;
    let result = (|| -> Result<()> {
        let version = crate::annotated_generation_db::storage_version(tx)?;
        ensure!(
            matches!(version, 20..=22),
            "CUDA capture requires creation-custody schema"
        );
        crate::live_cuda_evidence_db::base_version(tx, version)?;
        ensure!(
            crate::worker_fence_db::binding(tx, version)?
                .is_some_and(|b| b.minimum_ledger_version == 4),
            "CUDA capture ownership binding absent"
        );
        crate::cuda_attempts::validate(tx, version)?;
        crate::cuda_acceptance::validate(tx, version)?;
        let bytes = origin.bytes()?;
        ensure!(
            evidence.storage_reserved() && evidence.storage_extra() >= bytes.len() as u64,
            "CUDA capture origin storage was not admitted"
        );
        origin.membership(tx, evidence, true)?;
        if version == 20 {
            tx.execute_batch(crate::live_cuda_evidence_db::CREATE)?;
        }
        if version < 22 {
            tx.execute_batch(CREATE)?;
        }
        let (count, total) = crate::live_cuda_evidence_db::capacity(tx)?;
        ensure!(
            count < 256
                && evidence.byte_len() + bytes.len() as u64 <= (256 * 1024 * 1024 - total) as u64,
            "CUDA capture publication capacity exhausted"
        );
        let stream = &evidence.manifest().stream;
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM live_cuda_evidence WHERE attempt=?1 OR epoch=?2)",
            params![stream.attempt_id.as_str(), stream.identity.epoch.as_str()],
            |r| r.get(0),
        )?;
        ensure!(
            !duplicate,
            "CUDA capture conflicts with retained legacy scope"
        );
        tx.execute(
            "INSERT INTO live_cuda_capture_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                stream.attempt_id.as_str(),
                stream.identity.project.as_str(),
                stream.identity.epoch.as_str(),
                stream.identity.source.as_str(),
                i64::try_from(stream.identity.revision.value())?,
                origin.owner.as_str(),
                bytes,
                evidence.manifest_bytes(),
                evidence.summary_bytes(),
                evidence.receipt(),
                evidence.trace()
            ],
        )?;
        if version < 22 {
            crate::annotated_generation_db::publish_storage_version(tx, 22)?;
        }
        crate::live_cuda_evidence_db::base_version(tx, 22)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            tx.execute_batch("RELEASE pulsar_live_cuda_capture;")?;
            Ok(())
        }
        Err(error) => {
            tx.execute_batch(
                "ROLLBACK TO pulsar_live_cuda_capture; RELEASE pulsar_live_cuda_capture;",
            )?;
            Err(error)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[cfg(test)]
    type CaptureEvidenceRow = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
    fn fixture() -> (ResourcePool, Connection, LiveCudaOrigin, LiveCudaEvidence) {
        let pool = ResourcePool::new(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        let evidence = crate::live_cuda_evidence_db::tests::evidence(&pool);
        let db = crate::live_cuda_evidence_db::tests::fixture(&evidence);
        db.execute_batch("DELETE FROM sources; CREATE TABLE sessions(id TEXT PRIMARY KEY); CREATE TABLE requests(session TEXT,request TEXT,response TEXT,PRIMARY KEY(session,request)); INSERT INTO sessions VALUES('authored-owner');").unwrap();
        let owner = SessionId::new("authored-owner").unwrap();
        let request = RequestId::new("authored-start").unwrap();
        let stream = &evidence.manifest().stream;
        let report = LiveCaptureReport {
            input_close: None,
            continuation: None,
            age_basis: LiveCaptureAgeBasis::ProducerDeclared,
            encoded_source: None,
            engine_epoch: AttemptId::new("authored-boot").unwrap(),
            identity: stream.identity.clone(),
            clock: stream.clock.clone(),
            options: LiveCaptureOptions {
                inference_fallback: None,
                browser_analysis: None,
                browser_media: None,
                model_pack: None,
                source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
                continuation_mode: LiveContinuationMode::Immediate,
                encoded_source: None,
                inference: Some(LiveCaptureInference {
                    model_path: stream.model.path.clone(),
                    profile: stream.profile.clone(),
                }),
                policy: LivePolicy {
                    maximum_analysis_age_ns: 200_000_000,
                    added_buffer_ns: stream.clock.added_buffer_ns,
                },
                source_anchor: stream.clock.source_anchor,
                project_anchor: stream.clock.project_anchor,
                exact_project_anchor: stream.clock.exact_project_anchor,
                presentation_anchor: stream.clock.presentation_anchor,
                lease_ms: stream.budget.wall_time_ms as u32,
            },
            active: true,
            retirement: None,
            server_elapsed: Some(LiveClockTick(0)),
            admitted_pixel_bytes: 5 * 64 * 64 * 3,
            received_frames: 0,
            last_frame: None,
            queue: None,
            inference: Some(LiveCaptureInferenceReport {
                fallback_transition: None,
                fallback: None,
                cpu_manifest: None,
                cuda_manifest_sha256: None,
                cuda_evidence: None,
                attempt_id: stream.attempt_id.clone(),
                profile_sha256: crate::inference_profiles::profile_identity(&stream.profile)
                    .unwrap()
                    .sha256,
                state: LiveCaptureInferenceState::Starting,
                completed: 0,
                accepted: 0,
                stale: 0,
                latest: None,
                error: None,
            }),
        };
        let origin = LiveCudaOrigin::new(&owner, &report, &request).unwrap();
        db.execute(
            "INSERT INTO requests VALUES(?1,?2,?3)",
            params![
                owner.as_str(),
                request.as_str(),
                serde_json::to_string(&ResponseBody::LiveCapture(Box::new(report))).unwrap()
            ],
        )
        .unwrap();
        (pool, db, origin, evidence)
    }
    fn committed() -> (ResourcePool, Connection, LiveCudaOrigin, LiveCudaEvidence) {
        let (pool, mut db, origin, mut evidence) = fixture();
        evidence
            .reserve_storage_extra(u64::MAX, origin.bytes().unwrap().len() as u64)
            .unwrap();
        let tx = db.transaction().unwrap();
        retain(&tx, &origin, &evidence).unwrap();
        tx.commit().unwrap();
        (pool, db, origin, evidence)
    }
    // Seeds command-handler tests only; this is not startup/schema qualification.
    pub(crate) fn install_removal_fixture(
        db: &Connection,
        template_project: &ProjectId,
        template_session: &SessionId,
    ) -> RetainedLiveCudaEvidence {
        let (pool, source, origin, _evidence) = committed();
        let record = inspect(
            &source,
            &pool,
            &origin.identity.project,
            &origin.identity.epoch,
        )
        .unwrap()
        .unwrap();
        db.execute("INSERT INTO projects SELECT ?1,name,?2,program,history_cursor,protected FROM projects WHERE id=?3",params![origin.identity.project.as_str(),origin.identity.revision.value(),template_project.as_str()]).unwrap();
        db.execute(
            "INSERT INTO sessions SELECT ?1,actor,label,token_hash FROM sessions WHERE id=?2",
            params![origin.owner.as_str(), template_session.as_str()],
        )
        .unwrap();
        db.execute("INSERT INTO grants SELECT session,?1,scope FROM grants WHERE session=?2 AND project=?3",params![origin.identity.project.as_str(),template_session.as_str(),template_project.as_str()]).unwrap();
        let response: String = source
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![origin.owner.as_str(), origin.start_request.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4)",
            params![
                origin.owner.as_str(),
                origin.start_request.as_str(),
                "0".repeat(64),
                response
            ],
        )
        .unwrap();
        db.execute_batch(CREATE).unwrap();
        let values: Vec<rusqlite::types::Value> = source
            .query_row("SELECT * FROM live_cuda_capture_evidence", [], |row| {
                (0..11).map(|i| row.get(i)).collect()
            })
            .unwrap();
        db.execute(
            "INSERT INTO live_cuda_capture_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params_from_iter(values),
        )
        .unwrap();
        record
    }
    pub(crate) fn owned_download_fixture(
        part: LiveCudaEvidencePart,
    ) -> (ResourcePool, EvidenceDownload) {
        let (pool, db, origin, evidence) = committed();
        drop(evidence);
        let owned = download(
            &db,
            &pool,
            &origin.identity.project,
            &origin.identity.epoch,
            part,
        )
        .unwrap()
        .unwrap();
        (pool, owned)
    }
    #[test]
    fn index_pages_are_bounded_scoped_ordered_and_do_not_load_originals() {
        let (_pool, db, origin, _evidence) = committed();
        for suffix in ["a", "b", "c"] {
            db.execute("INSERT INTO live_cuda_capture_evidence SELECT ?1,project,?2,source,revision,owner,origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence LIMIT 1",params![format!("index-attempt-{suffix}"),format!("index-epoch-{suffix}")]).unwrap();
        }
        let project = &origin.identity.project;
        let page = list(&db, project, None, 2).unwrap();
        page.validate().unwrap();
        assert_eq!(page.entries.len(), 2);
        assert!(page.next_after.is_some());
        let next = list(&db, project, page.next_after.as_ref(), 2).unwrap();
        assert_eq!(next.entries.len(), 2);
        assert!(next.next_after.is_none());
        assert!(page.entries.last().unwrap().epoch.as_str() < next.entries[0].epoch.as_str());
        assert!(
            list(&db, project, Some(&next.entries.last().unwrap().epoch), 2)
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(list(&db, &ProjectId::new("foreign").unwrap(), None, 2)
            .unwrap()
            .entries
            .is_empty());
        for limit in [0, 17, u16::MAX] {
            assert!(list(&db, project, None, limit).is_err());
        }
        // A malformed original remains an index entry, but cannot be inspected.
        db.execute("UPDATE live_cuda_capture_evidence SET trace=x'7b7d'", [])
            .unwrap();
        assert_eq!(list(&db, project, None, 16).unwrap().entries.len(), 4);
        let mut invalid = page.clone();
        invalid.entries.reverse();
        assert!(invalid.validate().is_err());
        let mut invalid = page;
        invalid.next_after = Some(AttemptId::new("wrong-cursor").unwrap());
        assert!(invalid.validate().is_err());
        db.execute(
            "UPDATE live_cuda_capture_evidence SET epoch=zeroblob(129) WHERE epoch=?1",
            [origin.identity.epoch.as_str()],
        )
        .unwrap();
        assert!(list(&db, project, None, 16).is_err());
    }
    #[test]
    fn removal_is_identity_checked_transactional_and_preserves_admitted_originals() {
        let (pool, mut db, origin, evidence) = committed();
        let manifest = evidence.report_record().unwrap().manifest;
        drop(evidence);
        let held = download(
            &db,
            &pool,
            &origin.identity.project,
            &origin.identity.epoch,
            LiveCudaEvidencePart::Trace,
        )
        .unwrap()
        .unwrap();
        let original = held.bytes().to_vec();
        // Inspection needs a second full admission; held downloads keep theirs.
        let larger = ResourcePool::new(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES, 1);
        {
            let tx = db.transaction().unwrap();
            assert!(remove(
                &tx,
                &larger,
                &origin.identity.project,
                &origin.identity.epoch,
                &"0".repeat(64)
            )
            .is_err());
            assert!(remove(
                &tx,
                &larger,
                &ProjectId::new("foreign").unwrap(),
                &origin.identity.epoch,
                &manifest.sha256
            )
            .unwrap()
            .is_none());
            let receipt = remove(
                &tx,
                &larger,
                &origin.identity.project,
                &origin.identity.epoch,
                &manifest.sha256,
            )
            .unwrap()
            .unwrap();
            receipt.validate().unwrap();
            assert!(!receipt.physical_erasure);
            assert_eq!(held.bytes(), original);
            assert!(list(&tx, &origin.identity.project, None, 16)
                .unwrap()
                .entries
                .is_empty());
            tx.rollback().unwrap();
        }
        assert_eq!(
            list(&db, &origin.identity.project, None, 16)
                .unwrap()
                .entries
                .len(),
            1
        );
        let tx = db.transaction().unwrap();
        let receipt = remove(
            &tx,
            &larger,
            &origin.identity.project,
            &origin.identity.epoch,
            &manifest.sha256,
        )
        .unwrap()
        .unwrap();
        tx.commit().unwrap();
        assert!(inspect(
            &db,
            &larger,
            &origin.identity.project,
            &origin.identity.epoch
        )
        .unwrap()
        .is_none());
        assert_eq!(held.bytes(), original);
        let mut invalid = receipt;
        invalid.physical_erasure = true;
        assert!(invalid.validate().is_err());
        assert!(pool
            .reserve_memory(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES + 1)
            .is_err());
        drop(held);
        assert!(pool
            .reserve_memory(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES)
            .is_ok());
    }
    #[test]
    fn download_keeps_admission_and_returns_every_original_part_exactly() {
        let (pool, db, origin, evidence) = committed();
        let originals = [
            origin.bytes().unwrap(),
            evidence.manifest_bytes().to_vec(),
            evidence.summary_bytes().to_vec(),
            evidence.receipt().to_vec(),
            evidence.trace().to_vec(),
        ];
        drop(evidence);
        for (part, expected) in [
            LiveCudaEvidencePart::Origin,
            LiveCudaEvidencePart::Manifest,
            LiveCudaEvidencePart::Summary,
            LiveCudaEvidencePart::Receipt,
            LiveCudaEvidencePart::Trace,
        ]
        .into_iter()
        .zip(originals)
        {
            let owned = download(
                &db,
                &pool,
                &origin.identity.project,
                &origin.identity.epoch,
                part,
            )
            .unwrap()
            .unwrap();
            assert_eq!(owned.bytes(), expected);
            assert_eq!(
                owned.identity().unwrap().sha256,
                format!("{:x}", Sha256::digest(&expected))
            );
            assert!(pool
                .reserve_memory(crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES + 1)
                .is_err());
            drop(owned);
            assert!(pool
                .reserve_memory(2 * crate::live_cuda_evidence::EVIDENCE_MEMORY_BYTES)
                .is_ok());
        }
    }
    #[test]
    fn inspection_revalidates_originals_and_survives_later_project_revision() {
        let (pool, db, origin, evidence) = committed();
        let report = inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch)
            .unwrap()
            .unwrap();
        assert_eq!(report.identity, origin.identity);
        assert_eq!(report.engine_epoch, origin.engine_epoch);
        assert_eq!(report.owner, origin.owner);
        assert_eq!(report.start_request, origin.start_request);
        assert_eq!(report.evidence, evidence.report_record().unwrap());
        assert!(inspect(
            &db,
            &pool,
            &ProjectId::new("foreign-project").unwrap(),
            &origin.identity.epoch
        )
        .unwrap()
        .is_none());
        assert!(inspect(
            &db,
            &pool,
            &origin.identity.project,
            &AttemptId::new("absent-epoch").unwrap()
        )
        .unwrap()
        .is_none());
        db.execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        assert_eq!(
            inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch)
                .unwrap()
                .unwrap()
                .identity,
            origin.identity
        );
        let mut invalid = report.clone();
        invalid.evidence.qualified = true;
        assert!(invalid.validate().is_err());
        let mut invalid = report;
        invalid.schema_version = 2;
        assert!(invalid.validate().is_err());
        db.execute("UPDATE live_cuda_capture_evidence SET trace=x'7b7d'", [])
            .unwrap();
        assert!(inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch).is_err());
    }
    #[test]
    fn inspection_requires_memory_admission_and_rejects_oversized_sql_before_blob_read() {
        let (pool, db, origin, _evidence) = committed();
        let control = pool.reserve(1).unwrap();
        assert!(inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch).is_err());
        drop(control);
        assert!(
            inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch)
                .unwrap()
                .is_some()
        );
        db.execute(
            "UPDATE live_cuda_capture_evidence SET trace=zeroblob(8388609)",
            [],
        )
        .unwrap();
        assert!(inspect(&db, &pool, &origin.identity.project, &origin.identity.epoch).is_err());
    }
    #[test]
    fn capture_origin_commits_without_fabricating_media_and_preserves_originals() {
        let (pool, db, origin, evidence) = committed();
        crate::live_cuda_evidence_db::validate(&db, 22, &pool).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            22
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        let bytes: (Vec<u8>, Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT origin,receipt,trace FROM live_cuda_capture_evidence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(bytes.0, origin.bytes().unwrap());
        assert_eq!(bytes.1, evidence.receipt());
        assert_eq!(bytes.2, evidence.trace());
        assert_eq!(
            crate::worker_fence_db::binding(&db, 22)
                .unwrap()
                .unwrap()
                .minimum_ledger_version,
            4
        );
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 20).unwrap(),
            20
        );
        assert!(crate::live_cuda_evidence_db::validate(&db, 23, &pool).is_err());
    }

    #[test]
    fn annotated_barrier_wraps_existing_capture_and_preserves_exact_originals() {
        let (pool, mut db, origin, evidence) = committed();
        let before: CaptureEvidenceRow = db
            .query_row(
                "SELECT origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        db.execute_batch("ALTER TABLE sources ADD COLUMN sha256 TEXT; ALTER TABLE sources ADD COLUMN bytes INTEGER;").unwrap();
        let request = crate::annotated_generation_db::tests::request();
        {
            let tx = db.transaction().unwrap();
            crate::annotated_generation_db::tests::seed(&tx, &request);
            crate::annotated_generation_db::retain(&tx, &request, &pool).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(
            crate::annotated_generation_db::storage_version(&db).unwrap(),
            22
        );
        crate::annotated_generation_db::validate(&db, &pool).unwrap();
        crate::live_cuda_evidence_db::validate(&db, 23, &pool).unwrap();
        let after: CaptureEvidenceRow = db
            .query_row(
                "SELECT origin,manifest,summary,receipt,trace FROM live_cuda_capture_evidence",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(after.0, origin.bytes().unwrap());
        assert_eq!(after.3, evidence.receipt());
    }
    #[test]
    fn capture_publication_under_annotated_barrier_cannot_lower_the_header() {
        let (pool, mut db, origin, mut evidence) = fixture();
        evidence
            .reserve_storage_extra(u64::MAX, origin.bytes().unwrap().len() as u64)
            .unwrap();
        db.execute_batch("ALTER TABLE sources ADD COLUMN sha256 TEXT; ALTER TABLE sources ADD COLUMN bytes INTEGER;").unwrap();
        let request = crate::annotated_generation_db::tests::request();
        {
            let tx = db.transaction().unwrap();
            crate::annotated_generation_db::tests::seed(&tx, &request);
            crate::annotated_generation_db::retain(&tx, &request, &pool).unwrap();
            assert_eq!(
                crate::annotated_generation_db::storage_version(&tx).unwrap(),
                20
            );
            retain(&tx, &origin, &evidence).unwrap();
            tx.commit().unwrap();
        }
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(
            crate::annotated_generation_db::storage_version(&db).unwrap(),
            22
        );
        crate::annotated_generation_db::validate(&db, &pool).unwrap();
        crate::live_cuda_evidence_db::validate(&db, 23, &pool).unwrap();
    }
    #[test]
    fn capture_origin_storage_receipt_revision_duplicate_and_rollback_refuse() {
        let (pool, mut db, origin, mut evidence) = fixture();
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &origin, &evidence).is_err());
            tx.commit().unwrap();
        }
        assert!(!exists(&db).unwrap());
        evidence
            .reserve_storage_extra(u64::MAX, origin.bytes().unwrap().len() as u64)
            .unwrap();
        {
            let tx = db.transaction().unwrap();
            retain(&tx, &origin, &evidence).unwrap();
            tx.rollback().unwrap();
        }
        assert!(!exists(&db).unwrap());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            20
        );
        db.execute("UPDATE requests SET response='{}'", []).unwrap();
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &origin, &evidence).is_err());
            tx.commit().unwrap();
        }
        assert!(!exists(&db).unwrap());
        drop((db, evidence, pool));
        let (pool, mut db, origin, evidence) = committed();
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &origin, &evidence).is_err());
            tx.commit().unwrap();
        }
        db.execute("UPDATE projects SET revision=revision+1", [])
            .unwrap();
        crate::live_cuda_evidence_db::validate(&db, 22, &pool).unwrap();
        {
            let tx = db.transaction().unwrap();
            assert!(retain(&tx, &origin, &evidence).is_err());
            tx.commit().unwrap();
        }
    }
    #[test]
    fn capture_origin_readback_refuses_tampering_downgrade_and_bounds() {
        for sql in [
            "PRAGMA user_version=21",
            "PRAGMA user_version=23",
            "DROP TABLE live_cuda_capture_evidence",
            "UPDATE live_cuda_capture_evidence SET origin=x'7b7d'",
            "UPDATE live_cuda_capture_evidence SET origin=zeroblob(1048577)",
            "UPDATE live_cuda_capture_evidence SET manifest=x'01'",
            "UPDATE live_cuda_capture_evidence SET summary=x'01'",
            "UPDATE live_cuda_capture_evidence SET trace=x'01'",
            "UPDATE live_cuda_capture_evidence SET receipt=x'01'",
            "UPDATE live_cuda_capture_evidence SET source='foreign'",
            "UPDATE requests SET response='{}'",
            "UPDATE projects SET revision=-1",
        ] {
            let (pool, db, _, _) = committed();
            db.execute_batch(sql).unwrap();
            assert!(
                crate::live_cuda_evidence_db::validate(
                    &db,
                    db.query_row("PRAGMA user_version", [], |r| r.get(0))
                        .unwrap(),
                    &pool
                )
                .is_err(),
                "{sql}"
            );
        }
        let (pool, db, _, _) = committed();
        db.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<256) INSERT INTO live_cuda_capture_evidence SELECT 'authored-'||i,project,'authored-epoch-'||i,source,revision,owner,origin,manifest,summary,receipt,trace FROM n,live_cuda_capture_evidence LIMIT 256;").unwrap();
        assert!(crate::live_cuda_evidence_db::validate(&db, 22, &pool)
            .unwrap_err()
            .to_string()
            .contains("capacity"));
    }
}
