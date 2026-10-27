//! Authenticated, bounded native preparation operations. Reports never grant launch authority.
use super::live_cuda_preparation::PreparedLiveCudaImage;
use super::*;
use crate::native_preparation_db as journal;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use NativeImagePreparationState as State;
const THREAD_MEMORY: u64 = 512 * 1024;
const ACTIVE_LIMIT: usize = 8;
#[derive(Default)]
pub(super) struct Registry {
    pub(super) entries: HashMap<String, Entry>,
    monitoring: bool,
}
pub(super) struct Entry {
    pub(super) request: Request,
    pub(super) origin: Instant,
    pub(super) deadline: Instant,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) result: Arc<Mutex<Option<PResult<PreparedLiveCudaImage>>>>,
    pub(super) producer: Option<JoinHandle<()>>,
    pub(super) memory: Option<Arc<Reservation>>,
    pub(super) ready: Option<PResult<PreparedLiveCudaImage>>,
    pub(super) retained: Option<PreparedLiveCudaImage>,
}
fn elapsed(origin: Instant) -> u64 {
    origin.elapsed().as_millis().min((i64::MAX - 1) as u128) as u64
}
fn next(
    report: &NativeImagePreparationReport,
    state: State,
    time: u64,
) -> NativeImagePreparationReport {
    let mut value = report.clone();
    value.state = state;
    value.generation += 1;
    value.elapsed_ms = time.max(report.elapsed_ms);
    value
}
fn failed(
    report: &NativeImagePreparationReport,
    time: u64,
    message: &str,
) -> NativeImagePreparationReport {
    let mut value = next(report, State::Failed, time);
    value.error = Some(
        message
            .chars()
            .filter(|c| !c.is_control())
            .take(768)
            .collect(),
    );
    value
}
fn status(
    report: NativeImagePreparationReport,
    entry: Option<&Entry>,
) -> NativeImagePreparationStatus {
    let observed_elapsed_ms = entry.map(|e| elapsed(e.origin).max(report.elapsed_ms));
    NativeImagePreparationStatus {
        available: report.state == State::Retained
            && observed_elapsed_ms.is_some_and(|now| {
                report
                    .retained_at_elapsed_ms
                    .is_some_and(|at| now.saturating_sub(at) < report.intent.options.retention_ms)
            })
            && entry.is_some_and(|e| {
                e.retained
                    .as_ref()
                    .is_some_and(|image| image.retained_now())
            }),
        producer_active: entry.is_some_and(|e| e.producer.is_some()),
        observed_elapsed_ms,
        report,
    }
}
impl Engine {
    pub(super) fn start_preparation_monitor(
        self: &Arc<Self>,
        registry: &mut Registry,
    ) -> PResult<()> {
        if registry.monitoring {
            return Ok(());
        }
        let memory = self.pool.reserve_memory(THREAD_MEMORY)?;
        let weak = Arc::downgrade(self);
        registry.monitoring = true;
        if let Err(error) =
            self.spawn_task("pulsar-preparation-monitor", Some(256 * 1024), move || {
                let _memory = memory;
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let Some(engine) = weak.upgrade() else {
                        return;
                    };
                    if engine.is_shutting_down() {
                        return;
                    }
                    // Errors preserve custody; a later poll retries the journal transition.
                    let _ = engine.poll_native_preparations();
                    let Ok(mut registry) = engine.native_preparations.lock() else {
                        return;
                    };
                    if registry.entries.is_empty() {
                        registry.monitoring = false;
                        return;
                    }
                }
            })
        {
            registry.monitoring = false;
            return Err(unavailable(error));
        }
        Ok(())
    }
    pub(super) fn native_preparation_command(
        self: &Arc<Self>,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        // No controller locks are taken while the dispatch authorization DB lock is held.
        self.poll_native_preparations()?;
        let mut registry = self.native_preparations.lock().map_err(internal)?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        check_revision(&db, request)?;
        check_request_identity(&db, session.as_str(), request, fingerprint)?;
        if request.command.is_mutating() {
            if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                return Ok(body);
            }
        }
        if matches!(request.command, Command::NativeImagePreparationLimits) {
            return Ok(ResponseBody::NativeImagePreparationLimits(Box::new(
                NativeImagePreparationLimits {
                    engine_epoch: AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
                    supported: cfg!(feature = "cuda") && self.cuda_owner_policy.is_some(),
                    preparation_max_ms: self
                        .config
                        .native_image_preparation_timeout
                        .as_millis()
                        .min(3_600_000) as u64,
                    retention_max_ms: 3_600_000,
                    maximum_active: ACTIVE_LIMIT as u16,
                    maximum_records: 4096,
                },
            )));
        }
        if let Command::StartNativeImagePreparation {
            engine_epoch,
            options,
        } = &request.command
        {
            if engine_epoch.as_str() != self.engine_epoch {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "native preparation engine epoch changed",
                ));
            }
            options.validate()?;
            if !cfg!(feature = "cuda") {
                return Err(ProtocolError::unsupported(
                    "native preparation requires CUDA",
                ));
            }
            let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
                ProtocolError::unsupported("native preparation requires owner policy")
            })?;
            if Duration::from_millis(options.preparation_ms)
                > self.config.native_image_preparation_timeout
            {
                return Err(ProtocolError::invalid(
                    "native preparation exceeds owner duration limit",
                ));
            }
            if registry.entries.len() >= ACTIVE_LIMIT {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "native preparation active capacity exhausted",
                ));
            }
            let origin = Instant::now();
            let deadline = origin
                .checked_add(Duration::from_millis(options.preparation_ms))
                .ok_or_else(|| ProtocolError::invalid("native preparation deadline overflow"))?;
            let memory = Arc::new(self.pool.reserve_memory(THREAD_MEMORY)?);
            self.start_preparation_monitor(&mut registry)?;
            let policy = owner.status();
            let intent = NativeImagePreparationIntent {
                operation_id: AttemptId::new(Uuid::new_v4().to_string()).map_err(internal)?,
                engine_epoch: engine_epoch.clone(),
                owner: session.clone(),
                project: project(request)?.clone(),
                revision: request
                    .expected_revision
                    .ok_or_else(|| ProtocolError::invalid("revision required"))?
                    .value(),
                original_request_id: request.request_id.clone(),
                options: *options,
                image_sha256: policy.native_dependency_set_sha256,
                owner_policy_sha256: policy.source_identity.sha256,
                declared_bytes: policy.native_declared_bytes,
            };
            let tx = db.transaction().map_err(internal)?;
            self.arm_worker_fence(&tx)?;
            let queued = journal::begin(&tx, intent, fingerprint).map_err(internal)?;
            let body =
                ResponseBody::NativeImagePreparation(Box::new(NativeImagePreparationStatus {
                    report: queued.clone(),
                    available: false,
                    producer_active: true,
                    observed_elapsed_ms: Some(0),
                }));
            remember(&tx, session.as_str(), request, fingerprint, &body)?;
            journal::advance(&tx, &next(&queued, State::Preparing, elapsed(origin)))
                .map_err(internal)?;
            tx.commit().map_err(internal)?;
            drop(db);
            let result = Arc::new(Mutex::new(None));
            let cancel = Arc::new(AtomicBool::new(false));
            let engine = self.clone();
            let output = result.clone();
            let cancelled = cancel.clone();
            let held_memory = memory.clone();
            let options = *options;
            let producer = std::thread::Builder::new()
                .name("pulsar-native-preparation".into())
                .stack_size(256 * 1024)
                .spawn(move || {
                    let _memory = held_memory;
                    let value =
                        engine.prepare_live_cuda_image_in_lease(options, &cancelled, deadline);
                    *output.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
                });
            let (producer, ready) = match producer {
                Ok(thread) => (Some(thread), None),
                Err(error) => (None, Some(Err(unavailable(error)))),
            };
            registry.entries.insert(
                queued.intent.operation_id.as_str().to_owned(),
                Entry {
                    request: request.clone(),
                    origin,
                    deadline,
                    cancel,
                    result,
                    producer,
                    memory: Some(memory),
                    ready,
                    retained: None,
                },
            );
            return Ok(body);
        }
        let report = match &request.command {
            Command::ResolveNativeImagePreparation {
                original_request_id,
            } => journal::resolve(&db, session, original_request_id).map_err(internal)?,
            Command::NativeImagePreparationStatus { operation_id }
            | Command::CancelNativeImagePreparation { operation_id }
            | Command::ReleaseNativeImagePreparation { operation_id } => {
                journal::load(&db, operation_id.as_str()).map_err(internal)?
            }
            _ => {
                return Err(ProtocolError::unsupported(
                    "unknown native preparation command",
                ))
            }
        }
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, "native preparation operation absent")
        })?;
        if report.intent.owner != *session || report.intent.project != *project(request)? {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "native preparation belongs to another owner or project",
            ));
        }
        let operation = report.intent.operation_id.as_str();
        let entry = registry.entries.get_mut(operation);
        let desired = match request.command {
            Command::CancelNativeImagePreparation { .. }
                if matches!(report.state, State::Queued | State::Preparing) =>
            {
                Some(State::CancelRequested)
            }
            Command::CancelNativeImagePreparation { .. } if report.state == State::Retained => {
                return Err(ProtocolError::invalid("release a retained native image"))
            }
            Command::ReleaseNativeImagePreparation { .. } if report.state == State::Retained => {
                Some(State::Releasing)
            }
            Command::ReleaseNativeImagePreparation { .. }
                if matches!(
                    report.state,
                    State::Queued | State::Preparing | State::CancelRequested
                ) =>
            {
                return Err(ProtocolError::invalid(
                    "cancel an active native preparation",
                ))
            }
            _ => None,
        };
        let mut report = report;
        let tx = db.transaction().map_err(internal)?;
        if let Some(state) = desired {
            let entry = entry.as_ref().ok_or_else(|| {
                ProtocolError::new(ErrorCode::Unavailable, "native preparation runtime absent")
            })?;
            report = next(&report, state, elapsed(entry.origin));
            journal::advance(&tx, &report).map_err(internal)?;
        }
        let body = ResponseBody::NativeImagePreparation(Box::new(status(report, entry.as_deref())));
        if request.command.is_mutating() {
            remember(&tx, session.as_str(), request, fingerprint, &body)?;
        } else {
            claim_request_identity(&tx, session.as_str(), request, fingerprint)?;
        }
        tx.commit().map_err(internal)?;
        if matches!(
            request.command,
            Command::CancelNativeImagePreparation { .. }
        ) {
            if let Some(entry) = entry.as_ref() {
                entry.cancel.store(true, Ordering::Release);
            }
        }
        Ok(body)
    }
    fn poll_native_preparations(&self) -> PResult<()> {
        let mut discarded = Vec::new();
        let mut registry = self.native_preparations.lock().map_err(internal)?;
        let mut removable = Vec::new();
        for (operation, entry) in &mut registry.entries {
            if entry.producer.as_ref().is_some_and(|p| p.is_finished()) {
                let thread = entry.producer.take().expect("finished producer");
                entry.ready = Some(match thread.join() {
                    Ok(()) => entry
                        .result
                        .lock()
                        .map_err(internal)?
                        .take()
                        .unwrap_or_else(|| {
                            Err(unavailable(
                                "native preparation producer returned no result",
                            ))
                        }),
                    Err(_) => Err(unavailable("native preparation producer panicked")),
                });
                entry.memory.take();
            }
            let retained_available = entry
                .retained
                .as_ref()
                .is_some_and(|image| image.available());
            let ready_info = entry
                .ready
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .map(|image| (image.available(), image.retention_started()));
            let mut db = self.db.lock().map_err(internal)?;
            let report = journal::load(&db, operation)
                .map_err(internal)?
                .ok_or_else(|| internal("native preparation runtime journal missing"))?;
            let time = elapsed(entry.origin);
            let authority = self
                .authorize(
                    &db,
                    &entry.request,
                    entry
                        .request
                        .session
                        .as_ref()
                        .expect("authenticated preparation"),
                )
                .and_then(|()| check_revision(&db, &entry.request));
            let mut update = None;
            if !report.state.is_terminal() && authority.is_err() {
                update = Some(failed(
                    &report,
                    time,
                    "native preparation authorization or project revision changed",
                ));
            } else if report.state == State::Preparing && Instant::now() >= entry.deadline {
                update = Some(failed(&report, time, "native preparation deadline expired"));
            } else {
                match report.state {
                    State::Preparing => {
                        if let Some(result) = &entry.ready {
                            update = Some(match result {
                                Err(error) => failed(&report, time, &error.to_string()),
                                Ok(image)
                                    if image.engine_epoch() != &report.intent.engine_epoch
                                        || image.image_sha256() != report.intent.image_sha256
                                        || image.declared_bytes()
                                            != report.intent.declared_bytes =>
                                {
                                    failed(
                                        &report,
                                        time,
                                        "native preparation completion identity differs",
                                    )
                                }
                                Ok(_) => {
                                    let (available, started) = ready_info
                                        .expect("successful preparation has retention clock");
                                    let at =
                                        started.saturating_duration_since(entry.origin).as_millis()
                                            as u64;
                                    if !available
                                        || at >= report.intent.options.preparation_ms
                                        || time.saturating_sub(at)
                                            >= report.intent.options.retention_ms
                                    {
                                        failed(&report,time,"native preparation retention expired before publication")
                                    } else {
                                        let mut retained = next(&report, State::Retained, time);
                                        retained.retained_at_elapsed_ms = Some(at);
                                        retained
                                    }
                                }
                            });
                        }
                    }
                    State::CancelRequested if entry.producer.is_none() => {
                        update = Some(next(&report, State::Cancelled, time))
                    }
                    State::Retained if !retained_available => {
                        update = Some(next(&report, State::Expired, time))
                    }
                    State::Releasing => update = Some(next(&report, State::Released, time)),
                    _ => {}
                }
            }
            let current = if let Some(value) = update {
                let tx = db.transaction().map_err(internal)?;
                journal::advance(&tx, &value).map_err(internal)?;
                tx.commit().map_err(internal)?;
                value
            } else {
                report
            };
            drop(db);
            if current.state == State::Retained && entry.retained.is_none() {
                if let Some(Ok(image)) = entry.ready.take() {
                    entry.retained = Some(image);
                }
            }
            if current.state != State::Preparing && current.state != State::Retained {
                entry.cancel.store(true, Ordering::Release);
                if let Some(image) = entry.retained.take() {
                    discarded.push(image);
                }
                if let Some(Ok(image)) = entry.ready.take() {
                    discarded.push(image);
                }
            }
            if current.state.is_terminal()
                && entry.producer.is_none()
                && entry.retained.is_none()
                && discarded.is_empty()
                && self.pool.reserved_storage() == 0
                && !self
                    .worker_fence
                    .has_pending_storage_custody()
                    .map_err(internal)?
            {
                if !current.cleanup_confirmed {
                    let mut receipt = next(&current, current.state, time);
                    receipt.cleanup_confirmed = true;
                    let mut db = self.db.lock().map_err(internal)?;
                    let tx = db.transaction().map_err(internal)?;
                    journal::advance(&tx, &receipt).map_err(internal)?;
                    tx.commit().map_err(internal)?;
                }
                removable.push(operation.clone());
            }
        }
        for operation in removable {
            registry.entries.remove(&operation);
        }
        drop(registry);
        drop(discarded);
        Ok(())
    }
}
