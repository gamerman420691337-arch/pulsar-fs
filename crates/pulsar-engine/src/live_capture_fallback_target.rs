//! Automatic target admission consumes an original settled native failure.
//! Fresh epochs share cancellation/deadline, never pixels or graceful transfer.
use super::*;
use crate::live_cuda_stream::SettledLiveCudaProviderFailure;

pub(in crate::authority::live_captures) fn after_provider_failure(
    engine: &Arc<Engine>,
    intent: &Request,
    epoch: &AttemptId,
    attempt: &AttemptId,
    settled: SettledLiveCudaProviderFailure,
) -> PResult<()> {
    let actor = intent
        .session
        .as_ref()
        .ok_or_else(|| changed("fallback actor absent"))?;
    let (held, original, failure, end) = {
        let db = engine.db.lock().map_err(internal)?;
        engine.authorize(&db, intent, actor)?;
        check_revision(&db, intent)?;
        let mut registry = engine.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        let entry = inference::active(&mut registry, epoch, attempt)
            .ok_or_else(|| changed("original capture retired before fallback settlement"))?;
        if entry.owner != *actor || entry.report.identity.project != *project(intent)? {
            return Err(changed("provider failure belongs to another capture owner"));
        }
        let held = entry.fallback.clone().ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "provider unavailable; no fallback was selected",
            )
        })?;
        require_evidence(&db, entry)?;
        let original = report(entry)?;
        let failure = settled
            .capture_metadata(&original, &engine.binary_identity)
            .map_err(unavailable)?;
        // Provider-list absence precedes Ready, so no native output/delivery
        // may have been admitted. A consumer or graceful handoff is not reused.
        if entry.native_delivery.is_some()
            || entry.native_input_close.is_some()
            || entry.native_successor.is_some()
            || entry.native_transfer.is_some()
        {
            return Err(changed(
                "pre-Ready fallback has outstanding delivery or transfer custody",
            ));
        }
        let end = held.end_cap(settled.original_deadline());
        inference::fail_entry(
            entry,
            ProtocolError::new(
                ErrorCode::Unavailable,
                "original live CUDA provider unavailable; preparing declared fallback",
            ),
        )?;
        entry
            .report
            .inference
            .as_mut()
            .expect("original inference")
            .fallback_transition = Some(BrowserInferenceFallbackTransition {
            failure: failure.clone(),
            phase: BrowserInferenceFallbackPhase::Preparing,
            next_epoch: None,
            error: None,
        });
        entry.report.validate()?;
        (held, original, failure, end)
    };
    let result = prepare_and_start(engine, intent, &held, &original, &failure, end);
    if let Err(error) = &result {
        // Cancellation reaches every actual worker flag without depending on
        // another database transaction or a successful status poll.
        held.lifetime.cancel();
        let mut registry = engine.live_captures.lock().map_err(internal)?;
        if let Some(entry) = registry.entries.get_mut(epoch.as_str()) {
            if let Some(transition) = entry
                .report
                .inference
                .as_mut()
                .and_then(|i| i.fallback_transition.as_mut())
            {
                let mut bounded = error.clone();
                bounded.message = bounded.message.chars().take(512).collect();
                transition.phase = BrowserInferenceFallbackPhase::Refused;
                transition.next_epoch = None;
                transition.error = Some(bounded);
            }
        }
    }
    result
}

fn prepare_and_start(
    engine: &Arc<Engine>,
    intent: &Request,
    held: &Arc<Prepared>,
    original: &LiveCaptureReport,
    failure: &BrowserInferenceFallbackFailure,
    end: Instant,
) -> PResult<()> {
    held.recheck_io()?;
    let ordinal = held
        .ordinal
        .checked_add(1)
        .filter(|n| *n <= held.selection.maximum_fallbacks)
        .ok_or_else(|| changed("declared browser fallback allowance exhausted"))?;
    let remaining = end
        .checked_duration_since(Instant::now())
        .ok_or_else(|| changed("original browser fallback lifetime expired"))?;
    let lease_ms = u32::try_from(remaining.as_millis()).map_err(internal)?;
    if lease_ms == 0 {
        return Err(changed("no original browser fallback lifetime remains"));
    }
    let mut options = original.options.clone();
    options.lease_ms = lease_ms.min(options.lease_ms);
    options.continuation_mode = LiveContinuationMode::Immediate;
    options
        .browser_analysis
        .as_mut()
        .ok_or_else(|| changed("original browser feed absent"))?
        .feed_id = id!(ArtifactId);
    let mut request = intent.clone();
    request.request_id = id!(RequestId);
    let semantic = browser_inference_semantic_identity(&original.options)?;
    let mut last = ProtocolError::new(
        ErrorCode::Unavailable,
        "no current accepted target covers the original live failure",
    );
    for candidate in &held.selection.equivalents {
        held.check_lifetime()?;
        if Instant::now() >= end {
            return Err(changed("original fallback worker deadline expired"));
        }
        let subject = {
            let db = engine.db.lock().map_err(internal)?;
            engine.authorize(&db, &request, &held.owner)?;
            check_revision(&db, &request)?;
            let accepted = match inference_equivalence_store::read_current_accepted(
                &db,
                &candidate.dossier_id,
                candidate.expected_revision,
                &candidate.expected_report_sha256,
                inference_fallback_execution::wall_time()?,
            ) {
                Ok(accepted) => accepted,
                Err(error) => {
                    last = error;
                    continue;
                }
            };
            accepted.report().manifest.subject.clone()
        };
        if hash(&subject.from_profile)? != held.profile_sha256
            || subject.from_environment.identity_sha256()?
                != held.observed.environment().identity_sha256()?
            || subject.to_profile.model != subject.from_profile.model
            || !held
                .selection
                .matches_capture(&held.engine_epoch, &original.options, &subject)?
        {
            continue;
        }
        let target_hash = hash(&subject.to_profile)?;
        if target_hash == held.profile_sha256
            || held
                .history
                .iter()
                .any(|step| step.failure.observation.profile_sha256 == target_hash)
        {
            continue;
        }
        options
            .inference
            .as_mut()
            .expect("original inference")
            .profile = subject.to_profile.clone();
        if let Err(error) = options.validate() {
            last = error;
            continue;
        }
        let target_semantics = match browser_inference_semantic_identity(&options) {
            Ok(identity) => identity,
            Err(error) => {
                last = error;
                continue;
            }
        };
        if target_semantics != semantic {
            return Err(changed(
                "fallback target changed original browser semantics",
            ));
        }
        let observed = match engine.observe_fallback_profile(&subject.to_profile, end) {
            Ok(observed) => observed,
            Err(error) => {
                last = error;
                continue;
            }
        };
        if observed.environment().identity_sha256()? != subject.to_environment.identity_sha256()? {
            last = changed("target environment differs from accepted equivalence");
            continue;
        }
        observed.recheck()?;
        held.recheck_io()?;
        let mut history = held.history.clone();
        history.push(BrowserInferenceFallbackStep {
            failure: failure.clone(),
            reference: candidate.clone(),
            next_profile_sha256: target_hash.clone(),
        });
        request.command = Command::StartLiveCapture {
            engine_epoch: held.engine_epoch.clone(),
            options: options.clone(),
        };
        let prepared = Arc::new(Prepared {
            owner: held.owner.clone(),
            project: held.project.clone(),
            revision: held.revision,
            request: request.request_id.clone(),
            engine_epoch: held.engine_epoch.clone(),
            options_sha256: hash(&options)?,
            profile_sha256: target_hash,
            selection: held.selection.clone(),
            reference: candidate.clone(),
            ordinal,
            predecessor: Some(original.identity.clone()),
            history,
            lifetime: held.lifetime.clone(),
            execution_end: end,
            media: held.media.clone(),
            timing: held.timing.clone(),
            observed,
            binding: Mutex::new(None),
        });
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&request).map_err(internal)?)
        );
        let body = engine.live_capture_command_prepared(
            &request,
            &held.owner,
            &fingerprint,
            Some(prepared),
        )?;
        let ResponseBody::LiveCapture(target) = body else {
            return Err(internal("fallback target returned another response"));
        };
        target.validate()?;
        if target.identity.epoch == original.identity.epoch
            || target
                .inference
                .as_ref()
                .is_none_or(|i| i.attempt_id == failure.manifest.stream.attempt_id)
        {
            return Err(changed(
                "fallback target did not create a fresh native attempt and epoch",
            ));
        }
        return Ok(());
    }
    Err(last)
}
