use super::*;

/// The already captured base program, reviews and actor for one edit attempt.
/// The execution path still rechecks current authorization and revision fences.
struct WasmEditBase<'a> {
    original: &'a MotionProgram,
    reviews: lineage::ReviewState,
    actor_id: &'a str,
}

impl Engine {
    pub(in crate::authority) fn start_wasm_edit(
        self: &Arc<Self>,
        request: &Request,
        fingerprint: &str,
        spec: &WasmEditSpec,
    ) -> PResult<ResponseBody> {
        spec.validate()?;
        if spec.expected_worker != self.binary_identity
            || hash(&spec.module) != spec.manifest.artifact.sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "approved WASM module or native worker identity differs",
            ));
        }
        if !crate::wasm_launch::executable_size_admitted(self.binary_identity.byte_len) {
            return Err(ProtocolError::new(ErrorCode::ResourceExhausted,
                "WASM native worker exceeds the bounded executable image; install the dedicated companion"));
        }
        if !crate::launch::available() {
            return Err(ProtocolError::unsupported(
                "Linux WASM confinement unavailable",
            ));
        }
        let memory = self.pool.reserve_memory(EDIT_MEMORY)?;
        let owner = request
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        let (base_program, prior, actor_id) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, owner)?;
            check_revision(&db, request)?;
            let descriptor = motion_state::revision_descriptor(
                &db,
                project(request)?,
                request.expected_revision.unwrap(),
            )?;
            if descriptor.byte_len > 8 * WASM_BUFFER_MAX_BYTES as u64 {
                return Err(ProtocolError::new(ErrorCode::ResourceExhausted,"WASM editor base exceeds bounded whole-program input admission; no motion was truncated"));
            }
            (
                descriptor,
                lineage::state(&db, project(request)?, request.expected_revision.unwrap())?,
                actor(&db, owner)?,
            )
        };
        let original = self.motion_store.read(&base_program)?;
        let values =
            pulsar_core::EditValuesProgram::from_program_values(&original).map_err(internal)?;
        let input = serde_json::to_vec(&values).map_err(internal)?;
        if input.len() > WASM_BUFFER_MAX_BYTES {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "WASM whole-program values exceed the ABI buffer; no motion was truncated",
            ));
        }
        let plan = Plan {
            attempt: id!(AttemptId),
            project: project(request)?.clone(),
            owner: owner.clone(),
            base: request.expected_revision.unwrap(),
            epoch: self.engine_epoch.clone(),
            request_id: request.request_id.clone(),
            fingerprint: fingerprint.into(),
            manifest: spec.manifest.clone(),
            module: spec.module.clone(),
            input,
            base_program,
            worker: spec.expected_worker.clone(),
            guest_memory_bytes: spec.guest_memory_bytes,
            label: spec.label.clone(),
        };
        let flag = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let publication_hook = START_PUBLICATION_HOOKS
            .lock()
            .unwrap()
            .remove(request.request_id.as_str());
        #[cfg(test)]
        if let Some(hook) = publication_hook {
            hook();
        }
        let body = {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(internal)?;
            self.authorize(&tx, request, owner)?;
            check_revision(&tx, request)?;
            if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
                return Ok(body);
            }
            check_request_identity(&tx, owner.as_str(), request, fingerprint)?;
            let (total, active): (i64, i64) = tx
                .query_row(
                    "SELECT COUNT(*),COALESCE(SUM(state='running'),0) FROM wasm_edits",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(internal)?;
            if total >= MAX_RETAINED_INVOCATIONS
                || archives::operational_count(&tx)? >= MAX_OPERATIONS
                || active >= self.config.max_jobs as i64
            {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "WASM operational catalog, retained evidence or active admission exhausted",
                ));
            }
            self.arm_worker_fence(&tx)?;
            tx.execute("INSERT INTO wasm_edits(id,project,session,base_revision,engine_epoch,request_id,request_sha256,manifest,module,input,base_program,expected_worker,guest_memory_bytes,label,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'running')",
                params![plan.attempt.as_str(),plan.project.as_str(),plan.owner.as_str(),rev_sql(plan.base)?,plan.epoch,
                    plan.request_id.as_str(),plan.fingerprint,encode(&plan.manifest)?,plan.module,plan.input,
                    encode(&plan.base_program)?,encode(&plan.worker)?,plan.guest_memory_bytes as i64,plan.label]).map_err(internal)?;
            let body = ResponseBody::WasmEdit((report(&tx, &plan)?).into());
            remember(&tx, owner.as_str(), request, fingerprint, &body)?;
            // Install under the same DB lock used by cancellation and revocation.
            self.workers
                .lock()
                .map_err(internal)?
                .insert(key(&plan), flag.clone());
            if let Err(error) = tx.commit() {
                self.workers.lock().map_err(internal)?.remove(&key(&plan));
                return Err(internal(error));
            }
            body
        };
        let engine = self.clone();
        let accepted = plan.clone();
        let token = flag.clone();
        let authorization = request.clone();
        #[cfg(test)]
        let hook = take_admission_hook(&request.request_id);
        let spawned = self.spawn_task("pulsar-wasm-edit", None, move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.execute_wasm_edit(
                    &authorization,
                    &accepted,
                    &token,
                    WasmEditBase {
                        original: &original,
                        reviews: prior,
                        actor_id: &actor_id,
                    },
                    #[cfg(test)]
                    hook,
                )
            }))
            .unwrap_or_else(|_| {
                Err(internal(
                    "WASM edit supervisor panicked before proposal admission",
                ))
            });
            if let Err(error) = result {
                let _ = engine.fail_wasm_edit(&accepted, &token, error);
            }
            if let Ok(mut workers) = engine.workers.lock() {
                workers.remove(&key(&accepted));
            }
            drop(memory);
        });
        if let Err(error) = spawned {
            self.fail_wasm_edit(&plan, &flag, unavailable(error))?;
            self.workers.lock().map_err(internal)?.remove(&key(&plan));
        }
        Ok(body)
    }

    fn fail_wasm_edit(
        &self,
        plan: &Plan,
        cancel: &AtomicBool,
        mut error: ProtocolError,
    ) -> PResult<()> {
        error.message = error.message.chars().take(1024).collect();
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        let old = report(&tx, plan)?;
        if old.state == WasmEditState::Running {
            let cancelled = cancel.load(Ordering::Acquire) || error.code == ErrorCode::Cancelled;
            if cancelled {
                error = ProtocolError::new(
                    ErrorCode::Cancelled,
                    "WASM edit cancelled before proposal admission",
                );
            }
            tx.execute(
                "UPDATE wasm_edits SET state=?2,error=?3 WHERE id=?1 AND state='running'",
                params![
                    plan.attempt.as_str(),
                    if cancelled { "cancelled" } else { "failed" },
                    encode(&error)?
                ],
            )
            .map_err(internal)?;
        }
        tx.commit().map_err(internal)?;
        Ok(())
    }

    fn execute_wasm_edit(
        &self,
        request: &Request,
        plan: &Plan,
        cancel: &AtomicBool,
        base: WasmEditBase<'_>,
        #[cfg(test)] hook: Option<Box<dyn FnOnce() + Send>>,
    ) -> PResult<()> {
        let WasmEditBase {
            original,
            mut reviews,
            actor_id,
        } = base;
        {
            let db = self.db.lock().map_err(internal)?;
            self.wasm_admission_fence(&db, request, plan, cancel)?;
        }
        let envelope = WasmWorkerRequest {
            version: PROTOCOL_VERSION,
            attempt_id: plan.attempt.clone(),
            request: WasmProcessingRequest {
                manifest: plan.manifest.clone(),
                module: plan.module.clone(),
                input: plan.input.clone(),
                guest_memory_bytes: plan.guest_memory_bytes,
            },
        };
        let executed = crate::wasm_launch::execute(
            &self.config,
            &self.pool,
            self.worker_fence.clone(),
            &envelope,
            &plan.worker,
            cancel,
        )?;
        #[cfg(test)]
        if let Some(hook) = hook {
            hook();
        }
        let values: pulsar_core::EditValuesProgram =
            serde_json::from_slice(&executed.output.output).map_err(|e| {
                ProtocolError::invalid(format!("WASM result is not a values-only edit: {e}"))
            })?;
        let provenance =
            pulsar_core::ProvenanceRef::new(format!("edit:{}", plan.attempt)).map_err(internal)?;
        let reconciled = pulsar_core::reconcile_edit_values(original, &values, &provenance)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
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
        let result = self.motion_store.publish(&reconciled.program)?;
        let edit_input = &executed.output.output;
        let edit_receipt_bytes=serde_json::to_vec(&serde_json::json!({
            "format":"pulsar-edit-receipt-v1","kernel":pulsar_core::EDIT_VALUES_KERNEL_VERSION,
            "lease_id":plan.attempt.as_str(),"project_id":plan.project,"base_revision":plan.base,
            "base_program":plan.base_program,"upload_sha256":executed.output.output_sha256,
            "upload_byte_len":edit_input.len(),"result_program":result,"authored_provenance":reconciled.authored_provenance,
            "authored_ranges":reconciled.authored_ranges,"inherited_ranges":reconciled.inherited_ranges,
            "actor_id":actor_id,"label":plan.label
        })).map_err(internal)?;
        if edit_receipt_bytes.len() > 128 * 1024 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "WASM edit receipt exceeds admission",
            ));
        }
        let edit_receipt_hash = hash(&edit_receipt_bytes);
        publish_immutable(
            &self
                .config
                .state_dir
                .join("edit-inputs")
                .join(&executed.output.output_sha256),
            edit_input,
        )?;
        publish_immutable(
            &self
                .config
                .state_dir
                .join("edit-receipts")
                .join(&edit_receipt_hash),
            &edit_receipt_bytes,
        )?;
        let receipt_identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-receipt.{edit_receipt_hash}"))
                .map_err(internal)?,
            sha256: edit_receipt_hash,
            byte_len: edit_receipt_bytes.len() as u64,
        };
        let receipt = WasmEditReceipt {
            schema_version: PROTOCOL_VERSION,
            request_sha256: plan.fingerprint.clone(),
            module: plan.manifest.artifact.clone(),
            worker: ArtifactIdentity {
                artifact_id: plan.worker.artifact_id.clone(),
                sha256: executed.worker_executable_sha256,
                byte_len: executed.worker_executable_bytes,
            },
            base_program: plan.base_program.clone(),
            input_sha256: executed.output.input_sha256,
            input_byte_len: plan.input.len() as u64,
            output_sha256: executed.output.output_sha256.clone(),
            output_byte_len: edit_input.len() as u64,
            result_program: result.clone(),
            edit_receipt: receipt_identity.clone(),
            fuel_consumed: executed.output.fuel_consumed,
            worker_peak_memory_bytes: executed.worker_peak_memory_bytes,
        };
        receipt.validate()?;
        let receipt_json = encode(&receipt)?;
        if receipt_json.len() > MAX_METADATA {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "WASM operation receipt exceeds control metadata admission",
            ));
        }
        let uploaded = ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("edit-input.{}", receipt.output_sha256))
                .map_err(internal)?,
            sha256: receipt.output_sha256.clone(),
            byte_len: receipt.output_byte_len,
        };
        let base = ArtifactIdentity {
            artifact_id: plan.base_program.artifact_id.clone(),
            sha256: plan.base_program.sha256.clone(),
            byte_len: plan.base_program.byte_len,
        };
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.wasm_admission_fence(&tx, request, plan, cancel)?;
        let candidate = id!(CandidateId);
        tx.execute("INSERT INTO candidates(id,project,base_revision,program,job,lineage,review) VALUES(?1,?2,?3,?4,NULL,?5,?6)",
            params![candidate.as_str(),plan.project.as_str(),rev_sql(plan.base)?,motion_state::register(&tx,&result)?,
                encode(&vec![uploaded,receipt_identity.clone(),base])?,encode(&reviews.flags)?]).map_err(internal)?;
        tx.execute(
            "INSERT INTO edit_candidate_lineage VALUES(?1,?2,?3,?4,?5)",
            params![
                candidate.as_str(),
                encode(&receipt_identity)?,
                encode(&reviews)?,
                reconciled.authored_ranges.len() as i64,
                reconciled.inherited_ranges.len() as i64
            ],
        )
        .map_err(internal)?;
        tx.execute("UPDATE wasm_edits SET state='completed',candidate=?2,receipt=?3,output=?4 WHERE id=?1 AND state='running'",
            params![plan.attempt.as_str(),candidate.as_str(),receipt_json,edit_input]).map_err(internal)?;
        event(
            &tx,
            &plan.project,
            plan.base,
            EventBody::CandidateReady {
                candidate_id: candidate,
            },
        )?;
        tx.commit().map_err(internal)?;
        Ok(())
    }

    fn wasm_admission_fence(
        &self,
        db: &Connection,
        request: &Request,
        plan: &Plan,
        cancel: &AtomicBool,
    ) -> PResult<()> {
        if cancel.load(Ordering::Acquire) || report(db, plan)?.state != WasmEditState::Running {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "WASM edit no longer has active admission",
            ));
        }
        if plan.epoch != self.engine_epoch {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "WASM edit belongs to a previous engine epoch",
            ));
        }
        self.authorize(db, request, &plan.owner)?;
        check_revision(db, request)?;
        self.require_worker_fence(db)?;
        Ok(())
    }
}
fn publish_immutable(path: &Path, bytes: &[u8]) -> PResult<()> {
    if path.exists() {
        let (sha, len) = artifacts::digest_file(path, bytes.len() as u64).map_err(unavailable)?;
        if sha != hash(bytes) || len != bytes.len() as u64 {
            return Err(internal(
                "immutable WASM edit object conflicts with retained bytes",
            ));
        }
    } else {
        artifacts::publish_export(path, bytes).map_err(unavailable)?;
    }
    Ok(())
}

#[cfg(test)]
type AdmissionHook = Box<dyn FnOnce() + Send>;
#[cfg(test)]
static ADMISSION_HOOKS: std::sync::LazyLock<Mutex<HashMap<String, AdmissionHook>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(test)]
fn take_admission_hook(request: &RequestId) -> Option<AdmissionHook> {
    ADMISSION_HOOKS.lock().unwrap().remove(request.as_str())
}
#[cfg(test)]
#[path = "wasm_edit_tests.rs"]
mod tests;

#[cfg(test)]
static START_PUBLICATION_HOOKS: std::sync::LazyLock<Mutex<HashMap<String, AdmissionHook>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
