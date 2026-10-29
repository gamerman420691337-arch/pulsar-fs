//! P1a logical project closure. This is not an export/import operation or a byte hold.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

const MAX_CAPTURE_METADATA: usize = 256 * 1024 * 1024;
const MAX_CAPTURE_ROWS: usize = 100_000;
const MAX_COMPACT_EVIDENCE: u64 = 128 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

enum ObjectLocation {
    File(PathBuf),
    Inline(Arc<[u8]>),
}
struct CapturedObject {
    descriptor: PackageObjectDescriptor,
    location: ObjectLocation,
}

/// The logical records are immutable, but filesystem availability was only
/// observed. P1b must acquire real eviction holds and revalidate while streaming.
pub(super) struct ProjectCapturePlan {
    pub(super) manifest: PortableProjectManifest,
    pub(super) manifest_sha256: String,
    objects: BTreeMap<String, CapturedObject>,
    _holds: Option<crate::project_package_storage::ArtifactHolds>,
    _memory: Reservation,
}
impl ProjectCapturePlan {
    pub(super) fn open_object(
        &self,
        descriptor: &PackageObjectDescriptor,
    ) -> PResult<Box<dyn Read>> {
        let object = self.objects.get(&descriptor.sha256).ok_or_else(|| {
            ProtocolError::invalid("object is not in the captured logical closure")
        })?;
        if object.descriptor != *descriptor {
            return Err(ProtocolError::invalid("capture object descriptor mismatch"));
        }
        open_object(object)
    }
}
fn missing(object: &PackageObjectDescriptor, detail: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        format!(
            "required {:?} object {}: {detail}",
            object.roles, object.sha256
        ),
    )
}
fn open_object(object: &CapturedObject) -> PResult<Box<dyn Read>> {
    match &object.location {
        ObjectLocation::Inline(bytes) => Ok(Box::new(Cursor::new(bytes.clone()))),
        ObjectLocation::File(path) => artifacts::open_regular(path)
            .map(|file| Box::new(file) as Box<dyn Read>)
            .map_err(|_| missing(&object.descriptor, "missing or unsafe evidence")),
    }
}
fn add_object(
    objects: &mut BTreeMap<String, CapturedObject>,
    reference: PackageObjectRef,
    role: PackageObjectRole,
    location: ObjectLocation,
) -> PResult<()> {
    ArtifactIdentity {
        artifact_id: ArtifactId::new(format!("package.{}", reference.sha256)).map_err(internal)?,
        sha256: reference.sha256.clone(),
        byte_len: reference.byte_len,
    }
    .validate()?;
    if let Some(old) = objects.get_mut(&reference.sha256) {
        if old.descriptor.byte_len != reference.byte_len {
            return Err(ProtocolError::invalid(
                "same digest has conflicting object lengths",
            ));
        }
        if !old.descriptor.roles.contains(&role) {
            old.descriptor.roles.push(role);
            old.descriptor.roles.sort();
        }
        return Ok(());
    }
    if objects.len() >= MAX_CAPTURE_ROWS {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "too many capture objects",
        ));
    }
    objects.insert(
        reference.sha256.clone(),
        CapturedObject {
            descriptor: PackageObjectDescriptor {
                sha256: reference.sha256,
                byte_len: reference.byte_len,
                roles: vec![role],
            },
            location,
        },
    );
    Ok(())
}
fn object_ref(identity: &ArtifactIdentity) -> PackageObjectRef {
    PackageObjectRef {
        sha256: identity.sha256.clone(),
        byte_len: identity.byte_len,
    }
}
fn inline_object(
    objects: &mut BTreeMap<String, CapturedObject>,
    bytes: Vec<u8>,
    role: PackageObjectRole,
) -> PResult<PackageObjectRef> {
    let reference = PackageObjectRef {
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        byte_len: bytes.len() as u64,
    };
    add_object(
        objects,
        reference.clone(),
        role,
        ObjectLocation::Inline(bytes.into()),
    )?;
    Ok(reference)
}
fn motion(
    db: &Connection,
    root: &Path,
    reference: &str,
    objects: &mut BTreeMap<String, CapturedObject>,
) -> PResult<ProgramDescriptor> {
    let descriptor = motion_state::descriptor(db, reference)?;
    add_object(
        objects,
        PackageObjectRef {
            sha256: descriptor.sha256.clone(),
            byte_len: descriptor.byte_len,
        },
        PackageObjectRole::MotionProgram,
        ObjectLocation::File(root.join("motion-objects").join(&descriptor.sha256)),
    )?;
    Ok(descriptor)
}
fn unsigned(value: i64) -> PResult<u64> {
    u64::try_from(value).map_err(|_| ProtocolError::invalid("negative durable project coordinate"))
}
fn revision(value: i64) -> PResult<RevisionId> {
    Ok(RevisionId::new(unsigned(value)?))
}
fn optional_revision(value: Option<i64>) -> PResult<Option<RevisionId>> {
    value.map(revision).transpose()
}
fn review(raw: &str) -> PResult<PackageReviewState> {
    let state: lineage::ReviewState = decode(raw)?;
    validate_reviews(&state.flags)?;
    Ok(PackageReviewState {
        decisions: state.decisions,
        flags: state.flags,
        unknown: state.unknown,
        conservative: state.conservative,
    })
}
struct CaptureIndexes<'a> {
    candidates: BTreeMap<&'a str, &'a PackageCandidate>,
    revisions: BTreeMap<RevisionId, &'a PackageRevision>,
    sources: BTreeMap<&'a str, &'a PackageSource>,
    candidates_by_job: BTreeMap<&'a str, Vec<&'a PackageCandidate>>,
}
impl<'a> CaptureIndexes<'a> {
    fn new(
        candidates: &'a [PackageCandidate],
        revisions: &'a [PackageRevision],
        sources: &'a [PackageSource],
    ) -> Self {
        let mut candidates_by_job: BTreeMap<&str, Vec<&PackageCandidate>> = BTreeMap::new();
        for candidate in candidates {
            if let Some(job) = &candidate.job_origin {
                candidates_by_job
                    .entry(job.as_str())
                    .or_default()
                    .push(candidate);
            }
        }
        Self {
            candidates: candidates
                .iter()
                .map(|candidate| (candidate.candidate_id.as_str(), candidate))
                .collect(),
            revisions: revisions
                .iter()
                .map(|revision| (revision.revision, revision))
                .collect(),
            sources: sources
                .iter()
                .map(|source| (source.source_version.as_str(), source))
                .collect(),
            candidates_by_job,
        }
    }
}

#[derive(Default)]
struct SnapshotBudget {
    rows: usize,
    bytes: usize,
}
fn rows<T>(
    db: &Connection,
    sql: &str,
    project: &ProjectId,
    budget: &mut SnapshotBudget,
    mut convert: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> PResult<Vec<T>> {
    let mut statement = db.prepare(sql).map_err(internal)?;
    let mut cursor = statement.query([project.as_str()]).map_err(internal)?;
    let mut output = Vec::new();
    while let Some(row) = cursor.next().map_err(internal)? {
        budget.rows = budget
            .rows
            .checked_add(1)
            .ok_or_else(|| internal("capture count overflow"))?;
        for column in 0..row.as_ref().column_count() {
            let length = match row.get_ref(column).map_err(internal)? {
                rusqlite::types::ValueRef::Text(value) | rusqlite::types::ValueRef::Blob(value) => {
                    value.len()
                }
                _ => 16,
            };
            budget.bytes = budget
                .bytes
                .checked_add(length + 64)
                .ok_or_else(|| internal("capture size overflow"))?;
        }
        if budget.rows > MAX_CAPTURE_ROWS || budget.bytes > MAX_CAPTURE_METADATA {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "P1a capture metadata admission exceeded",
            ));
        }
        output.push(convert(row).map_err(internal)?);
    }
    Ok(output)
}
fn authenticate_capture(
    engine: &Engine,
    db: &Connection,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    expected: RevisionId,
) -> PResult<()> {
    engine.authenticate(db, session, Some(token))?;
    require_grant(db, session, project, Scope::PackageProject)?;
    let actual: Option<i64> = db
        .query_row(
            "SELECT revision FROM projects WHERE id=?1",
            [project.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    let actual =
        actual.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "project not found"))?;
    if revision(actual)? != expected {
        return Err(ProtocolError::new(
            ErrorCode::RevisionConflict,
            "package capture requires current expected revision",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditReceipt {
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

#[derive(Deserialize)]
struct WorkerReceiptEnvelope {
    schema_version: u16,
    recipe: String,
    project_id: ProjectId,
    base_revision: RevisionId,
    job_id: JobId,
    attempt_id: AttemptId,
    source_version: SourceVersionId,
    program: ArtifactIdentity,
    dependencies: Vec<ArtifactIdentity>,
}

/// Re-exported ancestors have inert worker manifests without local attempts.
/// Check the final object closure as well as the live history snapshot, before
/// returning a publication plan. Only bounded typed declaration bytes are read.
fn cpu_mode_bytes(object: &CapturedObject, limit: u64) -> PResult<Vec<u8>> {
    if object.descriptor.byte_len == 0 || object.descriptor.byte_len > limit {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "portable mode declaration exceeds byte admission",
        ));
    }
    let mut bytes = Vec::new();
    open_object(object)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| missing(&object.descriptor, "mode declaration read failed"))?;
    if bytes.len() as u64 != object.descriptor.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != object.descriptor.sha256
    {
        return Err(missing(
            &object.descriptor,
            "mode declaration identity changed",
        ));
    }
    Ok(bytes)
}
fn check_cpu_origin_objects(
    manifest: &PortableProjectManifest,
    objects: &BTreeMap<String, CapturedObject>,
) -> PResult<()> {
    for origin in &manifest.generated_origins {
        let object = objects
            .get(&origin.manifest.sha256)
            .filter(|object| {
                object.descriptor.byte_len == origin.manifest.byte_len
                    && object
                        .descriptor
                        .roles
                        .contains(&PackageObjectRole::WorkerManifest)
            })
            .ok_or_else(|| ProtocolError::invalid("portable CPU origin worker object absent"))?;
        let request: WorkerRequest = serde_json::from_slice(&cpu_mode_bytes(object, 1024 * 1024)?)
            .map_err(|_| ProtocolError::invalid("invalid portable worker mode declaration"))?;
        project_package_imports::checked_worker_scope(manifest, origin, &request)?;
    }
    Ok(())
}
fn require_portable_cpu_objects(
    manifest: &PortableProjectManifest,
    objects: &BTreeMap<String, CapturedObject>,
) -> PResult<()> {
    let mut index = crate::portable_cpu_archives::CpuArchiveIndex::default();
    index.observe_manifest(manifest)?;
    for object in objects.values().filter(|object| {
        object
            .descriptor
            .roles
            .contains(&PackageObjectRole::ImportedManifest)
    }) {
        let ancestor: PortableProjectManifest = serde_json::from_slice(&cpu_mode_bytes(
            object,
            MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
        )?)
        .map_err(|_| ProtocolError::invalid("invalid portable ancestor mode declaration"))?;
        index.observe_manifest(&ancestor)?;
    }
    // Check every role, including opaque inherited workers that have no local
    // attempt rows. A CPU tag needs an exact original declaration in this graph.
    for object in objects.values().filter(|object| {
        object
            .descriptor
            .roles
            .contains(&PackageObjectRole::WorkerManifest)
    }) {
        let request: WorkerRequest = serde_json::from_slice(&cpu_mode_bytes(object, 1024 * 1024)?)
            .map_err(|_| ProtocolError::invalid("invalid portable worker mode declaration"))?;
        index.check_worker(
            &PackageObjectRef {
                sha256: object.descriptor.sha256.clone(),
                byte_len: object.descriptor.byte_len,
            },
            &request,
        )?;
    }
    index.finish()?;
    check_cpu_origin_objects(manifest, objects)?;
    for object in objects.values().filter(|object| {
        object
            .descriptor
            .roles
            .contains(&PackageObjectRole::ImportedManifest)
    }) {
        let ancestor: PortableProjectManifest = serde_json::from_slice(&cpu_mode_bytes(
            object,
            MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
        )?)
        .map_err(|_| ProtocolError::invalid("invalid portable ancestor mode declaration"))?;
        check_cpu_origin_objects(&ancestor, objects)?;
    }
    Ok(())
}

fn read_compact(
    path: &Path,
    identity: &ArtifactIdentity,
    role: PackageObjectRole,
) -> PResult<Vec<u8>> {
    let descriptor = PackageObjectDescriptor {
        sha256: identity.sha256.clone(),
        byte_len: identity.byte_len,
        roles: vec![role],
    };
    if identity.byte_len > MAX_COMPACT_EVIDENCE {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "compact evidence exceeds capture bound",
        ));
    }
    let mut bytes = Vec::new();
    artifacts::open_regular(path)
        .map_err(|_| missing(&descriptor, "missing or unsafe evidence"))?
        .take(MAX_COMPACT_EVIDENCE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| missing(&descriptor, "evidence read failed"))?;
    if bytes.len() as u64 != identity.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != identity.sha256
    {
        return Err(missing(&descriptor, "evidence digest or length mismatch"));
    }
    Ok(bytes)
}

/// Private capture seam only: no request replay, operation readiness or transfer
/// authority is created. All record reads share one mutex-protected transaction.
#[cfg(test)]
pub(super) fn capture(
    engine: &Engine,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    expected_revision: RevisionId,
) -> PResult<ProjectCapturePlan> {
    capture_inner(engine, session, token, project, expected_revision, false)
}
/// Retains the captured closure before releasing the SQL snapshot. Full payload
/// verification is deferred to bounded materialization, not claimed by capture.
pub(super) fn capture_retained(
    engine: &Engine,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    expected_revision: RevisionId,
) -> PResult<ProjectCapturePlan> {
    capture_inner(engine, session, token, project, expected_revision, true)
}

fn capture_inner(
    engine: &Engine,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    expected_revision: RevisionId,
    retained: bool,
) -> PResult<ProjectCapturePlan> {
    let memory = engine.pool.reserve_memory(transfers::VALIDATION_MEMORY)?;
    let gate = if retained {
        Some(engine.artifact_gate.lock().map_err(internal)?)
    } else {
        None
    };
    let mut retained_holds = None;
    let root = &engine.config.state_dir;
    let (
        mut manifest,
        mut objects,
        legacy,
        authored,
        attempts,
        imported_archives,
        imported_aliases,
        mut cuda_members,
        mut cpu_runtimes,
        wasm_invocations,
    ) = {
        let mut db = engine.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
            .map_err(internal)?;
        authenticate_capture(engine, &tx, session, token, project, expected_revision)?;
        crate::annotated_generation_db::validate(&tx, &engine.pool).map_err(unavailable)?;
        let mut budget = SnapshotBudget::default();
        let (cuda_metadata_bytes, cuda_members) =
            crate::cuda_acceptance::portable_members(&tx, project).map_err(internal)?;
        budget.bytes = cuda_metadata_bytes;
        budget.rows = cuda_members.len();
        let mut objects = BTreeMap::new();
        let head: (String, i64, String, i64, String) = tx
            .query_row(
                "SELECT name,revision,program,history_cursor,protected FROM projects WHERE id=?1",
                [project.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(internal)?;
        let head = PackageProjectHead {
            name: head.0,
            revision: revision(head.1)?,
            motion: motion(&tx, root, &head.2, &mut objects)?,
            history_cursor: unsigned(head.3)?,
            protected: decode(&head.4)?,
        };
        let event_cursor: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(cursor),0) FROM events WHERE project=?1",
                [project.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let mut revisions = Vec::new();
        for (rev, actor, kind, label, program, protected) in rows(&tx,
            "SELECT revision,actor,kind,label,program,protected FROM revisions WHERE project=?1 ORDER BY revision",
            project, &mut budget, |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?)))? {
            revisions.push(PackageRevision {revision: revision(rev)?,actor,kind,label,
                motion: motion(&tx,root,&program,&mut objects)?,protected: decode(&protected)?});
        }
        let mut edit_states = Vec::new();
        for (position, program, protected, origin) in rows(&tx,
            "SELECT e.position,e.program,e.protected,h.revision FROM edit_states e LEFT JOIN history_lineage h ON h.project=e.project AND h.position=e.position WHERE e.project=?1 ORDER BY e.position",
            project, &mut budget, |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<i64>>(3)?)))? {
            edit_states.push(PackageEditState {position: unsigned(position)?,motion: motion(&tx,root,&program,&mut objects)?,
                protected:decode(&protected)?,source_revision:optional_revision(origin)?});
        }
        let mut revision_lineage = Vec::new();
        for (rev,parent,restored,position,candidate,operation,contributions,state) in rows(&tx,
            "SELECT revision,parent_revision,restored_from_revision,restored_history_position,candidate,operation,contributions,review_state FROM revision_lineage WHERE project=?1 ORDER BY revision",
            project,&mut budget,|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<i64>>(3)?,
                r.get::<_,Option<String>>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?)))? {
            let checked_review=review(&state)?;
            if !project_package_imports::imported_revision(&tx,project,revision(rev)?)? {for d in checked_review.decisions.iter().filter(|d|d.range.is_some()){review_interval_receipts::require(&tx,project,d)?;}}
            revision_lineage.push(PackageRevisionLineage {revision:revision(rev)?,parent_revision:optional_revision(parent)?,
                restored_from_revision:optional_revision(restored)?,restored_history_position:position.map(unsigned).transpose()?,
                candidate:candidate.map(CandidateId::new).transpose().map_err(internal)?,operation,
                contributions:decode(&contributions)?,review:checked_review});
        }
        let mut candidates = Vec::new();
        let mut rebase_parents:BTreeMap<String,CandidateId>=rows(&tx,"SELECT c.id,CASE WHEN typeof(b.parent)='text' AND length(CAST(b.parent AS BLOB)) BETWEEN 1 AND 128 THEN b.parent ELSE '' END FROM candidates c JOIN candidate_rebases b ON b.candidate=c.id WHERE c.project=?1 ORDER BY c.id",project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.into_iter().map(|(id,parent)|Ok((id,CandidateId::new(parent).map_err(internal)?))).collect::<PResult<_>>()?;
        for (id,base,program,job,lineage,committed,flags) in rows(&tx,
            "SELECT id,base_revision,program,job,lineage,committed_revision,review FROM candidates WHERE project=?1 ORDER BY id",
            project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,
                r.get::<_,String>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,String>(6)?)))? {
            candidates.push(PackageCandidate {rebased_from:rebase_parents.remove(&id),project_interval:candidate_generation_interval(&tx,&CandidateId::new(id.clone()).map_err(internal)?,project)?,candidate_id:CandidateId::new(id).map_err(internal)?,base_revision:revision(base)?,
                motion:motion(&tx,root,&program,&mut objects)?,job_origin:job.map(JobId::new).transpose().map_err(internal)?,
                lineage:decode(&lineage)?,committed_revision:optional_revision(committed)?,review:decode(&flags)?});
        }
        let mut sources = Vec::new();
        for (version,kind,label,hash,bytes,pinned,evicted,path) in rows(&tx,
            "SELECT version,kind,label,sha256,bytes,pinned,evicted,path FROM sources WHERE project=?1 ORDER BY version",
            project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,
                r.get::<_,i64>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?,r.get::<_,String>(7)?)))? {
            let identity=ArtifactIdentity {artifact_id:ArtifactId::new(hash.clone()).map_err(internal)?,sha256:hash.clone(),byte_len:unsigned(bytes)?};
            identity.validate()?;
            let expected_path=root.join("snapshots").join(&hash);
            let descriptor=PackageObjectDescriptor {sha256:hash,byte_len:identity.byte_len,roles:vec![PackageObjectRole::SourceSnapshot]};
            if evicted || Path::new(&path)!=expected_path {
                return Err(missing(&descriptor,"snapshot evicted or trusted locator unavailable"));
            }
            add_object(&mut objects,object_ref(&identity),PackageObjectRole::SourceSnapshot,ObjectLocation::File(expected_path))?;
            sources.push(PackageSource {source_version:SourceVersionId::new(version).map_err(internal)?,
                kind:decode(&encode(&kind)?)?,label,identity,pinned,evicted,stash_associations:vec![]});
        }
        // Original association receipts are compact inert provenance. Bound each
        // SQLite cell before String allocation, then bind every declaration to
        // its exact same-project source bytes. No remote or host path is opened.
        for row in rows(&tx,"SELECT version,scene,file,receipt FROM stash_sources WHERE project=?1 ORDER BY version,scene,file",project,&mut budget,|r| {
            let version=r.get_ref(0)?.as_str()?;let scene=r.get_ref(1)?.as_str()?;let file=r.get_ref(2)?.as_str()?;let receipt=r.get_ref(3)?.as_str()?;
            Ok((version.len()<=128 && scene.len()<=128 && file.len()<=128 && receipt.len()<=64*1024).then(||(version.to_owned(),scene.to_owned(),file.to_owned(),receipt.to_owned())))
        })? {
            let (version,scene,file,raw)=row.ok_or_else(||ProtocolError::new(ErrorCode::ResourceExhausted,"Stash association capture cell exceeds admitted bound"))?;
            let capture:StashSourceCapture=decode(&raw)?;
            let source=sources.iter_mut().find(|source|source.source_version.as_str()==version).ok_or_else(||ProtocolError::invalid("Stash association source is absent"))?;
            if capture.scene_id!=scene || capture.file_id!=file {return Err(ProtocolError::invalid("Stash association row identity differs from receipt"));}
            let capture_receipt=PackageObjectRef {sha256:format!("{:x}",Sha256::digest(raw.as_bytes())),byte_len:raw.len() as u64};
            let association=PackageStashAssociation {endpoint:capture.endpoint.clone(),scene_id:scene,file_id:file,server_path:capture.server_path.clone(),capture_receipt:capture_receipt.clone()};
            association.check_capture(source,&capture)?;
            if source.stash_associations.len()>=16 {return Err(ProtocolError::new(ErrorCode::ResourceExhausted,"source Stash association limit reached"));}
            source.stash_associations.push(association);
            add_object(&mut objects,capture_receipt,PackageObjectRole::DependencyEvidence,ObjectLocation::Inline(Arc::from(raw.into_bytes())))?;
        }
        for source in &mut sources {
            source
                .stash_associations
                .sort_by(|a, b| a.capture_receipt.sha256.cmp(&b.capture_receipt.sha256));
        }
        let wasm_invocations = wasm_edits::portable_invocations(&tx, project)?;
        for captured in wasm_invocations.values() {
            budget.bytes = budget
                .bytes
                .checked_add(captured.module.len())
                .and_then(|n| n.checked_add(captured.input.len()))
                .and_then(|n| n.checked_add(16 * 1024))
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "WASM capture metadata overflow",
                    )
                })?;
            if budget.bytes > MAX_CAPTURE_METADATA {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "WASM capture exceeds metadata admission",
                ));
            }
        }
        let authored=rows(&tx,
            "SELECT l.candidate,l.receipt,l.review_state,l.authored_count,l.inherited_count FROM edit_candidate_lineage l JOIN candidates c ON c.id=l.candidate WHERE c.project=?1 ORDER BY l.candidate",
            project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?)))?;
        let legacy=rows(&tx,
            "SELECT owner_table,row_key,program FROM legacy_motion_bytes WHERE (owner_table='projects' AND row_key=?1) OR (owner_table IN ('revisions','edit_states') AND substr(row_key,1,length(?1)+1)=?1||':' AND instr(substr(row_key,length(?1)+2),':')=0) OR (owner_table='candidates' AND row_key IN (SELECT id FROM candidates WHERE project=?1)) ORDER BY owner_table,row_key",
            project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?;
        let attempts=rows(&tx,
            "SELECT a.id,a.job,a.manifest,a.state FROM attempts a JOIN jobs j ON j.id=a.job WHERE j.project=?1 ORDER BY a.job,a.id",
            project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?;
        let has_cpu_table: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name='cpu_runtime_attempts' COLLATE NOCASE)",
                [],
                |row| row.get(0),
            )
            .map_err(internal)?;
        let mut fallback_tables = Vec::new();
        for table in [
            "inference_fallback_jobs",
            "inference_fallback_attempts",
            "inference_fallback_pending",
        ] {
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name=?1 COLLATE NOCASE)",
                    [table],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(internal)?
            {
                fallback_tables.push(table);
            }
        }
        let mut cpu_runtimes = BTreeMap::new();
        for (attempt, job, raw, _) in &attempts {
            let request: WorkerRequest = decode(raw)?;
            request.validate()?;
            let attempt_id = AttemptId::new(attempt).map_err(internal)?;
            let job_id = JobId::new(job).map_err(internal)?;
            if request.project_id != *project
                || request.attempt_id != attempt_id
                || request.job_id != job_id
                || request.output_dir != root.join("attempts").join(attempt_id.as_str())
            {
                return Err(ProtocolError::invalid(
                    "historical worker manifest identity mismatch",
                ));
            }
            crate::portable_cpu_archives::require_no_fallback(&request.dependencies)?;
            for table in &fallback_tables {
                // Only fixed module-owned names select SQL. Membership is bound
                // to this actual captured job/attempt, including pending intent.
                let sql = if *table == "inference_fallback_attempts" {
                    "SELECT EXISTS(SELECT 1 FROM main.inference_fallback_attempts WHERE job=?1 OR attempt=?2)".to_owned()
                } else {
                    format!("SELECT EXISTS(SELECT 1 FROM main.{table} WHERE job=?1)")
                };
                let declared = if *table == "inference_fallback_attempts" {
                    tx.query_row(
                        &sql,
                        params![request.job_id.as_str(), request.attempt_id.as_str()],
                        |row| row.get::<_, bool>(0),
                    )
                } else {
                    tx.query_row(&sql, [request.job_id.as_str()], |row| row.get::<_, bool>(0))
                }
                .map_err(internal)?;
                if declared {
                    super::inference_fallback_store::retained(&tx, &request)?;
                    return Err(ProtocolError::unsupported(
                        "portable inference fallback history is not implemented",
                    ));
                }
            }
            // Preserve legacy capture semantics. Consult the durable mode owner
            // only when this attempt actually has a retained declaration.
            let has_cpu_row = has_cpu_table
                && tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM main.cpu_runtime_attempts WHERE attempt=?1)",
                        [request.attempt_id.as_str()],
                        |row| row.get::<_, bool>(0),
                    )
                    .map_err(internal)?;
            let cpu_runtime = if has_cpu_row {
                Some(Box::new(
                    crate::cpu_runtime_db::retained(&tx, &request)
                        .map_err(internal)?
                        .ok_or_else(|| {
                            ProtocolError::invalid("portable CPU attempt row returned no image")
                        })?,
                ))
            } else {
                crate::project_package_import_plan::require_portable_cpu_dependencies(
                    &request.dependencies,
                )?;
                None
            };
            if let Some(image) = &cpu_runtime {
                budget.bytes = budget
                    .bytes
                    .checked_add(image.identity()?.byte_len as usize)
                    .filter(|bytes| *bytes <= MAX_CAPTURE_METADATA)
                    .ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "CPU archive metadata admission exceeded",
                        )
                    })?;
            }
            if cpu_runtimes
                .insert((attempt_id, job_id, raw.clone()), cpu_runtime)
                .is_some()
            {
                return Err(ProtocolError::invalid(
                    "duplicate captured worker attempt declaration",
                ));
            }
        }
        let mut export_receipts = Vec::new();
        for (rev,axis,hash,state,path) in rows(&tx,
            "SELECT revision,axis,sha256,state,path FROM exports WHERE project=?1 ORDER BY revision,axis,sha256,state,path",
            project,&mut budget,|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))? {
            let state=match state.as_str() {"pending"=>PackageExportState::Pending,"complete"=>PackageExportState::Complete,
                "failed_or_unknown"=>PackageExportState::FailedOrUnknown,_=>return Err(ProtocolError::unsupported("unknown archival export state"))};
            export_receipts.push(PackageExportReceipt {revision:revision(rev)?,axis:decode(&encode(&axis)?)?,sha256:hash,state,archival_path:Some(path)});
        }
        let imported_archives = rows(
            &tx,
            "SELECT manifest FROM project_import_archives WHERE project=?1",
            project,
            &mut budget,
            |row| row.get::<_, String>(0),
        )?;
        let imported_aliases = rows(&tx, "SELECT candidate,imported_candidate FROM project_import_candidate_aliases WHERE project=?1 ORDER BY candidate", project, &mut budget, |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?;
        let mut definitions = Vec::new();
        for (id,source,raw) in rows(&tx,"SELECT id,source,descriptor FROM source_placement_definitions WHERE project=?1 ORDER BY id",project,&mut budget,|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {
            let definition:SourcePlacement=decode(&raw)?;
            if definition.placement_id.as_str()!=id || definition.spec.source_version.as_str()!=source {
                return Err(ProtocolError::invalid("stored placement identity/source binding mismatch"));
            }
            definitions.push(definition);
        }
        let placements = if definitions.is_empty() {
            None
        } else {
            let mut placement_revisions = Vec::new();
            for (rev,raw) in rows(&tx,"SELECT revision,ids FROM source_placement_revisions WHERE project=?1 ORDER BY revision",project,&mut budget,|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))? {
                placement_revisions.push(PackageRevisionPlacements{revision:revision(rev)?,placements:decode(&raw)?});
            }
            let mut placement_history = Vec::new();
            for (position,raw) in rows(&tx,"SELECT position,ids FROM source_placement_history WHERE project=?1 ORDER BY position",project,&mut budget,|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))? {
                placement_history.push(PackageHistoryPlacements{position:unsigned(position)?,placements:decode(&raw)?});
            }
            Some(PackagePlacementClosure {
                definitions,
                active: source_placements::snapshot(&tx, project)?
                    .into_iter()
                    .map(|d| d.placement_id)
                    .collect(),
                revisions: placement_revisions,
                history: placement_history,
            })
        };
        let mut retiming_receipts = Vec::new();
        for rev in rows(
            &tx,
            "SELECT revision FROM motion_retime_receipts WHERE project=?1 ORDER BY revision",
            project,
            &mut budget,
            |r| r.get::<_, i64>(0),
        )? {
            retiming_receipts.push(motion_retime_receipts::load_verified(
                &tx,
                project,
                revision(rev)?,
                &engine.pool,
            )?);
        }
        let manifest = PortableProjectManifest {
            retiming_receipts,
            placements,
            format_version: if source_placements::has_definitions(&tx, project)? {
                PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
            } else {
                PROJECT_PACKAGE_FORMAT_VERSION
            },
            imported_origins: vec![],
            origin_project_id: project.clone(),
            captured_revision: expected_revision,
            captured_event_cursor: unsigned(event_cursor)?,
            completeness: PackageCompleteness::default(),
            counts: PackageRecordCounts::default(),
            head,
            revisions,
            edit_states,
            revision_lineage,
            candidates,
            authored_lineage: vec![],
            sources,
            legacy_cells: vec![],
            generated_origins: vec![],
            export_receipts,
            objects: vec![],
        };
        if retained {
            let mut identities = objects
                .values()
                .map(|object| ArtifactIdentity {
                    artifact_id: ArtifactId::new(format!(
                        "package.hold.{}",
                        object.descriptor.sha256
                    ))
                    .expect("digest identifier"),
                    sha256: object.descriptor.sha256.clone(),
                    byte_len: object.descriptor.byte_len,
                })
                .collect::<Vec<_>>();
            identities.extend(
                manifest
                    .candidates
                    .iter()
                    .flat_map(|candidate| candidate.lineage.iter().cloned()),
            );
            retained_holds = Some(engine.package_holds.acquire(&identities)?);
        }
        tx.commit().map_err(internal)?;
        (
            manifest,
            objects,
            legacy,
            authored,
            attempts,
            imported_archives,
            imported_aliases,
            cuda_members,
            cpu_runtimes,
            wasm_invocations,
        )
    };
    drop(gate);
    let imported_candidates = merge_imported_capture(
        root,
        &mut manifest,
        &mut objects,
        imported_archives,
        imported_aliases,
    )?;
    // No external object read or hash occurs while the authority mutex is held.
    // Snapshot/source holds already exist; receipt/input/attempt files have no deletion path.
    for (owner, key, raw) in legacy {
        let owner = match owner.as_str() {
            "projects" => PackageLegacyOwner::Projects,
            "candidates" => PackageLegacyOwner::Candidates,
            "revisions" | "edit_states" => {
                let Some((owner_id, suffix)) = key.rsplit_once(':') else {
                    return Err(ProtocolError::invalid("malformed legacy recovery key"));
                };
                if owner_id != project.as_str() {
                    continue;
                }
                if suffix.is_empty()
                    || !suffix.bytes().all(|byte| byte.is_ascii_digit())
                    || suffix.parse::<u64>().is_err()
                {
                    return Err(ProtocolError::invalid(
                        "malformed legacy recovery coordinate",
                    ));
                }
                if owner == "revisions" {
                    PackageLegacyOwner::Revisions
                } else {
                    PackageLegacyOwner::EditStates
                }
            }
            _ => return Err(ProtocolError::unsupported("unknown legacy recovery owner")),
        };
        let object = inline_object(
            &mut objects,
            raw.into_bytes(),
            PackageObjectRole::LegacyMotionBytes,
        )?;
        manifest.legacy_cells.push(PackageLegacyCell {
            owner,
            row_key: key,
            object,
        });
    }
    manifest
        .legacy_cells
        .sort_by(|a, b| (&a.owner, &a.row_key).cmp(&(&b.owner, &b.row_key)));
    let indexes = CaptureIndexes::new(&manifest.candidates, &manifest.revisions, &manifest.sources);
    for (candidate, identity, state, authored_count, inherited_count) in authored {
        let candidate_id = CandidateId::new(candidate).map_err(internal)?;
        let identity: ArtifactIdentity = decode(&identity)?;
        identity.validate()?;
        let path = root.join("edit-receipts").join(&identity.sha256);
        let bytes = read_compact(&path, &identity, PackageObjectRole::EditReceipt)?;
        let receipt: EditReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| ProtocolError::invalid("unsupported authored receipt schema"))?;
        let candidate = indexes
            .candidates
            .get(candidate_id.as_str())
            .copied()
            .ok_or_else(|| ProtocolError::invalid("authored lineage candidate is absent"))?;
        let base = indexes
            .revisions
            .get(&receipt.base_revision)
            .copied()
            .ok_or_else(|| ProtocolError::invalid("authored receipt origin revision is absent"))?;
        if receipt.format != "pulsar-edit-receipt-v1"
            || receipt.kernel != pulsar_core::EDIT_VALUES_KERNEL_VERSION
            || receipt.project_id != *project
            || receipt.base_program != base.motion
            || receipt.result_program != candidate.motion
            || receipt.authored_ranges.len() as u64 != unsigned(authored_count)?
            || receipt.inherited_ranges.len() as u64 != unsigned(inherited_count)?
        {
            return Err(ProtocolError::invalid(
                "authored receipt lineage disagrees with captured graph",
            ));
        }
        for (_, range) in receipt
            .authored_ranges
            .iter()
            .chain(&receipt.inherited_ranges)
        {
            if range.start() < ProjectTime::ZERO || range.start() >= range.end() {
                return Err(ProtocolError::invalid("invalid authored receipt interval"));
            }
        }
        let _inert_attribution = (
            &receipt.lease_id,
            &receipt.authored_provenance,
            &receipt.actor_id,
            &receipt.label,
        );
        let input = PackageObjectRef {
            sha256: receipt.upload_sha256,
            byte_len: receipt.upload_byte_len,
        };
        add_object(
            &mut objects,
            input.clone(),
            PackageObjectRole::EditInput,
            ObjectLocation::File(root.join("edit-inputs").join(&input.sha256)),
        )?;
        add_object(
            &mut objects,
            object_ref(&identity),
            PackageObjectRole::EditReceipt,
            ObjectLocation::File(path),
        )?;
        let wasm_invocation = wasm_invocations
            .get(&identity.sha256)
            .map(|captured| -> PResult<_> {
                add_object(
                    &mut objects,
                    object_ref(&captured.metadata.manifest.artifact),
                    PackageObjectRole::WasmModule,
                    ObjectLocation::Inline(Arc::from(captured.module.clone())),
                )?;
                add_object(
                    &mut objects,
                    captured.metadata.input.clone(),
                    PackageObjectRole::WasmInput,
                    ObjectLocation::Inline(Arc::from(captured.input.clone())),
                )?;
                Ok(captured.metadata.clone())
            })
            .transpose()?;
        manifest.authored_lineage.push(PackageAuthoredLineage {
            wasm_invocation,
            candidate_id,
            receipt: identity,
            input,
            review: review(&state)?,
            authored_count: unsigned(authored_count)?,
            inherited_count: unsigned(inherited_count)?,
        });
    }
    let mut dependency_identities = BTreeSet::new();
    for (attempt, job, raw, state) in attempts {
        let request: WorkerRequest = decode(&raw)?;
        request.validate()?;
        crate::portable_cpu_archives::require_no_fallback(&request.dependencies)?;
        let annotated_model = match &request.operation {
            WorkerOperation::Generate {
                settings,
                model: Some(model),
                ..
            } if settings.annotated_view.is_some() => {
                if model.identity.byte_len == 0 || model.identity.byte_len > 256 * 1024 * 1024 {
                    return Err(ProtocolError::invalid(
                        "unsupported annotated portable model size",
                    ));
                }
                add_object(
                    &mut objects,
                    object_ref(&model.identity),
                    PackageObjectRole::AnnotatedModel,
                    ObjectLocation::File(model.path.clone()),
                )?;
                Some(model.identity.clone())
            }
            _ => None,
        };
        let attempt_id = AttemptId::new(attempt).map_err(internal)?;
        let job_id = JobId::new(job).map_err(internal)?;
        if request.project_id != *project
            || request.attempt_id != attempt_id
            || request.job_id != job_id
            || request.output_dir != root.join("attempts").join(attempt_id.as_str())
        {
            return Err(ProtocolError::invalid(
                "historical worker manifest identity mismatch",
            ));
        }
        let cpu_runtime = cpu_runtimes
            .remove(&(attempt_id.clone(), job_id.clone(), raw.clone()))
            .ok_or_else(|| {
                ProtocolError::invalid("portable CPU declaration lacks exact captured attempt")
            })?;
        let primary = indexes
            .sources
            .get(request.source.source_version.as_str())
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    format!(
                        "required primary source {} is absent from the captured catalog",
                        request.source.source_version
                    ),
                )
            })?;
        if primary.identity != request.source.identity
            || !objects.get(&primary.identity.sha256).is_some_and(|object| {
                object.descriptor.byte_len == primary.identity.byte_len
                    && object
                        .descriptor
                        .roles
                        .contains(&PackageObjectRole::SourceSnapshot)
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                format!(
                    "primary source {} identity differs from the captured source payload",
                    request.source.source_version
                ),
            ));
        }
        let image_origin = if let Some((identity, bytes, image)) =
            assistant_image_origin::retained(root, &request)?
        {
            let original = indexes
                .sources
                .get(image.source_version.as_str())
                .ok_or_else(|| {
                    ProtocolError::invalid("image origin source is absent from package")
                })?;
            if original.kind != SourceKind::Media || original.identity != image.source {
                return Err(ProtocolError::invalid(
                    "image origin source differs from package catalog",
                ));
            }
            add_object(
                &mut objects,
                object_ref(&identity),
                PackageObjectRole::DependencyEvidence,
                ObjectLocation::Inline(Arc::from(bytes)),
            )?;
            add_object(
                &mut objects,
                object_ref(&image.rgb),
                PackageObjectRole::DependencyEvidence,
                ObjectLocation::File(root.join("snapshots").join(&image.rgb.sha256)),
            )?;
            Some(PackageImageOrigin {
                record: identity,
                rgb: image.rgb,
            })
        } else {
            None
        };
        let mut allowed_dependencies = request.dependencies.clone();
        if let Some(audio) = &request.independent_audio {
            let captured = indexes
                .sources
                .get(audio.source_version.as_str())
                .ok_or_else(|| {
                    ProtocolError::invalid("independent audio absent from captured source catalog")
                })?;
            if captured.kind != SourceKind::Media
                || captured.identity != audio.identity
                || !objects.get(&audio.identity.sha256).is_some_and(|object| {
                    object.descriptor.byte_len == audio.identity.byte_len
                        && object
                            .descriptor
                            .roles
                            .contains(&PackageObjectRole::SourceSnapshot)
                })
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "independent audio differs from captured source payload",
                ));
            }
            allowed_dependencies.push(audio.identity.clone());
        }
        allowed_dependencies.push(request.source.identity.clone());
        allowed_dependencies.push(request.tools.ffmpeg.identity.clone());
        allowed_dependencies.push(request.tools.ffprobe.identity.clone());
        if let Some(runtime) = &request.tools.onnx_runtime {
            allowed_dependencies.push(runtime.identity.clone());
        }
        if let WorkerOperation::Generate {
            model: Some(model), ..
        }
        | WorkerOperation::Preview {
            model: Some(model), ..
        } = &request.operation
        {
            allowed_dependencies.push(model.identity.clone());
        }
        for identity in &allowed_dependencies {
            identity.validate()?;
            dependency_identities.insert((identity.sha256.clone(), identity.byte_len));
        }
        let candidates = indexes
            .candidates_by_job
            .get(job_id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let cuda = cuda_members.remove(&attempt_id);
        if cuda.is_some() && (request.cuda_execution.is_none() || state != "completed")
            || request.cuda_execution.is_some() && state == "completed" && cuda.is_none()
        {
            return Err(ProtocolError::invalid(
                "portable CUDA completion lacks exact accepted membership",
            ));
        }
        let cuda_evidence = cuda.as_ref().map(|(evidence, _)| evidence.clone());
        let mut receipt = None;
        let mut original_program_identity = None;
        if !candidates.is_empty() && state == "completed" {
            for candidate in candidates.iter() {
                let identity = match &cuda_evidence {
                    Some(cuda) => cuda.general_receipt(&candidate.lineage)?,
                    None => candidate.lineage.last().ok_or_else(|| {
                        ProtocolError::invalid("generated candidate receipt identity is absent")
                    })?,
                };
                if receipt.as_ref().is_some_and(|old| old != identity) {
                    return Err(ProtocolError::invalid(
                        "generated candidates disagree on receipt identity",
                    ));
                }
                receipt = Some(identity.clone());
            }
        }
        if let Some(identity) = &receipt {
            let attempt_root = root.join("attempts").join(attempt_id.as_str());
            let path = attempt_root.join("receipt.json");
            let raw_receipt = read_compact(&path, identity, PackageObjectRole::WorkerReceipt)?;
            let envelope: WorkerReceiptEnvelope = serde_json::from_slice(&raw_receipt)
                .map_err(|_| ProtocolError::unsupported("unsupported generated receipt schema"))?;
            if (if annotated_model.is_some() {
                envelope.schema_version != 3
            } else {
                !matches!(envelope.schema_version, 1 | 2)
            }) || envelope.recipe.is_empty()
                || envelope.project_id != *project
                || envelope.job_id != job_id
                || envelope.attempt_id != attempt_id
                || envelope.base_revision != request.base_revision
                || envelope.source_version != request.source.source_version
            {
                return Err(ProtocolError::invalid(
                    "generated receipt disagrees with captured origin",
                ));
            }
            for dependency in &envelope.dependencies {
                dependency.validate()?;
                if !allowed_dependencies.contains(dependency) {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        format!(
                            "unresolved generated receipt dependency {}",
                            dependency.artifact_id
                        ),
                    ));
                }
            }
            for candidate in candidates.iter() {
                let suffix = if cuda_evidence.is_some() { 3 } else { 1 };
                if annotated_model.is_some() {
                    crate::worker::annotated_receipt::verify_file(
                        &request,
                        &envelope.program,
                        &ArtifactReference {
                            path: path.clone(),
                            identity: identity.clone(),
                        },
                        &candidate.lineage[..candidate.lineage.len() - suffix],
                        &candidate.review,
                    )?;
                }
                if candidate.lineage[..candidate.lineage.len() - suffix] != envelope.dependencies {
                    return Err(ProtocolError::invalid(
                        "generated receipt dependency list differs from candidate lineage",
                    ));
                }
            }
            envelope.program.validate()?;
            original_program_identity = Some(envelope.program.clone());
            if envelope.program.byte_len > MAX_MOTION_BYTES {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "original worker motion exceeds motion admission",
                ));
            }
            let program_path = attempt_root.join("program.json");
            let raw_program = read_compact(
                &program_path,
                &envelope.program,
                PackageObjectRole::DependencyEvidence,
            )?;
            let original_program: MotionProgram =
                serde_json::from_slice(&raw_program).map_err(|_| {
                    ProtocolError::invalid("original worker program is not checked motion")
                })?;
            let mut checked = BTreeSet::new();
            for candidate in candidates.iter() {
                if checked.insert(candidate.motion.sha256.clone())
                    && engine.motion_store.read(&candidate.motion)? != original_program
                {
                    return Err(ProtocolError::invalid(
                        "original worker program differs from canonical candidate motion",
                    ));
                }
            }
            add_object(
                &mut objects,
                object_ref(&envelope.program),
                PackageObjectRole::DependencyEvidence,
                ObjectLocation::File(program_path),
            )?;
            add_object(
                &mut objects,
                object_ref(identity),
                PackageObjectRole::WorkerReceipt,
                ObjectLocation::File(path),
            )?;
        }
        if let Some((_, artifacts)) = cuda {
            for (reference, role) in [
                (&artifacts.receipt, PackageObjectRole::CudaExecutionReceipt),
                (&artifacts.trace, PackageObjectRole::CudaNodeTrace),
            ] {
                add_object(
                    &mut objects,
                    object_ref(&reference.identity),
                    role,
                    ObjectLocation::File(reference.path.clone()),
                )?;
            }
        }
        let object = inline_object(
            &mut objects,
            raw.into_bytes(),
            PackageObjectRole::WorkerManifest,
        )?;
        manifest.generated_origins.push(PackageGeneratedOrigin {
            cpu_runtime,
            image_origin,
            annotated_model,
            cuda_evidence,
            job_id,
            attempt_id,
            base_revision: request.base_revision,
            source_version: request.source.source_version,
            manifest: object,
            receipt,
            program: original_program_identity,
            state,
        });
    }
    if !cuda_members.is_empty() {
        return Err(ProtocolError::invalid(
            "portable CUDA membership has no captured attempt",
        ));
    }
    if !cpu_runtimes.is_empty() {
        return Err(ProtocolError::invalid(
            "portable CPU declaration has no captured attempt",
        ));
    }
    crate::project_package_import_plan::require_portable_cpu_manifest(&manifest)?;
    require_portable_cpu_objects(&manifest, &objects)?;
    // Flat lineage may name executable dependencies without packaging them. Every
    // other declared identity must resolve to an explicitly required payload.
    for candidate in &manifest.candidates {
        if imported_candidates.contains(candidate.candidate_id.as_str()) {
            continue;
        }
        for identity in &candidate.lineage {
            identity.validate()?;
            if !objects
                .get(&identity.sha256)
                .is_some_and(|object| object.descriptor.byte_len == identity.byte_len)
                && !dependency_identities.contains(&(identity.sha256.clone(), identity.byte_len))
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    format!(
                        "unresolved required candidate evidence {}",
                        identity.artifact_id
                    ),
                ));
            }
        }
    }
    let mut receipts = manifest
        .export_receipts
        .into_iter()
        .map(|record| Ok((serde_json::to_vec(&record).map_err(internal)?, record)))
        .collect::<PResult<Vec<_>>>()?;
    receipts.sort_by(|a, b| a.0.cmp(&b.0));
    manifest.export_receipts = receipts.into_iter().map(|(_, record)| record).collect();
    manifest.objects = objects
        .values()
        .map(|object| object.descriptor.clone())
        .collect();
    manifest.format_version = manifest.required_format_version();
    manifest.counts = manifest.record_counts()?;
    manifest.validate()?;
    let total = manifest
        .objects
        .iter()
        .try_fold(0u64, |total, object| total.checked_add(object.byte_len))
        .ok_or_else(|| ProtocolError::invalid("package byte accounting overflow"))?;
    if total > MAX_PACKAGE_BYTES {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "P1a package byte admission exceeded",
        ));
    }
    if let Some(holds) = retained_holds.as_mut() {
        let _gate = engine.artifact_gate.lock().map_err(internal)?;
        let identities = objects
            .values()
            .map(|object| ArtifactIdentity {
                artifact_id: ArtifactId::new(format!("package.hold.{}", object.descriptor.sha256))
                    .expect("digest identifier"),
                sha256: object.descriptor.sha256.clone(),
                byte_len: object.descriptor.byte_len,
            })
            .collect::<Vec<_>>();
        holds.extend(&identities)?;
    }
    if !retained {
        for object in objects.values() {
            let mut reader = open_object(object)?;
            let mut hash = Sha256::new();
            let mut length = 0u64;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(read) => read,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(missing(&object.descriptor, "evidence read failed")),
                };
                if read == 0 {
                    break;
                }
                length = length
                    .checked_add(read as u64)
                    .ok_or_else(|| missing(&object.descriptor, "length overflow"))?;
                if length > object.descriptor.byte_len {
                    return Err(missing(
                        &object.descriptor,
                        "evidence exceeds declared length",
                    ));
                }
                hash.update(&buffer[..read]);
            }
            if length != object.descriptor.byte_len
                || format!("{:x}", hash.finalize()) != object.descriptor.sha256
            {
                return Err(missing(
                    &object.descriptor,
                    "evidence digest or length mismatch",
                ));
            }
        }
    }
    {
        let db = engine.db.lock().map_err(internal)?;
        if retained {
            engine.authenticate(&db, session, Some(token))?;
            require_grant(&db, session, project, Scope::PackageProject)?;
        } else {
            authenticate_capture(engine, &db, session, token, project, expected_revision)?;
        }
    }
    let manifest_sha256 = format!("{:x}", Sha256::digest(manifest.canonical_bytes()?));
    Ok(ProjectCapturePlan {
        manifest,
        manifest_sha256,
        objects,
        _holds: retained_holds,
        _memory: memory,
    })
}

fn merge_imported_capture(
    root: &Path,
    manifest: &mut PortableProjectManifest,
    objects: &mut BTreeMap<String, CapturedObject>,
    archives: Vec<String>,
    aliases: Vec<(String, String)>,
) -> PResult<BTreeSet<String>> {
    let mut imported_candidates = BTreeSet::new();
    for raw in archives {
        let archived: PortableProjectManifest = decode(&raw)?;
        archived.validate()?;
        if archived.origin_project_id != manifest.origin_project_id {
            return Err(ProtocolError::invalid(
                "stored imported archive project mismatch",
            ));
        }
        for old in &archived.candidates {
            if let Some(parent) = &old.rebased_from {
                let current = manifest
                    .candidates
                    .iter_mut()
                    .find(|c| c.candidate_id == old.candidate_id)
                    .ok_or_else(|| {
                        ProtocolError::invalid("retained rebase candidate disappeared")
                    })?;
                if current.rebased_from.as_ref().is_some_and(|p| p != parent) {
                    return Err(ProtocolError::invalid(
                        "retained rebase correspondence changed",
                    ));
                }
                current.rebased_from = Some(parent.clone());
            }
        }
        manifest.format_version = if manifest.placements.is_some() {
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_PLACEMENTS
        } else {
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_ORIGINS
        };
        manifest.imported_origins = archived.imported_origins;
        manifest.legacy_cells.extend(archived.legacy_cells);
        manifest.export_receipts.extend(archived.export_receipts);
        for original in &archived.sources {
            if original.stash_associations.is_empty() {
                continue;
            }
            let local = manifest
                .sources
                .iter_mut()
                .find(|s| s.source_version == original.source_version)
                .ok_or_else(|| {
                    ProtocolError::invalid("archival Stash source absent during re-export")
                })?;
            if local.identity != original.identity || local.kind != original.kind {
                return Err(ProtocolError::invalid(
                    "archival Stash source identity changed during re-export",
                ));
            }
            for association in &original.stash_associations {
                if !local.stash_associations.contains(association) {
                    local.stash_associations.push(association.clone());
                }
            }
            local
                .stash_associations
                .sort_by(|a, b| a.capture_receipt.sha256.cmp(&b.capture_receipt.sha256));
            if local.stash_associations.len() > 16 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "merged Stash association limit reached",
                ));
            }
        }
        for origin in &mut manifest.imported_origins {
            let original: BTreeMap<_, _> = origin
                .candidates
                .iter()
                .map(|map| (map.local.to_string(), map.origin.clone()))
                .collect();
            for (candidate, imported) in &aliases {
                let Some(target) = original.get(imported) else {
                    continue;
                };
                imported_candidates.insert(candidate.clone());
                if !original.contains_key(candidate) {
                    origin.candidates.push(PackageCandidateOrigin {
                        local: CandidateId::new(candidate).map_err(internal)?,
                        origin: target.clone(),
                    });
                }
            }
            origin.candidates.sort_by(|a, b| a.local.cmp(&b.local));
            for object in &origin.objects {
                let directory = if object.roles.contains(&PackageObjectRole::MotionProgram) {
                    "motion-objects"
                } else if object.roles.contains(&PackageObjectRole::SourceSnapshot) {
                    "snapshots"
                } else {
                    "imported-project-evidence"
                };
                for role in &object.roles {
                    add_object(
                        objects,
                        PackageObjectRef {
                            sha256: object.sha256.clone(),
                            byte_len: object.byte_len,
                        },
                        *role,
                        ObjectLocation::File(root.join(directory).join(&object.sha256)),
                    )?;
                }
            }
            add_object(
                objects,
                origin.manifest.clone(),
                PackageObjectRole::ImportedManifest,
                ObjectLocation::File(
                    root.join("imported-project-evidence")
                        .join(&origin.manifest.sha256),
                ),
            )?;
        }
        for cell in &manifest.legacy_cells {
            add_object(
                objects,
                cell.object.clone(),
                PackageObjectRole::LegacyMotionBytes,
                ObjectLocation::File(
                    root.join("imported-project-evidence")
                        .join(&cell.object.sha256),
                ),
            )?;
        }
    }
    Ok(imported_candidates)
}

#[cfg(test)]
mod capture_review_tests {
    use super::*;

    struct Fixture {
        engine: Arc<Engine>,
        session: SessionId,
        token: String,
        project: ProjectId,
        _dir: tempfile::TempDir,
    }
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("nonexecuted-worker");
        fs::write(&executable, b"capture review fixture, never executed").unwrap();
        let engine = Engine::open(EngineConfig::new(dir.path().join("state"), executable)).unwrap();
        let (session, token) = match engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "capture review".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Paired {
                session,
                auth_token,
            } => (session, auth_token),
            _ => panic!("paired response required"),
        };
        let project = match engine
            .handle(
                Request::new(
                    id!(RequestId),
                    Command::CreateProject {
                        name: "capture review".into(),
                    },
                )
                .with_session(session.clone())
                .with_auth_token(token.clone()),
            )
            .result
            .unwrap()
        {
            ResponseBody::Project(snapshot) => snapshot.project_id,
            _ => panic!("project response required"),
        };
        Fixture {
            engine,
            session,
            token,
            project,
            _dir: dir,
        }
    }
    impl Fixture {
        fn capture(&self) -> PResult<ProjectCapturePlan> {
            capture(
                &self.engine,
                &self.session,
                &self.token,
                &self.project,
                RevisionId::new(0),
            )
        }
        fn import(&self, bytes: &[u8]) -> SourceArtifact {
            let path = self._dir.path().join("source");
            fs::write(&path, bytes).unwrap();
            let version = match self
                .engine
                .handle(
                    Request::new(id!(RequestId), Command::ImportSource { path })
                        .with_session(self.session.clone())
                        .with_auth_token(self.token.clone())
                        .in_project(self.project.clone(), Some(RevisionId::new(0))),
                )
                .result
                .unwrap()
            {
                ResponseBody::Source { source_version } => source_version,
                _ => panic!("source response required"),
            };
            self.engine
                .source(&self.engine.db.lock().unwrap(), &self.project, &version)
                .unwrap()
        }
        fn failed_origin(&self, source: SourceArtifact) -> WorkerRequest {
            let job_id = id!(JobId);
            let attempt_id = id!(AttemptId);
            let tool = ArtifactReference {
                identity: self.engine.binary_identity.clone(),
                path: self.engine.config.worker_executable.clone(),
            };
            let manifest = WorkerRequest {
                cuda_execution: None,
                independent_audio: None,
                version: PROTOCOL_VERSION,
                job_id,
                attempt_id: attempt_id.clone(),
                project_id: self.project.clone(),
                base_revision: RevisionId::new(0),
                dependencies: vec![source.identity.clone()],
                source,
                output_dir: self
                    .engine
                    .config
                    .state_dir
                    .join("attempts")
                    .join(attempt_id.as_str()),
                operation: WorkerOperation::Generate {
                    preset: "fixture".into(),
                    settings: (GenerationSettings::default()).into(),
                    model: None,
                },
                budget: ResourceBudget {
                    memory_bytes: 1,
                    output_bytes: 1,
                    wall_time_ms: 1,
                    cpu_threads: 1,
                },
                tools: WorkerTools {
                    ffmpeg: tool.clone(),
                    ffprobe: tool,
                    onnx_runtime: None,
                },
            };
            manifest.validate().unwrap();
            let mut db = self.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'failed',?6)",
                params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),self.project.as_str(),
                    self.session.as_str(),manifest.source.source_version.as_str(),encode(&manifest).unwrap()]).unwrap();
            tx.execute(
                "INSERT INTO attempts(id,job,manifest,state) VALUES(?1,?2,?3,'failed')",
                params![
                    manifest.attempt_id.as_str(),
                    manifest.job_id.as_str(),
                    encode(&manifest).unwrap()
                ],
            )
            .unwrap();
            tx.commit().unwrap();
            manifest
        }
    }

    fn cpu_portable_tag() -> ArtifactIdentity {
        ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "{CPU_RUNTIME_IMAGE_ID_PREFIX}{}",
                "a".repeat(64)
            ))
            .unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        }
    }

    fn fallback_portable_tag() -> ArtifactIdentity {
        ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
                "a".repeat(64)
            ))
            .unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        }
    }

    #[test]
    fn portable_fallback_selection_refuses_export_import_and_inert_ancestor_objects() {
        let f = fixture();
        let source = f.import(b"neutral authored fallback portability fixture; never executed");
        let mut request = f.failed_origin(source);
        let plan = f.capture().unwrap();
        let origin = &plan.manifest.generated_origins[0];
        super::project_package_imports::checked_worker_scope(&plan.manifest, origin, &request)
            .unwrap();
        request.dependencies.push(fallback_portable_tag());
        for state in ["failed", "cancelled", "completed", "fallback_pending"] {
            let mut origin = origin.clone();
            origin.state = state.into();
            assert_eq!(
                super::project_package_imports::checked_worker_scope(
                    &plan.manifest,
                    &origin,
                    &request
                )
                .unwrap_err()
                .code,
                ErrorCode::Unsupported
            );
        }
        let mut objects = BTreeMap::new();
        inline_object(
            &mut objects,
            serde_json::to_vec(&request).unwrap(),
            PackageObjectRole::WorkerManifest,
        )
        .unwrap();
        assert_eq!(
            require_portable_cpu_objects(&plan.manifest, &objects)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let mut ancestor = plan.manifest.clone();
        ancestor.candidates.push(PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: CandidateId::new("fallback-ancestor-candidate").unwrap(),
            base_revision: RevisionId::new(0),
            motion: ancestor.head.motion.clone(),
            job_origin: None,
            lineage: vec![fallback_portable_tag()],
            committed_revision: None,
            review: vec![],
        });
        let mut objects = BTreeMap::new();
        inline_object(
            &mut objects,
            serde_json::to_vec(&ancestor).unwrap(),
            PackageObjectRole::ImportedManifest,
        )
        .unwrap();
        assert_eq!(
            require_portable_cpu_objects(&plan.manifest, &objects)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            super::project_package_imports::validate_evidence(
                &ancestor,
                &f.engine.pool,
                |_| panic!("fallback mode refusal must precede payload reads"),
                &mut || Ok(())
            )
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
        drop(plan);
        let raw = encode(&request).unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET manifest=?1 WHERE id=?2",
                params![raw, request.attempt_id.as_str()],
            )
            .unwrap();
            db.execute(
                "UPDATE jobs SET manifest=?1 WHERE id=?2",
                params![raw, request.job_id.as_str()],
            )
            .unwrap();
        }
        assert!(matches!(f.capture(),Err(error) if error.code==ErrorCode::Unsupported));
    }

    #[test]
    fn portable_export_refuses_fallback_ledger_rows_after_manifest_tag_is_lost() {
        for kind in 0..3 {
            let f = fixture();
            let source = f.import(b"neutral authored contradictory ledger fixture; never executed");
            let request = f.failed_origin(source);
            drop(f.capture().unwrap());
            {
                let db = f.engine.db.lock().unwrap();
                // Deliberately incomplete post-open storage, not a fallback
                // admission, accepted dossier or actual execution observation.
                match kind {
                    0 => {
                        db.execute_batch("CREATE TABLE inference_fallback_jobs(job TEXT)")
                            .unwrap();
                        db.execute(
                            "INSERT INTO inference_fallback_jobs VALUES(?1)",
                            [request.job_id.as_str()],
                        )
                        .unwrap();
                    }
                    1 => {
                        db.execute_batch(
                            "CREATE TABLE inference_fallback_attempts(attempt TEXT,job TEXT)",
                        )
                        .unwrap();
                        db.execute(
                            "INSERT INTO inference_fallback_attempts VALUES(?1,?2)",
                            params![request.attempt_id.as_str(), request.job_id.as_str()],
                        )
                        .unwrap();
                    }
                    _ => {
                        db.execute_batch("CREATE TABLE inference_fallback_pending(job TEXT)")
                            .unwrap();
                        db.execute(
                            "INSERT INTO inference_fallback_pending VALUES(?1)",
                            [request.job_id.as_str()],
                        )
                        .unwrap();
                    }
                }
            }
            assert!(f.capture().is_err());
        }
    }

    #[test]
    fn portable_export_refuses_closed_cpu_attempt_before_dropping_mode_history() {
        let f = fixture();
        let source = f.import(b"neutral authored source; no native execution");
        let mut request = f.failed_origin(source);
        drop(f.capture().unwrap());
        request.dependencies.push(cpu_portable_tag());
        let raw = encode(&request).unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET manifest=?1 WHERE id=?2",
                params![raw, request.attempt_id.as_str()],
            )
            .unwrap();
            db.execute(
                "UPDATE jobs SET manifest=?1 WHERE id=?2",
                params![raw, request.job_id.as_str()],
            )
            .unwrap();
        }
        assert!(matches!(f.capture(),Err(error) if error.code==ErrorCode::Unsupported));
    }

    #[test]
    fn portable_export_refuses_retained_cpu_record_even_when_manifest_tag_is_missing() {
        let f = fixture();
        let source = f.import(b"neutral authored source; no native execution");
        let request = f.failed_origin(source);
        drop(f.capture().unwrap());
        {
            let db = f.engine.db.lock().unwrap();
            // A deliberately corrupt post-open declaration must not be omitted.
            // This is not a valid CPU image or an execution observation.
            db.execute_batch("CREATE TABLE cpu_runtime_attempts(attempt TEXT PRIMARY KEY NOT NULL REFERENCES attempts(id),job TEXT NOT NULL REFERENCES jobs(id),manifest_sha256 TEXT NOT NULL,image_sha256 TEXT NOT NULL,image TEXT NOT NULL)").unwrap();
            db.execute(
                "INSERT INTO cpu_runtime_attempts VALUES(?1,?2,?3,?3,'{}')",
                params![
                    request.attempt_id.as_str(),
                    request.job_id.as_str(),
                    "a".repeat(64)
                ],
            )
            .unwrap();
        }
        assert!(f.capture().is_err());
    }

    #[test]
    fn portable_import_worker_validation_refuses_closed_cpu_mode_in_every_attempt_state() {
        let f = fixture();
        let source = f.import(b"neutral authored source; no native execution");
        let mut request = f.failed_origin(source);
        let plan = f.capture().unwrap();
        let origin = &plan.manifest.generated_origins[0];
        super::project_package_imports::checked_worker_scope(&plan.manifest, origin, &request)
            .unwrap();
        request.dependencies.push(cpu_portable_tag());
        for state in ["failed", "cancelled", "completed"] {
            let mut origin = origin.clone();
            origin.state = state.into();
            assert_eq!(
                super::project_package_imports::checked_worker_scope(
                    &plan.manifest,
                    &origin,
                    &request
                )
                .unwrap_err()
                .code,
                ErrorCode::Unsupported
            );
        }
    }

    #[test]
    fn reexport_scans_inert_worker_objects_and_ancestor_candidate_lineage() {
        let f = fixture();
        let source = f.import(b"neutral authored source; no native execution");
        let mut request = f.failed_origin(source);
        let plan = f.capture().unwrap();
        let mut objects = BTreeMap::new();
        inline_object(
            &mut objects,
            serde_json::to_vec(&request).unwrap(),
            PackageObjectRole::WorkerManifest,
        )
        .unwrap();
        require_portable_cpu_objects(&plan.manifest, &objects).unwrap();
        request.dependencies.push(cpu_portable_tag());
        let mut objects = BTreeMap::new();
        inline_object(
            &mut objects,
            serde_json::to_vec(&request).unwrap(),
            PackageObjectRole::WorkerManifest,
        )
        .unwrap();
        assert_eq!(
            require_portable_cpu_objects(&plan.manifest, &objects)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let mut ancestor = plan.manifest.clone();
        ancestor.candidates.push(PackageCandidate {
            rebased_from: None,
            project_interval: None,
            candidate_id: CandidateId::new("synthetic-candidate").unwrap(),
            base_revision: RevisionId::new(0),
            motion: ancestor.head.motion.clone(),
            job_origin: None,
            lineage: vec![cpu_portable_tag()],
            committed_revision: None,
            review: vec![],
        });
        let mut objects = BTreeMap::new();
        inline_object(
            &mut objects,
            serde_json::to_vec(&ancestor).unwrap(),
            PackageObjectRole::ImportedManifest,
        )
        .unwrap();
        assert_eq!(
            require_portable_cpu_objects(&plan.manifest, &objects)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            super::project_package_imports::validate_evidence(
                &ancestor,
                &f.engine.pool,
                |_| panic!("CPU mode refusal must precede payload reads"),
                &mut || Ok(())
            )
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn unrelated_nested_project_recovery_does_not_consume_capture_admission() {
        let f = fixture();
        drop(f.capture().unwrap());
        let foreign = ProjectId::new(format!("{}:unrelated", f.project)).unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            tx.execute("INSERT INTO projects SELECT ?1,'unrelated',revision,program,history_cursor,protected FROM projects WHERE id=?2",
                params![foreign.as_str(),f.project.as_str()]).unwrap();
            tx.execute("INSERT INTO revisions SELECT ?1,revision,actor,kind,label,program,protected FROM revisions WHERE project=?2",
                params![foreign.as_str(),f.project.as_str()]).unwrap();
            tx.execute("INSERT INTO edit_states SELECT ?1,position,program,protected FROM edit_states WHERE project=?2",
                params![foreign.as_str(),f.project.as_str()]).unwrap();
            lineage::created(&tx, &foreign).unwrap();
            {
                let mut insert = tx
                    .prepare("INSERT INTO legacy_motion_bytes VALUES('edit_states',?1,?2)")
                    .unwrap();
                for position in 0..MAX_CAPTURE_ROWS {
                    insert
                        .execute(params![
                            format!("{foreign}:{position}"),
                            "{\n \"tracks\": []\n}"
                        ])
                        .unwrap();
                }
            }
            tx.commit().unwrap();
        }
        let plan = f.capture().unwrap_or_else(|error| {
            panic!("unrelated project exhausted selected-project capture: {error}")
        });
        assert!(plan.manifest.legacy_cells.is_empty());
        assert_eq!(plan.manifest.revisions.len(), 1);
        assert_eq!(plan.manifest.origin_project_id, f.project);
    }

    #[test]
    fn stash_capture_is_bounded_inert_portable_evidence_bound_to_the_exact_source() {
        let f = fixture();
        let source = f.import(b"authored source");
        let local = f._dir.path().join("source.mp4");
        let receipt = StashSourceCapture {
            endpoint: Some("http://localhost:9999/graphql".into()),
            source_version: source.source_version.clone(),
            identity: source.identity.clone(),
            scene_id: "scene".into(),
            file_id: "file".into(),
            server_path: "/stash/source.mp4".into(),
            local_source: local.clone(),
            script_destination: local.with_extension("funscript"),
        };
        let raw = encode(&receipt).unwrap();
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO stash_sources VALUES(?1,?2,'scene','file',?3)",
                params![f.project.as_str(), source.source_version.as_str(), raw],
            )
            .unwrap();
        let plan = f.capture().unwrap();
        assert_eq!(plan.manifest.format_version, 8);
        assert_eq!(plan.manifest.sources[0].stash_associations.len(), 1);
        let association = &plan.manifest.sources[0].stash_associations[0];
        let descriptor = plan
            .manifest
            .objects
            .iter()
            .find(|o| o.sha256 == association.capture_receipt.sha256)
            .unwrap();
        assert!(descriptor
            .roles
            .contains(&PackageObjectRole::DependencyEvidence));
        let mut bytes = vec![];
        plan.open_object(descriptor)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, raw.as_bytes());
        let mut forged = plan.manifest.clone();
        forged.sources[0].stash_associations[0].scene_id = "other-scene".into();
        assert!(super::project_package_imports::validate_evidence(
            &forged,
            &f.engine.pool,
            |reference| {
                assert_eq!(reference.sha256, association.capture_receipt.sha256);
                Ok(bytes.clone())
            },
            &mut || Ok(())
        )
        .is_err());
        drop(plan);
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE stash_sources SET receipt=printf('%.*c',65537,'x')",
                [],
            )
            .unwrap();
        assert_eq!(
            f.capture().err().unwrap().code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn primary_source_identity_must_match_the_captured_source_catalog() {
        let f = fixture();
        let source = f.import(b"actual immutable primary source");
        let mut manifest = f.failed_origin(source);
        drop(f.capture().unwrap());
        let digest = format!("{:x}", Sha256::digest(b"missing different primary source"));
        manifest.source.identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(digest.clone()).unwrap(),
            sha256: digest.clone(),
            byte_len: 32,
        };
        // The typed worker manifest is structurally valid; the catalog binding
        // is the separate authority/closure invariant this regression tests.
        manifest.validate().unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET manifest=?2 WHERE id=?1",
                params![manifest.attempt_id.as_str(), encode(&manifest).unwrap()],
            )
            .unwrap();
            db.execute(
                "UPDATE jobs SET manifest=?2 WHERE id=?1",
                params![manifest.job_id.as_str(), encode(&manifest).unwrap()],
            )
            .unwrap();
        }
        match f.capture() {
            Err(error) => assert_eq!(error.code, ErrorCode::DependencyMismatch),
            Ok(plan) => panic!("mismatched primary source was accepted as identity-only: missing object bundled={}",
                plan.manifest.objects.iter().any(|object|object.sha256==digest)),
        }
    }

    #[test]
    fn independent_audio_payload_must_match_captured_catalog_even_when_dependency_is_declared() {
        let f = fixture();
        let primary = f.import(b"immutable visual source");
        let audio = f.import(b"immutable separate audio source");
        let mut manifest = f.failed_origin(primary);
        manifest.independent_audio = Some(audio.clone());
        manifest.dependencies.push(audio.identity.clone());
        if let WorkerOperation::Generate { settings, .. } = &mut manifest.operation {
            settings.mixed_audio = Some(MixedAudioSettings {
                intent: MultimodalIntent::Faithful,
                mapping: AudioMapping::new(
                    Axis::Stroke,
                    0.2,
                    NormalizedPosition::new(0.4).unwrap(),
                )
                .unwrap(),
                independent: Some(IndependentAudio {
                    source_version: audio.source_version.clone(),
                    alignment: AudioAlignment {
                        rate: None,
                        video_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        audio_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    },
                }),
            });
        }
        let save = |manifest: &WorkerRequest| {
            manifest.validate().unwrap();
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET manifest=?2 WHERE id=?1",
                params![manifest.attempt_id.as_str(), encode(manifest).unwrap()],
            )
            .unwrap();
            db.execute(
                "UPDATE jobs SET manifest=?2 WHERE id=?1",
                params![manifest.job_id.as_str(), encode(manifest).unwrap()],
            )
            .unwrap();
        };
        save(&manifest);
        drop(f.capture().unwrap());
        let captured = manifest.independent_audio.as_mut().unwrap();
        captured.identity.sha256 = "0".repeat(64);
        captured.identity.artifact_id = ArtifactId::new("0".repeat(64)).unwrap();
        manifest.dependencies.retain(|d| d != &audio.identity);
        manifest.dependencies.push(captured.identity.clone());
        save(&manifest);
        assert!(matches!(f.capture(),Err(error) if error.code==ErrorCode::DependencyMismatch));
    }

    #[test]
    fn capture_indexes_preserve_large_graph_membership_without_reordering_records() {
        let f = fixture();
        let plan = f.capture().unwrap();
        let motion = plan.manifest.head.motion.clone();
        drop(plan);
        let revisions: Vec<_> = (0..10_000)
            .map(|number| PackageRevision {
                revision: RevisionId::new(number),
                actor: "archival-test".into(),
                kind: "fixture".into(),
                label: String::new(),
                motion: motion.clone(),
                protected: vec![],
            })
            .collect();
        let candidates: Vec<_> = (0..10_000)
            .map(|number| PackageCandidate {
                rebased_from: None,
                project_interval: None,
                candidate_id: CandidateId::new(format!("candidate-{number:05}")).unwrap(),
                base_revision: RevisionId::new(number),
                motion: motion.clone(),
                job_origin: Some(JobId::new(format!("job-{:03}", number % 128)).unwrap()),
                lineage: vec![],
                committed_revision: None,
                review: vec![],
            })
            .collect();
        let indexes = CaptureIndexes::new(&candidates, &revisions, &[]);
        assert_eq!(indexes.candidates.len(), 10_000);
        assert_eq!(indexes.revisions.len(), 10_000);
        assert_eq!(
            indexes
                .candidates_by_job
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            10_000
        );
        for number in (0..10_000).rev() {
            let id = format!("candidate-{number:05}");
            let candidate = indexes.candidates[id.as_str()];
            assert_eq!(candidate.base_revision, RevisionId::new(number));
            assert_eq!(
                indexes.revisions[&candidate.base_revision].revision,
                candidate.base_revision
            );
        }
        for (number, candidate) in candidates.iter().enumerate() {
            assert_eq!(
                candidate.candidate_id.as_str(),
                format!("candidate-{number:05}")
            );
        }
        for group in indexes.candidates_by_job.values() {
            assert!(group
                .windows(2)
                .all(|pair| pair[0].candidate_id.as_str() < pair[1].candidate_id.as_str()));
        }
    }

    #[test]
    fn absent_executable_dependency_bytes_remain_identity_only() {
        let f = fixture();
        let source = f.import(b"actual primary source remains required");
        let mut manifest = f.failed_origin(source);
        let digest = format!("{:x}", Sha256::digest(b"unavailable historical executable"));
        let dependency = ArtifactIdentity {
            artifact_id: ArtifactId::new(digest.clone()).unwrap(),
            sha256: digest.clone(),
            byte_len: 33,
        };
        manifest.dependencies.push(dependency.clone());
        manifest.tools.ffmpeg = ArtifactReference {
            identity: dependency,
            path: f._dir.path().join("not-installed-ffmpeg"),
        };
        manifest.validate().unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET manifest=?2 WHERE id=?1",
                params![manifest.attempt_id.as_str(), encode(&manifest).unwrap()],
            )
            .unwrap();
        }
        let plan = f.capture().unwrap();
        assert_eq!(plan.manifest.generated_origins.len(), 1);
        assert!(!plan
            .manifest
            .objects
            .iter()
            .any(|object| object.sha256 == digest));
    }

    #[test]
    fn format15_cpu_runtime_declarations_preserve_inert_history_and_refuse_mutations() {
        let f = fixture();
        let source = f.import(b"neutral authored CPU portability fixture; never executed");
        let legacy = f.capture().unwrap();
        assert_eq!(legacy.manifest.format_version, 1);
        drop(legacy);

        let identity = |index: u8| {
            let sha256 = format!("{index:064x}");
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: 1,
            }
        };
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries: CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(index, name)| CapturedNativeLibrary {
                    identity: identity(index as u8 + 1),
                    names: vec![(*name).into()],
                })
                .collect(),
        };
        let image_id = image.identity().unwrap();
        let model = SourceArtifact {
            identity: identity(8),
            source_version: SourceVersionId::new("cpu-model").unwrap(),
            path: "/nonexistent/metadata-only/model.onnx".into(),
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: model.identity.clone(),
            runtime: identity(3),
            preprocessing: InferencePreprocessing {
                input: input.clone(),
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let mut request = f.failed_origin(source);
        request.budget = ResourceBudget {
            memory_bytes: 64 << 20,
            output_bytes: 16 << 20,
            wall_time_ms: 1000,
            cpu_threads: 1,
        };
        request.operation = WorkerOperation::Generate {
            preset: "fixture".into(),
            settings: Box::new(GenerationSettings {
                model_input: Some(input),
                inference_profile: Some(profile),
                ..Default::default()
            }),
            model: Some(model.clone()),
        };
        request.tools.onnx_runtime = Some(ArtifactReference {
            identity: identity(3),
            path: "/pulsar-cpu-libraries/libonnxruntime.so.1".into(),
        });
        request.dependencies.extend([
            f.engine.binary_identity.clone(),
            model.identity.clone(),
            identity(3),
            image.owner_policy.clone(),
            identity(1),
            identity(2),
            image_id.clone(),
        ]);
        request.validate().unwrap();
        let raw_request = serde_json::to_string(&request).unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            tx.execute(
                "UPDATE attempts SET manifest=?1 WHERE id=?2",
                params![raw_request, request.attempt_id.as_str()],
            )
            .unwrap();
            tx.execute(
                "UPDATE jobs SET manifest=?1 WHERE id=?2",
                params![raw_request, request.job_id.as_str()],
            )
            .unwrap();
            crate::cpu_runtime_db::declare(&tx, &request, &image).unwrap();
            tx.commit().unwrap();
        }

        for state in ["queued", "running", "interrupted", "failed", "cancelled"] {
            {
                let db = f.engine.db.lock().unwrap();
                db.execute(
                    "UPDATE attempts SET state=?1 WHERE id=?2",
                    params![state, request.attempt_id.as_str()],
                )
                .unwrap();
                db.execute(
                    "UPDATE jobs SET state=?1 WHERE id=?2",
                    params![state, request.job_id.as_str()],
                )
                .unwrap();
            }
            let plan = f.capture().unwrap();
            assert_eq!(
                plan.manifest.format_version,
                PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
            );
            assert_eq!(plan.manifest.generated_origins.len(), 1);
            assert_eq!(plan.manifest.generated_origins[0].state, state);
            assert_eq!(
                plan.manifest.generated_origins[0]
                    .cpu_runtime
                    .as_ref()
                    .unwrap()
                    .identity()
                    .unwrap(),
                image_id
            );
        }

        let head_motion = f.capture().unwrap().manifest.head.motion.clone();
        let program_bytes =
            serde_json::to_vec(&f.engine.motion_store.read(&head_motion).unwrap()).unwrap();
        let program_sha = format!("{:x}", Sha256::digest(&program_bytes));
        let program_id = ArtifactIdentity {
            artifact_id: ArtifactId::new(program_sha.clone()).unwrap(),
            sha256: program_sha,
            byte_len: program_bytes.len() as u64,
        };
        fs::create_dir_all(&request.output_dir).unwrap();
        fs::write(request.output_dir.join("program.json"), &program_bytes).unwrap();
        let receipt_deps = request.dependencies.clone();
        let receipt_json = serde_json::json!({
            "schema_version": 2,
            "recipe": "fixture-cpu",
            "project_id": f.project,
            "base_revision": 0,
            "job_id": request.job_id,
            "attempt_id": request.attempt_id,
            "source_version": request.source.source_version,
            "program": program_id,
            "dependencies": receipt_deps,
        });
        let receipt_bytes = serde_json::to_vec(&receipt_json).unwrap();
        let receipt_sha = format!("{:x}", Sha256::digest(&receipt_bytes));
        let receipt_id = ArtifactIdentity {
            artifact_id: ArtifactId::new(receipt_sha.clone()).unwrap(),
            sha256: receipt_sha,
            byte_len: receipt_bytes.len() as u64,
        };
        fs::write(request.output_dir.join("receipt.json"), &receipt_bytes).unwrap();
        let mut candidate_lineage = receipt_deps;
        candidate_lineage.push(receipt_id.clone());
        let candidate_id = id!(CandidateId);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE attempts SET state='completed' WHERE id=?1",
                [request.attempt_id.as_str()],
            )
            .unwrap();
            db.execute(
                "UPDATE jobs SET state='completed' WHERE id=?1",
                [request.job_id.as_str()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO candidates(id,project,base_revision,program,job,lineage,committed_revision,review) VALUES(?1,?2,0,(SELECT program FROM projects WHERE id=?2),?3,?4,NULL,'[]')",
                params![
                    candidate_id.as_str(),
                    f.project.as_str(),
                    request.job_id.as_str(),
                    encode(&candidate_lineage).unwrap(),
                ],
            )
            .unwrap();
        }

        let plan = f.capture().unwrap();
        assert_eq!(
            plan.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        assert_eq!(
            plan.manifest.generated_origins[0]
                .cpu_runtime
                .as_ref()
                .unwrap()
                .identity()
                .unwrap(),
            image_id
        );
        assert!(plan.manifest.candidates[0].lineage.contains(&image_id));
        let mut byte_map = BTreeMap::new();
        for descriptor in &plan.manifest.objects {
            let mut buf = Vec::new();
            plan.open_object(descriptor)
                .unwrap()
                .read_to_end(&mut buf)
                .unwrap();
            byte_map.insert(descriptor.sha256.clone(), buf);
        }
        let build_objects = |extra: Option<&[u8]>| {
            let mut objects = BTreeMap::new();
            for descriptor in &plan.manifest.objects {
                for role in &descriptor.roles {
                    inline_object(&mut objects, byte_map[&descriptor.sha256].clone(), *role)
                        .unwrap();
                }
            }
            if let Some(bytes) = extra {
                inline_object(
                    &mut objects,
                    bytes.to_vec(),
                    PackageObjectRole::ImportedManifest,
                )
                .unwrap();
            }
            objects
        };
        let captured_objects = build_objects(None);
        require_portable_cpu_objects(&plan.manifest, &captured_objects).unwrap();
        crate::project_package_import_plan::validate_retained_origin_graph(
            &plan.manifest,
            &BTreeMap::new(),
        )
        .unwrap();
        super::project_package_imports::validate_evidence(
            &plan.manifest,
            &f.engine.pool,
            |reference| Ok(byte_map[&reference.sha256].clone()),
            &mut || Ok(()),
        )
        .unwrap();

        let descriptor_for = |m: &PortableProjectManifest| PackageArtifactDescriptor {
            format_version: m.format_version,
            sha256: "b".repeat(64),
            byte_len: 100_000,
            manifest_sha256: format!("{:x}", Sha256::digest(m.canonical_bytes().unwrap())),
            project_id: m.origin_project_id.clone(),
            captured_revision: m.captured_revision,
            captured_event_cursor: m.captured_event_cursor,
            object_count: m.objects.len() as u32,
            verification: PackageReadyVerification::AllDeclaredObjectsVerified,
        };
        let assign_ids = |m: &PortableProjectManifest, suffix: &str| {
            crate::project_package_import_plan::CloneIdentityAssignment {
                project_id: ProjectId::new(format!("project.{suffix}")).unwrap(),
                placements: BTreeMap::new(),
                candidates: m
                    .candidates
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        (
                            c.candidate_id.clone(),
                            CandidateId::new(format!("candidate.{suffix}.{i}")).unwrap(),
                        )
                    })
                    .collect(),
            }
        };
        let first = crate::project_package_import_plan::plan_clone_import(
            &plan.manifest,
            &descriptor_for(&plan.manifest),
            &BTreeMap::new(),
            &assign_ids(&plan.manifest, "clone1"),
        )
        .unwrap();
        assert_eq!(
            first.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        assert!(first.manifest.generated_origins.is_empty());
        assert!(first.manifest.candidates[0].lineage.contains(&image_id));
        let mut ancestors1 = BTreeMap::from([(
            first.origin_manifest_ref.sha256.clone(),
            plan.manifest.clone(),
        )]);
        crate::project_package_import_plan::validate_retained_origin_graph(
            &first.manifest,
            &ancestors1,
        )
        .unwrap();
        let clone1_objects = build_objects(Some(&first.origin_manifest));
        require_portable_cpu_objects(&first.manifest, &clone1_objects).unwrap();

        let second = crate::project_package_import_plan::plan_clone_import(
            &first.manifest,
            &descriptor_for(&first.manifest),
            &ancestors1,
            &assign_ids(&first.manifest, "clone2"),
        )
        .unwrap();
        assert_eq!(
            second.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        ancestors1.insert(
            second.origin_manifest_ref.sha256.clone(),
            first.manifest.clone(),
        );
        crate::project_package_import_plan::validate_retained_origin_graph(
            &second.manifest,
            &ancestors1,
        )
        .unwrap();

        let mut stripped = plan.manifest.clone();
        stripped.generated_origins[0].cpu_runtime = None;
        assert!(
            crate::project_package_import_plan::validate_retained_origin_graph(
                &stripped,
                &BTreeMap::new()
            )
            .is_err()
        );
        assert!(require_portable_cpu_objects(&stripped, &captured_objects).is_err());
        assert!(super::project_package_imports::validate_evidence(
            &stripped,
            &f.engine.pool,
            |reference| Ok(byte_map[&reference.sha256].clone()),
            &mut || Ok(()),
        )
        .is_err());

        let mut substituted = plan.manifest.clone();
        substituted.generated_origins[0]
            .cpu_runtime
            .as_mut()
            .unwrap()
            .owner_policy = identity(10);
        assert!(
            crate::project_package_import_plan::validate_retained_origin_graph(
                &substituted,
                &BTreeMap::new()
            )
            .is_err()
        );
        assert!(super::project_package_imports::validate_evidence(
            &substituted,
            &f.engine.pool,
            |reference| Ok(byte_map[&reference.sha256].clone()),
            &mut || Ok(()),
        )
        .is_err());

        let bad_ancestors =
            BTreeMap::from([(first.origin_manifest_ref.sha256.clone(), stripped.clone())]);
        assert!(
            crate::project_package_import_plan::validate_retained_origin_graph(
                &first.manifest,
                &bad_ancestors
            )
            .is_err()
        );
        let bad_clone_objects = build_objects(Some(&serde_json::to_vec(&stripped).unwrap()));
        assert!(require_portable_cpu_objects(&first.manifest, &bad_clone_objects).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "explicit native CPU roundtrip and preceding-reader campaign; run serially"]
    fn native_portable_cpu_roundtrip_and_preceding_reader_campaign() {
        use crate::cpu_owner_policy::{CpuOwnerPolicy, LoadedCpuOwnerPolicy};
        use crate::project_package_format::{write_package, PackageCodecLimits};
        use serde_json::{json, Value};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::path::{Path, PathBuf};
        use std::process::{Command as ProcessCommand, Stdio};
        use std::time::{Duration, Instant};

        #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
        struct FilePin {
            present: bool,
            device: u64,
            inode: u64,
            mode: u32,
            uid: u32,
            gid: u32,
            links: u64,
            byte_len: u64,
            sha256: Option<String>,
        }
        fn pin_file(path: &Path, retained_fds: &mut Vec<fs::File>) -> FilePin {
            match fs::symlink_metadata(path) {
                Ok(meta) => {
                    assert!(
                        meta.file_type().is_file(),
                        "expected regular file: {path:?}"
                    );
                    let mut file = fs::File::open(path).unwrap();
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes).unwrap();
                    // Retain open descriptor while SQLite read pins are held so closing a
                    // raw descriptor does not drop this process's POSIX locks on SHM/DB.
                    retained_fds.push(file);
                    FilePin {
                        present: true,
                        device: meta.dev(),
                        inode: meta.ino(),
                        mode: meta.mode() & 0o7777,
                        uid: meta.uid(),
                        gid: meta.gid(),
                        links: meta.nlink(),
                        byte_len: bytes.len() as u64,
                        sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => FilePin {
                    present: false,
                    device: 0,
                    inode: 0,
                    mode: 0,
                    uid: 0,
                    gid: 0,
                    links: 0,
                    byte_len: 0,
                    sha256: None,
                },
                Err(err) => panic!("stat {path:?}: {err}"),
            }
        }
        fn state_pins(state_dir: &Path, retained_fds: &mut Vec<fs::File>) -> Value {
            json!({
                "database": pin_file(&state_dir.join("projects.sqlite3"), retained_fds),
                "wal": pin_file(&state_dir.join("projects.sqlite3-wal"), retained_fds),
                "shm": pin_file(&state_dir.join("projects.sqlite3-shm"), retained_fds),
                "fence": pin_file(&state_dir.join("worker-fence.json"), retained_fds),
                "engine_sock_present": state_dir.join("engine.sock").exists(),
                "bulk_sock_present": state_dir.join("bulk.sock").exists(),
            })
        }
        fn shm_diff(before: &[u8], after: &[u8]) -> Value {
            let mut changed = 0usize;
            for idx in 0..before.len().max(after.len()) {
                if before.get(idx) != after.get(idx) {
                    changed += 1;
                }
            }
            json!({
                "before_len": before.len(),
                "after_len": after.len(),
                "changed_byte_count": changed,
            })
        }
        fn file_identity(path: &Path) -> ArtifactIdentity {
            let (sha256, byte_len) = artifacts::digest_file(path, 512 * 1024 * 1024).unwrap();
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len,
            }
        }
        fn assert_engine_retired(engine: &Arc<Engine>) {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !engine.workers.lock().unwrap().is_empty()
                || engine.worker_fence.has_pending_worker_custody().unwrap()
                || engine.pool.reserved_storage() != 0
                || engine.pool.reserved_cpu_threads() != Ok(0)
            {
                assert!(Instant::now() < deadline, "Engine resources did not retire");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(engine.workers.lock().unwrap().is_empty());
            assert!(!engine.worker_fence.has_pending_worker_custody().unwrap());
            assert_eq!(engine.pool.reserved_storage(), 0);
            assert_eq!(engine.pool.reserved_cpu_threads(), Ok(0));
        }
        fn drop_and_join_engine(engine: Arc<Engine>) {
            assert_engine_retired(&engine);
            let weak = Arc::downgrade(&engine);
            drop(engine);
            let deadline = Instant::now() + Duration::from_secs(20);
            while weak.strong_count() != 0 {
                assert!(Instant::now() < deadline, "Engine Arc still held");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn pair_with_token(engine: &Arc<Engine>, label: &str) -> (SessionId, String) {
            let ResponseBody::Paired {
                session,
                auth_token,
            } = engine
                .handle(Request::new(
                    id!(RequestId),
                    Command::Pair {
                        client_name: label.into(),
                        pairing_token: engine.token.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("pair failed")
            };
            (session, auth_token)
        }
        fn authed_req(
            session: &SessionId,
            token: &str,
            project: Option<&ProjectId>,
            revision: Option<u64>,
            command: Command,
        ) -> Request {
            let mut req = Request::new(id!(RequestId), command)
                .with_session(session.clone())
                .with_auth_token(token.to_owned());
            if let Some(p) = project {
                req = req.in_project(p.clone(), revision.map(RevisionId::new));
            }
            req
        }
        fn export_package_bytes(
            engine: &Arc<Engine>,
            session: &SessionId,
            token: &str,
            project: &ProjectId,
        ) -> (PackageExportStatus, Vec<u8>) {
            let ResponseBody::PackageExport(started) = engine
                .handle(authed_req(
                    session,
                    token,
                    Some(project),
                    Some(1),
                    Command::StartProjectExport,
                ))
                .result
                .unwrap()
            else {
                panic!("StartProjectExport")
            };
            let deadline = Instant::now() + Duration::from_secs(30);
            let ready = loop {
                let ResponseBody::PackageExport(status) = engine
                    .handle(authed_req(
                        session,
                        token,
                        Some(project),
                        None,
                        Command::ProjectPackageStatus {
                            operation_id: started.operation_id.clone(),
                        },
                    ))
                    .result
                    .unwrap()
                else {
                    panic!("ProjectPackageStatus")
                };
                if status.state == PackageExportOperationState::Ready {
                    break *status;
                }
                assert!(
                    !matches!(
                        status.state,
                        PackageExportOperationState::Failed
                            | PackageExportOperationState::Cancelled
                    ) && Instant::now() < deadline,
                    "export failed or timed out: {:?}",
                    status
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            let artifact = ready.artifact.as_ref().unwrap();
            let pkg_path = engine
                .config
                .state_dir
                .join("project-packages")
                .join(&artifact.sha256);
            let bytes = fs::read(&pkg_path).unwrap();
            assert_eq!(bytes.len() as u64, artifact.byte_len);
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), artifact.sha256);
            (ready, bytes)
        }
        fn import_package_bytes(
            engine: &Arc<Engine>,
            session: &SessionId,
            token: &str,
            bytes: &[u8],
        ) -> PackageImportStatus {
            let package = PackageUploadDeclaration {
                format_version: package_format_version(&bytes[..52]).unwrap(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                byte_len: bytes.len() as u64,
            };
            let ResponseBody::PackageImport(started) = engine
                .handle(authed_req(
                    session,
                    token,
                    None,
                    None,
                    Command::StartProjectImport { package },
                ))
                .result
                .unwrap()
            else {
                panic!("StartProjectImport")
            };
            let ResponseBody::PackageUpload(lease) = engine
                .handle(authed_req(
                    session,
                    token,
                    None,
                    None,
                    Command::BeginProjectPackageUpload {
                        operation_id: started.operation_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("BeginProjectPackageUpload")
            };
            let handshake = PackageUploadHandshake {
                version: PROTOCOL_VERSION,
                channel: PackageUploadChannel::ProjectPackageUpload,
                session: session.clone(),
                auth_token: token.to_owned(),
                lease_id: lease.lease_id.clone(),
                engine_epoch: lease.engine_epoch.clone(),
                operation_id: lease.operation_id.clone(),
                generation: lease.generation,
                package_sha256: lease.package.sha256.clone(),
            };
            let (conn, _) = engine.import_bulk_open(&handshake).unwrap();
            let mut offset = 0;
            for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
                offset = engine
                    .import_bulk_append(&conn, &handshake, offset, chunk)
                    .unwrap();
            }
            assert_eq!(offset, bytes.len() as u64);
            drop(conn);
            engine
                .handle(authed_req(
                    session,
                    token,
                    None,
                    None,
                    Command::SealProjectImport {
                        operation_id: started.operation_id.clone(),
                        upload_generation: lease.generation,
                    },
                ))
                .result
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            let final_status = loop {
                let ResponseBody::PackageImport(status) = engine
                    .handle(authed_req(
                        session,
                        token,
                        None,
                        None,
                        Command::ProjectImportStatus {
                            operation_id: started.operation_id.clone(),
                        },
                    ))
                    .result
                    .unwrap()
                else {
                    panic!("ProjectImportStatus")
                };
                if status.state.is_terminal() {
                    break *status;
                }
                assert!(Instant::now() < deadline, "import timed out");
                std::thread::sleep(Duration::from_millis(5));
            };
            final_status
        }
        fn open_reader_pin(state_dir: &Path) -> Connection {
            let db_path = state_dir.join("projects.sqlite3");
            let pin =
                Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            pin.busy_timeout(Duration::from_secs(2)).unwrap();
            pin.execute_batch("PRAGMA query_only=ON; BEGIN;").unwrap();
            let mode: String = pin
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            let _: i64 = pin
                .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
                .unwrap();
            pin
        }
        fn run_preceding_reader_engine(
            preceding_pulsar: &Path,
            preceding_worker: &Path,
            state_dir: &Path,
            pin: Connection,
        ) -> Value {
            let mut retained_fds: Vec<fs::File> = Vec::new();
            let db_path = state_dir.join("projects.sqlite3");
            let wal_path = state_dir.join("projects.sqlite3-wal");
            let shm_path = state_dir.join("projects.sqlite3-shm");
            let pinned_user_version: i64 = pin
                .query_row("PRAGMA main.user_version", [], |r| r.get(0))
                .unwrap();
            let mut main_file = fs::File::open(&db_path).unwrap();
            let mut main_bytes = Vec::new();
            main_file.read_to_end(&mut main_bytes).unwrap();
            retained_fds.push(main_file);
            let main_file_user_version = i32::from_be_bytes(main_bytes[60..64].try_into().unwrap());
            assert!(
                fs::metadata(&wal_path).unwrap().len() > 32
                    && fs::metadata(&shm_path).unwrap().len() >= 32768,
                "WAL and SHM sidecars must be present and non-empty during reader pin"
            );
            let before = state_pins(state_dir, &mut retained_fds);
            let mut shm_before_file = fs::File::open(&shm_path).unwrap();
            let mut shm_before = Vec::new();
            shm_before_file.read_to_end(&mut shm_before).unwrap();
            retained_fds.push(shm_before_file);
            let mut child = ProcessCommand::new(preceding_pulsar)
                .arg("engine")
                .current_dir(state_dir)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", state_dir)
                .env("PULSAR_STATE_DIR", state_dir)
                .env("PULSAR_WORKER_EXECUTABLE", preceding_worker)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                assert!(Instant::now() < deadline, "preceding reader timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            let after = state_pins(state_dir, &mut retained_fds);
            let mut shm_after_file = fs::File::open(&shm_path).unwrap();
            let mut shm_after = Vec::new();
            shm_after_file.read_to_end(&mut shm_after).unwrap();
            retained_fds.push(shm_after_file);
            let _ = pin.execute_batch("ROLLBACK;");
            drop(pin);
            drop(retained_fds);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            json!({
                "exit_code": output.status.code(),
                "stdout": stdout,
                "stderr": stderr,
                "pinned_user_version": pinned_user_version,
                "main_file_user_version": main_file_user_version,
                "reader39_in_committed_wal": main_file_user_version < 39 && pinned_user_version == 39,
                "before": before,
                "after": after,
                "identical_state_files": before == after,
                "shm_diff": shm_diff(&shm_before, &shm_after),
                "inspection_descriptors_retained_until_pin_closed": true,
            })
        }

        let closure_path = PathBuf::from(
            std::env::var("PULSAR_PORTABLE_CPU_CLOSURE_JSON").unwrap_or_else(|_| {
                "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-24/qualified-fallback/native-subject-007/closure.json"
                    .into()
            }),
        );
        let preceding_pulsar = PathBuf::from(
            std::env::var("PULSAR_PORTABLE_CPU_PRECEDING_PULSAR").unwrap_or_else(|_| {
                "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-24/captured-cpu-live/native-subject-005/pulsar"
                    .into()
            }),
        );
        let preceding_worker = PathBuf::from(
            std::env::var("PULSAR_PORTABLE_CPU_PRECEDING_WORKER").unwrap_or_else(|_| {
                "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-24/captured-cpu-live/native-subject-005/pulsar-worker"
                    .into()
            }),
        );
        let worker_bin = PathBuf::from(
            std::env::var("PULSAR_PORTABLE_CPU_WORKER").unwrap_or_else(|_| {
                "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-28/source-browser/integrated-subject-001/pulsar-worker"
                    .into()
            }),
        );

        let closure_raw = fs::read(&closure_path).unwrap();
        let closure: Value = serde_json::from_slice(&closure_raw).unwrap();
        let subjects = &closure["subjects"];
        let ffmpeg_path = PathBuf::from(subjects["ffmpeg"]["path"].as_str().unwrap());
        let ffprobe_path = PathBuf::from(subjects["ffprobe"]["path"].as_str().unwrap());
        let media_path = PathBuf::from(subjects["media"]["path"].as_str().unwrap());
        let model_path = PathBuf::from(subjects["model"]["path"].as_str().unwrap());
        let runtime_path = PathBuf::from(subjects["runtime"]["path"].as_str().unwrap());

        assert_eq!(
            file_identity(&ffmpeg_path).sha256,
            subjects["ffmpeg"]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            file_identity(&ffprobe_path).sha256,
            subjects["ffprobe"]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            file_identity(&media_path).sha256,
            subjects["media"]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            file_identity(&model_path).sha256,
            subjects["model"]["sha256"].as_str().unwrap()
        );
        assert_eq!(
            file_identity(&runtime_path).sha256,
            subjects["runtime"]["sha256"].as_str().unwrap()
        );
        let preceding_pulsar_id = file_identity(&preceding_pulsar);
        let preceding_worker_id = file_identity(&preceding_worker);
        let worker_bin_id = file_identity(&worker_bin);
        assert_eq!(
            preceding_pulsar_id.sha256,
            "f47f665f133daedf3765b28dd2ae6d9dab57e86c48ba5e9769d8e4cc646b6092"
        );
        assert_eq!(
            preceding_worker_id.sha256,
            "abfc4065da452b71447521b23e5fc40063457bf48708db53c4a020392c4cb317"
        );

        let root_dir = tempfile::tempdir().unwrap();
        let invalid_ort = root_dir.path().join("invalid-generic-ort.bin");
        fs::write(
            &invalid_ort,
            b"generic ORT path must remain unused when cpu_owner_policy is configured\n",
        )
        .unwrap();
        let prev_ffmpeg = std::env::var_os("PULSAR_FFMPEG");
        let prev_ffmpeg_bin = std::env::var_os("PULSAR_FFMPEG_BIN");
        let prev_ffprobe = std::env::var_os("PULSAR_FFPROBE");
        let prev_ffprobe_bin = std::env::var_os("PULSAR_FFPROBE_BIN");
        let prev_ort = std::env::var_os("PULSAR_ORT_DYLIB");
        std::env::set_var("PULSAR_FFMPEG", &ffmpeg_path);
        std::env::set_var("PULSAR_FFMPEG_BIN", &ffmpeg_path);
        std::env::set_var("PULSAR_FFPROBE", &ffprobe_path);
        std::env::set_var("PULSAR_FFPROBE_BIN", &ffprobe_path);
        std::env::set_var("PULSAR_ORT_DYLIB", &invalid_ort);

        let mut policy_value: CpuOwnerPolicy =
            serde_json::from_value(closure["policy"].clone()).unwrap();
        for lib in &mut policy_value.native_dependencies.libraries {
            let real_path = fs::canonicalize(&lib.path).unwrap_or_else(|_| lib.path.clone());
            let id = file_identity(&real_path);
            lib.path = real_path;
            lib.sha256 = id.sha256;
            lib.byte_len = id.byte_len;
        }
        policy_value.validate().unwrap();
        let policy_path = root_dir.path().join("cpu-owner.json");
        let policy_bytes = serde_json::to_vec(&policy_value).unwrap();
        fs::write(&policy_path, &policy_bytes).unwrap();
        fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let owner = LoadedCpuOwnerPolicy::load(&policy_path).unwrap();
        let expected_image = owner.image().unwrap();
        let expected_image_id = expected_image.identity().unwrap();
        assert_eq!(
            owner.runtime_identity().unwrap().sha256,
            subjects["runtime"]["sha256"].as_str().unwrap()
        );
        assert_eq!(&expected_image.owner_policy, owner.identity());

        let local_media = root_dir.path().join("origin-media.mp4");
        fs::copy(&media_path, &local_media).unwrap();
        let local_model = root_dir.path().join("origin-temporal.onnx");
        fs::copy(&model_path, &local_model).unwrap();
        let model_id = file_identity(&local_model);
        let explicit_profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: model_id.clone(),
            runtime: owner.runtime_identity().unwrap(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 << 20,
            minimum_output_bytes: 1 << 20,
            cpu_threads: 2,
            cuda_resources: None,
        };
        explicit_profile.validate().unwrap();
        let profile_id = crate::inference_profiles::profile_identity(&explicit_profile).unwrap();

        // 1a. Negative confined CPU execution (broken policy missing libstdc++.so.6 in confined namespace).
        let neg_dir = root_dir.path().join("neg-missing-stdcpp");
        fs::create_dir_all(&neg_dir).unwrap();
        let mut broken_policy = policy_value.clone();
        broken_policy
            .native_dependencies
            .libraries
            .retain(|lib| !lib.names.iter().any(|n| n == "libstdc++.so.6"));
        assert_eq!(
            broken_policy.native_dependencies.libraries.len() + 1,
            policy_value.native_dependencies.libraries.len()
        );
        let broken_policy_path = neg_dir.join("broken-cpu-owner.json");
        fs::write(
            &broken_policy_path,
            serde_json::to_vec(&broken_policy).unwrap(),
        )
        .unwrap();
        fs::set_permissions(&broken_policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut neg_cfg = EngineConfig::new(neg_dir.join("state"), worker_bin.clone());
        neg_cfg.cpu_owner_policy = Some(broken_policy_path);
        let neg_engine = Engine::open(neg_cfg).unwrap();
        let (neg_session, neg_token) = pair_with_token(&neg_engine, "neg-cpu");
        let ResponseBody::Project(neg_proj) = neg_engine
            .handle(authed_req(
                &neg_session,
                &neg_token,
                None,
                None,
                Command::CreateProject {
                    name: "Negative CPU".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let ResponseBody::Source {
            source_version: neg_source,
        } = neg_engine
            .handle(authed_req(
                &neg_session,
                &neg_token,
                Some(&neg_proj.project_id),
                None,
                Command::ImportSource {
                    path: local_media.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let neg_settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            model_input: Some(explicit_profile.preprocessing.input.clone()),
            inference_profile: Some(explicit_profile.clone()),
            ..Default::default()
        };
        let ResponseBody::Job(neg_job) = neg_engine
            .handle(authed_req(
                &neg_session,
                &neg_token,
                Some(&neg_proj.project_id),
                Some(0),
                Command::Generate {
                    source_version: neg_source,
                    preset: "default".into(),
                    settings: Some(neg_settings),
                    model_path: Some(local_model.clone()),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let neg_deadline = Instant::now() + Duration::from_secs(30);
        let neg_final = loop {
            let snap = {
                let db = neg_engine.db.lock().unwrap();
                job_snapshot(&db, &neg_job.job_id, &neg_proj.project_id).unwrap()
            };
            if snap.state != "queued" && snap.state != "running" {
                break snap;
            }
            assert!(Instant::now() < neg_deadline, "negative CPU job timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(neg_final.state, "failed");
        let neg_err_msg = neg_final.error.as_ref().unwrap().clone();
        assert!(
            neg_err_msg.contains("failed to load pinned ONNX runtime")
                && neg_err_msg.contains("/pulsar-cpu-libraries/libonnxruntime.so.1"),
            "unexpected negative CPU error: {neg_err_msg}"
        );
        drop_and_join_engine(neg_engine);

        // 1b. Main origin Engine: execute actual completed, cancelled, and failed confined CPU attempts,
        // plus retained interrupted and captured-running attempts.
        let origin_state_dir = root_dir.path().join("origin-state");
        let mut origin_cfg = EngineConfig::new(origin_state_dir.clone(), worker_bin.clone());
        origin_cfg.cpu_owner_policy = Some(policy_path.clone());
        let origin_engine = Engine::open(origin_cfg.clone()).unwrap();
        let (origin_session, origin_token) = pair_with_token(&origin_engine, "origin-owner");
        let ResponseBody::Project(origin_proj) = origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                None,
                None,
                Command::CreateProject {
                    name: "Portable CPU Origin".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let origin_project_id = origin_proj.project_id;
        let ResponseBody::Source {
            source_version: origin_source,
        } = origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                None,
                Command::ImportSource {
                    path: local_media.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };

        // Attempt 1: Actual confined CPU generation -> Completed + CommitCandidate.
        let gen_settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            model_input: Some(explicit_profile.preprocessing.input.clone()),
            inference_profile: Some(explicit_profile.clone()),
            ..Default::default()
        };
        let ResponseBody::Job(completed_job_initial) = origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                Some(0),
                Command::Generate {
                    source_version: origin_source.clone(),
                    preset: "default".into(),
                    settings: Some(gen_settings.clone()),
                    model_path: Some(local_model.clone()),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let gen_deadline = Instant::now() + Duration::from_secs(60);
        let completed_job = loop {
            let snap = {
                let db = origin_engine.db.lock().unwrap();
                job_snapshot(&db, &completed_job_initial.job_id, &origin_project_id).unwrap()
            };
            if snap.state != "queued" && snap.state != "running" {
                break snap;
            }
            assert!(Instant::now() < gen_deadline, "completed CPU job timed out");
            std::thread::sleep(Duration::from_millis(15));
        };
        assert_eq!(
            completed_job.state, "completed",
            "{:?}",
            completed_job.error
        );
        let completed_candidate_id = completed_job.candidate_id.clone().unwrap();
        let completed_receipt_path = origin_state_dir
            .join("attempts")
            .join(completed_job.attempt_id.as_str())
            .join("receipt.json");
        let completed_receipt_json: Value =
            serde_json::from_slice(&fs::read(&completed_receipt_path).unwrap()).unwrap();
        assert_eq!(completed_receipt_json["qualification"], "unqualified");
        assert_eq!(
            completed_receipt_json["inference_execution"]["provider"],
            "cpu"
        );
        assert!(
            completed_receipt_json["inference_execution"]["successful_runs"]
                .as_u64()
                .unwrap()
                > 0
        );
        origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                Some(0),
                Command::CommitCandidate {
                    candidate_id: completed_candidate_id.clone(),
                },
            ))
            .result
            .unwrap();

        // Attempt 2: Actual confined CPU generation -> Cancelled mid-execution after progress checkpoint.
        let mut cancel_settings = gen_settings.clone();
        cancel_settings.source_sample_stride = 1;
        let ResponseBody::Job(cancelled_job_initial) = origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                Some(1),
                Command::Generate {
                    source_version: origin_source.clone(),
                    preset: "default".into(),
                    settings: Some(cancel_settings),
                    model_path: Some(local_model.clone()),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let cancel_attempt_dir = origin_state_dir
            .join("attempts")
            .join(cancelled_job_initial.attempt_id.as_str());
        let progress_deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < progress_deadline {
            let has_progress = fs::read_dir(&cancel_attempt_dir)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .any(|e| {
                    let name = e.file_name();
                    let s = name.to_string_lossy();
                    s.starts_with(".progress-") && s.ends_with(".meta.json")
                });
            if has_progress {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                None,
                Command::CancelJob {
                    job_id: cancelled_job_initial.job_id.clone(),
                },
            ))
            .result
            .unwrap();
        let cancel_retire_deadline = Instant::now() + Duration::from_secs(30);
        while !origin_engine.workers.lock().unwrap().is_empty()
            || origin_engine
                .worker_fence
                .has_pending_worker_custody()
                .unwrap()
        {
            assert!(
                Instant::now() < cancel_retire_deadline,
                "cancelled CPU worker did not retire"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let cancelled_job = {
            let db = origin_engine.db.lock().unwrap();
            job_snapshot(&db, &cancelled_job_initial.job_id, &origin_project_id).unwrap()
        };
        assert_eq!(cancelled_job.state, "cancelled");

        // Attempt 3: Actual confined CPU generation -> Failed inside confined worker (invalid ONNX bytes).
        let invalid_onnx_path = root_dir.path().join("invalid-model.onnx");
        fs::write(&invalid_onnx_path, b"not-a-valid-onnx-protobuf-model\n").unwrap();
        let invalid_onnx_id = file_identity(&invalid_onnx_path);
        let mut failed_profile = explicit_profile.clone();
        failed_profile.model = invalid_onnx_id;
        let failed_settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            model_input: Some(failed_profile.preprocessing.input.clone()),
            inference_profile: Some(failed_profile),
            ..Default::default()
        };
        let ResponseBody::Job(failed_job_initial) = origin_engine
            .handle(authed_req(
                &origin_session,
                &origin_token,
                Some(&origin_project_id),
                Some(1),
                Command::Generate {
                    source_version: origin_source.clone(),
                    preset: "default".into(),
                    settings: Some(failed_settings),
                    model_path: Some(invalid_onnx_path),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let failed_deadline = Instant::now() + Duration::from_secs(30);
        let failed_job = loop {
            let snap = {
                let db = origin_engine.db.lock().unwrap();
                job_snapshot(&db, &failed_job_initial.job_id, &origin_project_id).unwrap()
            };
            if snap.state != "queued" && snap.state != "running" {
                break snap;
            }
            assert!(Instant::now() < failed_deadline, "failed CPU job timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(failed_job.state, "failed");
        let retire_deadline = Instant::now() + Duration::from_secs(20);
        while !origin_engine.workers.lock().unwrap().is_empty()
            || origin_engine
                .worker_fence
                .has_pending_worker_custody()
                .unwrap()
        {
            assert!(Instant::now() < retire_deadline);
            std::thread::sleep(Duration::from_millis(5));
        }

        // Attempt 4 (interrupted) and Attempt 5 (captured-running):
        // Clone the exact captured WorkerRequest from Attempt 1 and declare both states in cpu_runtime_attempts.
        {
            let mut db = origin_engine.db.lock().unwrap();
            let completed_raw: String = db
                .query_row(
                    "SELECT manifest FROM attempts WHERE id=?1",
                    [completed_job.attempt_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let template_req: WorkerRequest = decode(&completed_raw).unwrap();
            let tx = db.transaction().unwrap();
            for state in ["interrupted", "running"] {
                let mut req = template_req.clone();
                req.job_id = JobId::new(format!("job.cpu.{state}")).unwrap();
                req.attempt_id = AttemptId::new(format!("attempt.cpu.{state}")).unwrap();
                req.base_revision = RevisionId::new(1);
                req.output_dir = origin_state_dir
                    .join("attempts")
                    .join(req.attempt_id.as_str());
                fs::create_dir_all(&req.output_dir).unwrap();
                req.validate().unwrap();
                let raw = encode(&req).unwrap();
                tx.execute(
                    "INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,1,?4,?5,?6,?7)",
                    params![
                        req.job_id.as_str(),
                        req.attempt_id.as_str(),
                        origin_project_id.as_str(),
                        origin_session.as_str(),
                        origin_source.as_str(),
                        state,
                        raw,
                    ],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO attempts(id,job,manifest,state) VALUES(?1,?2,?3,?4)",
                    params![req.attempt_id.as_str(), req.job_id.as_str(), raw, state],
                )
                .unwrap();
                crate::cpu_runtime_db::declare(&tx, &req, &expected_image).unwrap();
            }
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                36
            );
        }

        // 2. Export format-15 package from origin Engine.
        let origin_plan = super::project_packages::capture(
            &origin_engine,
            &origin_session,
            &origin_token,
            &origin_project_id,
            RevisionId::new(1),
        )
        .unwrap();
        let parsed_manifest = origin_plan.manifest.clone();
        let mut object_bytes = BTreeMap::new();
        for desc in &parsed_manifest.objects {
            let mut buf = Vec::new();
            origin_plan
                .open_object(desc)
                .unwrap()
                .read_to_end(&mut buf)
                .unwrap();
            object_bytes.insert(desc.sha256.clone(), buf);
        }
        drop(origin_plan);
        let (origin_export_status, origin_package_bytes) = export_package_bytes(
            &origin_engine,
            &origin_session,
            &origin_token,
            &origin_project_id,
        );
        let origin_artifact = origin_export_status.artifact.clone().unwrap();
        assert_eq!(
            origin_artifact.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        {
            let db = origin_engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                39
            );
            assert_eq!(crate::portable_cpu_archives::version(&db).unwrap(), 36);
        }
        // Retire the synthetic running row before shutting down origin Engine so assert_engine_retired is clean.
        {
            let db = origin_engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET state='interrupted' WHERE id='job.cpu.running'",
                [],
            )
            .unwrap();
            db.execute(
                "UPDATE attempts SET state='interrupted' WHERE id='attempt.cpu.running'",
                [],
            )
            .unwrap();
        }
        let origin_pin = open_reader_pin(&origin_state_dir);
        drop_and_join_engine(origin_engine);

        // Make original host source, model, and policy files unavailable to prove clones never touch origin paths.
        fs::remove_file(&local_media).unwrap();
        fs::remove_file(&local_model).unwrap();
        fs::remove_file(&policy_path).unwrap();

        // 3a. Exercise preceding reader against WAL-pinned origin state directory.
        let origin_reader_probe = run_preceding_reader_engine(
            &preceding_pulsar,
            &preceding_worker,
            &origin_state_dir,
            origin_pin,
        );
        assert_eq!(origin_reader_probe["exit_code"], 1);
        assert_eq!(origin_reader_probe["identical_state_files"], true);
        assert_eq!(origin_reader_probe["reader39_in_committed_wal"], true);
        assert_eq!(origin_reader_probe["shm_diff"]["changed_byte_count"], 0);
        assert!(origin_reader_probe["stderr"]
            .as_str()
            .unwrap()
            .contains("unknown captured CPU live reader version"));

        // 3b. Exercise preceding reader CLI package-import against format-15 package file.
        let pkg_file_path = root_dir.path().join("origin-format15.pulspkg");
        fs::write(&pkg_file_path, &origin_package_bytes).unwrap();
        let cli_pin = open_reader_pin(&origin_state_dir);
        let mut cli_retained_fds = Vec::new();
        let pkg_pin_before = pin_file(&pkg_file_path, &mut cli_retained_fds);
        let origin_pins_before_cli = state_pins(&origin_state_dir, &mut cli_retained_fds);
        let cli_out = ProcessCommand::new(&preceding_pulsar)
            .arg("package-import")
            .arg(&pkg_file_path)
            .current_dir(&origin_state_dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &origin_state_dir)
            .env("PULSAR_STATE_DIR", &origin_state_dir)
            .output()
            .unwrap();
        let cli_stderr = String::from_utf8_lossy(&cli_out.stderr).trim().to_owned();
        assert_eq!(cli_out.status.code(), Some(1));
        assert!(
            cli_stderr.contains("engine startup exited with exit status: 1")
                || cli_stderr.contains("unsupported package format version")
                || cli_stderr.contains("package format version is not supported"),
            "unexpected CLI stderr: {cli_stderr}"
        );
        assert_eq!(
            pin_file(&pkg_file_path, &mut cli_retained_fds),
            pkg_pin_before
        );
        assert_eq!(
            state_pins(&origin_state_dir, &mut cli_retained_fds),
            origin_pins_before_cli
        );
        let _ = cli_pin.execute_batch("ROLLBACK;");
        drop(cli_pin);
        drop(cli_retained_fds);

        // 3c. Exercise preceding reader CLI package-import against format-15 package file with a running preceding-reader Engine (clean state) so PreparedPackageImport::open rejects format 15 before upload.
        let cli_clean_dir = root_dir.path().join("cli-clean-state");
        fs::create_dir_all(&cli_clean_dir).unwrap();
        fs::set_permissions(&cli_clean_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let cli_pkg_out = ProcessCommand::new(&preceding_pulsar)
            .arg("package-import")
            .arg(&pkg_file_path)
            .current_dir(&cli_clean_dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &cli_clean_dir)
            .env("PULSAR_STATE_DIR", &cli_clean_dir)
            .env("PULSAR_WORKER_EXECUTABLE", &preceding_worker)
            .output()
            .unwrap();
        let cli_pkg_stderr = String::from_utf8_lossy(&cli_pkg_out.stderr)
            .trim()
            .to_owned();
        assert_eq!(cli_pkg_out.status.code(), Some(1));
        assert!(
            cli_pkg_stderr.contains("package format version")
                || cli_pkg_stderr.contains("unsupported"),
            "unexpected CLI package format refusal stderr: {cli_pkg_stderr}"
        );
        let mut pkg_check_fds = Vec::new();
        assert_eq!(pin_file(&pkg_file_path, &mut pkg_check_fds), pkg_pin_before);
        drop(pkg_check_fds);

        // 4. Import into first clean clone Engine, join/shutdown, exercise preceding reader, and restart.
        let clone1_state_dir = root_dir.path().join("clone1-state");
        let clone1_worker = root_dir.path().join("inert-clone-worker");
        fs::write(&clone1_worker, b"clone engine never executes workers\n").unwrap();
        let clone1_cfg = EngineConfig::new(clone1_state_dir.clone(), clone1_worker.clone());
        let clone1_engine = Engine::open(clone1_cfg.clone()).unwrap();
        // Pin an initial read snapshot before format-15 import so the later committed schema-39 header stays in the WAL.
        let clone1_initial_pin = open_reader_pin(&clone1_state_dir);
        let (clone1_session, clone1_token) = pair_with_token(&clone1_engine, "clone1-owner");
        let clone1_import = import_package_bytes(
            &clone1_engine,
            &clone1_session,
            &clone1_token,
            &origin_package_bytes,
        );
        assert_eq!(
            clone1_import.state,
            PackageImportOperationState::Completed,
            "{:?}",
            clone1_import.error
        );
        let clone1_receipt = clone1_import.receipt.clone().unwrap();
        let clone1_project_id = clone1_receipt.project_id.clone();
        assert_ne!(clone1_project_id, origin_project_id);
        assert_eq!(
            clone1_receipt.package.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        {
            let db = clone1_engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                39
            );
            assert_eq!(crate::portable_cpu_archives::version(&db).unwrap(), 25);
        }
        let clone1_pin = open_reader_pin(&clone1_state_dir);
        drop_and_join_engine(clone1_engine);

        // 4b. Reproduce and record what happens WITHOUT the conditional schema-39 barrier on a copy of clone1,
        // proving why the conditional portable_cpu_reader (39) barrier is required to prevent preceding-reader
        // progression past open.state_probe into open.layout_probe.
        let prebarrier_dir = root_dir.path().join("prebarrier-reproduction");
        let copy_status = ProcessCommand::new("/bin/cp")
            .args([
                "-a",
                clone1_state_dir.to_str().unwrap(),
                prebarrier_dir.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(copy_status.success());
        let prebarrier_pin = {
            let pre_db = Connection::open(prebarrier_dir.join("projects.sqlite3")).unwrap();
            pre_db
                .execute_batch(
                    "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; DROP TABLE portable_cpu_reader; PRAGMA main.user_version=25;",
                )
                .unwrap();
            let pin = open_reader_pin(&prebarrier_dir);
            drop(pre_db);
            pin
        };
        let prebarrier_probe = run_preceding_reader_engine(
            &preceding_pulsar,
            &preceding_worker,
            &prebarrier_dir,
            prebarrier_pin,
        );
        assert_eq!(prebarrier_probe["exit_code"], 1);
        assert!(
            !prebarrier_probe["stderr"]
                .as_str()
                .unwrap()
                .contains("unknown captured CPU live reader version"),
            "without schema-39 barrier, preceding reader does not refuse at open.state_probe's reader header"
        );
        assert!(
            prebarrier_probe["stderr"]
                .as_str()
                .unwrap()
                .contains("portable CUDA root archive binding differs")
                || prebarrier_probe["stderr"]
                    .as_str()
                    .unwrap()
                    .contains("unknown")
                || !prebarrier_probe["stderr"].as_str().unwrap().is_empty(),
            "unexpected prebarrier stderr: {}",
            prebarrier_probe["stderr"]
        );

        // 4c. Exercise preceding reader against the actual WAL-pinned first-clone state directory (with schema-39 barrier).
        let clone1_reader_probe = run_preceding_reader_engine(
            &preceding_pulsar,
            &preceding_worker,
            &clone1_state_dir,
            clone1_pin,
        );
        let _ = clone1_initial_pin.execute_batch("ROLLBACK;");
        drop(clone1_initial_pin);
        assert_eq!(clone1_reader_probe["exit_code"], 1);
        assert_eq!(clone1_reader_probe["identical_state_files"], true);
        assert_eq!(clone1_reader_probe["reader39_in_committed_wal"], true);
        assert_eq!(clone1_reader_probe["shm_diff"]["changed_byte_count"], 0);
        assert!(clone1_reader_probe["stderr"]
            .as_str()
            .unwrap()
            .contains("unknown captured CPU live reader version"));

        // 4d. Restart first-clone Engine and verify exact correspondence, inert history, and re-export.
        let clone1_engine = Engine::open(clone1_cfg).unwrap();
        assert!(clone1_engine.cpu_owner_policy.is_none());
        let clone1_candidate_id: CandidateId = {
            let db = clone1_engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            let cpu_tables: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('cpu_runtime_reader','cpu_runtime_attempts','live_cpu_reader','live_cpu_admissions','inference_fallback_reader','inference_fallback_jobs','inference_fallback_attempts','inference_fallback_pending')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(cpu_tables, 0);
            let origin_sess_count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE id=?1",
                    [origin_session.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(origin_sess_count, 0);
            let cid: String = db
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [clone1_project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            CandidateId::new(cid).unwrap()
        };
        let ResponseBody::ArchivedGeneration(clone1_arch) = clone1_engine
            .handle(authed_req(
                &clone1_session,
                &clone1_token,
                Some(&clone1_project_id),
                None,
                Command::ArchivedGenerationStatus {
                    candidate_id: clone1_candidate_id.clone(),
                    master_revision: Some(RevisionId::new(1)),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("ArchivedGenerationStatus on clone1")
        };
        assert!(
            clone1_arch.archival
                && !clone1_arch.active_job
                && !clone1_arch.publication_authority
                && !clone1_arch.release_qualified
                && clone1_arch.master_derivation.is_some()
        );
        assert_eq!(clone1_arch.origins.len(), 1);
        assert_eq!(clone1_arch.origins[0].project_id, origin_project_id);
        assert_eq!(clone1_arch.origins[0].job_id, completed_job.job_id);
        assert_eq!(clone1_arch.origins[0].attempt_id, completed_job.attempt_id);
        assert_eq!(
            encode(&clone1_arch.origins[0].settings.inference_profile).unwrap(),
            encode(&Some(explicit_profile.clone())).unwrap()
        );

        let (clone1_export_status, clone1_package_bytes) = export_package_bytes(
            &clone1_engine,
            &clone1_session,
            &clone1_token,
            &clone1_project_id,
        );
        assert_eq!(
            clone1_export_status
                .artifact
                .as_ref()
                .unwrap()
                .format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        drop_and_join_engine(clone1_engine);

        // 5. Import into second clean clone Engine, restart, and verify two-hop original + ancestor correspondence.
        let clone2_state_dir = root_dir.path().join("clone2-state");
        let clone2_cfg = EngineConfig::new(clone2_state_dir.clone(), clone1_worker.clone());
        let clone2_engine = Engine::open(clone2_cfg.clone()).unwrap();
        let clone2_initial_pin = open_reader_pin(&clone2_state_dir);
        let (clone2_session, clone2_token) = pair_with_token(&clone2_engine, "clone2-owner");
        let clone2_import = import_package_bytes(
            &clone2_engine,
            &clone2_session,
            &clone2_token,
            &clone1_package_bytes,
        );
        assert_eq!(
            clone2_import.state,
            PackageImportOperationState::Completed,
            "{:?}",
            clone2_import.error
        );
        let clone2_project_id = clone2_import.receipt.as_ref().unwrap().project_id.clone();
        assert_ne!(clone2_project_id, clone1_project_id);
        assert_ne!(clone2_project_id, origin_project_id);
        let clone2_pin = open_reader_pin(&clone2_state_dir);
        drop_and_join_engine(clone2_engine);

        let clone2_reader_probe = run_preceding_reader_engine(
            &preceding_pulsar,
            &preceding_worker,
            &clone2_state_dir,
            clone2_pin,
        );
        let _ = clone2_initial_pin.execute_batch("ROLLBACK;");
        drop(clone2_initial_pin);
        assert_eq!(clone2_reader_probe["exit_code"], 1);
        assert_eq!(clone2_reader_probe["identical_state_files"], true);
        assert_eq!(clone2_reader_probe["reader39_in_committed_wal"], true);
        assert_eq!(clone2_reader_probe["shm_diff"]["changed_byte_count"], 0);

        let clone2_engine = Engine::open(clone2_cfg).unwrap();
        let clone2_candidate_id: CandidateId = {
            let db = clone2_engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            let cid: String = db
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [clone2_project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            CandidateId::new(cid).unwrap()
        };
        let ResponseBody::ArchivedGeneration(clone2_arch) = clone2_engine
            .handle(authed_req(
                &clone2_session,
                &clone2_token,
                Some(&clone2_project_id),
                None,
                Command::ArchivedGenerationStatus {
                    candidate_id: clone2_candidate_id,
                    master_revision: Some(RevisionId::new(1)),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("ArchivedGenerationStatus on clone2")
        };
        assert_eq!(clone2_arch.origins.len(), 1);
        assert_eq!(clone2_arch.origins[0].project_id, origin_project_id);
        assert_eq!(clone2_arch.origins[0].job_id, completed_job.job_id);
        assert_eq!(clone2_arch.origins[0].attempt_id, completed_job.attempt_id);
        assert_eq!(
            encode(&clone2_arch.origins[0].settings.inference_profile).unwrap(),
            encode(&Some(explicit_profile.clone())).unwrap()
        );

        let clone2_plan = super::project_packages::capture(
            &clone2_engine,
            &clone2_session,
            &clone2_token,
            &clone2_project_id,
            RevisionId::new(1),
        )
        .unwrap();
        assert_eq!(
            clone2_plan.manifest.format_version,
            PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY
        );
        assert_eq!(clone2_plan.manifest.imported_origins.len(), 2);
        assert!(clone2_plan.manifest.generated_origins.is_empty());
        assert!(clone2_plan.manifest.candidates[0]
            .lineage
            .contains(&expected_image_id));
        assert!(clone2_plan.manifest.candidates[0]
            .lineage
            .contains(&profile_id));

        // Verify all 5 attempt states and their exact CpuRuntimeImage declarations in the retained origin manifest inside clone2.
        let mut retained_states = BTreeSet::new();
        for descriptor in clone2_plan
            .manifest
            .objects
            .iter()
            .filter(|d| d.roles.contains(&PackageObjectRole::ImportedManifest))
        {
            let mut buf = Vec::new();
            clone2_plan
                .open_object(descriptor)
                .unwrap()
                .read_to_end(&mut buf)
                .unwrap();
            let ancestor: PortableProjectManifest = serde_json::from_slice(&buf).unwrap();
            for origin in &ancestor.generated_origins {
                assert_eq!(
                    origin.cpu_runtime.as_ref().unwrap().identity().unwrap(),
                    expected_image_id
                );
                retained_states.insert(origin.state.clone());
            }
        }
        assert_eq!(
            retained_states,
            BTreeSet::from([
                "completed".to_owned(),
                "cancelled".to_owned(),
                "failed".to_owned(),
                "interrupted".to_owned(),
                "running".to_owned(),
            ])
        );
        drop(clone2_plan);

        // 6. Verify actual package import refusals for stripped/mutated CPU image declarations and fallback tags.
        let mut mutated_manifest = parsed_manifest.clone();
        let fake_policy_sha = "f".repeat(64);
        let mutated_image = mutated_manifest.generated_origins[0]
            .cpu_runtime
            .as_mut()
            .unwrap();
        mutated_image.owner_policy.artifact_id = ArtifactId::new(fake_policy_sha.clone()).unwrap();
        mutated_image.owner_policy.sha256 = fake_policy_sha;
        let mut mutated_pkg = Vec::new();
        write_package(
            &mutated_manifest,
            &mut mutated_pkg,
            |desc| {
                Ok(Box::new(std::io::Cursor::new(
                    object_bytes[&desc.sha256].clone(),
                )))
            },
            PackageCodecLimits {
                max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
            },
        )
        .unwrap();
        let refused_import =
            import_package_bytes(&clone2_engine, &clone2_session, &clone2_token, &mutated_pkg);
        assert_eq!(refused_import.state, PackageImportOperationState::Failed);
        {
            let db = clone2_engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        drop_and_join_engine(clone2_engine);

        match prev_ffmpeg {
            Some(v) => std::env::set_var("PULSAR_FFMPEG", v),
            None => std::env::remove_var("PULSAR_FFMPEG"),
        }
        match prev_ffmpeg_bin {
            Some(v) => std::env::set_var("PULSAR_FFMPEG_BIN", v),
            None => std::env::remove_var("PULSAR_FFMPEG_BIN"),
        }
        match prev_ffprobe {
            Some(v) => std::env::set_var("PULSAR_FFPROBE", v),
            None => std::env::remove_var("PULSAR_FFPROBE"),
        }
        match prev_ffprobe_bin {
            Some(v) => std::env::set_var("PULSAR_FFPROBE_BIN", v),
            None => std::env::remove_var("PULSAR_FFPROBE_BIN"),
        }
        match prev_ort {
            Some(v) => std::env::set_var("PULSAR_ORT_DYLIB", v),
            None => std::env::remove_var("PULSAR_ORT_DYLIB"),
        }

        if let Ok(receipt_path) = std::env::var("PULSAR_PORTABLE_CPU_ROUNDTRIP_RECEIPT") {
            let report = json!({
                "status": "PASS",
                "task": "CPU-ROUNDTRIP",
                "qualified": false,
                "physical_device_contacted": false,
                "cpu_roundtrip_closed": true,
                "subjects": {
                    "worker": {
                        "path": worker_bin,
                        "sha256": worker_bin_id.sha256,
                        "byte_len": worker_bin_id.byte_len,
                    },
                    "preceding_reader_pulsar": {
                        "path": preceding_pulsar,
                        "sha256": preceding_pulsar_id.sha256,
                        "byte_len": preceding_pulsar_id.byte_len,
                    },
                    "preceding_reader_worker": {
                        "path": preceding_worker,
                        "sha256": preceding_worker_id.sha256,
                        "byte_len": preceding_worker_id.byte_len,
                    },
                    "cpu_owner_policy_sha256": owner.identity().sha256,
                    "cpu_runtime_image_sha256": expected_image_id.sha256,
                    "model_sha256": model_id.sha256,
                    "profile_sha256": profile_id.sha256,
                },
                "confined_cpu_attempts": {
                    "negative_missing_stdcpp_failed": neg_final.state == "failed",
                    "completed_attempt_id": completed_job.attempt_id,
                    "completed_successful_runs": completed_receipt_json["inference_execution"]["successful_runs"],
                    "cancelled_attempt_id": cancelled_job.attempt_id,
                    "failed_attempt_id": failed_job.attempt_id,
                    "retained_states_across_two_clones": retained_states,
                },
                "roundtrip": {
                    "origin_project_id": origin_project_id,
                    "origin_package_sha256": origin_artifact.sha256,
                    "origin_package_byte_len": origin_artifact.byte_len,
                    "clone1_project_id": clone1_project_id,
                    "clone1_package_sha256": clone1_export_status.artifact.as_ref().unwrap().sha256,
                    "clone2_project_id": clone2_project_id,
                    "clone2_imported_origins_count": 2,
                    "inert_history_verified": true,
                    "mutated_package_import_refused": true,
                },
                "preceding_reader_verification": {
                    "prebarrier_reproduction_mutated_fence_inode": prebarrier_probe["before"]["fence"]["inode"] != prebarrier_probe["after"]["fence"]["inode"],
                    "prebarrier_probe": prebarrier_probe,
                    "origin_state_probe": origin_reader_probe,
                    "clone1_state_probe": clone1_reader_probe,
                    "clone2_state_probe": clone2_reader_probe,
                    "cli_package_import_exit_code": cli_out.status.code(),
                    "cli_package_import_stderr": cli_stderr,
                    "cli_clean_state_package_import_exit_code": cli_pkg_out.status.code(),
                    "cli_clean_state_package_import_stderr": cli_pkg_stderr,
                },
            });
            fs::write(receipt_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
}
