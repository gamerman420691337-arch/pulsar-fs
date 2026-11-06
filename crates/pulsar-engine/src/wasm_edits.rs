//! Authenticated values-only plugin proposals. This module never commits master motion.
use super::*;

const MAX_OPERATIONS: i64 = 64;
const MAX_RETAINED_INVOCATIONS: i64 = 256;
#[path = "wasm_edit_archives.rs"]
mod archives;
#[path = "wasm_start_resolution.rs"]
mod start_resolution;
const MAX_METADATA: usize = 16 * 1024;
#[cfg(all(feature = "wasm", target_os = "linux"))]
const EDIT_MEMORY: u64 = transfers::VALIDATION_MEMORY;
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS wasm_edits(
    id TEXT PRIMARY KEY NOT NULL,project TEXT NOT NULL REFERENCES projects(id),
    session TEXT NOT NULL REFERENCES sessions(id),base_revision INTEGER NOT NULL,
    engine_epoch TEXT NOT NULL,request_id TEXT NOT NULL,request_sha256 TEXT NOT NULL,
    manifest TEXT NOT NULL,module BLOB NOT NULL,input BLOB NOT NULL,
    base_program TEXT NOT NULL,expected_worker TEXT NOT NULL,guest_memory_bytes INTEGER NOT NULL,
    label TEXT NOT NULL,state TEXT NOT NULL,candidate TEXT REFERENCES candidates(id),
    error TEXT,receipt TEXT,output BLOB,UNIQUE(session,request_id),
    FOREIGN KEY(project,base_revision) REFERENCES revisions(project,revision));";
const COLUMNS: &[&str] = &[
    "id",
    "project",
    "session",
    "base_revision",
    "engine_epoch",
    "request_id",
    "request_sha256",
    "manifest",
    "module",
    "input",
    "base_program",
    "expected_worker",
    "guest_memory_bytes",
    "label",
    "state",
    "candidate",
    "error",
    "receipt",
    "output",
];

#[derive(Clone)]
struct Plan {
    attempt: AttemptId,
    project: ProjectId,
    owner: SessionId,
    base: RevisionId,
    epoch: String,
    request_id: RequestId,
    fingerprint: String,
    manifest: ExtensionManifest,
    module: Vec<u8>,
    input: Vec<u8>,
    base_program: ProgramDescriptor,
    worker: ArtifactIdentity,
    guest_memory_bytes: u64,
    label: String,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn key(plan: &Plan) -> String {
    format!("wasm-edit:{}:{}:{}", plan.project, plan.owner, plan.attempt)
}
fn state(value: &str) -> PResult<WasmEditState> {
    match value {
        "running" => Ok(WasmEditState::Running),
        "completed" => Ok(WasmEditState::Completed),
        "failed" => Ok(WasmEditState::Failed),
        "cancelled" => Ok(WasmEditState::Cancelled),
        "interrupted" => Ok(WasmEditState::Interrupted),
        _ => Err(internal("unknown WASM edit state")),
    }
}
fn read_plan(db: &Connection, id: &AttemptId) -> PResult<Plan> {
    db.query_row("SELECT project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label FROM wasm_edits WHERE id=?1",
        [id.as_str()],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,
            row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?,
            row.get::<_,Vec<u8>>(7)?,row.get::<_,Vec<u8>>(8)?,row.get::<_,String>(9)?,row.get::<_,String>(10)?,
            row.get::<_,i64>(11)?,row.get::<_,String>(12)?)))
        .optional().map_err(internal)?.ok_or_else(||ProtocolError::new(ErrorCode::NotFound,"WASM edit operation absent"))
        .and_then(|(project,owner,base,epoch,request,fingerprint,manifest,module,input,base_program,worker,memory,label)|{
            Ok(Plan { attempt:id.clone(),project:ProjectId::new(project).map_err(internal)?,owner:SessionId::new(owner).map_err(internal)?,
                base:RevisionId::new(u64::try_from(base).map_err(internal)?),epoch,request_id:RequestId::new(request).map_err(internal)?,
                fingerprint,manifest:decode(&manifest)?,module,input,base_program:decode(&base_program)?,worker:decode(&worker)?,
                guest_memory_bytes:u64::try_from(memory).map_err(internal)?,label })
        })
}
fn report(db: &Connection, plan: &Plan) -> PResult<WasmEditReport> {
    let (raw, candidate, error, receipt): (String, Option<String>, Option<String>, Option<String>) =
        db.query_row(
            "SELECT state,candidate,error,receipt FROM wasm_edits WHERE id=?1",
            [plan.attempt.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(internal)?;
    let report = WasmEditReport {
        attempt_id: plan.attempt.clone(),
        request_id: plan.request_id.clone(),
        project_id: plan.project.clone(),
        base_revision: plan.base,
        engine_epoch: plan.epoch.clone(),
        state: state(&raw)?,
        candidate_id: candidate
            .map(CandidateId::new)
            .transpose()
            .map_err(internal)?,
        receipt: receipt.map(|r| decode(&r)).transpose()?,
        error: error.map(|r| decode(&r)).transpose()?,
    };
    report.validate()?;
    Ok(report)
}

/// Called on the read-only startup probe before any writable SQLite effects.
pub(super) fn validate(db: &Connection) -> PResult<()> {
    archives::validate(db)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='wasm_edits')",
            [],
            |r| r.get(0),
        )
        .map_err(internal)?;
    if !exists {
        return Ok(());
    }
    // Compare against SQLite's own normalized declaration, including foreign
    // keys, uniqueness, affinities and nullability. Column names alone cannot
    // establish the retained catalog's integrity constraints.
    let reference = Connection::open_in_memory().map_err(internal)?;
    reference.execute_batch(SCHEMA).map_err(internal)?;
    let declaration = |connection: &Connection| {
        connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='wasm_edits'",
            [],
            |r| r.get::<_, String>(0),
        )
    };
    if declaration(db).map_err(internal)? != declaration(&reference).map_err(internal)? {
        return Err(internal("WASM edit schema requires explicit recovery"));
    }
    let mut columns = db
        .prepare("PRAGMA table_info(wasm_edits)")
        .map_err(internal)?;
    let names = columns
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(internal)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(internal)?;
    if names.iter().map(String::as_str).collect::<Vec<_>>() != COLUMNS {
        return Err(internal("WASM edit schema requires explicit recovery"));
    }
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM wasm_edits", [], |r| r.get(0))
        .map_err(internal)?;
    if count > MAX_RETAINED_INVOCATIONS || archives::operational_count(db)? > MAX_OPERATIONS {
        return Err(internal(
            "WASM edit catalog exceeds retained operation bound",
        ));
    }
    let malformed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM wasm_edits WHERE
        typeof(module)<>'blob' OR length(module)>?1 OR typeof(input)<>'blob' OR length(input)>?2
        OR (output IS NOT NULL AND (typeof(output)<>'blob' OR length(output)>?2))
        OR length(manifest)>?3 OR length(base_program)>?3 OR length(expected_worker)>?3
        OR length(label)>256 OR length(engine_epoch)>128 OR length(request_sha256)<>64
        OR length(error)>?3 OR length(receipt)>?3
        OR (state<>'completed' AND (candidate IS NOT NULL OR receipt IS NOT NULL OR output IS NOT NULL))
        OR (state='completed' AND output IS NULL))",params![WASM_MODULE_MAX_BYTES as i64,WASM_BUFFER_MAX_BYTES as i64,MAX_METADATA as i64],|r|r.get(0)).map_err(internal)?;
    if malformed {
        return Err(internal("WASM edit metadata or byte bounds changed"));
    }
    let mut query = db.prepare("SELECT id FROM wasm_edits").map_err(internal)?;
    let ids = query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(internal)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(internal)?;
    for id in ids {
        let plan = read_plan(db, &AttemptId::new(id).map_err(internal)?)?;
        validate_sha256(&plan.fingerprint)?;
        let spec = WasmEditSpec {
            manifest: plan.manifest.clone(),
            module: plan.module.clone(),
            guest_memory_bytes: plan.guest_memory_bytes,
            expected_worker: plan.worker.clone(),
            approve_execution: true,
            label: plan.label.clone(),
        };
        spec.validate()?;
        plan.base_program.validate()?;
        if motion_state::revision_descriptor(db, &plan.project, plan.base)? != plan.base_program {
            return Err(internal("WASM edit base differs from retained revision"));
        }
        if hash(&plan.module) != plan.manifest.artifact.sha256 {
            return Err(internal("retained WASM module identity changed"));
        }
        let _: pulsar_core::EditValuesProgram =
            serde_json::from_slice(&plan.input).map_err(internal)?;
        let view = report(db, &plan)?;
        if let Some(receipt) = &view.receipt {
            let output: Option<Vec<u8>> = db
                .query_row(
                    "SELECT output FROM wasm_edits WHERE id=?1",
                    [plan.attempt.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            let output =
                output.ok_or_else(|| internal("completed WASM edit lost original output"))?;
            if receipt.module != plan.manifest.artifact
                || receipt.worker != plan.worker
                || receipt.base_program != plan.base_program
                || receipt.request_sha256 != plan.fingerprint
                || receipt.input_sha256 != hash(&plan.input)
                || receipt.input_byte_len != plan.input.len() as u64
                || receipt.output_sha256 != hash(&output)
                || receipt.output_byte_len != output.len() as u64
            {
                return Err(internal(
                    "WASM edit receipt differs from retained invocation",
                ));
            }
            let candidate =
                candidate_snapshot(db, view.candidate_id.as_ref().unwrap(), &plan.project)?;
            if candidate.base_revision != plan.base
                || candidate.motion.program != receipt.result_program
            {
                return Err(internal("WASM edit candidate association changed"));
            }
            let lineage_receipt: String = db
                .query_row(
                    "SELECT receipt FROM edit_candidate_lineage WHERE candidate=?1",
                    [view.candidate_id.as_ref().unwrap().as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if decode::<ArtifactIdentity>(&lineage_receipt)? != receipt.edit_receipt {
                return Err(internal("WASM edit core receipt association changed"));
            }
            let _: pulsar_core::EditValuesProgram =
                serde_json::from_slice(&output).map_err(internal)?;
        }
    }
    Ok(())
}
pub(super) fn initialize(db: &mut Connection) -> PResult<()> {
    validate(db)?;
    let tx = db.transaction().map_err(internal)?;
    tx.execute_batch(SCHEMA).map_err(internal)?;
    archives::initialize(&tx)?;
    let error = encode(&ProtocolError::new(
        ErrorCode::Unavailable,
        "engine restart interrupted WASM edit; fresh approval is required",
    ))?;
    tx.execute(
        "UPDATE wasm_edits SET state='interrupted',error=?1 WHERE state='running'",
        [error],
    )
    .map_err(internal)?;
    tx.commit().map_err(internal)?;
    Ok(())
}
pub(super) fn revoke(
    tx: &Transaction<'_>,
    owner: &SessionId,
    project: &ProjectId,
    workers: &Mutex<HashMap<String, Arc<AtomicBool>>>,
) -> PResult<Vec<Arc<AtomicBool>>> {
    let error = encode(&ProtocolError::new(
        ErrorCode::Cancelled,
        "authorizing WASM edit grant revoked",
    ))?;
    tx.execute("UPDATE wasm_edits SET state='cancelled',error=?3 WHERE session=?1 AND project=?2 AND state IN ('running','interrupted')",
        params![owner.as_str(),project.as_str(),error]).map_err(internal)?;
    let prefix = format!("wasm-edit:{project}:{owner}:");
    // Return captured handles; the enclosing authority transaction owns COMMIT.
    Ok(workers
        .lock()
        .map_err(internal)?
        .iter()
        .filter(|(name, _)| name.starts_with(&prefix))
        .map(|(_, flag)| flag.clone())
        .collect())
}

impl Engine {
    pub(super) fn wasm_edit_control(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let owner = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        if matches!(request.command, Command::WasmEditorInfo) {
            #[cfg(all(feature = "wasm", target_os = "linux"))]
            let supported = crate::launch::available()
                && crate::wasm_launch::executable_size_admitted(self.binary_identity.byte_len);
            #[cfg(not(all(feature = "wasm", target_os = "linux")))]
            let supported = false;
            return Ok(ResponseBody::WasmEditorInfo(WasmEditorInfo {
                worker: self.binary_identity.clone(),
                supported,
                abi: WASM_PROCESSING_ABI.into(),
                maximum_buffer_bytes: WASM_BUFFER_MAX_BYTES as u64,
                native_runtime_qualified: false,
            }));
        }
        if matches!(request.command, Command::ResolveWasmStart { .. }) {
            return self.resolve_wasm_start(request, fingerprint, owner);
        }
        if let Command::StartWasmEdit { spec } = &request.command {
            #[cfg(all(feature = "wasm", target_os = "linux"))]
            {
                return self.start_wasm_edit(request, fingerprint, spec);
            }
            #[cfg(not(all(feature = "wasm", target_os = "linux")))]
            {
                let _ = spec;
                return Err(ProtocolError::unsupported(
                    "authenticated WASM editing requires the WASM build and Linux confinement",
                ));
            }
        }
        if matches!(request.command, Command::ListArchivedWasmEdits { .. }) {
            return self.list_archived_wasm(request, owner);
        }
        if matches!(request.command, Command::ArchiveCompletedWasmEdit { .. }) {
            return self.archive_completed_wasm(request, fingerprint, owner);
        }
        if matches!(
            request.command,
            Command::ListWasmEdits | Command::RemoveUnsuccessfulWasmEdit { .. }
        ) {
            return self.wasm_edit_catalog_control(request, fingerprint, owner);
        }
        let id = match &request.command {
            Command::WasmEditStatus { attempt_id } | Command::CancelWasmEdit { attempt_id } => {
                attempt_id
            }
            _ => return Err(ProtocolError::invalid("not a WASM edit operation")),
        };
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if request.command.is_mutating() {
            if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
                return Ok(body);
            }
        }
        let plan = read_plan(&tx, id)?;
        if &plan.owner != owner || &plan.project != project(request)? {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "WASM edit belongs to another session or project",
            ));
        }
        let old = report(&tx, &plan)?;
        let mut cancel_after_commit = None;
        if matches!(request.command, Command::CancelWasmEdit { .. })
            && matches!(
                old.state,
                WasmEditState::Running | WasmEditState::Interrupted
            )
        {
            let error = encode(&ProtocolError::new(
                ErrorCode::Cancelled,
                "WASM edit cancelled by authorized client",
            ))?;
            tx.execute(
                "UPDATE wasm_edits SET state='cancelled',error=?2 WHERE id=?1",
                params![id.as_str(), error],
            )
            .map_err(internal)?;
            cancel_after_commit = self
                .workers
                .lock()
                .map_err(internal)?
                .get(&key(&plan))
                .cloned();
        }
        let body = ResponseBody::WasmEdit((report(&tx, &plan)?).into());
        if request.command.is_mutating() {
            remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        }
        tx.commit().map_err(internal)?;
        // Signal the captured handle only after durable state/receipt publication,
        // before the database mutex releases the completion fence.
        if let Some(flag) = cancel_after_commit {
            flag.store(true, Ordering::Release);
        }
        Ok(body)
    }
}

#[cfg(all(feature = "wasm", target_os = "linux"))]
#[path = "wasm_edit_execution.rs"]
mod execution;

pub(super) struct PortableInvocation {
    pub metadata: PackageWasmInvocation,
    pub module: Vec<u8>,
    pub input: Vec<u8>,
}
/// Snapshot original facts under the caller's already-authorized SQL snapshot.
/// No credentials or executable request are returned.
pub(super) fn portable_invocations(
    db: &Connection,
    project: &ProjectId,
) -> PResult<std::collections::BTreeMap<String, PortableInvocation>> {
    validate(db)?;
    let mut query = db
        .prepare("SELECT id FROM wasm_edits WHERE project=?1 AND state='completed' ORDER BY id")
        .map_err(internal)?;
    let ids = query
        .query_map([project.as_str()], |r| r.get::<_, String>(0))
        .map_err(internal)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(internal)?;
    let mut invocations = std::collections::BTreeMap::new();
    for id in ids {
        let plan = read_plan(db, &AttemptId::new(id).map_err(internal)?)?;
        let report = report(db, &plan)?;
        let receipt = report.receipt.unwrap();
        let metadata = PackageWasmInvocation {
            format: "pulsar-wasm-invocation-v1".into(),
            attempt_id: plan.attempt,
            request_id: plan.request_id,
            engine_epoch: plan.epoch,
            origin_candidate_id: report.candidate_id.unwrap(),
            base_revision: plan.base,
            manifest: plan.manifest,
            guest_memory_bytes: plan.guest_memory_bytes,
            approve_execution: true,
            approved_worker: plan.worker,
            input: PackageObjectRef {
                sha256: receipt.input_sha256.clone(),
                byte_len: receipt.input_byte_len,
            },
            receipt,
        };
        metadata.validate()?;
        let key = metadata.receipt.edit_receipt.sha256.clone();
        if invocations
            .insert(
                key,
                PortableInvocation {
                    metadata,
                    module: plan.module,
                    input: plan.input,
                },
            )
            .is_some()
        {
            return Err(internal(
                "multiple WASM operations claim the same core edit receipt",
            ));
        }
    }
    Ok(invocations)
}

impl Engine {
    fn wasm_edit_catalog_control(
        &self,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if request.command.is_mutating() {
            if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
                return Ok(body);
            }
        }
        let body = match &request.command {
            Command::ListWasmEdits => {
                let mut statement = tx.prepare("SELECT id FROM wasm_edits WHERE project=?1 AND session=?2 AND NOT EXISTS(SELECT 1 FROM wasm_edit_archives WHERE attempt=wasm_edits.id) ORDER BY id LIMIT 65").map_err(internal)?;
                let ids = statement
                    .query_map(params![project(request)?.as_str(), owner.as_str()], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(internal)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(internal)?;
                if ids.len() > MAX_OPERATIONS as usize {
                    return Err(internal("WASM catalog exceeds discovery bound"));
                }
                let operations = ids
                    .into_iter()
                    .map(|id| {
                        let plan = read_plan(&tx, &AttemptId::new(id).map_err(internal)?)?;
                        report(&tx, &plan)
                    })
                    .collect::<PResult<Vec<_>>>()?;
                let catalog = WasmEditCatalog {
                    project_id: project(request)?.clone(),
                    operations,
                };
                catalog.validate_for(request)?;
                ResponseBody::WasmEditCatalog(catalog)
            }
            Command::RemoveUnsuccessfulWasmEdit {
                attempt_id,
                original_request_id,
                expected_state,
            } => {
                let plan = read_plan(&tx, attempt_id)?;
                if &plan.owner != owner || &plan.project != project(request)? {
                    return Err(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "WASM edit belongs to another session or project",
                    ));
                }
                let old = report(&tx, &plan)?;
                if &old.request_id != original_request_id
                    || &old.state != expected_state
                    || !old.state.is_unsuccessful_terminal()
                {
                    return Err(ProtocolError::new(ErrorCode::RequestConflict,"WASM removal requires the observed unsuccessful terminal state and original request"));
                }
                if self
                    .workers
                    .lock()
                    .map_err(internal)?
                    .contains_key(&key(&plan))
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "WASM worker retirement is still pending",
                    ));
                }
                #[cfg(target_os = "linux")]
                if self
                    .worker_fence
                    .has_pending_worker_custody()
                    .map_err(internal)?
                {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "worker custody retirement or creation is still pending",
                    ));
                }
                old.validate_removal_for(request)?;
                let removed = tx.execute("DELETE FROM wasm_edits WHERE id=?1 AND session=?2 AND project=?3 AND state=?4 AND request_id=?5",params![attempt_id.as_str(),owner.as_str(),project(request)?.as_str(),encode_state(&old.state),original_request_id.as_str()]).map_err(internal)?;
                if removed != 1 {
                    return Err(internal(
                        "WASM removal did not remove exactly the selected operation",
                    ));
                }
                ResponseBody::WasmEditRemoved((old).into())
            }
            _ => return Err(ProtocolError::invalid("not a WASM catalog operation")),
        };
        if request.command.is_mutating() {
            remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        }
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}
fn encode_state(state: &WasmEditState) -> &'static str {
    match state {
        WasmEditState::Running => "running",
        WasmEditState::Completed => "completed",
        WasmEditState::Failed => "failed",
        WasmEditState::Cancelled => "cancelled",
        WasmEditState::Interrupted => "interrupted",
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn retained_schema_constraints_are_checked_without_mutation() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&mut db).unwrap();
        validate(&db).unwrap();
        db.execute_batch("ALTER TABLE wasm_edits RENAME TO prior_wasm_edits")
            .unwrap();
        db.execute_batch(&SCHEMA.replace("UNIQUE(session,request_id),", ""))
            .unwrap();
        let before: i64 = db
            .query_row("PRAGMA schema_version", [], |r| r.get(0))
            .unwrap();
        assert!(validate(&db).is_err());
        assert_eq!(
            before,
            db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
                .unwrap()
        );
    }
    #[test]
    fn absent_catalog_and_exact_empty_catalog_are_accepted() {
        let db = Connection::open_in_memory().unwrap();
        validate(&db).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        validate(&db).unwrap();
        db.execute_batch("ALTER TABLE wasm_edits ADD COLUMN unreviewed TEXT")
            .unwrap();
        assert!(validate(&db).is_err());
    }
}
