//! Explicit browser fallback intent and inert admission observations. Neither
//! serialized form constructs current evidence, source custody or failure proof.
use super::*;

pub const BROWSER_INFERENCE_FALLBACK_SELECTION_ID_PREFIX: &str =
    "browser-inference-fallback-selection-";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackSelection {
    pub schema_version: u16,
    pub tier: InferenceEquivalenceTier,
    pub axis: Axis,
    /// Preference order is owner-declared and must not be reordered.
    pub equivalents: Vec<InferenceFallbackReference>,
    pub maximum_fallbacks: u8,
    pub source_policy: BrowserInferenceFallbackSourcePolicy,
    pub view_policy: BrowserInferenceFallbackViewPolicy,
}
impl BrowserInferenceFallbackSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported browser fallback selection version",
        )?;
        // The current live temporal adapter produces one script-position
        // channel. Other axes require their own actual live output contract.
        check(
            self.axis == Axis::Stroke,
            "browser temporal fallback currently requires an explicit stroke axis",
        )?;
        validate_fallback_references(&self.equivalents, self.maximum_fallbacks)?;
        self.source_policy.validate()?;
        self.view_policy.validate()?;
        encoded(self).map(|_| ())
    }
    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        self.validate()?;
        let mut value = identity(self)?;
        value.artifact_id = ArtifactId::new(format!(
            "{BROWSER_INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
            value.sha256
        ))
        .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        Ok(value)
    }
    /// Metadata correspondence only; Engine must resolve the actual current
    /// accepted record and held source/environment before publishing admission.
    pub fn matches_subject(
        &self,
        engine_epoch: &AttemptId,
        options: &LiveCaptureOptions,
        stream: &LiveInferenceManifest,
        subject: &InferenceEquivalenceSubject,
    ) -> Result<bool, ProtocolError> {
        Ok(self.matches_capture(engine_epoch, options, subject)?
            && self
                .source_policy
                .matches_manifest(engine_epoch, options, stream)?)
    }
    pub fn matches_capture(
        &self,
        engine_epoch: &AttemptId,
        options: &LiveCaptureOptions,
        subject: &InferenceEquivalenceSubject,
    ) -> Result<bool, ProtocolError> {
        self.validate()?;
        subject.validate()?;
        Ok(subject.scope.tier == self.tier
            && subject.scope.axes.contains(&self.axis)
            && subject.scope.recipe == InferenceEquivalenceRecipe::TemporalPosition
            && subject.scope.source_policy == self.source_policy.identity()?
            && subject.scope.view_policy == self.view_policy.identity()?
            && self.source_policy.matches_capture(engine_epoch, options)?
            && self.view_policy.matches_capture(options)?)
    }
}

/// Historical description of an actual admission, not a reusable execution
/// token. Original native capture/worker/source custody remains private.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackAdmission {
    pub schema_version: u16,
    pub selection: ArtifactIdentity,
    pub reference: InferenceFallbackReference,
    pub semantic_identity: ArtifactIdentity,
    pub observed_environment: InferenceEquivalenceEnvironment,
    pub admitted_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub ordinal: u8,
    pub predecessor: Option<LiveIdentity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<BrowserInferenceFallbackStep>,
}
impl BrowserInferenceFallbackAdmission {
    pub fn validate(&self, capture: &LiveCaptureReport) -> Result<(), ProtocolError> {
        let attempt = &capture
            .inference
            .as_ref()
            .ok_or_else(|| {
                ProtocolError::invalid("browser fallback history requires an inference attempt")
            })?
            .attempt_id;
        self.validate_scope(&capture.options, &capture.identity, attempt)
    }
    pub(crate) fn validate_scope(
        &self,
        options: &LiveCaptureOptions,
        capture_identity: &LiveIdentity,
        current_attempt: &AttemptId,
    ) -> Result<(), ProtocolError> {
        let selected = options.inference_fallback.as_ref().ok_or_else(|| {
            ProtocolError::invalid("browser fallback admission lacks original selection")
        })?;
        selected.validate()?;
        self.reference.validate()?;
        self.observed_environment.validate()?;
        self.semantic_identity.validate()?;
        let profile = &options
            .inference
            .as_ref()
            .ok_or_else(|| {
                ProtocolError::invalid("browser fallback admission requires a resolved profile")
            })?
            .profile;
        let reference = encoded(&self.reference)?;
        let mut selected_reference = false;
        for candidate in &selected.equivalents {
            selected_reference |= encoded(candidate)? == reference;
        }
        check(
            self.schema_version == 1
                && self.selection == selected.identity()?
                && self
                    .observed_environment
                    .dependencies
                    .contains(&profile.runtime)
                && selected_reference
                && self.semantic_identity == browser_inference_semantic_identity(options)?
                && self.admitted_at_unix_ms > 0
                && self.admitted_at_unix_ms < self.expires_at_unix_ms
                && self.expires_at_unix_ms <= i64::MAX as u64
                && self.ordinal <= selected.maximum_fallbacks
                && self.history.len() == usize::from(self.ordinal)
                && (self.ordinal == 0) == self.predecessor.is_none(),
            "browser fallback admission differs from selected scope or original history",
        )?;
        if let Some(previous) = &self.predecessor {
            check(
                previous.project == capture_identity.project
                    && previous.revision == capture_identity.revision
                    && previous.source == capture_identity.source
                    && previous.epoch != capture_identity.epoch,
                "browser fallback predecessor scope differs",
            )?;
        }
        let current_profile = identity(profile)?.sha256;
        let allowed_references = selected
            .equivalents
            .iter()
            .map(encoded)
            .collect::<Result<Vec<_>, _>>()?;
        let mut epochs = std::collections::HashSet::new();
        let mut attempts = std::collections::HashSet::new();
        for (index, step) in self.history.iter().enumerate() {
            step.failure.validate()?;
            step.reference.validate()?;
            let stream = &step.failure.manifest.stream;
            let next_profile = self
                .history
                .get(index + 1)
                .map(|next| identity(&next.failure.manifest.stream.profile).map(|id| id.sha256))
                .transpose()?
                .unwrap_or_else(|| current_profile.clone());
            check(
                stream.identity.project == capture_identity.project
                    && stream.identity.revision == capture_identity.revision
                    && stream.identity.source == capture_identity.source
                    && stream.identity.epoch != capture_identity.epoch
                    && stream.attempt_id != *current_attempt
                    && stream.model.identity == profile.model
                    && epochs.insert(&stream.identity.epoch)
                    && attempts.insert(&stream.attempt_id)
                    && step.next_profile_sha256 == next_profile
                    && allowed_references.contains(&encoded(&step.reference)?),
                "browser fallback history scope, chain or selected reference differs",
            )?;
        }
        if let Some(last) = self.history.last() {
            check(
                self.predecessor.as_ref() == Some(&last.failure.manifest.stream.identity)
                    && encoded(&self.reference)? == encoded(&last.reference)?,
                "browser fallback admission lost its original failure step",
            )?;
        }
        Ok(())
    }
}

/// Inert original failure metadata. Only private Engine transport and retirement
/// custody can establish that this observation actually occurred.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackFailure {
    pub manifest: Box<LiveCudaInferenceManifest>,
    pub observation: InferenceProviderUnavailable,
}
impl BrowserInferenceFallbackFailure {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.manifest.validate()?;
        self.observation.validate_for(
            &self.manifest.stream.profile,
            &identity(&self.manifest.stream.profile)?.sha256,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackStep {
    pub failure: BrowserInferenceFallbackFailure,
    pub reference: InferenceFallbackReference,
    pub next_profile_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserInferenceFallbackPhase {
    Preparing,
    Started,
    Refused,
}

/// A status link for the first-party producer. It never restores a failed
/// worker, source pin, cancellation token or permission to launch a target.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceFallbackTransition {
    pub failure: BrowserInferenceFallbackFailure,
    pub phase: BrowserInferenceFallbackPhase,
    pub next_epoch: Option<AttemptId>,
    pub error: Option<ProtocolError>,
}
impl BrowserInferenceFallbackTransition {
    pub fn validate(&self, capture: &LiveCaptureReport) -> Result<(), ProtocolError> {
        self.failure.validate()?;
        let info = capture
            .inference
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("fallback transition lacks inference"))?;
        check(
            !capture.active
                && info.fallback.is_some()
                && matches!(
                    info.state,
                    LiveCaptureInferenceState::Failed | LiveCaptureInferenceState::Retired
                ),
            "fallback transition requires a retired declared capture",
        )?;
        self.validate_bindings(
            &capture.identity,
            &capture.clock,
            &info.attempt_id,
            &info.profile_sha256,
            capture.options.lease_ms,
        )
    }
    fn validate_bindings(
        &self,
        capture_identity: &LiveIdentity,
        clock: &LivePresentationClock,
        attempt: &AttemptId,
        profile_sha256: &str,
        lease_ms: u32,
    ) -> Result<(), ProtocolError> {
        self.failure.validate()?;
        check(
            self.failure.manifest.stream.identity == *capture_identity
                && self.failure.manifest.stream.clock == *clock
                && self.failure.manifest.stream.budget.wall_time_ms == u64::from(lease_ms)
                && self.failure.manifest.stream.attempt_id == *attempt
                && self.failure.observation.profile_sha256 == profile_sha256
                && self
                    .next_epoch
                    .as_ref()
                    .is_none_or(|epoch| *epoch != capture_identity.epoch)
                && (self.phase == BrowserInferenceFallbackPhase::Started)
                    == self.next_epoch.is_some()
                && (self.phase == BrowserInferenceFallbackPhase::Refused) == self.error.is_some(),
            "browser fallback transition scope or phase differs",
        )
    }
}

/// Original fallback declarations retained with a recording. These bytes are
/// inert provenance; the Engine separately checks its exact admission receipt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInferenceRecordingProvenance {
    pub options: Box<LiveCaptureOptions>,
    pub admission: BrowserInferenceFallbackAdmission,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<BrowserInferenceFallbackTransition>,
}

/// Descriptive correspondence for a target link returned by the Engine. The
/// caller must obtain both reports from its existing authenticated connection.
pub fn validate_browser_fallback_target(
    previous: &LiveCaptureReport,
    target: &LiveCaptureReport,
) -> Result<(), ProtocolError> {
    previous.validate()?;
    target.validate()?;
    let old = previous
        .inference
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback predecessor inference absent"))?;
    let new = target
        .inference
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback target inference absent"))?;
    let transition = old
        .fallback_transition
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback target link absent"))?;
    let before = old
        .fallback
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback predecessor admission absent"))?;
    let after = new
        .fallback
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("fallback target admission absent"))?;
    let a = previous
        .options
        .browser_analysis
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("original browser feed absent"))?;
    let b = target
        .options
        .browser_analysis
        .as_ref()
        .ok_or_else(|| ProtocolError::invalid("target browser feed absent"))?;
    check(
        transition.phase == BrowserInferenceFallbackPhase::Started
            && transition.next_epoch.as_ref() == Some(&target.identity.epoch)
            && previous.engine_epoch == target.engine_epoch
            && previous.identity.project == target.identity.project
            && previous.identity.revision == target.identity.revision
            && previous.identity.source == target.identity.source
            && previous.identity.epoch != target.identity.epoch
            && old.attempt_id != new.attempt_id
            && old.profile_sha256 != new.profile_sha256
            && before.selection == after.selection
            && before.semantic_identity == after.semantic_identity
            && Some(after.ordinal) == before.ordinal.checked_add(1)
            && after.history.len() == before.history.len() + 1
            && encoded(&after.history[..before.history.len()])? == encoded(&before.history)?
            && encoded(
                &after
                    .history
                    .last()
                    .ok_or_else(|| ProtocolError::invalid("target failure history absent"))?
                    .failure,
            )? == encoded(&transition.failure)?
            && previous.options.browser_media == target.options.browser_media
            && a.player == b.player
            && a.timing_binding_id == b.timing_binding_id
            && a.policy == b.policy
            && a.feed_id != b.feed_id,
        "fallback target changed original source, selection, failure chain or feed correspondence",
    )
}
impl BrowserInferenceRecordingProvenance {
    pub fn from_capture(capture: &LiveCaptureReport) -> Result<Option<Self>, ProtocolError> {
        let info = capture.inference.as_ref();
        match (
            &capture.options.inference_fallback,
            info.and_then(|i| i.fallback.as_ref()),
        ) {
            (None, None) if info.is_none_or(|i| i.fallback_transition.is_none()) => Ok(None),
            (Some(_), Some(admission)) => {
                admission.validate(capture)?;
                if let Some(transition) = info.and_then(|i| i.fallback_transition.as_ref()) {
                    transition.validate(capture)?;
                }
                Ok(Some(Self {
                    options: Box::new(capture.options.clone()),
                    admission: admission.clone(),
                    transition: info.and_then(|i| i.fallback_transition.clone()),
                }))
            }
            _ => Err(ProtocolError::invalid(
                "recording fallback intent/admission differs",
            )),
        }
    }
    pub fn validate_page(&self, page: &LiveRecordingPage) -> Result<(), ProtocolError> {
        self.options.validate()?;
        let declared = self
            .options
            .inference
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("recorded fallback profile absent"))?;
        check(
            self.options.browser_media == page.browser_media
                && self.options.model_pack == page.model_pack
                && self.options.source_time_basis == page.source_time_basis
                && self.options.clock(page.identity.epoch.clone()) == page.clock
                && encoded(&declared.profile)? == encoded(&page.profile)?,
            "recording fallback options differ from page scope",
        )?;
        self.admission
            .validate_scope(&self.options, &page.identity, &page.inference_attempt)?;
        if page.profile.provider == InferenceProvider::Cpu {
            check(
                page.cpu_manifest
                    .as_ref()
                    .is_some_and(|m| m.stream.worker == self.admission.observed_environment.worker),
                "recorded fallback lacks its original captured CPU worker",
            )?;
        }
        if let Some(transition) = &self.transition {
            check(
                !page.active && page.total_samples == 0,
                "pre-Ready provider failure cannot carry recorded estimates",
            )?;
            transition.validate_bindings(
                &page.identity,
                &page.clock,
                &page.inference_attempt,
                &identity(&page.profile)?.sha256,
                self.options.lease_ms,
            )?;
        }
        encoded(self).map(|_| ())
    }
}
