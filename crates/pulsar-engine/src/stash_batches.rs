//! Engine-owned, restartable batch phase ledger. No bearer/API credential is
//! stored. A scoped in-memory permit authorizes only the exact internal request
//! on its executing thread; ordinary project grants remain mandatory.
use super::*;
use std::{
    collections::{HashMap, HashSet},
    thread::{self, ThreadId},
    time::Duration,
};
#[path = "stash_batch_annotations.rs"]
mod annotations;
#[path = "stash_batch_source_plans.rs"]
mod plans;

#[derive(Default)]
pub(super) struct Runtime {
    active: HashSet<(String, String)>,
    permits: HashMap<ThreadId, Permit>,
}
struct Permit {
    session: SessionId,
    batch: RequestId,
    request: RequestId,
    fingerprint: Vec<u8>,
    model_sha256: Option<String>,
}
struct PermitGuard<'a>(&'a Engine, ThreadId);
impl Drop for PermitGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut runtime) = self.0.stash_batch_runtime.lock() {
            runtime.permits.remove(&self.1);
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    phase: String,
    id: RequestId,
    command: Command,
    project: Option<ProjectId>,
    revision: Option<RevisionId>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    placement: Option<SourcePlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation_plan: Option<StashBatchSourcePlan>,
    view: StashBatchItem,
    revision: RevisionId,
    cursor: u8,
    pending: Option<Pending>,
    capture: Option<StashSourceCapture>,
    server_path: Option<String>,
    polls: u32,
}
#[derive(Clone, Serialize, Deserialize)]
struct Migration {
    previous_options: StashBatchOptions,
    previous_owner_capability_sha256: String,
    superseded: Vec<(usize, Pending)>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    migration_history: Vec<Migration>,
    options: StashBatchOptions,
    worker_stamp: String,
    owner_capability_sha256: String,
    report: StashBatchReport,
    items: Vec<Item>,
}

impl Engine {
    pub(super) fn stash_batch_permitted(
        &self,
        db: &Connection,
        request: &Request,
        session: &SessionId,
    ) -> PResult<bool> {
        if request.auth_token.is_some() {
            return Ok(false);
        }
        let runtime = self.stash_batch_runtime.lock().map_err(internal)?;
        let Some(permit) = runtime.permits.get(&thread::current().id()) else {
            return Ok(false);
        };
        if &permit.session != session
            || permit.request != request.request_id
            || permit.fingerprint != request_fingerprint(request).map_err(internal)?
        {
            return Ok(false);
        }
        let state: Option<String> = db
            .query_row(
                "SELECT state FROM stash_batches WHERE session=?1 AND request=?2",
                params![session.as_str(), permit.batch.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(internal)?;
        Ok(state.as_deref() == Some("running")
            || (state.as_deref() == Some("cancelled")
                && matches!(request.command, Command::CancelJob { .. })))
    }
    /// A captured model locator is issued by snapshot_reserved and carries the
    /// original content digest. Re-capturing it cannot authorize different bytes.
    fn batch_model_sha256(&self, path: &Path) -> PResult<String> {
        let sha = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "batch model snapshot identity missing",
                )
            })?;
        if path.parent() != Some(self.config.state_dir.join("snapshots").as_path())
            || sha.len() != 64
            || !sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"batch model is not its original captured content locator; explicit migration is required"));
        }
        Ok(sha.to_owned())
    }
    /// Compare the prepared immutable artifact under the exact scoped permit,
    /// after copying and before authoritative job/attempt publication.
    pub(super) fn check_stash_batch_model(
        &self,
        request: &Request,
        model: Option<&SourceArtifact>,
    ) -> PResult<()> {
        let runtime = self.stash_batch_runtime.lock().map_err(internal)?;
        let Some(permit) = runtime.permits.get(&thread::current().id()) else {
            return Ok(());
        };
        if request.auth_token.is_some()
            || request.session.as_ref() != Some(&permit.session)
            || request.request_id != permit.request
            || request_fingerprint(request).map_err(internal)? != permit.fingerprint
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "prepared model does not belong to the exact batch permit",
            ));
        }
        if permit.model_sha256.as_deref() != model.map(|model| model.identity.sha256.as_str()) {
            return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"batch model bytes differ from original capture; restore matching bytes or explicitly migrate before generation"));
        }
        Ok(())
    }
    fn batch_call(
        self: &Arc<Self>,
        owner: &SessionId,
        batch: &RequestId,
        pending: &Pending,
    ) -> PResult<ResponseBody> {
        let mut command = pending.command.clone();
        match &mut command {
            Command::StashImportSource { owner_proof, .. }
            | Command::StashExportScript { owner_proof, .. }
            | Command::StashSubmitScan { owner_proof, .. }
            | Command::StashScanStatus { owner_proof, .. }
            | Command::StashAddTag { owner_proof, .. }
            | Command::StashTagStatus { owner_proof, .. }
            | Command::StashResumeTag { owner_proof, .. } => {
                *owner_proof = self.stash_owner_token.clone()
            }
            _ => {}
        }
        let model_sha256 = match &command {
            Command::Generate {
                model_path: Some(path),
                ..
            } => Some(self.batch_model_sha256(path)?),
            Command::GenerateFromModelPack {
                settings: Some(settings),
                ..
            } => Some(
                settings
                    .inference_profile
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("batch pack profile is absent"))?
                    .model
                    .sha256
                    .clone(),
            ),
            _ => None,
        };
        let mut request = Request::new(pending.id.clone(), command).with_session(owner.clone());
        request.project = pending.project.clone();
        request.expected_revision = pending.revision;
        let thread = thread::current().id();
        {
            let mut runtime = self.stash_batch_runtime.lock().map_err(internal)?;
            if runtime.permits.contains_key(&thread) {
                return Err(ProtocolError::new(
                    ErrorCode::Internal,
                    "nested batch permit refused",
                ));
            }
            runtime.permits.insert(
                thread,
                Permit {
                    session: owner.clone(),
                    batch: batch.clone(),
                    request: pending.id.clone(),
                    fingerprint: request_fingerprint(&request).map_err(internal)?,
                    model_sha256,
                },
            );
        }
        let _guard = PermitGuard(self, thread);
        self.dispatch(&request)
    }
    pub(super) fn stash_batch_control(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let _metadata = self.pool.reserve_memory(64 * 1024 * 1024)?;
        let owner = request.session.as_ref().expect("authenticated");
        match &request.command {
            Command::StashStartBatch { options, .. } => {
                let _admission = self.pool.reserve_memory(16 * 1024 * 1024)?;
                options.validate()?;
                let mut options = options.clone();
                if options.annotated_views.is_empty() && options.source_plans.is_empty() {
                    if let Some(choice) = &options.model_pack {
                        let _gate = self.artifact_gate.lock().map_err(internal)?;
                        options.settings = self
                            .prepare_selected_stash_pack(
                                choice,
                                options.settings.clone(),
                                &options.annotated_views,
                            )?
                            .settings;
                    }
                }
                if options.model_pack.is_none() || options.settings.model_input.is_some() {
                    options.settings.validate_execution_preset(
                        &options.preset,
                        options.model_path.is_some() || options.model_pack.is_some(),
                    )?;
                }
                self.admit_stash_generation(&options.settings, &options.annotated_views)?;
                let discovery = crate::stash::discover(&options.connection, options.limit)?;
                let plan = crate::stash_files::plan(discovery, &options.mappings)?;
                let items = plan
                    .files
                    .into_iter()
                    .map(|file| {
                        let ready = file.status == StashFileStatus::ReadyForSnapshot;
                        let error = (!ready
                            && !matches!(
                                file.status,
                                StashFileStatus::ExistingScript
                                    | StashFileStatus::AlreadyInteractive
                            ))
                        .then(|| format!("scene is ineligible: {:?}", file.status));
                        let server_path =
                            plan.discovery
                                .scenes
                                .iter()
                                .find(|scene| scene.id == file.scene_id)
                                .and_then(|scene| {
                                    scene.files.iter().find(|selected| {
                                        Some(&selected.id) == file.file_id.as_ref()
                                    })
                                })
                                .map(|selected| selected.path.clone());
                        Item {
                            placement: None,
                            generation_plan: None,
                            view: StashBatchItem {
                                scene_id: file.scene_id,
                                file_id: file.file_id,
                                state: if ready { "selected" } else { "skipped" }.into(),
                                eligibility: file.status,
                                project_id: None,
                                source_version: None,
                                job_id: None,
                                candidate_id: None,
                                requests: vec![],
                                export: None,
                                scan: None,
                                tag: None,
                                error,
                            },
                            revision: RevisionId::new(0),
                            cursor: 0,
                            pending: None,
                            capture: None,
                            server_path,
                            polls: 0,
                        }
                    })
                    .collect::<Vec<_>>();
                annotations::validate_selection(&options, &items)?;
                plans::validate_selection(&options, &items)?;
                if !options.annotated_views.is_empty() || !options.source_plans.is_empty() {
                    if let Some(choice) = &options.model_pack {
                        let _gate = self.artifact_gate.lock().map_err(internal)?;
                        options.settings = self
                            .prepare_selected_stash_pack(
                                choice,
                                options.settings.clone(),
                                &options.annotated_views,
                            )?
                            .settings;
                    }
                }
                options.settings.validate_execution_preset(
                    &options.preset,
                    options.model_path.is_some() || options.model_pack.is_some(),
                )?;
                self.admit_stash_generation(&options.settings, &options.annotated_views)?;
                if let Some(path) = &options.model_path {
                    let (path, _, _) = artifacts::snapshot_reserved(
                        path,
                        &self.config.state_dir.join("snapshots"),
                        self.config.max_snapshot_bytes,
                        self.pool.reserved_storage(),
                    )
                    .map_err(unavailable)?;
                    options.model_path = Some(path);
                }
                let report = StashBatchReport {
                    model_pack: options.model_pack.clone(),
                    schema_version: if !options.source_plans.is_empty() {
                        6
                    } else if !options.annotated_views.is_empty() {
                        5
                    } else if options.model_pack.is_some() {
                        4
                    } else {
                        2
                    },
                    batch_request_id: request.request_id.clone(),
                    state: "running".into(),
                    item_count: items.len() as u32,
                    offset: 0,
                    has_more: false,
                    pagination_complete: plan.discovery.pagination_complete,
                    items: vec![],
                    release_qualified: false,
                    stash_mutations_requested: options.scan,
                    ledger_sha256: None,
                    migrations: vec![],
                };
                let record = Record {
                    migration_history: vec![],
                    options,
                    worker_stamp: self.batch_worker_stamp()?,
                    owner_capability_sha256: format!(
                        "{:x}",
                        Sha256::digest(self.stash_owner_token.as_bytes())
                    ),
                    report,
                    items,
                };
                let body = ResponseBody::StashBatch(page(&record, 0)?);
                {
                    let mut db = self.db.lock().map_err(internal)?;
                    let tx = db.transaction().map_err(internal)?;
                    self.authorize(&tx, request, owner)?;
                    if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
                        return Ok(body);
                    }
                    let active: i64 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM stash_batches WHERE state='running'",
                            [],
                            |r| r.get(0),
                        )
                        .map_err(internal)?;
                    if active >= 2 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "at most two active Stash batches are admitted",
                        ));
                    }
                    if let Some(choice) = &record.options.model_pack {
                        self.require_current_pack_choice(&tx, choice)?;
                        if crate::inference_equivalence_reader::version(&tx).map_err(internal)? < 13
                        {
                            crate::inference_equivalence_reader::publish(&tx, 13)
                                .map_err(internal)?;
                        }
                    }
                    annotations::arm_reader(&tx, &record)?;
                    tx.execute(
                        "INSERT INTO stash_batches VALUES(?1,?2,'running',?3)",
                        params![
                            owner.as_str(),
                            request.request_id.as_str(),
                            encode(&record)?
                        ],
                    )
                    .map_err(internal)?;
                    remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                    tx.commit().map_err(internal)?;
                }
                self.spawn_stash_batch(owner.clone(), request.request_id.clone())?;
                Ok(body)
            }
            Command::StashBatchStatus {
                batch_request_id,
                offset,
                ..
            } => {
                let (raw, record) = self.load_batch(owner, batch_request_id)?;
                let mut report = page(&record, *offset)?;
                report.ledger_sha256 = Some(sha256_text(&raw));
                let db = self.db.lock().map_err(internal)?;
                self.authorize(&db, request, owner)?;
                for item in &report.items {
                    if let Some(project) = &item.project_id {
                        require_grant(&db, owner, project, Scope::Read)?;
                    }
                }
                Ok(ResponseBody::StashBatch(report))
            }
            Command::StashResumeBatch {
                batch_request_id, ..
            } => {
                let (raw, mut record) = self.load_batch(owner, batch_request_id)?;
                {
                    let db = self.db.lock().map_err(internal)?;
                    for item in &record.items {
                        if let Some(project) = &item.view.project_id {
                            require_grant(&db, owner, project, Scope::Read)?;
                        }
                    }
                }
                if record.report.state == "cancelled" || record.report.state == "finished" {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "terminal batch cannot resume; retained artifacts remain available",
                    ));
                }
                for item in &mut record.items {
                    if item.view.state == "unknown_outcome" {
                        item.view.state = "selected".into();
                        item.view.error = None;
                        item.polls = 0;
                    }
                }
                record.report.state = "running".into();
                {
                    let mut db = self.db.lock().map_err(internal)?;
                    let tx = db.transaction().map_err(internal)?;
                    self.authorize(&tx, request, owner)?;
                    let active:i64=tx.query_row("SELECT COUNT(*) FROM stash_batches WHERE state='running' AND NOT(session=?1 AND request=?2)",params![owner.as_str(),batch_request_id.as_str()],|r|r.get(0)).map_err(internal)?;
                    if active >= 2 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "active Stash batch admission limit",
                        ));
                    }
                    let changed=tx.execute("UPDATE stash_batches SET state='running',record=?4 WHERE session=?1 AND request=?2 AND record=?3",params![owner.as_str(),batch_request_id.as_str(),raw,encode(&record)?]).map_err(internal)?;
                    if changed != 1 {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "batch changed before resume",
                        ));
                    }
                    let body = ResponseBody::StashBatch(page(&record, 0)?);
                    remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                    tx.commit().map_err(internal)?;
                }
                self.spawn_stash_batch(owner.clone(), batch_request_id.clone())?;
                Ok(ResponseBody::StashBatch(page(&record, 0)?))
            }
            Command::StashMigrateBatch {
                batch_request_id,
                expected_ledger_sha256,
                generation,
                ..
            } => self.migrate_stash_batch(
                request,
                fingerprint,
                owner,
                batch_request_id,
                expected_ledger_sha256,
                generation,
            ),
            Command::StashCancelBatch {
                batch_request_id, ..
            } => {
                let (_, mut record) = self.load_batch(owner, batch_request_id)?;
                record.report.state = "cancelled".into();
                for item in &mut record.items {
                    if !terminal(item) {
                        item.view.state = "cancelled".into();
                    }
                }
                let body = ResponseBody::Ack;
                {
                    let mut db = self.db.lock().map_err(internal)?;
                    let tx = db.transaction().map_err(internal)?;
                    self.authorize(&tx, request, owner)?;
                    // Load under the same transaction so completed phase identities
                    // are not erased by a stale cancellation snapshot.
                    let current = load_record_text(&tx, owner, batch_request_id)?;
                    record = decode(&current)?;
                    if record.report.state != "finished" {
                        record.report.state = "cancelled".into();
                        for item in &mut record.items {
                            if !terminal(item) {
                                item.view.state = "cancelled".into();
                            }
                        }
                    }
                    tx.execute("UPDATE stash_batches SET state=?4,record=?3 WHERE session=?1 AND request=?2",params![owner.as_str(),batch_request_id.as_str(),encode(&record)?,record.report.state]).map_err(internal)?;
                    remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                    tx.commit().map_err(internal)?;
                }
                for item in &record.items {
                    if item.cursor == 3 {
                        if let (Some(project), Some(job)) =
                            (&item.view.project_id, &item.view.job_id)
                        {
                            let _ = self.batch_call(
                                owner,
                                batch_request_id,
                                &Pending {
                                    phase: "cancel_job".into(),
                                    id: id!(RequestId),
                                    command: Command::CancelJob {
                                        job_id: job.clone(),
                                    },
                                    project: Some(project.clone()),
                                    revision: None,
                                },
                            );
                        }
                    }
                }
                Ok(ResponseBody::Ack)
            }
            _ => Err(ProtocolError::invalid("unexpected Stash batch command")),
        }
    }
    fn migrate_stash_batch(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
        batch: &RequestId,
        expected: &str,
        generation: &StashBatchGeneration,
    ) -> PResult<ResponseBody> {
        // In addition to control admission, cover retained/raw/new ledger and
        // previous-options history simultaneously at the 16 MiB cell limit.
        let _migration_memory = self.pool.reserve_memory(64 * 1024 * 1024)?;
        // Stopped state plus raw-record CAS fences concurrent resume/cancel. Do
        // not hold runtime while acquiring DB: dispatch locks in reverse order.
        if self
            .stash_batch_runtime
            .lock()
            .map_err(internal)?
            .active
            .contains(&(owner.to_string(), batch.to_string()))
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "batch scheduler is still active; observe stopped state before migration",
            ));
        }
        let (raw, mut record) = self.load_batch(owner, batch)?;
        if sha256_text(&raw) != expected {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "batch ledger differs from observed migration identity",
            ));
        }
        if record.report.state != "needs_attention" {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "only a stopped needs_attention batch can migrate",
            ));
        }
        if record.report.migrations.len() >= 16 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "batch migration history limit reached",
            ));
        }
        generation.validate()?;
        if generation.model_pack.is_none() {
            self.admit_stash_generation(&generation.settings, &generation.annotated_views)?;
        }
        let mut options = record.options.clone();
        options.preset = generation.preset.clone();
        options.settings = generation.settings.clone();
        options.annotated_views = generation.annotated_views.clone();
        options.source_plans = generation.source_plans.clone();
        plans::validate_selection(&options, &record.items)?;
        // Replacement is explicit; None is explicit no-custom-model intent.
        // Snapshot preparation cannot publish a job or rewrite the old ledger.
        let _gate = self.artifact_gate.lock().map_err(internal)?;
        options.model_pack = generation.model_pack.clone();
        let model = if let Some(choice) = &generation.model_pack {
            let captured = self.prepare_selected_stash_pack(
                choice,
                options.settings.clone(),
                &options.annotated_views,
            )?;
            options.settings = captured.settings;
            Some(captured.model)
        } else {
            generation
                .model_path
                .as_ref()
                .map(|path| self.snapshot_model(path))
                .transpose()?
        };
        options.model_path = if options.model_pack.is_some() {
            None
        } else {
            model.as_ref().map(|model| model.path.clone())
        };
        options.validate()?;
        options
            .settings
            .validate_execution_preset(&options.preset, model.is_some())?;
        self.admit_stash_generation(&options.settings, &options.annotated_views)?;
        if let Some(profile) = &options.settings.inference_profile {
            if model
                .as_ref()
                .is_none_or(|model| model.identity != profile.model)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "migration captured model differs from declared inference profile",
                ));
            }
        }
        let mut receipt = StashBatchMigrationReceipt {
            superseded_placement_requests: vec![],
            reconciled_placement_requests: vec![],
            request_id: request.request_id.clone(),
            previous_ledger_sha256: expected.into(),
            previous_options_sha256: sha256_text(&encode(&record.options)?),
            current_options_sha256: sha256_text(&encode(&options)?),
            previous_worker_sha256: record.worker_stamp.clone(),
            current_worker_sha256: self.batch_worker_stamp()?,
            previous_model_sha256: self.batch_options_model_sha256(&record.options)?,
            current_model_sha256: model.as_ref().map(|model| model.identity.sha256.clone()),
            previous_model_pack: record.options.model_pack.clone(),
            current_model_pack: options.model_pack.clone(),
            superseded_generation_requests: vec![],
            reconciled_generation_requests: vec![],
        };
        let mut history = Migration {
            previous_options: record.options.clone(),
            previous_owner_capability_sha256: record.owner_capability_sha256.clone(),
            superseded: vec![],
        };
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        // Check again after artifact preparation, before inspecting successful
        // authoritative receipts or allocating new future phase identities.
        if load_record_text(&tx, owner, batch)? != raw {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "batch changed during migration preparation",
            ));
        }
        let mut future = 0;
        for (index, item) in record.items.iter_mut().enumerate() {
            if let Some(project) = &item.view.project_id {
                require_grant(&tx, owner, project, Scope::Read)?;
            }
            if item.cursor > 2
                || matches!(
                    item.view.state.as_str(),
                    "skipped" | "failed_or_unknown" | "cancelled"
                )
            {
                continue;
            }
            if let Some(pending) = item.pending.clone() {
                if item.cursor == 2 {
                    let (phase, expected_command) = if pending.phase == "generate" {
                        (
                            "generate",
                            annotations::generation_command(&record.options, item)?,
                        )
                    } else {
                        plans::placement_command(&record.options, item)?.ok_or_else(|| {
                            ProtocolError::invalid("pending placement intent absent")
                        })?
                    };
                    if pending.phase != phase
                        || encode(&pending.command)? != encode(&expected_command)?
                        || pending.project != item.view.project_id
                        || pending.revision != Some(item.revision)
                        || !item.view.requests.iter().any(|effect| {
                            effect.phase == phase
                                && effect.request_id == pending.id
                                && effect.expected_revision == pending.revision
                        })
                    {
                        return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"pending generation/placement intent does not match retained batch identity"));
                    }
                    let mut original = Request::new(pending.id.clone(), pending.command.clone())
                        .with_session(owner.clone());
                    original.project = pending.project.clone();
                    original.expected_revision = pending.revision;
                    let original_fingerprint = format!(
                        "{:x}",
                        Sha256::digest(request_fingerprint(&original).map_err(internal)?)
                    );
                    if let Some(body) =
                        replay(&tx, owner.as_str(), &original, &original_fingerprint)?
                    {
                        if phase == "generate" {
                            let ResponseBody::Job(job) = body else {
                                return Err(ProtocolError::new(
                                    ErrorCode::DependencyMismatch,
                                    "accepted generation receipt has unexpected type",
                                ));
                            };
                            if Some(&job.project_id) != pending.project.as_ref()
                                || Some(job.base_revision) != pending.revision
                            {
                                return Err(ProtocolError::new(
                                    ErrorCode::DependencyMismatch,
                                    "accepted generation receipt has different project/revision",
                                ));
                            }
                            receipt
                                .reconciled_generation_requests
                                .push(pending.id.clone());
                            item.view.job_id = Some(job.job_id);
                            item.cursor = 3;
                            item.pending = None;
                            item.polls = 0;
                            continue;
                        }
                        let ResponseBody::Project(project) = body else {
                            return Err(ProtocolError::new(
                                ErrorCode::DependencyMismatch,
                                "accepted placement receipt has unexpected type",
                            ));
                        };
                        plans::acknowledge(item, &pending, &project)?;
                        receipt
                            .reconciled_placement_requests
                            .push(pending.id.clone());
                    } else {
                        if phase == "generate" {
                            receipt
                                .superseded_generation_requests
                                .push(pending.id.clone());
                        } else {
                            receipt
                                .superseded_placement_requests
                                .push(pending.id.clone());
                        }
                        history.superseded.push((index, pending));
                    }
                    item.pending = None;
                    item.generation_plan = None;
                    item.polls = 0;
                }
            }
            annotations::validate_future(&options, item)?;
            plans::validate_future(&options, item)?;
            if let Some(project) = &item.view.project_id {
                require_grant(&tx, owner, project, Scope::Generate)?;
                if item.placement.is_some()
                    || plans::entry(&options, item)?.is_some_and(|p| p.placement.is_some())
                {
                    require_grant(&tx, owner, project, Scope::Edit)?;
                }
                if model.is_some() {
                    require_grant(&tx, owner, project, Scope::ImportSource)?;
                }
                let revision: i64 = tx
                    .query_row(
                        "SELECT revision FROM projects WHERE id=?1",
                        [project.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(internal)?;
                if u64::try_from(revision).map_err(internal)? != item.revision.value() {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "project revision changed before future-generation migration",
                    ));
                }
            }
            future += 1;
        }
        if future == 0 && receipt.reconciled_generation_requests.is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "batch has no future generation or unacknowledged generation receipt to migrate",
            ));
        }
        record.options = options;
        record.worker_stamp = receipt.current_worker_sha256.clone();
        record.owner_capability_sha256 = sha256_text(&self.stash_owner_token);
        if let Some(choice) = &record.options.model_pack {
            self.require_current_pack_choice(&tx, choice)?;
        }
        record.report.model_pack = record.options.model_pack.clone();
        record.report.schema_version = if record.options.model_pack.is_some()
            || record.report.schema_version == 4
            || history.previous_options.model_pack.is_some()
        {
            4
        } else {
            3
        };
        if record.report.schema_version == 4
            && crate::inference_equivalence_reader::version(&tx).map_err(internal)? < 13
        {
            crate::inference_equivalence_reader::publish(&tx, 13).map_err(internal)?;
        }
        record.report.migrations.push(receipt);
        record.migration_history.push(history);
        if annotations::requires_reader(&record) {
            record.report.schema_version = 5;
        }
        if plans::requires_reader(&record) {
            record.report.schema_version = 6;
        }
        annotations::arm_reader(&tx, &record)?;
        // The batch remains paused. Unknown publication/scan/tag state, existing
        // jobs and every old phase ID are unchanged; resume reconciles them.
        let encoded = encode(&record)?;
        if encoded.len() > 16 * 1024 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "batch ledger exceeds 16 MiB",
            ));
        }
        let body = ResponseBody::StashBatch(page(&record, 0)?);
        let changed=tx.execute("UPDATE stash_batches SET record=?4 WHERE session=?1 AND request=?2 AND state='needs_attention' AND record=?3",params![owner.as_str(),batch.as_str(),raw,encoded]).map_err(internal)?;
        if changed != 1 {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "batch changed before migration commit",
            ));
        }
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
    fn admit_stash_profile(&self, settings: &GenerationSettings) -> PResult<()> {
        if let Some(profile) = &settings.inference_profile {
            crate::inference_profiles::admit_adapter(
                profile,
                &ResourceBudget {
                    memory_bytes: WORKER_MEMORY,
                    output_bytes: OUTPUT_LIMIT,
                    wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128)
                        as u64,
                    cpu_threads: 2,
                },
            )
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        }
        Ok(())
    }
    fn batch_options_model_sha256(&self, options: &StashBatchOptions) -> PResult<Option<String>> {
        if options.model_pack.is_some() {
            return Ok(Some(
                options
                    .settings
                    .inference_profile
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("batch pack profile is absent"))?
                    .model
                    .sha256
                    .clone(),
            ));
        }
        options
            .model_path
            .as_ref()
            .map(|path| self.batch_model_sha256(path))
            .transpose()
    }
    fn batch_worker_stamp(&self) -> PResult<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(
                encode(&(&self.binary_identity.sha256, self.tools.as_ref().ok()))?.as_bytes()
            )
        ))
    }
    fn load_batch(&self, owner: &SessionId, id: &RequestId) -> PResult<(String, Record)> {
        let db = self.db.lock().map_err(internal)?;
        let raw = load_record_text(&db, owner, id)?;
        let record: Record = decode(&raw)?;
        if record.items.len() > 100
            || record.report.item_count as usize != record.items.len()
            || record.report.batch_request_id != *id
            || !matches!(record.report.schema_version, 2..=6)
            || record.report.ledger_sha256.is_some()
            || record.report.migrations.len() > 16
            || record.migration_history.len() != record.report.migrations.len()
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "batch ledger identity/layout is inconsistent",
            ));
        }
        validate_record_pack(&record)
            .map_err(|e| ProtocolError::new(ErrorCode::DependencyMismatch, e.to_string()))?;
        Ok((raw, record))
    }
    pub(super) fn recover_stash_batches(self: &Arc<Self>) -> Result<()> {
        let pending = {
            let db = self
                .db
                .lock()
                .map_err(|_| anyhow::anyhow!("batch recovery lock"))?;
            let mut statement=db.prepare("SELECT session,request FROM stash_batches WHERE state='running' ORDER BY rowid LIMIT 2")?;
            let rows = statement
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (owner, batch) in pending {
            self.spawn_stash_batch(SessionId::new(owner)?, RequestId::new(batch)?)?;
        }
        Ok(())
    }
    fn spawn_stash_batch(self: &Arc<Self>, owner: SessionId, batch: RequestId) -> PResult<()> {
        let key = (owner.to_string(), batch.to_string());
        {
            let mut runtime = self.stash_batch_runtime.lock().map_err(internal)?;
            if !runtime.active.insert(key.clone()) {
                return Ok(());
            }
        }
        let weak = Arc::downgrade(self);
        let failed_owner = owner.clone();
        let failed_batch = batch.clone();
        let cleanup_key = key.clone();
        let spawned = self.spawn_task("pulsar-stash-batch", None, move || {
            loop {
                let Some(engine) = weak.upgrade() else {
                    break;
                };
                if engine.is_shutting_down() {
                    break;
                }
                let keep = match engine.tick_stash_batch(&owner, &batch) {
                    Ok(keep) => keep,
                    Err(error) => {
                        let _ = engine.fail_stash_batch(&owner, &batch, error);
                        false
                    }
                };
                drop(engine);
                if !keep {
                    break;
                }
                thread::sleep(Duration::from_millis(250));
            }
            if let Some(engine) = weak.upgrade() {
                if let Ok(mut runtime) = engine.stash_batch_runtime.lock() {
                    runtime.active.remove(&key);
                }
                // A resume can commit while the previous worker is exiting. Hand
                // its running ledger back to a worker after releasing the slot.
                if !engine.is_shutting_down()
                    && engine
                        .load_batch(&owner, &batch)
                        .is_ok_and(|(_, record)| record.report.state == "running")
                {
                    let _ = engine.spawn_stash_batch(owner, batch);
                }
            }
        });
        if let Err(error) = spawned {
            if let Ok(mut runtime) = self.stash_batch_runtime.lock() {
                runtime.active.remove(&cleanup_key);
            }
            let error = ProtocolError::new(
                ErrorCode::Unavailable,
                format!("batch scheduler could not start: {error}"),
            );
            let _ = self.fail_stash_batch(&failed_owner, &failed_batch, error.clone());
            return Err(error);
        }
        Ok(())
    }
    fn fail_stash_batch(
        &self,
        owner: &SessionId,
        batch: &RequestId,
        error: ProtocolError,
    ) -> PResult<()> {
        let (raw, mut record) = self.load_batch(owner, batch)?;
        record.report.state = "needs_attention".into();
        if let Some(item) = record.items.iter_mut().find(|item| !terminal(item)) {
            item.view.state = "unknown_outcome".into();
            item.view.error = Some(
                format!("{:?}: {}", error.code, error.message)
                    .chars()
                    .take(2048)
                    .collect(),
            );
        }
        self.save_batch(owner, batch, &raw, &record)?;
        Ok(())
    }
    fn tick_stash_batch(self: &Arc<Self>, owner: &SessionId, batch: &RequestId) -> PResult<bool> {
        let _metadata = self.pool.reserve_memory(64 * 1024 * 1024)?;
        let (raw, mut record) = self.load_batch(owner, batch)?;
        if record.report.state != "running" {
            return Ok(false);
        }
        if record.owner_capability_sha256
            != format!("{:x}", Sha256::digest(self.stash_owner_token.as_bytes()))
        {
            return Err(ProtocolError::new(ErrorCode::Forbidden,"batch owner capability changed; previous scheduling authority is not renewed automatically"));
        }
        let Some(index) = record.items.iter().position(|item| !terminal(item)) else {
            record.report.state = if record
                .items
                .iter()
                .any(|item| item.view.state == "unknown_outcome")
            {
                "needs_attention"
            } else {
                "finished"
            }
            .into();
            self.save_batch(owner, batch, &raw, &record)?;
            return Ok(false);
        };
        let result = self.advance_batch_item(
            owner,
            batch,
            &record.options,
            &record.worker_stamp,
            &mut record.items[index],
        );
        if let Err(error) = result {
            let item = &mut record.items[index];
            item.view.state = if matches!(
                error.code,
                ErrorCode::UnknownOutcome | ErrorCode::Unavailable | ErrorCode::ResourceExhausted
            ) || (item.cursor == 2
                && error.code == ErrorCode::DependencyMismatch)
            {
                "unknown_outcome"
            } else {
                "failed_or_unknown"
            }
            .into();
            item.view.error = Some(
                format!("{:?}: {}", error.code, error.message)
                    .chars()
                    .take(2048)
                    .collect(),
            );
        }
        self.save_batch(owner, batch, &raw, &record)
    }
    fn save_batch(
        &self,
        owner: &SessionId,
        batch: &RequestId,
        raw: &str,
        record: &Record,
    ) -> PResult<bool> {
        let db = self.db.lock().map_err(internal)?;
        let encoded = encode(record)?;
        if encoded.len() > 16 * 1024 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "batch ledger exceeds 16 MiB",
            ));
        }
        let updated=db.execute("UPDATE stash_batches SET state=?4,record=?5 WHERE session=?1 AND request=?2 AND state='running' AND record=?3",params![owner.as_str(),batch.as_str(),raw,record.report.state,encoded]).map_err(internal)?;
        Ok(updated == 1)
    }
    fn advance_batch_item(
        self: &Arc<Self>,
        owner: &SessionId,
        batch: &RequestId,
        options: &StashBatchOptions,
        worker_stamp: &str,
        item: &mut Item,
    ) -> PResult<()> {
        if item.cursor == 2 && worker_stamp != self.batch_worker_stamp()? {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "batch generation engine/tool identity changed; explicit migration is required",
            ));
        }
        if let Some(pending) = item.pending.clone() {
            let body = self.batch_call(owner, batch, &pending)?;
            match (pending.phase.as_str(), body) {
                ("create", ResponseBody::Project(project)) => {
                    item.view.project_id = Some(project.project_id);
                    item.revision = project.revision;
                    item.cursor = 1;
                }
                ("capture", ResponseBody::StashSourceCaptured(capture)) => {
                    item.view.source_version = Some(capture.source_version.clone());
                    item.capture = Some(*capture);
                    item.cursor = 2;
                }
                ("place" | "unplace", ResponseBody::Project(project)) => {
                    plans::acknowledge(item, &pending, &project)?;
                }
                ("generate" | "resume_job", ResponseBody::Job(job)) => {
                    item.view.job_id = Some(job.job_id);
                    item.cursor = 3;
                }
                ("commit", ResponseBody::Project(project)) => {
                    item.revision = project.revision;
                    item.cursor = 5;
                }
                ("publish", ResponseBody::StashScriptExported(export)) => {
                    item.view.export = Some(*export);
                    item.cursor = if options.scan { 6 } else { 10 };
                }
                ("scan", ResponseBody::StashScan(scan)) => {
                    item.view.scan = Some(*scan);
                    item.cursor = 7;
                }
                ("tag" | "resume_tag", ResponseBody::StashTag(tag)) => {
                    item.view.tag = Some(*tag);
                    item.cursor = 9;
                }
                _ => {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "batch effect returned an unexpected response",
                    ))
                }
            }
            item.pending = None;
            item.polls = 0;
            return Ok(());
        }
        let project = item.view.project_id.clone();
        let rev = Some(item.revision);
        let connection = options.connection.clone();
        let proof = String::new();
        let (phase, command, scope, revision) = match item.cursor {
            0 => (
                "create",
                Command::CreateProject {
                    name: format!("Stash scene {}", item.view.scene_id),
                },
                None,
                None,
            ),
            1 => (
                "capture",
                Command::StashImportSource {
                    connection,
                    scene_id: item.view.scene_id.clone(),
                    file_id: item
                        .view
                        .file_id
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("selected file ID absent"))?,
                    expected_server_path: item.server_path.clone(),
                    expected_source_version: None,
                    mappings: options.mappings.clone(),
                    owner_proof: proof,
                },
                project,
                rev,
            ),
            2 => {
                if let Some((phase, command)) = plans::placement_command(options, item)? {
                    (phase, command, project, rev)
                } else {
                    item.generation_plan = plans::entry(options, item)?.cloned();
                    (
                        "generate",
                        annotations::generation_command(options, item)?,
                        project,
                        rev,
                    )
                }
            }
            3 => {
                item.polls += 1;
                if item.polls > 14400 {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "batch generation observation limit reached; job retained",
                    ));
                }
                let body = self.batch_call(
                    owner,
                    batch,
                    &Pending {
                        phase: "job_status".into(),
                        id: id!(RequestId),
                        command: Command::JobStatus {
                            job_id: item
                                .view
                                .job_id
                                .clone()
                                .ok_or_else(|| ProtocolError::invalid("batch job absent"))?,
                        },
                        project,
                        revision: None,
                    },
                )?;
                let ResponseBody::Job(job) = body else {
                    return Err(ProtocolError::invalid("batch job status response"));
                };
                if job.state == "interrupted"
                    && !item
                        .view
                        .requests
                        .iter()
                        .any(|effect| effect.phase == "resume_job")
                {
                    let id = id!(RequestId);
                    item.view.requests.push(StashBatchEffect {
                        phase: "resume_job".into(),
                        request_id: id.clone(),
                        expected_revision: None,
                    });
                    item.pending = Some(Pending {
                        phase: "resume_job".into(),
                        id,
                        command: Command::ResumeJob { job_id: job.job_id },
                        project: item.view.project_id.clone(),
                        revision: None,
                    });
                    return Ok(());
                }
                if job.error.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "recorded generation job reports an error; existing job retained",
                    ));
                }
                if let Some(candidate) = job.candidate_id {
                    item.view.candidate_id = Some(candidate);
                    item.cursor = 4;
                    item.polls = 0;
                } else if matches!(job.state.as_str(), "cancelled" | "failed" | "interrupted") {
                    return Err(ProtocolError::new(ErrorCode::Unavailable,"recorded generation job needs explicit recovery; no replacement job was created"));
                }
                return Ok(());
            }
            4 => (
                "commit",
                Command::CommitCandidate {
                    candidate_id: item
                        .view
                        .candidate_id
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("batch candidate absent"))?,
                },
                project,
                rev,
            ),
            5 => (
                "publish",
                Command::StashExportScript {
                    connection,
                    source_version: item
                        .view
                        .source_version
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("batch source absent"))?,
                    scene_id: item.view.scene_id.clone(),
                    file_id: item
                        .view
                        .file_id
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("batch file absent"))?,
                    candidate_id: item
                        .view
                        .candidate_id
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("batch candidate absent"))?,
                    job_id: item
                        .view
                        .job_id
                        .clone()
                        .ok_or_else(|| ProtocolError::invalid("batch job absent"))?,
                    owner_proof: proof,
                    source_time: item.generation_plan.as_ref().is_some_and(|p| p.source_time),
                    archived_origin: None,
                },
                project,
                rev,
            ),
            6 => (
                "scan",
                Command::StashSubmitScan {
                    connection,
                    publication_request_id: effect_id(item, "publish")?,
                    owner_proof: proof,
                },
                project,
                rev,
            ),
            7 => {
                item.polls += 1;
                if item.polls > 3600 {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "scan observation limit reached; recorded job retained",
                    ));
                }
                let body = self.batch_call(
                    owner,
                    batch,
                    &Pending {
                        phase: "scan_status".into(),
                        id: id!(RequestId),
                        command: Command::StashScanStatus {
                            connection,
                            scan_request_id: effect_id(item, "scan")?,
                            owner_proof: proof,
                        },
                        project,
                        revision: None,
                    },
                )?;
                let ResponseBody::StashScan(scan) = body else {
                    return Err(ProtocolError::invalid("batch scan status response"));
                };
                let state = scan.state.clone();
                item.view.scan = Some(*scan);
                match state {
                    StashScanState::VerifiedLinked => {
                        item.cursor = if options.tag.is_some() { 8 } else { 10 };
                        item.polls = 0;
                    }
                    StashScanState::DispatchUnknown
                    | StashScanState::JobMissing
                    | StashScanState::Failed
                    | StashScanState::Cancelled => {
                        return Err(ProtocolError::new(
                            ErrorCode::UnknownOutcome,
                            "scan is failed, missing or unknown; publication is retained",
                        ))
                    }
                    _ => {}
                }
                return Ok(());
            }
            8 => {
                let tag = options
                    .tag
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("requested batch tag absent"))?;
                (
                    "tag",
                    Command::StashAddTag {
                        connection,
                        scan_request_id: effect_id(item, "scan")?,
                        tag_name: tag.name.clone(),
                        create: tag.create,
                        owner_proof: proof,
                    },
                    project,
                    rev,
                )
            }
            9 => {
                let body = self.batch_call(
                    owner,
                    batch,
                    &Pending {
                        phase: "tag_status".into(),
                        id: id!(RequestId),
                        command: Command::StashTagStatus {
                            connection: connection.clone(),
                            tag_request_id: effect_id(item, "tag")?,
                            owner_proof: proof.clone(),
                        },
                        project: project.clone(),
                        revision: None,
                    },
                )?;
                let ResponseBody::StashTag(tag) = body else {
                    return Err(ProtocolError::invalid("batch tag status response"));
                };
                let state = tag.state.clone();
                let dispatched = tag.add_dispatched;
                item.view.tag = Some(*tag);
                if state == StashTagState::VerifiedAdded {
                    item.cursor = 10;
                    return Ok(());
                }
                if state == StashTagState::Resolved && !dispatched {
                    (
                        "resume_tag",
                        Command::StashResumeTag {
                            connection,
                            tag_request_id: effect_id(item, "tag")?,
                            owner_proof: proof,
                        },
                        project,
                        rev,
                    )
                } else {
                    return Err(ProtocolError::new(
                        ErrorCode::UnknownOutcome,
                        "tag creation or ADD is unresolved; no duplicate mutation was dispatched",
                    ));
                }
            }
            10 => {
                item.view.state = if options.tag.is_some() {
                    "tagged_unqualified"
                } else if options.scan {
                    "linked_unqualified"
                } else {
                    "published_unqualified"
                }
                .into();
                return Ok(());
            }
            _ => return Err(ProtocolError::invalid("unknown batch cursor")),
        };
        let id = id!(RequestId);
        item.view.requests.push(StashBatchEffect {
            phase: phase.into(),
            request_id: id.clone(),
            expected_revision: revision,
        });
        item.view.state = phase.into();
        item.pending = Some(Pending {
            phase: phase.into(),
            id,
            command,
            project: scope,
            revision,
        });
        Ok(())
    }
}
fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
/// Bound the SQLite cell before allocating its Rust String or parsing JSON.
fn load_record_text(db: &Connection, owner: &SessionId, id: &RequestId) -> PResult<String> {
    let row: Option<Option<String>> = db
        .query_row(
            "SELECT record FROM stash_batches WHERE session=?1 AND request=?2",
            params![owner.as_str(), id.as_str()],
            |row| {
                let value = row.get_ref(0)?.as_str()?;
                Ok((value.len() <= 16 * 1024 * 1024).then(|| value.to_owned()))
            },
        )
        .optional()
        .map_err(internal)?;
    row.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::NotFound,
            "batch is absent for this authenticated session",
        )
    })?
    .ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "batch ledger exceeds 16 MiB before parsing",
        )
    })
}
fn terminal(item: &Item) -> bool {
    matches!(
        item.view.state.as_str(),
        "skipped"
            | "published_unqualified"
            | "linked_unqualified"
            | "tagged_unqualified"
            | "failed_or_unknown"
            | "unknown_outcome"
            | "cancelled"
    )
}
fn effect_id(item: &Item, phase: &str) -> PResult<RequestId> {
    item.view
        .requests
        .iter()
        .find(|effect| effect.phase == phase)
        .map(|effect| effect.request_id.clone())
        .ok_or_else(|| ProtocolError::invalid("batch effect identity absent"))
}
fn page(record: &Record, offset: u32) -> PResult<StashBatchReport> {
    if offset as usize > record.items.len() {
        return Err(ProtocolError::invalid("batch offset exceeds selection"));
    }
    let mut report = record.report.clone();
    report.ledger_sha256 = Some(sha256_text(&encode(record)?));
    report.offset = offset;
    report.items = record
        .items
        .iter()
        .skip(offset as usize)
        .take(5)
        .map(|item| item.view.clone())
        .collect();
    report.has_more = offset as usize + report.items.len() < record.items.len();
    if encode(&report)?.len() > 768 * 1024 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "batch page exceeds bounded response limit",
        ));
    }
    Ok(report)
}

fn batch_generation_command(
    options: &StashBatchOptions,
    source_version: SourceVersionId,
) -> Command {
    if let Some(choice) = &options.model_pack {
        Command::GenerateFromModelPack {
            source_version,
            preset: options.preset.clone(),
            settings: Some(options.settings.clone()),
            choice: choice.clone(),
        }
    } else {
        Command::Generate {
            source_version,
            preset: options.preset.clone(),
            settings: Some(options.settings.clone()),
            model_path: options.model_path.clone(),
        }
    }
}
fn validate_record_pack(record: &Record) -> Result<()> {
    record.options.validate()?;
    annotations::validate_record(record)?;
    plans::validate_record(record)?;
    anyhow::ensure!(
        record.report.model_pack == record.options.model_pack,
        "batch report pack differs from captured intent"
    );
    if record.options.model_pack.is_some() {
        anyhow::ensure!(
            matches!(record.report.schema_version, 4..=6)
                && record.options.settings.inference_profile.is_some(),
            "pack batch profile/reader contract is absent"
        );
    }
    for (history, receipt) in record
        .migration_history
        .iter()
        .zip(&record.report.migrations)
    {
        history.previous_options.validate()?;
        anyhow::ensure!(
            history.previous_options.model_pack == receipt.previous_model_pack,
            "migration previous pack differs from retained intent"
        );
    }
    if let Some(receipt) = record.report.migrations.last() {
        anyhow::ensure!(
            receipt.current_model_pack == record.options.model_pack,
            "migration current pack differs from retained intent"
        );
    }
    Ok(())
}
/// Read-only startup checking before any writable SQLite checkpoint or scheduler.
pub(super) fn validate_pack_layout(db: &Connection) -> Result<()> {
    let names = db
        .prepare("PRAGMA table_info(stash_batches)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    anyhow::ensure!(
        names == ["session", "request", "state", "record"],
        "pack batch storage layout mismatch"
    );
    let mut query=db.prepare("SELECT request,state,length(CAST(record AS BLOB)),CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM stash_batches")?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        let bytes: i64 = row.get(2)?;
        anyhow::ensure!(
            (0..=16 * 1024 * 1024).contains(&bytes),
            "batch record exceeds bounded cell limit"
        );
        let raw: String = row.get(3)?;
        let record: Record = serde_json::from_str(&raw)?;
        validate_record_pack(&record)?;
        let request: String = row.get(0)?;
        let state: String = row.get(1)?;
        anyhow::ensure!(
            request == record.report.batch_request_id.as_str()
                && state == record.report.state
                && record.items.len() == record.report.item_count as usize
                && record.items.len() <= 100
                && matches!(record.report.schema_version, 2..=6)
                && record.migration_history.len() == record.report.migrations.len()
                && record.report.migrations.len() <= 16,
            "pack batch stored identity is inconsistent"
        );
    }
    Ok(())
}
pub(super) fn validate_annotated_reader(db: &Connection) -> Result<()> {
    annotations::validate_reader_requirement(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn setup() -> (
        tempfile::TempDir,
        EngineConfig,
        Arc<Engine>,
        SessionId,
        String,
        Record,
        Pending,
    ) {
        let directory = tempfile::tempdir().unwrap();
        // Ledger fixtures bind an authored identity and never execute this worker.
        let worker = directory.path().join("ledger-worker-identity");
        fs::write(&worker, b"authored Stash ledger worker identity").unwrap();
        let config = EngineConfig::new(directory.path().join("engine"), worker);
        let engine = Engine::open(config.clone()).unwrap();
        let ResponseBody::Paired {
            session,
            auth_token,
            ..
        } = engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "batch-ledger-test".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!()
        };
        let batch = id!(RequestId);
        let pending = Pending {
            phase: "create".into(),
            id: id!(RequestId),
            command: Command::CreateProject {
                name: "Stash scene authored".into(),
            },
            project: None,
            revision: None,
        };
        let view = StashBatchItem {
            scene_id: "authored".into(),
            file_id: Some("file".into()),
            state: "create".into(),
            eligibility: StashFileStatus::ReadyForSnapshot,
            project_id: None,
            source_version: None,
            job_id: None,
            candidate_id: None,
            requests: vec![StashBatchEffect {
                phase: "create".into(),
                request_id: pending.id.clone(),
                expected_revision: None,
            }],
            export: None,
            scan: None,
            tag: None,
            error: None,
        };
        let report = StashBatchReport {
            model_pack: None,
            schema_version: 2,
            batch_request_id: batch.clone(),
            state: "running".into(),
            item_count: 1,
            offset: 0,
            has_more: false,
            pagination_complete: true,
            items: vec![],
            release_qualified: false,
            stash_mutations_requested: false,
            ledger_sha256: None,
            migrations: vec![],
        };
        let options = StashBatchOptions {
            source_plans: vec![],
            connection: StashConnection {
                endpoint: "http://127.0.0.1:1/graphql".into(),
                credential_env: Some("AUTHORED_STASH_KEY_NAME".into()),
            },
            mappings: vec![StashRootMapping {
                server_root: "/stash".into(),
                local_root: directory.path().to_path_buf(),
            }],
            limit: 1,
            preset: "fast".into(),
            settings: GenerationSettings::default(),
            annotated_views: vec![],
            model_path: None,
            model_pack: None,
            scan: false,
            tag: None,
        };
        let record = Record {
            migration_history: vec![],
            options,
            worker_stamp: engine.batch_worker_stamp().unwrap(),
            owner_capability_sha256: format!(
                "{:x}",
                Sha256::digest(engine.stash_owner_token.as_bytes())
            ),
            report,
            items: vec![Item {
                placement: None,
                generation_plan: None,
                view,
                revision: RevisionId::new(0),
                cursor: 0,
                pending: Some(pending.clone()),
                capture: None,
                server_path: Some("/stash/source.mp4".into()),
                polls: 0,
            }],
        };
        engine
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO stash_batches VALUES(?1,?2,'running',?3)",
                params![session.as_str(), batch.as_str(), encode(&record).unwrap()],
            )
            .unwrap();
        (
            directory, config, engine, session, auth_token, record, pending,
        )
    }
    #[test]
    fn unsupported_maximum_batch_refuses_discovery_and_preserves_migration_ledger() {
        let (_directory, _config, engine, owner, token, mut record, _pending) = setup();
        let before = encode(&record).unwrap();
        let mut options = record.options.clone();
        options.preset = "maximum".into();
        let start = Request::new(
            id!(RequestId),
            Command::StashStartBatch {
                options,
                owner_proof: engine.stash_owner_token.clone(),
            },
        )
        .with_session(owner.clone())
        .with_auth_token(token.clone());
        assert_eq!(
            engine.handle(start).result.unwrap_err().code,
            ErrorCode::Unsupported
        );
        let (_, unchanged) = engine
            .load_batch(&owner, &record.report.batch_request_id)
            .unwrap();
        assert_eq!(encode(&unchanged).unwrap(), before);
        record.report.state = "needs_attention".into();
        let raw = encode(&record).unwrap();
        engine.db.lock().unwrap().execute("UPDATE stash_batches SET state='needs_attention',record=?3 WHERE session=?1 AND request=?2",params![owner.as_str(),record.report.batch_request_id.as_str(),raw]).unwrap();
        let migrate = Request::new(
            id!(RequestId),
            Command::StashMigrateBatch {
                batch_request_id: record.report.batch_request_id.clone(),
                expected_ledger_sha256: sha256_text(&raw),
                generation: StashBatchGeneration {
                    source_plans: vec![],
                    preset: "maximum".into(),
                    settings: GenerationSettings::default(),
                    annotated_views: vec![],
                    model_path: None,
                    model_pack: None,
                },
                owner_proof: engine.stash_owner_token.clone(),
            },
        )
        .with_session(owner.clone())
        .with_auth_token(token);
        assert_eq!(
            engine.handle(migrate).result.unwrap_err().code,
            ErrorCode::Unsupported
        );
        let (retained, _) = engine
            .load_batch(&owner, &record.report.batch_request_id)
            .unwrap();
        assert_eq!(retained, raw);
        let db = engine.db.lock().unwrap();
        let batches: i64 = db
            .query_row("SELECT COUNT(*) FROM stash_batches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(batches, 1);
        let jobs: i64 = db
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jobs, 0);
        #[cfg(target_os = "linux")]
        assert!(!engine.worker_fence.has_pending_worker_custody().unwrap());
    }

    #[test]
    fn committed_create_replays_after_restart_without_persisting_batch_credentials() {
        let (_directory, config, engine, owner, token, mut record, pending) = setup();
        let batch = record.report.batch_request_id.clone();
        let encoded = encode(&record).unwrap();
        assert!(!encoded.contains(&token));
        assert!(!encoded.contains(&engine.stash_owner_token));
        assert!(encoded.contains("AUTHORED_STASH_KEY_NAME"));
        let ResponseBody::Project(original) = engine.batch_call(&owner, &batch, &pending).unwrap()
        else {
            panic!()
        };
        // Lose the ledger acknowledgement after the authoritative create commit.
        // Pause only the fixture scheduler so recovery can be inspected directly.
        record.report.state = "needs_attention".into();
        engine.db.lock().unwrap().execute("UPDATE stash_batches SET state='needs_attention',record=?3 WHERE session=?1 AND request=?2",params![owner.as_str(),batch.as_str(),encode(&record).unwrap()]).unwrap();
        drop(engine);
        let engine = Engine::open(config).unwrap();
        record.report.state = "running".into();
        engine.db.lock().unwrap().execute("UPDATE stash_batches SET state='running',record=?3 WHERE session=?1 AND request=?2",params![owner.as_str(),batch.as_str(),encode(&record).unwrap()]).unwrap();
        let ResponseBody::Project(replayed) = engine.batch_call(&owner, &batch, &pending).unwrap()
        else {
            panic!()
        };
        assert_eq!(original.project_id, replayed.project_id);
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE name='Stash scene authored'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let untrusted =
            Request::new(pending.id.clone(), pending.command.clone()).with_session(owner);
        assert_eq!(
            engine.handle(untrusted).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
    }
    #[test]
    fn captured_model_locator_requires_exact_issued_namespace_and_digest() {
        let (_directory, _config, engine, _owner, _token, _record, _pending) = setup();
        let sha = "a".repeat(64);
        let path = engine.config.state_dir.join("snapshots").join(&sha);
        assert_eq!(engine.batch_model_sha256(&path).unwrap(), sha);
        for invalid in [
            engine.config.state_dir.join("other").join("a".repeat(64)),
            engine
                .config
                .state_dir
                .join("snapshots")
                .join("A".repeat(64)),
            engine.config.state_dir.join("snapshots").join("short"),
        ] {
            assert_eq!(
                engine.batch_model_sha256(&invalid).unwrap_err().code,
                ErrorCode::DependencyMismatch
            );
        }
    }
    #[test]
    fn prepared_model_identity_is_fenced_by_exact_internal_request() {
        let (_directory, _config, engine, owner, _token, record, _pending) = setup();
        let hash = "a".repeat(64);
        let path = engine.config.state_dir.join("snapshots").join(&hash);
        let request = Request::new(
            id!(RequestId),
            Command::Generate {
                source_version: SourceVersionId::new("source").unwrap(),
                preset: "fast".into(),
                settings: None,
                model_path: Some(path.clone()),
            },
        )
        .with_session(owner.clone());
        let thread = thread::current().id();
        engine.stash_batch_runtime.lock().unwrap().permits.insert(
            thread,
            Permit {
                session: owner,
                batch: record.report.batch_request_id,
                request: request.request_id.clone(),
                fingerprint: request_fingerprint(&request).unwrap(),
                model_sha256: Some(hash.clone()),
            },
        );
        let _guard = PermitGuard(&engine, thread);
        let mut artifact = SourceArtifact {
            source_version: SourceVersionId::new(hash.clone()).unwrap(),
            path,
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(hash.clone()).unwrap(),
                sha256: hash,
                byte_len: 7,
            },
        };
        engine
            .check_stash_batch_model(&request, Some(&artifact))
            .unwrap();
        artifact.identity.sha256 = "b".repeat(64);
        assert_eq!(
            engine
                .check_stash_batch_model(&request, Some(&artifact))
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        assert_eq!(
            engine
                .check_stash_batch_model(&request, None)
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        let mut foreign = request.clone();
        foreign.request_id = id!(RequestId);
        assert_eq!(
            engine
                .check_stash_batch_model(&foreign, Some(&artifact))
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    fn pack_profile() -> DeclaredInferenceProfile {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 134217728,
            minimum_output_bytes: 1048576,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }
    #[test]
    fn unsupported_cuda_start_refuses_before_discovery_or_new_ledger() {
        let (_dir, _config, engine, owner, token, record, _) = setup();
        let mut profile = pack_profile();
        profile.schema_version = 2;
        profile.provider = InferenceProvider::Cuda;
        profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 0,
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            address_space_bytes: 16 * 1024u64.pow(3),
            device_reservation_bytes: 1024u64.pow(3),
            arena_limit_bytes: 64 * 1024u64.pow(2),
        });
        let mut options = record.options.clone();
        options.model_path = Some("/must-not-open.onnx".into());
        options.settings.analysis_width = 64;
        options.settings.analysis_height = 64;
        options.settings.model_input = Some(profile.preprocessing.input.clone());
        options.settings.inference_profile = Some(profile);
        options.validate().unwrap();
        let before = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM stash_batches", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
        let response = engine.handle(
            Request::new(
                id!(RequestId),
                Command::StashStartBatch {
                    options,
                    owner_proof: engine.stash_owner_token.clone(),
                },
            )
            .with_session(owner)
            .with_auth_token(token),
        );
        assert!(response
            .result
            .unwrap_err()
            .message
            .contains("unsupported pinned provider"));
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM stash_batches", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            before
        );
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn unsupported_cuda_migration_preserves_original_ledger_before_model_capture() {
        let (_dir, _config, engine, owner, token, mut record, _) = setup();
        let raw = pause(&engine, &owner, &mut record);
        let mut profile = pack_profile();
        profile.schema_version = 2;
        profile.provider = InferenceProvider::Cuda;
        profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 0,
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            address_space_bytes: 16 * 1024u64.pow(3),
            device_reservation_bytes: 1024u64.pow(3),
            arena_limit_bytes: 64 * 1024u64.pow(2),
        });
        let settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile),
            ..GenerationSettings::default()
        };
        let request = Request::new(
            id!(RequestId),
            Command::StashMigrateBatch {
                batch_request_id: record.report.batch_request_id.clone(),
                expected_ledger_sha256: sha256_text(&raw),
                generation: StashBatchGeneration {
                    source_plans: vec![],
                    preset: "fast".into(),
                    settings,
                    annotated_views: vec![],
                    model_path: Some("/must-not-open.onnx".into()),
                    model_pack: None,
                },
                owner_proof: engine.stash_owner_token.clone(),
            },
        )
        .with_session(owner.clone())
        .with_auth_token(token);
        let result = engine.handle(request);
        assert!(result
            .result
            .unwrap_err()
            .message
            .contains("unsupported pinned provider"));
        assert_eq!(
            load_record_text(
                &engine.db.lock().unwrap(),
                &owner,
                &record.report.batch_request_id
            )
            .unwrap(),
            raw
        );
    }
    #[test]
    fn pack_record_and_readonly_startup_refuse_changed_choice_missing_profile_and_file_model() {
        let (_dir, _config, engine, owner, _token, mut record, _) = setup();
        record.options.model_pack = Some(ModelPackChoice {
            manifest_sha256: "c".repeat(64),
            selection_revision: 1,
        });
        record.options.settings.inference_profile = Some(pack_profile());
        record.options.settings.model_input = Some(pack_profile().preprocessing.input);
        record.report.model_pack = record.options.model_pack.clone();
        record.report.schema_version = 4;
        pause(&engine, &owner, &mut record);
        validate_pack_layout(&engine.db.lock().unwrap()).unwrap();
        let mut changed = record.clone();
        changed
            .report
            .model_pack
            .as_mut()
            .unwrap()
            .selection_revision = 2;
        assert!(validate_record_pack(&changed).is_err());
        let mut changed = record.clone();
        changed.options.settings.inference_profile = None;
        assert!(validate_record_pack(&changed).is_err());
        let mut changed = record.clone();
        changed.options.model_path = Some("/fixture/file-model".into());
        assert!(changed.options.validate().is_err());
        let raw = encode(&changed).unwrap();
        engine
            .db
            .lock()
            .unwrap()
            .execute("UPDATE stash_batches SET record=?1", [raw])
            .unwrap();
        assert!(validate_pack_layout(&engine.db.lock().unwrap()).is_err());
    }
    #[test]
    fn readonly_pack_startup_bounds_cell_in_sql_before_deserializing_it() {
        let (_dir, _config, engine, _owner, _token, _record, _) = setup();
        let db = engine.db.lock().unwrap();
        db.execute(
            "UPDATE stash_batches SET record=printf('%.*c',16777217,'x')",
            [],
        )
        .unwrap();
        assert!(validate_pack_layout(&db)
            .unwrap_err()
            .to_string()
            .contains("bounded cell limit"));
        assert_eq!(db.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=16777216 THEN record ELSE NULL END FROM stash_batches",[],|row|row.get::<_,Option<String>>(0)).unwrap(),None);
    }
    fn pause(engine: &Engine, owner: &SessionId, record: &mut Record) -> String {
        record.report.state = "needs_attention".into();
        let raw = encode(record).unwrap();
        engine.db.lock().unwrap().execute("UPDATE stash_batches SET state='needs_attention',record=?3 WHERE session=?1 AND request=?2",params![owner.as_str(),record.report.batch_request_id.as_str(),raw]).unwrap();
        raw
    }
    fn migration_request(
        engine: &Engine,
        owner: &SessionId,
        token: &str,
        record: &Record,
        raw: &str,
    ) -> Request {
        Request::new(
            id!(RequestId),
            Command::StashMigrateBatch {
                batch_request_id: record.report.batch_request_id.clone(),
                expected_ledger_sha256: sha256_text(raw),
                generation: StashBatchGeneration {
                    source_plans: vec![],
                    preset: "fast".into(),
                    settings: GenerationSettings::default(),
                    annotated_views: vec![],
                    model_path: None,
                    model_pack: None,
                },
                owner_proof: engine.stash_owner_token.clone(),
            },
        )
        .with_session(owner.clone())
        .with_auth_token(token.into())
    }
    #[test]
    fn explicit_migration_is_cas_bound_paused_replayable_and_retains_old_intent() {
        let (_dir, _config, engine, owner, token, mut record, pending) = setup();
        let raw = pause(&engine, &owner, &mut record);
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        let mut stale = request.clone();
        stale.request_id = id!(RequestId);
        if let Command::StashMigrateBatch {
            expected_ledger_sha256,
            ..
        } = &mut stale.command
        {
            *expected_ledger_sha256 = "0".repeat(64);
        }
        assert_eq!(
            engine.handle(stale).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let ResponseBody::StashBatch(report) = engine.handle(request.clone()).result.unwrap()
        else {
            panic!()
        };
        assert_eq!(report.state, "needs_attention");
        assert_eq!(report.schema_version, 3);
        assert_eq!(report.migrations.len(), 1);
        let ResponseBody::StashBatch(replayed) = engine.handle(request).result.unwrap() else {
            panic!()
        };
        assert_eq!(encode(&report).unwrap(), encode(&replayed).unwrap());
        let (after, stored) = engine
            .load_batch(&owner, &record.report.batch_request_id)
            .unwrap();
        assert_eq!(
            report.ledger_sha256.as_deref(),
            Some(sha256_text(&after).as_str())
        );
        assert_eq!(stored.items[0].pending.as_ref().unwrap().id, pending.id);
        assert_eq!(stored.migration_history.len(), 1);
        assert!(!after.contains(&token));
        assert!(!after.contains(&engine.stash_owner_token));
        assert!(engine.stash_batch_runtime.lock().unwrap().active.is_empty());
        // The previous deny_unknown_fields report reader refuses migration rows;
        // current default fields still decode unmodified schema-2 records.
        assert!(serde_json::from_str::<Record>(&raw).is_ok());
    }
    #[test]
    fn migration_refuses_running_active_terminal_and_overlarge_ledgers() {
        let (_dir, _config, engine, owner, token, mut record, _) = setup();
        let raw = encode(&record).unwrap();
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        assert_eq!(
            engine.handle(request).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        let raw = pause(&engine, &owner, &mut record);
        engine.stash_batch_runtime.lock().unwrap().active.insert((
            owner.to_string(),
            record.report.batch_request_id.to_string(),
        ));
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        assert_eq!(
            engine.handle(request).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        engine.stash_batch_runtime.lock().unwrap().active.clear();
        record.report.state = "cancelled".into();
        let raw = encode(&record).unwrap();
        engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE stash_batches SET state='cancelled',record=?1",
                [&raw],
            )
            .unwrap();
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        assert_eq!(
            engine.handle(request).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE stash_batches SET record=printf('%.*c',16777217,'x')",
                [],
            )
            .unwrap();
        assert_eq!(
            engine
                .load_batch(&owner, &record.report.batch_request_id)
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn migration_reconciles_accepted_generation_without_new_job_or_superseding_id() {
        for pack in [false, true] {
            let (_dir, _config, engine, owner, token, mut record, create) = setup();
            let batch = record.report.batch_request_id.clone();
            let ResponseBody::Project(project) =
                engine.batch_call(&owner, &batch, &create).unwrap()
            else {
                panic!()
            };
            if pack {
                record.options.model_pack = Some(ModelPackChoice {
                    manifest_sha256: "c".repeat(64),
                    selection_revision: 1,
                });
                record.options.settings.inference_profile = Some(pack_profile());
                record.options.settings.model_input = Some(pack_profile().preprocessing.input);
                record.report.model_pack = record.options.model_pack.clone();
                record.report.schema_version = 4;
            }
            let pending = Pending {
                phase: "generate".into(),
                id: id!(RequestId),
                command: batch_generation_command(
                    &record.options,
                    SourceVersionId::new("authored-source").unwrap(),
                ),
                project: Some(project.project_id.clone()),
                revision: Some(project.revision),
            };
            let item = &mut record.items[0];
            item.cursor = 2;
            item.view.project_id = Some(project.project_id.clone());
            item.view.source_version = Some(SourceVersionId::new("authored-source").unwrap());
            item.pending = Some(pending.clone());
            item.view.requests.push(StashBatchEffect {
                phase: "generate".into(),
                request_id: pending.id.clone(),
                expected_revision: pending.revision,
            });
            let job = JobSnapshot {
                job_id: id!(JobId),
                attempt_id: id!(AttemptId),
                project_id: project.project_id,
                base_revision: project.revision,
                state: "queued".into(),
                candidate_id: None,
                error: None,
            };
            let mut accepted = Request::new(pending.id.clone(), pending.command.clone())
                .with_session(owner.clone());
            accepted.project = pending.project.clone();
            accepted.expected_revision = pending.revision;
            let fp = format!(
                "{:x}",
                Sha256::digest(request_fingerprint(&accepted).unwrap())
            );
            {
                let mut db = engine.db.lock().unwrap();
                let tx = db.transaction().unwrap();
                remember(
                    &tx,
                    owner.as_str(),
                    &accepted,
                    &fp,
                    &ResponseBody::Job(job.clone()),
                )
                .unwrap();
                tx.commit().unwrap();
            }
            let raw = pause(&engine, &owner, &mut record);
            let request = migration_request(&engine, &owner, &token, &record, &raw);
            let ResponseBody::StashBatch(report) = engine.handle(request).result.unwrap() else {
                panic!()
            };
            assert_eq!(
                report.migrations[0].reconciled_generation_requests,
                vec![pending.id.clone()]
            );
            assert!(report.migrations[0]
                .superseded_generation_requests
                .is_empty());
            assert_eq!(report.items[0].job_id, Some(job.job_id));
            let (_, stored) = engine.load_batch(&owner, &batch).unwrap();
            assert_eq!(stored.items[0].cursor, 3);
            assert!(stored.items[0].pending.is_none());
            assert_eq!(stored.items[0].view.requests.len(), 2);
        }
    }
    #[test]
    fn migration_requires_fresh_owner_and_future_project_grants_and_revision() {
        let (_dir, _config, engine, owner, token, mut record, create) = setup();
        let batch = record.report.batch_request_id.clone();
        let ResponseBody::Project(project) = engine.batch_call(&owner, &batch, &create).unwrap()
        else {
            panic!()
        };
        record.items[0].view.project_id = Some(project.project_id.clone());
        record.items[0].pending = None;
        record.items[0].cursor = 1;
        let raw = pause(&engine, &owner, &mut record);
        let mut wrong_owner = migration_request(&engine, &owner, &token, &record, &raw);
        if let Command::StashMigrateBatch { owner_proof, .. } = &mut wrong_owner.command {
            *owner_proof = "wrong-owner".into();
        }
        assert_eq!(
            engine.handle(wrong_owner).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        {
            let db = engine.db.lock().unwrap();
            db.execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
                params![
                    owner.as_str(),
                    project.project_id.as_str(),
                    scope_name(Scope::Generate)
                ],
            )
            .unwrap();
        }
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        assert_eq!(
            engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        {
            let db = engine.db.lock().unwrap();
            db.execute(
                "INSERT INTO grants VALUES(?1,?2,?3)",
                params![
                    owner.as_str(),
                    project.project_id.as_str(),
                    scope_name(Scope::Generate)
                ],
            )
            .unwrap();
            db.execute(
                "UPDATE projects SET revision=1 WHERE id=?1",
                [project.project_id.as_str()],
            )
            .unwrap();
        }
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        assert_eq!(
            engine.handle(request).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(engine.load_batch(&owner, &batch).unwrap().0, raw);
    }
    #[test]
    fn migration_supersedes_only_unaccepted_generation_and_preserves_later_effects() {
        let (_dir, _config, engine, owner, token, mut record, create) = setup();
        let batch = record.report.batch_request_id.clone();
        let ResponseBody::Project(project) = engine.batch_call(&owner, &batch, &create).unwrap()
        else {
            panic!()
        };
        let pending = Pending {
            phase: "generate".into(),
            id: id!(RequestId),
            command: Command::Generate {
                source_version: SourceVersionId::new("authored-source").unwrap(),
                preset: record.options.preset.clone(),
                settings: Some(record.options.settings.clone()),
                model_path: None,
            },
            project: Some(project.project_id.clone()),
            revision: Some(project.revision),
        };
        let item = &mut record.items[0];
        item.cursor = 2;
        item.view.project_id = Some(project.project_id);
        item.view.source_version = Some(SourceVersionId::new("authored-source").unwrap());
        item.pending = Some(pending.clone());
        item.view.requests.push(StashBatchEffect {
            phase: "generate".into(),
            request_id: pending.id.clone(),
            expected_revision: pending.revision,
        });
        let mut later = item.clone();
        later.cursor = 7;
        later.pending = None;
        later.view.state = "unknown_outcome".into();
        later.view.scene_id = "retained-scan".into();
        let retained = encode(&later).unwrap();
        record.items.push(later);
        record.report.item_count = 2;
        let raw = pause(&engine, &owner, &mut record);
        let request = migration_request(&engine, &owner, &token, &record, &raw);
        engine.handle(request).result.unwrap();
        let (_, stored) = engine.load_batch(&owner, &batch).unwrap();
        assert_eq!(encode(&stored.items[1]).unwrap(), retained);
        assert_eq!(
            stored.report.migrations[0].superseded_generation_requests,
            vec![pending.id.clone()]
        );
        assert_eq!(stored.migration_history[0].superseded[0].1.id, pending.id);
        assert!(stored.items[0].pending.is_none());
        assert_eq!(stored.items[0].view.requests.len(), 2);
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn internal_permit_cannot_cross_threads_or_authorize_another_request() {
        let (_directory, _config, engine, owner, _token, record, pending) = setup();
        let batch = record.report.batch_request_id;
        let request =
            Request::new(pending.id.clone(), pending.command.clone()).with_session(owner.clone());
        let thread = thread::current().id();
        engine.stash_batch_runtime.lock().unwrap().permits.insert(
            thread,
            Permit {
                session: owner.clone(),
                batch,
                request: request.request_id.clone(),
                fingerprint: request_fingerprint(&request).unwrap(),
                model_sha256: None,
            },
        );
        let _guard = PermitGuard(&engine, thread);
        let foreign_engine = engine.clone();
        let foreign_request = request.clone();
        assert_eq!(
            thread::spawn(move || foreign_engine
                .handle(foreign_request)
                .result
                .unwrap_err()
                .code)
            .join()
            .unwrap(),
            ErrorCode::Unauthorized
        );
        let other = Request::new(
            id!(RequestId),
            Command::CreateProject {
                name: "untrusted replacement".into(),
            },
        )
        .with_session(owner);
        assert_eq!(
            engine.handle(other).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
    }
}
