//! Checked assistant recipes enter the existing authored candidate/undo pipeline.
use super::*;
impl Engine {
    fn checked_assistant_recipe(
        &self,
        db: &Connection,
        request: &Request,
        owner: &SessionId,
    ) -> PResult<AssistantEditRecipe> {
        let Command::ApplyAssistantEdit { approval } = &request.command else {
            return Err(ProtocolError::invalid("edit approval required"));
        };
        require_grant(db, owner, project(request)?, Scope::Read)?;
        check_revision(db, request)?;
        let report = assistant_operations::read(db, &approval.attempt_id)?;
        if report.project_id != *project(request)?
            || report.session_id != *owner
            || report.engine_epoch.as_str() != self.engine_epoch
            || Some(report.base_revision) != request.expected_revision
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "assistant edit owner, revision or engine differs",
            ));
        }
        let exact = AssistantEditApproval::from_report(&report)?;
        let snapshot = project_snapshot(db, project(request)?)?;
        let (_, context) = self.edit_assistant_context(
            db,
            &snapshot,
            &report.scope.review_ids,
            &report.scope.target_ids,
            &report.scope.edit_recipe_ids,
            owner,
        )?;
        if approval.context_sha256 != exact.context_sha256
            || approval.proposal_sha256 != exact.proposal_sha256
            || format!("{:x}", Sha256::digest(context.as_bytes())) != exact.context_sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "approved assistant edit or current context differs",
            ));
        }
        let Some(AssistantProposal::ApplyEditRecipe { recipe_id }) = report.proposal else {
            return Err(ProtocolError::invalid("edit recipe required"));
        };
        AssistantEditRecipe::catalog(&snapshot)?
            .into_iter()
            .find(|r| r.recipe_id == recipe_id)
            .ok_or_else(|| ProtocolError::new(ErrorCode::NotFound, "approved recipe is absent"))
    }
    pub(super) fn apply_assistant_edit(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let (recipe, base, protected, mut reviews, actor_id) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, owner)?;
            require_grant(&db, owner, project(request)?, Scope::Read)?;
            if let Some(body) = replay(&db, owner.as_str(), request, fingerprint)? {
                return Ok(body);
            }
            let recipe = self.checked_assistant_recipe(&db, request, owner)?;
            let snapshot = project_snapshot(&db, project(request)?)?;
            let bytes: i64 = db
                .query_row(
                    "SELECT length(CAST(protected AS BLOB)) FROM projects WHERE id=?1",
                    [project(request)?.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if !(0..=1024 * 1024).contains(&bytes) {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "protected capture exceeds admission",
                ));
            }
            let raw: String = db
                .query_row(
                    "SELECT protected FROM projects WHERE id=?1",
                    [project(request)?.as_str()],
                    |r| r.get(0),
                )
                .map_err(internal)?;
            if raw.len() > 1024 * 1024 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "protected capture exceeds admission",
                ));
            }
            let protected: Vec<ProtectedRegion> = decode(&raw)?;
            if protected.len() > 1024 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "protected region count exceeds admission",
                ));
            }
            (
                recipe,
                snapshot.motion.program,
                protected,
                lineage::state(&db, project(request)?, snapshot.revision)?,
                actor(&db, owner)?,
            )
        };
        let _memory = self.pool.reserve_memory(2 * 1024 * 1024 * 1024)?;
        let original = self.motion_store.read(&base)?;
        let values =
            pulsar_core::recipe_edit_values(&original, recipe.axis, recipe.operation, &protected)
                .map_err(internal)?;
        let edit_id = id!(TransferId);
        let provenance =
            pulsar_core::ProvenanceRef::new(format!("edit:{edit_id}")).map_err(internal)?;
        let reconciled = pulsar_core::reconcile_edit_values(&original, &values, &provenance)
            .map_err(internal)?;
        pulsar_core::ensure_protected_segments_unchanged(
            &original,
            &reconciled.program,
            &protected,
        )
        .map_err(|e| ProtocolError::new(ErrorCode::Forbidden, e.to_string()))?;
        if reconciled.authored_ranges.is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "recipe has no writable change",
            ));
        }
        validate_program(&reconciled.program)?;
        reviews.decisions.clear();
        reviews.flags = lineage::clipped(&reviews.flags, &lineage::coverage(&reconciled.program)?)?;
        for axis in Axis::ALL {
            let ranges: Vec<_> = reconciled
                .authored_ranges
                .iter()
                .filter(|(a, _)| *a == axis)
                .map(|(_, r)| *r)
                .collect();
            if let (Some(start), Some(end)) = (
                ranges.iter().map(|r| r.start()).min(),
                ranges.iter().map(|r| r.end()).max(),
            ) {
                reviews.flags.push(ReviewFlag {
                    axis: Some(axis),
                    range: TimeRange::new(start, end).map_err(internal)?,
                    reason: ReviewReason::new("authored_edit_envelope")?,
                    evidence: EvidenceKind::Synthesized,
                    confidence: None,
                });
            }
        }
        reviews.conservative = true;
        validate_reviews(&reviews.flags)?;
        let output = self.motion_store.publish(&reconciled.program)?;
        let input = serde_json::to_vec(&values).map_err(internal)?;
        if input.len() as u64 > MAX_MOTION_BYTES {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "recipe values exceed admission",
            ));
        }
        let input_sha = format!("{:x}", Sha256::digest(&input));
        let label = format!(
            "Assistant {:?} {:?} recipe {}",
            recipe.operation, recipe.axis, recipe.recipe_id
        );
        let receipt_bytes = serde_json::to_vec(&serde_json::json!({"format":"pulsar-edit-receipt-v1","kernel":pulsar_core::EDIT_VALUES_KERNEL_VERSION,"lease_id":edit_id.as_str(),"project_id":recipe.project_id,"base_revision":recipe.revision,"base_program":base,"upload_sha256":input_sha,"upload_byte_len":input.len(),"result_program":output,"authored_provenance":reconciled.authored_provenance,"authored_ranges":reconciled.authored_ranges,"inherited_ranges":reconciled.inherited_ranges,"actor_id":actor_id,"label":label})).map_err(internal)?;
        if receipt_bytes.len() > 128 * 1024 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "recipe receipt exceeds admission",
            ));
        }
        let receipt_sha = format!("{:x}", Sha256::digest(&receipt_bytes));
        for (directory, hash, bytes) in [
            ("edit-inputs", &input_sha, &input),
            ("edit-receipts", &receipt_sha, &receipt_bytes),
        ] {
            let path = self.config.state_dir.join(directory).join(hash);
            if path.exists() {
                let (actual, length) =
                    artifacts::digest_file(&path, bytes.len() as u64).map_err(unavailable)?;
                if actual != *hash || length != bytes.len() as u64 {
                    return Err(internal("immutable assistant edit object differs"));
                }
            } else {
                artifacts::publish_export(&path, bytes).map_err(unavailable)?;
            }
        }
        let receipt = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-receipt.{receipt_sha}"))
                .map_err(internal)?,
            sha256: receipt_sha,
            byte_len: receipt_bytes.len() as u64,
        };
        let uploaded = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-input.{input_sha}")).map_err(internal)?,
            sha256: input_sha,
            byte_len: input.len() as u64,
        };
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        require_grant(&tx, owner, project(request)?, Scope::Read)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let fresh = self.checked_assistant_recipe(&tx, request, owner)?;
        if fresh.recipe_id != recipe.recipe_id {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "recipe changed during preparation",
            ));
        }
        let candidate = id!(CandidateId);
        let reference = motion_state::register(&tx, &output)?;
        let base_artifact = ArtifactIdentity {
            artifact_id: base.artifact_id.clone(),
            sha256: base.sha256.clone(),
            byte_len: base.byte_len,
        };
        tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,?5,?6)", params![candidate.as_str(),project(request)?.as_str(),rev_sql(recipe.revision)?,reference,encode(&vec![uploaded,receipt.clone(),base_artifact])?,encode(&reviews.flags)?]).map_err(internal)?;
        tx.execute(
            "INSERT INTO edit_candidate_lineage VALUES(?1,?2,?3,?4,?5)",
            params![
                candidate.as_str(),
                encode(&receipt)?,
                encode(&reviews)?,
                i64::try_from(reconciled.authored_ranges.len()).map_err(internal)?,
                i64::try_from(reconciled.inherited_ranges.len()).map_err(internal)?
            ],
        )
        .map_err(internal)?;
        tx.execute(
            "INSERT INTO edit_upload_receipts VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                edit_id.as_str(),
                project(request)?.as_str(),
                owner.as_str(),
                rev_sql(recipe.revision)?,
                candidate.as_str(),
                encode(&receipt)?
            ],
        )
        .map_err(internal)?;
        let body = ResponseBody::Candidate(Box::new(candidate_snapshot(
            &tx,
            &candidate,
            project(request)?,
        )?));
        event(
            &tx,
            project(request)?,
            recipe.revision,
            EventBody::CandidateReady {
                candidate_id: candidate,
            },
        )?;
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}
