//! Separate browser/source evidence and historical start decisions; never native authority.
use crate::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

pub const HANDY_BROWSER_START_MANIFEST_LIMIT: u64 = 128 * 1024;
pub const HANDY_BROWSER_START_EVIDENCE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyBrowserStartReference {
    Callback,
    Composition,
    ExpectedDisplay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartTiming {
    pub clock: BrowserClockAssumptions,
    pub reference: HandyBrowserStartReference,
    pub display_early_ns: u64,
    pub display_late_ns: u64,
    pub source_correspondence_error_ns: u64,
    pub discrete_media_phase_error_ns: u64,
    pub media_rate_error_ppm: u32,
    pub physical_execution_error_ns: u64,
    pub maximum_start_age_ns: u64,
    pub maximum_alignment_error_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartSubject {
    pub schema_version: u8,
    pub origin: HandyEvidenceOrigin,
    pub context: HandyQualificationContext,
    pub device_qualification: HandyQualificationBinding,
    pub browser_environment: ArtifactIdentity,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub selection: BrowserMediaSelection,
    pub clock: DeclaredSourceClock,
    pub master_sha256: String,
    pub script_actor_session: SessionId,
    pub script_request_id: RequestId,
    pub script: ArtifactIdentity,
    pub timing: HandyBrowserStartTiming,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyBrowserStartEvidenceKind {
    EnvironmentConfiguration,
    ClockAndDisplay,
    MediaProgression,
    SourceCorrespondence,
}
impl HandyBrowserStartEvidenceKind {
    pub const ALL: [Self; 4] = [
        Self::EnvironmentConfiguration,
        Self::ClockAndDisplay,
        Self::MediaProgression,
        Self::SourceCorrespondence,
    ];
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationInputEvidence {
    pub kind: HandyBrowserStartEvidenceKind,
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationInput {
    pub subject: HandyBrowserStartSubject,
    pub evidence: Vec<HandyBrowserStartQualificationInputEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationEvidence {
    pub kind: HandyBrowserStartEvidenceKind,
    pub identity: ArtifactIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationManifest {
    pub subject: HandyBrowserStartSubject,
    pub evidence: Vec<HandyBrowserStartQualificationEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationBinding {
    pub dossier_id: ArtifactId,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationDecision {
    pub revision: u64,
    pub state: HandyQualificationState,
    pub request_id: RequestId,
    pub actor_session: SessionId,
    pub operator: Option<String>,
    pub note: String,
    pub recorded_at_unix_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationReport {
    pub dossier_id: ArtifactId,
    pub revision: u64,
    pub state: HandyQualificationState,
    pub manifest: HandyBrowserStartQualificationManifest,
    pub decision: Option<HandyBrowserStartQualificationDecision>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartQualificationReceipt {
    pub input_sha256: Option<String>,
    pub request_id: RequestId,
    pub report: HandyBrowserStartQualificationReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserStartDecision {
    pub qualification: HandyBrowserStartQualificationBinding,
    pub subject: HandyBrowserStartSubject,
    pub timing_binding: BrowserTimingBinding,
    pub observation: BrowserTimingObservation,
    pub reference_boottime_ns: u64,
    pub alignment: BrowserStartAlignment,
}

/// Current native-owner observation; does not rewrite the original start decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyBrowserPlaybackReport {
    pub project_id: ProjectId,
    pub browser_timing_binding_id: ArtifactId,
    pub play_request_id: RequestId,
    pub play_record_id: Option<ArtifactId>,
    pub controller: Option<HandyControllerLease>,
    pub current: bool,
    pub physical_stop_pending: bool,
    pub browser_observation_id: Option<ArtifactId>,
    pub alignment: Option<BrowserStartAlignment>,
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(format!("browser start: {message}"))
}
fn core_error(error: CoreError) -> ProtocolError {
    invalid(&error.to_string())
}
fn checked_artifact(value: &ArtifactIdentity, limit: u64) -> Result<(), ProtocolError> {
    value.validate()?;
    if value.artifact_id.as_str() != value.sha256 || value.byte_len == 0 || value.byte_len > limit {
        return Err(invalid("invalid content identity or size"));
    }
    Ok(())
}
fn input_path(path: &std::path::Path) -> Result<(), ProtocolError> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if !path.is_absolute()
        || bytes.len() > 32768
        || bytes.contains(&0)
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid("invalid bounded absolute input path"));
    }
    Ok(())
}
fn revision(value: u64) -> Result<(), ProtocolError> {
    if value == 0 || value >= i64::MAX as u64 {
        return Err(invalid("invalid decision revision"));
    }
    Ok(())
}
impl HandyBrowserStartTiming {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.clock.validate().map_err(core_error)?;
        if self.media_rate_error_ppm >= 1_000_000
            || self.maximum_start_age_ns == 0
            || self.maximum_start_age_ns > self.clock.maximum_age_ns
            || self.maximum_alignment_error_ns == 0
            || self
                .display_early_ns
                .checked_add(self.display_late_ns)
                .and_then(|n| n.checked_add(self.source_correspondence_error_ns))
                .and_then(|n| n.checked_add(self.discrete_media_phase_error_ns))
                .and_then(|n| n.checked_add(self.physical_execution_error_ns))
                .is_none()
        {
            return Err(invalid("invalid explicit timing bounds"));
        }
        Ok(())
    }
    pub fn reference_tick(&self, frame: &BrowserTimingFrame) -> Result<u64, ProtocolError> {
        self.validate()?;
        frame.validate()?;
        match self.reference {
            HandyBrowserStartReference::Callback => Ok(frame.callback_tick_us),
            HandyBrowserStartReference::Composition => frame
                .presentation_time_us
                .ok_or_else(|| invalid("composition reference absent")),
            HandyBrowserStartReference::ExpectedDisplay => frame
                .expected_display_time_us
                .ok_or_else(|| invalid("expected display reference absent")),
        }
    }
    pub fn phase_request(
        &self,
        observation: &BrowserTimingObservation,
        reference_boottime_ns: u64,
        script_quantization_error_ns: u64,
        server_device_uncertainty_ns: u64,
        valid_until_boottime_ns: u64,
    ) -> Result<BrowserStartPhaseRequest, ProtocolError> {
        self.validate()?;
        observation.validate()?;
        if !observation.frame.is_current() || script_quantization_error_ns > 500_000 {
            return Err(invalid("ineligible frame or script quantization"));
        }
        Ok(BrowserStartPhaseRequest {
            bracket: observation.bracket,
            browser_reference_us: self.reference_tick(&observation.frame)?,
            observed_project_phase: observation.project_phase,
            reference_boottime_ns,
            clock: self.clock,
            display_early_ns: self.display_early_ns,
            display_late_ns: self.display_late_ns,
            source_correspondence_error_ns: self.source_correspondence_error_ns,
            discrete_media_phase_error_ns: self.discrete_media_phase_error_ns,
            media_rate_error_ppm: self.media_rate_error_ppm,
            script_quantization_error_ns,
            server_device_uncertainty_ns,
            physical_execution_error_ns: self.physical_execution_error_ns,
            maximum_start_age_ns: self.maximum_start_age_ns,
            maximum_alignment_error_ns: self.maximum_alignment_error_ns,
            valid_until_boottime_ns: valid_until_boottime_ns
                .min(observation.expires_at_boottime_ns),
        })
    }
}
impl HandyBrowserStartSubject {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.context.validate()?;
        self.device_qualification.validate()?;
        checked_artifact(
            &self.browser_environment,
            HANDY_BROWSER_START_EVIDENCE_LIMIT,
        )?;
        checked_artifact(&self.script, HANDY_STROKE_SCRIPT_MAX_BYTES)?;
        self.selection.validate()?;
        self.clock.validate().map_err(core_error)?;
        self.timing.validate()?;
        validate_sha256(&self.master_sha256)?;
        let (_, end) = self
            .clock
            .presentation_trim_micros(&self.selection.placement.spec)
            .map_err(core_error)?;
        if self.schema_version != 1
            || self.selection.placement.created_revision > self.revision
            || i64::from(self.selection.placement.spec.rate_numerator)
                != i64::from(self.selection.placement.spec.rate_denominator)
            || end > BROWSER_MEDIA_MAX_EXACT_INTEGER
        {
            return Err(invalid("unsupported subject or placement"));
        }
        Ok(())
    }
}
impl HandyBrowserStartQualificationInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        if self.evidence.len() != 4 {
            return Err(invalid("four evidence roles required"));
        }
        let mut kinds = BTreeSet::new();
        for item in &self.evidence {
            input_path(&item.path)?;
            validate_sha256(&item.sha256)?;
            if !kinds.insert(item.kind)
                || item.byte_len == 0
                || item.byte_len > HANDY_BROWSER_START_EVIDENCE_LIMIT
                || (item.kind == HandyBrowserStartEvidenceKind::EnvironmentConfiguration
                    && (item.sha256 != self.subject.browser_environment.sha256
                        || item.byte_len != self.subject.browser_environment.byte_len))
            {
                return Err(invalid("evidence input differs or exceeds limits"));
            }
        }
        Ok(())
    }
}
impl HandyBrowserStartQualificationManifest {
    pub fn identity(&self) -> Result<ArtifactId, ProtocolError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| invalid("manifest encoding failed"))?;
        if bytes.len() as u64 > HANDY_BROWSER_START_MANIFEST_LIMIT {
            return Err(invalid("manifest too large"));
        }
        ArtifactId::new(format!("{:x}", Sha256::digest(bytes))).map_err(core_error)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        if !self
            .evidence
            .iter()
            .map(|e| e.kind)
            .eq(HandyBrowserStartEvidenceKind::ALL)
        {
            return Err(invalid("evidence roles must be complete and canonical"));
        }
        for item in &self.evidence {
            checked_artifact(&item.identity, HANDY_BROWSER_START_EVIDENCE_LIMIT)?;
            if item.kind == HandyBrowserStartEvidenceKind::EnvironmentConfiguration
                && item.identity != self.subject.browser_environment
            {
                return Err(invalid("environment evidence differs from subject"));
            }
        }
        Ok(())
    }
}
impl HandyBrowserStartQualificationBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.dossier_id.as_str())?;
        revision(self.revision)?;
        if self.revision < 2 {
            return Err(invalid(
                "qualification binding requires a reviewed revision",
            ));
        }
        Ok(())
    }
}
impl HandyBrowserStartQualificationReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        revision(self.revision)?;
        if self.manifest.identity()? != self.dossier_id
            || (self.state == HandyQualificationState::Pending) != self.decision.is_none()
            || (self.state == HandyQualificationState::Pending && self.revision != 1)
            || (self.state == HandyQualificationState::Accepted
                && self.manifest.subject.origin != HandyEvidenceOrigin::Physical)
        {
            return Err(invalid("inconsistent dossier identity or review state"));
        }
        if let Some(d) = &self.decision {
            if d.revision != self.revision
                || d.revision < 2
                || d.state != self.state
                || d.recorded_at_unix_ms == 0
            {
                return Err(invalid("inconsistent decision history"));
            }
            match d.state {
                HandyQualificationState::Accepted => {
                    validate_handy_qualification_note(
                        d.operator
                            .as_deref()
                            .ok_or_else(|| invalid("operator acknowledgment missing"))?,
                    )?;
                    if !d.note.is_empty() {
                        return Err(invalid("acceptance carries an unauthored note"));
                    }
                }
                HandyQualificationState::Withdrawn => {
                    if d.operator.is_some() {
                        return Err(invalid("withdrawal carries an unauthored operator"));
                    }
                    validate_handy_qualification_note(&d.note)?;
                }
                HandyQualificationState::Pending => {
                    return Err(invalid("pending dossier has a decision"))
                }
            }
        }
        Ok(())
    }
}
impl HandyBrowserStartQualificationReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(hash) = &self.input_sha256 {
            validate_sha256(hash)?;
        }
        self.report.validate()
    }
}
impl HandyBrowserStartDecision {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.qualification.validate()?;
        self.subject.validate()?;
        BrowserTimingReport {
            binding: self.timing_binding.clone(),
            current: true,
            observation: Some(self.observation.clone()),
            qualified: false,
        }
        .validate()?;
        let media = &self.timing_binding.browser_media;
        if self.subject.project_id != self.timing_binding.project_id
            || self.subject.revision != self.timing_binding.revision
            || self.subject.selection != media.selection
            || self.subject.clock != media.clock
        {
            return Err(invalid("decision source or project differs"));
        }
        let request = self.subject.timing.phase_request(
            &self.observation,
            self.reference_boottime_ns,
            self.alignment.script_quantization_error_ns,
            self.alignment.server_device_uncertainty_ns,
            self.alignment.valid_until_boottime_ns,
        )?;
        if request.calculate().map_err(core_error)? != self.alignment {
            return Err(invalid("decision interval differs"));
        }
        // Check the complete uncertainty interval AND selected wire phase against
        // the exact half-open placement; the Engine separately checks real script coverage.
        for ns in [
            self.alignment.phase_earliest_ns,
            self.alignment.phase_latest_ns,
            self.alignment
                .start_time_ms
                .checked_mul(1_000_000)
                .ok_or_else(|| invalid("wire phase overflow"))?,
        ] {
            let ns = i64::try_from(ns).map_err(|_| invalid("project phase overflow"))?;
            self.subject
                .selection
                .placement
                .spec
                .source_at_project(ProjectTime::from_nanos(ns))
                .map_err(core_error)?;
        }
        Ok(())
    }
    pub fn start_time_ms(&self) -> u64 {
        self.alignment.start_time_ms
    }
    pub fn valid_until_boottime_ns(&self) -> u64 {
        self.alignment.valid_until_boottime_ns
    }
}
impl HandyBrowserPlaybackReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(id) = &self.play_record_id {
            validate_sha256(id.as_str())?;
        }
        if let Some(controller) = &self.controller {
            controller.validate()?;
            if controller.project_id != self.project_id {
                return Err(invalid("owner controller project differs"));
            }
        }
        if let Some(alignment) = &self.alignment {
            alignment.validate().map_err(core_error)?;
        }
        let pre_record = self.play_record_id.is_none()
            && self.alignment.is_none()
            && self.browser_observation_id.is_none();
        let post_record = self.play_record_id.is_some()
            && self.alignment.is_some()
            && self.browser_observation_id.is_some();
        // Before durable admission, only an Active controller may own this
        // pending request. Stop custody is present without an execution claim or
        // acknowledgment. Once admitted, retain the complete evidence triple.
        if (!pre_record && !post_record)
            || (self.current
                && (!self.physical_stop_pending
                    || self.controller.as_ref().is_none_or(|controller| {
                        controller.state == HandyControllerState::Retired
                    })
                    || (pre_record
                        && self.controller.as_ref().is_none_or(|controller| {
                            controller.state != HandyControllerState::Active
                        }))))
        {
            return Err(invalid("inconsistent current owner report"));
        }
        Ok(())
    }
}

pub(crate) fn validate_request(r: &Request) -> Result<(), ProtocolError> {
    let (proof, revision_required) = match &r.command {
        Command::ImportHandyBrowserStartQualification {
            path,
            manifest_sha256,
            owner_proof,
        } => {
            input_path(path)?;
            validate_sha256(manifest_sha256)?;
            (owner_proof, true)
        }
        Command::HandyBrowserStartQualificationStatus {
            dossier_id,
            owner_proof,
        } => {
            validate_sha256(dossier_id.as_str())?;
            (owner_proof, false)
        }
        Command::AcceptHandyBrowserStartQualification {
            dossier_id,
            expected_record_revision,
            operator,
            acknowledge_measured_evidence,
            owner_proof,
        } => {
            validate_sha256(dossier_id.as_str())?;
            revision(*expected_record_revision)?;
            validate_handy_qualification_note(operator)?;
            if !acknowledge_measured_evidence {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "measured evidence requires explicit operator acknowledgment",
                ));
            }
            (owner_proof, true)
        }
        Command::WithdrawHandyBrowserStartQualification {
            dossier_id,
            expected_record_revision,
            reason,
            owner_proof,
        } => {
            validate_sha256(dossier_id.as_str())?;
            revision(*expected_record_revision)?;
            validate_handy_qualification_note(reason)?;
            (owner_proof, false)
        }
        Command::StartHandyPlaybackFromBrowser {
            acquisition_request_id,
            expected_generation,
            clock_request_id,
            qualification,
            acknowledge_current_environment,
            allow_play,
            owner_proof,
            ..
        } => {
            qualification.validate()?;
            if acquisition_request_id == &r.request_id
                || clock_request_id == &r.request_id
                || *expected_generation > i64::MAX as u64
            {
                return Err(invalid("start requires exact distinct request identities"));
            }
            if !acknowledge_current_environment || !allow_play {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "browser start requires explicit environment acknowledgment and play consent",
                ));
            }
            (owner_proof, true)
        }
        Command::RenewHandyBrowserPlayback {
            play_request_id,
            play_record_id,
            owner_proof,
            ..
        }
        | Command::ReleaseHandyBrowserPlayback {
            play_request_id,
            play_record_id,
            owner_proof,
            ..
        }
        | Command::HandyBrowserPlaybackStatus {
            play_request_id,
            play_record_id,
            owner_proof,
            ..
        }
        | Command::RefreshHandyBrowserPlaybackTiming {
            play_request_id,
            play_record_id,
            owner_proof,
            ..
        } => {
            if play_request_id == &r.request_id {
                return Err(invalid("owner operation requires its own request identity"));
            }
            if let Some(id) = play_record_id {
                validate_sha256(id.as_str())?;
            }
            if matches!(
                r.command,
                Command::RefreshHandyBrowserPlaybackTiming {
                    allow_online_inspection: false,
                    ..
                }
            ) {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "browser device refresh requires explicit inspection consent",
                ));
            }
            (owner_proof, false)
        }
        _ => return Ok(()),
    };
    bounded_text(proof, 64, "Handy owner capability")?;
    if r.expected_revision.is_some() != revision_required {
        return Err(invalid("incorrect project revision semantics"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn artifact(c: char, bytes: u64) -> ArtifactIdentity {
        let hash = c.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: bytes,
        }
    }
    fn subject() -> HandyBrowserStartSubject {
        let media = crate::browser_media::test_binding();
        HandyBrowserStartSubject {
            schema_version: 1,
            origin: HandyEvidenceOrigin::Synthetic,
            context: HandyQualificationContext {
                schema_version: 1,
                control_image: artifact('a', 1),
                platform: "linux/authored-fixture".into(),
                transport: ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap(),
            },
            device_qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                revision: 2,
            },
            browser_environment: artifact('c', 16),
            project_id: media.project_id,
            revision: media.revision,
            selection: media.selection,
            clock: media.clock,
            master_sha256: "e".repeat(64),
            script_actor_session: SessionId::new("script-owner").unwrap(),
            script_request_id: RequestId::new("script-export").unwrap(),
            script: artifact('d', 32),
            timing: HandyBrowserStartTiming {
                clock: BrowserClockAssumptions {
                    timestamp_error_ns: 1000,
                    relative_rate_error_ppm: 100,
                    maximum_age_ns: 1_000_000_000,
                    maximum_interval_ns: 10_000_000,
                },
                reference: HandyBrowserStartReference::ExpectedDisplay,
                display_early_ns: 1_000_000,
                display_late_ns: 2_000_000,
                source_correspondence_error_ns: 1000,
                discrete_media_phase_error_ns: 1_000_000,
                media_rate_error_ppm: 100,
                physical_execution_error_ns: 2_000_000,
                maximum_start_age_ns: 500_000_000,
                maximum_alignment_error_ns: 20_000_000,
            },
        }
    }
    fn manifest() -> HandyBrowserStartQualificationManifest {
        let s = subject();
        let evidence = HandyBrowserStartEvidenceKind::ALL
            .into_iter()
            .map(|kind| HandyBrowserStartQualificationEvidence {
                kind,
                identity: if kind == HandyBrowserStartEvidenceKind::EnvironmentConfiguration {
                    s.browser_environment.clone()
                } else {
                    artifact('f', 8)
                },
            })
            .collect();
        HandyBrowserStartQualificationManifest {
            subject: s,
            evidence,
        }
    }
    fn decision() -> HandyBrowserStartDecision {
        let subject = subject();
        let media = crate::browser_media::test_binding();
        let timing_binding = BrowserTimingBinding {
            request_id: RequestId::new("timing-open").unwrap(),
            binding_id: ArtifactId::new("timing-binding").unwrap(),
            engine_epoch: media.engine_epoch.clone(),
            project_id: media.project_id.clone(),
            revision: media.revision,
            player: HandyPlayerIdentity {
                launch_id: AttemptId::new("launch").unwrap(),
                page_id: AttemptId::new("page").unwrap(),
                player_id: AttemptId::new("player").unwrap(),
                source_id: AttemptId::new("selection").unwrap(),
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            },
            browser_media: media,
            policy: BrowserTimingPolicy {
                probe_timeout_ms: 100,
                observation_timeout_ms: 1000,
            },
        };
        let observation = BrowserTimingObservation {
            observation_id: ArtifactId::new("observation").unwrap(),
            binding_id: timing_binding.binding_id.clone(),
            sequence: 1,
            frame: BrowserTimingFrame {
                sample: HandyPlayerSample {
                    browser_tick_us: 1_000_000,
                    media_time_us: 6_000_000,
                    presented_frames: 1,
                    playing: true,
                    seeking: false,
                    ended: false,
                    visible: true,
                    playback_rate_milli: 1000,
                },
                callback_tick_us: 1_000_000,
                presentation_time_us: Some(1_000_000),
                expected_display_time_us: Some(1_000_000),
            },
            bracket: BrowserClockBracket {
                native_issue_ns: 1_000_000_000,
                native_receive_ns: 1_000_100_000,
                browser_sample_us: 1_000_000,
            },
            project_phase: ExactSeconds::new(1, 1).unwrap(),
            expires_at_boottime_ns: 2_000_000_000,
        };
        let reference_boottime_ns = 1_010_000_000;
        let alignment = subject
            .timing
            .phase_request(
                &observation,
                reference_boottime_ns,
                500_000,
                1000,
                1_100_000_000,
            )
            .unwrap()
            .calculate()
            .unwrap();
        HandyBrowserStartDecision {
            qualification: HandyBrowserStartQualificationBinding {
                dossier_id: manifest().identity().unwrap(),
                revision: 2,
            },
            subject,
            timing_binding,
            observation,
            reference_boottime_ns,
            alignment,
        }
    }
    #[test]
    fn evidence_identity_roles_and_environment_correspondence_are_exact() {
        let m = manifest();
        m.validate().unwrap();
        let original = m.identity().unwrap();
        for case in 0..6 {
            let mut bad = m.clone();
            match case {
                0 => bad.evidence.swap(0, 1),
                1 => {
                    bad.evidence.pop();
                }
                2 => bad.evidence[0].identity = artifact('f', 16),
                3 => bad.subject.schema_version = 2,
                4 => bad.subject.selection.placement.spec.rate_numerator = 2,
                _ => bad.subject.browser_environment.byte_len = 0,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        let mut changed = m.clone();
        changed.subject.timing.display_late_ns += 1;
        assert_ne!(changed.identity().unwrap(), original);
        changed = m.clone();
        changed.subject.script_actor_session = SessionId::new("different-script-owner").unwrap();
        assert_ne!(changed.identity().unwrap(), original);
        let input = HandyBrowserStartQualificationInput {
            subject: m.subject,
            evidence: m
                .evidence
                .into_iter()
                .map(|e| HandyBrowserStartQualificationInputEvidence {
                    kind: e.kind,
                    path: test_host_path("evidence.bin").into(),
                    sha256: e.identity.sha256,
                    byte_len: e.identity.byte_len,
                })
                .collect(),
        };
        input.validate().unwrap();
        let mut bad = input.clone();
        bad.evidence[0].sha256 = "f".repeat(64);
        assert!(bad.validate().is_err());
        bad = input;
        bad.evidence[1].path = "relative".into();
        assert!(bad.validate().is_err());
    }
    #[test]
    fn decisions_are_explicit_and_synthetic_acceptance_refuses() {
        let m = manifest();
        let mut r = HandyBrowserStartQualificationReport {
            dossier_id: m.identity().unwrap(),
            revision: 1,
            state: HandyQualificationState::Pending,
            manifest: m,
            decision: None,
        };
        r.validate().unwrap();
        r.revision = 2;
        r.state = HandyQualificationState::Accepted;
        r.decision = Some(HandyBrowserStartQualificationDecision {
            revision: 2,
            state: r.state,
            request_id: RequestId::new("accept").unwrap(),
            actor_session: SessionId::new("actor").unwrap(),
            operator: Some("authored protocol test operator".into()),
            note: String::new(),
            recorded_at_unix_ms: 1,
        });
        assert!(r.validate().is_err());
        // Pure structural fixture, never imported or accepted by a live Engine.
        r.manifest.subject.origin = HandyEvidenceOrigin::Physical;
        r.dossier_id = r.manifest.identity().unwrap();
        r.validate().unwrap();
        r.decision.as_mut().unwrap().operator = None;
        assert!(r.validate().is_err());
        r.state = HandyQualificationState::Withdrawn;
        let d = r.decision.as_mut().unwrap();
        d.state = r.state;
        d.note = "fixture withdrawn".into();
        r.validate().unwrap();
        r.decision.as_mut().unwrap().operator = Some("invented".into());
        assert!(r.validate().is_err());
    }
    #[test]
    fn alignment_decision_recomputes_components_source_and_original_expiry() {
        decision().validate().unwrap();
        for case in 0..9 {
            let mut d = decision();
            match case {
                0 => d.subject.project_id = ProjectId::new("other").unwrap(),
                1 => d.subject.clock.presentation_anchor = SourceTimestamp::new(4, 1).unwrap(),
                2 => d.alignment.total_error_ns -= 1,
                3 => d.alignment.start_time_ms += 1,
                4 => d.observation.frame.expected_display_time_us = None,
                5 => d.reference_boottime_ns = d.alignment.valid_until_boottime_ns,
                6 => d.alignment.script_quantization_error_ns = 500_001,
                7 => d.observation.frame.sample.playing = false,
                _ => d.observation.frame.sample.media_time_us = 15_000_000,
            }
            assert!(d.validate().is_err(), "case {case}");
        }
        let mut d = decision();
        d.subject.timing.source_correspondence_error_ns = 2_000_000_000;
        d.subject.timing.maximum_alignment_error_ns = 3_000_000_000;
        assert!(d
            .subject
            .timing
            .phase_request(
                &d.observation,
                d.reference_boottime_ns,
                0,
                0,
                d.alignment.valid_until_boottime_ns
            )
            .unwrap()
            .calculate()
            .is_err());
    }
    #[test]
    fn commands_check_consent_revision_and_keep_owner_proof_out_of_debug() {
        let d = decision();
        let q = d.qualification.clone();
        let id = q.dossier_id.clone();
        let timing = d.timing_binding.binding_id.clone();
        let play = RequestId::new("original-play").unwrap();
        let proof = "native-owner-secret".to_string();
        let commands = [
            Command::ImportHandyBrowserStartQualification {
                path: test_host_path("manifest.json").into(),
                manifest_sha256: "a".repeat(64),
                owner_proof: proof.clone(),
            },
            Command::HandyBrowserStartQualificationStatus {
                dossier_id: id.clone(),
                owner_proof: proof.clone(),
            },
            Command::AcceptHandyBrowserStartQualification {
                dossier_id: id.clone(),
                expected_record_revision: 1,
                operator: "fixture operator".into(),
                acknowledge_measured_evidence: true,
                owner_proof: proof.clone(),
            },
            Command::WithdrawHandyBrowserStartQualification {
                dossier_id: id,
                expected_record_revision: 2,
                reason: "fixture reason".into(),
                owner_proof: proof.clone(),
            },
            Command::StartHandyPlaybackFromBrowser {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                clock_request_id: RequestId::new("clock").unwrap(),
                browser_timing_binding_id: timing.clone(),
                qualification: q,
                acknowledge_current_environment: true,
                allow_play: true,
                owner_proof: proof.clone(),
            },
            Command::RenewHandyBrowserPlayback {
                browser_timing_binding_id: timing.clone(),
                play_request_id: play.clone(),
                play_record_id: None,
                owner_proof: proof.clone(),
            },
            Command::ReleaseHandyBrowserPlayback {
                browser_timing_binding_id: timing.clone(),
                play_request_id: play.clone(),
                play_record_id: None,
                owner_proof: proof.clone(),
            },
            Command::HandyBrowserPlaybackStatus {
                browser_timing_binding_id: timing.clone(),
                play_request_id: play.clone(),
                play_record_id: None,
                owner_proof: proof.clone(),
            },
            Command::RefreshHandyBrowserPlaybackTiming {
                browser_timing_binding_id: timing,
                play_request_id: play,
                play_record_id: None,
                allow_online_inspection: true,
                owner_proof: proof.clone(),
            },
        ];
        for (i, command) in commands.into_iter().enumerate() {
            let needs_revision = [0, 2, 4].contains(&i);
            assert!(command.requires_project());
            assert_eq!(command.requires_revision(), needs_revision);
            assert_eq!(command.is_mutating(), i != 1 && i != 7);
            assert!(!format!("{command:?}").contains(&proof));
            let request = Request::new(RequestId::new("operation").unwrap(), command).in_project(
                d.subject.project_id.clone(),
                needs_revision.then_some(d.subject.revision),
            );
            request.validate().unwrap();
            let mut bad = request.clone();
            bad.project = None;
            assert!(bad.validate().is_err());
            bad = request.clone();
            bad.expected_revision = if needs_revision {
                None
            } else {
                Some(d.subject.revision)
            };
            assert!(bad.validate().is_err());
            bad = request;
            match &mut bad.command {
                Command::AcceptHandyBrowserStartQualification {
                    acknowledge_measured_evidence,
                    ..
                } => *acknowledge_measured_evidence = false,
                Command::StartHandyPlaybackFromBrowser { allow_play, .. } => *allow_play = false,
                Command::RefreshHandyBrowserPlaybackTiming {
                    allow_online_inspection,
                    ..
                } => *allow_online_inspection = false,
                _ => continue,
            }
            assert_eq!(bad.validate().unwrap_err().code, ErrorCode::Forbidden);
        }
    }
    #[test]
    fn manual_history_omits_none_and_keeps_legacy_bytes() {
        #[derive(Serialize)]
        struct Legacy<'a> {
            record_id: &'a ArtifactId,
            original_controller: &'a HandyControllerLease,
            qualification: &'a HandyQualificationBinding,
            setup_record_id: &'a ArtifactId,
            clock_request_id: &'a RequestId,
            start_time_ms: u64,
            state: HandyPlaybackOperationState,
        }
        let controller: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        let h = HandyPlaybackHistory {
            record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            original_controller: controller.lease,
            qualification: subject().device_qualification,
            setup_record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            clock_request_id: RequestId::new("clock").unwrap(),
            start_time_ms: 0,
            browser_start: None,
            state: HandyPlaybackOperationState::Prepared,
        };
        let legacy = Legacy {
            record_id: &h.record_id,
            original_controller: &h.original_controller,
            qualification: &h.qualification,
            setup_record_id: &h.setup_record_id,
            clock_request_id: &h.clock_request_id,
            start_time_ms: h.start_time_ms,
            state: h.state,
        };
        let bytes = serde_json::to_vec(&legacy).unwrap();
        assert_eq!(serde_json::to_vec(&h).unwrap(), bytes);
        let restored: HandyPlaybackHistory = serde_json::from_slice(&bytes).unwrap();
        assert!(restored.browser_start.is_none());
        assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    }
    #[test]
    fn owner_report_refuses_partial_decision_evidence_even_in_history() {
        let mut r = HandyBrowserPlaybackReport {
            project_id: subject().project_id,
            browser_timing_binding_id: ArtifactId::new("binding").unwrap(),
            play_request_id: RequestId::new("play").unwrap(),
            play_record_id: None,
            controller: None,
            current: false,
            physical_stop_pending: false,
            browser_observation_id: None,
            alignment: None,
        };
        r.validate().unwrap();
        r.current = true;
        assert!(r.validate().is_err());
        r.current = false;
        r.alignment = Some(decision().alignment);
        assert!(r.validate().is_err());
        r.browser_observation_id = Some(ArtifactId::new("observation").unwrap());
        assert!(r.validate().is_err());
        r.play_record_id = Some(ArtifactId::new("a".repeat(64)).unwrap());
        r.validate().unwrap();
    }
    #[test]
    fn current_owner_requires_valid_controller_and_stop_obligation_without_ack() {
        let controller: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        let mut lease = controller.lease;
        lease.project_id = subject().project_id;
        let mut r = HandyBrowserPlaybackReport {
            project_id: lease.project_id.clone(),
            browser_timing_binding_id: ArtifactId::new("binding").unwrap(),
            play_request_id: RequestId::new("play").unwrap(),
            play_record_id: Some(ArtifactId::new("a".repeat(64)).unwrap()),
            controller: Some(lease),
            current: true,
            physical_stop_pending: true,
            browser_observation_id: Some(ArtifactId::new("observation").unwrap()),
            alignment: Some(decision().alignment),
        };
        // Active controller plus a native owner stop obligation is valid before
        // transport acknowledgment. The DTO grants no execution authority.
        r.validate().unwrap();
        let mut bad = r.clone();
        bad.physical_stop_pending = false;
        assert!(bad.validate().is_err());
        bad.current = false;
        bad.validate().unwrap();
        bad = r.clone();
        let c = bad.controller.as_mut().unwrap();
        c.state = HandyControllerState::Retired;
        c.armed = false;
        c.validate().unwrap();
        assert!(bad.validate().is_err());
        bad.current = false;
        bad.validate().unwrap();
        bad = r.clone();
        bad.controller.as_mut().unwrap().armed = false;
        assert!(bad.validate().is_err());
        let c = r.controller.as_mut().unwrap();
        c.state = HandyControllerState::Playing;
        c.physical_stop_pending = true;
        r.validate().unwrap();
    }
    #[test]
    fn queued_owner_has_exact_two_state_evidence_and_active_controller_before_record() {
        let controller: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        let mut lease = controller.lease;
        lease.project_id = subject().project_id;
        let base = HandyBrowserPlaybackReport {
            project_id: lease.project_id.clone(),
            browser_timing_binding_id: ArtifactId::new("binding").unwrap(),
            play_request_id: RequestId::new("play").unwrap(),
            play_record_id: None,
            controller: Some(lease),
            current: true,
            physical_stop_pending: true,
            browser_observation_id: None,
            alignment: None,
        };
        base.validate().unwrap();
        for mask in 0..8 {
            let mut r = base.clone();
            if mask & 1 != 0 {
                r.play_record_id = Some(ArtifactId::new("a".repeat(64)).unwrap());
            }
            if mask & 2 != 0 {
                r.alignment = Some(decision().alignment);
            }
            if mask & 4 != 0 {
                r.browser_observation_id = Some(ArtifactId::new("observation").unwrap());
            }
            assert_eq!(
                r.validate().is_ok(),
                mask == 0 || mask == 7,
                "Active mask {mask}"
            );
            r.current = false;
            assert_eq!(
                r.validate().is_ok(),
                mask == 0 || mask == 7,
                "historical mask {mask}"
            );
            r.current = true;
            let c = r.controller.as_mut().unwrap();
            c.state = HandyControllerState::Playing;
            c.physical_stop_pending = true;
            assert_eq!(r.validate().is_ok(), mask == 7, "Playing mask {mask}");
        }
        let mut missing_stop = base.clone();
        missing_stop.physical_stop_pending = false;
        assert!(missing_stop.validate().is_err());
        let mut tombstone = base;
        tombstone.current = false;
        tombstone.controller = None;
        tombstone.physical_stop_pending = false;
        tombstone.validate().unwrap();
    }
}
