//! Exact retained source bytes and declared placement clocks; no playback authority.
use super::*;
use crate::project_package_storage::ArtifactHolds;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, AtomicUsize},
        Weak,
    },
    time::{Duration, Instant},
};
#[path = "browser_media_reader.rs"]
mod reader;
use reader::VerifiedSourceReader;
const MAX_PREPARATIONS: usize = 4;
const MAX_LEASES: usize = 8;
const MAX_CONNECTIONS: usize = 8;
const VERIFY_LIMIT: Duration = Duration::from_secs(120);
const CHUNK_DEADLINE: Duration = Duration::from_secs(10);
#[cfg(test)]
type CaptureGate = Arc<dyn Fn(&AtomicBool, Instant) + Send + Sync>;

pub(super) fn is_command(c: &Command) -> bool {
    matches!(
        c,
        Command::BrowserMediaCatalog
            | Command::PrepareBrowserMedia { .. }
            | Command::PrepareBrowserMediaSuccessor { .. }
            | Command::BrowserMediaPreparationStatus { .. }
            | Command::ReleaseBrowserMedia { .. }
    )
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "browser media authority is not current",
    )
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "browser media selection changed",
    )
}
fn capacity() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::ResourceExhausted,
        "browser media admission exhausted",
    )
}
fn epoch(engine: &Engine) -> PResult<AttemptId> {
    AttemptId::new(engine.engine_epoch.clone()).map_err(internal)
}
fn hash_token(r: &Request) -> PResult<String> {
    r.auth_token
        .as_ref()
        .map(|token| format!("{:x}", Sha256::digest(token.as_bytes())))
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::Unauthorized, "session authentication required")
        })
}
fn lease_id() -> ArtifactId {
    ArtifactId::new(format!("{:x}", Sha256::digest(Uuid::new_v4().as_bytes()))).expect("digest ID")
}
fn key(actor: &SessionId, request: &RequestId) -> (SessionId, RequestId) {
    (actor.clone(), request.clone())
}

struct Preparation {
    owner: Weak<Engine>,
    actor: SessionId,
    auth_hash: String,
    fingerprint: String,
    report: Mutex<BrowserMediaPreparationReport>,
    cancel: Arc<AtomicBool>,
    progress: AtomicU64,
    deadline: Instant,
    done: AtomicBool,
    persisted: AtomicBool,
    verification: Mutex<Option<BrowserMediaPreparationReport>>,
    successor: Option<Arc<MediaSuccessorLineage>>,
    _metadata: Reservation,
}
struct Lease {
    owner: Weak<Engine>,
    actor: SessionId,
    auth_hash: String,
    binding: BrowserMediaBinding,
    reader: VerifiedSourceReader,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    connections: Mutex<HashSet<String>>,
    successor: Option<Arc<MediaSuccessorLineage>>,
    successor_committed: AtomicBool,
    _holds: ArtifactHolds,
    _memory: Reservation,
}
struct MediaSuccessorLineage {
    request_id: RequestId,
    predecessor: Weak<Lease>,
    committed: AtomicBool,
    cancelled: Arc<AtomicBool>,
}
impl MediaSuccessorLineage {
    fn predecessor_current(&self) -> bool {
        self.predecessor
            .upgrade()
            .is_some_and(|lease| SourceMediaPin(lease).current())
    }
}
/// Actual independently verified old/new lease custody. No serialized binding
/// or historical preparation response can construct this transfer witness.
#[derive(Clone)]
pub(super) struct NativeBrowserMediaSuccessor {
    predecessor: SourceMediaPin,
    successor: SourceMediaPin,
    lineage: Arc<MediaSuccessorLineage>,
}
impl NativeBrowserMediaSuccessor {
    /// Derive the one-use lineage only from existing native source custody.
    pub(super) fn from_pins(old: SourceMediaPin, next: SourceMediaPin) -> PResult<Self> {
        let predecessor = old.binding();
        let successor = next.binding();
        if predecessor.engine_epoch != successor.engine_epoch
            || predecessor.project_id != successor.project_id
            || predecessor.revision != successor.revision
            || predecessor.selection != successor.selection
            || predecessor.clock != successor.clock
            || predecessor.lease_id == successor.lease_id
            || predecessor.preparation_request_id == successor.preparation_request_id
        {
            return Err(mismatch());
        }
        let lineage = next.0.successor.as_ref().ok_or_else(mismatch)?.clone();
        if lineage.request_id != successor.preparation_request_id
            || lineage.committed.load(Ordering::Acquire)
            || !lineage
                .predecessor
                .upgrade()
                .is_some_and(|parent| Arc::ptr_eq(&parent, &old.0))
        {
            return Err(mismatch());
        }
        let proof = NativeBrowserMediaSuccessor {
            predecessor: old,
            successor: next,
            lineage,
        };
        if !proof.current() {
            return Err(unavailable());
        }
        Ok(proof)
    }
    #[cfg(test)]
    pub(super) fn predecessor(&self) -> &SourceMediaPin {
        &self.predecessor
    }
    pub(super) fn successor(&self) -> &SourceMediaPin {
        &self.successor
    }
    #[cfg(test)]
    pub(super) fn original_request_id(&self) -> &RequestId {
        &self.lineage.request_id
    }
    pub(super) fn current(&self) -> bool {
        self.predecessor.current()
            && self.successor.current()
            && !self.lineage.cancelled.load(Ordering::Acquire)
            && self
                .lineage
                .predecessor
                .upgrade()
                .is_some_and(|old| Arc::ptr_eq(&old, &self.predecessor.0))
            && self
                .successor
                .0
                .successor
                .as_ref()
                .is_some_and(|lineage| Arc::ptr_eq(lineage, &self.lineage))
    }
    /// Native lineage eligibility only. The producer's joint transfer guard
    /// invokes this after its exact boundary proof; this never activates a source.
    /// No locks or I/O are acquired here, and no original lease is extended.
    pub(super) fn commit(&self) -> PResult<()> {
        if !self.current() || self.lineage.committed.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        self.predecessor
            .0
            .successor_committed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| mismatch())?;
        if !self.current() {
            self.lineage.cancelled.store(true, Ordering::Release);
            return Err(unavailable());
        }
        self.lineage.committed.store(true, Ordering::Release);
        Ok(())
    }
    pub(super) fn matches(&self, predecessor: &SourceMediaPin, successor: &SourceMediaPin) -> bool {
        Arc::ptr_eq(&self.predecessor.0, &predecessor.0)
            && Arc::ptr_eq(&self.successor.0, &successor.0)
    }
}
/// A native capability, never reconstructed from a serialized binding. Capture
/// and guard owners retain source custody after the registry retires admission.
#[derive(Clone)]
pub(super) struct SourceMediaPin(Arc<Lease>);
impl SourceMediaPin {
    pub(super) fn binding(&self) -> &BrowserMediaBinding {
        &self.0.binding
    }
    /// Conservative future coverage of this original lease. The Instant is
    /// sampled after the caller's native reference, so elapsed work shortens
    /// coverage. This never changes the deadline or revives a retired owner.
    pub(super) fn covers_duration(&self, duration: std::time::Duration) -> bool {
        let sampled = Instant::now();
        self.current()
            && sampled
                .checked_add(duration)
                .is_some_and(|until| until < self.0.deadline)
            && self.current()
    }
    pub(super) fn current(&self) -> bool {
        self.0.owner.strong_count() > 0
            && !self.0.cancel.load(Ordering::Acquire)
            && Instant::now() < self.0.deadline
            && self.0.successor.as_ref().is_none_or(|lineage| {
                lineage.committed.load(Ordering::Acquire) || lineage.predecessor_current()
            })
    }
}
#[derive(Default)]
pub(super) struct Registry {
    pending: HashMap<(SessionId, RequestId), Arc<Preparation>>,
    leases: HashMap<ArtifactId, Arc<Lease>>,
    successors: HashMap<ArtifactId, Weak<MediaSuccessorLineage>>,
    connections: Arc<AtomicUsize>,
    #[cfg(test)]
    capture_gate: Option<CaptureGate>,
}
impl Registry {
    pub(super) fn cancel_all(&mut self) {
        for p in self.pending.values() {
            p.cancel.store(true, Ordering::Release);
        }
        for l in self.leases.values() {
            l.cancel.store(true, Ordering::Release);
        }
        self.sweep();
    }
    fn sweep(&mut self) {
        let now = Instant::now();
        for p in self.pending.values() {
            if now >= p.deadline
                || p.successor
                    .as_ref()
                    .is_some_and(|lineage| !lineage.predecessor_current())
            {
                p.cancel.store(true, Ordering::Release);
            }
        }
        self.pending.retain(|_, p| {
            !(p.done.load(Ordering::Acquire) && p.persisted.load(Ordering::Acquire))
        });
        self.leases.retain(|_, l| {
            if now >= l.deadline
                || l.successor.as_ref().is_some_and(|lineage| {
                    !lineage.committed.load(Ordering::Acquire) && !lineage.predecessor_current()
                })
            {
                l.cancel.store(true, Ordering::Release);
            }
            !l.cancel.load(Ordering::Acquire)
        });
        self.successors
            .retain(|_, lineage| lineage.strong_count() > 0);
    }
}
pub(crate) struct SourceMediaConnection {
    pin: SourceMediaPin,
    nonce: String,
    count: Arc<AtomicUsize>,
    _memory: Reservation,
}
impl Drop for SourceMediaConnection {
    fn drop(&mut self) {
        if let Ok(mut connections) = self.pin.0.connections.lock() {
            connections.remove(&self.nonce);
        } else {
            self.pin.0.cancel.store(true, Ordering::Release);
        }
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}
pub(super) fn start_sweeper(engine: &Arc<Engine>) -> Result<()> {
    let weak = Arc::downgrade(engine);
    engine.spawn_task("pulsar-source-media-expiry", Some(256 * 1024), move || {
        loop {
            let Some(engine) = weak.upgrade() else {
                break;
            };
            if engine.is_shutting_down() {
                break;
            }
            engine
                .browser_media
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .sweep();
            engine.sweep_browser_timing();
            drop(engine);
            std::thread::sleep(Duration::from_millis(50));
        }
    })?;
    Ok(())
}
fn load(
    db: &Connection,
    actor: &SessionId,
    original: &RequestId,
) -> PResult<BrowserMediaPreparationReport> {
    let raw: Option<String> = db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=32768 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2", params![actor.as_str(), original.as_str()], |row| row.get(0)).optional().map_err(internal)?;
    let ResponseBody::BrowserMediaPreparation(report) = decode(&raw.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "browser media preparation not found")
    })?)?
    else {
        return Err(mismatch());
    };
    report.validate()?;
    if report.request_id != *original {
        return Err(mismatch());
    }
    Ok(*report)
}
fn save(
    db: &Connection,
    actor: &SessionId,
    fingerprint: &str,
    report: &BrowserMediaPreparationReport,
) -> PResult<()> {
    report.validate()?;
    let body = ResponseBody::BrowserMediaPreparation(Box::new(report.clone()));
    let changed = db
        .execute(
            "UPDATE requests SET response=?4 WHERE session=?1 AND request=?2 AND fingerprint=?3",
            params![
                actor.as_str(),
                report.request_id.as_str(),
                fingerprint,
                encode(&body)?
            ],
        )
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "browser media history write unavailable",
            )
        })?;
    if changed != 1 {
        return Err(mismatch());
    }
    Ok(())
}
fn same_intent(a: &BrowserMediaPreparationReport, b: &BrowserMediaPreparationReport) -> bool {
    a.request_id == b.request_id
        && a.engine_epoch == b.engine_epoch
        && a.project_id == b.project_id
        && a.revision == b.revision
        && a.selection == b.selection
        && a.clock == b.clock
        && a.lease_ms == b.lease_ms
}
fn body(report: BrowserMediaPreparationReport) -> PResult<ResponseBody> {
    report.validate()?;
    Ok(ResponseBody::BrowserMediaPreparation(Box::new(report)))
}
impl Engine {
    fn browser_media_context(
        &self,
        db: &Connection,
        actor: &SessionId,
        auth_hash: &str,
        project: &ProjectId,
        revision: RevisionId,
        selection: &BrowserMediaSelection,
    ) -> PResult<SourceArtifact> {
        if self.is_shutting_down() {
            return Err(unavailable());
        }
        selection.validate()?;
        require_grant(db, actor, project, Scope::Read)?;
        require_grant(db, actor, project, Scope::Export)?;
        let stored: Option<String> = db
            .query_row(
                "SELECT token_hash FROM sessions WHERE id=?1",
                [actor.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(internal)?;
        if stored.as_deref() != Some(auth_hash) {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "browser media session changed",
            ));
        }
        let current: i64 = db
            .query_row(
                "SELECT revision FROM projects WHERE id=?1",
                [project.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if u64::try_from(current).ok() != Some(revision.value()) {
            return Err(mismatch());
        }
        source_kinds::require_media(db, project, &selection.source_version)?;
        if source_placements::active(db, project, &selection.placement.placement_id)?
            != selection.placement
        {
            return Err(mismatch());
        }
        let source = self.source(db, project, &selection.source_version)?;
        source.validate()?;
        if source.identity != selection.source
            || source.identity.byte_len == 0
            || source.identity.byte_len > self.config.max_snapshot_bytes
        {
            return Err(mismatch());
        }
        Ok(source)
    }
    /// Called before a public project mutation releases DB ownership. An old
    /// Arc remains permanently cancelled even if the same grant is later added.
    pub(super) fn retire_changed_browser_media(&self, db: &Connection) {
        let (jobs, leases) = {
            let r = self.browser_media.lock().unwrap_or_else(|e| e.into_inner());
            (
                r.pending.values().cloned().collect::<Vec<_>>(),
                r.leases.values().cloned().collect::<Vec<_>>(),
            )
        };
        for p in jobs {
            let report = p.report.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if self
                .browser_media_context(
                    db,
                    &p.actor,
                    &p.auth_hash,
                    &report.project_id,
                    report.revision,
                    &report.selection,
                )
                .is_err()
            {
                p.cancel.store(true, Ordering::Release);
            }
        }
        for l in leases {
            let b = &l.binding;
            if self
                .browser_media_context(
                    db,
                    &l.actor,
                    &l.auth_hash,
                    &b.project_id,
                    b.revision,
                    &b.selection,
                )
                .is_err()
            {
                l.cancel.store(true, Ordering::Release);
            }
        }
        self.browser_media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sweep();
    }
    #[cfg(test)]
    pub(super) fn pin_browser_media(
        &self,
        r: &Request,
        binding: &BrowserMediaBinding,
    ) -> PResult<SourceMediaPin> {
        let db = self.db.lock().map_err(internal)?;
        self.pin_browser_media_locked(&db, r, binding)
    }
    pub(super) fn pin_browser_media_locked(
        &self,
        db: &Connection,
        r: &Request,
        binding: &BrowserMediaBinding,
    ) -> PResult<SourceMediaPin> {
        binding.validate()?;
        let actor = r.session.as_ref().ok_or_else(unavailable)?;
        self.authenticate(db, actor, r.auth_token.as_deref())?;
        if r.project.as_ref() != Some(&binding.project_id)
            || binding.engine_epoch != epoch(self)?
            || r.expected_revision
                .is_some_and(|revision| revision != binding.revision)
        {
            return Err(mismatch());
        }
        let auth_hash = hash_token(r)?;
        self.browser_media_context(
            db,
            actor,
            &auth_hash,
            &binding.project_id,
            binding.revision,
            &binding.selection,
        )?;
        let mut registry = self.browser_media.lock().map_err(internal)?;
        registry.sweep();
        let lease = registry
            .leases
            .get(&binding.lease_id)
            .filter(|l| l.actor == *actor && l.auth_hash == auth_hash && l.binding == *binding)
            .cloned()
            .ok_or_else(unavailable)?;
        let pin = SourceMediaPin(lease);
        if !pin.current() {
            return Err(unavailable());
        }
        Ok(pin)
    }
    pub(super) fn capture_browser_media_successor_locked(
        &self,
        db: &Connection,
        request: &Request,
        predecessor: &BrowserMediaBinding,
        successor: &BrowserMediaBinding,
    ) -> PResult<NativeBrowserMediaSuccessor> {
        let old = self.pin_browser_media_locked(db, request, predecessor)?;
        let next = self.pin_browser_media_locked(db, request, successor)?;
        NativeBrowserMediaSuccessor::from_pins(old, next)
    }

    fn browser_media_view(
        &self,
        db: &Connection,
        actor: &SessionId,
        r: &Request,
        mut report: BrowserMediaPreparationReport,
    ) -> PResult<ResponseBody> {
        if report.project_id != *project(r)? {
            return Err(mismatch());
        }
        self.retire_changed_browser_media(db);
        let pending = self
            .browser_media
            .lock()
            .map_err(internal)?
            .pending
            .get(&key(actor, &report.request_id))
            .cloned();
        if let Some(p) = pending {
            let native = p.report.lock().map_err(internal)?.clone();
            if !same_intent(&report, &native) {
                return Err(mismatch());
            }
            if p.done.load(Ordering::Acquire) && !p.persisted.load(Ordering::Acquire) {
                if report.state == BrowserMediaPreparationState::Pending {
                    let mut outcome = p
                        .verification
                        .lock()
                        .map_err(internal)?
                        .clone()
                        .unwrap_or(native);
                    outcome.current = false;
                    save(db, actor, &p.fingerprint, &outcome)?;
                    report = outcome;
                }
                p.verification.lock().map_err(internal)?.take();
                p.persisted.store(true, Ordering::Release);
            } else if report.state == BrowserMediaPreparationState::Pending {
                report.verified_bytes = p
                    .progress
                    .load(Ordering::Acquire)
                    .min(report.selection.source.byte_len);
            }
            report.current = report.state == BrowserMediaPreparationState::Pending
                && !p.done.load(Ordering::Acquire)
                && !p.cancel.load(Ordering::Acquire)
                && Instant::now() < p.deadline
                && p.successor
                    .as_ref()
                    .is_none_or(|lineage| lineage.predecessor_current());
        } else {
            report.current = false;
        }
        if let Some(binding) = &report.binding {
            report.current = self.pin_browser_media_locked(db, r, binding).is_ok();
        }
        body(report)
    }
    pub(super) fn browser_media_control(
        self: &Arc<Self>,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        if matches!(r.command, Command::BrowserMediaCatalog) {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_revision(&db, r)?;
            let revision = project_snapshot(&db, project(r)?)?.revision;
            let mut statement = db.prepare("SELECT a.id,s.label FROM source_placement_active a JOIN source_placement_definitions d ON d.project=a.project AND d.id=a.id JOIN sources s ON s.project=d.project AND s.version=d.source WHERE a.project=?1 AND s.evicted=0 AND s.kind='media' ORDER BY a.id LIMIT 257").map_err(internal)?;
            let rows = statement
                .query_map([project(r)?.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(internal)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(internal)?;
            if rows.len() > 256 {
                return Err(capacity());
            }
            let mut entries = Vec::with_capacity(rows.len());
            for (id, label) in rows {
                let placement = source_placements::active(
                    &db,
                    project(r)?,
                    &SourcePlacementId::new(id).map_err(internal)?,
                )?;
                let source = self.source(&db, project(r)?, &placement.spec.source_version)?;
                let selection = BrowserMediaSelection {
                    source_version: source.source_version,
                    source: source.identity,
                    placement,
                };
                selection.validate()?;
                entries.push(BrowserMediaCatalogEntry { label, selection });
            }
            let catalog = BrowserMediaCatalog {
                engine_epoch: epoch(self)?,
                project_id: project(r)?.clone(),
                revision,
                entries,
            };
            catalog.validate()?;
            return Ok(ResponseBody::BrowserMediaCatalog(Box::new(catalog)));
        }
        {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Command::BrowserMediaPreparationStatus {
                original_request_id,
            } = &r.command
            {
                return self.browser_media_view(
                    &db,
                    actor,
                    r,
                    load(&db, actor, original_request_id)?,
                );
            }
            if let Some(old) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::BrowserMediaPreparation(report) = old else {
                    return Err(mismatch());
                };
                return self.browser_media_view(&db, actor, r, *report);
            }
            if let Command::ReleaseBrowserMedia {
                original_request_id,
            } = &r.command
            {
                let mut report = load(&db, actor, original_request_id)?;
                if report.project_id != *project(r)? {
                    return Err(mismatch());
                }
                {
                    let mut registry = self.browser_media.lock().map_err(internal)?;
                    if let Some(p) = registry.pending.get(&key(actor, original_request_id)) {
                        p.cancel.store(true, Ordering::Release);
                    }
                    if let Some(b) = &report.binding {
                        if let Some(l) = registry.leases.get(&b.lease_id) {
                            l.cancel.store(true, Ordering::Release);
                        }
                    }
                    registry.sweep();
                }
                if report.state == BrowserMediaPreparationState::Pending {
                    report.state = BrowserMediaPreparationState::Cancelled;
                    report.binding = None;
                }
                report.current = false;
                let tx = db.transaction().map_err(internal)?;
                tx.execute(
                    "UPDATE requests SET response=?3 WHERE session=?1 AND request=?2",
                    params![
                        actor.as_str(),
                        original_request_id.as_str(),
                        encode(&ResponseBody::BrowserMediaPreparation(Box::new(
                            report.clone()
                        )))?
                    ],
                )
                .map_err(internal)?;
                let reply = body(report)?;
                remember(&tx, actor.as_str(), r, fingerprint, &reply)?;
                tx.commit().map_err(internal)?;
                return Ok(reply);
            }
        }
        self.prepare_browser_media(r, actor, fingerprint)
    }
    fn prepare_browser_media(
        self: &Arc<Self>,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let (engine_epoch, selection, clock, lease_ms, predecessor) = match &r.command {
            Command::PrepareBrowserMedia {
                engine_epoch,
                selection,
                clock,
                lease_ms,
                ..
            } => (engine_epoch, selection, clock, lease_ms, None),
            Command::PrepareBrowserMediaSuccessor {
                predecessor,
                lease_ms,
            } => (
                &predecessor.engine_epoch,
                &predecessor.selection,
                &predecessor.clock,
                lease_ms,
                Some(predecessor),
            ),
            _ => return Err(ProtocolError::unsupported("browser media operation")),
        };
        if engine_epoch != &epoch(self)? {
            return Err(unavailable());
        }
        let auth_hash = hash_token(r)?;
        let (source, holds, memory, parent) = {
            let _gate = self.artifact_gate.lock().map_err(internal)?;
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_revision(&db, r)?;
            let parent = predecessor
                .map(|binding| self.pin_browser_media_locked(&db, r, binding))
                .transpose()?;
            if parent.as_ref().is_some_and(|pin| {
                pin.0.successor_committed.load(Ordering::Acquire)
                    || pin
                        .0
                        .successor
                        .as_ref()
                        .is_some_and(|lineage| !lineage.committed.load(Ordering::Acquire))
            }) {
                return Err(mismatch());
            }
            let source = self.browser_media_context(
                &db,
                actor,
                &auth_hash,
                project(r)?,
                r.expected_revision.ok_or_else(mismatch)?,
                selection,
            )?;
            let holds = self
                .package_holds
                .acquire(std::slice::from_ref(&source.identity))?;
            let memory = self.pool.reserve_memory(
                VerifiedSourceReader::reservation_bytes(source.identity.byte_len)?
                    .checked_add(64 * 1024)
                    .ok_or_else(capacity)?,
            )?;
            (source, holds, memory, parent)
        };
        // No DB/registry or artifact gate during filesystem I/O. The hold already
        // prevents controlled eviction; verification authenticates this exact FD.
        let metadata = self.pool.reserve_memory(64 * 1024)?;
        let file = artifacts::open_regular(&source.path).map_err(|_| unavailable())?;
        let preparation = {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(old) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::BrowserMediaPreparation(report) = old else {
                    return Err(mismatch());
                };
                return self.browser_media_view(&db, actor, r, *report);
            }
            let current = self.browser_media_context(
                &db,
                actor,
                &auth_hash,
                project(r)?,
                r.expected_revision.ok_or_else(mismatch)?,
                selection,
            )?;
            if current.path != source.path || current.identity != source.identity {
                return Err(mismatch());
            }
            if parent.as_ref().is_some_and(|pin| {
                !pin.current() || pin.0.successor_committed.load(Ordering::Acquire)
            }) {
                return Err(unavailable());
            }
            {
                let mut registry = self.browser_media.lock().map_err(internal)?;
                registry.sweep();
                if let Some(parent) = &parent {
                    if registry
                        .successors
                        .get(&parent.0.binding.lease_id)
                        .and_then(Weak::upgrade)
                        .is_some_and(|lineage| !lineage.cancelled.load(Ordering::Acquire))
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "original media already has a pending successor",
                        ));
                    }
                }
                if registry.pending.len() >= MAX_PREPARATIONS || registry.leases.len() >= MAX_LEASES
                {
                    return Err(capacity());
                }
            }
            let report = BrowserMediaPreparationReport {
                request_id: r.request_id.clone(),
                engine_epoch: engine_epoch.clone(),
                project_id: project(r)?.clone(),
                revision: r.expected_revision.ok_or_else(mismatch)?,
                selection: selection.clone(),
                clock: *clock,
                lease_ms: *lease_ms,
                state: BrowserMediaPreparationState::Pending,
                verified_bytes: 0,
                binding: None,
                current: true,
                error: None,
            };
            let tx = db.transaction().map_err(internal)?;
            remember(&tx, actor.as_str(), r, fingerprint, &body(report.clone())?)?;
            tx.commit().map_err(internal)?;
            let cancel = Arc::new(AtomicBool::new(false));
            let successor = parent.as_ref().map(|pin| {
                Arc::new(MediaSuccessorLineage {
                    request_id: r.request_id.clone(),
                    predecessor: Arc::downgrade(&pin.0),
                    committed: AtomicBool::new(false),
                    cancelled: cancel.clone(),
                })
            });
            let p = Arc::new(Preparation {
                owner: Arc::downgrade(self),
                actor: actor.clone(),
                auth_hash,
                fingerprint: fingerprint.to_owned(),
                report: Mutex::new(report),
                cancel,
                progress: AtomicU64::new(0),
                deadline: Instant::now() + VERIFY_LIMIT,
                done: AtomicBool::new(false),
                persisted: AtomicBool::new(false),
                verification: Mutex::new(None),
                successor,
                _metadata: metadata,
            });
            let mut registry = self.browser_media.lock().map_err(internal)?;
            if let (Some(parent), Some(lineage)) = (&parent, &p.successor) {
                registry
                    .successors
                    .insert(parent.0.binding.lease_id.clone(), Arc::downgrade(lineage));
            }
            registry
                .pending
                .insert(key(actor, &r.request_id), p.clone());
            p
        };
        let worker = preparation.clone();
        let weak = Arc::downgrade(self);
        #[cfg(test)]
        let gate = self
            .browser_media
            .lock()
            .map_err(internal)?
            .capture_gate
            .clone();
        let spawned = self.spawn_task("pulsar-source-media-verify", Some(512 * 1024), move || {
            // Unwind the file, retention holds and reader reservation before
            // recording failure. Then resume the original panic so the owned
            // task group's join retains its diagnostic rather than reporting a
            // successful worker after recovery of its native admission state.
            let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate(&worker.cancel, worker.deadline);
                }
                let identity = worker
                    .report
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .selection
                    .source
                    .clone();
                let verified = VerifiedSourceReader::capture(
                    file,
                    &identity,
                    &worker.cancel,
                    worker.deadline,
                    &worker.progress,
                );
                if let Some(engine) = weak.upgrade() {
                    engine.finish_browser_media(&worker, verified, holds, memory);
                } else {
                    worker.cancel.store(true, Ordering::Release);
                    worker.done.store(true, Ordering::Release);
                }
            }));
            if let Err(panic) = execution {
                worker.cancel.store(true, Ordering::Release);
                if !worker.done.load(Ordering::Acquire) {
                    worker
                        .verification
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take();
                    worker.verification.clear_poison();
                    if let Some(engine) = weak.upgrade() {
                        engine.finish_browser_media_failure(&worker, false);
                        worker.report.clear_poison();
                    } else {
                        worker.done.store(true, Ordering::Release);
                    }
                }
                std::panic::resume_unwind(panic);
            }
        });
        if spawned.is_err() {
            self.finish_browser_media_failure(&preparation, false);
        }
        let report = preparation.report.lock().map_err(internal)?.clone();
        body(report)
    }
    fn finish_browser_media_failure(&self, p: &Preparation, cancelled: bool) {
        let mut report = p.report.lock().unwrap_or_else(|e| e.into_inner()).clone();
        report.state = if cancelled {
            BrowserMediaPreparationState::Cancelled
        } else {
            BrowserMediaPreparationState::Failed
        };
        report.verified_bytes = p
            .progress
            .load(Ordering::Acquire)
            .min(report.selection.source.byte_len);
        report.current = false;
        report.binding = None;
        report.error = (!cancelled).then(|| BROWSER_MEDIA_PREPARATION_ERROR.to_owned());
        *p.report.lock().unwrap_or_else(|e| e.into_inner()) = report.clone();
        p.done.store(true, Ordering::Release);
        // Metadata repair is retained in native custody if SQLite is busy. Status
        // may publish this exact terminal report, never reconstruct a live lease.
        if let Ok(db) = self.db.try_lock() {
            if let Ok(old) = load(&db, &p.actor, &report.request_id) {
                if !same_intent(&old, &report) {
                    return;
                }
                if old.state != BrowserMediaPreparationState::Pending {
                    *p.report.lock().unwrap_or_else(|e| e.into_inner()) = old;
                    p.persisted.store(true, Ordering::Release);
                } else if save(&db, &p.actor, &p.fingerprint, &report).is_ok() {
                    p.persisted.store(true, Ordering::Release);
                }
            }
        }
    }
    fn finish_browser_media(
        &self,
        p: &Preparation,
        verified: PResult<VerifiedSourceReader>,
        holds: ArtifactHolds,
        memory: Reservation,
    ) {
        let Ok(reader) = verified else {
            self.finish_browser_media_failure(p, p.cancel.load(Ordering::Acquire));
            return;
        };
        let mut db = loop {
            match self.db.try_lock() {
                Ok(db) => break db,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    self.finish_browser_media_failure(p, false);
                    return;
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    if p.cancel.load(Ordering::Acquire)
                        || self.is_shutting_down()
                        || Instant::now() >= p.deadline
                    {
                        self.finish_browser_media_failure(
                            p,
                            p.cancel.load(Ordering::Acquire) || self.is_shutting_down(),
                        );
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        let original = p.report.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let publish = (|| -> PResult<()> {
            let old = load(&db, &p.actor, &original.request_id)?;
            if !same_intent(&old, &original) {
                return Err(mismatch());
            }
            if old.state != BrowserMediaPreparationState::Pending {
                *p.report.lock().map_err(internal)? = old;
                p.persisted.store(true, Ordering::Release);
                p.done.store(true, Ordering::Release);
                return Ok(());
            }
            if p.cancel.load(Ordering::Acquire)
                || Instant::now() >= p.deadline
                || p.successor
                    .as_ref()
                    .is_some_and(|lineage| !lineage.predecessor_current())
            {
                return Err(unavailable());
            }
            self.browser_media_context(
                &db,
                &p.actor,
                &p.auth_hash,
                &original.project_id,
                original.revision,
                &original.selection,
            )?;
            {
                let mut registry = self.browser_media.lock().map_err(internal)?;
                registry.sweep();
                if registry.leases.len() >= MAX_LEASES {
                    return Err(capacity());
                }
            }
            let binding = BrowserMediaBinding {
                preparation_request_id: original.request_id.clone(),
                lease_id: lease_id(),
                engine_epoch: original.engine_epoch.clone(),
                project_id: original.project_id.clone(),
                revision: original.revision,
                selection: original.selection.clone(),
                clock: original.clock,
                lease_ms: original.lease_ms,
            };
            binding.validate()?;
            let deadline = Instant::now() + Duration::from_millis(u64::from(binding.lease_ms));
            let lease = Arc::new(Lease {
                owner: p.owner.clone(),
                actor: p.actor.clone(),
                auth_hash: p.auth_hash.clone(),
                binding: binding.clone(),
                reader,
                cancel: p.cancel.clone(),
                deadline,
                connections: Mutex::new(HashSet::new()),
                successor: p.successor.clone(),
                successor_committed: AtomicBool::new(false),
                _holds: holds,
                _memory: memory,
            });
            let mut report = original.clone();
            report.state = BrowserMediaPreparationState::Ready;
            report.verified_bytes = report.selection.source.byte_len;
            report.binding = Some(binding.clone());
            report.current = true;
            report.error = None;
            // Preserve only bounded verification metadata if the durable Ready
            // publication fails. A later status repair is historical-only.
            *p.verification.lock().map_err(internal)? = Some(report.clone());
            let tx = db.transaction().map_err(internal)?;
            save(&tx, &p.actor, &p.fingerprint, &report)?;
            tx.commit().map_err(internal)?;
            let mut registry = self.browser_media.lock().map_err(internal)?;
            report.current = !self.is_shutting_down()
                && !p.cancel.load(Ordering::Acquire)
                && Instant::now() < deadline
                && Instant::now() < p.deadline
                && p.successor
                    .as_ref()
                    .is_none_or(|lineage| lineage.predecessor_current());
            if report.current {
                registry.leases.insert(binding.lease_id, lease);
            } else {
                p.cancel.store(true, Ordering::Release);
            }
            *p.report.lock().map_err(internal)? = report;
            p.verification.lock().map_err(internal)?.take();
            p.persisted.store(true, Ordering::Release);
            p.done.store(true, Ordering::Release);
            registry.sweep();
            Ok(())
        })();
        drop(db);
        if publish.is_err() {
            if p.verification
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some()
            {
                // No authority survives an unknown Ready publication. Reader,
                // source holds and their reservation have already been dropped.
                p.cancel.store(true, Ordering::Release);
                let mut report = p.report.lock().unwrap_or_else(|e| e.into_inner());
                report.current = false;
                report.verified_bytes = p
                    .progress
                    .load(Ordering::Acquire)
                    .min(report.selection.source.byte_len);
                p.done.store(true, Ordering::Release);
            } else {
                self.finish_browser_media_failure(
                    p,
                    p.cancel.load(Ordering::Acquire) || self.is_shutting_down(),
                );
            }
        }
    }
    fn source_media_db_until<'a>(
        &'a self,
        pin: &SourceMediaPin,
        deadline: Instant,
    ) -> PResult<std::sync::MutexGuard<'a, Connection>> {
        loop {
            if !pin.current() || self.is_shutting_down() || Instant::now() >= deadline {
                return Err(unavailable());
            }
            match self.db.try_lock() {
                Ok(db) => return Ok(db),
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(unavailable()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
            }
        }
    }
    fn check_source_media_connection(
        &self,
        connection: &SourceMediaConnection,
        handshake: &SourceMediaBulkHandshake,
        deadline: Instant,
    ) -> PResult<()> {
        handshake.validate()?;
        if !connection.pin.current() {
            return Err(unavailable());
        }
        let l = &connection.pin.0;
        let b = &l.binding;
        if handshake.session != l.actor
            || handshake.lease_id != b.lease_id
            || handshake.engine_epoch != b.engine_epoch
            || handshake.preparation_request_id != b.preparation_request_id
            || handshake.source_sha256 != b.selection.source.sha256
            || !l
                .connections
                .lock()
                .map_err(internal)?
                .contains(&connection.nonce)
        {
            return Err(mismatch());
        }
        let db = self.source_media_db_until(&connection.pin, deadline)?;
        self.authenticate(&db, &handshake.session, Some(&handshake.auth_token))?;
        if let Err(error) = self.browser_media_context(
            &db,
            &l.actor,
            &l.auth_hash,
            &b.project_id,
            b.revision,
            &b.selection,
        ) {
            l.cancel.store(true, Ordering::Release);
            return Err(error);
        }
        if !connection.pin.current() || Instant::now() >= deadline {
            return Err(unavailable());
        }
        Ok(())
    }
    pub(crate) fn source_media_bulk_open(
        &self,
        handshake: &SourceMediaBulkHandshake,
    ) -> PResult<(SourceMediaConnection, BrowserMediaBinding)> {
        handshake.validate()?;
        let (pin, count) = {
            let mut registry = self.browser_media.lock().map_err(internal)?;
            registry.sweep();
            let lease = registry
                .leases
                .get(&handshake.lease_id)
                .cloned()
                .ok_or_else(unavailable)?;
            (SourceMediaPin(lease), registry.connections.clone())
        };
        let memory = self.pool.reserve_memory(reader::BLOCK_BYTES as u64)?;
        let nonce = Uuid::new_v4().to_string();
        {
            let mut connections = pin.0.connections.lock().map_err(internal)?;
            if connections.len() >= 2 {
                return Err(capacity());
            }
            let mut current = count.load(Ordering::Acquire);
            loop {
                let next = current
                    .checked_add(1)
                    .filter(|n| *n <= MAX_CONNECTIONS)
                    .ok_or_else(capacity)?;
                match count.compare_exchange_weak(
                    current,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(observed) => current = observed,
                }
            }
            connections.insert(nonce.clone());
        }
        let connection = SourceMediaConnection {
            pin,
            nonce,
            count,
            _memory: memory,
        };
        self.check_source_media_connection(
            &connection,
            handshake,
            Instant::now() + CHUNK_DEADLINE,
        )?;
        let binding = connection.pin.0.binding.clone();
        Ok((connection, binding))
    }
    pub(crate) fn source_media_bulk_read(
        &self,
        connection: &mut SourceMediaConnection,
        handshake: &SourceMediaBulkHandshake,
        offset: u64,
        byte_len: u32,
    ) -> PResult<Vec<u8>> {
        SourceMediaBulkRequest { offset, byte_len }
            .validate(connection.pin.0.binding.selection.source.byte_len)?;
        let deadline = connection
            .pin
            .0
            .deadline
            .min(Instant::now() + CHUNK_DEADLINE);
        self.check_source_media_connection(connection, handshake, deadline)?;
        let lease = &connection.pin.0;
        let bytes = lease
            .reader
            .read_range(offset, byte_len, &lease.cancel, deadline);
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                lease.cancel.store(true, Ordering::Release);
                return Err(error);
            }
        };
        self.check_source_media_connection(connection, handshake, deadline)?;
        Ok(bytes)
    }
    #[cfg(test)]
    pub(super) fn set_browser_media_capture_gate_for_test(&self, gate: CaptureGate) {
        self.browser_media.lock().unwrap().capture_gate = Some(gate);
    }
}
