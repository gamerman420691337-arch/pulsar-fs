//! Pre-worker browser fallback admission. Deserialized reports cannot create
//! this held source/environment proof; actual target execution needs settlement.
use super::*;
use crate::authority::browser_media::SourceMediaPin;
use crate::authority::browser_timing::BrowserTimingPin;
use crate::authority::{inference_equivalence_store, inference_fallback_execution};
#[cfg(target_os = "linux")]
#[path = "live_capture_fallback_target.rs"]
mod target;
#[cfg(target_os = "linux")]
pub(super) use target::after_provider_failure;

fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
fn hash<T: Serialize>(value: &T) -> PResult<String> {
    Ok(format!("{:x}", Sha256::digest(encode(value)?.as_bytes())))
}
fn current_source(media: &SourceMediaPin, timing: &BrowserTimingPin) -> PResult<()> {
    if !media.current() || timing.binding().browser_media != *media.binding() {
        return Err(changed(
            "browser fallback original media custody retired or changed",
        ));
    }
    let selected = timing.select_read().map_err(browser_timing_read_error)?;
    let observed = selected
        .evaluate_at(crate::handy_clock::boottime_ns()?)
        .map_err(browser_timing_read_error)?;
    if !observed.frame.is_current() {
        return Err(changed(
            "browser fallback player is not currently visible and playing",
        ));
    }
    Ok(())
}

struct Bound {
    identity: LiveIdentity,
    attempt: AttemptId,
    admission: BrowserInferenceFallbackAdmission,
}
/// Process-local original lifetime shared only by automatic fallback attempts.
/// A retired ancestor keeps this small cancellation handle, not source pixels.
pub(super) struct Lifetime {
    end: Instant,
    cancelled: AtomicBool,
    workers: Mutex<Vec<std::sync::Weak<AtomicBool>>>,
}
impl Lifetime {
    pub(super) fn retirement_reason(&self) -> Option<LiveCaptureRetirement> {
        if self.cancelled.load(Ordering::Acquire) {
            Some(LiveCaptureRetirement::Stopped)
        } else if Instant::now() >= self.end {
            Some(LiveCaptureRetirement::Expired)
        } else {
            None
        }
    }
    pub(super) fn new(end: Instant) -> Arc<Self> {
        Arc::new(Self {
            end,
            cancelled: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        })
    }
    pub(super) fn check(&self) -> PResult<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "original browser fallback lifetime cancelled",
            ));
        }
        if Instant::now() >= self.end {
            return Err(changed("original browser fallback deadline expired"));
        }
        Ok(())
    }
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        workers.retain(|weak| {
            if let Some(cancel) = weak.upgrade() {
                cancel.store(true, Ordering::Release);
                true
            } else {
                false
            }
        });
    }
    pub(super) fn register(&self, cancel: &Arc<AtomicBool>) {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        workers.retain(|w| w.strong_count() != 0);
        workers.push(Arc::downgrade(cancel));
        if self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.end {
            cancel.store(true, Ordering::Release);
        }
    }
}
pub(super) struct Prepared {
    owner: SessionId,
    project: ProjectId,
    revision: RevisionId,
    request: RequestId,
    engine_epoch: AttemptId,
    options_sha256: String,
    profile_sha256: String,
    selection: BrowserInferenceFallbackSelection,
    reference: InferenceFallbackReference,
    ordinal: u8,
    predecessor: Option<LiveIdentity>,
    history: Vec<BrowserInferenceFallbackStep>,
    lifetime: Arc<Lifetime>,
    execution_end: Instant,
    media: SourceMediaPin,
    timing: BrowserTimingPin,
    #[cfg(target_os = "linux")]
    observed: crate::authority::inference_environment_control::HeldInferenceEnvironment,
    binding: Mutex<Option<Bound>>,
}

pub(super) struct Publication<'a> {
    pub request: &'a Request,
    pub actor: &'a SessionId,
    pub options: &'a LiveCaptureOptions,
    pub stream: &'a LiveInferenceManifest,
    pub media: &'a SourceMediaPin,
    pub timing: &'a BrowserTimingPin,
}

impl Prepared {
    pub(super) fn retirement_reason(&self) -> Option<LiveCaptureRetirement> {
        self.lifetime.retirement_reason().or_else(|| {
            (Instant::now() >= self.execution_end).then_some(LiveCaptureRetirement::Expired)
        })
    }
    pub(super) fn lifetime(&self) -> Arc<Lifetime> {
        self.lifetime.clone()
    }
    pub(super) fn end_cap(&self, end: Instant) -> Instant {
        end.min(self.lifetime.end).min(self.execution_end)
    }
    pub(super) fn check_lifetime(&self) -> PResult<()> {
        self.lifetime.check()?;
        if Instant::now() >= self.execution_end {
            return Err(changed("original worker fallback deadline expired"));
        }
        Ok(())
    }
    pub(super) fn target_parent(&self) -> PResult<&LiveIdentity> {
        if self.ordinal == 0 {
            return Err(changed("automatic target has no original failure"));
        }
        self.predecessor
            .as_ref()
            .ok_or_else(|| changed("automatic target predecessor absent"))
    }
    fn record_matches(&self, entry: &Entry) -> PResult<()> {
        let bound = self.binding.lock().map_err(internal)?;
        let bound = bound
            .as_ref()
            .ok_or_else(|| changed("browser fallback preparation was not published"))?;
        let info = entry
            .report
            .inference
            .as_ref()
            .ok_or_else(|| changed("browser fallback inference retired"))?;
        if entry.owner != self.owner
            || entry.report.identity != bound.identity
            || info.attempt_id != bound.attempt
            || hash(&entry.report.options)? != self.options_sha256
            || info.fallback.as_ref().map(encode).transpose()? != Some(encode(&bound.admission)?)
            || entry
                .report
                .options
                .inference_fallback
                .as_ref()
                .map(|s| s.identity())
                .transpose()?
                != Some(self.selection.identity()?)
            || entry
                .browser_media
                .as_ref()
                .is_none_or(|p| !self.media.same_owner(p))
            || entry
                .browser_analysis_pin
                .as_ref()
                .is_none_or(|p| !self.timing.same_owner(p))
        {
            return Err(changed(
                "browser fallback original native admission differs",
            ));
        }
        Ok(())
    }

    /// Explicit native metadata/hardware I/O; callers release authority locks.
    pub(super) fn recheck_io(&self) -> PResult<()> {
        self.check_lifetime()?;
        #[cfg(target_os = "linux")]
        self.observed.recheck()?;
        #[cfg(not(target_os = "linux"))]
        return Err(ProtocolError::unsupported(
            "browser fallback requires captured Linux execution",
        ));
        current_source(&self.media, &self.timing)
    }

    #[cfg(target_os = "linux")]
    pub(super) fn declare(
        &self,
        tx: &Transaction<'_>,
        context: Publication<'_>,
    ) -> PResult<BrowserInferenceFallbackAdmission> {
        self.check_lifetime()?;
        self.observed.check_lifetime()?;
        let mut binding = self.binding.lock().map_err(internal)?;
        if binding.is_some() {
            return Err(changed("browser fallback preparation already consumed"));
        }
        if context.actor != &self.owner
            || context.request.request_id != self.request
            || project(context.request)? != &self.project
            || context.request.expected_revision != Some(self.revision)
            || hash(context.options)? != self.options_sha256
            || hash(&context.stream.profile)? != self.profile_sha256
            || !self.media.same_owner(context.media)
            || !self.timing.same_owner(context.timing)
        {
            return Err(changed(
                "browser fallback publication differs from original preparation",
            ));
        }
        require_grant(tx, &self.owner, &self.project, Scope::Generate)?;
        current_source(&self.media, &self.timing)?;
        let checked = inference_equivalence_store::resolve_current_accepted(
            tx,
            &self.reference.dossier_id,
            self.reference.expected_revision,
            &self.reference.expected_report_sha256,
            inference_fallback_execution::wall_time()?,
        )?;
        let subject = &checked.report().manifest.subject;
        if !self.selection.matches_subject(
            &self.engine_epoch,
            context.options,
            context.stream,
            subject,
        )? {
            return Err(changed(
                "accepted browser equivalence no longer covers actual capture semantics",
            ));
        }
        self.require_environment(subject)?;
        self.observed.check_lifetime()?;
        let admission = BrowserInferenceFallbackAdmission {
            schema_version: 1,
            selection: self.selection.identity()?,
            reference: self.reference.clone(),
            semantic_identity: browser_inference_semantic_identity(context.options)?,
            observed_environment: self.observed.environment().clone(),
            admitted_at_unix_ms: checked.checked_at_unix_ms(),
            expires_at_unix_ms: subject.expires_unix_ms,
            ordinal: self.ordinal,
            predecessor: self.predecessor.clone(),
            history: self.history.clone(),
        };
        *binding = Some(Bound {
            identity: context.stream.identity.clone(),
            attempt: context.stream.attempt_id.clone(),
            admission: admission.clone(),
        });
        Ok(admission)
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn declare(
        &self,
        _: &Transaction<'_>,
        _: Publication<'_>,
    ) -> PResult<BrowserInferenceFallbackAdmission> {
        Err(ProtocolError::unsupported(
            "browser fallback requires captured Linux execution",
        ))
    }

    #[cfg(target_os = "linux")]
    fn require_environment(&self, subject: &InferenceEquivalenceSubject) -> PResult<()> {
        let (profile, environment) = if self.ordinal == 0 {
            (&subject.from_profile, &subject.from_environment)
        } else {
            (&subject.to_profile, &subject.to_environment)
        };
        if hash(profile)? != self.profile_sha256
            || environment.identity_sha256()? != self.observed.environment().identity_sha256()?
        {
            return Err(changed(
                "browser fallback current profile or environment differs from accepted scope",
            ));
        }
        Ok(())
    }
}

/// Select the original source before observing files, then re-resolve the
/// accepted record inside publication. Exact historical retries do no native work.
pub(super) fn prepare(
    engine: &Engine,
    request: &Request,
    actor: &SessionId,
    fingerprint: &str,
) -> PResult<Option<Arc<Prepared>>> {
    match &request.command {
        Command::StartLiveCapture { options, .. } if options.inference_fallback.is_some() => {}
        Command::PrepareBrowserAnalysisSuccessor { .. } => {}
        _ => return Ok(None),
    }
    let (options, inherited, media, timing, epoch) = {
        let db = engine.db.lock().map_err(internal)?;
        engine.authorize(&db, request, actor)?;
        if matches!(request.command, Command::StartLiveCapture { .. }) {
            check_revision(&db, request)?;
        }
        // Replay is handled by the ordinary command path. It must not acquire
        // a new environment or depend on a predecessor that has since retired.
        if replay(&db, actor.as_str(), request, fingerprint)?.is_some() {
            return Ok(None);
        }
        let (options, inherited, epoch) = match &request.command {
            Command::StartLiveCapture {
                engine_epoch,
                options,
            } if options.inference_fallback.is_some() => {
                (options.clone(), None, engine_epoch.clone())
            }
            Command::PrepareBrowserAnalysisSuccessor {
                engine_epoch,
                predecessor,
                options,
            } => {
                let registry = engine.live_captures.lock().map_err(internal)?;
                let Some(previous) = registry.entries.get(predecessor.as_str()) else {
                    return Ok(None);
                };
                if previous.report.options.inference_fallback.is_none() {
                    return Ok(None);
                }
                if previous.owner != *actor
                    || previous.report.identity.project != *project(request)?
                    || Some(previous.report.identity.revision) != request.expected_revision
                {
                    return Err(changed("browser fallback predecessor owner differs"));
                }
                let held = previous.fallback.as_ref().ok_or_else(|| {
                    changed("browser fallback original native preparation absent")
                })?;
                held.record_matches(previous)?;
                let mut derived = previous.report.options.clone();
                derived.browser_media = Some(options.browser_media.clone());
                derived.browser_analysis = Some(options.browser_analysis.clone());
                derived.lease_ms = options.lease_ms;
                derived.continuation_mode = LiveContinuationMode::Immediate;
                validate_browser_inference_semantic_successor(&previous.report.options, &derived)?;
                (
                    derived,
                    previous
                        .report
                        .inference
                        .as_ref()
                        .and_then(|i| i.fallback.clone()),
                    engine_epoch.clone(),
                )
            }
            _ => return Ok(None),
        };
        check_revision(&db, request)?;
        if replay(&db, actor.as_str(), request, fingerprint)?.is_some() {
            return Ok(None);
        }
        if epoch.as_str() != engine.engine_epoch {
            return Err(changed(
                "browser fallback preparation belongs to another boot",
            ));
        }
        options.validate()?;
        let binding = options
            .browser_media
            .as_ref()
            .ok_or_else(|| changed("browser fallback source binding absent"))?;
        let declared = options
            .browser_analysis
            .as_ref()
            .ok_or_else(|| changed("browser fallback analysis binding absent"))?;
        let media = engine.pin_browser_media_locked(&db, request, binding)?;
        let timing =
            engine.capture_browser_timing_locked(&db, request, &declared.timing_binding_id)?;
        if timing.binding().player != declared.player || timing.binding().browser_media != *binding
        {
            return Err(changed(
                "browser fallback native timing does not bind the declared player/source",
            ));
        }
        current_source(&media, &timing)?;
        (options, inherited, media, timing, epoch)
    };
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (options, inherited, media, timing, epoch);
        Err(ProtocolError::unsupported(
            "browser fallback requires captured Linux execution",
        ))
    }
    #[cfg(target_os = "linux")]
    {
        let selection = options.inference_fallback.as_ref().expect("selected");
        let profile = &options
            .inference
            .as_ref()
            .expect("validated inference")
            .profile;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(u64::from(options.lease_ms)))
            .ok_or_else(|| {
                ProtocolError::invalid("browser fallback preparation deadline overflow")
            })?;
        let observed = engine.observe_fallback_profile(profile, deadline)?;
        let profile_sha256 = hash(profile)?;
        let ordinal = inherited.as_ref().map_or(0, |r| r.ordinal);
        let inherited_reference = inherited
            .as_ref()
            .map(|r| encode(&r.reference))
            .transpose()?;
        let reference = {
            let db = engine.db.lock().map_err(internal)?;
            let mut selected = None;
            let mut last = ProtocolError::unsupported(
                "no current accepted equivalence covers this browser capture",
            );
            for candidate in &selection.equivalents {
                observed.check_lifetime()?;
                if let Some(expected) = &inherited_reference {
                    if encode(candidate)? != *expected {
                        continue;
                    }
                }
                let checked = match inference_equivalence_store::read_current_accepted(
                    &db,
                    &candidate.dossier_id,
                    candidate.expected_revision,
                    &candidate.expected_report_sha256,
                    inference_fallback_execution::wall_time()?,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        last = error;
                        continue;
                    }
                };
                let subject = &checked.report().manifest.subject;
                let (approved, environment) = if ordinal == 0 {
                    (&subject.from_profile, &subject.from_environment)
                } else {
                    (&subject.to_profile, &subject.to_environment)
                };
                if selection.matches_capture(&epoch, &options, subject)?
                    && hash(approved)? == profile_sha256
                    && environment.identity_sha256()? == observed.environment().identity_sha256()?
                {
                    selected = Some(candidate.clone());
                    break;
                }
            }
            selected.ok_or(last)?
        };
        // Explicit I/O recheck is outside DB/capture guards. Final publication
        // checks its original lifetime plus current native source and DB decision.
        observed.recheck()?;
        current_source(&media, &timing)?;
        Ok(Some(Arc::new(Prepared {
            owner: actor.clone(),
            project: project(request)?.clone(),
            revision: request
                .expected_revision
                .ok_or_else(|| changed("browser fallback revision absent"))?,
            request: request.request_id.clone(),
            engine_epoch: epoch,
            options_sha256: hash(&options)?,
            profile_sha256,
            selection: selection.clone(),
            reference,
            ordinal,
            history: inherited
                .as_ref()
                .map_or_else(Vec::new, |r| r.history.clone()),
            predecessor: inherited.and_then(|r| r.predecessor),
            lifetime: Lifetime::new(deadline),
            execution_end: deadline,
            media,
            timing,
            observed,
            binding: Mutex::new(None),
        })))
    }
}

pub(super) fn recheck_io(
    engine: &Engine,
    epoch: &AttemptId,
    attempt: &AttemptId,
) -> PResult<Option<Arc<Prepared>>> {
    let held = {
        let registry = engine.live_captures.lock().map_err(internal)?;
        let Some(entry) = registry.entries.get(epoch.as_str()) else {
            return Ok(None);
        };
        if entry.report.options.inference_fallback.is_none() {
            return Ok(None);
        }
        if entry
            .report
            .inference
            .as_ref()
            .is_none_or(|i| &i.attempt_id != attempt)
        {
            return Err(changed("browser fallback original attempt changed"));
        }
        entry
            .fallback
            .clone()
            .ok_or_else(|| changed("browser fallback native preparation absent"))?
    };
    held.recheck_io()?;
    Ok(Some(held))
}

pub(super) fn require_bound(
    db: &Connection,
    entry: &Entry,
    stream: &LiveInferenceManifest,
    observed: Option<&Arc<Prepared>>,
) -> PResult<()> {
    match (
        &entry.report.options.inference_fallback,
        &entry.fallback,
        observed,
    ) {
        (None, None, None) => Ok(()),
        (Some(_), Some(held), Some(observed)) if Arc::ptr_eq(held, observed) => {
            require_evidence(db, entry)?;
            #[cfg(target_os = "linux")]
            held.observed.check_lifetime()?;
            current_source(&held.media, &held.timing)?;
            require_grant(db, &held.owner, &held.project, Scope::Generate)?;
            let checked = inference_equivalence_store::read_current_accepted(
                db,
                &held.reference.dossier_id,
                held.reference.expected_revision,
                &held.reference.expected_report_sha256,
                inference_fallback_execution::wall_time()?,
            )?;
            if !held.selection.matches_subject(
                &held.engine_epoch,
                &entry.report.options,
                stream,
                &checked.report().manifest.subject,
            )? {
                return Err(changed(
                    "browser fallback accepted scope changed before dispatch",
                ));
            }
            #[cfg(target_os = "linux")]
            held.require_environment(&checked.report().manifest.subject)?;
            Ok(())
        }
        _ => Err(changed(
            "browser fallback dispatch lacks its exact held preparation",
        )),
    }
}

/// Passive checks use durable decision state and the original private binding;
/// a contended/new timing publication is not evidence of lifetime retirement.
pub(super) fn require_evidence(db: &Connection, entry: &Entry) -> PResult<()> {
    match (&entry.report.options.inference_fallback, &entry.fallback) {
        (None, None) => Ok(()),
        (Some(_), Some(held)) => {
            held.check_lifetime()?;
            held.record_matches(entry)?;
            require_grant(db, &held.owner, &held.project, Scope::Generate)?;
            let checked = inference_equivalence_store::read_current_accepted(
                db,
                &held.reference.dossier_id,
                held.reference.expected_revision,
                &held.reference.expected_report_sha256,
                inference_fallback_execution::wall_time()?,
            )?;
            let subject = &checked.report().manifest.subject;
            let (profile, environment) = if held.ordinal == 0 {
                (&subject.from_profile, &subject.from_environment)
            } else {
                (&subject.to_profile, &subject.to_environment)
            };
            let bound = held.binding.lock().map_err(internal)?;
            let bound = bound
                .as_ref()
                .ok_or_else(|| changed("browser fallback admission absent"))?;
            if !held.selection.matches_capture(
                &held.engine_epoch,
                &entry.report.options,
                subject,
            )? || hash(profile)? != held.profile_sha256
                || environment.identity_sha256()?
                    != bound.admission.observed_environment.identity_sha256()?
            {
                return Err(changed(
                    "browser fallback current decision no longer covers original admission",
                ));
            }
            Ok(())
        }
        _ => Err(changed(
            "browser fallback report has no matching native admission",
        )),
    }
}

/// Native CUDA startup shares the same proof checks; I/O observations precede
/// the authority guards and actual publication/dispatch remains under them.
pub(in crate::authority) fn verify_dispatch(
    engine: &Engine,
    stream: &LiveInferenceManifest,
) -> PResult<()> {
    let observed = recheck_io(engine, &stream.identity.epoch, &stream.attempt_id)?;
    let db = engine.db.lock().map_err(internal)?;
    let mut registry = engine.live_captures.lock().map_err(internal)?;
    registry.sweep(&db)?;
    let entry = inference::active(&mut registry, &stream.identity.epoch, &stream.attempt_id)
        .ok_or_else(|| changed("browser fallback original capture retired before dispatch"))?;
    require_bound(&db, entry, stream, observed.as_ref())
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    #[test]
    fn worker_failure_does_not_cancel_root_but_root_stop_reaches_all_registrations() {
        let lifetime = Lifetime::new(Instant::now() + Duration::from_secs(5));
        let failed = Arc::new(AtomicBool::new(false));
        lifetime.register(&failed);
        failed.store(true, Ordering::Release);
        lifetime.check().unwrap();
        let target = Arc::new(AtomicBool::new(false));
        lifetime.register(&target);
        assert!(!target.load(Ordering::Acquire));
        lifetime.cancel();
        assert_eq!(
            lifetime.retirement_reason(),
            Some(LiveCaptureRetirement::Stopped)
        );
        assert!(failed.load(Ordering::Acquire) && target.load(Ordering::Acquire));
        assert_eq!(lifetime.check().unwrap_err().code, ErrorCode::Cancelled);
        let racing_target = Arc::new(AtomicBool::new(false));
        lifetime.register(&racing_target);
        assert!(racing_target.load(Ordering::Acquire));
    }
    #[test]
    fn shared_deadline_does_not_restart_when_another_worker_registers() {
        let lifetime = Lifetime::new(Instant::now());
        assert_eq!(
            lifetime.retirement_reason(),
            Some(LiveCaptureRetirement::Expired)
        );
        assert!(lifetime.check().is_err());
        let target = Arc::new(AtomicBool::new(false));
        lifetime.register(&target);
        assert!(lifetime.check().is_err());
        assert!(target.load(Ordering::Acquire));
    }
}
