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

fn require_fallback_disclosure_approval(
    db: &Connection,
    session: &SessionId,
    project: &ProjectId,
) -> PResult<()> {
    // PackageProject alone is not an implicit registry-disclosure capability.
    // Require the project-specific host-owner approval and its retained Ack.
    let allowed:bool=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.project_package_approvals a JOIN main.requests r ON r.session=a.session AND r.request=a.request JOIN main.request_identities i ON i.session=r.session AND i.request=r.request WHERE a.session=?1 AND a.project=?2 AND a.approval_kind='os_owner_capability' AND typeof(r.fingerprint)='text' AND length(r.fingerprint)=64 AND r.fingerprint=i.fingerprint AND r.response=?3)",
        params![session.as_str(),project.as_str(),encode(&ResponseBody::Ack)?],|r|r.get(0),
    ).map_err(internal)?;
    if !allowed {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "fallback evidence disclosure requires the project's host-owner packaging approval",
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
) -> PResult<BTreeMap<String, PortableProjectManifest>> {
    let mut index = crate::portable_cpu_archives::CpuArchiveIndex::default();
    let mut ancestors = BTreeMap::new();
    let mut ancestor_bytes = 0u64;
    for object in objects.values().filter(|o| {
        o.descriptor
            .roles
            .contains(&PackageObjectRole::ImportedManifest)
    }) {
        ancestor_bytes = ancestor_bytes
            .checked_add(object.descriptor.byte_len)
            .filter(|n| *n <= MAX_PACKAGE_ORIGIN_MANIFEST_BYTES)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "fallback ancestor metadata admission exceeded",
                )
            })?;
        if ancestors.len() >= MAX_PACKAGE_IMPORTED_ORIGINS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "fallback ancestor count admission exceeded",
            ));
        }
        let node: PortableProjectManifest = serde_json::from_slice(&cpu_mode_bytes(
            object,
            MAX_PROJECT_PACKAGE_MANIFEST_BYTES as u64,
        )?)
        .map_err(|_| ProtocolError::invalid("invalid portable ancestor metadata"))?;
        ancestors.insert(object.descriptor.sha256.clone(), node);
    }
    super::portable_fallback_payload::validate_metadata_graph(manifest, &ancestors)?;
    let mut fallback = super::portable_fallback_payload::GraphIndex::default();
    let read = |reference: &PackageObjectRef| {
        let object = objects
            .get(&reference.sha256)
            .filter(|o| o.descriptor.byte_len == reference.byte_len)
            .ok_or_else(|| ProtocolError::invalid("portable mode object absent"))?;
        cpu_mode_bytes(
            object,
            if object
                .descriptor
                .roles
                .contains(&PackageObjectRole::InferenceFallbackHistory)
            {
                MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64
            } else {
                1024 * 1024
            },
        )
    };
    if !objects.values().any(|o| {
        o.descriptor
            .roles
            .contains(&PackageObjectRole::InferenceFallbackHistory)
    }) {
        for object in objects.values().filter(|o| {
            o.descriptor
                .roles
                .contains(&PackageObjectRole::WorkerManifest)
        }) {
            let request: WorkerRequest =
                serde_json::from_slice(&cpu_mode_bytes(object, 1024 * 1024)?)
                    .map_err(|_| ProtocolError::invalid("invalid opaque worker"))?;
            crate::portable_cpu_archives::require_no_fallback(&request.dependencies)?;
        }
    }
    index.observe_manifest(manifest)?;
    for ancestor in ancestors.values() {
        index.observe_manifest(ancestor)?;
    }
    // Preserve mode-declaration refusal before reading unrelated payloads.
    index.finish()?;
    for object in objects.values().filter(|o| {
        o.descriptor
            .roles
            .contains(&PackageObjectRole::WorkerManifest)
    }) {
        let request: WorkerRequest = serde_json::from_slice(&cpu_mode_bytes(object, 1024 * 1024)?)
            .map_err(|_| ProtocolError::invalid("invalid opaque CPU worker"))?;
        index.check_worker(
            &PackageObjectRef {
                sha256: object.descriptor.sha256.clone(),
                byte_len: object.descriptor.byte_len,
            },
            &request,
        )?;
    }
    fallback.observe_node(manifest, read)?;
    for ancestor in ancestors.values() {
        fallback.observe_node(ancestor, read)?;
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
        fallback.check_worker(
            &PackageObjectRef {
                sha256: object.descriptor.sha256.clone(),
                byte_len: object.descriptor.byte_len,
            },
            &request,
        )?;
    }
    index.finish()?;
    for object in objects.values().filter(|o| {
        o.descriptor
            .roles
            .contains(&PackageObjectRole::InferenceFallbackHistory)
    }) {
        fallback.check_payload(&PackageObjectRef {
            sha256: object.descriptor.sha256.clone(),
            byte_len: object.descriptor.byte_len,
        })?;
    }
    fallback.finish()?;
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
    Ok(ancestors)
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

struct PreparedBrowserRecording {
    identity: ArtifactIdentity,
    raw: Arc<[u8]>,
    recording: super::portable_browser_history::Recording,
    inherited: bool,
}
/// Authorize/catalogue first, read immutable snapshot bytes without DB authority,
/// then recheck every tuple in the actual capture transaction. No registry bytes
/// are read until that transaction also checks host-owner disclosure approval.
fn prepare_browser_recordings(
    engine: &Engine,
    session: &SessionId,
    token: &str,
    project: &ProjectId,
    expected_revision: RevisionId,
) -> PResult<BTreeMap<SourceVersionId, PreparedBrowserRecording>> {
    let (catalog, inherited) = {
        let db = engine.db.lock().map_err(internal)?;
        authenticate_capture(engine, &db, session, token, project, expected_revision)?;
        let mut budget = SnapshotBudget::default();
        let catalog = rows(&db, "SELECT version,sha256,bytes,path,evicted FROM main.sources WHERE project=?1 AND kind='live_recording' ORDER BY version", project, &mut budget, |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?)))?;
        let archives = rows(&db, "SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 16777216 THEN manifest ELSE NULL END FROM main.project_import_archives WHERE project=?1", project, &mut budget, |r|r.get::<_,String>(0))?;
        let mut inherited = BTreeMap::new();
        for raw in archives {
            let manifest: PortableProjectManifest = decode(&raw)?;
            manifest.validate()?;
            for source in manifest.sources {
                inherited.insert(source.source_version.clone(), source);
            }
        }
        (catalog, inherited)
    };
    catalog
        .iter()
        .try_fold(0u64, |n, row| n.checked_add(u64::try_from(row.2).ok()?))
        .filter(|n| *n <= 64 * 1024 * 1024)
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "recording snapshot preflight admission exceeded",
            )
        })?;
    let mut prepared = BTreeMap::new();
    for (version, hash, len, path, evicted) in catalog {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).map_err(internal)?,
            sha256: hash.clone(),
            byte_len: unsigned(len)?,
        };
        identity.validate()?;
        let expected = engine.config.state_dir.join("snapshots").join(&hash);
        if evicted || Path::new(&path) != expected {
            return Err(ProtocolError::invalid(
                "recording snapshot locator changed or was evicted",
            ));
        }
        let object = CapturedObject {
            descriptor: PackageObjectDescriptor {
                sha256: hash,
                byte_len: identity.byte_len,
                roles: vec![PackageObjectRole::SourceSnapshot],
            },
            location: ObjectLocation::File(expected),
        };
        let raw = cpu_mode_bytes(&object, super::portable_browser_history::RECORDING_LIMIT)?;
        let recording = super::portable_browser_history::Recording::parse(&raw)?;
        let version = SourceVersionId::new(version).map_err(internal)?;
        let inherited = inherited
            .get(&version)
            .is_some_and(|s| s.kind == SourceKind::LiveRecording && s.identity == identity);
        if recording.project() != project && !inherited {
            return Err(ProtocolError::invalid(
                "recording snapshot lacks its original project namespace",
            ));
        }
        prepared.insert(
            version,
            PreparedBrowserRecording {
                identity,
                raw: Arc::from(raw),
                recording,
                inherited,
            },
        );
    }
    Ok(prepared)
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
    let mut browser_recordings =
        prepare_browser_recordings(engine, session, token, project, expected_revision)?;
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
        fallback_histories,
        wasm_invocations,
    ) = {
        let mut db = engine.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Deferred)
            .map_err(internal)?;
        authenticate_capture(engine, &tx, session, token, project, expected_revision)?;
        let inherited_fallback:bool=tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.project_import_archives WHERE project=?1 AND CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 16777216 AND json_valid(manifest) THEN json_extract(manifest,'$.format_version')>=16 ELSE 0 END)",
            [project.as_str()],|r|r.get(0),
        ).map_err(internal)?;
        if inherited_fallback {
            require_fallback_disclosure_approval(&tx, session, project)?;
        }
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
            let source_version=SourceVersionId::new(version).map_err(internal)?;
            let kind:SourceKind=decode(&encode(&kind)?)?;
            let mut source=PackageSource { browser_fallback_history:None, source_version:source_version.clone(),kind,label,identity,pinned,evicted,stash_associations:vec![] };
            let location=if kind == SourceKind::LiveRecording {
                let prepared=browser_recordings.remove(&source_version).ok_or_else(||ProtocolError::invalid("recording source appeared after snapshot preflight"))?;
                if prepared.identity != source.identity {return Err(ProtocolError::invalid("recording source identity changed after preflight"));}
                budget.bytes=budget.bytes.checked_add(prepared.raw.len().saturating_mul(2)).filter(|n|*n<=MAX_CAPTURE_METADATA).ok_or_else(||ProtocolError::new(ErrorCode::ResourceExhausted,"recording source metadata admission exceeded"))?;
                if !prepared.inherited {
                    for page in &prepared.recording.pages {crate::live_cpu_history::validate_recording_page(&tx,&prepared.recording.actor,page).map_err(internal)?;}
                    if prepared.recording.selected() {require_fallback_disclosure_approval(&tx,session,project)?;}
                    if let Some(raw)=super::portable_browser_history::capture(&tx,&source,&prepared.recording,MAX_CAPTURE_METADATA-budget.bytes)? {
                        budget.bytes=budget.bytes.checked_add(raw.len()).filter(|n|*n<=MAX_CAPTURE_METADATA).ok_or_else(||ProtocolError::new(ErrorCode::ResourceExhausted,"browser history capture admission exceeded"))?;
                        source.browser_fallback_history=Some(inline_object(&mut objects,raw,PackageObjectRole::BrowserInferenceFallbackHistory)?);
                    }
                }
                ObjectLocation::Inline(prepared.raw)
            } else {ObjectLocation::File(expected_path)};
            add_object(&mut objects,object_ref(&source.identity),PackageObjectRole::SourceSnapshot,location)?;
            sources.push(source);
        }
        if !browser_recordings.is_empty() {
            return Err(ProtocolError::invalid(
                "recording source set changed after preflight",
            ));
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
        let mut fallback_histories = BTreeMap::new();
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
            let mut needs_fallback = request.dependencies.iter().any(|d| {
                d.artifact_id
                    .as_str()
                    .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
            });
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
                    needs_fallback = true;
                }
            }
            if needs_fallback && !fallback_histories.contains_key(&job_id) {
                require_fallback_disclosure_approval(&tx, session, project)?;
                let available =
                    MAX_CAPTURE_METADATA
                        .checked_sub(budget.bytes)
                        .ok_or_else(|| {
                            ProtocolError::new(
                                ErrorCode::ResourceExhausted,
                                "fallback capture admission exhausted",
                            )
                        })?;
                let bytes =
                    super::portable_fallback_payload::capture(&tx, project, &job_id, available)?
                        .ok_or_else(|| {
                            ProtocolError::unsupported(
                                "portable inference fallback history has no original declaration",
                            )
                        })?;
                budget.bytes = budget
                    .bytes
                    .checked_add(bytes.len())
                    .filter(|n| *n <= MAX_CAPTURE_METADATA)
                    .ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "fallback capture metadata admission exceeded",
                        )
                    })?;
                let reference = inline_object(
                    &mut objects,
                    bytes,
                    PackageObjectRole::InferenceFallbackHistory,
                )?;
                fallback_histories.insert(job_id.clone(), reference);
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
                    &request
                        .dependencies
                        .iter()
                        .filter(|d| {
                            !fallback_histories.contains_key(&job_id)
                                || !d
                                    .artifact_id
                                    .as_str()
                                    .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
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
                "failed_or_unknown" | "destination_exists"=>PackageExportState::FailedOrUnknown,_=>return Err(ProtocolError::unsupported("unknown archival export state"))};
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
            fallback_histories,
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
        let fallback_history = fallback_histories.get(&request.job_id).cloned();
        if fallback_history.is_none() {
            crate::portable_cpu_archives::require_no_fallback(&request.dependencies)?;
        }
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
            fallback_history,
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
    if !fallback_histories.is_empty() {
        manifest.objects = objects
            .values()
            .map(|object| object.descriptor.clone())
            .collect();
        super::portable_fallback_payload::validate_node(&manifest, |reference| {
            let object = objects
                .get(&reference.sha256)
                .filter(|o| o.descriptor.byte_len == reference.byte_len)
                .ok_or_else(|| ProtocolError::invalid("fallback captured object absent"))?;
            cpu_mode_bytes(
                object,
                if object
                    .descriptor
                    .roles
                    .contains(&PackageObjectRole::InferenceFallbackHistory)
                {
                    MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64
                } else {
                    1024 * 1024
                },
            )
        })?;
    }
    crate::project_package_import_plan::require_portable_cpu_manifest(&manifest)?;
    let browser_ancestors = require_portable_cpu_objects(&manifest, &objects)?;
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
    super::portable_browser_history::validate_graph(&manifest, &browser_ancestors, |reference| {
        let object = objects
            .get(&reference.sha256)
            .filter(|o| o.descriptor.byte_len == reference.byte_len)
            .ok_or_else(|| ProtocolError::invalid("browser graph object absent"))?;
        cpu_mode_bytes(
            object,
            if object
                .descriptor
                .roles
                .contains(&PackageObjectRole::BrowserInferenceFallbackHistory)
            {
                MAX_PACKAGE_FALLBACK_HISTORY_BYTES as u64
            } else {
                super::portable_browser_history::RECORDING_LIMIT
            },
        )
    })?;

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
            if original.stash_associations.is_empty() && original.browser_fallback_history.is_none()
            {
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
            if local.browser_fallback_history.is_some()
                && local.browser_fallback_history != original.browser_fallback_history
            {
                return Err(ProtocolError::invalid(
                    "archived browser evidence changed during re-export",
                ));
            }
            local.browser_fallback_history = original.browser_fallback_history.clone();
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
#[path = "project_packages_tests.rs"]
mod capture_review_tests;
