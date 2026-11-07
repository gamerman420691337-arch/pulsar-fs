// Linked continuation of unaccepted work; original receipts and outputs remain owned.
fn checked_migration_membership(
    db: &Connection,
    record: &Record,
    session: &str,
    id: &str,
) -> Result<()> {
    let floor = crate::inference_equivalence_reader::version(db)?;
    if floor < 32 {
        anyhow::ensure!(
            record.report.migration.is_none() && record.report.successor_batch_request_id.is_none(),
            "batch migration reader floor downgraded"
        );
        return Ok(());
    }
    let row:Option<(String,String)>=db.query_row("SELECT parent_request,parent_sha256 FROM library_batch_migrations WHERE session=?1 AND request=?2",params![session,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    anyhow::ensure!(
        row == record.report.migration.as_ref().map(|m| (
            m.parent_batch_request_id.to_string(),
            m.parent_report_sha256.clone()
        )),
        "batch migration origin membership differs"
    );
    if let Some((parent, sha)) = row {
        let raw: String = db.query_row(
            "SELECT record FROM library_batches WHERE session=?1 AND request=?2",
            params![session, parent],
            |r| r.get(0),
        )?;
        let mut previous: Record = serde_json::from_str(&raw)?;
        anyhow::ensure!(
            previous
                .report
                .successor_batch_request_id
                .as_deref()
                .map(RequestId::as_str)
                == Some(id),
            "batch migration original fence differs"
        );
        previous.report.successor_batch_request_id = None;
        anyhow::ensure!(
            previous
                .report
                .report_sha256()
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
                == sha,
            "batch migration original report changed"
        );
    }
    let child: Option<String> = db
        .query_row(
            "SELECT request FROM library_batch_migrations WHERE session=?1 AND parent_request=?2",
            params![session, id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(child) = child {
        anyhow::ensure!(
            record
                .report
                .successor_batch_request_id
                .as_deref()
                .map(RequestId::as_str)
                == Some(child.as_str()),
            "batch migration successor membership differs"
        );
    } else if let Some(child) = &record.report.successor_batch_request_id {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_batches WHERE session=?1 AND request=?2)",
            params![session, child.as_str()],
            |r| r.get(0),
        )?;
        anyhow::ensure!(!exists, "batch migration successor membership absent");
    }
    let mut cursor = id.to_owned();
    let mut seen = std::collections::BTreeSet::new();
    for depth in 0..=16 {
        anyhow::ensure!(seen.insert(cursor.clone()), "batch migration lineage cycle");
        let parent:Option<String>=db.query_row("SELECT parent_request FROM library_batch_migrations WHERE session=?1 AND request=?2",params![session,cursor],|r|r.get(0)).optional()?;
        let Some(parent) = parent else { return Ok(()) };
        anyhow::ensure!(depth < 16, "batch migration lineage exceeds capacity");
        cursor = parent;
    }
    anyhow::bail!("batch migration lineage exceeds capacity")
}
impl Engine {
    fn library_migration_terminal_jobs(
        &self,
        db: &Connection,
        record: &Record,
        owner: &SessionId,
        auth_token: &str,
    ) -> PResult<()> {
        let workers = self.workers.lock().map_err(internal)?;
        for (index, item) in record.report.items.iter().enumerate() {
            if item.phase != Phase::Generating {
                continue;
            }
            let project = item
                .project_id
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("Accepted batch job lacks project"))?;
            let job = item
                .job_id
                .as_ref()
                .ok_or_else(|| ProtocolError::invalid("Accepted batch job absent"))?;
            let effect = item
                .effects
                .last()
                .filter(|effect| effect.phase == Phase::SourceImported)
                .ok_or_else(|| {
                    ProtocolError::invalid("Accepted batch generation request absent")
                })?;
            let mut original = record.clone();
            original.report.items[index].phase = Phase::SourceImported;
            let (scope, revision, command) = phase_command(&original, index)?;
            let mut accepted = Request::new(effect.request_id.clone(), command)
                .with_session(owner.clone())
                .with_auth_token(auth_token.to_owned());
            accepted.project = scope;
            accepted.expected_revision = revision;
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(request_fingerprint(&accepted).map_err(internal)?)
            );
            let Some(ResponseBody::Job(receipt)) =
                replay(db, owner.as_str(), &accepted, &fingerprint)?
            else {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "Original accepted job receipt is unresolved; retain its request identity",
                ));
            };
            if receipt.job_id != *job
                || receipt.project_id != *project
                || receipt.base_revision != item.revision
            {
                return Err(ProtocolError::invalid(
                    "Original accepted job receipt differs",
                ));
            }
            let snapshot = job_snapshot(db, job, project)?;
            if snapshot.base_revision != item.revision
                || snapshot.candidate_id.is_some()
                || !matches!(snapshot.state.as_str(), "failed" | "cancelled")
                || workers.contains_key(job.as_str())
            {
                return Err(ProtocolError::new(ErrorCode::RequestConflict,"Accepted job is not failed/cancelled with absent Worker ownership; finish or reconcile it before migration"));
            }
        }
        Ok(())
    }
    fn library_migrate(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        batch: &RequestId,
        expected: &str,
        recipe: &LibraryBatchRecipe,
    ) -> PResult<ResponseBody> {
        let owner = request.session.as_ref().expect("authenticated");
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, owner)?;
            if let Some(body) = replay(&db, owner.as_str(), request, fingerprint)? {
                return Ok(body);
            }
        }
        let mut parent = self.library_load(owner, batch)?;
        let original_record = encode(&parent)?;
        if parent.report.report_sha256()? != expected {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Observed original batch report changed",
            ));
        }
        if parent.pending.is_some()
            || parent.report.pending_advance_request_id.is_some()
            || parent.report.cancellation.is_some()
            || parent.report.successor_batch_request_id.is_some()
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Original batch has unresolved, cancelled or migrated work",
            ));
        }
        if parent
            .report
            .items
            .iter()
            .any(|i| i.phase == Phase::CandidateReady)
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Finish accepted candidates before migration",
            ));
        }
        if matches!(parent.options.recipe, LibraryBatchRecipe::Visual(_))
            != matches!(recipe, LibraryBatchRecipe::Visual(_))
        {
            return Err(ProtocolError::invalid(
                "Migration must preserve publication axes",
            ));
        }
        if !parent.report.items.iter().any(|i| i.error.is_some())
            && parent.worker_stamp == self.library_worker_stamp()?
            && !parent
                .report
                .items
                .iter()
                .any(|i| i.phase == Phase::Generating)
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Migration requires a stopped diagnostic or changed tool identity",
            ));
        }
        let indices: Vec<usize> = parent
            .report
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (item.phase != Phase::ReadyForPublication).then_some(index))
            .collect();
        if indices.is_empty() {
            return Err(ProtocolError::invalid("No unfinished inputs to migrate"));
        }
        let mut options = LibraryBatchOptions {
            inputs: indices
                .iter()
                .map(|i| parent.options.inputs[*i].clone())
                .collect(),
            recipe: recipe.clone(),
        };
        options.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            for index in &indices {
                let item = &parent.report.items[*index];
                if let Some(project) = &item.project_id {
                    for scope in [Scope::Read, Scope::Generate, Scope::ImportSource] {
                        require_grant(&db, owner, project, scope)?;
                    }
                    if project_snapshot(&db, project)?.revision != item.revision {
                        return Err(ProtocolError::new(
                            ErrorCode::RevisionConflict,
                            "Imported project revision changed; reconcile before migration",
                        ));
                    }
                }
            }
        }
        {
            let db = self.db.lock().map_err(internal)?;
            self.library_migration_terminal_jobs(
                &db,
                &parent,
                owner,
                request.auth_token.as_deref().expect("authenticated"),
            )?;
        }
        let _artifacts = self.artifact_gate.lock().map_err(internal)?;
        // Current inputs are the captured sources, never external paths or rediscovery.
        for index in &indices {
            let capture = &parent.captures[*index];
            let (sha, bytes) =
                artifacts::digest_file(&capture.path, self.config.max_snapshot_bytes)
                    .map_err(unavailable)?;
            if sha != capture.sha256 || bytes != capture.bytes {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Original captured source changed",
                ));
            }
            let destination = &parent.report.items[*index].destination;
            let directory = destination
                .parent()
                .ok_or_else(|| ProtocolError::invalid("Destination parent absent"))?;
            let canonical = directory.canonicalize().map_err(unavailable)?;
            if canonical != directory
                || canonical.starts_with(self.config.state_dir.canonicalize().map_err(unavailable)?)
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Captured destination directory changed",
                ));
            }
            if let Some(identity) = &parent.report.items[*index].publication_directory {
                artifacts::check_publication_directory(destination, identity)
                    .map_err(unavailable)?;
            } else {
                match fs::symlink_metadata(&parent.report.items[*index].destination) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(unavailable(e)),
                    Ok(_) => {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "Unfinished destination already exists; preserve it",
                        ))
                    }
                }
            }
        }
        let (model_capture, pack_documents) = self.library_capture_recipe(&mut options.recipe)?;
        let mut items: Vec<_> = indices
            .iter()
            .map(|i| parent.report.items[*i].clone())
            .collect();
        for item in &mut items {
            if item.phase == Phase::Generating {
                item.phase = Phase::SourceImported;
                item.job_id = None;
                item.candidate_id = None;
                item.effects.retain(|effect| {
                    matches!(effect.phase, Phase::Selected | Phase::ProjectCreated)
                });
            }
            item.error = None;
        }
        let schema = options.publication_schema(false);
        let successor = Record {
            worker_stamp: self.library_worker_stamp()?,
            model_capture,
            pack_documents,
            report: LibraryBatchReport {
                schema,
                model_pack: options.recipe.model_pack().cloned().map(Box::new),
                migration: Some(Box::new(LibraryBatchMigration {
                    parent_batch_request_id: batch.clone(),
                    parent_report_sha256: expected.to_owned(),
                })),
                successor_batch_request_id: None,
                batch_request_id: request.request_id.clone(),
                items,
                pending_advance_request_id: None,
                cancellation: None,
                generation_ready: false,
                publication_completed: false,
                qualified: false,
            },
            captures: indices
                .iter()
                .map(|i| parent.captures[*i].clone())
                .collect(),
            options,
            pending: None,
        };
        checked(&successor, &self.config.state_dir)?;
        let child_raw = encode(&successor)?;
        parent.report.successor_batch_request_id = Some(Box::new(request.request_id.clone()));
        checked(&parent, &self.config.state_dir)?;
        let parent_raw = encode(&parent)?;
        if child_raw.len() > crate::library_batch_db::MAX_RECORD
            || parent_raw.len() > crate::library_batch_db::MAX_RECORD
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Migrated batch metadata exceeds capacity",
            ));
        }
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let original: String = tx
            .query_row(
                "SELECT record FROM library_batches WHERE session=?1 AND request=?2",
                params![owner.as_str(), batch.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let original: Record = decode(&original)?;
        if original.report.report_sha256()? != expected
            || original.pending.is_some()
            || encode(&original)? != original_record
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Original batch changed during capture",
            ));
        }
        let mut cursor = batch.to_string();
        let mut depth = 0;
        let floor = crate::inference_equivalence_reader::version(&tx).map_err(internal)?;
        if floor >= 32 {
            loop {
                let next:Option<String>=tx.query_row("SELECT parent_request FROM library_batch_migrations WHERE session=?1 AND request=?2",params![owner.as_str(),cursor],|r|r.get(0)).optional().map_err(internal)?;
                let Some(next) = next else { break };
                depth += 1;
                if depth >= 16 {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "Migration lineage exceeds 16 continuations",
                    ));
                }
                cursor = next;
            }
        }
        for item in &successor.report.items {
            if let Some(project) = &item.project_id {
                for scope in [Scope::Read, Scope::Generate, Scope::ImportSource] {
                    require_grant(&tx, owner, project, scope)?;
                }
                if project_snapshot(&tx, project)?.revision != item.revision {
                    return Err(ProtocolError::new(
                        ErrorCode::RevisionConflict,
                        "Imported project revision changed; reconcile before migration",
                    ));
                }
            }
        }
        self.library_migration_terminal_jobs(
            &tx,
            &original,
            owner,
            request.auth_token.as_deref().expect("authenticated"),
        )?;
        if let Some(choice) = successor.options.recipe.model_pack() {
            self.require_current_pack_choice(&tx, choice)?;
        }
        crate::library_batch_db::arm_migration(&tx).map_err(internal)?;
        if successor.report.schema == 6 {
            crate::library_batch_db::arm_partial_audio(&tx).map_err(internal)?;
        }
        if successor.report.schema == 5 {
            crate::library_batch_db::arm_selected_audio(&tx).map_err(internal)?;
        }
        claim_request_identity(&tx, owner.as_str(), request, fingerprint)?;
        tx.execute(
            "UPDATE library_batches SET record=?3 WHERE session=?1 AND request=?2",
            params![owner.as_str(), batch.as_str(), parent_raw],
        )
        .map_err(internal)?;
        tx.execute(
            "INSERT INTO library_batches VALUES(?1,?2,?3)",
            params![owner.as_str(), request.request_id.as_str(), child_raw],
        )
        .map_err(internal)?;
        for (index, capture) in successor.captures.iter().enumerate() {
            tx.execute(
                "INSERT INTO library_batch_sources VALUES(?1,?2,?3,?4)",
                params![
                    owner.as_str(),
                    request.request_id.as_str(),
                    index as i64,
                    capture.sha256
                ],
            )
            .map_err(internal)?;
        }
        if let Some(model) = &successor.model_capture {
            tx.execute(
                "INSERT INTO library_batch_models VALUES(?1,?2,?3)",
                params![owner.as_str(), request.request_id.as_str(), model.sha256],
            )
            .map_err(internal)?;
        }
        for (index, doc) in successor.pack_documents.iter().enumerate() {
            tx.execute(
                "INSERT INTO library_batch_pack_documents VALUES(?1,?2,?3,?4)",
                params![
                    owner.as_str(),
                    request.request_id.as_str(),
                    index as i64,
                    doc.sha256
                ],
            )
            .map_err(internal)?;
        }
        tx.execute(
            "INSERT INTO library_batch_migrations VALUES(?1,?2,?3,?4)",
            params![
                owner.as_str(),
                request.request_id.as_str(),
                batch.as_str(),
                expected
            ],
        )
        .map_err(internal)?;
        crate::library_batch_db::capacity(&tx).map_err(internal)?;
        checked_migration_membership(&tx, &successor, owner.as_str(), request.request_id.as_str())
            .map_err(internal)?;
        let body = ResponseBody::LibraryBatch(successor.report);
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}
