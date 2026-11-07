//! Engine-owned local input/phase ledger. No bearer token or publication claim.
use super::*;
use pulsar_protocol::LibraryBatchPhase as Phase;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    id: RequestId,
    command: Command,
    project: Option<ProjectId>,
    revision: Option<RevisionId>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    options: LibraryBatchOptions,
    worker_stamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_capture: Option<Capture>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pack_documents: Vec<Capture>,
    report: LibraryBatchReport,
    captures: Vec<Capture>,
    pending: Option<(usize, Pending)>,
}
include!("library_migration.rs");
fn checked(record: &Record, state: &Path) -> PResult<()> {
    record.report.validate_migration()?;
    record.options.validate()?;
    let report = &record.report;
    let visual = matches!(record.options.recipe, LibraryBatchRecipe::Visual(_));
    if report.schema
        != record
            .options
            .publication_schema(report.cancellation.is_some())
        || report.model_pack.as_deref() != record.options.recipe.model_pack()
        || report.qualified
        || report.publication_completed
        || report.items.len() != record.options.inputs.len()
        || record.captures.len() != report.items.len()
        || record.worker_stamp.len() != 64
        || report.generation_ready
            != report
                .items
                .iter()
                .all(|i| i.phase == Phase::ReadyForPublication)
    {
        return Err(ProtocolError::invalid("Local batch ledger shape differs"));
    }
    for ((item, input), capture) in report
        .items
        .iter()
        .zip(&record.options.inputs)
        .zip(&record.captures)
    {
        if capture.path != state.join("snapshots").join(&capture.sha256)
            || capture.sha256.len() != 64
            || !capture
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || capture.bytes == 0
            || item.source != input.source
            || item.destination != input.destination
            || item.source_sha256 != capture.sha256
            || item.source_bytes != capture.bytes
            || item.publication_directory != input.publication_directory
            || item.publication_axes
                != if visual {
                    Some(vec![Axis::Stroke])
                } else {
                    input.publication_axes.clone()
                }
            || item.effects.len() > 5
        {
            return Err(ProtocolError::invalid(
                "Local batch capture/report identity differs",
            ));
        }
        if item.phase != Phase::Selected && item.project_id.is_none() {
            return Err(ProtocolError::invalid("Local batch phase lacks project"));
        }
        if matches!(
            item.phase,
            Phase::SourceImported
                | Phase::Generating
                | Phase::CandidateReady
                | Phase::ReadyForPublication
        ) && item.source_version.as_ref().map(SourceVersionId::as_str)
            != Some(capture.sha256.as_str())
        {
            return Err(ProtocolError::invalid(
                "Local batch phase lacks captured source",
            ));
        }
        if matches!(
            item.phase,
            Phase::Generating | Phase::CandidateReady | Phase::ReadyForPublication
        ) && item.job_id.is_none()
        {
            return Err(ProtocolError::invalid("Local batch phase lacks job"));
        }
        if matches!(
            item.phase,
            Phase::CandidateReady | Phase::ReadyForPublication
        ) && item.candidate_id.is_none()
        {
            return Err(ProtocolError::invalid("Local batch phase lacks candidate"));
        }
    }
    let expected_model = match &record.options.recipe {
        LibraryBatchRecipe::Visual(recipe) => {
            recipe.model_path.is_some() || recipe.model_pack.is_some()
        }
        LibraryBatchRecipe::Audio(_) => false,
    };
    if expected_model != record.model_capture.is_some() {
        return Err(ProtocolError::invalid(
            "Batch model capture absent or unexpected",
        ));
    }
    if let Some(model) = &record.model_capture {
        if model.path != state.join("snapshots").join(&model.sha256)
            || model.bytes == 0
            || model.sha256.len() != 64
            || !model
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(ProtocolError::invalid(
                "Batch model capture identity differs",
            ));
        }
    }
    if record.pack_documents.len()
        != if record.options.recipe.model_pack().is_some() {
            3
        } else {
            0
        }
    {
        return Err(ProtocolError::invalid(
            "Pack batch document capture differs",
        ));
    }
    for document in &record.pack_documents {
        if document.path != state.join("snapshots").join(&document.sha256)
            || document.bytes == 0
            || document.sha256.len() != 64
            || !document
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(ProtocolError::invalid(
                "Pack batch document identity differs",
            ));
        }
    }
    if let LibraryBatchRecipe::Visual(recipe) = &record.options.recipe {
        if let Some(choice) = &recipe.model_pack {
            if record.pack_documents[0].sha256 != choice.manifest_sha256 {
                return Err(ProtocolError::invalid(
                    "Captured pack manifest identity differs",
                ));
            }
            let profile = recipe
                .settings
                .inference_profile
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("Pack batch profile absent"))?;
            let model = record
                .model_capture
                .as_ref()
                .expect("checked model capture");
            if profile.model.sha256 != model.sha256 || profile.model.byte_len != model.bytes {
                return Err(ProtocolError::invalid(
                    "Pack batch model/profile identity differs",
                ));
            }
            recipe
                .settings
                .validate_execution_preset(&recipe.preset, true)?;
        }
    }
    if let Some(cancel) = &report.cancellation {
        if cancel.worker_retirement_verified
            || cancel.jobs.len() > report.items.len()
            || (cancel.completed
                && (cancel.error.is_some()
                    || record.pending.is_some()
                    || report.pending_advance_request_id.is_some()
                    || cancel
                        .jobs
                        .iter()
                        .any(|effect| effect.acknowledgement.is_none())))
        {
            return Err(ProtocolError::invalid(
                "Local batch cancellation shape differs",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut jobs = std::collections::BTreeSet::new();
        for effect in &cancel.jobs {
            let item = report
                .items
                .iter()
                .find(|item| item.job_id.as_ref() == Some(&effect.job_id))
                .ok_or_else(|| ProtocolError::invalid("Cancelled job absent from batch"))?;
            if !ids.insert(&effect.request_id)
                || !jobs.insert(&effect.job_id)
                || item.phase != Phase::Generating
                || item.project_id.as_ref() != Some(&effect.project_id)
                || item.revision != effect.base_revision
            {
                return Err(ProtocolError::invalid(
                    "Local batch cancellation binding differs",
                ));
            }
            if let Some(ack) = &effect.acknowledgement {
                if ack.job_id != effect.job_id
                    || ack.project_id != effect.project_id
                    || ack.base_revision != effect.base_revision
                    || !matches!(ack.state.as_str(), "completed" | "failed" | "cancelled")
                {
                    return Err(ProtocolError::invalid(
                        "Local batch cancellation acknowledgement differs",
                    ));
                }
            }
        }
        if cancel.completed
            && report
                .items
                .iter()
                .filter(|item| item.phase == Phase::Generating)
                .any(|item| {
                    !cancel
                        .jobs
                        .iter()
                        .any(|effect| Some(&effect.job_id) == item.job_id.as_ref())
                })
        {
            return Err(ProtocolError::invalid(
                "Local batch cancellation lacks known job",
            ));
        }
    }
    if record.report.pending_advance_request_id.is_some() && record.pending.is_none() {
        return Err(ProtocolError::invalid(
            "Local batch advance identity lacks pending phase",
        ));
    }
    if let Some((index, pending)) = &record.pending {
        let item = report
            .items
            .get(*index)
            .ok_or_else(|| ProtocolError::invalid("Local batch pending index differs"))?;
        let expected = phase_command(record, *index)?;
        if pending.project != expected.0
            || pending.revision != expected.1
            || encode(&pending.command)? != encode(&expected.2)?
            || item.effects.last().map(|effect| &effect.request_id) != Some(&pending.id)
        {
            return Err(ProtocolError::invalid(
                "Local batch pending identity differs",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_reader(db: &Connection, config: &EngineConfig) -> Result<()> {
    crate::library_batch_db::capacity(db)?;
    let present: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_batches')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(());
    }
    let pool = ResourcePool::new(config.max_ram_bytes, config.max_jobs)
        .with_cpu_limit(config.max_cpu_threads)?;
    let _metadata = pool.reserve_memory(16 * 1024 * 1024)?;
    let mut statement = db.prepare("SELECT session,request,record FROM library_batches")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let session: String = row.get(0)?;
        let id: String = row.get(1)?;
        let raw: String = row.get(2)?;
        let record: Record = serde_json::from_str(&raw)?;
        let floor = crate::inference_equivalence_reader::version(db)?;
        anyhow::ensure!(
            (record.report.cancellation.is_none() || floor >= 29)
                && (!matches!(record.options.recipe, LibraryBatchRecipe::Visual(_)) || floor >= 30),
            "local batch cancellation reader floor downgraded"
        );
        checked_pack_membership(db, &record, &session, &id)?;
        checked_migration_membership(db, &record, &session, &id)?;
        checked(&record, &config.state_dir).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        anyhow::ensure!(
            record.report.batch_request_id.as_str() == id,
            "local batch request index differs"
        );
        let count: i64 = db.query_row(
            "SELECT count(*) FROM library_batch_sources WHERE session=?1 AND request=?2",
            params![session, id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            count as usize == record.captures.len(),
            "local batch source membership differs"
        );
        if floor >= 30 {
            let sha: Option<String> = db
                .query_row(
                    "SELECT sha256 FROM library_batch_models WHERE session=?1 AND request=?2",
                    params![session, id],
                    |r| r.get(0),
                )
                .optional()?;
            anyhow::ensure!(
                sha.as_deref()
                    == record
                        .model_capture
                        .as_ref()
                        .map(|model| model.sha256.as_str()),
                "local batch model membership differs"
            );
        }
        for (index, capture) in record.captures.iter().enumerate() {
            let sha:String=db.query_row("SELECT sha256 FROM library_batch_sources WHERE session=?1 AND request=?2 AND item=?3",params![session,id,index],|r|r.get(0))?;
            anyhow::ensure!(
                sha == capture.sha256,
                "local batch source membership digest differs"
            );
        }
    }
    Ok(())
}
fn checked_pack_membership(
    db: &Connection,
    record: &Record,
    session: &str,
    request: &str,
) -> Result<()> {
    let floor = crate::inference_equivalence_reader::version(db)?;
    anyhow::ensure!(
        record.options.recipe.model_pack().is_none() || floor >= 31,
        "pack batch reader floor downgraded"
    );
    if floor >= 31 {
        let mut stmt = db.prepare("SELECT item,sha256 FROM library_batch_pack_documents WHERE session=?1 AND request=?2 ORDER BY item")?;
        let rows = stmt
            .query_map(params![session, request], |r| {
                Ok((r.get::<_, usize>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        anyhow::ensure!(
            rows == record
                .pack_documents
                .iter()
                .enumerate()
                .map(|(i, d)| (i, d.sha256.clone()))
                .collect::<Vec<_>>(),
            "pack batch document membership differs"
        );
    }
    Ok(())
}
fn phase_command(
    record: &Record,
    index: usize,
) -> PResult<(Option<ProjectId>, Option<RevisionId>, Command)> {
    let item = &record.report.items[index];
    let project = item.project_id.clone();
    let revision = Some(item.revision);
    Ok(match item.phase {
        Phase::Selected => (
            None,
            None,
            Command::CreateProject {
                name: format!("Local batch item {}", index + 1),
            },
        ),
        Phase::ProjectCreated => (
            project,
            revision,
            Command::ImportSourcePinned {
                path: record.captures[index].path.clone(),
                sha256: item.source_sha256.clone(),
                byte_len: item.source_bytes,
            },
        ),
        Phase::SourceImported => {
            let source_version = item
                .source_version
                .clone()
                .ok_or_else(|| ProtocolError::invalid("Local batch source absent"))?;
            let command = match &record.options.recipe {
                LibraryBatchRecipe::Audio(recipe) => Command::GenerateInput {
                    input: GenerationInput::AudioBundle {
                        source_version,
                        recipe: recipe.clone(),
                    },
                },
                LibraryBatchRecipe::Visual(recipe) if recipe.model_pack.is_some() => {
                    Command::GenerateFromModelPack {
                        source_version,
                        preset: recipe.preset.clone(),
                        settings: Some(recipe.settings.clone()),
                        choice: recipe.model_pack.clone().expect("selected pack"),
                    }
                }
                LibraryBatchRecipe::Visual(recipe) => Command::Generate {
                    source_version,
                    preset: recipe.preset.clone(),
                    settings: Some(recipe.settings.clone()),
                    model_path: record
                        .model_capture
                        .as_ref()
                        .map(|model| model.path.clone()),
                },
            };
            (project, revision, command)
        }
        Phase::CandidateReady => (
            project,
            revision,
            Command::CommitCandidate {
                candidate_id: item
                    .candidate_id
                    .clone()
                    .ok_or_else(|| ProtocolError::invalid("Local batch candidate absent"))?,
            },
        ),
        _ => {
            return Err(ProtocolError::invalid(
                "Local batch phase has no pending mutation",
            ))
        }
    })
}
impl Engine {
    fn library_capture_recipe(
        &self,
        recipe: &mut LibraryBatchRecipe,
    ) -> PResult<(Option<Capture>, Vec<Capture>)> {
        let mut pack_documents = Vec::new();
        let model_capture = if let LibraryBatchRecipe::Visual(recipe) = recipe {
            if let Some(choice) = &recipe.model_pack {
                let prepared = self.prepare_selected_pack(choice, recipe.settings.clone())?;
                recipe.settings = prepared.settings;
                recipe
                    .settings
                    .validate_execution_preset(&recipe.preset, true)?;
                pack_documents = prepared
                    .dependencies
                    .into_iter()
                    .map(|d| Capture {
                        path: self.config.state_dir.join("snapshots").join(&d.sha256),
                        sha256: d.sha256,
                        bytes: d.byte_len,
                    })
                    .collect();
                Some(Capture {
                    path: prepared.model.path,
                    sha256: prepared.model.identity.sha256,
                    bytes: prepared.model.identity.byte_len,
                })
            } else if let Some(model) = &recipe.model_path {
                let (path, sha256, bytes) = artifacts::snapshot_reserved(
                    model,
                    &self.config.state_dir.join("snapshots"),
                    self.config.max_snapshot_bytes,
                    self.pool.reserved_storage(),
                )
                .map_err(unavailable)?;
                if bytes == 0 {
                    return Err(ProtocolError::invalid("Batch model is empty"));
                }
                Some(Capture {
                    path,
                    sha256,
                    bytes,
                })
            } else {
                None
            }
        } else {
            None
        };
        Ok((model_capture, pack_documents))
    }
    fn library_worker_stamp(&self) -> PResult<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(
                encode(&(&self.binary_identity.sha256, self.tools.as_ref().ok()))?.as_bytes()
            )
        ))
    }
    fn library_load(&self, owner: &SessionId, id: &RequestId) -> PResult<Record> {
        let db = self.db.lock().map_err(internal)?;
        crate::library_batch_db::capacity(&db).map_err(internal)?;
        let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_batches')",[],|r|r.get(0)).map_err(internal)?;
        if !present {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                "Local batch not owned by this session",
            ));
        }
        let raw:Option<String>=db.query_row("SELECT record FROM library_batches WHERE session=?1 AND request=?2 AND typeof(record)='text' AND length(CAST(record AS BLOB)) BETWEEN 1 AND 1048576",params![owner.as_str(),id.as_str()],|r|r.get(0)).optional().map_err(internal)?;
        let record: Record = decode(&raw.ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, "Local batch not owned by this session")
        })?)?;
        if record.report.batch_request_id != *id {
            return Err(ProtocolError::invalid("Local batch request index differs"));
        }
        let floor = crate::inference_equivalence_reader::version(&db).map_err(internal)?;
        if (record.report.cancellation.is_some() && floor < 29)
            || (matches!(record.options.recipe, LibraryBatchRecipe::Visual(_)) && floor < 30)
        {
            return Err(ProtocolError::invalid(
                "Local batch cancellation reader floor downgraded",
            ));
        }
        checked_pack_membership(&db, &record, owner.as_str(), id.as_str()).map_err(internal)?;
        checked_migration_membership(&db, &record, owner.as_str(), id.as_str())
            .map_err(internal)?;
        checked(&record, &self.config.state_dir)?;
        if floor >= 30 {
            let digest: Option<String> = db
                .query_row(
                    "SELECT sha256 FROM library_batch_models WHERE session=?1 AND request=?2",
                    params![owner.as_str(), id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if digest.as_deref()
                != record
                    .model_capture
                    .as_ref()
                    .map(|model| model.sha256.as_str())
            {
                return Err(ProtocolError::invalid("Batch model membership differs"));
            }
        }
        Ok(record)
    }
    fn library_save(&self, request: &Request, record: &Record) -> PResult<()> {
        checked(record, &self.config.state_dir)?;
        let raw = encode(record)?;
        if raw.len() > crate::library_batch_db::MAX_RECORD {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Local batch record exceeds capacity",
            ));
        }
        let owner = request.session.as_ref().expect("authenticated");
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if record.report.cancellation.is_some() {
            crate::library_batch_db::arm_cancel(&tx).map_err(internal)?;
        }
        tx.execute(
            "UPDATE library_batches SET record=?3 WHERE session=?1 AND request=?2",
            params![owner.as_str(), record.report.batch_request_id.as_str(), raw],
        )
        .map_err(internal)?;
        crate::library_batch_db::capacity(&tx).map_err(internal)?;
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(request).map_err(internal)?)
        );
        claim_request_identity(&tx, owner.as_str(), request, &fingerprint)?;
        tx.commit().map_err(internal)?;
        Ok(())
    }
    pub(super) fn library_batch_control(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let _admission = self.pool.reserve_memory(64 * 1024 * 1024)?;
        if request.auth_token.is_none() {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "Local batch control requires current client bearer authority",
            ));
        }
        if request.project.is_some() || request.expected_revision.is_some() {
            return Err(ProtocolError::invalid(
                "Local batch control has no project scope",
            ));
        }
        let _gate = self.library_batch_gate.try_lock().map_err(|_| {
            ProtocolError::new(
                ErrorCode::RequestConflict,
                "Another local batch control owns advancement",
            )
        })?;
        let owner = request.session.as_ref().expect("authenticated");
        match &request.command {
            Command::LibraryBatchMigrate {
                batch_request_id,
                expected_report_sha256,
                recipe,
            } => self.library_migrate(
                request,
                fingerprint,
                batch_request_id,
                expected_report_sha256,
                recipe,
            ),
            Command::LibraryBatchPrepare { options } => {
                options.validate()?;
                let mut options = options.clone();
                // Validate every destination before the first input copy.
                let state = self.config.state_dir.canonicalize().map_err(unavailable)?;
                let mut sources = std::collections::BTreeSet::new();
                let mut destinations = std::collections::BTreeSet::new();
                for input in &mut options.inputs {
                    input.source = input.source.canonicalize().map_err(unavailable)?;
                    let parent = input
                        .destination
                        .parent()
                        .ok_or_else(|| ProtocolError::invalid("Destination parent absent"))?
                        .canonicalize()
                        .map_err(unavailable)?;
                    input.destination =
                        parent.join(input.destination.file_name().ok_or_else(|| {
                            ProtocolError::invalid("Destination filename absent")
                        })?);
                    if !parent.is_dir()
                        || input.destination.starts_with(&state)
                        || !sources.insert(input.source.clone())
                        || !destinations.insert(input.destination.clone())
                    {
                        return Err(ProtocolError::invalid(
                            "Batch selected paths alias or target Engine storage",
                        ));
                    }
                    if let Some(identity) = &input.publication_directory {
                        artifacts::check_publication_directory(&input.destination, identity)
                            .map_err(unavailable)?;
                    } else {
                        match fs::symlink_metadata(&input.destination) {
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => return Err(unavailable(e)),
                            Ok(_) => {
                                return Err(ProtocolError::new(
                                    ErrorCode::RequestConflict,
                                    "Batch destination exists; nothing overwritten",
                                ))
                            }
                        }
                    }
                }
                options.validate()?;
                let mut db = self.db.lock().map_err(internal)?;
                self.authorize(&db, request, owner)?;
                // Early capacity check avoids needless source copying at the retained limit.
                crate::library_batch_db::capacity(&db).map_err(internal)?;
                let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_batches')",[],|r|r.get(0)).map_err(internal)?;
                if present {
                    let count: i64 = db
                        .query_row("SELECT count(*) FROM library_batches", [], |r| r.get(0))
                        .map_err(internal)?;
                    if count >= 128 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "Local batch retained capacity reached",
                        ));
                    }
                }
                drop(db);
                let _artifacts = self.artifact_gate.lock().map_err(internal)?;
                let (model_capture, pack_documents) =
                    self.library_capture_recipe(&mut options.recipe)?;
                let visual = matches!(options.recipe, LibraryBatchRecipe::Visual(_));
                let mut captures = Vec::new();
                let mut items = Vec::new();
                for input in &options.inputs {
                    let (path, sha256, bytes) = artifacts::snapshot_reserved(
                        &input.source,
                        &self.config.state_dir.join("snapshots"),
                        self.config.max_snapshot_bytes,
                        self.pool.reserved_storage(),
                    )
                    .map_err(unavailable)?;
                    if bytes == 0 {
                        return Err(ProtocolError::invalid("Batch source is empty"));
                    }
                    items.push(LibraryBatchItem {
                        source: input.source.clone(),
                        destination: input.destination.clone(),
                        source_sha256: sha256.clone(),
                        source_bytes: bytes,
                        publication_directory: input.publication_directory,
                        publication_axes: if visual {
                            Some(vec![Axis::Stroke])
                        } else {
                            input.publication_axes.clone()
                        },
                        phase: Phase::Selected,
                        project_id: None,
                        source_version: None,
                        revision: RevisionId::new(0),
                        job_id: None,
                        candidate_id: None,
                        effects: vec![],
                        error: None,
                    });
                    captures.push(Capture {
                        path,
                        sha256,
                        bytes,
                    });
                }
                let record = Record {
                    options: options.clone(),
                    worker_stamp: self.library_worker_stamp()?,
                    model_capture,
                    pack_documents,
                    report: LibraryBatchReport {
                        migration: None,
                        successor_batch_request_id: None,
                        model_pack: options.recipe.model_pack().cloned().map(Box::new),
                        schema: options.publication_schema(false),
                        batch_request_id: request.request_id.clone(),
                        items,
                        pending_advance_request_id: None,
                        cancellation: None,
                        generation_ready: false,
                        publication_completed: false,
                        qualified: false,
                    },
                    captures,
                    pending: None,
                };
                checked(&record, &self.config.state_dir)?;
                let raw = encode(&record)?;
                if raw.len() > crate::library_batch_db::MAX_RECORD {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "Local batch record exceeds capacity",
                    ));
                }
                db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.authorize(&tx, request, owner)?;
                if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
                    return Ok(body);
                }
                if let Some(choice) = record.options.recipe.model_pack() {
                    self.require_current_pack_choice(&tx, choice)?;
                    crate::library_batch_db::arm_pack(&tx).map_err(internal)?;
                } else if record.report.schema == 6 {
                    crate::library_batch_db::arm_partial_audio(&tx).map_err(internal)?;
                } else if record.report.schema == 5 {
                    crate::library_batch_db::arm_selected_audio(&tx).map_err(internal)?;
                } else if visual {
                    crate::library_batch_db::arm_visual(&tx).map_err(internal)?;
                } else {
                    crate::library_batch_db::arm(&tx).map_err(internal)?;
                }
                tx.execute(
                    "INSERT INTO library_batches VALUES(?1,?2,?3)",
                    params![owner.as_str(), request.request_id.as_str(), raw],
                )
                .map_err(internal)?;
                if let Some(model) = &record.model_capture {
                    tx.execute(
                        "INSERT INTO library_batch_models VALUES(?1,?2,?3)",
                        params![owner.as_str(), request.request_id.as_str(), model.sha256],
                    )
                    .map_err(internal)?;
                }
                for (index, document) in record.pack_documents.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO library_batch_pack_documents VALUES(?1,?2,?3,?4)",
                        params![
                            owner.as_str(),
                            request.request_id.as_str(),
                            index,
                            document.sha256
                        ],
                    )
                    .map_err(internal)?;
                }
                for (index, capture) in record.captures.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO library_batch_sources VALUES(?1,?2,?3,?4)",
                        params![
                            owner.as_str(),
                            request.request_id.as_str(),
                            index,
                            capture.sha256
                        ],
                    )
                    .map_err(internal)?;
                }
                crate::library_batch_db::capacity(&tx).map_err(internal)?;
                let body = ResponseBody::LibraryBatch(record.report);
                remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Command::LibraryBatchList => {
                let ids = {
                    let db = self.db.lock().map_err(internal)?;
                    crate::library_batch_db::capacity(&db).map_err(internal)?;
                    let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='library_batches')",[],|r|r.get(0)).map_err(internal)?;
                    if !present {
                        return Ok(ResponseBody::LibraryBatchCatalog(LibraryBatchCatalog {
                            batches: vec![],
                            qualified: false,
                        }));
                    }
                    let mut statement=db.prepare("SELECT request FROM library_batches WHERE session=?1 ORDER BY request LIMIT 129").map_err(internal)?;
                    let ids = statement
                        .query_map([owner.as_str()], |r| r.get::<_, String>(0))
                        .map_err(internal)?
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(internal)?;
                    if ids.len() > 128 {
                        return Err(ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "Local batch catalog exceeds capacity",
                        ));
                    }
                    ids
                };
                let mut batches = Vec::new();
                for id in ids {
                    let record =
                        self.library_load(owner, &RequestId::new(id).map_err(internal)?)?;
                    batches.push(LibraryBatchSummary {
                        successor_batch_request_id: record.report.successor_batch_request_id,
                        batch_request_id: record.report.batch_request_id,
                        first_source: record.report.items[0].source.clone(),
                        item_count: record.report.items.len() as u16,
                        cancellation_requested: record.report.cancellation.is_some(),
                        generation_ready: record.report.generation_ready,
                    });
                }
                Ok(ResponseBody::LibraryBatchCatalog(LibraryBatchCatalog {
                    batches,
                    qualified: false,
                }))
            }
            Command::LibraryBatchStatus { batch_request_id } => Ok(ResponseBody::LibraryBatch(
                self.library_load(owner, batch_request_id)?.report,
            )),
            Command::LibraryBatchCancel { batch_request_id } => {
                self.library_cancel(request, fingerprint, batch_request_id)
            }
            Command::LibraryBatchRelease { batch_request_id } => {
                let record = self.library_load(owner, batch_request_id)?;
                // Never discard an unresolved inner acknowledgement or an active generation chain.
                let cancelled = record
                    .report
                    .cancellation
                    .as_ref()
                    .is_some_and(|c| c.completed);
                if record.pending.is_some()
                    || record.report.pending_advance_request_id.is_some()
                    || (!cancelled
                        && record.report.successor_batch_request_id.is_none()
                        && record.report.items.iter().any(|item| {
                            item.error.is_some()
                                || !matches!(
                                    item.phase,
                                    Phase::Selected | Phase::ReadyForPublication
                                )
                        }))
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Local batch has unfinished or unresolved effects; reconcile before release",
                    ));
                }
                if cancelled || record.report.successor_batch_request_id.is_some() {
                    let db = self.db.lock().map_err(internal)?;
                    let workers = self.workers.lock().map_err(internal)?;
                    for item in &record.report.items {
                        if let Some(job) = &item.job_id {
                            let current =
                                job_snapshot(&db, job, item.project_id.as_ref().expect("checked"))?;
                            if !matches!(
                                current.state.as_str(),
                                "completed" | "failed" | "cancelled"
                            ) || workers.contains_key(job.as_str())
                            {
                                return Err(ProtocolError::new(
                                    ErrorCode::RequestConflict,
                                    "Local batch job remains active; retain metadata",
                                ));
                            }
                        }
                    }
                }
                let body = ResponseBody::LibraryBatchReleased(LibraryBatchReleaseReceipt {
                    released_source_memberships: (record.captures.len()
                        + usize::from(record.model_capture.is_some())
                        + record.pack_documents.len())
                        as u16,
                    report: record.report,
                });
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.authorize(&tx, request, owner)?;
                let floor = crate::inference_equivalence_reader::version(&tx).map_err(internal)?;
                if floor >= 32 {
                    let retained:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM library_batch_migrations WHERE session=?1 AND parent_request=?2)",params![owner.as_str(),batch_request_id.as_str()],|r|r.get(0)).map_err(internal)?;
                    if retained {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "Release successor metadata before its original batch",
                        ));
                    }
                    tx.execute(
                        "DELETE FROM library_batch_migrations WHERE session=?1 AND request=?2",
                        params![owner.as_str(), batch_request_id.as_str()],
                    )
                    .map_err(internal)?;
                }
                if record.model_capture.is_some() {
                    tx.execute(
                        "DELETE FROM library_batch_models WHERE session=?1 AND request=?2",
                        params![owner.as_str(), batch_request_id.as_str()],
                    )
                    .map_err(internal)?;
                }
                if !record.pack_documents.is_empty() {
                    tx.execute(
                        "DELETE FROM library_batch_pack_documents WHERE session=?1 AND request=?2",
                        params![owner.as_str(), batch_request_id.as_str()],
                    )
                    .map_err(internal)?;
                }
                tx.execute(
                    "DELETE FROM library_batch_sources WHERE session=?1 AND request=?2",
                    params![owner.as_str(), batch_request_id.as_str()],
                )
                .map_err(internal)?;
                let removed = tx
                    .execute(
                        "DELETE FROM library_batches WHERE session=?1 AND request=?2",
                        params![owner.as_str(), batch_request_id.as_str()],
                    )
                    .map_err(internal)?;
                if removed != 1 {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Local batch release ownership changed",
                    ));
                }
                crate::library_batch_db::capacity(&tx).map_err(internal)?;
                // Metadata removal and the exact replay receipt have one transaction boundary.
                remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Command::LibraryBatchAdvance { batch_request_id } => {
                let mut record = self.library_load(owner, batch_request_id)?;
                if record.report.successor_batch_request_id.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Original batch was migrated; run its explicit successor",
                    ));
                }
                if record.report.cancellation.is_some() {
                    return Err(ProtocolError::new(
                        ErrorCode::Cancelled,
                        "Local batch cancellation requested; no further generation dispatched",
                    ));
                }
                if record
                    .report
                    .pending_advance_request_id
                    .as_ref()
                    .is_some_and(|id| id != &request.request_id)
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Reconcile the retained advance request identity first",
                    ));
                }
                let index = record
                    .report
                    .items
                    .iter()
                    .position(|i| i.phase != Phase::ReadyForPublication);
                if let Some(index) = index {
                    if record.worker_stamp != self.library_worker_stamp()? {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "Local batch tool identity changed; no replacement work dispatched",
                        ));
                    }
                    let result = self.library_step(request, &mut record, index);
                    if let Err(error) = result {
                        record.report.items[index].error = Some(error);
                    }
                    record.report.generation_ready = record
                        .report
                        .items
                        .iter()
                        .all(|i| i.phase == Phase::ReadyForPublication);
                }
                record.report.pending_advance_request_id = None;
                checked(&record, &self.config.state_dir)?;
                let raw = encode(&record)?;
                if raw.len() > crate::library_batch_db::MAX_RECORD {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "Local batch record exceeds capacity",
                    ));
                }
                let body = ResponseBody::LibraryBatch(record.report);
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.authorize(&tx, request, owner)?;
                tx.execute(
                    "UPDATE library_batches SET record=?3 WHERE session=?1 AND request=?2",
                    params![owner.as_str(), batch_request_id.as_str(), raw],
                )
                .map_err(internal)?;
                crate::library_batch_db::capacity(&tx).map_err(internal)?;
                remember(&tx, owner.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            _ => Err(ProtocolError::unsupported("local batch command")),
        }
    }
    fn library_cancel(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        batch: &RequestId,
    ) -> PResult<ResponseBody> {
        let owner = request.session.as_ref().expect("authenticated");
        let mut record = self.library_load(owner, batch)?;
        if record.report.successor_batch_request_id.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Migrated original batch is immutable; cancel the successor",
            ));
        }
        if let Some(cancel) = &record.report.cancellation {
            if cancel.request_id != request.request_id {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "Retry the retained cancellation request identity",
                ));
            }
        } else {
            record.report.schema = record.options.publication_schema(true);
            record.report.cancellation = Some(LibraryBatchCancellation {
                request_id: request.request_id.clone(),
                jobs: Vec::new(),
                resolved_generation_advance_request_id: None,
                completed: false,
                worker_retirement_verified: false,
                error: None,
            });
        }
        record.report.cancellation.as_mut().expect("set").error = None;
        // Claim stop authority before dispatch. An unresolved original phase is never executed.
        self.library_save(request, &record)?;
        let work = (|| -> PResult<()> {
            if let Some((index, pending)) = record.pending.clone() {
                let mut inner = Request::new(pending.id, pending.command)
                    .with_session(owner.clone())
                    .with_auth_token(request.auth_token.clone().expect("checked"));
                inner.project = pending.project;
                inner.expected_revision = pending.revision;
                let hash = format!(
                    "{:x}",
                    Sha256::digest(request_fingerprint(&inner).map_err(internal)?)
                );
                let ack = {
                    let db = self.db.lock().map_err(internal)?;
                    replay(&db, owner.as_str(), &inner, &hash)?
                };
                let ack = ack.ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "Original generation acknowledgement unresolved; no replacement dispatched",
                    )
                })?;
                Self::library_apply_ack(&mut record, index, ack)?;
                let original = record.report.pending_advance_request_id.take();
                record
                    .report
                    .cancellation
                    .as_mut()
                    .expect("set")
                    .resolved_generation_advance_request_id = original;
                record.report.generation_ready = record
                    .report
                    .items
                    .iter()
                    .all(|item| item.phase == Phase::ReadyForPublication);
                self.library_save(request, &record)?;
            }
            for item in &record.report.items {
                if item.phase == Phase::Generating {
                    let cancel = record.report.cancellation.as_mut().expect("set");
                    let job = item.job_id.clone().expect("checked");
                    if !cancel.jobs.iter().any(|effect| effect.job_id == job) {
                        cancel.jobs.push(LibraryBatchCancelEffect {
                            request_id: id!(RequestId),
                            project_id: item.project_id.clone().expect("checked"),
                            job_id: job,
                            base_revision: item.revision,
                            acknowledgement: None,
                        });
                    }
                }
            }
            self.library_save(request, &record)?;
            let count = record.report.cancellation.as_ref().expect("set").jobs.len();
            for index in 0..count {
                let effect = record.report.cancellation.as_ref().expect("set").jobs[index].clone();
                if effect.acknowledgement.is_some() {
                    continue;
                }
                let mut inner = Request::new(
                    effect.request_id.clone(),
                    Command::CancelJob {
                        job_id: effect.job_id.clone(),
                    },
                )
                .with_session(owner.clone())
                .with_auth_token(request.auth_token.clone().expect("checked"));
                inner.project = Some(effect.project_id.clone());
                inner.expected_revision = Some(effect.base_revision);
                let ResponseBody::Job(ack) = self.dispatch(&inner)? else {
                    return Err(ProtocolError::invalid("Cancellation reply differs"));
                };
                if ack.job_id != effect.job_id
                    || ack.project_id != effect.project_id
                    || ack.base_revision != effect.base_revision
                    || !matches!(ack.state.as_str(), "completed" | "failed" | "cancelled")
                {
                    return Err(ProtocolError::invalid(
                        "Cancellation acknowledgement differs",
                    ));
                }
                record.report.cancellation.as_mut().expect("set").jobs[index].acknowledgement =
                    Some(ack);
                self.library_save(request, &record)?;
            }
            Ok(())
        })();
        match work {
            Ok(()) => record.report.cancellation.as_mut().expect("set").completed = true,
            Err(error) => record.report.cancellation.as_mut().expect("set").error = Some(error),
        }
        checked(&record, &self.config.state_dir)?;
        let raw = encode(&record)?;
        if raw.len() > crate::library_batch_db::MAX_RECORD {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Local batch record exceeds capacity",
            ));
        }
        let completed = record.report.cancellation.as_ref().expect("set").completed;
        let body = ResponseBody::LibraryBatch(record.report);
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        tx.execute(
            "UPDATE library_batches SET record=?3 WHERE session=?1 AND request=?2",
            params![owner.as_str(), batch.as_str(), raw],
        )
        .map_err(internal)?;
        crate::library_batch_db::capacity(&tx).map_err(internal)?;
        if completed {
            remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        }
        tx.commit().map_err(internal)?;
        Ok(body)
    }
    fn library_step(
        self: &Arc<Self>,
        request: &Request,
        record: &mut Record,
        index: usize,
    ) -> PResult<()> {
        let owner = request.session.as_ref().expect("authenticated");
        let execute = |pending: &Pending| {
            let mut inner = Request::new(pending.id.clone(), pending.command.clone())
                .with_session(owner.clone())
                .with_auth_token(request.auth_token.clone().ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Unauthorized,
                        "Local batch advance requires current bearer authority",
                    )
                })?);
            inner.project = pending.project.clone();
            inner.expected_revision = pending.revision;
            self.dispatch(&inner)
        };
        if record.report.items[index].phase == Phase::SourceImported {
            for document in &record.pack_documents {
                let (digest, bytes) =
                    artifacts::digest_file(&document.path, self.config.max_snapshot_bytes)
                        .map_err(unavailable)?;
                if digest != document.sha256 || bytes != document.bytes {
                    return Err(ProtocolError::invalid(
                        "Captured pack document bytes changed",
                    ));
                }
            }
            if let Some(model) = &record.model_capture {
                let (digest, bytes) =
                    artifacts::digest_file(&model.path, self.config.max_snapshot_bytes)
                        .map_err(unavailable)?;
                if digest != model.sha256 || bytes != model.bytes {
                    return Err(ProtocolError::invalid("Captured batch model bytes changed"));
                }
            }
        }
        if record.report.items[index].phase == Phase::Generating {
            let item = &record.report.items[index];
            let job = item
                .job_id
                .clone()
                .ok_or_else(|| ProtocolError::invalid("Local batch job absent"))?;
            let status = execute(&Pending {
                id: id!(RequestId),
                command: Command::JobStatus {
                    job_id: job.clone(),
                },
                project: item.project_id.clone(),
                revision: None,
            })?;
            let ResponseBody::Job(status) = status else {
                return Err(ProtocolError::invalid("Local batch job response differs"));
            };
            if status.project_id != item.project_id.clone().expect("checked project")
                || status.base_revision != item.revision
                || status.job_id != job
            {
                return Err(ProtocolError::invalid("Local batch job binding differs"));
            }
            if status.state == "completed" {
                record.report.items[index].candidate_id = Some(
                    status
                        .candidate_id
                        .ok_or_else(|| ProtocolError::invalid("Completed job lacks candidate"))?,
                );
                record.report.items[index].phase = Phase::CandidateReady;
                record.report.items[index].error = None;
            } else if matches!(
                status.state.as_str(),
                "failed" | "cancelled" | "interrupted"
            ) {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "Recorded local batch job needs recovery; no replacement job created",
                ));
            }
            return Ok(());
        }
        if record.pending.is_none() {
            // Refuse an already stale choice before recording a new generation
            // effect. Dispatch still checks the selection transactionally; an
            // existing pending request must retain its original identity.
            if record.report.items[index].phase == Phase::SourceImported {
                if let Some(choice) = record.options.recipe.model_pack() {
                    let db = self.db.lock().map_err(internal)?;
                    self.authorize(&db, request, owner)?;
                    self.require_current_pack_choice(&db, choice)?;
                }
            }
            let (project, revision, command) = phase_command(record, index)?;
            let id = id!(RequestId);
            let phase = record.report.items[index].phase;
            record.report.items[index].effects.push(LibraryBatchEffect {
                request_id: id.clone(),
                phase,
            });
            record.pending = Some((
                index,
                Pending {
                    id,
                    command,
                    project,
                    revision,
                },
            ));
        }
        record.report.pending_advance_request_id = Some(request.request_id.clone());
        self.library_save(request, record)?;
        let (pending_index, pending) = record.pending.clone().expect("persisted pending");
        if pending_index != index {
            return Err(ProtocolError::invalid("Local batch pending item differs"));
        }
        let body = execute(&pending)?;
        Self::library_apply_ack(record, index, body)
    }
    fn library_apply_ack(record: &mut Record, index: usize, body: ResponseBody) -> PResult<()> {
        let generation_axes: &[Axis] =
            if matches!(record.options.recipe, LibraryBatchRecipe::Visual(_)) {
                &[Axis::Stroke]
            } else {
                &Axis::ALL
            };
        let item = &mut record.report.items[index];
        match (item.phase, body) {
            (Phase::Selected, ResponseBody::Project(p)) => {
                item.project_id = Some(p.project_id);
                item.revision = p.revision;
                item.phase = Phase::ProjectCreated;
            }
            (Phase::ProjectCreated, ResponseBody::Source { source_version }) => {
                if source_version.as_str() != item.source_sha256 {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "Captured batch source acknowledgement differs",
                    ));
                }
                item.source_version = Some(source_version);
                item.phase = Phase::SourceImported;
            }
            (Phase::SourceImported, ResponseBody::Job(job)) => {
                if Some(&job.project_id) != item.project_id.as_ref()
                    || job.base_revision != item.revision
                {
                    return Err(ProtocolError::invalid(
                        "Batch generation acknowledgement differs",
                    ));
                }
                item.job_id = Some(job.job_id);
                item.phase = Phase::Generating;
            }
            (Phase::CandidateReady, ResponseBody::Project(p)) => {
                if Some(&p.project_id) != item.project_id.as_ref()
                    || p.motion.program.axes.len() != generation_axes.len()
                    || p.motion
                        .program
                        .axes
                        .iter()
                        .map(|axis| axis.axis)
                        .collect::<std::collections::BTreeSet<_>>()
                        != generation_axes.iter().copied().collect()
                {
                    return Err(ProtocolError::invalid("Batch committed project differs"));
                }
                item.revision = p.revision;
                item.phase = Phase::ReadyForPublication;
            }
            _ => {
                return Err(ProtocolError::invalid(
                    "Local batch phase acknowledgement differs",
                ))
            }
        }
        item.error = None;
        record.pending = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn existing_directory_capture_refuses_replacement_before_generation_effects() {
        let (dir, engine, owner, token) = setup();
        let mut options = options(dir.path());
        std::fs::create_dir(&options.inputs[0].destination).unwrap();
        let identity =
            artifacts::publication_directory_identity(&options.inputs[0].destination).unwrap();
        options.inputs[0].publication_directory = Some(identity);
        options.inputs[0].publication_axes = Some(vec![Axis::Roll]);
        let prepared = prepare(&engine, &owner, &token, options.clone());
        assert_eq!(prepared.schema, 6);
        assert_eq!(prepared.items[0].publication_directory, Some(identity));
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            34
        );
        std::fs::rename(&options.inputs[0].destination, dir.path().join("retained")).unwrap();
        std::fs::create_dir(&options.inputs[0].destination).unwrap();
        let response = engine.handle(req(
            &owner,
            &token,
            id!(RequestId),
            Command::LibraryBatchPrepare { options },
        ));
        assert!(response.result.is_err());
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM library_batches", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn selected_audio_capture_restart_and_cancel_preserve_axes_and_reader_floor() {
        let (dir, engine, owner, token) = setup();
        let mut options = options(dir.path());
        options.inputs[0].publication_axes = Some(Axis::ALL[1..].to_vec());
        let report = prepare(&engine, &owner, &token, options);
        assert_eq!(report.schema, 5);
        assert_eq!(report.items[0].publication_axes(), &Axis::ALL[1..]);
        let raw: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw, 33);
        let config = engine.config.clone();
        drop(engine);
        let reopened = Engine::open(config).unwrap();
        let ResponseBody::LibraryBatch(restored) = reopened
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchStatus {
                    batch_request_id: report.batch_request_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("status")
        };
        assert_eq!(
            restored.report_sha256().unwrap(),
            report.report_sha256().unwrap()
        );
        let stopped = cancel(
            &reopened,
            &owner,
            &token,
            &report.batch_request_id,
            id!(RequestId),
        );
        assert_eq!(stopped.schema, 5);
        stopped.validate_publication_shape().unwrap();
        assert_eq!(stopped.items[0].publication_axes(), &Axis::ALL[1..]);
    }
    fn setup() -> (tempfile::TempDir, Arc<Engine>, SessionId, String) {
        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("authored-ledger-worker");
        fs::write(&worker, b"authored worker identity, not an executable").unwrap();
        let engine = Engine::open(EngineConfig::new(dir.path().join("state"), worker)).unwrap();
        let (owner, token) = pair(&engine);
        (dir, engine, owner, token)
    }
    fn pair(engine: &Arc<Engine>) -> (SessionId, String) {
        let ResponseBody::Paired {
            session,
            auth_token,
            ..
        } = engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "local-batch-fixture".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("pair");
        };
        (session, auth_token)
    }
    fn options(root: &Path) -> LibraryBatchOptions {
        let source = root.join("input.wav");
        fs::write(&source, b"captured authored input").unwrap();
        let recipe:AudioBundleRecipe=serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap();
        LibraryBatchOptions {
            inputs: vec![LibraryBatchInput {
                publication_directory: None,
                publication_axes: None,
                source,
                destination: root.join("output"),
            }],
            recipe: recipe.into(),
        }
    }
    fn req(owner: &SessionId, token: &str, id: RequestId, command: Command) -> Request {
        Request::new(id, command)
            .with_session(owner.clone())
            .with_auth_token(token.to_owned())
    }
    fn prepare(
        engine: &Arc<Engine>,
        owner: &SessionId,
        token: &str,
        options: LibraryBatchOptions,
    ) -> LibraryBatchReport {
        let ResponseBody::LibraryBatch(report) = engine
            .handle(req(
                owner,
                token,
                id!(RequestId),
                Command::LibraryBatchPrepare { options },
            ))
            .result
            .unwrap()
        else {
            panic!("prepare");
        };
        report
    }
    fn advance(
        engine: &Arc<Engine>,
        owner: &SessionId,
        token: &str,
        batch: &RequestId,
    ) -> LibraryBatchReport {
        let ResponseBody::LibraryBatch(report) = engine
            .handle(req(
                owner,
                token,
                id!(RequestId),
                Command::LibraryBatchAdvance {
                    batch_request_id: batch.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("advance");
        };
        report
    }
    fn cancel(
        engine: &Arc<Engine>,
        owner: &SessionId,
        token: &str,
        batch: &RequestId,
        id: RequestId,
    ) -> LibraryBatchReport {
        let ResponseBody::LibraryBatch(report) = engine
            .handle(req(
                owner,
                token,
                id,
                Command::LibraryBatchCancel {
                    batch_request_id: batch.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("cancel");
        };
        report
    }
    #[test]
    fn visual_capture_survives_restart_and_original_model_replacement() {
        let (dir, engine, owner, token) = setup();
        let model = dir.path().join("authored.onnx");
        fs::write(&model, b"authored model bytes").unwrap();
        let mut opts = options(dir.path());
        let settings = GenerationSettings {
            model_input: Some(ModelInputContract {
                width: 64,
                height: 64,
                channels: 3,
                layout: TensorLayout::Nchw,
                class_count: 1,
                decoder: DetectorDecoder::YoloChannelMajor,
            }),
            ..Default::default()
        };
        opts.recipe = LibraryBatchRecipe::Visual(Box::new(LibraryVisualRecipe {
            kind: LibraryVisualKind::Visual,
            preset: "default".into(),
            settings,
            model_pack: None,
            model_path: Some(model.clone()),
        }));
        let report = prepare(&engine, &owner, &token, opts);
        assert_eq!(report.schema, 3);
        assert_eq!(report.items[0].publication_axes(), &[Axis::Stroke]);
        let record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        let captured = record.model_capture.unwrap();
        fs::write(model, b"external replacement").unwrap();
        assert_eq!(fs::read(&captured.path).unwrap(), b"authored model bytes");
        let config = engine.config.clone();
        drop(engine);
        let engine = Engine::open(config).unwrap();
        let record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        assert_eq!(record.model_capture.unwrap().sha256, captured.sha256);
        assert!(crate::library_batch_db::is_retained_source(
            &engine.db.lock().unwrap(),
            &captured.sha256
        )
        .unwrap());
        let stopped = cancel(
            &engine,
            &owner,
            &token,
            &report.batch_request_id,
            id!(RequestId),
        );
        assert_eq!(stopped.schema, 3);
        assert!(stopped.cancellation.unwrap().completed);
        assert_eq!(
            engine
                .db
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            30
        );
    }
    #[test]
    fn cancellation_is_durable_owner_scoped_replayable_and_releases_partial_metadata() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let created = advance(&engine, &owner, &token, &report.batch_request_id);
        let (other, other_token) = pair(&engine);
        assert_eq!(
            engine
                .handle(req(
                    &other,
                    &other_token,
                    id!(RequestId),
                    Command::LibraryBatchCancel {
                        batch_request_id: report.batch_request_id.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let id = id!(RequestId);
        let stopped = cancel(
            &engine,
            &owner,
            &token,
            &report.batch_request_id,
            id.clone(),
        );
        assert!(stopped.cancellation.as_ref().unwrap().completed);
        assert!(
            !stopped
                .cancellation
                .as_ref()
                .unwrap()
                .worker_retirement_verified
        );
        assert_eq!(stopped.items[0].project_id, created.items[0].project_id);
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        assert_eq!(
            encode(&stopped).unwrap(),
            encode(&cancel(
                &engine,
                &owner,
                &token,
                &report.batch_request_id,
                id
            ))
            .unwrap()
        );
        assert_eq!(
            engine
                .handle(req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchAdvance {
                        batch_request_id: report.batch_request_id.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            29
        );
        drop(db);
        assert!(matches!(
            engine
                .handle(req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchRelease {
                        batch_request_id: report.batch_request_id.clone()
                    }
                ))
                .result
                .unwrap(),
            ResponseBody::LibraryBatchReleased(_)
        ));
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(dir
            .path()
            .join("state/snapshots")
            .join(&report.items[0].source_sha256)
            .exists());
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn cancellation_retains_unknown_pending_and_recovers_only_cached_generation_ack() {
        for cached in [false, true] {
            let (dir, engine, owner, token) = setup();
            let report = prepare(&engine, &owner, &token, options(dir.path()));
            let original = req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchAdvance {
                    batch_request_id: report.batch_request_id.clone(),
                },
            );
            let mut record = engine
                .library_load(&owner, &report.batch_request_id)
                .unwrap();
            let (project, revision, command) = phase_command(&record, 0).unwrap();
            let pending = Pending {
                id: id!(RequestId),
                project,
                revision,
                command,
            };
            record.report.items[0].effects.push(LibraryBatchEffect {
                request_id: pending.id.clone(),
                phase: Phase::Selected,
            });
            record.pending = Some((0, pending.clone()));
            record.report.pending_advance_request_id = Some(original.request_id.clone());
            engine.library_save(&original, &record).unwrap();
            if cached {
                engine
                    .handle(req(
                        &owner,
                        &token,
                        pending.id.clone(),
                        pending.command.clone(),
                    ))
                    .result
                    .unwrap();
            }
            let id = id!(RequestId);
            let stopped = cancel(
                &engine,
                &owner,
                &token,
                &report.batch_request_id,
                id.clone(),
            );
            let c = stopped.cancellation.as_ref().unwrap();
            assert_eq!(c.completed, cached);
            assert_eq!(c.error.is_some(), !cached);
            assert_eq!(stopped.pending_advance_request_id.is_some(), !cached);
            assert_eq!(
                c.resolved_generation_advance_request_id,
                if cached {
                    Some(original.request_id.clone())
                } else {
                    None
                }
            );
            assert_eq!(
                engine.handle(original).result.unwrap_err().code,
                ErrorCode::Cancelled
            );
            assert_eq!(
                engine
                    .handle(req(
                        &owner,
                        &token,
                        id!(RequestId),
                        Command::LibraryBatchCancel {
                            batch_request_id: report.batch_request_id.clone()
                        }
                    ))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::RequestConflict
            );
            let config = Engine::retire_for_test(engine);
            let engine = Engine::open(config).unwrap();
            assert_eq!(
                encode(&stopped).unwrap(),
                encode(&cancel(
                    &engine,
                    &owner,
                    &token,
                    &report.batch_request_id,
                    id
                ))
                .unwrap()
            );
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                i64::from(cached)
            );
            drop(db);
            Engine::retire_for_test(engine);
        }
    }
    #[test]
    fn known_jobs_cancel_exactly_and_terminal_states_are_preserved_without_retirement_claim() {
        for state in [
            "queued",
            "running",
            "interrupted",
            "failed",
            "completed",
            "cancelled",
        ] {
            let (dir, engine, owner, token) = setup();
            let report = prepare(&engine, &owner, &token, options(dir.path()));
            advance(&engine, &owner, &token, &report.batch_request_id);
            advance(&engine, &owner, &token, &report.batch_request_id);
            let mut record = engine
                .library_load(&owner, &report.batch_request_id)
                .unwrap();
            let item = &mut record.report.items[0];
            let job = id!(JobId);
            let attempt = id!(AttemptId);
            let db = engine.db.lock().unwrap();
            db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,?6,'{}')",params![job.as_str(),attempt.as_str(),item.project_id.as_ref().unwrap().as_str(),owner.as_str(),item.source_version.as_ref().unwrap().as_str(),state]).unwrap();
            db.execute(
                "INSERT INTO attempts VALUES(?1,?2,'{}',?3)",
                params![attempt.as_str(), job.as_str(), state],
            )
            .unwrap();
            drop(db);
            item.job_id = Some(job.clone());
            item.phase = Phase::Generating;
            engine
                .library_save(
                    &req(
                        &owner,
                        &token,
                        id!(RequestId),
                        Command::LibraryBatchAdvance {
                            batch_request_id: report.batch_request_id.clone(),
                        },
                    ),
                    &record,
                )
                .unwrap();
            let flag = Arc::new(AtomicBool::new(false));
            engine
                .workers
                .lock()
                .unwrap()
                .insert(job.as_str().to_owned(), flag.clone());
            let id = id!(RequestId);
            let stopped = cancel(
                &engine,
                &owner,
                &token,
                &report.batch_request_id,
                id.clone(),
            );
            let c = stopped.cancellation.as_ref().unwrap();
            assert!(c.completed);
            assert_eq!(c.jobs.len(), 1);
            let active = matches!(state, "queued" | "running" | "interrupted");
            assert_eq!(
                c.jobs[0].acknowledgement.as_ref().unwrap().state,
                if active { "cancelled" } else { state }
            );
            assert_eq!(flag.load(Ordering::Acquire), active);
            assert_eq!(
                encode(&stopped).unwrap(),
                encode(&cancel(
                    &engine,
                    &owner,
                    &token,
                    &report.batch_request_id,
                    id
                ))
                .unwrap()
            );
            assert_eq!(
                engine
                    .handle(req(
                        &owner,
                        &token,
                        id!(RequestId),
                        Command::LibraryBatchRelease {
                            batch_request_id: report.batch_request_id.clone()
                        }
                    ))
                    .result
                    .unwrap_err()
                    .code,
                ErrorCode::RequestConflict
            );
            engine.workers.lock().unwrap().remove(job.as_str());
            assert!(matches!(
                engine
                    .handle(req(
                        &owner,
                        &token,
                        id!(RequestId),
                        Command::LibraryBatchRelease {
                            batch_request_id: report.batch_request_id.clone()
                        }
                    ))
                    .result
                    .unwrap(),
                ResponseBody::LibraryBatchReleased(_)
            ));
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT count(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            drop(db);
            Engine::retire_for_test(engine);
        }
    }
    #[test]
    fn cancellation_outer_receipt_failure_preserves_stop_and_exact_retry_after_restart() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let id = id!(RequestId);
        let request = req(
            &owner,
            &token,
            id.clone(),
            Command::LibraryBatchCancel {
                batch_request_id: report.batch_request_id.clone(),
            },
        );
        let db = engine.db.lock().unwrap();
        db.execute_batch("CREATE TEMP TRIGGER reject_cancel_outer BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'authored outer receipt failure'); END;").unwrap();
        drop(db);
        assert!(engine
            .handle(request.clone())
            .result
            .unwrap_err()
            .message
            .contains("authored outer receipt failure"));
        let stopped = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap()
            .report;
        assert!(stopped.cancellation.is_some());
        assert!(!stopped.cancellation.as_ref().unwrap().completed);
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        assert_eq!(
            engine
                .handle(req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchAdvance {
                        batch_request_id: report.batch_request_id.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        let done = cancel(&engine, &owner, &token, &report.batch_request_id, id);
        assert!(done.cancellation.unwrap().completed);
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn cancelled_metadata_refuses_downgraded_reader_floor() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        cancel(
            &engine,
            &owner,
            &token,
            &report.batch_request_id,
            id!(RequestId),
        );
        let db = engine.db.lock().unwrap();
        crate::library_batch_db::publish(&db, 27).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            29
        );
        db.pragma_update(None, "user_version", 28).unwrap();
        drop(db);
        assert!(engine
            .library_load(&owner, &report.batch_request_id)
            .is_err());
        let config = Engine::retire_for_test(engine);
        assert!(Engine::open(config).is_err());
    }
    #[test]
    fn release_frees_capacity_without_unpinning_other_batches_with_same_snapshot() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let mut reports = Vec::new();
        for _ in 0..128 {
            reports.push(prepare(&engine, &owner, &token, opts.clone()));
        }
        let refused = engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchPrepare {
                    options: opts.clone(),
                },
            ))
            .result
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::ResourceExhausted);
        let ResponseBody::LibraryBatchReleased(receipt) = engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: reports[0].batch_request_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("release");
        };
        assert_eq!(receipt.released_source_memberships, 1);
        assert!(crate::library_batch_db::is_retained_source(
            &engine.db.lock().unwrap(),
            &reports[0].items[0].source_sha256
        )
        .unwrap());
        assert_eq!(
            engine
                .library_load(&owner, &reports[1].batch_request_id)
                .unwrap()
                .report
                .items[0]
                .source_sha256,
            reports[0].items[0].source_sha256
        );
        prepare(&engine, &owner, &token, opts);
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM library_batches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            128
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM library_batch_sources", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            128
        );
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn unstarted_release_is_owner_scoped_replayable_and_preserves_bytes_across_restart() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let report = prepare(&engine, &owner, &token, opts.clone());
        let snapshot = engine
            .config
            .state_dir
            .join("snapshots")
            .join(&report.items[0].source_sha256);
        let destination = &opts.inputs[0].destination;
        fs::create_dir(destination).unwrap();
        fs::write(destination.join("sentinel"), b"preserve user output").unwrap();
        let command = Command::LibraryBatchRelease {
            batch_request_id: report.batch_request_id.clone(),
        };
        let (other, other_token) = pair(&engine);
        assert!(engine
            .handle(req(&other, &other_token, id!(RequestId), command.clone()))
            .result
            .is_err());
        assert!(crate::library_batch_db::is_retained_source(
            &engine.db.lock().unwrap(),
            &report.items[0].source_sha256
        )
        .unwrap());
        let release = req(&owner, &token, id!(RequestId), command.clone());
        let ResponseBody::LibraryBatchReleased(receipt) =
            engine.handle(release.clone()).result.unwrap()
        else {
            panic!("receipt");
        };
        assert_eq!(receipt.released_source_memberships, 1);
        assert!(!receipt.report.publication_completed);
        assert!(!receipt.report.qualified);
        assert_eq!(receipt.report.items[0].phase, Phase::Selected);
        assert!(engine
            .library_load(&owner, &report.batch_request_id)
            .is_err());
        assert!(!crate::library_batch_db::is_retained_source(
            &engine.db.lock().unwrap(),
            &report.items[0].source_sha256
        )
        .unwrap());
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        let ResponseBody::LibraryBatchReleased(replayed) =
            engine.handle(release.clone()).result.unwrap()
        else {
            panic!("replay");
        };
        assert_eq!(encode(&receipt).unwrap(), encode(&replayed).unwrap());
        assert!(engine
            .handle(req(&owner, &token, id!(RequestId), command))
            .result
            .is_err());
        let mut conflict = release;
        conflict.command = Command::LibraryBatchRelease {
            batch_request_id: id!(RequestId),
        };
        assert_eq!(
            engine.handle(conflict).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(fs::read(snapshot).unwrap(), b"captured authored input");
        assert_eq!(
            fs::read(&opts.inputs[0].source).unwrap(),
            b"captured authored input"
        );
        assert_eq!(
            fs::read(destination.join("sentinel")).unwrap(),
            b"preserve user output"
        );
        let next = prepare(
            &engine,
            &owner,
            &token,
            LibraryBatchOptions {
                inputs: vec![LibraryBatchInput {
                    publication_directory: None,
                    publication_axes: None,
                    source: opts.inputs[0].source.clone(),
                    destination: dir.path().join("next-output"),
                }],
                recipe: opts.recipe,
            },
        );
        assert_ne!(next.batch_request_id, report.batch_request_id);
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM library_batches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            28
        );
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn release_refuses_partial_generation_and_pending_inner_ack() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let command = Command::LibraryBatchRelease {
            batch_request_id: report.batch_request_id.clone(),
        };
        let mut record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        let (project, revision, inner) = phase_command(&record, 0).unwrap();
        let pending = Pending {
            id: id!(RequestId),
            command: inner,
            project,
            revision,
        };
        record.report.items[0].effects.push(LibraryBatchEffect {
            request_id: pending.id.clone(),
            phase: Phase::Selected,
        });
        record.pending = Some((0, pending));
        // Known errors may clear the outer identity while retaining the unresolved inner command.
        record.report.items[0].error = Some(ProtocolError::new(
            ErrorCode::Unavailable,
            "authored interruption",
        ));
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchAdvance {
                        batch_request_id: report.batch_request_id.clone(),
                    },
                ),
                &record,
            )
            .unwrap();
        assert_eq!(
            engine
                .handle(req(&owner, &token, id!(RequestId), command.clone()))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        record.pending = None;
        record.report.items[0].error = None;
        record.report.items[0].effects.clear();
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchAdvance {
                        batch_request_id: report.batch_request_id.clone(),
                    },
                ),
                &record,
            )
            .unwrap();
        let created = advance(&engine, &owner, &token, &report.batch_request_id);
        assert_eq!(created.items[0].phase, Phase::ProjectCreated);
        assert_eq!(
            engine
                .handle(req(&owner, &token, id!(RequestId), command))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        assert!(crate::library_batch_db::is_retained_source(
            &engine.db.lock().unwrap(),
            &report.items[0].source_sha256
        )
        .unwrap());
        assert_eq!(
            engine
                .library_load(&owner, &report.batch_request_id)
                .unwrap()
                .report
                .items[0]
                .project_id,
            created.items[0].project_id
        );
        Engine::retire_for_test(engine);
    }
    #[test]
    fn captured_inputs_survive_original_change_and_cannot_be_evicted_through_source_alias() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let report = prepare(&engine, &owner, &token, opts.clone());
        fs::write(
            &opts.inputs[0].source,
            b"changed original must not be consumed",
        )
        .unwrap();
        let created = advance(&engine, &owner, &token, &report.batch_request_id);
        assert_eq!(created.items[0].phase, Phase::ProjectCreated);
        let imported = advance(&engine, &owner, &token, &report.batch_request_id);
        assert_eq!(imported.items[0].phase, Phase::SourceImported);
        let db = engine.db.lock().unwrap();
        let (sha, path): (String, String) = db
            .query_row(
                "SELECT sha256,path FROM sources WHERE project=?1",
                [imported.items[0].project_id.as_ref().unwrap().as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(sha, report.items[0].source_sha256);
        assert_eq!(fs::read(path).unwrap(), b"captured authored input");
        drop(db);
        let mut evict = req(
            &owner,
            &token,
            id!(RequestId),
            Command::EvictSource {
                source_version: imported.items[0].source_version.clone().unwrap(),
            },
        );
        evict.project = imported.items[0].project_id.clone();
        evict.expected_revision = Some(imported.items[0].revision);
        assert_eq!(
            engine.handle(evict).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let raw: String = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT record FROM library_batches", [], |r| r.get(0))
            .unwrap();
        assert!(!raw.contains(&token));
        assert!(!raw.contains(&engine.token));
        assert!(!imported.publication_completed);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn lost_create_ack_reconciles_original_outer_and_inner_ids_after_restart() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let mut record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        let original = req(
            &owner,
            &token,
            id!(RequestId),
            Command::LibraryBatchAdvance {
                batch_request_id: report.batch_request_id.clone(),
            },
        );
        let (project, revision, command) = phase_command(&record, 0).unwrap();
        let pending = Pending {
            id: id!(RequestId),
            command,
            project,
            revision,
        };
        record.report.items[0].effects.push(LibraryBatchEffect {
            request_id: pending.id.clone(),
            phase: Phase::Selected,
        });
        record.pending = Some((0, pending.clone()));
        record.report.pending_advance_request_id = Some(original.request_id.clone());
        engine.library_save(&original, &record).unwrap();
        let ResponseBody::Project(created) = engine
            .handle(req(&owner, &token, pending.id, pending.command))
            .result
            .unwrap()
        else {
            panic!("create");
        };
        let config = Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        let retained = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        assert_eq!(
            retained.report.pending_advance_request_id,
            Some(original.request_id.clone())
        );
        assert_eq!(
            engine
                .handle(req(
                    &owner,
                    &token,
                    id!(RequestId),
                    original.command.clone()
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        let ResponseBody::LibraryBatch(recovered) = engine.handle(original.clone()).result.unwrap()
        else {
            panic!("recover");
        };
        assert_eq!(
            recovered.items[0].project_id,
            Some(created.project_id.clone())
        );
        assert_eq!(recovered.items[0].phase, Phase::ProjectCreated);
        let ResponseBody::LibraryBatch(replayed) = engine.handle(original).result.unwrap() else {
            panic!("replay");
        };
        assert_eq!(replayed.items[0].phase, Phase::ProjectCreated);
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let ResponseBody::LibraryBatchCatalog(catalog) = engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchList,
            ))
            .result
            .unwrap()
        else {
            panic!("list");
        };
        assert_eq!(catalog.batches.len(), 1);
        assert_eq!(catalog.batches[0].batch_request_id, report.batch_request_id);
        let (foreign, foreign_token) = pair(&engine);
        assert_eq!(
            engine
                .handle(req(
                    &foreign,
                    &foreign_token,
                    id!(RequestId),
                    Command::LibraryBatchStatus {
                        batch_request_id: report.batch_request_id
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let ResponseBody::LibraryBatchCatalog(catalog) = engine
            .handle(req(
                &foreign,
                &foreign_token,
                id!(RequestId),
                Command::LibraryBatchList,
            ))
            .result
            .unwrap()
        else {
            panic!("foreign list");
        };
        assert!(catalog.batches.is_empty());
        Engine::retire_for_test(engine);
    }
    #[test]
    fn corrupted_capture_refuses_import_without_attaching_changed_bytes() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&record.captures[0].path, fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        #[cfg(not(unix))]
        {
            let mut permissions = fs::metadata(&record.captures[0].path)
                .unwrap()
                .permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&record.captures[0].path, permissions).unwrap();
        }
        fs::write(&record.captures[0].path, b"corrupt captured bytes").unwrap();
        advance(&engine, &owner, &token, &report.batch_request_id);
        let refused = advance(&engine, &owner, &token, &report.batch_request_id);
        assert_eq!(refused.items[0].phase, Phase::ProjectCreated);
        assert_eq!(
            refused.items[0].error.as_ref().unwrap().code,
            ErrorCode::DependencyMismatch
        );
        let db = engine.db.lock().unwrap();
        let count: i64 = db
            .query_row("SELECT count(*) FROM sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let jobs: i64 = db
            .query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jobs, 0);
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn first_worker_creation_preserves_outer_batch_barrier_and_source_membership() {
        let (dir, engine, owner, token) = setup();
        let report = prepare(&engine, &owner, &token, options(dir.path()));
        let mut db = engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        crate::worker_fence_db::arm_creation(&tx, &uuid::Uuid::new_v4().to_string()).unwrap();
        assert_eq!(
            tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            28
        );
        assert_eq!(crate::library_batch_db::version(&tx).unwrap(), 20);
        assert!(
            crate::library_batch_db::is_retained_source(&tx, &report.items[0].source_sha256)
                .unwrap()
        );
        tx.rollback().unwrap();
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn destination_refusal_and_changed_tool_stamp_create_no_project_or_job() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        fs::write(&opts.inputs[0].destination, b"preserve blocker").unwrap();
        let before = fs::read_dir(engine.config.state_dir.join("snapshots"))
            .unwrap()
            .count();
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchPrepare {
                    options: opts.clone()
                }
            ))
            .result
            .is_err());
        assert_eq!(
            fs::read(&opts.inputs[0].destination).unwrap(),
            b"preserve blocker"
        );
        assert_eq!(
            fs::read_dir(engine.config.state_dir.join("snapshots"))
                .unwrap()
                .count(),
            before
        );
        fs::remove_file(&opts.inputs[0].destination).unwrap();
        let report = prepare(&engine, &owner, &token, opts);
        let mut record = engine
            .library_load(&owner, &report.batch_request_id)
            .unwrap();
        record.worker_stamp = "0".repeat(64);
        let status = req(
            &owner,
            &token,
            id!(RequestId),
            Command::LibraryBatchAdvance {
                batch_request_id: report.batch_request_id.clone(),
            },
        );
        engine.library_save(&status, &record).unwrap();
        assert_eq!(
            engine.handle(status).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn migration_reuses_captured_import_and_replays_after_restart_without_generation() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let prepared = prepare(&engine, &owner, &token, opts.clone());
        let batch = prepared.batch_request_id;
        advance(&engine, &owner, &token, &batch);
        advance(&engine, &owner, &token, &batch);
        let mut parent = engine.library_load(&owner, &batch).unwrap();
        parent.report.items[0].error = Some(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "authored stopped diagnostic",
        ));
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchStatus {
                        batch_request_id: batch.clone(),
                    },
                ),
                &parent,
            )
            .unwrap();
        let observed = parent.report.report_sha256().unwrap();
        let child = id!(RequestId);
        let migration = req(
            &owner,
            &token,
            child.clone(),
            Command::LibraryBatchMigrate {
                batch_request_id: batch.clone(),
                expected_report_sha256: observed.clone(),
                recipe: opts.recipe.clone(),
            },
        );
        fs::write(
            &opts.inputs[0].source,
            b"external replacement must not be imported",
        )
        .unwrap();
        let ResponseBody::LibraryBatch(report) = engine.handle(migration.clone()).result.unwrap()
        else {
            panic!("migration")
        };
        assert_eq!(report.items[0].phase, Phase::SourceImported);
        assert_eq!(
            report.items[0].project_id,
            parent.report.items[0].project_id
        );
        assert_eq!(
            report.items[0].source_version,
            parent.report.items[0].source_version
        );
        assert_eq!(report.items[0].effects.len(), 2);
        assert!(report.items[0].error.is_none());
        assert_eq!(
            report.migration.as_ref().unwrap().parent_report_sha256,
            observed
        );
        assert_eq!(
            fs::read(&engine.library_load(&owner, &child).unwrap().captures[0].path).unwrap(),
            b"captured authored input"
        );
        let current = engine.library_load(&owner, &batch).unwrap();
        assert_eq!(
            current.report.successor_batch_request_id,
            Some(Box::new(child.clone()))
        );
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchAdvance {
                    batch_request_id: batch.clone()
                }
            ))
            .result
            .is_err());
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchCancel {
                    batch_request_id: batch.clone()
                }
            ))
            .result
            .is_err());
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: batch.clone()
                }
            ))
            .result
            .is_err());
        let config = engine.config.clone();
        Engine::retire_for_test(engine);
        let engine = Engine::open(config).unwrap();
        let replay = engine.handle(migration).result.unwrap();
        let ResponseBody::LibraryBatch(replay) = replay else {
            panic!("replay")
        };
        assert_eq!(
            replay.report_sha256().unwrap(),
            report.report_sha256().unwrap()
        );
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM projects", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            32
        );
        drop(db);
        assert!(
            cancel(&engine, &owner, &token, &child, id!(RequestId))
                .cancellation
                .unwrap()
                .completed
        );
        engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: child,
                },
            ))
            .result
            .unwrap();
        engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: batch,
                },
            ))
            .result
            .unwrap();
        Engine::retire_for_test(engine);
    }
    #[test]
    fn migration_refuses_unresolved_effects_accepted_work_stale_observation_and_wrong_owner() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let report = prepare(&engine, &owner, &token, opts.clone());
        let batch = report.batch_request_id;
        let mut parent = engine.library_load(&owner, &batch).unwrap();
        let make = |sha: String| Command::LibraryBatchMigrate {
            batch_request_id: batch.clone(),
            expected_report_sha256: sha,
            recipe: opts.recipe.clone(),
        };
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                make(parent.report.report_sha256().unwrap())
            ))
            .result
            .is_err());
        let (other, other_token) = pair(&engine);
        assert_eq!(
            engine
                .handle(req(
                    &other,
                    &other_token,
                    id!(RequestId),
                    make(parent.report.report_sha256().unwrap())
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        parent.report.items[0].error = Some(ProtocolError::invalid("authored stopped diagnostic"));
        let (project, revision, command) = phase_command(&parent, 0).unwrap();
        let pending = Pending {
            id: id!(RequestId),
            command,
            project,
            revision,
        };
        parent.report.items[0].effects.push(LibraryBatchEffect {
            request_id: pending.id.clone(),
            phase: Phase::Selected,
        });
        parent.pending = Some((0, pending));
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchStatus {
                        batch_request_id: batch.clone(),
                    },
                ),
                &parent,
            )
            .unwrap();
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                make(parent.report.report_sha256().unwrap())
            ))
            .result
            .is_err());
        assert!(engine
            .library_load(&owner, &batch)
            .unwrap()
            .pending
            .is_some());
        parent.pending = None;
        parent.report.items[0].effects.clear();
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchStatus {
                        batch_request_id: batch.clone(),
                    },
                ),
                &parent,
            )
            .unwrap();
        assert!(engine
            .handle(req(&owner, &token, id!(RequestId), make("0".repeat(64))))
            .result
            .is_err());
        advance(&engine, &owner, &token, &batch);
        advance(&engine, &owner, &token, &batch);
        let mut parent = engine.library_load(&owner, &batch).unwrap();
        parent.report.items[0].phase = Phase::Generating;
        parent.report.items[0].job_id = Some(id!(JobId));
        parent.report.items[0].error = Some(ProtocolError::invalid("authored accepted phase"));
        engine
            .library_save(
                &req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchStatus {
                        batch_request_id: batch.clone(),
                    },
                ),
                &parent,
            )
            .unwrap();
        assert!(engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                make(parent.report.report_sha256().unwrap())
            ))
            .result
            .is_err());
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM library_batches", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            28
        );
        drop(db);
        Engine::retire_for_test(engine);
    }
    #[test]
    fn migration_preserves_failed_job_receipt_and_resets_only_successor_generation() {
        let (dir, engine, owner, token) = setup();
        let opts = options(dir.path());
        let report = prepare(&engine, &owner, &token, opts.clone());
        let batch = report.batch_request_id;
        advance(&engine, &owner, &token, &batch);
        advance(&engine, &owner, &token, &batch);
        let accepted = advance(&engine, &owner, &token, &batch);
        assert_eq!(accepted.items[0].phase, Phase::Generating);
        let job = accepted.items[0].job_id.clone().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let db = engine.db.lock().unwrap();
            let state =
                job_snapshot(&db, &job, accepted.items[0].project_id.as_ref().unwrap()).unwrap();
            let retired = !engine.workers.lock().unwrap().contains_key(job.as_str());
            drop(db);
            if state.state == "failed" && retired {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authored nonexecutable Worker did not fail/retire"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let observed = engine.library_load(&owner, &batch).unwrap().report;
        let effect = observed.items[0].effects.last().unwrap().request_id.clone();
        let cached: (String, String) = {
            let db = engine.db.lock().unwrap();
            let cached = db
                .query_row(
                    "SELECT fingerprint,response FROM requests WHERE session=?1 AND request=?2",
                    params![owner.as_str(), effect.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            db.execute(
                "DELETE FROM requests WHERE session=?1 AND request=?2",
                params![owner.as_str(), effect.as_str()],
            )
            .unwrap();
            cached
        };
        assert_eq!(
            engine
                .handle(req(
                    &owner,
                    &token,
                    id!(RequestId),
                    Command::LibraryBatchMigrate {
                        batch_request_id: batch.clone(),
                        expected_report_sha256: observed.report_sha256().unwrap(),
                        recipe: opts.recipe.clone()
                    }
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        {
            let db = engine.db.lock().unwrap();
            assert_eq!(
                db.query_row("SELECT count(*) FROM library_batches", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            db.execute(
                "INSERT INTO requests VALUES(?1,?2,?3,?4)",
                params![owner.as_str(), effect.as_str(), cached.0, cached.1],
            )
            .unwrap();
        }
        let child = id!(RequestId);
        let ResponseBody::LibraryBatch(successor) = engine
            .handle(req(
                &owner,
                &token,
                child.clone(),
                Command::LibraryBatchMigrate {
                    batch_request_id: batch.clone(),
                    expected_report_sha256: observed.report_sha256().unwrap(),
                    recipe: opts.recipe,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("migration")
        };
        assert_eq!(successor.items[0].phase, Phase::SourceImported);
        assert!(successor.items[0].job_id.is_none());
        assert_eq!(successor.items[0].effects.len(), 2);
        assert_eq!(
            engine.library_load(&owner, &batch).unwrap().report.items[0].job_id,
            Some(job.clone())
        );
        let db = engine.db.lock().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            job_snapshot(&db, &job, observed.items[0].project_id.as_ref().unwrap())
                .unwrap()
                .state,
            "failed"
        );
        drop(db);
        assert!(
            cancel(&engine, &owner, &token, &child, id!(RequestId))
                .cancellation
                .unwrap()
                .completed
        );
        engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: child,
                },
            ))
            .result
            .unwrap();
        engine
            .handle(req(
                &owner,
                &token,
                id!(RequestId),
                Command::LibraryBatchRelease {
                    batch_request_id: batch,
                },
            ))
            .result
            .unwrap();
        Engine::retire_for_test(engine);
    }
}
