//! Full-byte, projectless clone import. Archived identifiers never confer authority.
use super::*;
use crate::project_package_format::PackageCodecLimits;
use crate::project_package_import_plan::{plan_clone_import, CloneIdentityAssignment};
use crate::project_package_import_storage::{
    ImportStoragePhase, ImportStore, ProvisionalImportUpload, PublishedImportObjects,
    VerifiedImportBytes,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::time::{Duration, Instant};

const MAX_IMPORTS: i64 = 10_000;
const MAX_ACTIVE_IMPORTS: usize = 2;
const IMPORT_MEMORY: u64 = 1024 * 1024 * 1024;
const UPLOAD_LIFETIME: Duration = Duration::from_secs(30 * 60);
const IMPORT_LIFETIME: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub(super) struct ImportRuntime {
    active: HashMap<String, Arc<ActiveImport>>,
}
impl ImportRuntime {
    pub(super) fn cancel_all(&mut self) {
        for operation in self.active.values() {
            operation.cancel.store(true, Ordering::Release);
        }
    }
}
struct ActiveImport {
    operation: PackageImportOperationId,
    session: SessionId,
    declaration: PackageUploadDeclaration,
    cancel: AtomicBool,
    worker: AtomicBool,
    deadline: Instant,
    reserved_bytes: std::sync::atomic::AtomicU64,
    _reservation: Reservation,
    data: Mutex<UploadData>,
}
#[derive(Default)]
struct UploadData {
    generation: u64,
    lease: Option<PackageUploadLease>,
    connection: Option<String>,
    lease_deadline: Option<Instant>,
    upload: Option<ProvisionalImportUpload>,
}
pub(crate) struct ImportConnection {
    active: Arc<ActiveImport>,
    generation: u64,
    nonce: String,
}
impl Drop for ImportConnection {
    fn drop(&mut self) {
        if let Ok(mut data) = self.active.data.lock() {
            if data.connection.as_deref() == Some(&self.nonce) {
                data.connection = None;
            }
        }
    }
}
fn unfinished(state: PackageImportOperationState) -> bool {
    matches!(
        state,
        PackageImportOperationState::AwaitingUpload
            | PackageImportOperationState::Uploading
            | PackageImportOperationState::Sealing
            | PackageImportOperationState::Sealed
            | PackageImportOperationState::Validating
            | PackageImportOperationState::Publishing
    )
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::StartProjectImport { .. }
            | Command::ProjectImportStatus { .. }
            | Command::BeginProjectPackageUpload { .. }
            | Command::ProjectPackageUploadStatus { .. }
            | Command::AbandonProjectPackageUpload { .. }
            | Command::SealProjectImport { .. }
            | Command::CancelProjectImport { .. }
    )
}
pub(super) fn initialize(db: &Connection) -> PResult<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS project_package_imports(
        id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES sessions(id),request TEXT NOT NULL,status TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS project_import_archives(
        project TEXT PRIMARY KEY REFERENCES projects(id),captured_revision INTEGER NOT NULL,
        manifest TEXT NOT NULL,origin_map TEXT NOT NULL,package TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS project_import_candidate_aliases(
        candidate TEXT PRIMARY KEY REFERENCES candidates(id),project TEXT NOT NULL REFERENCES projects(id),
        imported_candidate TEXT NOT NULL,review_state TEXT NOT NULL);").map_err(internal)?;
    let mut statement = db
        .prepare("SELECT id,status FROM project_package_imports")
        .map_err(internal)?;
    let mut rows = statement.query([]).map_err(internal)?;
    let mut changes = Vec::new();
    let mut count = 0usize;
    while let Some(row) = rows.next().map_err(internal)? {
        count += 1;
        if count > MAX_IMPORTS as usize {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "import operation journal exceeds bound",
            ));
        }
        let raw: String = row.get(1).map_err(internal)?;
        if raw.len() > 16 * 1024 {
            return Err(ProtocolError::invalid("import status exceeds bound"));
        }
        let mut status: PackageImportStatus = decode(&raw)?;
        status.validate()?;
        if unfinished(status.state) {
            status.state = PackageImportOperationState::Interrupted;
            status.error = Some(unavailable(
                "engine restarted before clone publication; a new operation must upload all bytes",
            ));
            changes.push((row.get::<_, String>(0).map_err(internal)?, status));
        }
    }
    drop(rows);
    drop(statement);
    for (id, status) in changes {
        db.execute(
            "UPDATE project_package_imports SET status=?2 WHERE id=?1",
            params![id, encode(&status)?],
        )
        .map_err(internal)?;
    }
    Ok(())
}
fn owned(
    db: &Connection,
    operation: &PackageImportOperationId,
    session: &SessionId,
) -> PResult<PackageImportStatus> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT session,status FROM project_package_imports WHERE id=?1",
            [operation.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(internal)?;
    let (owner, raw) =
        row.ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "clone import not found"))?;
    if owner != session.as_str() {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "clone import belongs to another session",
        ));
    }
    let status: PackageImportStatus = decode(&raw)?;
    status.validate()?;
    Ok(status)
}
fn save(db: &Connection, status: &PackageImportStatus) -> PResult<()> {
    status.validate()?;
    let raw = encode(status)?;
    if raw.len() > 16 * 1024 {
        return Err(ProtocolError::invalid("import status exceeds bound"));
    }
    db.execute(
        "UPDATE project_package_imports SET status=?2 WHERE id=?1",
        params![status.operation_id.as_str(), raw],
    )
    .map_err(internal)?;
    Ok(())
}
fn spawn(owner: &Engine, task: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_SPAWN.with(|flag| flag.replace(false)) {
        return Err(std::io::Error::other("injected import spawn failure"));
    }
    owner.spawn_task("pulsar-clone-import", None, task)
}
#[cfg(test)]
thread_local! { static FAIL_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

impl Engine {
    pub(super) fn import_control(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let session = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing required"))?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                return match body {
                    ResponseBody::PackageImport(old) => owned(&db, &old.operation_id, session)
                        .map(|value| ResponseBody::PackageImport(value.into())),
                    ResponseBody::PackageUpload(old) => {
                        drop(db);
                        self.import_lease_view(session, &old.lease_id)
                            .map(ResponseBody::PackageUpload)
                    }
                    other => Ok(other),
                };
            }
        }
        match &request.command {
            Command::StartProjectImport { package } => {
                self.start_import(request, fingerprint, package)
            }
            Command::ProjectImportStatus { operation_id } => {
                let db = self.db.lock().map_err(internal)?;
                self.authorize(&db, request, session)?;
                owned(&db, operation_id, session)
                    .map(|value| ResponseBody::PackageImport(value.into()))
            }
            Command::BeginProjectPackageUpload { operation_id } => {
                self.begin_import_upload(request, fingerprint, operation_id)
            }
            Command::ProjectPackageUploadStatus { lease_id } => self
                .import_lease_view(session, lease_id)
                .map(ResponseBody::PackageUpload),
            Command::AbandonProjectPackageUpload { lease_id } => {
                let active = self.import_active_by_lease(session, lease_id)?;
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(internal)?;
                self.authorize(&tx, request, session)?;
                if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
                    return Ok(body);
                }
                let mut status = owned(&tx, &active.operation, session)?;
                if status.state != PackageImportOperationState::Uploading {
                    return Err(ProtocolError::invalid(
                        "only an unsealed upload can be abandoned",
                    ));
                }
                // try_lock avoids letting slow upload I/O hold the authority lock.
                let mut data = active.data.try_lock().map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "upload chunk is in flight; retry abandonment",
                    )
                })?;
                if data.lease.as_ref().map(|lease| &lease.lease_id) != Some(lease_id) {
                    return Err(unavailable("upload lease was replaced"));
                }
                status.state = PackageImportOperationState::AwaitingUpload;
                status.progress.received_bytes = 0;
                save(&tx, &status)?;
                let body = ResponseBody::PackageUploadAbandoned {
                    lease_id: lease_id.clone(),
                };
                remember(&tx, session.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                let discarded = data.upload.take();
                data.lease = None;
                data.connection = None;
                drop(data);
                drop(db);
                drop(discarded);
                Ok(body)
            }
            Command::SealProjectImport {
                operation_id,
                upload_generation,
            } => self.seal_import(request, fingerprint, operation_id, *upload_generation),
            Command::CancelProjectImport { operation_id } => {
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(internal)?;
                self.authorize(&tx, request, session)?;
                if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
                    return Ok(body);
                }
                let mut status = owned(&tx, operation_id, session)?;
                if status.state == PackageImportOperationState::Completed {
                    return Err(ProtocolError::invalid(
                        "completed clone cannot be cancelled or deleted by import",
                    ));
                }
                if unfinished(status.state) {
                    status.state = PackageImportOperationState::Cancelled;
                    status.error = Some(ProtocolError::new(
                        ErrorCode::Forbidden,
                        "clone import cancelled",
                    ));
                    save(&tx, &status)?;
                }
                let body = ResponseBody::PackageImport((status).into());
                remember(&tx, session.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                drop(db);
                self.cancel_import_runtime(operation_id);
                Ok(body)
            }
            _ => Err(ProtocolError::invalid("not an import command")),
        }
    }
    fn start_import(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        package: &PackageUploadDeclaration,
    ) -> PResult<ResponseBody> {
        package.validate()?;
        let session = request.session.as_ref().unwrap();
        // Auth first; expensive filesystem admission is outside all SQL/runtime locks.
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                return Ok(body);
            }
        }
        let reserve = package
            .byte_len
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(16 * 1024 * 1024))
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "import storage reservation overflow",
                )
            })?;
        let reservation = self.pool.reserve_memory(IMPORT_MEMORY)?.with_storage(
            reserve,
            fs2::available_space(&self.config.state_dir).map_err(unavailable)?,
        )?;
        let _gate = self.artifact_gate.lock().map_err(internal)?;
        let reserved = {
            let runtime = self.package_imports.lock().map_err(internal)?;
            if runtime.active.len() >= MAX_ACTIVE_IMPORTS {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "two clone imports already active",
                ));
            }
            runtime
                .active
                .values()
                .try_fold(0u64, |sum, entry| {
                    sum.checked_add(entry.reserved_bytes.load(Ordering::Acquire))
                })
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::ResourceExhausted, "import accounting overflow")
                })?
        };
        let allocated = self.import_store.allocated_bytes()?;
        if allocated
            .checked_add(reserved)
            .and_then(|sum| sum.checked_add(reserve))
            .is_none_or(|sum| sum > self.config.max_snapshot_bytes)
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "shared import/source/evidence storage allowance exhausted",
            ));
        }
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM project_package_imports", [], |row| {
                row.get(0)
            })
            .map_err(internal)?;
        if count >= MAX_IMPORTS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "durable import journal is full; replay tombstones are not discarded",
            ));
        }
        let operation = id!(PackageImportOperationId);
        let status = PackageImportStatus {
            operation_id: operation.clone(),
            request_id: request.request_id.clone(),
            package: package.clone(),
            state: PackageImportOperationState::AwaitingUpload,
            progress: PackageImportProgress {
                received_bytes: 0,
                verified_bytes: 0,
                total_bytes: package.byte_len,
                completed_objects: 0,
                total_objects: None,
            },
            capture: None,
            receipt: None,
            error: None,
        };
        let body = ResponseBody::PackageImport((status.clone()).into());
        tx.execute(
            "INSERT INTO project_package_imports VALUES(?1,?2,?3,?4)",
            params![
                operation.as_str(),
                session.as_str(),
                request.request_id.as_str(),
                encode(&status)?
            ],
        )
        .map_err(internal)?;
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        self.package_imports
            .lock()
            .map_err(internal)?
            .active
            .insert(
                operation.to_string(),
                Arc::new(ActiveImport {
                    operation,
                    session: session.clone(),
                    declaration: package.clone(),
                    cancel: AtomicBool::new(false),
                    worker: AtomicBool::new(false),
                    deadline: Instant::now() + self.config.worker_timeout.min(IMPORT_LIFETIME),
                    reserved_bytes: std::sync::atomic::AtomicU64::new(reserve),
                    _reservation: reservation,
                    data: Mutex::new(UploadData::default()),
                }),
            );
        Ok(body)
    }
    fn import_active(
        &self,
        operation: &PackageImportOperationId,
        session: &SessionId,
    ) -> PResult<Arc<ActiveImport>> {
        let runtime = self.package_imports.lock().map_err(internal)?;
        let active = runtime
            .active
            .get(operation.as_str())
            .cloned()
            .ok_or_else(|| unavailable("import belongs to an old epoch or terminal operation"))?;
        if &active.session != session {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "import belongs to another session",
            ));
        }
        if active.cancel.load(Ordering::Acquire) || Instant::now() >= active.deadline {
            return Err(unavailable("import cancelled or expired"));
        }
        Ok(active)
    }
    fn import_active_by_lease(
        &self,
        session: &SessionId,
        lease: &PackageUploadId,
    ) -> PResult<Arc<ActiveImport>> {
        let entries = self
            .package_imports
            .lock()
            .map_err(internal)?
            .active
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut busy = false;
        for active in entries {
            if &active.session == session {
                let data = match active.data.try_lock() {
                    Ok(data) => data,
                    Err(_) => {
                        busy = true;
                        continue;
                    }
                };
                if data
                    .lease
                    .as_ref()
                    .is_some_and(|entry| &entry.lease_id == lease)
                {
                    drop(data);
                    return Ok(active);
                }
            }
        }
        if busy {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "upload busy; retry",
            ));
        }
        Err(unavailable(
            "upload lease is absent, abandoned, or from a prior epoch",
        ))
    }
    fn begin_import_upload(
        &self,
        request: &Request,
        fingerprint: &str,
        operation: &PackageImportOperationId,
    ) -> PResult<ResponseBody> {
        let session = request.session.as_ref().unwrap();
        let active = self.import_active(operation, session)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            let status = owned(&db, operation, session)?;
            if !matches!(
                status.state,
                PackageImportOperationState::AwaitingUpload
                    | PackageImportOperationState::Uploading
            ) {
                return Err(ProtocolError::invalid(
                    "import no longer accepts upload leases",
                ));
            }
        }
        // Private provisional file creation and cleanup do not hold the DB mutex.
        let mut prepared = Some(self.import_store.begin_upload(
            operation.clone(),
            active.declaration.clone(),
            PackageCodecLimits {
                max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
            },
        )?);
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
            drop(tx);
            drop(db);
            drop(prepared);
            return Ok(body);
        }
        let mut status = owned(&tx, operation, session)?;
        if !matches!(
            status.state,
            PackageImportOperationState::AwaitingUpload | PackageImportOperationState::Uploading
        ) || active.cancel.load(Ordering::Acquire)
            || Instant::now() >= active.deadline
        {
            return Err(unavailable("import stopped during upload admission"));
        }
        let mut data = active.data.try_lock().map_err(|_| {
            ProtocolError::new(ErrorCode::ResourceExhausted, "upload chunk is in flight")
        })?;
        let lease = if let Some(lease) = &data.lease {
            if data
                .lease_deadline
                .is_none_or(|deadline| Instant::now() >= deadline)
            {
                return Err(unavailable(
                    "upload lease expired; abandon it before starting a new generation",
                ));
            }
            lease.clone()
        } else {
            data.generation = data
                .generation
                .checked_add(1)
                .ok_or_else(|| ProtocolError::invalid("upload generation exhausted"))?;
            PackageUploadLease {
                lease_id: id!(PackageUploadId),
                engine_epoch: self.engine_epoch.clone(),
                operation_id: operation.clone(),
                generation: data.generation,
                package: active.declaration.clone(),
                next_offset: 0,
                replay: None,
                expires_after_ms: active
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .min(UPLOAD_LIFETIME)
                    .as_millis() as u64,
                bulk_endpoint: self.config.bulk_endpoint(),
            }
        };
        lease.validate()?;
        status.state = PackageImportOperationState::Uploading;
        save(&tx, &status)?;
        let body = ResponseBody::PackageUpload(lease.clone());
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        if data.lease.is_none() {
            data.lease_deadline = Some(active.deadline.min(Instant::now() + UPLOAD_LIFETIME));
            data.upload = prepared.take();
            data.lease = Some(lease);
        }
        drop(data);
        drop(db);
        drop(prepared);
        Ok(body)
    }
    fn import_lease_view(
        &self,
        session: &SessionId,
        lease_id: &PackageUploadId,
    ) -> PResult<PackageUploadLease> {
        let active = self.import_active_by_lease(session, lease_id)?;
        let db = self.db.lock().map_err(internal)?;
        let status = owned(&db, &active.operation, session)?;
        if status.state != PackageImportOperationState::Uploading {
            return Err(unavailable("import upload was sealed or stopped"));
        }
        let data = active.data.try_lock().map_err(|_| {
            ProtocolError::new(ErrorCode::ResourceExhausted, "upload chunk is in flight")
        })?;
        let mut lease = data
            .lease
            .clone()
            .ok_or_else(|| unavailable("upload abandoned"))?;
        lease.expires_after_ms = data
            .lease_deadline
            .ok_or_else(|| unavailable("upload lease has no deadline"))?
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        lease.validate()?;
        Ok(lease)
    }
    fn seal_import(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        operation: &PackageImportOperationId,
        generation: u64,
    ) -> PResult<ResponseBody> {
        let session = request.session.as_ref().unwrap();
        let active = self.import_active(operation, session)?;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, session)?;
        if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let mut status = owned(&tx, operation, session)?;
        if status.state != PackageImportOperationState::Uploading {
            return Err(ProtocolError::invalid("import is not uploadable"));
        }
        let data = active.data.try_lock().map_err(|_| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "upload chunk is in flight; retry seal",
            )
        })?;
        let lease = data
            .lease
            .as_ref()
            .ok_or_else(|| unavailable("upload has no live lease"))?;
        if data
            .lease_deadline
            .is_none_or(|deadline| Instant::now() >= deadline)
        {
            return Err(unavailable("upload lease expired"));
        }
        if lease.generation != generation || lease.engine_epoch != self.engine_epoch {
            return Err(unavailable("stale upload generation"));
        }
        if lease.next_offset != active.declaration.byte_len {
            return Err(ProtocolError::invalid("upload is incomplete"));
        }
        if active.worker.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "import worker already owns upload",
            ));
        }
        status.state = PackageImportOperationState::Sealing;
        save(&tx, &status)?;
        let body = ResponseBody::PackageImport((status).into());
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        active.worker.store(true, Ordering::Release);
        drop(data);
        drop(db);
        #[cfg(test)]
        let fail_publication = FAIL_PUBLICATION.with(|flag| flag.replace(false));
        let engine = self.clone();
        let request_owned = request.clone();
        let active_owned = active.clone();
        if let Err(error) = spawn(self, move || {
            #[cfg(test)]
            FAIL_PUBLICATION.with(|flag| flag.set(fail_publication));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.run_import(&request_owned, &active_owned)
            }))
            .unwrap_or_else(|_| Err(internal("clone import worker panicked")));
            if let Err(error) = result {
                let _ = engine.fail_import(&active_owned.operation, &active_owned.session, error);
            }
            let removed = engine
                .package_imports
                .lock()
                .ok()
                .and_then(|mut runtime| runtime.active.remove(active_owned.operation.as_str()));
            drop(removed);
        }) {
            self.fail_import(operation, session, unavailable(error))?;
            let removed = self
                .package_imports
                .lock()
                .map_err(internal)?
                .active
                .remove(operation.as_str());
            drop(removed);
        }
        Ok(body)
    }
    fn check_import(&self, request: &Request, active: &ActiveImport) -> PResult<()> {
        if active.cancel.load(Ordering::Acquire) || Instant::now() >= active.deadline {
            return Err(unavailable("clone import cancelled or deadline expired"));
        }
        let db = self.db.lock().map_err(internal)?;
        self.authenticate(&db, &active.session, request.auth_token.as_deref())?;
        let status = owned(&db, &active.operation, &active.session)?;
        if !unfinished(status.state) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "clone import is no longer active",
            ));
        }
        Ok(())
    }
    fn import_progress(
        &self,
        active: &ActiveImport,
        state: PackageImportOperationState,
        bytes: u64,
        objects: u64,
        total: Option<u64>,
    ) -> PResult<()> {
        let db = self.db.lock().map_err(internal)?;
        let mut status = owned(&db, &active.operation, &active.session)?;
        if !unfinished(status.state) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "clone import cancelled",
            ));
        }
        status.state = state;
        status.progress.verified_bytes = bytes.min(status.package.byte_len);
        status.progress.completed_objects = objects;
        if total.is_some() {
            status.progress.total_objects = total;
        }
        save(&db, &status)
    }
    fn fail_import(
        &self,
        operation: &PackageImportOperationId,
        session: &SessionId,
        error: ProtocolError,
    ) -> PResult<()> {
        let db = self.db.lock().map_err(internal)?;
        let mut status = owned(&db, operation, session)?;
        if unfinished(status.state) {
            status.state = PackageImportOperationState::Failed;
            status.error = Some(error);
            save(&db, &status)?;
        }
        Ok(())
    }
    fn cancel_import_runtime(&self, operation: &PackageImportOperationId) {
        let removed = if let Ok(mut runtime) = self.package_imports.lock() {
            if let Some(active) = runtime.active.get(operation.as_str()) {
                active.cancel.store(true, Ordering::Release);
                // Cancellation acknowledges a fence, not completed cleanup. A
                // connection or locked append still owns the entire reservation.
                let drained = active
                    .data
                    .try_lock()
                    .map(|data| data.connection.is_none())
                    .unwrap_or(false);
                if !active.worker.load(Ordering::Acquire) && drained {
                    runtime.active.remove(operation.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        drop(removed);
    }
    fn run_import(&self, request: &Request, active: &ActiveImport) -> PResult<()> {
        self.check_import(request, active)?;
        let upload = active
            .data
            .lock()
            .map_err(internal)?
            .upload
            .take()
            .ok_or_else(|| internal("sealed upload is absent"))?;
        let sealed = self.import_store.seal_upload(
            upload,
            || self.check_import(request, active),
            |_| Ok(()),
        )?;
        self.import_progress(active, PackageImportOperationState::Sealed, 0, 0, None)?;
        self.import_progress(active, PackageImportOperationState::Validating, 0, 0, None)?;
        let mut last = Instant::now();
        let verified = self.import_store.verify_and_stage(
            sealed,
            || self.check_import(request, active),
            |progress| {
                let state = match progress.phase {
                    ImportStoragePhase::Extracting | ImportStoragePhase::Validating => {
                        PackageImportOperationState::Validating
                    }
                    ImportStoragePhase::Publishing => {
                        return Err(internal(
                            "unexpected publication during import verification",
                        ))
                    }
                };
                if last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    self.import_progress(
                        active,
                        state,
                        progress.completed_bytes,
                        progress.completed_objects,
                        None,
                    )?;
                }
                Ok(())
            },
        )?;
        self.check_import(request, active)?;
        let native_intervals =
            validate_all_evidence(&verified, &self.import_store, &self.pool, || {
                self.check_import(request, active)
            })?;
        let manifest = verified.manifest();
        let container = verified.container().clone();
        let identities = CloneIdentityAssignment {
            project_id: id!(ProjectId),
            placements: manifest
                .placements
                .iter()
                .flat_map(|p| p.definitions.iter())
                .map(|d| (d.placement_id.clone(), id!(SourcePlacementId)))
                .collect(),
            candidates: manifest
                .candidates
                .iter()
                .map(|candidate| (candidate.candidate_id.clone(), id!(CandidateId)))
                .collect(),
        };
        let intervals = crate::project_package_import_plan::resolve_candidate_intervals(
            manifest,
            verified.origins(),
            &native_intervals,
        )?;
        let mut plan = plan_clone_import(manifest, &container, verified.origins(), &identities)?;
        for candidate in &mut plan.manifest.candidates {
            let original = identities
                .candidates
                .iter()
                .find(|(_, local)| **local == candidate.candidate_id)
                .ok_or_else(|| ProtocolError::invalid("clone interval identity missing"))?
                .0;
            candidate.project_interval = *intervals
                .get(original)
                .ok_or_else(|| ProtocolError::invalid("clone interval policy missing"))?;
        }
        plan.manifest.format_version = plan.manifest.required_format_version();
        plan.manifest.validate()?;
        let origin_map_bytes=serde_json::to_vec(&serde_json::json!({"project_id":plan.identities.project_id,"candidates":plan.identities.candidates,"placements":plan.identities.placements})).map_err(internal)?;
        let additional = verified
            .publication_bytes()?
            .checked_add(origin_map_bytes.len() as u64)
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::ResourceExhausted, "origin map storage overflow")
            })?;
        let additional_reservation = self.pool.reserve_memory(128 * 1024)?.with_storage(
            additional,
            fs2::available_space(&self.config.state_dir).map_err(unavailable)?,
        )?;
        let held = {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            let reserved = self
                .package_imports
                .lock()
                .map_err(internal)?
                .active
                .values()
                .try_fold(0u64, |sum, entry| {
                    sum.checked_add(entry.reserved_bytes.load(Ordering::Acquire))
                })
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "import publication accounting overflow",
                    )
                })?;
            let allocated = self.import_store.allocated_bytes()?;
            if allocated
                .checked_add(reserved)
                .and_then(|sum| sum.checked_add(additional))
                .is_none_or(|sum| sum > self.config.max_snapshot_bytes)
            {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "import publication exceeds shared storage allowance",
                ));
            }
            self.check_import(request, active)?;
            active
                .reserved_bytes
                .fetch_add(additional, Ordering::AcqRel);
            self.import_store.prepare_publication(verified)?
        };
        {
            let db = self.db.lock().map_err(internal)?;
            let mut status = owned(&db, &active.operation, &active.session)?;
            if !unfinished(status.state) {
                return Err(unavailable("clone import stopped"));
            }
            status.capture = Some(PackageCaptureBinding {
                project_id: container.project_id.clone(),
                captured_revision: container.captured_revision,
                captured_event_cursor: container.captured_event_cursor,
                manifest_sha256: container.manifest_sha256.clone(),
            });
            save(&db, &status)?;
        }
        self.import_progress(
            active,
            PackageImportOperationState::Publishing,
            active.declaration.byte_len,
            0,
            Some(plan.manifest.objects.len() as u64),
        )?;
        let published = self.import_store.publish_objects(
            held,
            || self.check_import(request, active),
            |_| Ok(()),
        )?;
        self.check_import(request, active)?;
        let map_identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!(
                "import-map.{:x}",
                Sha256::digest(&origin_map_bytes)
            ))
            .map_err(internal)?,
            sha256: format!("{:x}", Sha256::digest(&origin_map_bytes)),
            byte_len: origin_map_bytes.len() as u64,
        };
        let _map_hold = {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            self.package_holds.acquire(&[map_identity])?
        };
        let origin_map = self.import_store.publish_evidence(&origin_map_bytes)?;
        let encoded_manifest = encode(&plan.manifest)?;
        let receipt = PackageImportReceipt {
            project_id: plan.manifest.origin_project_id.clone(),
            revision: plan.manifest.captured_revision,
            capture: PackageCaptureBinding {
                project_id: manifest_origin_id(&published)?,
                captured_revision: container_revision(&published)?,
                captured_event_cursor: container_cursor(&published)?,
                manifest_sha256: published.container.manifest_sha256.clone(),
            },
            package: active.declaration.clone(),
            origin_map: origin_map.clone(),
        };
        self.publish_clone(
            request,
            active,
            &plan,
            &published,
            &receipt,
            &encoded_manifest,
        )?;
        drop(published);
        drop(additional_reservation);
        Ok(())
    }
    fn publish_clone(
        &self,
        request: &Request,
        active: &ActiveImport,
        plan: &crate::project_package_import_plan::CloneImportPlan,
        published: &PublishedImportObjects,
        receipt: &PackageImportReceipt,
        encoded_manifest: &str,
    ) -> PResult<()> {
        published.validate_clone_binding(&active.operation, &active.declaration, plan)?;
        let manifest = &plan.manifest;
        let project = &manifest.origin_project_id;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authenticate(&tx, &active.session, request.auth_token.as_deref())?;
        let mut status = owned(&tx, &active.operation, &active.session)?;
        if !unfinished(status.state)
            || active.cancel.load(Ordering::Acquire)
            || Instant::now() >= active.deadline
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "clone publication was cancelled or expired",
            ));
        }
        let mut registered = BTreeSet::new();
        for descriptor in std::iter::once(&manifest.head.motion)
            .chain(manifest.revisions.iter().map(|row| &row.motion))
            .chain(manifest.edit_states.iter().map(|row| &row.motion))
            .chain(manifest.candidates.iter().map(|row| &row.motion))
        {
            if registered.insert(descriptor.sha256.clone()) {
                motion_state::register(&tx, descriptor)?;
            }
        }
        let _published_guard = published;
        let reference = |motion: &ProgramDescriptor| format!("motion:{}", motion.sha256);
        tx.execute("INSERT INTO projects(id,name,revision,program,history_cursor,protected) VALUES(?1,?2,?3,?4,?5,?6)",
            params![project.as_str(),manifest.head.name,rev_sql(manifest.head.revision)?,reference(&manifest.head.motion),
                i64::try_from(manifest.head.history_cursor).map_err(internal)?,encode(&manifest.head.protected)?]).map_err(internal)?;
        for revision in &manifest.revisions {
            tx.execute(
                "INSERT INTO revisions VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    project.as_str(),
                    rev_sql(revision.revision)?,
                    revision.actor,
                    revision.kind,
                    revision.label,
                    reference(&revision.motion),
                    encode(&revision.protected)?
                ],
            )
            .map_err(internal)?;
        }
        if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_REBASE_LINKS
            && crate::inference_equivalence_reader::version(&tx).map_err(internal)? < 11
        {
            crate::inference_equivalence_reader::publish(&tx, 11).map_err(internal)?;
        }
        if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CUDA_EVIDENCE {
            crate::portable_cuda_db::arm(&tx).map_err(internal)?;
        }
        if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_ANNOTATED_MODELS {
            crate::portable_annotated_db::arm(&tx).map_err(internal)?;
        }
        if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_WASM_INVOCATIONS {
            crate::portable_wasm_db::arm(&tx).map_err(internal)?;
        }
        if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CPU_HISTORY {
            crate::portable_cpu_archives::arm(&tx).map_err(internal)?;
        }
        for candidate in &manifest.candidates {
            tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,committed_revision,review) VALUES(?1,?2,?3,?4,NULL,?5,?6,?7)",
                params![candidate.candidate_id.as_str(),project.as_str(),rev_sql(candidate.base_revision)?,reference(&candidate.motion),
                    encode(&candidate.lineage)?,candidate.committed_revision.map(rev_sql).transpose()?,encode(&candidate.review)?]).map_err(internal)?;
            imported_candidate_intervals::insert(
                &tx,
                &candidate.candidate_id,
                project,
                candidate.project_interval,
            )?;
            let review = plan
                .candidate_reviews
                .get(&candidate.candidate_id)
                .cloned()
                .unwrap_or(PackageReviewState {
                    decisions: vec![],
                    flags: candidate.review.clone(),
                    unknown: false,
                    conservative: false,
                });
            tx.execute(
                "INSERT INTO project_import_candidate_aliases VALUES(?1,?2,?1,?3)",
                params![
                    candidate.candidate_id.as_str(),
                    project.as_str(),
                    encode(&review)?
                ],
            )
            .map_err(internal)?;
        }
        for state in &manifest.edit_states {
            tx.execute(
                "INSERT INTO edit_states VALUES(?1,?2,?3,?4)",
                params![
                    project.as_str(),
                    i64::try_from(state.position).map_err(internal)?,
                    reference(&state.motion),
                    encode(&state.protected)?
                ],
            )
            .map_err(internal)?;
            if let Some(revision) = state.source_revision {
                tx.execute(
                    "INSERT INTO history_lineage VALUES(?1,?2,?3)",
                    params![
                        project.as_str(),
                        i64::try_from(state.position).map_err(internal)?,
                        rev_sql(revision)?
                    ],
                )
                .map_err(internal)?;
            }
        }
        for lineage in &manifest.revision_lineage {
            tx.execute(
                "INSERT INTO revision_lineage VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    project.as_str(),
                    rev_sql(lineage.revision)?,
                    lineage.parent_revision.map(rev_sql).transpose()?,
                    lineage.restored_from_revision.map(rev_sql).transpose()?,
                    lineage
                        .restored_history_position
                        .map(i64::try_from)
                        .transpose()
                        .map_err(internal)?,
                    lineage.candidate.as_ref().map(|id| id.as_str()),
                    lineage.operation,
                    encode(&lineage.contributions)?,
                    encode(&lineage.review)?
                ],
            )
            .map_err(internal)?;
        }
        for source in &manifest.sources {
            if source.source_version.as_str() != source.identity.sha256 {
                return Err(ProtocolError::invalid(
                    "import source version is not its immutable content identity",
                ));
            }
            let path = self
                .config
                .state_dir
                .join("snapshots")
                .join(&source.identity.sha256);
            tx.execute("INSERT INTO sources(project,version,path,original,label,sha256,bytes,pinned,evicted,kind) VALUES(?1,?2,?3,'',?4,?5,?6,?7,0,?8)",
                params![project.as_str(),source.source_version.as_str(),path.to_string_lossy(),source.label,source.identity.sha256,
                    i64::try_from(source.identity.byte_len).map_err(internal)?,source.pinned,source.kind.as_str()]).map_err(internal)?;
        }
        if let Some(closure) = &manifest.placements {
            for definition in &closure.definitions {
                tx.execute(
                    "INSERT INTO source_placement_definitions VALUES(?1,?2,?3,?4)",
                    params![
                        definition.placement_id.as_str(),
                        project.as_str(),
                        definition.spec.source_version.as_str(),
                        encode(definition)?
                    ],
                )
                .map_err(internal)?;
            }
            for id in &closure.active {
                tx.execute(
                    "INSERT INTO source_placement_active VALUES(?1,?2)",
                    params![project.as_str(), id.as_str()],
                )
                .map_err(internal)?;
            }
            for row in &closure.revisions {
                tx.execute(
                    "INSERT INTO source_placement_revisions VALUES(?1,?2,?3)",
                    params![
                        project.as_str(),
                        rev_sql(row.revision)?,
                        encode(&row.placements)?
                    ],
                )
                .map_err(internal)?;
            }
            for row in &closure.history {
                tx.execute(
                    "INSERT INTO source_placement_history VALUES(?1,?2,?3)",
                    params![
                        project.as_str(),
                        i64::try_from(row.position).map_err(internal)?,
                        encode(&row.placements)?
                    ],
                )
                .map_err(internal)?;
            }
        }
        tx.execute(
            "INSERT INTO project_import_archives VALUES(?1,?2,?3,?4,?5)",
            params![
                project.as_str(),
                rev_sql(manifest.captured_revision)?,
                encoded_manifest,
                encode(&receipt.origin_map)?,
                encode(&receipt.package)?
            ],
        )
        .map_err(internal)?;
        for scope in [
            Scope::Read,
            Scope::Edit,
            Scope::Generate,
            Scope::Export,
            Scope::ManageGrants,
            Scope::ManageProtection,
            Scope::ImportSource,
            Scope::PackageProject,
        ] {
            tx.execute(
                "INSERT INTO grants VALUES(?1,?2,?3)",
                params![active.session.as_str(), project.as_str(), scope_name(scope)],
            )
            .map_err(internal)?;
        }
        event(
            &tx,
            project,
            manifest.captured_revision,
            EventBody::ProjectChanged,
        )?;
        status.state = PackageImportOperationState::Completed;
        status.progress.received_bytes = status.package.byte_len;
        status.progress.verified_bytes = status.package.byte_len;
        status.progress.completed_objects = manifest.objects.len() as u64;
        status.progress.total_objects = Some(manifest.objects.len() as u64);
        status.capture = Some(receipt.capture.clone());
        status.receipt = Some(receipt.clone());
        status.error = None;
        save(&tx, &status)?;
        #[cfg(test)]
        if FAIL_PUBLICATION.with(|flag| flag.replace(false)) {
            return Err(internal("injected pre-commit clone publication failure"));
        }
        tx.commit().map_err(internal)?;
        Ok(())
    }
    pub(crate) fn import_bulk_open(
        &self,
        handshake: &PackageUploadHandshake,
    ) -> PResult<(ImportConnection, PackageUploadLease)> {
        handshake.validate()?;
        let active = self.import_active(&handshake.operation_id, &handshake.session)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authenticate(&db, &handshake.session, Some(&handshake.auth_token))?;
            let status = owned(&db, &active.operation, &handshake.session)?;
            if status.state != PackageImportOperationState::Uploading {
                return Err(unavailable("import no longer accepts chunks"));
            }
        }
        let mut data = active.data.lock().map_err(internal)?;
        let mut lease = data
            .lease
            .clone()
            .ok_or_else(|| unavailable("upload lease missing"))?;
        lease.expires_after_ms = data
            .lease_deadline
            .ok_or_else(|| unavailable("upload lease deadline missing"))?
            .saturating_duration_since(Instant::now())
            .as_millis() as u64;
        lease.validate()?;
        if handshake.lease_id != lease.lease_id
            || handshake.engine_epoch != self.engine_epoch
            || handshake.generation != lease.generation
            || handshake.package_sha256 != lease.package.sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "upload handshake binding mismatch",
            ));
        }
        if data.connection.is_some() {
            let mut error = ProtocolError::new(
                ErrorCode::PackageUploadConnectionBusy,
                "upload lease already has a connection",
            );
            error.retryable = true;
            return Err(error);
        }
        let nonce = Uuid::new_v4().to_string();
        data.connection = Some(nonce.clone());
        drop(data);
        Ok((
            ImportConnection {
                active,
                generation: lease.generation,
                nonce,
            },
            lease,
        ))
    }
    pub(crate) fn import_bulk_append(
        &self,
        connection: &ImportConnection,
        handshake: &PackageUploadHandshake,
        offset: u64,
        bytes: &[u8],
    ) -> PResult<u64> {
        if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_CHUNK_BYTES {
            return Err(ProtocolError::invalid("upload chunk exceeds bound"));
        }
        self.check_import_connection(connection, handshake)?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        let next = {
            let mut data = connection.active.data.lock().map_err(internal)?;
            let lease = data
                .lease
                .as_ref()
                .ok_or_else(|| unavailable("upload abandoned"))?;
            if data
                .lease_deadline
                .is_none_or(|deadline| Instant::now() >= deadline)
            {
                return Err(unavailable("upload lease expired"));
            }
            if lease.generation != connection.generation
                || data.connection.as_deref() != Some(&connection.nonce)
            {
                return Err(unavailable("upload connection was replaced"));
            }
            if offset != lease.next_offset
                && !lease.replay.as_ref().is_some_and(|last| {
                    last.offset == offset
                        && last.byte_len == bytes.len() as u32
                        && last.sha256 == digest
                })
            {
                return Err(ProtocolError::invalid(
                    "upload must be next prefix or exact last payload replay",
                ));
            }
            #[cfg(test)]
            BEFORE_APPEND_IO.with(|hook| {
                let callback = hook.borrow_mut().take();
                if let Some(callback) = callback {
                    callback();
                }
            });
            let next = data
                .upload
                .as_mut()
                .ok_or_else(|| unavailable("upload sealed"))?
                .append(offset, bytes)?;
            let lease = data.lease.as_mut().unwrap();
            lease.next_offset = next;
            lease.replay = Some(PackageUploadChunkReceipt {
                offset,
                byte_len: bytes.len() as u32,
                sha256: digest,
            });
            next
        };
        #[cfg(test)]
        BEFORE_ACK.with(|hook| {
            let callback = hook.borrow_mut().take();
            if let Some(callback) = callback {
                callback();
            }
        });
        // Bytes already written into private staging cannot become a project until
        // a separately authorized seal and atomic publication.
        self.check_import_connection(connection, handshake)?;
        let db = self.db.lock().map_err(internal)?;
        let mut status = owned(&db, &connection.active.operation, &handshake.session)?;
        if status.state != PackageImportOperationState::Uploading {
            return Err(unavailable("upload stopped before acknowledgment"));
        }
        // Hold the same SQL admission lock through binding validation and
        // progress publication; replacement cannot interleave between them.
        let data = connection.active.data.try_lock().map_err(|_| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "upload is busy before acknowledgment",
            )
        })?;
        check_upload_binding(&data, connection, handshake)?;
        status.progress.received_bytes = next;
        save(&db, &status)?;
        Ok(next)
    }
    fn check_import_connection(
        &self,
        connection: &ImportConnection,
        handshake: &PackageUploadHandshake,
    ) -> PResult<()> {
        let active = &connection.active;
        if active.session != handshake.session
            || active.operation != handshake.operation_id
            || handshake.engine_epoch != self.engine_epoch
            || active.cancel.load(Ordering::Acquire)
            || Instant::now() >= active.deadline
        {
            return Err(unavailable("upload authority expired or mismatched"));
        }
        let db = self.db.lock().map_err(internal)?;
        self.authenticate(&db, &handshake.session, Some(&handshake.auth_token))?;
        let status = owned(&db, &active.operation, &handshake.session)?;
        if status.state != PackageImportOperationState::Uploading {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "upload operation no longer admits chunks",
            ));
        }
        let data = active.data.try_lock().map_err(|_| {
            ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "upload is busy during admission",
            )
        })?;
        check_upload_binding(&data, connection, handshake)
    }
}
#[cfg(test)]
thread_local! {
    static FAIL_PUBLICATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static BEFORE_ACK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static BEFORE_APPEND_IO: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

fn check_upload_binding(
    data: &UploadData,
    connection: &ImportConnection,
    handshake: &PackageUploadHandshake,
) -> PResult<()> {
    let lease = data
        .lease
        .as_ref()
        .ok_or_else(|| unavailable("upload was abandoned"))?;
    if lease.generation != connection.generation
        || lease.generation != handshake.generation
        || lease.lease_id != handshake.lease_id
        || lease.engine_epoch != handshake.engine_epoch
        || lease.operation_id != handshake.operation_id
        || lease.package.sha256 != handshake.package_sha256
        || data.connection.as_deref() != Some(&connection.nonce)
        || data
            .lease_deadline
            .is_none_or(|deadline| Instant::now() >= deadline)
        || Instant::now() >= connection.active.deadline
        || connection.active.cancel.load(Ordering::Acquire)
    {
        return Err(unavailable(
            "upload generation, connection, or lifetime was replaced",
        ));
    }
    Ok(())
}

fn manifest_origin_id(value: &PublishedImportObjects) -> PResult<ProjectId> {
    Ok(value.container.project_id.clone())
}
fn container_revision(value: &PublishedImportObjects) -> PResult<RevisionId> {
    Ok(value.container.captured_revision)
}
fn container_cursor(value: &PublishedImportObjects) -> PResult<u64> {
    Ok(value.container.captured_event_cursor)
}

#[cfg(test)]
thread_local! {
    static FAIL_EXPIRY_SPAWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn spawn_expiry_sweeper(
    owner: &Engine,
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_EXPIRY_SPAWN.with(|flag| flag.replace(false)) {
        return Err(std::io::Error::other(
            "injected clone import expiry thread failure",
        ));
    }
    owner.spawn_task("pulsar-import-expiry", None, task)
}

pub(super) fn start_sweeper(engine: &Arc<Engine>) -> PResult<()> {
    let weak = Arc::downgrade(engine);
    spawn_expiry_sweeper(engine, move || loop {
        std::thread::sleep(Duration::from_millis(250));
        let Some(engine) = weak.upgrade() else { break };
        if engine.is_shutting_down() {
            break;
        }
        let entries = match engine.package_imports.lock() {
            Ok(runtime) => runtime.active.values().cloned().collect::<Vec<_>>(),
            Err(_) => continue,
        };
        for active in entries {
            if active.cancel.load(Ordering::Acquire) || Instant::now() >= active.deadline {
                if Instant::now() >= active.deadline {
                    active.cancel.store(true, Ordering::Release);
                    let _ = engine.fail_import(
                        &active.operation,
                        &active.session,
                        unavailable("clone import absolute deadline expired"),
                    );
                }
                if !active.worker.load(Ordering::Acquire) {
                    engine.cancel_import_runtime(&active.operation);
                }
            }
        }
    })
    .map_err(|error| unavailable(format!("clone import expiry worker unavailable: {error}")))?;
    Ok(())
}

/// Imported evidence is archival, never local authored-edit protection authority.
pub(super) fn candidate_review(
    db: &Connection,
    candidate: &CandidateId,
) -> PResult<Option<lineage::ReviewState>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT review_state FROM project_import_candidate_aliases WHERE candidate=?1",
            [candidate.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(internal)?;
    raw.map(|raw| {
        let mut state: lineage::ReviewState = decode(&raw)?;
        validate_reviews(&state.flags)?;
        state.unknown = true;
        state.conservative = true;
        state.decisions.clear();
        Ok(state)
    })
    .transpose()
}
pub(super) fn imported_revision(
    db: &Connection,
    project: &ProjectId,
    revision: RevisionId,
) -> PResult<bool> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM project_import_archives WHERE project=?1 AND captured_revision>=?2)",
        params![project.as_str(),rev_sql(revision)?],|row|row.get(0)).map_err(internal)
}
pub(super) fn rebase_alias(db: &Connection, old: &CandidateId, new: &CandidateId) -> PResult<()> {
    db.execute("INSERT INTO project_import_candidate_aliases(candidate,project,imported_candidate,review_state)
        SELECT ?2,project,imported_candidate,review_state FROM project_import_candidate_aliases WHERE candidate=?1",
        params![old.as_str(),new.as_str()]).map_err(internal)?;
    imported_candidate_intervals::rebase(db, old, new)?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedEditReceipt {
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
struct ArchivedWorkerReceipt {
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
fn validate_all_evidence(
    verified: &VerifiedImportBytes,
    store: &ImportStore,
    pool: &ResourcePool,
    mut check: impl FnMut() -> PResult<()>,
) -> PResult<crate::project_package_import_plan::NativeCandidateIntervals> {
    let catalog: BTreeMap<_, _> = verified
        .manifest()
        .objects
        .iter()
        .map(|entry| (entry.sha256.as_str(), entry))
        .collect();
    let mut scopes = BTreeMap::new();
    for manifest in std::iter::once(verified.manifest()).chain(verified.origins().values()) {
        check()?;
        let native = validate_evidence(
            manifest,
            pool,
            |reference| {
                let descriptor = catalog
                    .get(reference.sha256.as_str())
                    .copied()
                    .filter(|entry| entry.byte_len == reference.byte_len)
                    .ok_or_else(|| {
                        ProtocolError::invalid(
                            "archival evidence not present in uploaded object catalog",
                        )
                    })?;
                if descriptor.byte_len
                    > if descriptor
                        .roles
                        .contains(&PackageObjectRole::AnnotatedModel)
                    {
                        256 * 1024 * 1024
                    } else {
                        128 * 1024 * 1024
                    }
                {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "compact imported evidence exceeds128MiB",
                    ));
                }
                let file = store.open_staged_object(verified, descriptor)?;
                let mut bytes = Vec::new();
                file.take(descriptor.byte_len + 1)
                    .read_to_end(&mut bytes)
                    .map_err(unavailable)?;
                if bytes.len() as u64 != descriptor.byte_len
                    || format!("{:x}", Sha256::digest(&bytes)) != descriptor.sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "import evidence changed after verification",
                    ));
                }
                Ok(bytes)
            },
            &mut check,
        )?;
        let namespace = format!("{:x}", Sha256::digest(manifest.canonical_bytes()?));
        scopes.extend(
            native
                .into_iter()
                .map(|(id, scope)| ((namespace.clone(), id), scope)),
        );
    }
    Ok(scopes)
}
pub(crate) fn checked_worker_scope(
    manifest: &PortableProjectManifest,
    origin: &PackageGeneratedOrigin,
    request: &WorkerRequest,
) -> PResult<Option<TimeRange>> {
    request.validate()?;
    crate::portable_cpu_archives::check_request(origin, request)?;
    let source = manifest
        .sources
        .iter()
        .find(|s| s.source_version == request.source.source_version)
        .ok_or_else(|| ProtocolError::invalid("archived worker primary source absent"))?;
    if request.project_id != manifest.origin_project_id
        || request.job_id != origin.job_id
        || request.attempt_id != origin.attempt_id
        || request.base_revision != origin.base_revision
        || request.source.source_version != origin.source_version
        || request.source.identity != source.identity
    {
        return Err(ProtocolError::invalid(
            "archived worker identity differs from logical graph",
        ));
    }
    Ok(match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.project_interval,
        _ => None,
    })
}

pub(crate) fn validate_evidence(
    manifest: &PortableProjectManifest,
    pool: &ResourcePool,
    mut bytes: impl FnMut(&PackageObjectRef) -> PResult<Vec<u8>>,
    check: &mut impl FnMut() -> PResult<()>,
) -> PResult<BTreeMap<CandidateId, Option<TimeRange>>> {
    crate::project_package_import_plan::require_portable_cpu_manifest(manifest)?;
    let mut scopes = BTreeMap::new();
    let candidates: BTreeMap<_, _> = manifest
        .candidates
        .iter()
        .map(|value| (&value.candidate_id, value))
        .collect();
    let revisions: BTreeMap<_, _> = manifest
        .revisions
        .iter()
        .map(|value| (value.revision, value))
        .collect();
    let sources: BTreeMap<_, _> = manifest
        .sources
        .iter()
        .map(|value| (&value.source_version, value))
        .collect();
    for source in &manifest.sources {
        for association in &source.stash_associations {
            check()?;
            let capture: StashSourceCapture =
                serde_json::from_slice(&bytes(&association.capture_receipt)?)
                    .map_err(|_| ProtocolError::invalid("invalid inert Stash capture receipt"))?;
            association.check_capture(source, &capture)?;
        }
    }
    let identity_ref = |identity: &ArtifactIdentity| PackageObjectRef {
        sha256: identity.sha256.clone(),
        byte_len: identity.byte_len,
    };
    for receipt in &manifest.retiming_receipts {
        check()?;
        let needed = receipt
            .before
            .byte_len
            .checked_add(receipt.after.byte_len)
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(16 * 1024 * 1024))
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "archived retiming memory overflow",
                )
            })?;
        let _memory = pool.reserve_memory(needed)?;
        let before: MotionProgram = serde_json::from_slice(&bytes(&identity_ref(&receipt.before))?)
            .map_err(|_| ProtocolError::invalid("invalid retiming before motion"))?;
        let after: MotionProgram =
            serde_json::from_slice(&bytes(&identity_ref(&receipt.after))?)
                .map_err(|_| ProtocolError::invalid("invalid retiming after motion"))?;
        let recomputed = retime_motion_between_placements(
            &before,
            &receipt.from.spec,
            &receipt.to.spec,
            &receipt.axes,
        )
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "archived retiming cannot be recomputed",
            )
        })?;
        if recomputed != after {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "archived retiming result differs from its facts",
            ));
        }
    }
    for authored in &manifest.authored_lineage {
        check()?;
        let receipt: ArchivedEditReceipt =
            serde_json::from_slice(&bytes(&identity_ref(&authored.receipt))?)
                .map_err(|_| ProtocolError::invalid("invalid archived authored receipt"))?;
        let candidate = candidates
            .get(&authored.candidate_id)
            .ok_or_else(|| ProtocolError::invalid("missing archived candidate"))?;
        let base = revisions
            .get(&receipt.base_revision)
            .ok_or_else(|| ProtocolError::invalid("missing original authored base revision"))?;
        if receipt.format != "pulsar-edit-receipt-v1"
            || receipt.kernel != pulsar_core::EDIT_VALUES_KERNEL_VERSION
            || receipt.project_id != manifest.origin_project_id
            || receipt.base_program != base.motion
            || receipt.result_program != candidate.motion
            || receipt.upload_sha256 != authored.input.sha256
            || receipt.upload_byte_len != authored.input.byte_len
            || receipt.authored_ranges.len() as u64 != authored.authored_count
            || receipt.inherited_ranges.len() as u64 != authored.inherited_count
        {
            return Err(ProtocolError::invalid(
                "archived authored receipt disagrees with logical graph",
            ));
        }
        for (_, range) in receipt
            .authored_ranges
            .iter()
            .chain(&receipt.inherited_ranges)
        {
            if range.start() < ProjectTime::ZERO || range.start() >= range.end() {
                return Err(ProtocolError::invalid(
                    "invalid archived authored influence interval",
                ));
            }
        }
        if let Some(invocation) = &authored.wasm_invocation {
            invocation.validate()?;
            if invocation.attempt_id.as_str() != receipt.lease_id.as_str()
                || invocation.base_revision != receipt.base_revision
            {
                return Err(ProtocolError::invalid(
                    "WASM invocation differs from original core receipt",
                ));
            }
            let needed = receipt
                .base_program
                .byte_len
                .checked_add(receipt.result_program.byte_len)
                .and_then(|n| n.checked_mul(8))
                .and_then(|n| n.checked_add(16 * 1024 * 1024))
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "WASM archive validation memory overflow",
                    )
                })?;
            let _memory = pool.reserve_memory(needed)?;
            let base_program: MotionProgram = serde_json::from_slice(&bytes(&PackageObjectRef {
                sha256: receipt.base_program.sha256.clone(),
                byte_len: receipt.base_program.byte_len,
            })?)
            .map_err(|_| ProtocolError::invalid("invalid WASM archived base program"))?;
            let result_program: MotionProgram =
                serde_json::from_slice(&bytes(&PackageObjectRef {
                    sha256: receipt.result_program.sha256.clone(),
                    byte_len: receipt.result_program.byte_len,
                })?)
                .map_err(|_| ProtocolError::invalid("invalid WASM archived result program"))?;
            let checked = crate::package_wasm_values::validate(
                invocation,
                &bytes(&identity_ref(&invocation.manifest.artifact))?,
                &bytes(&invocation.input)?,
                &bytes(&authored.input)?,
                &base_program,
                &result_program,
            )?;
            if checked.authored_ranges != receipt.authored_ranges
                || checked.inherited_ranges != receipt.inherited_ranges
                || checked.authored_provenance != receipt.authored_provenance
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "WASM authored range/provenance facts differ from kernel",
                ));
            }
        }
        let _inert = (
            &receipt.lease_id,
            &receipt.authored_provenance,
            &receipt.actor_id,
            &receipt.label,
        );
        let _: pulsar_core::EditValuesProgram = serde_json::from_slice(&bytes(&authored.input)?)
            .map_err(|_| {
                ProtocolError::invalid("archived edit input is not checked values-only data")
            })?;
    }
    let mut candidates_by_job: BTreeMap<&JobId, Vec<&PackageCandidate>> = BTreeMap::new();
    for candidate in &manifest.candidates {
        if let Some(job) = &candidate.job_origin {
            candidates_by_job.entry(job).or_default().push(candidate);
        }
    }
    for origin in &manifest.generated_origins {
        check()?;
        let request: WorkerRequest = serde_json::from_slice(&bytes(&origin.manifest)?)
            .map_err(|_| ProtocolError::invalid("invalid inert worker manifest"))?;
        let scope = checked_worker_scope(manifest, origin, &request)?;
        if let Some(image) = assistant_image_origin::archived(&request, |identity| {
            if !manifest.objects.iter().any(|o| {
                o.sha256 == identity.sha256
                    && o.byte_len == identity.byte_len
                    && o.roles.contains(&PackageObjectRole::DependencyEvidence)
            }) {
                return Err(ProtocolError::invalid(
                    "required image provenance payload is absent",
                ));
            }
            bytes(&identity_ref(identity))
        })? {
            let declared = origin
                .image_origin
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("image origin membership is absent"))?;
            if !request.dependencies.contains(&declared.record) || declared.rgb != image.rgb {
                return Err(ProtocolError::invalid(
                    "image origin membership differs from worker declaration",
                ));
            }
            let source = sources
                .get(&image.source_version)
                .ok_or_else(|| ProtocolError::invalid("archived image source absent"))?;
            if source.kind != SourceKind::Media || source.identity != image.source {
                return Err(ProtocolError::invalid(
                    "archived image source identity differs",
                ));
            }
        }
        if origin.image_origin.is_some()
            && !request.dependencies.iter().any(|d| {
                d.artifact_id
                    .as_str()
                    .starts_with("assistant-image-origin-v1:")
            })
        {
            return Err(ProtocolError::invalid("unexpected image origin membership"));
        }
        let annotated = match &request.operation {
            WorkerOperation::Generate {
                settings, model, ..
            } if settings.annotated_view.is_some() => Some(
                model
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("annotated archival model absent"))?,
            ),
            _ => None,
        };
        if origin.annotated_model.as_ref() != annotated.map(|m| &m.identity) {
            return Err(ProtocolError::invalid(
                "annotated worker model membership missing or unexpected",
            ));
        }
        let completed_cuda = request.cuda_execution.is_some() && origin.state == "completed";
        if origin.cuda_evidence.is_some() != completed_cuda {
            return Err(ProtocolError::invalid(
                "archived CUDA completion membership is missing or unexpected",
            ));
        }
        if let Some(cuda) = &origin.cuda_evidence {
            let _memory = pool.reserve_memory(128 * 1024 * 1024)?;
            if format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&request).map_err(internal)?)
            ) != cuda.request_sha256
            {
                return Err(ProtocolError::invalid(
                    "archived CUDA canonical request digest differs",
                ));
            }
            check()?;
            let receipt = bytes(&identity_ref(&cuda.receipt))?;
            let trace = bytes(&identity_ref(&cuda.trace))?;
            crate::cuda_result::verify_bytes(&request, &receipt, &trace).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "archived CUDA receipt/trace correspondence failed",
                )
            })?;
        }
        for candidate in candidates_by_job.get(&origin.job_id).into_iter().flatten() {
            if manifest.format_version >= PROJECT_PACKAGE_FORMAT_VERSION_WITH_CANDIDATE_INTERVALS
                && candidate.project_interval != scope
            {
                return Err(ProtocolError::invalid(
                    "native portable interval differs from checked worker settings",
                ));
            }
            scopes.insert(candidate.candidate_id.clone(), scope);
        }
        // Paths remain inert text. Never open, execute, or translate WorkerRequest paths.
        if let Some(audio) = &request.independent_audio {
            let source = sources.get(&audio.source_version).ok_or_else(|| {
                ProtocolError::invalid("archived independent audio source absent")
            })?;
            if source.kind != SourceKind::Media || audio.identity != source.identity {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "archived independent audio differs from source payload",
                ));
            }
        }
        if let Some(identity) = &origin.receipt {
            let receipt: ArchivedWorkerReceipt =
                serde_json::from_slice(&bytes(&identity_ref(identity))?)
                    .map_err(|_| ProtocolError::invalid("invalid archived worker receipt"))?;
            if (if annotated.is_some() {
                receipt.schema_version != 3
            } else {
                !matches!(receipt.schema_version, 1 | 2)
            }) || receipt.recipe.is_empty()
                || receipt.project_id != manifest.origin_project_id
                || receipt.job_id != origin.job_id
                || receipt.attempt_id != origin.attempt_id
                || receipt.base_revision != origin.base_revision
                || receipt.source_version != origin.source_version
                || origin.program.as_ref() != Some(&receipt.program)
            {
                return Err(ProtocolError::invalid(
                    "archived worker receipt differs from declared origin",
                ));
            }
            let mut dependencies = request.dependencies.clone();
            dependencies.extend([
                request.source.identity.clone(),
                request.tools.ffmpeg.identity.clone(),
                request.tools.ffprobe.identity.clone(),
            ]);
            if let Some(runtime) = &request.tools.onnx_runtime {
                dependencies.push(runtime.identity.clone());
            }
            if let WorkerOperation::Generate {
                model: Some(model), ..
            }
            | WorkerOperation::Preview {
                model: Some(model), ..
            } = &request.operation
            {
                dependencies.push(model.identity.clone());
            }
            if receipt
                .dependencies
                .iter()
                .any(|dependency| !dependencies.contains(dependency))
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "archived worker dependency not declared by inert manifest",
                ));
            }
            let original: pulsar_core::MotionProgram =
                serde_json::from_slice(&bytes(&identity_ref(&receipt.program))?).map_err(|_| {
                    ProtocolError::invalid("archived worker output is not checked motion")
                })?;
            for candidate in candidates_by_job.get(&origin.job_id).into_iter().flatten() {
                let suffix = if origin.cuda_evidence.is_some() { 3 } else { 1 };
                let end = candidate.lineage.len().checked_sub(suffix).ok_or_else(|| {
                    ProtocolError::invalid("archived candidate receipt lineage incomplete")
                })?;
                let general = match &origin.cuda_evidence {
                    Some(cuda) => cuda.general_receipt(&candidate.lineage)?,
                    None => &candidate.lineage[end],
                };
                if general != identity || candidate.lineage[..end] != receipt.dependencies {
                    return Err(ProtocolError::invalid(
                        "archived candidate receipt lineage disagrees",
                    ));
                }
                if let Some(model) = annotated {
                    let needed = model
                        .identity
                        .byte_len
                        .checked_add(identity.byte_len)
                        .and_then(|n| n.checked_mul(8))
                        .and_then(|n| n.checked_add(16 * 1024 * 1024))
                        .ok_or_else(|| {
                            ProtocolError::invalid("annotated archival verification size overflow")
                        })?;
                    let _memory = pool.reserve_memory(needed)?;
                    let raw_receipt = bytes(&identity_ref(identity))?;
                    let raw_model = bytes(&identity_ref(&model.identity))?;
                    crate::worker::annotated_receipt::verify_readers(
                        &request,
                        &receipt.program,
                        identity,
                        &candidate.lineage[..end],
                        &candidate.review,
                        std::io::Cursor::new(&raw_receipt),
                        std::io::Cursor::new(raw_model),
                    )?;
                    if matches!(&request.operation,WorkerOperation::Generate {settings,..} if settings.source_placement.is_some())
                    {
                        let canonical = ArtifactIdentity {
                            artifact_id: candidate.motion.artifact_id.clone(),
                            sha256: candidate.motion.sha256.clone(),
                            byte_len: candidate.motion.byte_len,
                        };
                        crate::source_clock_receipts::load_verified_readers(
                            &request,
                            &candidate.lineage,
                            &canonical,
                            std::io::Cursor::new(raw_receipt),
                            identity.byte_len,
                            |raw, _| {
                                if origin.program.as_ref() != Some(raw) {
                                    return Err(ProtocolError::invalid(
                                        "annotated raw clock program differs from origin",
                                    ));
                                }
                                Ok(Box::new(std::io::Cursor::new(bytes(&identity_ref(raw))?))
                                    as Box<dyn std::io::Read>)
                            },
                        )?;
                    }
                }
                let canonical: pulsar_core::MotionProgram =
                    serde_json::from_slice(&bytes(&PackageObjectRef {
                        sha256: candidate.motion.sha256.clone(),
                        byte_len: candidate.motion.byte_len,
                    })?)
                    .map_err(|_| ProtocolError::invalid("archived canonical motion invalid"))?;
                if canonical != original {
                    return Err(ProtocolError::invalid(
                        "archived original output differs from candidate motion",
                    ));
                }
            }
        }
    }
    Ok(scopes)
}

#[cfg(test)]
pub(super) fn exercise_authored_cuda_portable_lifecycle(
    bytes: &[u8],
    trace: &ArtifactIdentity,
    old_reader: Option<&std::path::Path>,
) {
    tests::exercise_authored_cuda_portable_lifecycle(bytes, trace, old_reader);
}
#[cfg(all(test, feature = "wasm"))]
pub(in crate::authority) fn exercise_authored_wasm_portable_lifecycle(
    bytes: &[u8],
    module: &ArtifactIdentity,
    old_reader: Option<&std::path::Path>,
) {
    tests::exercise_authored_portable_lifecycle(bytes, module, old_reader, 12, None);
}
#[cfg(test)]
pub(crate) fn exercise_authored_annotated_portable_lifecycle(
    bytes: &[u8],
    model: &ArtifactIdentity,
    old_reader: Option<&std::path::Path>,
) {
    tests::exercise_authored_portable_lifecycle(bytes, model, old_reader, 11, None);
}
#[cfg(test)]
pub(in crate::authority) fn exercise_authored_image_portable_lifecycle(
    bytes: &[u8],
    image: &PackageImageOrigin,
) {
    let reader = std::env::var_os("PULSAR_IMAGE_PRECEDING_READER").map(PathBuf::from);
    let probe = std::env::var_os("PULSAR_IMAGE_PRECEDING_PACKAGE_PROBE").map(PathBuf::from);
    assert_eq!(
        reader.is_some(),
        probe.is_some(),
        "both preceding witnesses must be supplied"
    );
    if let Some(probe) = &probe {
        tests::exercise_image_preceding_packages(bytes, probe);
    }
    // Independent subjects exercise refusal after either required payload changes.
    for identity in [&image.record, &image.rgb] {
        tests::exercise_authored_portable_lifecycle(
            bytes,
            identity,
            reader.as_deref(),
            13,
            Some(image),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_package_format::write_package;
    struct Fixture {
        engine: Arc<Engine>,
        session: SessionId,
        token: String,
        project: ProjectId,
        dir: tempfile::TempDir,
    }
    impl Fixture {
        fn request(&self, command: Command) -> Request {
            Request::new(id!(RequestId), command)
                .with_session(self.session.clone())
                .with_auth_token(self.token.clone())
        }
        fn call(&self, command: Command) -> PResult<ResponseBody> {
            self.engine.handle(self.request(command)).result
        }
        fn scoped(
            &self,
            command: Command,
            project: &ProjectId,
            revision: Option<u64>,
        ) -> PResult<ResponseBody> {
            self.engine
                .handle(
                    self.request(command)
                        .in_project(project.clone(), revision.map(RevisionId::new)),
                )
                .result
        }
        fn bytes(&self, project: &ProjectId, revision: u64) -> Vec<u8> {
            let plan = project_packages::capture(
                &self.engine,
                &self.session,
                &self.token,
                project,
                RevisionId::new(revision),
            )
            .unwrap();
            let mut bytes = Vec::new();
            write_package(
                &plan.manifest,
                &mut bytes,
                |object| plan.open_object(object),
                PackageCodecLimits {
                    max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
                },
            )
            .unwrap();
            bytes
        }
        fn start(&self, bytes: &[u8]) -> (Request, PackageImportStatus) {
            let package = PackageUploadDeclaration {
                format_version: package_format_version(&bytes[..52]).unwrap(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                byte_len: bytes.len() as u64,
            };
            let request = self.request(Command::StartProjectImport { package });
            let ResponseBody::PackageImport(status) =
                self.engine.handle(request.clone()).result.unwrap()
            else {
                panic!()
            };
            (request, *status)
        }
        fn begin(&self, status: &PackageImportStatus) -> PackageUploadLease {
            let ResponseBody::PackageUpload(lease) = self
                .call(Command::BeginProjectPackageUpload {
                    operation_id: status.operation_id.clone(),
                })
                .unwrap()
            else {
                panic!()
            };
            lease
        }
        fn handshake(&self, lease: &PackageUploadLease) -> PackageUploadHandshake {
            PackageUploadHandshake {
                version: PROTOCOL_VERSION,
                channel: PackageUploadChannel::ProjectPackageUpload,
                session: self.session.clone(),
                auth_token: self.token.clone(),
                lease_id: lease.lease_id.clone(),
                engine_epoch: lease.engine_epoch.clone(),
                operation_id: lease.operation_id.clone(),
                generation: lease.generation,
                package_sha256: lease.package.sha256.clone(),
            }
        }
        fn upload(&self, status: &PackageImportStatus, bytes: &[u8]) -> PackageUploadLease {
            let lease = self.begin(status);
            let handshake = self.handshake(&lease);
            let (connection, _) = self.engine.import_bulk_open(&handshake).unwrap();
            let mut offset = 0;
            for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
                offset = self
                    .engine
                    .import_bulk_append(&connection, &handshake, offset, chunk)
                    .unwrap();
            }
            assert_eq!(offset, bytes.len() as u64);
            drop(connection);
            lease
        }
        fn finish(
            &self,
            status: &PackageImportStatus,
            lease: &PackageUploadLease,
        ) -> PackageImportStatus {
            self.call(Command::SealProjectImport {
                operation_id: status.operation_id.clone(),
                upload_generation: lease.generation,
            })
            .unwrap();
            self.wait(&status.operation_id)
        }
        fn wait(&self, id: &PackageImportOperationId) -> PackageImportStatus {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let ResponseBody::PackageImport(status) = self
                    .call(Command::ProjectImportStatus {
                        operation_id: id.clone(),
                    })
                    .unwrap()
                else {
                    panic!()
                };
                if status.state.is_terminal() {
                    return *status;
                }
                assert!(Instant::now() < deadline, "import timed out");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn drain(&self) {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if self
                    .engine
                    .package_imports
                    .lock()
                    .unwrap()
                    .active
                    .is_empty()
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn count(&self, table: &str) -> i64 {
            self.engine
                .db
                .lock()
                .unwrap()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        }
        fn import(&self, bytes: &[u8]) -> PackageImportReceipt {
            let (_, status) = self.start(bytes);
            let lease = self.upload(&status, bytes);
            let result = self.finish(&status, &lease);
            assert_eq!(
                result.state,
                PackageImportOperationState::Completed,
                "{:?}",
                result.error
            );
            self.drain();
            result.receipt.unwrap()
        }
    }
    pub(super) fn exercise_image_preceding_packages(bytes: &[u8], probe: &std::path::Path) {
        let f = fixture();
        let control = f.bytes(&f.project, 0);
        assert!(package_format_version(&control[..52]).unwrap() <= 12);
        for (name, package, code) in [("control", control.as_slice(), 0), ("image", bytes, 1)] {
            let path = f.dir.path().join(format!("preceding-{name}.pulspkg"));
            fs::write(&path, package).unwrap();
            let before = format!("{:x}", Sha256::digest(package));
            let result = std::process::Command::new("/usr/bin/timeout")
                .args(["--signal=TERM", "--kill-after=1", "5"])
                .arg(probe)
                .arg(&path)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .output()
                .unwrap();
            assert_eq!(
                result.status.code(),
                Some(code),
                "preceding package probe: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if code == 1 {
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("unsupported package format")
                );
            } else {
                assert!(String::from_utf8_lossy(&result.stdout)
                    .contains("preceding package prepared: format="));
            }
            assert_eq!(
                format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                before
            );
            eprintln!(
                "preceding package witness: {name} exit={code}; input unchanged; {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        assert_eq!(f.count("project_import_archives"), 0);
        assert_eq!(f.count("jobs"), 0);
    }
    pub(super) fn exercise_authored_cuda_portable_lifecycle(
        bytes: &[u8],
        trace: &ArtifactIdentity,
        old_reader: Option<&std::path::Path>,
    ) {
        exercise_authored_portable_lifecycle(bytes, trace, old_reader, 10, None);
    }
    pub(super) fn exercise_authored_portable_lifecycle(
        bytes: &[u8],
        trace: &ArtifactIdentity,
        old_reader: Option<&std::path::Path>,
        format: u16,
        image: Option<&PackageImageOrigin>,
    ) {
        fn check_image(
            f: &Fixture,
            project: &ProjectId,
            revision: RevisionId,
            image: &PackageImageOrigin,
        ) {
            let plan =
                project_packages::capture(&f.engine, &f.session, &f.token, project, revision)
                    .unwrap();
            assert_eq!(plan.manifest.format_version, 13);
            assert!(plan.manifest.generated_origins.is_empty());
            for identity in [&image.record, &image.rgb] {
                let object = plan
                    .manifest
                    .objects
                    .iter()
                    .find(|o| o.sha256 == identity.sha256 && o.byte_len == identity.byte_len)
                    .expect("retained image object");
                assert!(object
                    .roles
                    .contains(&PackageObjectRole::DependencyEvidence));
                let mut bytes = Vec::new();
                plan.open_object(object)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                assert_eq!(bytes.len() as u64, identity.byte_len);
                assert_eq!(format!("{:x}", Sha256::digest(&bytes)), identity.sha256);
            }
            let mut original_found = false;
            for object in plan
                .manifest
                .objects
                .iter()
                .filter(|o| o.roles.contains(&PackageObjectRole::ImportedManifest))
            {
                let mut bytes = Vec::new();
                plan.open_object(object)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                let original: PortableProjectManifest = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(original.canonical_bytes().unwrap(), bytes);
                for origin in &original.generated_origins {
                    if origin.image_origin.as_ref() == Some(image) {
                        original_found = true;
                    }
                }
            }
            assert!(
                original_found,
                "exact image origin remains in retained original namespace"
            );
            drop(plan);
            let id: String = f
                .engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [project.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let ResponseBody::CandidateImageOrigin(report) = f
                .scoped(
                    Command::CandidateImageOriginStatus {
                        candidate_id: CandidateId::new(id).unwrap(),
                    },
                    project,
                    None,
                )
                .unwrap()
            else {
                panic!("clone image inspection");
            };
            report.validate().unwrap();
            assert_eq!(report.origins.len(), 1);
            assert_eq!(report.origins[0].record, image.record);
            assert_eq!(
                report.origins[0]
                    .approval
                    .report
                    .image
                    .as_ref()
                    .unwrap()
                    .rgb,
                image.rgb
            );
            assert!(report.origins[0].origin_manifest.is_some());
            assert_ne!(report.origins[0].approval.report.project_id, *project);
            assert!(
                !report.execution_authority
                    && !report.observed_temporal_motion
                    && !report.qualified
            );
            assert_eq!(f.count("jobs"), 0);
            assert_eq!(f.count("attempts"), 0);
            assert!(f.engine.workers.lock().unwrap().is_empty());
            #[cfg(unix)]
            assert!(!f.engine.worker_fence.has_pending_worker_custody().unwrap());
            let ResponseBody::CandidateImageOriginPixels(pixels) = f
                .scoped(
                    Command::CandidateImageOriginPixels {
                        candidate_id: report.candidate_id.clone(),
                        record_sha256: image.record.sha256.clone(),
                    },
                    project,
                    None,
                )
                .unwrap()
            else {
                panic!("inert retained pixels");
            };
            pixels.validate().unwrap();
            assert_eq!(pixels.frame.artifact.identity, image.rgb);
            assert!(pixels
                .frame
                .artifact
                .path
                .starts_with(f.engine.config.state_dir.join("imported-project-evidence")));
            candidate_image_origin::retain_pixel_observation(&pixels);
            assert_eq!(f.count("jobs"), 0);
            assert_eq!(f.count("attempts"), 0);
            eprintln!("inert conditioning pixels: exact retained RGB selected from current clone CAS; no old host paths or workers");
            eprintln!("inert image clone inspection: format=13 exact approval/origin/RGB and original namespace verified; jobs=0 attempts=0 workers=0");
        }
        fn refuse_old_reader(config: &EngineConfig, old: &std::path::Path, format: u16) {
            let database = config.state_dir.join("projects.sqlite3");
            let wal = config.state_dir.join("projects.sqlite3-wal");
            assert!(fs::metadata(&wal).unwrap().len() > 32);
            let identity = |path: &std::path::Path| {
                fs::read(path)
                    .ok()
                    .map(|bytes| (bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
            };
            let before = [identity(&database), identity(&wal)];
            let mut command = std::process::Command::new("/usr/bin/timeout");
            command
                .args(["--signal=TERM", "--kill-after=1", "5"])
                .arg(old)
                .arg("engine")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PULSAR_STATE_DIR", &config.state_dir);
            if format == 13 {
                command.env("PULSAR_WORKER_EXECUTABLE", &config.worker_executable);
            }
            let result = command.output().unwrap();
            assert_eq!(
                result.status.code(),
                Some(1),
                "old clone reader refusal: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let stderr = String::from_utf8_lossy(&result.stderr);
            if format == 13 {
                assert!(
                    stderr.contains("unsupported portable project format")
                        || (stderr.contains("unknown field") && stderr.contains("image_origin")),
                    "preceding format-12 reader must refuse image origin: {stderr}"
                );
                assert!(!config.state_dir.join("engine.sock").exists());
                eprintln!("preceding image state refusal: exit=1; {stderr}");
            } else {
                assert!(stderr.contains("explicit recoverable migration"));
            }
            assert_eq!(
                [identity(&database), identity(&wal)],
                before,
                "old clone reader must preserve committed DB/WAL"
            );
        }
        fn release_owned_reader_probe(f: Fixture, old: &std::path::Path) -> Fixture {
            f.drain();
            let config = f.engine.config.clone();
            let keeper = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
            keeper
                .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
                .unwrap();
            let _: i64 = keeper
                .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
                .unwrap();
            let lifetime = Arc::downgrade(&f.engine);
            let Fixture {
                engine,
                session,
                token,
                project,
                dir,
            } = f;
            drop(engine);
            let deadline = Instant::now() + Duration::from_secs(20);
            while lifetime.strong_count() != 0 {
                assert!(Instant::now() < deadline, "owned import engine still live");
                std::thread::sleep(Duration::from_millis(1));
            }
            refuse_old_reader(&config, old, 13);
            let engine = Engine::open(config).unwrap();
            drop(keeper);
            Fixture {
                engine,
                session,
                token,
                project,
                dir,
            }
        }
        let f = fixture();
        let first = f.import(bytes);
        assert_eq!(first.package.format_version, format);
        {
            let db = f.engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                if format >= 12 {
                    25
                } else if format == 11 {
                    24
                } else {
                    18
                }
            );
            let state = crate::portable_cuda_db::validate(&db, 18).unwrap().unwrap();
            assert!(!state.worker_fence && !state.cuda_intents && !state.cuda_evidence);
        }
        let f = if let Some(old) = old_reader {
            if format == 13 {
                release_owned_reader_probe(f, old)
            } else {
                refuse_old_reader(&f.engine.config, old, format);
                f
            }
        } else {
            f
        };
        let f = restart(f);
        if let Some(image) = image {
            check_image(&f, &first.project_id, first.revision, image);
        }
        let first_cuda_origin = if format == 10 {
            let id: String = f
                .engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [first.project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let candidate = CandidateId::new(id).unwrap();
            let ResponseBody::ArchivedGeneration(report) = f
                .scoped(
                    Command::ArchivedGenerationStatus {
                        candidate_id: candidate.clone(),
                        master_revision: None,
                    },
                    &first.project_id,
                    None,
                )
                .unwrap()
            else {
                panic!("archived CUDA generation")
            };
            assert_eq!(report.origins.len(), 1);
            assert!(
                report.origins[0].clock.is_none(),
                "unplaced fixture must not invent sampled clocks"
            );
            assert_ne!(
                report.origins[0].worker_receipt, *trace,
                "node trace is not the producer receipt"
            );
            assert!(
                report.archival
                    && !report.active_job
                    && !report.publication_authority
                    && !report.release_qualified
            );
            {
                let db = f.engine.db.lock().unwrap();
                let mut links = stash_archived_lineage::Links::new(
                    &db,
                    &first.project_id,
                    &f.engine.pool,
                    &f.engine.import_store,
                    &report.origins,
                )
                .unwrap();
                assert!(matches!(
                    links
                        .candidate(&db, &first.project_id, candidate.as_str())
                        .unwrap(),
                    Some(stash_archived_lineage::CandidateLink::Generation)
                ));
            }
            Some(report.origins)
        } else {
            None
        };
        if format == 11 {
            let id: String = f
                .engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [first.project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let report = f
                .scoped(
                    Command::ArchivedGenerationStatus {
                        candidate_id: CandidateId::new(id).unwrap(),
                        master_revision: None,
                    },
                    &first.project_id,
                    None,
                )
                .unwrap();
            let ResponseBody::ArchivedGeneration(report) = report else {
                panic!("archived generation")
            };
            assert_eq!(report.origins.len(), 1);
            assert_eq!(report.origins[0].clock.as_ref().unwrap().sample_count, 28);
            assert!(report.origins[0].settings.annotated_view.is_some());
        }
        let exported = f.bytes(&first.project_id, first.revision.value());
        assert_eq!(package_format_version(&exported[..52]).unwrap(), format);
        let second = f.import(&exported);
        assert_ne!(first.project_id, second.project_id);
        let f = if let Some(old) = old_reader {
            if format == 13 {
                release_owned_reader_probe(f, old)
            } else {
                refuse_old_reader(&f.engine.config, old, format);
                f
            }
        } else {
            f
        };
        let f = restart(f);
        if let Some(image) = image {
            check_image(&f, &second.project_id, second.revision, image);
        }
        if let Some(expected) = first_cuda_origin {
            let id: String = f
                .engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [second.project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            let candidate = CandidateId::new(id).unwrap();
            let ResponseBody::ArchivedGeneration(report) = f
                .scoped(
                    Command::ArchivedGenerationStatus {
                        candidate_id: candidate.clone(),
                        master_revision: None,
                    },
                    &second.project_id,
                    None,
                )
                .unwrap()
            else {
                panic!("second-clone archived CUDA generation")
            };
            assert_eq!(encode(&report.origins).unwrap(), encode(&expected).unwrap());
            assert!(
                report.archival
                    && !report.active_job
                    && !report.publication_authority
                    && !report.release_qualified
            );
            let db = f.engine.db.lock().unwrap();
            let mut links = stash_archived_lineage::Links::new(
                &db,
                &second.project_id,
                &f.engine.pool,
                &f.engine.import_store,
                &report.origins,
            )
            .unwrap();
            assert!(matches!(
                links
                    .candidate(&db, &second.project_id, candidate.as_str())
                    .unwrap(),
                Some(stash_archived_lineage::CandidateLink::Generation)
            ));
        }
        let plan = project_packages::capture(
            &f.engine,
            &f.session,
            &f.token,
            &second.project_id,
            second.revision,
        )
        .unwrap();
        assert_eq!(plan.manifest.format_version, format);
        assert_eq!(plan.manifest.imported_origins.len(), 2);
        assert!(plan.manifest.generated_origins.is_empty());
        assert_eq!(f.count("jobs"), 0);
        if format == 12 {
            assert_eq!(
                f.count("wasm_edits"),
                0,
                "inert imports must never restore executable operations"
            );
            assert!(f.engine.workers.lock().unwrap().is_empty());
        }
        drop(plan);
        let path = f
            .engine
            .config
            .state_dir
            .join("imported-project-evidence")
            .join(&trace.sha256);
        assert_eq!(fs::metadata(&path).unwrap().len(), trace.byte_len);
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(&path, b"[]").unwrap();
        fs::set_permissions(&path, original_permissions).unwrap();
        if let Some(image) = image {
            let id: String = f
                .engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT id FROM candidates WHERE project=?1",
                    [second.project_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                f.scoped(
                    Command::CandidateImageOriginStatus {
                        candidate_id: CandidateId::new(id.clone()).unwrap()
                    },
                    &second.project_id,
                    None
                )
                .is_err(),
                "changed retained image evidence must refuse inspection"
            );
            assert!(
                f.scoped(
                    Command::CandidateImageOriginPixels {
                        candidate_id: CandidateId::new(id).unwrap(),
                        record_sha256: image.record.sha256.clone()
                    },
                    &second.project_id,
                    None
                )
                .is_err(),
                "changed retained evidence refuses pixel preview"
            );
        }
        let config = f.engine.config.clone();
        let engine_lifetime = Arc::downgrade(&f.engine);
        drop(f.engine);
        let deadline = Instant::now() + Duration::from_secs(20);
        while engine_lifetime.strong_count() != 0 {
            assert!(
                Instant::now() < deadline,
                "import worker still holds engine"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let database = config.state_dir.join("projects.sqlite3");
        let wal = config.state_dir.join("projects.sqlite3-wal");
        // Retain an actual committed WAL across refusal; opening a WAL-mode
        // database with no WAL may create an empty sidecar even read-only.
        let keeper = Connection::open(&database).unwrap();
        keeper.execute_batch("PRAGMA journal_mode=WAL; UPDATE projects SET name=name || ' committed WAL fixture';").unwrap();
        assert!(fs::metadata(&wal).unwrap().len() > 32);
        let identity = |path: &std::path::Path| {
            fs::read(path)
                .ok()
                .map(|bytes| (bytes.len(), format!("{:x}", Sha256::digest(&bytes))))
        };
        let before = [identity(&database), identity(&wal)];
        assert!(
            Engine::open(config).is_err(),
            "changed retained required evidence must refuse startup"
        );
        assert_eq!(
            [identity(&database), identity(&wal)],
            before,
            "refused startup must preserve database and WAL bytes"
        );
        drop(keeper);
    }
    fn fixture() -> Fixture {
        fixture_with_budget(32 * 1024 * 1024 * 1024)
    }
    fn fixture_with_budget(budget: u64) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("worker");
        fs::write(&worker, b"not executed by clone import").unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        config.max_snapshot_bytes = budget;
        let engine = Engine::open(config).unwrap();
        let (session, token) = match engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "clone import test".into(),
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
            _ => panic!(),
        };
        let ResponseBody::Project(project) = engine
            .handle(
                Request::new(
                    id!(RequestId),
                    Command::CreateProject {
                        name: "Original".into(),
                    },
                )
                .with_session(session.clone())
                .with_auth_token(token.clone()),
            )
            .result
            .unwrap()
        else {
            panic!()
        };
        Fixture {
            engine,
            session,
            token,
            project: project.project_id,
            dir,
        }
    }
    fn restart(f: Fixture) -> Fixture {
        let Fixture {
            engine,
            session,
            token,
            project,
            dir,
        } = f;
        let config = engine.config.clone();
        drop(engine);
        Fixture {
            engine: Engine::open(config).unwrap(),
            session,
            token,
            project,
            dir,
        }
    }
    #[test]
    fn clone_publication_is_fresh_atomic_and_start_seal_replay_survives_restart() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (start, status) = f.start(&bytes);
        let lease = f.upload(&status, &bytes);
        let seal = f.request(Command::SealProjectImport {
            operation_id: status.operation_id.clone(),
            upload_generation: lease.generation,
        });
        f.engine.handle(seal.clone()).result.unwrap();
        let done = f.wait(&status.operation_id);
        assert_eq!(
            done.state,
            PackageImportOperationState::Completed,
            "{:?}",
            done.error
        );
        let receipt = done.receipt.unwrap();
        assert_ne!(receipt.project_id, f.project);
        assert_eq!(f.count("projects"), 2);
        assert_eq!(f.count("jobs"), 0);
        assert_eq!(f.count("attempts"), 0);
        assert_eq!(f.count("project_import_archives"), 1);
        f.drain();
        let f = restart(f);
        for request in [start, seal] {
            let ResponseBody::PackageImport(replayed) = f.engine.handle(request).result.unwrap()
            else {
                panic!()
            };
            assert_eq!(replayed.receipt.unwrap().project_id, receipt.project_id);
        }
        assert_eq!(f.count("projects"), 2);
        let db = f.engine.db.lock().unwrap();
        require_grant(&db, &f.session, &receipt.project_id, Scope::PackageProject).unwrap();
    }
    #[test]
    fn imported_originals_survive_new_local_edit_reexport_and_second_clone() {
        let f = fixture();
        let input = f.dir.path().join("original.funscript");
        fs::write(
            &input,
            br#"{"actions":[{"at":0,"pos":20},{"at":1000,"pos":80}]}"#,
        )
        .unwrap();
        let ResponseBody::Candidate(candidate) = f
            .scoped(
                Command::ImportFunscript { path: input },
                &f.project,
                Some(0),
            )
            .unwrap()
        else {
            panic!()
        };
        f.scoped(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            &f.project,
            Some(0),
        )
        .unwrap();
        let first = f.import(&f.bytes(&f.project, 1));
        let ResponseBody::Diagnostics(diagnostics) = f
            .scoped(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: None,
                },
                &first.project_id,
                None,
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(diagnostics
            .issues
            .iter()
            .any(|issue| issue.code == "imported_origin_unverified"));
        let edit = f.dir.path().join("new.funscript");
        fs::write(
            &edit,
            br#"{"actions":[{"at":0,"pos":30},{"at":1000,"pos":70}]}"#,
        )
        .unwrap();
        let ResponseBody::Candidate(candidate) = f
            .scoped(
                Command::ImportFunscript { path: edit },
                &first.project_id,
                Some(1),
            )
            .unwrap()
        else {
            panic!()
        };
        f.scoped(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            &first.project_id,
            Some(1),
        )
        .unwrap();
        let second_bytes = f.bytes(&first.project_id, 2);
        assert_eq!(package_format_version(&second_bytes[..52]).unwrap(), 2);
        let second = f.import(&second_bytes);
        assert_ne!(second.project_id, first.project_id);
        let plan = project_packages::capture(
            &f.engine,
            &f.session,
            &f.token,
            &second.project_id,
            RevisionId::new(2),
        )
        .unwrap();
        assert_eq!(plan.manifest.imported_origins.len(), 2);
        assert_eq!(plan.manifest.revisions.len(), 3);
        assert_eq!(plan.manifest.candidates.len(), 2);
        assert!(plan.manifest.generated_origins.is_empty());
        assert!(plan.manifest.authored_lineage.is_empty());
        let ResponseBody::Project(snapshot) = f
            .scoped(Command::GetSnapshot, &second.project_id, None)
            .unwrap()
        else {
            panic!()
        };
        let program = f
            .engine
            .motion_store
            .read(&snapshot.motion.program)
            .unwrap();
        assert_eq!(program.tracks()[0].actions().len(), 2);
    }
    #[test]
    fn upload_rejects_identity_offsets_and_conflicting_replay_without_advancing() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        let lease = f.begin(&status);
        let handshake = f.handshake(&lease);
        let mut bad = handshake.clone();
        bad.auth_token = "wrong".into();
        assert!(f.engine.import_bulk_open(&bad).is_err());
        let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
        assert!(f
            .engine
            .import_bulk_append(&connection, &handshake, 1, &bytes[..8])
            .is_err());
        assert_eq!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
                .unwrap(),
            8
        );
        assert_eq!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
                .unwrap(),
            8
        );
        assert!(f
            .engine
            .import_bulk_append(&connection, &handshake, 0, b"wrong---")
            .is_err());
        assert_eq!(
            f.engine
                .import_lease_view(&f.session, &lease.lease_id)
                .unwrap()
                .next_offset,
            8
        );
        drop(connection);
        f.call(Command::AbandonProjectPackageUpload {
            lease_id: lease.lease_id.clone(),
        })
        .unwrap();
        let new = f.begin(&status);
        assert!(new.generation > lease.generation);
        assert!(f.engine.import_bulk_open(&handshake).is_err());
        assert_eq!(f.count("projects"), 1);
        f.call(Command::CancelProjectImport {
            operation_id: status.operation_id,
        })
        .unwrap();
        f.drain();
    }
    #[test]
    fn unfinished_restart_interrupts_and_never_reuses_acknowledged_prefix() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (start, status) = f.start(&bytes);
        let lease = f.begin(&status);
        let handshake = f.handshake(&lease);
        let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
        f.engine
            .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
            .unwrap();
        drop(connection);
        let f = restart(f);
        let ResponseBody::PackageImport(result) = f.engine.handle(start).result.unwrap() else {
            panic!()
        };
        assert_eq!(result.state, PackageImportOperationState::Interrupted);
        assert!(f.engine.import_bulk_open(&handshake).is_err());
        assert_eq!(f.count("projects"), 1);
        assert!(f
            .call(Command::BeginProjectPackageUpload {
                operation_id: status.operation_id
            })
            .is_err());
    }
    #[test]
    fn operation_owner_and_global_request_identity_are_checked_before_effect() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (start, status) = f.start(&bytes);
        let (other, token) = match f
            .engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "other".into(),
                    pairing_token: f.engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        {
            ResponseBody::Paired {
                session,
                auth_token,
            } => (session, auth_token),
            _ => panic!(),
        };
        let request = Request::new(
            id!(RequestId),
            Command::ProjectImportStatus {
                operation_id: status.operation_id.clone(),
            },
        )
        .with_session(other)
        .with_auth_token(token);
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut conflict = start;
        conflict.command = Command::CreateProject {
            name: "must not exist".into(),
        };
        assert_eq!(
            f.engine.handle(conflict).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(f.count("projects"), 1);
        f.call(Command::CancelProjectImport {
            operation_id: status.operation_id,
        })
        .unwrap();
        f.drain();
    }
    #[test]
    fn digest_failure_and_spawn_failure_leave_no_project_or_owner_grant() {
        let f = fixture();
        let mut bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        bytes[51] ^= 1;
        let lease = f.upload(&status, &bytes);
        let failed = f.finish(&status, &lease);
        assert_eq!(failed.state, PackageImportOperationState::Failed);
        f.drain();
        assert_eq!(f.count("projects"), 1);
        let bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        let lease = f.upload(&status, &bytes);
        FAIL_SPAWN.with(|flag| flag.set(true));
        let failed = f.finish(&status, &lease);
        assert_eq!(failed.state, PackageImportOperationState::Failed);
        f.drain();
        assert_eq!(f.count("projects"), 1);
        assert_eq!(f.count("grants"), 8);
    }

    #[test]
    fn old_generation_cannot_ack_after_abandon_and_replacement() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        let lease = f.begin(&status);
        let handshake = f.handshake(&lease);
        let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
        let engine = f.engine.clone();
        let session = f.session.clone();
        let token = f.token.clone();
        let old = lease.lease_id.clone();
        let operation = status.operation_id.clone();
        BEFORE_ACK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let request = |command| {
                    Request::new(id!(RequestId), command)
                        .with_session(session.clone())
                        .with_auth_token(token.clone())
                };
                engine
                    .handle(request(Command::AbandonProjectPackageUpload {
                        lease_id: old,
                    }))
                    .result
                    .unwrap();
                engine
                    .handle(request(Command::BeginProjectPackageUpload {
                        operation_id: operation,
                    }))
                    .result
                    .unwrap();
            }))
        });
        assert!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
                .is_err(),
            "old-generation ACK was admitted after replacement"
        );
        let ResponseBody::PackageImport(current) = f
            .call(Command::ProjectImportStatus {
                operation_id: status.operation_id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(current.progress.received_bytes, 0);
        drop(connection);
        f.call(Command::CancelProjectImport {
            operation_id: status.operation_id,
        })
        .unwrap();
        f.drain();
    }
    #[test]
    fn publication_failure_rolls_back_whole_graph_and_keeps_replay_tombstone() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (start, status) = f.start(&bytes);
        let lease = f.upload(&status, &bytes);
        FAIL_PUBLICATION.with(|flag| flag.set(true));
        let failed = f.finish(&status, &lease);
        assert_eq!(failed.state, PackageImportOperationState::Failed);
        f.drain();
        assert_eq!(f.count("projects"), 1);
        assert_eq!(f.count("grants"), 8);
        assert_eq!(f.count("project_import_archives"), 0);
        assert!(
            f.engine.import_store.allocated_bytes().unwrap() > 0,
            "completed orphans remain conservatively charged"
        );
        let ResponseBody::PackageImport(replayed) = f.engine.handle(start).result.unwrap() else {
            panic!()
        };
        assert_eq!(replayed.state, PackageImportOperationState::Failed);
    }
    #[test]
    fn exact_legacy_cells_and_absent_imported_lineage_survive_clone_restart() {
        let f = fixture();
        let raw = serde_json::to_string_pretty(&pulsar_core::MotionProgram::default()).unwrap();
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE revisions SET program=?2 WHERE project=?1 AND revision=0",
                params![f.project.as_str(), raw],
            )
            .unwrap();
        let f = restart(f);
        // An explicitly absent old lineage row is preserved, not inferred from equal bytes.
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM revision_lineage WHERE project=?1",
                [f.project.as_str()],
            )
            .unwrap();
        let receipt = f.import(&f.bytes(&f.project, 0));
        let f = restart(f);
        let plan = project_packages::capture(
            &f.engine,
            &f.session,
            &f.token,
            &receipt.project_id,
            RevisionId::new(0),
        )
        .unwrap();
        assert!(plan.manifest.revision_lineage.is_empty());
        let cell = plan
            .manifest
            .legacy_cells
            .iter()
            .find(|cell| cell.owner == PackageLegacyOwner::Revisions)
            .unwrap();
        let descriptor = plan
            .manifest
            .objects
            .iter()
            .find(|object| object.sha256 == cell.object.sha256)
            .unwrap();
        let mut got = String::new();
        plan.open_object(descriptor)
            .unwrap()
            .read_to_string(&mut got)
            .unwrap();
        assert_eq!(got, raw);
    }

    #[test]
    fn cancelled_inflight_upload_keeps_snapshot_reservation_until_io_drains() {
        let f = fixture_with_budget(16 * 1024 * 1024 + 224 * 1024);
        let source = f.dir.path().join("retained-source");
        fs::write(&source, vec![7u8; 64 * 1024]).unwrap();
        f.scoped(Command::ImportSource { path: source }, &f.project, None)
            .unwrap();
        let bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        let lease = f.begin(&status);
        let handshake = f.handshake(&lease);
        let (connection, _) = f.engine.import_bulk_open(&handshake).unwrap();
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        let engine = f.engine.clone();
        let session = f.session.clone();
        let token = f.token.clone();
        let operation = status.operation_id.clone();
        let declaration = status.package.clone();
        BEFORE_APPEND_IO.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let request = |command| {
                    Request::new(id!(RequestId), command)
                        .with_session(session.clone())
                        .with_auth_token(token.clone())
                };
                engine
                    .handle(request(Command::CancelProjectImport {
                        operation_id: operation,
                    }))
                    .result
                    .unwrap();
                *output.lock().unwrap() = Some(
                    engine
                        .handle(request(Command::StartProjectImport {
                            package: declaration,
                        }))
                        .result,
                );
            }))
        });
        assert!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes)
                .is_err(),
            "cancelled write must not ACK"
        );
        let allocation = f.engine.import_store.allocated_bytes().unwrap();
        let reserved = f
            .engine
            .package_imports
            .lock()
            .unwrap()
            .active
            .values()
            .map(|active| active.reserved_bytes.load(Ordering::Acquire))
            .sum::<u64>();
        let outcome = result.lock().unwrap().take().unwrap();
        let admitted = match &outcome {
            Ok(ResponseBody::PackageImport(value)) => Some(value.operation_id.clone()),
            _ => None,
        };
        drop(connection);
        if let Some(operation) = admitted {
            f.call(Command::CancelProjectImport {
                operation_id: operation,
            })
            .unwrap();
        }
        f.drain();
        assert!(matches!(outcome,Err(ProtocolError{code:ErrorCode::ResourceExhausted,..})),
            "new import admitted while cancelled I/O still owned reservation; allocation={allocation}, active_reserved={reserved}, configured_cap={}",f.engine.config.max_snapshot_bytes);
    }

    #[test]
    fn busy_unrelated_upload_does_not_block_idle_lease_status_or_abandon() {
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (_, first) = f.start(&bytes);
        let (_, second) = f.start(&bytes);
        f.begin(&first);
        f.begin(&second);
        let entries = f
            .engine
            .package_imports
            .lock()
            .unwrap()
            .active
            .values()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        let idle_lease = entries[1].data.lock().unwrap().lease.clone().unwrap();
        let busy = entries[0].data.lock().unwrap();
        let status = f.call(Command::ProjectPackageUploadStatus {
            lease_id: idle_lease.lease_id.clone(),
        });
        let abandoned = f.call(Command::AbandonProjectPackageUpload {
            lease_id: idle_lease.lease_id.clone(),
        });
        drop(busy);
        for operation_id in [first.operation_id, second.operation_id] {
            f.call(Command::CancelProjectImport { operation_id })
                .unwrap();
        }
        f.drain();
        match status.unwrap() {
            ResponseBody::PackageUpload(value) => {
                assert_eq!(value.lease_id, idle_lease.lease_id);
                assert_eq!(value.generation, idle_lease.generation);
                assert_eq!(value.next_offset, 0);
            }
            other => panic!("idle lease status returned {other:?}"),
        }
        match abandoned.unwrap() {
            ResponseBody::PackageUploadAbandoned { lease_id } => {
                assert_eq!(lease_id, idle_lease.lease_id);
            }
            other => panic!("idle lease abandon returned {other:?}"),
        }
    }

    #[test]
    fn occupied_upload_connection_is_typed_and_resumes_only_after_guard_drop() {
        fn rejection(engine: &Engine, handshake: &PackageUploadHandshake) -> ProtocolError {
            match engine.import_bulk_open(handshake) {
                Err(error) => error,
                Ok(_) => panic!("second upload connection was admitted"),
            }
        }
        let f = fixture();
        let bytes = f.bytes(&f.project, 0);
        let (_, status) = f.start(&bytes);
        let lease = f.begin(&status);
        let handshake = f.handshake(&lease);
        let (connection, admitted) = f.engine.import_bulk_open(&handshake).unwrap();
        assert_eq!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
                .unwrap(),
            8
        );
        let invalid_chunk = f
            .engine
            .import_bulk_append(&connection, &handshake, 0, b"wrong---")
            .unwrap_err();
        assert_eq!(invalid_chunk.code, ErrorCode::InvalidRequest);
        let active = connection.active.clone();
        let original_deadline = active.data.lock().unwrap().lease_deadline;
        let busy = rejection(&f.engine, &handshake);
        assert_eq!(busy.code, ErrorCode::PackageUploadConnectionBusy);
        assert!(busy.retryable);
        assert_eq!(
            active.data.lock().unwrap().connection.as_deref(),
            Some(connection.nonce.as_str())
        );
        for field in 0..5 {
            let mut invalid = handshake.clone();
            match field {
                0 => invalid.auth_token = "wrong".into(),
                1 => invalid.lease_id = id!(PackageUploadId),
                2 => invalid.engine_epoch = "prior-epoch".into(),
                3 => invalid.generation += 1,
                _ => invalid.package_sha256 = "0".repeat(64),
            }
            let error = rejection(&f.engine, &invalid);
            assert_ne!(error.code, ErrorCode::PackageUploadConnectionBusy);
        }
        assert_eq!(
            f.engine
                .import_bulk_append(&connection, &handshake, 0, &bytes[..8])
                .unwrap(),
            8
        );
        drop(connection);
        let (resumed, progress) = f.engine.import_bulk_open(&handshake).unwrap();
        assert_eq!(progress.lease_id, admitted.lease_id);
        assert_eq!(progress.generation, admitted.generation);
        assert_eq!(progress.engine_epoch, admitted.engine_epoch);
        assert_eq!(progress.next_offset, 8);
        assert!(progress.expires_after_ms <= admitted.expires_after_ms);
        assert_eq!(
            active.data.lock().unwrap().lease_deadline,
            original_deadline
        );
        assert_eq!(
            f.engine
                .import_bulk_append(&resumed, &handshake, 0, &bytes[..8])
                .unwrap(),
            8
        );
        drop(resumed);
        f.call(Command::CancelProjectImport {
            operation_id: status.operation_id,
        })
        .unwrap();
        f.drain();
    }

    #[test]
    fn unavailable_expiry_sweeper_prevents_engine_initialization() {
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("nonexecuted-worker");
        fs::write(&worker, b"expiry initialization fixture").unwrap();
        let config = EngineConfig::new(dir.path().join("state"), worker);
        FAIL_EXPIRY_SPAWN.with(|flag| flag.set(true));
        match Engine::open(config.clone()) {
            Err(error) => assert!(error.to_string().contains("expiry")),
            Ok(_) => panic!("engine initialized without its required import expiry worker"),
        }
        assert!(
            !config.endpoint().exists(),
            "failed initialization must not expose a control endpoint"
        );
        let recovered = Engine::open(config).unwrap();
        assert!(recovered.package_imports.lock().unwrap().active.is_empty());
    }
}
