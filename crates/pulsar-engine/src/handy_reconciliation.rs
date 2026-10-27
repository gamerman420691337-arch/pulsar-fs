//! Qualified, explicitly consented custody recovery. Historical DTOs cannot
//! reconstruct the native drain/sampling proof or enqueue a replacement worker.
use super::*;
use crate::device_effect_journal::{CustodyResolutionRecord, CustodySnapshot};
use crate::handy_credentials::Credentials;
use std::sync::{RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::time::{Duration, Instant};

const PLAN_LIMIT: usize = 16;
const PLAN_TTL: Duration = Duration::from_secs(300);
const PLAN_MEMORY: u64 = 64 * 1024 * 1024;
const WORK_BUDGET: Duration = Duration::from_secs(60);
const RESPONSE_LIMIT: usize = 128 * 1024;
type Inspector =
    Arc<dyn Fn(Arc<Credentials>, Instant) -> PResult<HandySetupDeviceObservation> + Send + Sync>;

struct CapturedPlan {
    owner: SessionId,
    report: HandyRecoveryPlan,
    basis: CustodySnapshot,
    credentials: Arc<Credentials>,
    accepted: HandyQualificationReport,
    expires: Instant,
    _memory: Reservation,
}

struct Live {
    cancel: AtomicBool,
    finished: AtomicBool,
}

#[derive(Default)]
pub(super) struct Registry {
    plans: HashMap<(SessionId, ArtifactId), Arc<CapturedPlan>>,
    workers: HashMap<(SessionId, RequestId), Arc<Live>>,
}
impl Registry {
    fn prune(&mut self) {
        let now = Instant::now();
        self.plans.retain(|_, entry| now < entry.expires);
        self.workers
            .retain(|_, worker| !worker.finished.load(Ordering::Acquire));
    }
    pub(super) fn cancel_all(&mut self) {
        for worker in self.workers.values() {
            worker.cancel.store(true, Ordering::Release);
        }
        self.plans.clear();
    }
    #[cfg(test)]
    pub(super) fn worker_cancelled(&self, actor: &SessionId, request: &RequestId) -> Option<bool> {
        self.workers
            .get(&(actor.clone(), request.clone()))
            .map(|live| live.cancel.load(Ordering::Acquire))
    }
}

/// This sealed, non-deserializable value exists only after the native worker
/// has drained the exact target, sampled it, and rechecked current acceptance.
pub(super) struct NativeRecoveryProof {
    device: DeviceId,
    route: String,
    namespace: String,
    _evidence: HandyRecoveryEvidence,
}
impl NativeRecoveryProof {
    pub(super) fn matches_target(&self, device: &DeviceId, route: &str, namespace: &str) -> bool {
        &self.device == device && self.route == route && self.namespace == namespace
    }
}

struct WorkerLifetime {
    engine: Arc<Engine>,
    key: (SessionId, RequestId),
    live: Arc<Live>,
}
impl Drop for WorkerLifetime {
    fn drop(&mut self) {
        self.live.finished.store(true, Ordering::Release);
        let mut registry = self
            .engine
            .handy_reconciliations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if registry
            .workers
            .get(&self.key)
            .is_some_and(|live| Arc::ptr_eq(live, &self.live))
        {
            registry.workers.remove(&self.key);
        }
    }
}

fn unavailable(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}
fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::PlanHandyReconciliation { .. }
            | Command::ReconcileHandyEffects { .. }
            | Command::HandyReconciliationStatus { .. }
            | Command::CancelHandyReconciliation { .. }
    )
}
fn needs_admission(command: &Command) -> bool {
    // Renewals and timing refreshes must remain available to unrelated active
    // controllers. Their final registry checks refuse retired controllers and
    // removed synchronization contexts; none can rearm a retired target or
    // start new playback.
    matches!(
        command,
        Command::SetupHandyDevice { .. }
            | Command::InspectHandySetup { .. }
            | Command::InspectHandyProgram { .. }
            | Command::PrepareHandyPlayback { .. }
            | Command::PrepareHandyHsp { .. }
            | Command::CancelHandyHspPreparation { .. }
            | Command::AcquireHandyController { .. }
            | Command::SynchronizeHandyClock { .. }
            | Command::StartHandyPlayback { .. }
            | Command::StartHandyPlaybackFromBrowser { .. }
            | Command::CancelHandySetup { .. }
            | Command::CancelHandyClockSync { .. }
            | Command::CancelHandyPlayback { .. }
    )
}
fn check_cancel(engine: &Engine, live: &Live, deadline: Instant) -> PResult<()> {
    if live.cancel.load(Ordering::Acquire) || engine.is_shutting_down() {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "Handy reconciliation was cancelled or interrupted",
        ));
    }
    if Instant::now() >= deadline {
        return Err(unavailable("Handy reconciliation deadline expired"));
    }
    Ok(())
}
fn bounded_lock<'a, T>(
    engine: &Engine,
    lock: &'a Mutex<T>,
    live: &Live,
    deadline: Instant,
) -> PResult<std::sync::MutexGuard<'a, T>> {
    loop {
        check_cancel(engine, live, deadline)?;
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(internal("reconciliation reference lock poisoned"))
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}
fn cleanup_lock<T>(lock: &Mutex<T>, deadline: Instant) -> PResult<std::sync::MutexGuard<'_, T>> {
    loop {
        if Instant::now() >= deadline {
            return Err(unavailable("reconciliation cleanup lock deadline expired"));
        }
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(internal("reconciliation cleanup reference is poisoned"))
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}
fn scope(
    db: &Connection,
    actor: &SessionId,
    plan: &HandyRecoveryPlan,
    export: bool,
) -> PResult<()> {
    for project in std::iter::once(&plan.project_id).chain(plan.projects.iter()) {
        require_grant(db, actor, project, Scope::Read)?;
        if export {
            require_grant(db, actor, project, Scope::Export)?;
        }
    }
    Ok(())
}
pub(super) fn historical_subjects(
    db: &Connection,
    snapshot: &CustodySnapshot,
    accepted: &HandyQualificationSubject,
) -> PResult<()> {
    snapshot
        .validate_transport(&accepted.transport()?)
        .map_err(internal)?;
    for binding in snapshot.historical_qualifications().map_err(internal)? {
        let previous = handy_qualification::historical_recovery_subject(db, &binding)?;
        if previous.identity != accepted.identity
            || previous.operative_settings.is_none()
            || previous.operative_settings != accepted.operative_settings
        {
            return Err(changed(
                "original physical subject differs from accepted recovery identity or settings",
            ));
        }
    }
    for preparation in &snapshot.hsp_preparations {
        let record = &preparation.record;
        let previous = handy_qualification::historical_recovery_subject(db, &record.qualification)?;
        if previous.transport()? != HandyQualificationTransport::Hsp
            || previous.sha256()? != record.subject_sha256
            || previous.hsp_profile_sha256()? != record.profile_sha256
            || previous.identity != record.device
            || previous.operative_settings.as_ref() != Some(&record.settings().map_err(internal)?)
        {
            return Err(changed(
                "original HSP preparation subject or profile differs",
            ));
        }
    }
    for stream in &snapshot.hsp_streams {
        let link = stream.qualification_link.as_ref().ok_or_else(|| {
            changed("historical HSP stream lacks its original qualification link")
        })?;
        let previous = handy_qualification::historical_recovery_subject(db, &link.qualification)?;
        if previous.transport()? != HandyQualificationTransport::Hsp
            || previous.sha256()? != link.subject_sha256
            || previous.hsp_profile_sha256()? != link.profile_sha256
        {
            return Err(changed(
                "original HSP qualification subject or profile differs",
            ));
        }
    }
    Ok(())
}
fn pending(plan: &HandyRecoveryPlan, request: &RequestId) -> HandyRecoveryStatus {
    HandyRecoveryStatus {
        project_id: plan.project_id.clone(),
        reconciliation_request_id: request.clone(),
        plan: plan.clone(),
        state: HandyRecoveryState::Pending,
        resolution: None,
        error: None,
        playback_enabled: false,
        authority_restored: false,
    }
}
fn completed(id: String, record: &CustodyResolutionRecord) -> PResult<HandyRecoveryStatus> {
    let mut status = pending(&record.plan, &record.request_id);
    status.state = HandyRecoveryState::Completed;
    status.resolution = Some(HandyRecoveryResolution {
        record_id: ArtifactId::new(id).map_err(internal)?,
        basis_snapshot_id: ArtifactId::new(record.basis.id().map_err(internal)?)
            .map_err(internal)?,
        snapshot_id: ArtifactId::new(record.snapshot.id().map_err(internal)?).map_err(internal)?,
        counts: record.snapshot.counts(),
        evidence: record.evidence.clone(),
    });
    status.validate()?;
    Ok(status)
}

fn load(
    db: &Connection,
    actor: &SessionId,
    request: &RequestId,
) -> PResult<Option<(String, HandyRecoveryStatus)>> {
    let row: Option<(String,String)> = db.query_row(
        "SELECT CASE WHEN length(fingerprint)=64 THEN fingerprint ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=131072 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
        params![actor.as_str(),request.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(internal)?;
    row.map(|(fingerprint, raw)| {
        validate_sha256(&fingerprint)?;
        let ResponseBody::HandyRecovery(status) = decode(&raw)? else {
            return Err(changed("original request is not a Handy reconciliation"));
        };
        status.validate()?;
        Ok((fingerprint, *status))
    })
    .transpose()
}
fn save_in_transaction(
    tx: &Transaction<'_>,
    actor: &SessionId,
    request: &RequestId,
    fingerprint: &str,
    status: &HandyRecoveryStatus,
) -> PResult<()> {
    status.validate()?;
    validate_sha256(fingerprint)?;
    let body = ResponseBody::HandyRecovery(Box::new(status.clone()));
    let raw = encode(&body)?;
    if raw.len() > RESPONSE_LIMIT {
        return Err(internal("Handy reconciliation witness exceeds limit"));
    }
    let identity = Request::new(
        request.clone(),
        Command::HandyReconciliationStatus {
            reconciliation_request_id: status.reconciliation_request_id.clone(),
            owner_proof: "metadata-only".into(),
        },
    );
    check_request_identity(tx, actor.as_str(), &identity, fingerprint)?;
    if let Some((old, previous)) = load(tx, actor, request)? {
        if old != fingerprint
            || previous.project_id != status.project_id
            || previous.reconciliation_request_id != status.reconciliation_request_id
            || previous.plan.plan_id != status.plan.plan_id
        {
            return Err(changed("original reconciliation witness changed identity"));
        }
        if encode(&previous)? == encode(status)? {
            return Ok(());
        }
        tx.execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
            params![raw, actor.as_str(), request.as_str()],
        )
        .map_err(internal)?;
        Ok(())
    } else {
        remember(tx, actor.as_str(), &identity, fingerprint, &body)
    }
}
fn save(
    db: &mut Connection,
    actor: &SessionId,
    request: &RequestId,
    fingerprint: &str,
    status: &HandyRecoveryStatus,
) -> PResult<()> {
    let tx = db.transaction().map_err(internal)?;
    save_in_transaction(&tx, actor, request, fingerprint, status)?;
    tx.commit().map_err(internal)
}

/// Existing completed witnesses must correspond to the exact read-only journal
/// record before any database migration or replacement journal can begin.
pub(super) fn validate_journal_links(db: &Connection, state_dir: &Path) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut query = db.prepare("SELECT DISTINCT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=131072 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_recovery' ELSE response LIKE '%handy_recovery%' END LIMIT 4097")?;
    let rows = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut count = 0usize;
    for row in rows {
        count += 1;
        anyhow::ensure!(count <= 4096, "reconciliation witness capacity exceeded");
        let (actor, raw) = row?;
        let ResponseBody::HandyRecovery(status) = serde_json::from_str(&raw)? else {
            anyhow::bail!("invalid recovery witness");
        };
        status.validate()?;
        if let Some(resolution) = &status.resolution {
            let actor = SessionId::new(actor)?;
            let record = crate::device_effect_journal::DeviceEffectJournal::verify_resolution(
                state_dir,
                &actor,
                &status.reconciliation_request_id,
                resolution.record_id.as_str(),
            )?;
            anyhow::ensure!(
                serde_json::to_vec(&completed(resolution.record_id.as_str().into(), &record)?)?
                    == serde_json::to_vec(&*status)?,
                "recovery witness differs from durable resolution"
            );
            if let Some((fingerprint, _)) = load(db, &actor, &status.reconciliation_request_id)? {
                anyhow::ensure!(
                    fingerprint == record.request_fingerprint,
                    "original recovery fingerprint differs"
                );
            }
        }
    }
    Ok(())
}

impl Engine {
    pub(super) fn handy_reconciliation_admission(
        &self,
        command: &Command,
    ) -> PResult<Option<RwLockReadGuard<'_, ()>>> {
        if !needs_admission(command) {
            return Ok(None);
        }
        self.handy_reconciliation_admission
            .try_read()
            .map(Some)
            .map_err(|_| unavailable("Handy admission is reserved for reconciliation"))
    }
    fn recovery_write_guard(
        &self,
        live: &Live,
        deadline: Instant,
    ) -> PResult<RwLockWriteGuard<'_, ()>> {
        loop {
            check_cancel(self, live, deadline)?;
            match self.handy_reconciliation_admission.try_write() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(internal("Handy admission lock poisoned"))
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
    fn recovery_references(
        &self,
        db: &Connection,
        actor: &SessionId,
        selection: &HandyRecoverySelection,
    ) -> PResult<(
        HandyRegistration,
        Arc<Credentials>,
        HandyQualificationReport,
        crate::HandyRouteIdentity,
    )> {
        selection.validate()?;
        if selection.engine_epoch.as_str() != self.engine_epoch {
            return Err(changed(
                "recovery selection belongs to a retired Engine epoch",
            ));
        }
        let registration = handy_registrations::current(
            db,
            &selection.device_id,
            selection.registration_revision,
        )?;
        if registration.disabled {
            return Err(changed("recovery registration is disabled"));
        }
        let (connection, credentials) = self
            .handy_sessions
            .lock()
            .map_err(internal)?
            .capture(actor, &selection.device_session)?;
        if connection.engine_epoch != selection.engine_epoch
            || connection.registration != registration
        {
            return Err(changed("recovery connection or registration changed"));
        }
        let accepted = handy_qualification::accepted_recovery_record_for_selected_transport(
            db,
            &selection.qualification,
            &registration,
        )?;
        if connection
            .inspection
            .as_ref()
            .and_then(|inspection| inspection.identity.as_ref())
            != Some(&accepted.manifest.subject.identity)
        {
            return Err(changed(
                "retained connection identity differs from accepted recovery subject",
            ));
        }
        let route = crate::HandyRouteIdentity::from_snapshot(
            &self.handy_owner_token,
            &credentials.connection,
        )?;
        Ok((registration, credentials, accepted, route))
    }
    fn recovery_plan(
        &self,
        request: &Request,
        actor: &SessionId,
        selection: &HandyRecoverySelection,
    ) -> PResult<ResponseBody> {
        let memory = self.pool.reserve_memory(PLAN_MEMORY)?;
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        let (registration, credentials, accepted, route) =
            self.recovery_references(&db, actor, selection)?;
        let basis = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .custody_snapshot(
                &selection.device_id,
                route.route_sha256(),
                route.namespace_sha256(),
            )
            .map_err(internal)?;
        historical_subjects(&db, &basis, &accepted.manifest.subject)?;
        let mut report = HandyRecoveryPlan {
            plan_id: ArtifactId::new("0".repeat(64)).map_err(internal)?,
            snapshot_id: ArtifactId::new(basis.id().map_err(internal)?).map_err(internal)?,
            project_id: project(request)?.clone(),
            selection: selection.clone(),
            registration,
            route_sha256: route.route_sha256().into(),
            route_namespace_sha256: route.namespace_sha256().into(),
            counts: basis.counts(),
            projects: basis.projects(),
            devices: basis.devices(),
            contract: accepted
                .manifest
                .subject
                .recovery
                .clone()
                .ok_or_else(|| changed("recovery contract is absent"))?,
            playback_enabled: false,
        };
        report.plan_id = report.identity()?;
        report.validate()?;
        scope(&db, actor, &report, true)?;
        let mut registry = self.handy_reconciliations.lock().map_err(internal)?;
        registry.prune();
        let key = (actor.clone(), report.plan_id.clone());
        if !registry.plans.contains_key(&key) && registry.plans.len() >= PLAN_LIMIT {
            return Err(unavailable("Handy reconciliation plan capacity reached"));
        }
        registry.plans.insert(
            key,
            Arc::new(CapturedPlan {
                owner: actor.clone(),
                report: report.clone(),
                basis,
                credentials,
                accepted,
                expires: Instant::now() + PLAN_TTL,
                _memory: memory,
            }),
        );
        Ok(ResponseBody::HandyRecoveryPlan(Box::new(report)))
    }
    fn recovery_history(
        &self,
        db: &mut Connection,
        actor: &SessionId,
        project_id: &ProjectId,
        original: &RequestId,
    ) -> PResult<Option<(String, HandyRecoveryStatus)>> {
        let saved = load(db, actor, original)?;
        if saved.as_ref().is_some_and(|(_, s)| {
            s.project_id != *project_id || s.reconciliation_request_id != *original
        }) {
            return Err(changed("reconciliation history subject differs"));
        }
        let committed = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .resolution_history(actor, original)
            .map_err(internal)?;
        if let Some((id, record)) = committed {
            if record.plan.project_id != *project_id
                || saved.as_ref().is_some_and(|(f, s)| {
                    *f != record.request_fingerprint || s.plan.plan_id != record.plan.plan_id
                })
            {
                return Err(changed("durable reconciliation subject differs"));
            }
            let status = completed(id, &record)?;
            scope(db, actor, &status.plan, false)?;
            save(db, actor, original, &record.request_fingerprint, &status)?;
            return Ok(Some((record.request_fingerprint, status)));
        }
        let Some((fingerprint, mut status)) = saved else {
            return Ok(None);
        };
        scope(db, actor, &status.plan, false)?;
        if status.resolution.is_some() {
            return Err(internal(
                "committed reconciliation journal record is missing",
            ));
        }
        if status.state == HandyRecoveryState::Pending {
            let live = self
                .handy_reconciliations
                .lock()
                .map_err(internal)?
                .workers
                .get(&(actor.clone(), original.clone()))
                .cloned();
            if live.is_none_or(|live| live.finished.load(Ordering::Acquire)) {
                status.state = HandyRecoveryState::Interrupted;
                status.error = Some(unavailable(
                    "original reconciliation worker is absent; history cannot restart it",
                ));
                save(db, actor, original, &fingerprint, &status)?;
            }
        }
        Ok(Some((fingerprint, status)))
    }
    pub(super) fn handy_reconciliation_control(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_reconciliation_with(
            request,
            actor,
            fingerprint,
            Arc::new(crate::handy_online::inspect_setup),
        )
    }
    pub(super) fn handy_reconciliation_with(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        inspector: Inspector,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        if let Command::PlanHandyReconciliation { selection, .. } = &request.command {
            return self.recovery_plan(request, actor, selection);
        }
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, actor)?;
        check_request_identity(&db, actor.as_str(), request, fingerprint)?;
        if let Command::HandyReconciliationStatus {
            reconciliation_request_id,
            ..
        } = &request.command
        {
            let (_, status) = self
                .recovery_history(&mut db, actor, project(request)?, reconciliation_request_id)?
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, "original reconciliation not found")
                })?;
            return Ok(ResponseBody::HandyRecovery(Box::new(status)));
        }
        if let Command::CancelHandyReconciliation {
            reconciliation_request_id,
            ..
        } = &request.command
        {
            // Check this cancellation's fingerprint before touching the original.
            if let Some((old, receipt)) = load(&db, actor, &request.request_id)? {
                if old != fingerprint
                    || receipt.reconciliation_request_id != *reconciliation_request_id
                    || receipt.project_id != *project(request)?
                {
                    return Err(changed("cancellation identity changed"));
                }
            }
            let (original_fingerprint, mut status) = self
                .recovery_history(&mut db, actor, project(request)?, reconciliation_request_id)?
                .ok_or_else(|| {
                    ProtocolError::new(ErrorCode::NotFound, "original reconciliation not found")
                })?;
            let live = if status.state == HandyRecoveryState::Pending {
                let live = self
                    .handy_reconciliations
                    .lock()
                    .map_err(internal)?
                    .workers
                    .get(&(actor.clone(), reconciliation_request_id.clone()))
                    .cloned();
                status.state = HandyRecoveryState::Cancelled;
                status.error = None;
                live
            } else {
                None
            };
            // Completed always wins once its independent journal commit exists.
            // Original state and cancellation identity/receipt commit together;
            // a failed receipt cannot cancel work under an unreserved identity.
            let tx = db.transaction().map_err(internal)?;
            save_in_transaction(
                &tx,
                actor,
                reconciliation_request_id,
                &original_fingerprint,
                &status,
            )?;
            save_in_transaction(&tx, actor, &request.request_id, fingerprint, &status)?;
            tx.commit().map_err(internal)?;
            if let Some(live) = live {
                live.cancel.store(true, Ordering::Release);
            }
            return Ok(ResponseBody::HandyRecovery(Box::new(status)));
        }
        let Command::ReconcileHandyEffects { plan_id, .. } = &request.command else {
            return Err(ProtocolError::invalid(
                "expected Handy reconciliation command",
            ));
        };
        if let Some((old, status)) =
            self.recovery_history(&mut db, actor, project(request)?, &request.request_id)?
        {
            if old != fingerprint || status.plan.plan_id != *plan_id {
                return Err(changed("original reconciliation request selection differs"));
            }
            return Ok(ResponseBody::HandyRecovery(Box::new(status)));
        }
        let captured = {
            let mut registry = self.handy_reconciliations.lock().map_err(internal)?;
            registry.prune();
            if registry.workers.len() >= PLAN_LIMIT {
                return Err(unavailable("Handy reconciliation worker capacity reached"));
            }
            let captured = registry
                .plans
                .get(&(actor.clone(), plan_id.clone()))
                .cloned()
                .ok_or_else(|| {
                    changed("reconciliation plan expired or is not owned by this session")
                })?;
            if captured.report.project_id != *project(request)? {
                return Err(changed("reconciliation plan belongs to another project"));
            }
            captured
        };
        scope(&db, actor, &captured.report, true)?;
        save(
            &mut db,
            actor,
            &request.request_id,
            fingerprint,
            &pending(&captured.report, &request.request_id),
        )?;
        let live = Arc::new(Live {
            cancel: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        // The main DB guard prevents status from observing the reservation
        // between commit and worker registration. No SQLite write holds this registry.
        self.handy_reconciliations
            .lock()
            .map_err(internal)?
            .workers
            .insert((actor.clone(), request.request_id.clone()), live.clone());
        let original = pending(&captured.report, &request.request_id);
        drop(db);
        let engine = self.clone();
        let owned_request = request.clone();
        let owner = actor.clone();
        let fingerprint = fingerprint.to_owned();
        let worker_live = live.clone();
        let spawn = self.spawn_task("pulsar-handy-recovery", Some(512 * 1024), move || {
            let _lifetime = WorkerLifetime {
                engine: engine.clone(),
                key: (owner.clone(), owned_request.request_id.clone()),
                live: worker_live.clone(),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.perform_recovery(
                    &owned_request,
                    &owner,
                    &fingerprint,
                    &captured,
                    &worker_live,
                    &inspector,
                )
            }));
            let failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(_) => Some(internal("Handy reconciliation worker panicked")),
            };
            if let Some(error) = failure {
                engine.finish_recovery_failure(
                    &owned_request,
                    &owner,
                    &fingerprint,
                    &worker_live,
                    error,
                );
            }
        });
        if let Err(error) = spawn {
            live.finished.store(true, Ordering::Release);
            self.handy_reconciliations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .workers
                .remove(&(actor.clone(), request.request_id.clone()));
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(request_fingerprint(request).map_err(internal)?)
            );
            self.finish_recovery_failure(
                request,
                actor,
                &fingerprint,
                &live,
                unavailable("owned reconciliation worker could not start"),
            );
            let mut db = self.db.lock().map_err(internal)?;
            let (_, status) = self
                .recovery_history(&mut db, actor, project(request)?, &request.request_id)?
                .ok_or_else(|| internal(error))?;
            return Ok(ResponseBody::HandyRecovery(Box::new(status)));
        }
        Ok(ResponseBody::HandyRecovery(Box::new(original)))
    }

    fn recheck_recovery(
        &self,
        db: &Connection,
        request: &Request,
        actor: &SessionId,
        captured: &CapturedPlan,
    ) -> PResult<()> {
        self.authorize(db, request, actor)?;
        scope(db, actor, &captured.report, true)?;
        let (registration, credentials, accepted, route) =
            self.recovery_references(db, actor, &captured.report.selection)?;
        if actor != &captured.owner
            || registration != captured.report.registration
            || !Arc::ptr_eq(&credentials, &captured.credentials)
            || route.route_sha256() != captured.report.route_sha256
            || route.namespace_sha256() != captured.report.route_namespace_sha256
            || encode(&accepted)? != encode(&captured.accepted)?
        {
            return Err(changed("captured reconciliation references changed"));
        }
        historical_subjects(db, &captured.basis, &accepted.manifest.subject)?;
        Ok(())
    }
    fn wait_recovery_clock(&self, target: u64, live: &Live, deadline: Instant) -> PResult<()> {
        loop {
            check_cancel(self, live, deadline)?;
            let now = crate::handy_clock::boottime_ns()?;
            if now >= target {
                return Ok(());
            }
            std::thread::sleep(Duration::from_nanos(target - now).min(Duration::from_millis(2)));
        }
    }
    fn recovery_sample(
        &self,
        captured: &CapturedPlan,
        live: &Live,
        deadline: Instant,
        inspector: &Inspector,
    ) -> PResult<HandyRecoverySample> {
        check_cancel(self, live, deadline)?;
        let started_boottime_ns = crate::handy_clock::boottime_ns()?;
        let sample_deadline = deadline.min(
            Instant::now() + Duration::from_nanos(captured.report.contract.maximum_sample_span_ns),
        );
        let observation = inspector(captured.credentials.clone(), sample_deadline)?;
        let finished_boottime_ns = crate::handy_clock::boottime_ns()?;
        check_cancel(self, live, deadline)?;
        observation.validate()?;
        let subject = &captured.accepted.manifest.subject;
        if observation.identity != subject.identity
            || subject.operative_settings.as_ref() != Some(&observation.operative_settings)
        {
            return Err(changed(
                "fresh recovery sample differs from accepted identity or settings",
            ));
        }
        Ok(HandyRecoverySample {
            started_boottime_ns,
            finished_boottime_ns,
            observation,
        })
    }
    fn perform_recovery(
        self: &Arc<Self>,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        captured: &CapturedPlan,
        live: &Live,
        inspector: &Inspector,
    ) -> PResult<()> {
        let deadline = (Instant::now() + WORK_BUDGET).min(captured.expires);
        let _admission = self.recovery_write_guard(live, deadline)?;
        {
            let db = bounded_lock(self, &self.db, live, deadline)?;
            self.recheck_recovery(&db, request, actor, captured)?;
            let journal = bounded_lock(self, &self._device_effect_journal, live, deadline)?;
            let current = journal
                .custody_snapshot(
                    &captured.report.selection.device_id,
                    &captured.report.route_sha256,
                    &captured.report.route_namespace_sha256,
                )
                .map_err(internal)?;
            if current.id().map_err(internal)? != captured.report.snapshot_id.as_str() {
                return Err(changed("custody changed before reconciliation drain"));
            }
        }
        let retirement = self.retire_handy_for_reconciliation(
            &captured.report.selection.device_id,
            &captured.credentials,
        )?;
        let ids = retirement.ids();
        let execution = {
            let slot = bounded_lock(self, &self.handy_execution, live, deadline)?;
            slot.as_ref()
                .map(|pump| pump.reconciliation_handle())
                .transpose()?
        };
        // The handle owns only the bounded mailbox, stop flag and wake handle.
        // Unrelated handlers can inspect the execution owner while drain waits.
        if let Some(pump) = &execution {
            pump.drain_for_reconciliation(ids.clone(), deadline, &live.cancel)?;
        } else if !ids.is_empty() {
            return Err(unavailable("targeted native execution owner is absent"));
        }
        let mut preparation_drained =
            if captured.report.contract.transport == HandyQualificationTransport::Hsp {
                Some(self.drain_handy_hsp_preparations(&captured.basis, deadline, &live.cancel)?)
            } else {
                None
            };
        let hsp_execution =
            if captured.report.contract.transport == HandyQualificationTransport::Hsp {
                Some(self.ensure_handy_hsp_execution()?)
            } else {
                None
            };
        let hsp_drained = if let Some(pump) = &hsp_execution {
            let targets = pump
                .capture_recovery_targets(
                    &captured.report.selection.device_id,
                    &captured.report.route_sha256,
                    &captured.report.route_namespace_sha256,
                )
                .map_err(|_| unavailable("HSP recovery targets are unavailable"))?;
            if targets
                .stream_ids()
                .iter()
                .any(|id| !captured.basis.hsp_stream_ids().contains(&id.as_str()))
            {
                return Err(changed(
                    "native HSP target was not present in the captured plan",
                ));
            }
            Some(
                pump.drain_recovery(targets, deadline, &live.cancel)
                    .map_err(|_| unavailable("exact HSP native drain is incomplete"))?,
            )
        } else {
            None
        };
        if let Some(drained) = &mut preparation_drained {
            self.seal_handy_hsp_preparation_drain(drained, deadline, &live.cancel)?;
        }
        let drained_boottime_ns = crate::handy_clock::boottime_ns()?;
        let sample_deadline =
            deadline.min(Instant::now() + Duration::from_nanos(HANDY_RECOVERY_MAX_WINDOW_NS));
        self.wait_recovery_clock(
            drained_boottime_ns
                .checked_add(captured.report.contract.prior_command_settle_ns)
                .ok_or_else(|| internal("recovery clock overflow"))?,
            live,
            sample_deadline,
        )?;
        let first = self.recovery_sample(captured, live, sample_deadline, inspector)?;
        self.wait_recovery_clock(
            first
                .finished_boottime_ns
                .checked_add(captured.report.contract.stopped_sample_separation_ns)
                .ok_or_else(|| internal("recovery clock overflow"))?,
            live,
            sample_deadline,
        )?;
        let second = self.recovery_sample(captured, live, sample_deadline, inspector)?;
        let evidence = HandyRecoveryEvidence {
            drained_boottime_ns,
            first,
            second,
        };
        evidence.validate(&captured.report.contract)?;
        let proof;
        let hsp_resolution;
        {
            let mut db = bounded_lock(self, &self.db, live, deadline)?;
            self.recheck_recovery(&db, request, actor, captured)?;
            check_request_identity(&db, actor.as_str(), request, fingerprint)?;
            let (original_fingerprint, original) = load(&db, actor, &request.request_id)?
                .ok_or_else(|| internal("original recovery reservation disappeared"))?;
            if original_fingerprint != fingerprint
                || original.plan.plan_id != captured.report.plan_id
                || original.state != HandyRecoveryState::Pending
            {
                return Err(changed("original reconciliation is no longer pending"));
            }
            check_cancel(self, live, deadline)?;
            let mut journal = bounded_lock(self, &self._device_effect_journal, live, deadline)?;
            let snapshot = journal
                .custody_snapshot(
                    &captured.report.selection.device_id,
                    &captured.report.route_sha256,
                    &captured.report.route_namespace_sha256,
                )
                .map_err(internal)?;
            let added = captured
                .basis
                .added_stop_records(&snapshot)
                .map_err(internal)?;
            if added.iter().any(|effect| !retirement.permits_stop(effect)) {
                return Err(changed(
                    "custody extension is not from the exact retired native targets",
                ));
            }
            historical_subjects(&db, &snapshot, &captured.accepted.manifest.subject)?;
            if let Some(drained) = &hsp_drained {
                if drained
                    .memberships()
                    .iter()
                    .any(|sealed| !snapshot.hsp_streams.iter().any(|current| current == sealed))
                {
                    return Err(changed(
                        "HSP history differs from the exact joined native drain",
                    ));
                }
            }
            if let Some(drained) = &preparation_drained {
                if drained.memberships().iter().any(|sealed| {
                    !snapshot
                        .hsp_preparations
                        .iter()
                        .any(|current| current == sealed)
                }) {
                    return Err(changed(
                        "HSP preparation history differs from exact native drain",
                    ));
                }
            }
            for project_id in snapshot.projects() {
                require_grant(&db, actor, &project_id, Scope::Read)?;
                require_grant(&db, actor, &project_id, Scope::Export)?;
            }
            proof = NativeRecoveryProof {
                device: captured.report.selection.device_id.clone(),
                route: captured.report.route_sha256.clone(),
                namespace: captured.report.route_namespace_sha256.clone(),
                _evidence: evidence.clone(),
            };
            retirement.check_proof(&proof)?;
            let record = CustodyResolutionRecord {
                actor_session: actor.clone(),
                request_id: request.request_id.clone(),
                request_fingerprint: fingerprint.into(),
                plan: captured.report.clone(),
                basis: captured.basis.clone(),
                snapshot,
                evidence,
            };
            // Cancellation takes this same DB guard before setting its flag or
            // original outcome. A durable journal commit wins every later race.
            check_cancel(self, live, deadline)?;
            let id = journal.record_resolution(&record).map_err(internal)?;
            hsp_resolution = if hsp_drained.is_some() {
                Some(
                    journal
                        .hsp_resolution_receipt(&id, &record.snapshot.hsp_streams)
                        .map_err(internal)?,
                )
            } else {
                None
            };
            drop(journal);
            let status = completed(id, &record)?;
            // If this write fails, status/retry repairs only metadata from the
            // committed journal. It can never reconstruct this native proof.
            if save(&mut db, actor, &request.request_id, fingerprint, &status).is_err() {
                eprintln!(
                    "Handy reconciliation committed; original status requires metadata repair"
                );
            }
        }
        // After commit, cancellation cannot skip conservative owner release.
        // Failure retains old native custody; the historical result stays Completed.
        let cleanup_cancel = AtomicBool::new(false);
        let cleanup_deadline = Instant::now() + Duration::from_secs(5);
        let cleanup = if let Some(pump) = &execution {
            pump.finish_reconciliation(ids, cleanup_deadline, &cleanup_cancel)
        } else {
            Ok(())
        };
        let preparation_cleanup = match (preparation_drained, hsp_resolution.as_ref()) {
            (Some(drained), Some(receipt)) => {
                self.finish_handy_hsp_preparation_recovery(drained, receipt)
            }
            (None, None) => Ok(()),
            _ => Err(internal("HSP preparation recovery receipt is absent")),
        };
        let hsp_cleanup = match (&hsp_execution, hsp_drained, hsp_resolution) {
            (Some(pump), Some(drained), Some(receipt)) => pump
                .finish_recovery(drained, &receipt, cleanup_deadline, &cleanup_cancel)
                .map_err(|_| unavailable("committed HSP recovery retains native custody")),
            (None, None, None) => Ok(()),
            _ => Err(internal(
                "HSP recovery proof or committed receipt is absent",
            )),
        };
        if cleanup.is_ok() && hsp_cleanup.is_ok() && preparation_cleanup.is_ok() {
            if self
                .finish_handy_reconciliation(retirement, &proof)
                .is_err()
            {
                eprintln!(
                    "Handy reconciliation committed; exact native retirement remains conservative"
                );
            }
        } else {
            eprintln!(
                "Handy reconciliation committed; selected execution cleanup remains conservative"
            );
        }
        Ok(())
    }
    fn finish_recovery_failure(
        &self,
        request: &Request,
        actor: &SessionId,
        fingerprint: &str,
        live: &Live,
        error: ProtocolError,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let Ok(mut db) = cleanup_lock(&self.db, deadline) else {
            return;
        };
        // A resolution might have committed immediately before a later failure.
        let Ok(journal) = cleanup_lock(&self._device_effect_journal, deadline) else {
            return;
        };
        let committed = journal.resolution_history(actor, &request.request_id);
        drop(journal);
        if committed.is_err() {
            return;
        }
        if let Ok(Some((id, record))) = committed {
            if record.request_fingerprint == fingerprint {
                if let Ok(status) = completed(id, &record) {
                    let _ = save(&mut db, actor, &request.request_id, fingerprint, &status);
                }
            }
            return;
        }
        let Ok(Some((old, mut status))) = load(&db, actor, &request.request_id) else {
            return;
        };
        if old != fingerprint || status.state != HandyRecoveryState::Pending {
            return;
        }
        if self.is_shutting_down() {
            status.state = HandyRecoveryState::Interrupted;
            status.error = Some(unavailable(
                "Engine shutdown interrupted original reconciliation",
            ));
        } else if live.cancel.load(Ordering::Acquire) {
            status.state = HandyRecoveryState::Cancelled;
            status.error = None;
        } else {
            status.state = HandyRecoveryState::Failed;
            status.error = Some(error);
        }
        let _ = save(&mut db, actor, &request.request_id, fingerprint, &status);
    }
}
