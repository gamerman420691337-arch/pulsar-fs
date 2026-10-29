//! Private admission of explicitly selected, currently accepted fallback evidence.
//! Serialized declarations and history never construct this held proof.
use super::inference_fallback_store::{FallbackAdmission, FallbackAdmissionKind};
use super::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) fn wall_time() -> PResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| u64::try_from(v.as_millis()).ok())
        .filter(|v| *v > 0 && *v <= i64::MAX as u64)
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::Unavailable, "fallback wall clock unavailable")
        })
}
fn digest<T: Serialize>(value: &T) -> PResult<String> {
    Ok(format!("{:x}", Sha256::digest(encode(value)?.as_bytes())))
}
fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
fn profile(request: &WorkerRequest) -> PResult<&DeclaredInferenceProfile> {
    match &request.operation {
        WorkerOperation::Generate {
            settings,
            model: Some(_),
            ..
        } => settings.inference_profile.as_ref(),
        _ => None,
    }
    .ok_or_else(|| ProtocolError::invalid("fallback requires explicit model-backed generation"))
}
fn scope_matches(
    selection: &InferenceFallbackSelection,
    subject: &InferenceEquivalenceSubject,
    manifest: &WorkerRequest,
) -> PResult<()> {
    let WorkerOperation::Generate {
        preset, settings, ..
    } = &manifest.operation
    else {
        return Err(ProtocolError::unsupported(
            "fallback is a generation job operation",
        ));
    };
    let recipe = match profile(manifest)?.preprocessing.input.decoder {
        DetectorDecoder::YoloChannelMajor => InferenceEquivalenceRecipe::YoloTracking,
        DetectorDecoder::YoloEndToEnd => {
            return Err(ProtocolError::unsupported(
                "end-to-end decoder has no implemented equivalence recipe",
            ))
        }
        DetectorDecoder::TemporalScriptPosition if settings.annotated_view.is_some() => {
            InferenceEquivalenceRecipe::AnnotatedTemporalPosition
        }
        DetectorDecoder::TemporalScriptPosition => InferenceEquivalenceRecipe::TemporalPosition,
    };
    if !selection.matches_scope(&subject.scope)?
        || !selection.matches_worker_request(manifest)?
        || subject.scope.tier != InferenceEquivalenceTier::from_preset(preset)?
        || subject.scope.recipe != recipe
        || !subject.scope.axes.contains(&settings.axis)
    {
        return Err(changed(
            "fallback evidence does not cover the actual source, view, recipe, tier and axis",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) struct PreparedFallback {
    selection: InferenceFallbackSelection,
    reference: InferenceFallbackReference,
    profile_sha256: String,
    uses_to_profile: bool,
    kind: FallbackAdmissionKind,
    ordinal: u8,
    predecessor: Option<AttemptId>,
    observed: super::inference_environment_control::HeldInferenceEnvironment,
    binding: Mutex<Option<(String, FallbackAdmission)>>,
}
#[cfg(target_os = "linux")]
impl PreparedFallback {
    pub(super) fn bind(&self, manifest: &mut WorkerRequest) -> PResult<()> {
        if digest(profile(manifest)?)? != self.profile_sha256 {
            return Err(changed(
                "fallback profile changed after environment observation",
            ));
        }
        let identity = self.selection.identity()?;
        if manifest.dependencies.iter().any(|item| {
            item.artifact_id
                .as_str()
                .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
                && *item != identity
        }) {
            return Err(changed("attempt carries another fallback selection"));
        }
        if !manifest.dependencies.contains(&identity) {
            manifest.dependencies.push(identity);
        }
        Ok(())
    }
    /// Called after the new job/attempt rows exist, inside their publishing transaction.
    pub(super) fn declare(
        &self,
        tx: &Transaction<'_>,
        manifest: &WorkerRequest,
        previous: Option<&WorkerRequest>,
    ) -> PResult<()> {
        if self.binding.lock().map_err(internal)?.is_some() {
            return Err(ProtocolError::invalid(
                "held fallback preparation already consumed",
            ));
        }
        self.observed.recheck()?;
        source_kinds::require_media(tx, &manifest.project_id, &manifest.source.source_version)?;
        let now = wall_time()?;
        let checked = inference_equivalence_store::resolve_current_accepted(
            tx,
            &self.reference.dossier_id,
            self.reference.expected_revision,
            &self.reference.expected_report_sha256,
            now,
        )?;
        if checked.assessment().manifest_id() != &self.reference.dossier_id
            || checked.report_sha256() != self.reference.expected_report_sha256
        {
            return Err(changed(
                "accepted evidence proof differs from selected reference",
            ));
        }
        let subject = &checked.report().manifest.subject;
        scope_matches(&self.selection, subject, manifest)?;
        let (expected_profile, expected_environment) = if self.uses_to_profile {
            (&subject.to_profile, &subject.to_environment)
        } else {
            (&subject.from_profile, &subject.from_environment)
        };
        if digest(profile(manifest)?)? != digest(expected_profile)?
            || self.observed.environment().identity_sha256()?
                != expected_environment.identity_sha256()?
        {
            return Err(changed(
                "current fallback environment or profile differs from accepted scope",
            ));
        }
        self.observed.recheck()?;
        let admission = FallbackAdmission {
            schema_version: 1,
            kind: self.kind,
            ordinal: self.ordinal,
            predecessor: self.predecessor.clone(),
            reference: self.reference.clone(),
            observed_environment: self.observed.environment().clone(),
            admitted_at_unix_ms: checked.checked_at_unix_ms(),
            expires_at_unix_ms: subject.expires_unix_ms,
        };
        match self.kind {
            FallbackAdmissionKind::Initial => inference_fallback_store::declare_initial(
                tx,
                manifest,
                &self.selection,
                &admission,
            )?,
            FallbackAdmissionKind::Resume => inference_fallback_store::record_resume(
                tx,
                previous.ok_or_else(|| {
                    ProtocolError::invalid("fallback-enabled resume predecessor absent")
                })?,
                manifest,
                &admission,
            )?,
            FallbackAdmissionKind::Fallback => inference_fallback_store::record_successor(
                tx,
                previous.ok_or_else(|| ProtocolError::invalid("fallback predecessor absent"))?,
                manifest,
                &admission,
            )?,
        }
        *self.binding.lock().map_err(internal)? = Some((digest(manifest)?, admission));
        Ok(())
    }
    pub(super) fn require_bound(
        &self,
        db: &Connection,
        manifest: &WorkerRequest,
    ) -> PResult<FallbackAdmission> {
        self.observed.recheck()?;
        let binding = self.binding.lock().map_err(internal)?;
        let (hash, admission) = binding
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("fallback preparation was not admitted"))?;
        if *hash != digest(manifest)? {
            return Err(changed(
                "fallback dispatch manifest differs from its held preparation",
            ));
        }
        let retained = inference_fallback_store::retained(db, manifest)?
            .ok_or_else(|| changed("fallback declaration missing before dispatch"))?;
        if encode(&retained.admission)? != encode(admission)?
            || encode(&retained.selection)? != encode(&self.selection)?
        {
            return Err(changed("fallback dispatch differs from retained admission"));
        }
        let checked = inference_equivalence_store::read_current_accepted(
            db,
            &self.reference.dossier_id,
            self.reference.expected_revision,
            &self.reference.expected_report_sha256,
            wall_time()?,
        )?;
        scope_matches(
            &self.selection,
            &checked.report().manifest.subject,
            manifest,
        )?;
        Ok(admission.clone())
    }
}

#[cfg(target_os = "linux")]
impl Engine {
    fn observe_fallback_profile(
        &self,
        profile: &DeclaredInferenceProfile,
        deadline: Instant,
    ) -> PResult<super::inference_environment_control::HeldInferenceEnvironment> {
        match profile.provider {
            InferenceProvider::Cpu if self.cpu_owner_policy.is_some() => {}
            InferenceProvider::Cuda
                if self.cuda_owner_policy.is_some() && cfg!(feature = "cuda") => {}
            _ => return Err(ProtocolError::unsupported(
                "automatic inference selection requires a configured captured CPU or CUDA runtime",
            )),
        }
        self.observe_inference_environment(profile, deadline)
    }
    /// Prepare only the environment/evidence side; final source, grant and resource checks occur in admission.
    pub(super) fn prepare_initial_fallback(
        &self,
        selection: &InferenceFallbackSelection,
        selected: &DeclaredInferenceProfile,
    ) -> PResult<PreparedFallback> {
        selection.validate()?;
        let end = Instant::now()
            .checked_add(Duration::from_secs(30))
            .ok_or_else(|| ProtocolError::invalid("fallback observation deadline overflow"))?;
        let observed = self.observe_fallback_profile(selected, end)?;
        let profile_sha256 = digest(selected)?;
        let db = self.db.lock().map_err(internal)?;
        let mut last = ProtocolError::unsupported(
            "no currently accepted equivalence covers the selected initial profile/environment",
        );
        for reference in &selection.equivalents {
            if Instant::now() >= end {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "fallback evidence preparation deadline elapsed",
                ));
            }
            let checked = match inference_equivalence_store::read_current_accepted(
                &db,
                &reference.dossier_id,
                reference.expected_revision,
                &reference.expected_report_sha256,
                wall_time()?,
            ) {
                Ok(value) => value,
                Err(error) => {
                    last = error;
                    continue;
                }
            };
            let subject = &checked.report().manifest.subject;
            if digest(&subject.from_profile)? != profile_sha256
                || !selection.matches_scope(&subject.scope)?
                || observed.environment().identity_sha256()?
                    != subject.from_environment.identity_sha256()?
            {
                continue;
            }
            return Ok(PreparedFallback {
                selection: selection.clone(),
                reference: reference.clone(),
                profile_sha256,
                uses_to_profile: false,
                kind: FallbackAdmissionKind::Initial,
                ordinal: 0,
                predecessor: None,
                observed,
                binding: Mutex::new(None),
            });
        }
        Err(last)
    }
    pub(super) fn prepare_fallback_resume(
        &self,
        previous: &WorkerRequest,
    ) -> PResult<Option<PreparedFallback>> {
        let retained = {
            let db = self.db.lock().map_err(internal)?;
            inference_fallback_store::retained(&db, previous)?
        };
        let Some(retained) = retained else {
            return Ok(None);
        };
        let end = Instant::now()
            .checked_add(Duration::from_secs(30))
            .ok_or_else(|| ProtocolError::invalid("fallback observation deadline overflow"))?;
        let selected = profile(previous)?;
        let observed = self.observe_fallback_profile(selected, end)?;
        let reference = retained.admission.reference.clone();
        let db = self.db.lock().map_err(internal)?;
        let checked = inference_equivalence_store::read_current_accepted(
            &db,
            &reference.dossier_id,
            reference.expected_revision,
            &reference.expected_report_sha256,
            wall_time()?,
        )?;
        let subject = &checked.report().manifest.subject;
        let uses_to_profile = retained.admission.ordinal > 0;
        let (approved_profile, approved_environment) = if uses_to_profile {
            (&subject.to_profile, &subject.to_environment)
        } else {
            (&subject.from_profile, &subject.from_environment)
        };
        scope_matches(&retained.selection, subject, previous)?;
        if digest(approved_profile)? != digest(selected)?
            || observed.environment().identity_sha256()?
                != approved_environment.identity_sha256()?
        {
            return Err(changed(
                "fallback-enabled resume requires its exact current accepted profile/environment",
            ));
        }
        Ok(Some(PreparedFallback {
            selection: retained.selection,
            reference,
            profile_sha256: digest(selected)?,
            uses_to_profile,
            kind: FallbackAdmissionKind::Resume,
            ordinal: retained.admission.ordinal,
            predecessor: Some(previous.attempt_id.clone()),
            observed,
            binding: Mutex::new(None),
        }))
    }
    pub(super) fn prepare_fallback_target(
        &self,
        previous: &WorkerRequest,
        selection: &InferenceFallbackSelection,
        reference: &InferenceFallbackReference,
        ordinal: u8,
    ) -> PResult<(PreparedFallback, DeclaredInferenceProfile)> {
        selection.validate()?;
        if ordinal == 0 || ordinal > selection.maximum_fallbacks {
            return Err(ProtocolError::invalid("fallback ordinal exhausted"));
        }
        let report = {
            let db = self.db.lock().map_err(internal)?;
            let checked = inference_equivalence_store::read_current_accepted(
                &db,
                &reference.dossier_id,
                reference.expected_revision,
                &reference.expected_report_sha256,
                wall_time()?,
            )?;
            checked.report().clone()
        };
        let subject = &report.manifest.subject;
        scope_matches(selection, subject, previous)?;
        if digest(&subject.from_profile)? != digest(profile(previous)?)? {
            return Err(changed("fallback direction differs from failed profile"));
        }
        let end = Instant::now()
            .checked_add(Duration::from_secs(30))
            .ok_or_else(|| ProtocolError::invalid("fallback observation deadline overflow"))?;
        let observed = self.observe_fallback_profile(&subject.to_profile, end)?;
        if observed.environment().identity_sha256()? != subject.to_environment.identity_sha256()? {
            return Err(changed(
                "fresh target environment differs from accepted comparison",
            ));
        }
        Ok((
            PreparedFallback {
                selection: selection.clone(),
                reference: reference.clone(),
                profile_sha256: digest(&subject.to_profile)?,
                uses_to_profile: true,
                kind: FallbackAdmissionKind::Fallback,
                ordinal,
                predecessor: Some(previous.attempt_id.clone()),
                observed,
                binding: Mutex::new(None),
            },
            subject.to_profile.clone(),
        ))
    }
}

impl Engine {
    pub(super) fn require_fallback_publication(
        &self,
        db: &Connection,
        manifest: &WorkerRequest,
        lease: Option<&super::inference_fallback_lifetime::Lease>,
    ) -> PResult<()> {
        let retained = inference_fallback_store::retained(db, manifest)?;
        let Some(retained) = retained else {
            return if lease.is_none() {
                Ok(())
            } else {
                Err(changed("fallback lifetime has no retained declaration"))
            };
        };
        let lease = lease
            .ok_or_else(|| changed("retained fallback history cannot authorize publication"))?;
        lease.check()?;
        if encode(lease.reference())? != encode(&retained.admission.reference)? {
            return Err(changed(
                "fallback lifetime differs from admitted comparison",
            ));
        }
        let reference = &retained.admission.reference;
        let checked = inference_equivalence_store::read_current_accepted(
            db,
            &reference.dossier_id,
            reference.expected_revision,
            &reference.expected_report_sha256,
            wall_time()?,
        )?;
        let subject = &checked.report().manifest.subject;
        scope_matches(&retained.selection, subject, manifest)?;
        let (approved_profile, environment) = if retained.admission.ordinal == 0 {
            (&subject.from_profile, &subject.from_environment)
        } else {
            (&subject.to_profile, &subject.to_environment)
        };
        if digest(profile(manifest)?)? != digest(approved_profile)?
            || retained.admission.observed_environment.identity_sha256()?
                != environment.identity_sha256()?
        {
            return Err(changed(
                "fallback completion differs from accepted environment or profile",
            ));
        }
        lease.check()
    }

    pub(super) fn fail_pending_fallback(
        &self,
        previous: &WorkerRequest,
        error: &ProtocolError,
    ) -> PResult<()> {
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        let Some(pending) = inference_fallback_store::pending(&tx, &previous.job_id)? else {
            return Ok(());
        };
        if pending.predecessor != previous.attempt_id
            || pending.engine_epoch.as_str() != self.engine_epoch
        {
            return Err(changed(
                "fallback failure cannot replace another pending attempt",
            ));
        }
        let diagnostic = JobAttemptDiagnostic::from_error(error);
        let changed = tx.execute("UPDATE jobs SET state='failed',error=?3 WHERE id=?1 AND attempt=?2 AND state='fallback_pending'",
            params![previous.job_id.as_str(), previous.attempt_id.as_str(), format!("automatic fallback refused: {}",diagnostic.message)]).map_err(internal)?;
        if changed == 1 {
            inference_fallback_store::clear_pending(&tx, &previous.job_id)?;
            event(
                &tx,
                &previous.project_id,
                previous.base_revision,
                EventBody::JobChanged(job_snapshot(&tx, &previous.job_id, &previous.project_id)?),
            )?;
        }
        tx.commit().map_err(internal)
    }
}

#[cfg(target_os = "linux")]
impl Engine {
    /// Rechecks the original live preparation inside capture/publication admission.
    /// This private check carries no client credential and cannot start continuation.
    pub(super) fn require_fallback_capture_admission(
        &self,
        db: &Connection,
        previous: &WorkerRequest,
        owner: &SessionId,
        cancel: &Arc<AtomicBool>,
    ) -> PResult<Option<JobId>> {
        if self.is_shutting_down() || cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "fallback preparation cancelled",
            ));
        }
        let pending = inference_fallback_store::pending(db, &previous.job_id)?
            .ok_or_else(|| changed("fallback pending intent retired"))?;
        if pending.predecessor != previous.attempt_id
            || pending.engine_epoch.as_str() != self.engine_epoch
        {
            return Err(changed(
                "fallback capture is not from the current settled attempt",
            ));
        }
        let job = job_snapshot(db, &previous.job_id, &previous.project_id)?;
        if job.state != "fallback_pending" || job.attempt_id != previous.attempt_id {
            return Err(changed("fallback capture predecessor is no longer pending"));
        }
        let recorded_owner: String = db
            .query_row(
                "SELECT session FROM jobs WHERE id=?1",
                [previous.job_id.as_str()],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if recorded_owner != owner.as_str() {
            return Err(changed(
                "fallback capture actor differs from original job owner",
            ));
        }
        require_grant(db, owner, &previous.project_id, Scope::Generate)?;
        inference_fallback_store::retained(db, previous)?
            .ok_or_else(|| changed("fallback capture declaration absent"))?;
        let workers = self.workers.lock().map_err(internal)?;
        if !workers
            .get(previous.job_id.as_str())
            .is_some_and(|current| Arc::ptr_eq(current, cancel))
        {
            return Err(changed(
                "fallback capture lacks its original live preparation",
            ));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "fallback preparation cancelled",
            ));
        }
        Ok(Some(previous.job_id.clone()))
    }

    /// Only a live settlement token from the current worker supervisor can enter this path.
    /// The predecessor's reservations and consumer have already retired in run_job.
    pub(super) fn continue_inference_fallback(
        self: &Arc<Self>,
        previous: &WorkerRequest,
        settled: launch::SettledWorkerFailure,
    ) -> PResult<()> {
        let provider_absence = settled.provider_unavailable().ok_or_else(|| {
            ProtocolError::invalid("fallback requires live provider-unavailable settlement")
        })?;
        provider_absence.validate_for(profile(previous)?, &digest(profile(previous)?)?)?;
        let epoch = AttemptId::new(self.engine_epoch.clone()).map_err(internal)?;
        let preparation_cancel = Arc::new(AtomicBool::new(false));
        let (retained, owner) = {
            let db = self.db.lock().map_err(internal)?;
            let pending = inference_fallback_store::pending(&db, &previous.job_id)?
                .ok_or_else(|| changed("fallback pending intent absent"))?;
            if pending.predecessor != previous.attempt_id || pending.engine_epoch != epoch {
                return Err(changed(
                    "fallback pending intent is not from this settled Engine attempt",
                ));
            }
            let job = job_snapshot(&db, &previous.job_id, &previous.project_id)?;
            if job.state != "fallback_pending" || job.attempt_id != previous.attempt_id {
                return Err(changed("fallback job is no longer pending"));
            }
            let owner: String = db
                .query_row(
                    "SELECT session FROM jobs WHERE id=?1",
                    [previous.job_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(internal)?;
            let owner = SessionId::new(owner).map_err(internal)?;
            require_grant(&db, &owner, &previous.project_id, Scope::Generate)?;
            let retained = inference_fallback_store::retained(&db, previous)?
                .ok_or_else(|| changed("fallback original selection absent"))?;
            let mut workers = self.workers.lock().map_err(internal)?;
            if workers.contains_key(previous.job_id.as_str()) {
                return Err(changed(
                    "fallback predecessor worker handle was not retired",
                ));
            }
            workers.insert(previous.job_id.to_string(), preparation_cancel.clone());
            (retained, owner)
        };
        struct Preparation<'a> {
            workers: &'a Mutex<HashMap<String, Arc<AtomicBool>>>,
            job: String,
            flag: Arc<AtomicBool>,
        }
        impl Drop for Preparation<'_> {
            fn drop(&mut self) {
                if let Ok(mut workers) = self.workers.lock() {
                    if workers
                        .get(&self.job)
                        .is_some_and(|current| Arc::ptr_eq(current, &self.flag))
                    {
                        workers.remove(&self.job);
                    }
                }
            }
        }
        let _preparation = Preparation {
            workers: &self.workers,
            job: previous.job_id.to_string(),
            flag: preparation_cancel.clone(),
        };
        let ordinal = retained
            .admission
            .ordinal
            .checked_add(1)
            .filter(|v| *v <= retained.selection.maximum_fallbacks)
            .ok_or_else(|| ProtocolError::invalid("fallback attempt limit exhausted"))?;
        let mut last = ProtocolError::unsupported(
            "no accepted equivalent can be admitted for the failed profile",
        );
        for reference in &retained.selection.equivalents {
            if self.is_shutting_down() || preparation_cancel.load(Ordering::Acquire) {
                return Err(ProtocolError::new(
                    ErrorCode::Cancelled,
                    "fallback preparation cancelled",
                ));
            }
            let (prepared, target) = match self.prepare_fallback_target(
                previous,
                &retained.selection,
                reference,
                ordinal,
            ) {
                Ok(value) => value,
                Err(error) => {
                    last = error;
                    continue;
                }
            };
            let WorkerOperation::Generate {
                preset,
                settings,
                model,
            } = &previous.operation
            else {
                unreachable!("explicit generation checked");
            };
            let mut settings = *settings.clone();
            settings.model_input = Some(target.preprocessing.input.clone());
            settings.inference_profile = Some(target.clone());
            let operation = WorkerOperation::Generate {
                preset: preset.clone(),
                settings: Box::new(settings),
                model: model.clone(),
            };
            let budget = crate::inference_budget::admit(
                ResourceBudget {
                    memory_bytes: WORKER_MEMORY,
                    output_bytes: OUTPUT_LIMIT,
                    wall_time_ms: self.config.worker_timeout.as_millis().min(u64::MAX as u128)
                        as u64,
                    cpu_threads: 2,
                },
                Some(&target),
                None,
                self.config.max_ram_bytes,
            )?;
            // An internal preparation request uses the original granted actor; it is
            // never sent, authenticated as a new client, or saved as a request receipt.
            let internal_request = Request::new(
                RequestId::new(Uuid::new_v4().to_string()).map_err(internal)?,
                Command::ResumeJob {
                    job_id: previous.job_id.clone(),
                },
            )
            .with_session(owner.clone())
            .in_project(previous.project_id.clone(), None);
            let extra = {
                let db = self.db.lock().map_err(internal)?;
                require_grant(&db, &owner, &previous.project_id, Scope::Generate)?;
                self.fallback_pack_dependencies(&db, previous)?
            };
            let (cpu, cuda) = match target.provider {
                InferenceProvider::Cpu => match self.capture_cpu_inference_with_admission(
                    &target,
                    &budget,
                    crate::resources::GpuPriority::Offline,
                    &preparation_cancel,
                    |tx| {
                        self.require_fallback_capture_admission(
                            tx,
                            previous,
                            &owner,
                            &preparation_cancel,
                        )
                    },
                ) {
                    Ok(value) => (Some(value), None),
                    Err(error) => {
                        last = error;
                        continue;
                    }
                },
                InferenceProvider::Cuda => match self.capture_cuda_inference_with_admission(
                    &target,
                    &budget,
                    crate::resources::GpuPriority::Offline,
                    &preparation_cancel,
                    |tx| {
                        self.require_fallback_capture_admission(
                            tx,
                            previous,
                            &owner,
                            &preparation_cancel,
                        )
                    },
                ) {
                    Ok(value) => (None, Some(value)),
                    Err(error) => {
                        last = error;
                        continue;
                    }
                },
                _ => {
                    last = ProtocolError::unsupported(
                        "fallback target has no captured execution adapter",
                    );
                    continue;
                }
            };
            let work = (|| -> PResult<_> {
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(internal)?;
                self.require_fallback_capture_admission(
                    &tx,
                    previous,
                    &owner,
                    &preparation_cancel,
                )?;
                if encode(&self.fallback_pack_dependencies(&tx, previous)?)? != encode(&extra)? {
                    return Err(changed("fallback pack approval changed during capture"));
                }
                let work = self.prepare_operation_with_capture(
                    &tx,
                    &internal_request,
                    &previous.source.source_version,
                    operation,
                    OperationDependencies {
                        origin: generation_attempt_origin::AttemptOrigin::Fallback(previous),
                        extra: &extra,
                        cuda: cuda.as_ref(),
                        cpu: cpu.as_ref(),
                        fallback: Some(&prepared),
                    },
                )?;
                tx.commit().map_err(internal)?;
                Ok(work)
            })();
            let (manifest, reservation, cancel) = match work {
                Ok(value) => value,
                Err(error) => {
                    last = error;
                    continue;
                }
            };
            let failed_manifest = manifest.clone();
            let engine = self.clone();
            if let Err(error) = self.spawn_task("pulsar-qualified-fallback", None, move || {
                engine.run_job(manifest, cancel, reservation, cuda, cpu, Some(prepared));
            }) {
                self.finish_job(&failed_manifest, Err(unavailable(error)))?;
            }
            return Ok(());
        }
        Err(last)
    }
}
