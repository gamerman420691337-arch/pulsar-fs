//! Explicit source/presentation evidence for HSP. These records cannot restore
//! native acceptance or certify useful model output, acquisition, or actuation.
use crate::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};
pub const HANDY_HSP_SOURCE_MANIFEST_LIMIT: u64 = 128 * 1024;
pub const HANDY_HSP_SOURCE_EVIDENCE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspSourceRecipe {
    RvfcRgb64x64NearestStretchV1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspSourceAgeObservation {
    HttpReceiptMonotonic,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceSubject {
    pub schema_version: u8,
    pub origin: HandyEvidenceOrigin,
    pub context: HandyQualificationContext,
    pub device_qualification: HandyQualificationBinding,
    pub browser_environment: ArtifactIdentity,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub selection: BrowserMediaSelection,
    pub clock: DeclaredSourceClock,
    pub inference_profile: DeclaredInferenceProfile,
    /// Exact captured CPU image identity; absence declares the original host mode.
    /// This declaration alone does not authorize capture, inference or playback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_runtime_image: Option<ArtifactIdentity>,
    pub source_time_basis: LiveSourceTimeBasis,
    pub recipe: HandyHspSourceRecipe,
    pub age_basis: LiveCaptureAgeBasis,
    pub age_observation: HandyHspSourceAgeObservation,
    pub lookahead: BrowserAnalysisLookaheadPolicy,
    pub maximum_analysis_age_ns: u64,
    /// Accepted evidence bound, not a producer timestamp or receipt-as-exposure
    /// assertion. Native age accounting must include this whole interval.
    pub maximum_capture_to_engine_receipt_ns: u64,
    pub timing: HandyBrowserStartTiming,
    /// Explicit finite-drain selection; absence retains conservative input-end Stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_drain_policy_sha256: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspSourceEvidenceKind {
    /// Actual browser configuration and independent analysis/visible players.
    EnvironmentConfiguration,
    /// Measured browser clock and visible display applicability/uncertainty.
    ClockAndDisplay,
    /// Actual analysis-to-visible lead across progression, gaps and stalls.
    MediaProgression,
    /// Checked correspondence of source bytes, rVFC RGB/PTS and visible media;
    /// includes capture-to-Engine receipt bounds without claiming exposure time.
    SourceCorrespondence,
    /// Exact original input closure, settled final delivery and retained clocks.
    FiniteInputClosure,
}
impl HandyHspSourceEvidenceKind {
    pub const ALL: [Self; 4] = [
        Self::EnvironmentConfiguration,
        Self::ClockAndDisplay,
        Self::MediaProgression,
        Self::SourceCorrespondence,
    ];
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationInputEvidence {
    pub kind: HandyHspSourceEvidenceKind,
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationInput {
    pub subject: HandyHspSourceSubject,
    pub evidence: Vec<HandyHspSourceQualificationInputEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationEvidence {
    pub kind: HandyHspSourceEvidenceKind,
    pub identity: ArtifactIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationManifest {
    pub subject: HandyHspSourceSubject,
    pub evidence: Vec<HandyHspSourceQualificationEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationBinding {
    pub dossier_id: ArtifactId,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationDecision {
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
pub struct HandyHspSourceQualificationReport {
    pub dossier_id: ArtifactId,
    pub revision: u64,
    pub state: HandyQualificationState,
    pub manifest: HandyHspSourceQualificationManifest,
    pub decision: Option<HandyHspSourceQualificationDecision>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspSourceQualificationReceipt {
    pub input_sha256: Option<String>,
    pub request_id: RequestId,
    pub report: HandyHspSourceQualificationReport,
}

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(format!("HSP source: {message}"))
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
impl HandyHspSourceSubject {
    pub fn required_evidence_kinds(&self) -> Vec<HandyHspSourceEvidenceKind> {
        let mut kinds = HandyHspSourceEvidenceKind::ALL.to_vec();
        if self.finite_drain_policy_sha256.is_some() {
            kinds.push(HandyHspSourceEvidenceKind::FiniteInputClosure);
        }
        kinds
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.context.validate()?;
        self.device_qualification.validate()?;
        checked_artifact(&self.browser_environment, HANDY_HSP_SOURCE_EVIDENCE_LIMIT)?;
        self.selection.validate()?;
        self.clock.validate().map_err(core_error)?;
        self.inference_profile.validate()?;
        if let Some(image) = &self.cpu_runtime_image {
            image.validate()?;
            if image.byte_len == 0 || self.inference_profile.provider != InferenceProvider::Cpu {
                return Err(invalid(
                    "captured runtime image requires a nonempty CPU identity",
                ));
            }
        }
        self.timing.validate()?;
        self.lookahead.validate()?;
        if let Some(hash) = &self.finite_drain_policy_sha256 {
            validate_sha256(hash)?;
        }
        let (_, end) = self
            .clock
            .presentation_trim_micros(&self.selection.placement.spec)
            .map_err(core_error)?;
        let p = &self.inference_profile.preprocessing;
        if self.schema_version != 1
            || self.context.transport.as_str() != HANDY_HSP_QUALIFICATION_TRANSPORT
            || self.selection.placement.created_revision > self.revision
            || i64::from(self.selection.placement.spec.rate_numerator)
                != i64::from(self.selection.placement.spec.rate_denominator)
            || end > BROWSER_MEDIA_MAX_EXACT_INTEGER
            || self.source_time_basis != LiveSourceTimeBasis::SourceMediaPresentation
            || self.age_basis != LiveCaptureAgeBasis::ProducerDeclared
            || self.maximum_analysis_age_ns == 0
            || self.maximum_analysis_age_ns > 10_000_000_000
            || self.maximum_capture_to_engine_receipt_ns == 0
            || self.maximum_capture_to_engine_receipt_ns > 10_000_000_000
            || self.inference_profile.provider != InferenceProvider::Cpu
            || self.inference_profile.schema_version != 1
            || self.inference_profile.cuda_resources.is_some()
            || p.input.width != 64
            || p.input.height != 64
            || p.input.channels != 6
            || p.input.class_count != 1
            || p.input.layout != TensorLayout::Nchw
            || p.input.decoder != DetectorDecoder::TemporalScriptPosition
            || !matches!(p.resize, InferenceResize::NearestStretch)
            || !matches!(p.color, InferenceColor::RgbFramePair)
            || p.scale != vec![1.0f32 / 255.0; 6]
            || p.bias != vec![0.0; 6]
            || p.padding_byte != 0
        {
            return Err(invalid(
                "unsupported HSP source, profile, recipe or age declaration",
            ));
        }
        Ok(())
    }
    pub fn inference_profile_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&self.inference_profile)
                    .map_err(|_| invalid("profile encoding failed"))?
            )
        ))
    }
    pub fn identity_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(self).map_err(|_| invalid("subject encoding failed"))?
            )
        ))
    }
    pub fn exact_eq(&self, other: &Self) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && serde_json::to_vec(self)
                .ok()
                .zip(serde_json::to_vec(other).ok())
                .is_some_and(|(a, b)| a == b)
    }
}
impl HandyHspSourceQualificationInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        let required = self.subject.required_evidence_kinds();
        if self.evidence.len() != required.len() {
            return Err(invalid("all declared source evidence roles required"));
        }
        let mut kinds = BTreeSet::new();
        for item in &self.evidence {
            input_path(&item.path)?;
            validate_sha256(&item.sha256)?;
            if !required.contains(&item.kind)
                || !kinds.insert(item.kind)
                || item.byte_len == 0
                || item.byte_len > HANDY_HSP_SOURCE_EVIDENCE_LIMIT
                || (item.kind == HandyHspSourceEvidenceKind::EnvironmentConfiguration
                    && (item.sha256 != self.subject.browser_environment.sha256
                        || item.byte_len != self.subject.browser_environment.byte_len))
            {
                return Err(invalid("evidence input differs or exceeds limits"));
            }
        }
        Ok(())
    }
}
impl HandyHspSourceQualificationManifest {
    pub fn identity(&self) -> Result<ArtifactId, ProtocolError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| invalid("manifest encoding failed"))?;
        if bytes.len() as u64 > HANDY_HSP_SOURCE_MANIFEST_LIMIT {
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
            .eq(self.subject.required_evidence_kinds())
        {
            return Err(invalid("evidence roles must be complete and canonical"));
        }
        for item in &self.evidence {
            checked_artifact(&item.identity, HANDY_HSP_SOURCE_EVIDENCE_LIMIT)?;
            if item.kind == HandyHspSourceEvidenceKind::EnvironmentConfiguration
                && item.identity != self.subject.browser_environment
            {
                return Err(invalid("environment evidence differs from subject"));
            }
        }
        Ok(())
    }
}
impl HandyHspSourceQualificationBinding {
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
impl HandyHspSourceQualificationReport {
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
impl HandyHspSourceQualificationReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(hash) = &self.input_sha256 {
            validate_sha256(hash)?;
        }
        self.report.validate()
    }
}

pub(crate) fn validate_request(r: &Request) -> Result<(), ProtocolError> {
    let (proof, requires_revision) = match &r.command {
        Command::ImportHandyHspSourceQualification {
            path,
            manifest_sha256,
            owner_proof,
        } => {
            input_path(path)?;
            validate_sha256(manifest_sha256)?;
            (owner_proof, true)
        }
        Command::HandyHspSourceQualificationStatus {
            dossier_id,
            owner_proof,
        } => {
            validate_sha256(dossier_id.as_str())?;
            (owner_proof, false)
        }
        Command::AcceptHandyHspSourceQualification {
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
                    "measured HSP source evidence requires explicit operator acknowledgment",
                ));
            }
            (owner_proof, true)
        }
        Command::WithdrawHandyHspSourceQualification {
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
        _ => return Ok(()),
    };
    bounded_text(proof, 64, "Handy owner capability")?;
    if r.project.is_none() || r.expected_revision.is_some() != requires_revision {
        return Err(invalid(
            "incorrect source qualification project/revision semantics",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn authored_hsp_source_subject_for_test() -> HandyHspSourceSubject {
    let artifact = |c: char, byte_len| {
        let sha256 = c.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len,
        }
    };
    let media = crate::browser_media::test_binding();
    HandyHspSourceSubject {
        schema_version: 1,
        origin: HandyEvidenceOrigin::Synthetic,
        context: HandyQualificationContext {
            schema_version: 1,
            control_image: artifact('a', 1),
            platform: "linux/authored-fixture".into(),
            transport: ArtifactId::new(HANDY_HSP_QUALIFICATION_TRANSPORT).unwrap(),
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
        inference_profile: DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact('d', 32),
            runtime: artifact('e', 64),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0f32 / 255.0; 6],
                bias: vec![0.; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        },
        cpu_runtime_image: None,
        source_time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        recipe: HandyHspSourceRecipe::RvfcRgb64x64NearestStretchV1,
        age_basis: LiveCaptureAgeBasis::ProducerDeclared,
        age_observation: HandyHspSourceAgeObservation::HttpReceiptMonotonic,
        lookahead: BrowserAnalysisLookaheadPolicy {
            target_lead_us: 500_000,
            minimum_lead_us: 100_000,
            maximum_lead_us: 1_000_000,
        },
        maximum_analysis_age_ns: 500_000_000,
        maximum_capture_to_engine_receipt_ns: 100_000_000,
        finite_drain_policy_sha256: None,
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_subject_is_exact_and_does_not_relabel_http_receipt_as_capture() {
        let good = authored_hsp_source_subject_for_test();
        good.validate().unwrap();
        let wire = serde_json::to_vec(&good).unwrap();
        let restored: HandyHspSourceSubject = serde_json::from_slice(&wire).unwrap();
        assert!(good.exact_eq(&restored));
        for case in 0..11 {
            let mut changed = good.clone();
            match case {
                0 => {
                    changed.context.transport =
                        ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap()
                }
                1 => changed.source_time_basis = LiveSourceTimeBasis::ProducerDeclared,
                2 => changed.age_basis = LiveCaptureAgeBasis::DecodedStdoutLocalMonotonicEqualRate,
                3 => changed.inference_profile.provider = InferenceProvider::Cuda,
                4 => changed.inference_profile.preprocessing.resize = InferenceResize::Stretch,
                5 => changed.inference_profile.preprocessing.input.width = 32,
                6 => changed.inference_profile.preprocessing.bias[0] = 1.,
                7 => changed.maximum_capture_to_engine_receipt_ns = 0,
                8 => changed.maximum_analysis_age_ns = 10_000_000_001,
                9 => changed.lookahead.minimum_lead_us = 0,
                _ => changed.lookahead.maximum_lead_us = 30_000_001,
            }
            assert!(changed.validate().is_err(), "case {case}");
            assert!(!changed.exact_eq(&good));
        }
        let mut changed = good.clone();
        changed.inference_profile.cpu_threads = 2;
        changed.validate().unwrap();
        assert_ne!(
            changed.identity_sha256().unwrap(),
            good.identity_sha256().unwrap()
        );
        let mut changed = good.clone();
        changed.maximum_capture_to_engine_receipt_ns += 1;
        changed.validate().unwrap();
        assert_ne!(
            changed.identity_sha256().unwrap(),
            good.identity_sha256().unwrap()
        );
    }
    fn authored_cpu_image(c: char) -> ArtifactIdentity {
        let sha256 = c.to_string().repeat(64);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("{CPU_RUNTIME_IMAGE_ID_PREFIX}{sha256}")).unwrap(),
            sha256,
            byte_len: 128,
        }
    }
    #[test]
    fn source_subject_binds_captured_image_without_changing_profile_or_legacy_mode() {
        let host = authored_hsp_source_subject_for_test();
        let mut captured = host.clone();
        captured.cpu_runtime_image = Some(authored_cpu_image('f'));
        captured.validate().unwrap();
        assert_eq!(
            captured.inference_profile_sha256().unwrap(),
            host.inference_profile_sha256().unwrap()
        );
        assert!(!captured.exact_eq(&host));
        assert_ne!(
            captured.identity_sha256().unwrap(),
            host.identity_sha256().unwrap()
        );
        let mut replacement = captured.clone();
        replacement.cpu_runtime_image = Some(authored_cpu_image('e'));
        assert!(!replacement.exact_eq(&captured));
        assert_ne!(
            replacement.identity_sha256().unwrap(),
            captured.identity_sha256().unwrap()
        );
        let restored: HandyHspSourceSubject =
            serde_json::from_slice(&serde_json::to_vec(&captured).unwrap()).unwrap();
        assert!(restored.exact_eq(&captured));
        assert_eq!(restored.cpu_runtime_image, captured.cpu_runtime_image);
        assert_eq!(captured.origin, HandyEvidenceOrigin::Synthetic);
        let value = serde_json::to_value(&host).unwrap();
        assert!(value.get("cpu_runtime_image").is_none());
    }
    #[test]
    fn captured_source_subject_rejects_empty_invalid_and_non_cpu_image_declarations() {
        for case in 0..4 {
            let mut subject = authored_hsp_source_subject_for_test();
            subject.cpu_runtime_image = Some(authored_cpu_image('f'));
            match case {
                0 => subject.cpu_runtime_image.as_mut().unwrap().byte_len = 0,
                1 => subject.cpu_runtime_image.as_mut().unwrap().sha256 = "not-a-digest".into(),
                2 => subject.inference_profile.provider = InferenceProvider::Cuda,
                _ => subject.inference_profile.provider = InferenceProvider::Rocm,
            }
            assert!(subject.validate().is_err(), "case {case}");
            assert!(subject.identity_sha256().is_err(), "case {case}");
        }
    }
    fn manifest() -> HandyHspSourceQualificationManifest {
        let subject = authored_hsp_source_subject_for_test();
        let evidence = HandyHspSourceEvidenceKind::ALL
            .into_iter()
            .map(|kind| HandyHspSourceQualificationEvidence {
                kind,
                identity: subject.browser_environment.clone(),
            })
            .collect();
        HandyHspSourceQualificationManifest { subject, evidence }
    }
    #[test]
    fn absent_finite_policy_keeps_legacy_source_bytes_and_dossier_digest() {
        let manifest = manifest();
        let s = &manifest.subject;
        let mut serializer = serde_json::Serializer::new(Vec::new());
        let mut fields =
            serde::Serializer::serialize_struct(&mut serializer, "HandyHspSourceSubject", 18)
                .unwrap();
        macro_rules! legacy_field {
            ($name:ident) => {
                serde::ser::SerializeStruct::serialize_field(
                    &mut fields,
                    stringify!($name),
                    &s.$name,
                )
                .unwrap()
            };
        }
        legacy_field!(schema_version);
        legacy_field!(origin);
        legacy_field!(context);
        legacy_field!(device_qualification);
        legacy_field!(browser_environment);
        legacy_field!(project_id);
        legacy_field!(revision);
        legacy_field!(selection);
        legacy_field!(clock);
        legacy_field!(inference_profile);
        legacy_field!(source_time_basis);
        legacy_field!(recipe);
        legacy_field!(age_basis);
        legacy_field!(age_observation);
        legacy_field!(lookahead);
        legacy_field!(maximum_analysis_age_ns);
        legacy_field!(maximum_capture_to_engine_receipt_ns);
        legacy_field!(timing);
        serde::ser::SerializeStruct::end(fields).unwrap();
        let old_bytes = serializer.into_inner();
        assert_eq!(serde_json::to_vec(s).unwrap(), old_bytes);
        assert_eq!(
            s.identity_sha256().unwrap(),
            format!("{:x}", Sha256::digest(&old_bytes))
        );
        let old: HandyHspSourceSubject = serde_json::from_slice(&old_bytes).unwrap();
        assert!(old.finite_drain_policy_sha256.is_none());
        assert!(old.cpu_runtime_image.is_none());
        assert!(old.exact_eq(s));
        assert_eq!(s.required_evidence_kinds(), HandyHspSourceEvidenceKind::ALL);
        let old_manifest = format!(
            "{{\"subject\":{},\"evidence\":{}}}",
            String::from_utf8(old_bytes).unwrap(),
            serde_json::to_string(&manifest.evidence).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&manifest).unwrap(),
            old_manifest.as_bytes()
        );
        assert_eq!(
            manifest.identity().unwrap().as_str(),
            format!("{:x}", Sha256::digest(old_manifest.as_bytes()))
        );
    }
    #[test]
    fn finite_source_policy_requires_exact_additional_evidence_role() {
        let old = manifest();
        let mut finite = old.clone();
        finite.subject.finite_drain_policy_sha256 = Some("f".repeat(64));
        assert!(finite.validate().is_err());
        finite.evidence.push(HandyHspSourceQualificationEvidence {
            kind: HandyHspSourceEvidenceKind::FiniteInputClosure,
            identity: finite.subject.browser_environment.clone(),
        });
        finite.validate().unwrap();
        assert_ne!(finite.identity().unwrap(), old.identity().unwrap());
        assert!(!finite.subject.exact_eq(&old.subject));
        let mut changed = finite.clone();
        changed.subject.finite_drain_policy_sha256 = Some("e".repeat(64));
        assert_ne!(changed.identity().unwrap(), finite.identity().unwrap());
        changed.subject.finite_drain_policy_sha256 = Some("invalid".into());
        assert!(changed.validate().is_err());
        changed.subject.finite_drain_policy_sha256 = None;
        assert!(changed.validate().is_err());
        changed.evidence.remove(1);
        assert!(changed.validate().is_err());
        let input = HandyHspSourceQualificationInput {
            subject: changed.subject,
            evidence: changed
                .evidence
                .into_iter()
                .map(|e| HandyHspSourceQualificationInputEvidence {
                    kind: e.kind,
                    path: PathBuf::from("/authored-fixture.bin"),
                    sha256: e.identity.sha256,
                    byte_len: e.identity.byte_len,
                })
                .collect(),
        };
        assert!(input.validate().is_err());
    }
    #[test]
    fn hsp_source_evidence_requires_four_exact_roles_and_explicit_review() {
        let good = manifest();
        good.validate().unwrap();
        let mut missing = good.clone();
        missing.evidence.pop();
        assert!(missing.validate().is_err());
        let mut duplicate = good.clone();
        duplicate.evidence[1].kind = duplicate.evidence[0].kind;
        assert!(duplicate.validate().is_err());
        let mut altered = good.clone();
        altered.subject.browser_environment.byte_len += 1;
        assert!(altered.validate().is_err());
        let report = HandyHspSourceQualificationReport {
            dossier_id: good.identity().unwrap(),
            revision: 1,
            state: HandyQualificationState::Pending,
            manifest: good,
            decision: None,
        };
        report.validate().unwrap();
        let mut accepted = report;
        accepted.revision = 2;
        accepted.state = HandyQualificationState::Accepted;
        accepted.decision = Some(HandyHspSourceQualificationDecision {
            revision: 2,
            state: HandyQualificationState::Accepted,
            request_id: RequestId::new("authored").unwrap(),
            actor_session: SessionId::new("actor").unwrap(),
            operator: Some("authored operator".into()),
            note: String::new(),
            recorded_at_unix_ms: 1,
        });
        assert!(accepted.validate().is_err());
    }
    #[test]
    fn source_acceptance_consent_and_revision_are_not_implicit() {
        let mut request = Request::new(
            RequestId::new("review").unwrap(),
            Command::AcceptHandyHspSourceQualification {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                expected_record_revision: 1,
                operator: "operator".into(),
                acknowledge_measured_evidence: false,
                owner_proof: "b".repeat(64),
            },
        );
        request.project = Some(ProjectId::new("project").unwrap());
        request.expected_revision = Some(RevisionId::new(1));
        assert_eq!(
            validate_request(&request).unwrap_err().code,
            ErrorCode::Forbidden
        );
        if let Command::AcceptHandyHspSourceQualification {
            acknowledge_measured_evidence,
            ..
        } = &mut request.command
        {
            *acknowledge_measured_evidence = true;
        }
        validate_request(&request).unwrap();
        request.expected_revision = None;
        assert!(validate_request(&request).is_err());
    }
}
