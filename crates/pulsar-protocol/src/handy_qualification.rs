//! Exact local evidence and operator decisions; these DTOs are not playback grants.
use crate::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};
#[path = "handy_recovery_contract.rs"]
mod recovery;
pub use recovery::*;
#[path = "handy_hsp_qualification.rs"]
mod hsp;
pub use hsp::*;
pub const HANDY_QUALIFICATION_MANIFEST_LIMIT: u64 = 128 * 1024;
pub const HANDY_QUALIFICATION_EVIDENCE_LIMIT: u64 = 4 * 1024 * 1024;
pub const HANDY_QUALIFICATION_TRANSPORT: &str =
    "handy-rest-v3-hssp.9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationContext {
    pub schema_version: u8,
    pub control_image: ArtifactIdentity,
    pub platform: String,
    pub transport: ArtifactId,
}
impl HandyQualificationContext {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.control_image.validate()?;
        if self.schema_version != 1
            || self.control_image.byte_len == 0
            || self.control_image.artifact_id.as_str() != self.control_image.sha256
            || self.platform.is_empty()
            || self.platform.len() > 256
            || self.platform.chars().any(char::is_control)
            || HandyQualificationTransport::from_transport_id(self.transport.as_str()).is_err()
        {
            return Err(ProtocolError::invalid("invalid qualification context"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyEvidenceOrigin {
    Synthetic,
    Physical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyEvidenceKind {
    TravelAndDynamics,
    NativeCurveAndQuantization,
    TimingAndSynchronization,
    Stop,
    Disconnect,
    ControllerLoss,
    EngineDeath,
    EmergencyProcedure,
    /// Finite terminal behavior and Stop timing from the final native send gate.
    FiniteTailDrain,
}
impl HandyEvidenceKind {
    pub const ALL: [Self; 8] = [
        Self::TravelAndDynamics,
        Self::NativeCurveAndQuantization,
        Self::TimingAndSynchronization,
        Self::Stop,
        Self::Disconnect,
        Self::ControllerLoss,
        Self::EngineDeath,
        Self::EmergencyProcedure,
    ];
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHsspQualificationProfile {
    pub dynamics: DynamicsProfile,
    pub minimum_segment_ms: u64,
    pub maximum_step_percent: u8,
    pub minimum_position_percent: u8,
    pub maximum_position_percent: u8,
    pub maximum_command_age_ns: u64,
    pub maximum_clock_uncertainty_ns: u64,
    pub drift_ppm: u32,
    pub stopping_bound_ns: Option<u64>,
}
impl HandyHsspQualificationProfile {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.dynamics
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if self.dynamics.axes.len() != 1
            || self.dynamics.axes[0].axis != Axis::Stroke
            || self.dynamics.axes[0].unit != "mm"
            || !self.dynamics.coupled.is_empty()
            || self.minimum_segment_ms == 0
            || self.minimum_segment_ms > (1u64 << 53) - 1
            || self.maximum_step_percent == 0
            || self.maximum_step_percent > 100
            || self.minimum_position_percent >= self.maximum_position_percent
            || self.maximum_position_percent > 100
            || self.maximum_command_age_ns == 0
            || self.maximum_clock_uncertainty_ns == 0
            || self.stopping_bound_ns == Some(0)
        {
            return Err(ProtocolError::invalid(
                "invalid declared native HSSP qualification envelope",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationSubject {
    pub schema_version: u8,
    pub origin: HandyEvidenceOrigin,
    pub unit_label: String,
    pub device_id: DeviceId,
    pub registration_revision: u64,
    pub registration_binding_sha256: String,
    pub identity: HandyIdentity,
    pub context: HandyQualificationContext,
    pub profile: HandyHsspQualificationProfile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operative_settings: Option<HandyOperativeSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_clock: Option<HandyServerClockQualification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_clock: Option<HandyDeviceClockQualification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<HandyRecoveryQualification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuous_timing: Option<HandyContinuousTimingQualification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsp: Option<HandyHspQualificationProfile>,
}
impl HandyQualificationSubject {
    pub fn required_evidence_kinds(&self) -> Vec<HandyEvidenceKind> {
        let mut kinds = HandyEvidenceKind::ALL.to_vec();
        if self
            .hsp
            .as_ref()
            .is_some_and(|hsp| hsp.finite_drain.is_some())
        {
            kinds.push(HandyEvidenceKind::FiniteTailDrain);
        }
        kinds
    }
    pub fn transport(&self) -> Result<HandyQualificationTransport, ProtocolError> {
        HandyQualificationTransport::from_transport_id(self.context.transport.as_str())
    }
    /// Exact historical subject identity; this digest establishes no acceptance.
    pub fn sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("qualification subject encoding failed"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    /// HSP policy plus the common physical envelope and all reviewed operating
    /// contracts. A different context, unit or decision remains bound separately
    /// by the full subject and dossier identities.
    pub fn hsp_profile_sha256(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        if self.schema_version != 7 || self.transport()? != HandyQualificationTransport::Hsp {
            return Err(ProtocolError::invalid(
                "HSP profile requires a v7 HSP subject",
            ));
        }
        let bytes = serde_json::to_vec(&(
            &self.profile,
            &self.operative_settings,
            &self.server_clock,
            &self.device_clock,
            &self.recovery,
            &self.hsp,
        ))
        .map_err(|_| ProtocolError::invalid("HSP qualification profile encoding failed"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    /// Exact finite extension and unchanged existing stopping bound. A digest
    /// is a selection identity only; it cannot confer acceptance or permission.
    pub fn hsp_finite_drain_policy_sha256(&self) -> Result<Option<String>, ProtocolError> {
        self.validate()?;
        let Some(finite) = self.hsp.as_ref().and_then(|hsp| hsp.finite_drain.as_ref()) else {
            return Ok(None);
        };
        let bytes = serde_json::to_vec(&(
            "handy_hsp_finite_drain_policy_v1",
            finite,
            self.profile.stopping_bound_ns,
        ))
        .map_err(|_| ProtocolError::invalid("HSP finite drain policy encoding failed"))?;
        Ok(Some(format!("{:x}", Sha256::digest(bytes))))
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.identity.validate()?;
        self.context.validate()?;
        self.profile.validate()?;
        let transport = self.transport()?;
        if let Some(hsp) = &self.hsp {
            hsp.validate()?;
            if let Some(finite) = &hsp.finite_drain {
                finite.stop_budget_ns(self.profile.stopping_bound_ns)?;
            }
        }
        if let Some(settings) = &self.operative_settings {
            settings.validate()?;
        }
        validate_sha256(&self.registration_binding_sha256)?;
        if let Some(continuous) = &self.continuous_timing {
            continuous.validate()?;
            if continuous.timestamp_error_ns > self.profile.maximum_clock_uncertainty_ns {
                return Err(ProtocolError::invalid(
                    "continuous observation error exceeds the timing budget",
                ));
            }
        }
        if let Some(recovery) = &self.recovery {
            recovery.validate()?;
            if recovery.transport != transport {
                return Err(ProtocolError::invalid(
                    "recovery transport differs from qualification",
                ));
            }
            if self
                .profile
                .stopping_bound_ns
                .is_none_or(|bound| bound == 0 || bound > recovery.stopped_sample_separation_ns)
            {
                return Err(ProtocolError::invalid(
                    "recovery requires a qualified stopping bound within the sample separation",
                ));
            }
        }
        if let Some(clock) = &self.device_clock {
            clock.validate()?;
            let server = self.server_clock.as_ref().ok_or_else(|| {
                ProtocolError::invalid("device clock requires a server clock contract")
            })?;
            if server
                .timestamp_error_ns
                .checked_add(clock.device_error_ns)
                .is_none_or(|n| n > self.profile.maximum_clock_uncertainty_ns)
                || self
                    .profile
                    .drift_ppm
                    .checked_add(clock.drift_ppm)
                    .is_none()
            {
                return Err(ProtocolError::invalid(
                    "combined clock error or drift exceeds supported budget",
                ));
            }
        }
        if self.server_clock.as_ref().is_some_and(|clock| {
            clock.timestamp_error_ns > self.profile.maximum_clock_uncertainty_ns
        }) {
            return Err(ProtocolError::invalid(
                "server timestamp error exceeds clock budget",
            ));
        }
        if !matches!(
            (
                self.schema_version,
                self.operative_settings.is_some(),
                self.server_clock.is_some(),
                self.device_clock.is_some(),
                self.recovery.is_some(),
                self.continuous_timing.is_some(),
                self.hsp.is_some(),
                transport,
            ),
            (
                1,
                false,
                false,
                false,
                false,
                false,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                2,
                true,
                false,
                false,
                false,
                false,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                3,
                true,
                true,
                false,
                false,
                false,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                4,
                true,
                true,
                true,
                false,
                false,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                5,
                true,
                true,
                true,
                true,
                false,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                6,
                true,
                true,
                true,
                true,
                true,
                false,
                HandyQualificationTransport::Hssp
            ) | (
                7,
                true,
                true,
                true,
                true,
                false,
                true,
                HandyQualificationTransport::Hsp
            )
        ) || self.unit_label.trim().is_empty()
            || self.unit_label.len() > 128
            || self.unit_label.chars().any(char::is_control)
            || self.registration_revision == 0
            || self.registration_revision >= i64::MAX as u64
        {
            return Err(ProtocolError::invalid(
                "invalid Handy qualification subject",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationInputEvidence {
    pub kind: HandyEvidenceKind,
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationInput {
    pub subject: HandyQualificationSubject,
    pub evidence: Vec<HandyQualificationInputEvidence>,
}
impl HandyQualificationInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        let mut kinds = BTreeSet::new();
        let required = self.subject.required_evidence_kinds();
        if self.evidence.len() != required.len() {
            return Err(ProtocolError::invalid(
                "all declared physical evidence roles are required",
            ));
        }
        for item in &self.evidence {
            validate_sha256(&item.sha256)?;
            let bytes = item.path.as_os_str().as_encoded_bytes();
            if !required.contains(&item.kind)
                || !kinds.insert(item.kind)
                || item.byte_len == 0
                || item.byte_len > HANDY_QUALIFICATION_EVIDENCE_LIMIT
                || !item.path.is_absolute()
                || bytes.len() > 32768
                || bytes.contains(&0)
                || item
                    .path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ProtocolError::invalid(
                    "invalid qualification evidence input",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationEvidence {
    pub kind: HandyEvidenceKind,
    pub identity: ArtifactIdentity,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationManifest {
    pub subject: HandyQualificationSubject,
    pub evidence: Vec<HandyQualificationEvidence>,
}
impl HandyQualificationManifest {
    pub fn identity(&self) -> Result<ArtifactId, ProtocolError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("qualification manifest encoding failed"))?;
        ArtifactId::new(format!("{:x}", Sha256::digest(bytes)))
            .map_err(|e| ProtocolError::invalid(e.to_string()))
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.subject.validate()?;
        if !self
            .evidence
            .iter()
            .map(|e| e.kind)
            .eq(self.subject.required_evidence_kinds())
        {
            return Err(ProtocolError::invalid(
                "qualification evidence roles must be complete and canonical",
            ));
        }
        for item in &self.evidence {
            item.identity.validate()?;
            if item.identity.artifact_id.as_str() != item.identity.sha256
                || item.identity.byte_len == 0
                || item.identity.byte_len > HANDY_QUALIFICATION_EVIDENCE_LIMIT
            {
                return Err(ProtocolError::invalid(
                    "invalid retained qualification evidence identity",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyQualificationState {
    Pending,
    Accepted,
    Withdrawn,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationDecision {
    pub revision: u64,
    pub state: HandyQualificationState,
    pub request_id: RequestId,
    pub actor_session: SessionId,
    pub note: String,
    pub recorded_at_unix_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationReport {
    pub dossier_id: ArtifactId,
    pub revision: u64,
    pub state: HandyQualificationState,
    pub manifest: HandyQualificationManifest,
    pub decision: Option<HandyQualificationDecision>,
}
impl HandyQualificationReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.dossier_id.as_str())?;
        self.manifest.validate()?;
        if self.manifest.identity()? != self.dossier_id {
            return Err(ProtocolError::invalid(
                "qualification dossier digest differs",
            ));
        }
        if self.revision == 0
            || self.revision > i64::MAX as u64
            || (self.state == HandyQualificationState::Pending) != self.decision.is_none()
            || (self.state == HandyQualificationState::Accepted
                && self.manifest.subject.origin != HandyEvidenceOrigin::Physical)
        {
            return Err(ProtocolError::invalid(
                "inconsistent qualification review state",
            ));
        }
        if let Some(d) = &self.decision {
            validate_handy_qualification_note(&d.note)?;
            if d.recorded_at_unix_ms == 0
                || d.revision != self.revision
                || d.state != self.state
                || d.state == HandyQualificationState::Pending
            {
                return Err(ProtocolError::invalid(
                    "qualification decision has no timestamp",
                ));
            }
        }
        Ok(())
    }
}
pub fn validate_handy_qualification_note(note: &str) -> Result<(), ProtocolError> {
    if note.trim().is_empty() || note.len() > 512 || note.chars().any(char::is_control) {
        return Err(ProtocolError::invalid(
            "qualification decisions require a bounded explicit note",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationReceipt {
    pub input_sha256: Option<String>,
    pub request_id: RequestId,
    pub report: HandyQualificationReport,
}
impl HandyQualificationReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(hash) = &self.input_sha256 {
            validate_sha256(hash)?;
        }
        self.report.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy_subject(version: u8) -> HandyQualificationSubject {
        let mut s = subject();
        s.schema_version = version;
        s.profile.maximum_clock_uncertainty_ns = 10;
        s.profile.stopping_bound_ns = Some(100);
        if version >= 2 {
            s.operative_settings = Some(
                serde_json::from_str(include_str!(
                    "../tests/fixtures/handy-operative-settings.json"
                ))
                .unwrap(),
            );
        }
        if version >= 3 {
            s.server_clock = Some(HandyServerClockQualification {
                model: HandyServerClockModel::RequestBracketMonotonic,
                timestamp_error_ns: 1,
            });
        }
        if version >= 4 {
            s.device_clock = Some(HandyDeviceClockQualification {
                model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
                user_offset_ms: 0,
                device_error_ns: 1,
                maximum_sync_age_ns: 100,
                maximum_sync_round_trip_ms: 1,
                drift_ppm: 1,
            });
        }
        if version >= 5 {
            s.recovery = Some(HandyRecoveryQualification {
                transport: HandyQualificationTransport::Hssp,
                model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                remote_state: HandyRecoveryRemoteState::Stopped,
                prior_command_settle_ns: 100,
                stopped_sample_separation_ns: 100,
                maximum_sample_span_ns: 100,
            });
        }
        if version >= 6 {
            s.continuous_timing = Some(HandyContinuousTimingQualification {
                model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
                timestamp_error_ns: 1,
                maximum_observation_age_ns: 100,
                maximum_sample_span_ns: 50,
                drift_ppm: 1,
            });
        }
        s
    }
    fn hsp_subject() -> HandyQualificationSubject {
        let mut s = legacy_subject(5);
        s.schema_version = 7;
        s.context.transport = ArtifactId::new(HANDY_HSP_QUALIFICATION_TRANSPORT).unwrap();
        s.recovery.as_mut().unwrap().transport = HandyQualificationTransport::Hsp;
        s.hsp = Some(HandyHspQualificationProfile {
            policy: HandyHspPolicy {
                max_buffered_points: 100,
                device_capacity_points: 100,
                max_chunk_points: 50,
                min_lead_ms: 100,
                max_future_ms: 1000,
                max_sample_age_ns: 1_000_000,
                max_gap_ms: 100,
            },
            playback_rate: 1,
            looping: false,
            pause_on_starving: false,
            append_flush: false,
            sole_controller: true,
            finite_drain: None,
        });
        s
    }
    fn finite_subject() -> HandyQualificationSubject {
        let mut s = hsp_subject();
        s.hsp.as_mut().unwrap().finite_drain = Some(HandyHspFiniteDrainQualification {
            schema_version: 1,
            model: HandyHspFiniteDrainModel::EarliestPhaseTailThenOwnedStop,
            maximum_drain_duration_ns: 1000,
            maximum_terminal_overrun_ns: 200,
            maximum_stop_dispatch_delay_ns: 50,
        });
        s
    }
    #[test]
    fn absent_finite_drain_keeps_legacy_v7_subject_and_profile_digests() {
        let s = hsp_subject();
        let old_hsp = r#"{"policy":{"max_buffered_points":100,"device_capacity_points":100,"max_chunk_points":50,"min_lead_ms":100,"max_future_ms":1000,"max_sample_age_ns":1000000,"max_gap_ms":100},"playback_rate":1,"looping":false,"pause_on_starving":false,"append_flush":false,"sole_controller":true}"#;
        // Reconstruct the old v7 bytes from the frozen legacy-v5 fixture and
        // literal pre-extension HSP profile, independently of the new serializer.
        let mut old = serde_json::to_string(&legacy_subject(5))
            .unwrap()
            .replacen("\"schema_version\":5", "\"schema_version\":7", 1)
            .replace(
                HANDY_QUALIFICATION_TRANSPORT,
                HANDY_HSP_QUALIFICATION_TRANSPORT,
            )
            .replace("\"recovery\":{", "\"recovery\":{\"transport\":\"hsp\",");
        assert_eq!(old.pop(), Some('}'));
        old.push_str(",\"hsp\":");
        old.push_str(old_hsp);
        old.push('}');
        assert_eq!(serde_json::to_vec(&s).unwrap(), old.as_bytes());
        assert_eq!(
            s.sha256().unwrap(),
            format!("{:x}", Sha256::digest(old.as_bytes()))
        );
        let old_profile = format!(
            "[{},{},{},{},{},{}]",
            serde_json::to_string(&s.profile).unwrap(),
            serde_json::to_string(&s.operative_settings).unwrap(),
            serde_json::to_string(&s.server_clock).unwrap(),
            serde_json::to_string(&s.device_clock).unwrap(),
            serde_json::to_string(&s.recovery).unwrap(),
            old_hsp
        );
        assert_eq!(
            s.hsp_profile_sha256().unwrap(),
            format!("{:x}", Sha256::digest(old_profile.as_bytes()))
        );
        assert!(s.hsp_finite_drain_policy_sha256().unwrap().is_none());
        let decoded: HandyQualificationSubject = serde_json::from_str(&old).unwrap();
        assert_eq!(decoded, s);
        assert_eq!(s.required_evidence_kinds(), HandyEvidenceKind::ALL);
    }
    #[test]
    fn finite_policy_digest_binds_every_bound_and_unchanged_stopping_bound() {
        let s = finite_subject();
        let hash = s.hsp_finite_drain_policy_sha256().unwrap().unwrap();
        for case in 0..4 {
            let mut changed = s.clone();
            let finite = changed.hsp.as_mut().unwrap().finite_drain.as_mut().unwrap();
            match case {
                0 => finite.maximum_drain_duration_ns += 1,
                1 => finite.maximum_terminal_overrun_ns += 1,
                2 => finite.maximum_stop_dispatch_delay_ns += 1,
                _ => changed.profile.stopping_bound_ns = Some(99),
            }
            assert_ne!(
                changed.hsp_finite_drain_policy_sha256().unwrap().unwrap(),
                hash
            );
        }
        let mut unrelated = s.clone();
        unrelated.unit_label.push('x');
        assert_eq!(
            unrelated.hsp_finite_drain_policy_sha256().unwrap().unwrap(),
            hash
        );
        assert_ne!(unrelated.sha256().unwrap(), s.sha256().unwrap());
        let mut invalid = s;
        invalid.profile.stopping_bound_ns = None;
        assert!(invalid.hsp_finite_drain_policy_sha256().is_err());
    }
    #[test]
    fn finite_drain_evidence_is_conditional_and_cannot_replace_a_legacy_role() {
        let subject = finite_subject();
        let identity = subject.context.control_image.clone();
        let evidence = subject
            .required_evidence_kinds()
            .into_iter()
            .map(|kind| HandyQualificationEvidence {
                kind,
                identity: identity.clone(),
            })
            .collect();
        let manifest = HandyQualificationManifest { subject, evidence };
        manifest.validate().unwrap();
        let mut missing = manifest.clone();
        missing.evidence.pop();
        assert!(missing.validate().is_err());
        let mut mismatched = manifest.clone();
        mismatched.subject.hsp.as_mut().unwrap().finite_drain = None;
        assert!(mismatched.validate().is_err());
        mismatched.evidence.remove(0);
        assert!(mismatched.validate().is_err());
        let input = HandyQualificationInput {
            subject: mismatched.subject,
            evidence: mismatched
                .evidence
                .into_iter()
                .map(|e| HandyQualificationInputEvidence {
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
    fn qualification_versions_one_through_six_keep_exact_legacy_subject_hashes() {
        // Frozen canonical pre-HSP subject digests for each historical schema.
        let expected = [
            "956c75692ade1f828596207ed5fe5f2ed9c92eec618c66cd825c33a6deeb62fc",
            "6336644962c0a40f99314ee99f5a2408942252e8e82847499baa45737eb0f869",
            "8c4e402b2f1e24e0689d38004d77d0e0a4199478e4a0530d98b90fc9c37c9d8e",
            "aad01d33e7a070f7f778173b5cae0b43b5dacd3c2105d15eb7b6fa8d217acc74",
            "13c9a8e4eb98b72b3342041a808a6c9a8b2b6f91edda1be232cd1c272238e7e1",
            "752a33f519ace2044a05f9ab9b9b87367fb967464f8c6a211e5178646bc13085",
        ];
        for (version, hash) in (1..=6).zip(expected) {
            let s = legacy_subject(version);
            s.validate().unwrap();
            let bytes = serde_json::to_vec(&s).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                hash,
                "schema {version}"
            );
            assert_eq!(s.sha256().unwrap(), hash);
            let decoded: HandyQualificationSubject = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
            assert!(decoded.hsp.is_none());
            assert!(decoded
                .recovery
                .as_ref()
                .is_none_or(|r| r.transport.is_hssp()));
            assert!(s.hsp_profile_sha256().is_err());
        }
    }
    #[test]
    fn hsp_qualification_requires_exact_transport_version_and_reviewed_contracts() {
        let good = hsp_subject();
        good.validate().unwrap();
        for case in 0..12 {
            let mut bad = good.clone();
            match case {
                0 => bad.schema_version = 6,
                1 => {
                    bad.context.transport = ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap()
                }
                2 => bad.hsp = None,
                3 => bad.operative_settings = None,
                4 => bad.server_clock = None,
                5 => bad.device_clock = None,
                6 => bad.recovery = None,
                7 => bad.recovery.as_mut().unwrap().transport = HandyQualificationTransport::Hssp,
                8 => bad.continuous_timing = legacy_subject(6).continuous_timing,
                9 => bad.hsp.as_mut().unwrap().sole_controller = false,
                10 => bad.profile.maximum_clock_uncertainty_ns = 1,
                _ => bad.profile.stopping_bound_ns = None,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        for version in 1..=6 {
            let mut old = legacy_subject(version);
            old.hsp = good.hsp.clone();
            assert!(old.validate().is_err());
            old.hsp = None;
            old.context = good.context.clone();
            assert!(old.validate().is_err());
        }
    }
    #[test]
    fn hsp_profile_digest_binds_every_contract_and_subject_digest_binds_identity() {
        let good = hsp_subject();
        let profile = good.hsp_profile_sha256().unwrap();
        let subject = good.sha256().unwrap();
        for case in 0..7 {
            let mut changed = good.clone();
            match case {
                0 => changed.profile.maximum_step_percent = 2,
                1 => {
                    changed
                        .operative_settings
                        .as_mut()
                        .unwrap()
                        .slider
                        .x_inverse_motor = true
                }
                2 => changed.server_clock.as_mut().unwrap().timestamp_error_ns = 2,
                3 => changed.device_clock.as_mut().unwrap().device_error_ns = 2,
                4 => changed.recovery.as_mut().unwrap().prior_command_settle_ns = 101,
                5 => changed.hsp.as_mut().unwrap().policy.min_lead_ms = 101,
                _ => changed.hsp.as_mut().unwrap().policy.max_sample_age_ns += 1,
            }
            assert_ne!(
                changed.hsp_profile_sha256().unwrap(),
                profile,
                "case {case}"
            );
            assert_ne!(changed.sha256().unwrap(), subject);
        }
        let mut other_unit = good.clone();
        other_unit.unit_label.push_str(" other");
        assert_eq!(other_unit.hsp_profile_sha256().unwrap(), profile);
        assert_ne!(other_unit.sha256().unwrap(), subject);
        let wire = serde_json::to_vec(&good).unwrap();
        let decoded: HandyQualificationSubject = serde_json::from_slice(&wire).unwrap();
        assert_eq!(decoded.hsp_profile_sha256().unwrap(), profile);
        assert_eq!(decoded.sha256().unwrap(), subject);
    }
    #[test]
    fn qualification_device_clock_requires_v4_and_a_single_total_error_and_drift_budget() {
        let mut s = subject();
        s.schema_version = 4;
        s.operative_settings = Some(
            serde_json::from_str(include_str!(
                "../tests/fixtures/handy-operative-settings.json"
            ))
            .unwrap(),
        );
        s.server_clock = Some(HandyServerClockQualification {
            model: HandyServerClockModel::RequestBracketMonotonic,
            timestamp_error_ns: 1,
        });
        s.device_clock = Some(HandyDeviceClockQualification {
            model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
            user_offset_ms: -50,
            device_error_ns: 1,
            maximum_sync_age_ns: 10,
            maximum_sync_round_trip_ms: 0,
            drift_ppm: 1,
        });
        assert!(s.validate().is_err());
        s.profile.maximum_clock_uncertainty_ns = 2;
        s.validate().unwrap();
        let encoded = serde_json::to_vec(&s).unwrap();
        serde_json::from_slice::<HandyQualificationSubject>(&encoded)
            .unwrap()
            .validate()
            .unwrap();
        for case in 0..5 {
            let mut bad = s.clone();
            match case {
                0 => bad.schema_version = 3,
                1 => bad.device_clock = None,
                2 => bad.server_clock = None,
                3 => bad.device_clock.as_mut().unwrap().maximum_sync_age_ns = 0,
                _ => bad.profile.drift_ppm = u32::MAX,
            };
            assert!(bad.validate().is_err());
        }
        // Existing subject bytes and identity remain unchanged until recovery is
        // explicitly included in a new qualification subject.
        assert!(!String::from_utf8(encoded.clone())
            .unwrap()
            .contains("recovery"));
        let mut recovered: HandyQualificationSubject = serde_json::from_slice(&encoded).unwrap();
        recovered.schema_version = 5;
        recovered.profile.stopping_bound_ns = Some(100);
        recovered.recovery = Some(HandyRecoveryQualification {
            transport: HandyQualificationTransport::Hssp,
            model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
            remote_state: HandyRecoveryRemoteState::Stopped,
            prior_command_settle_ns: 100,
            stopped_sample_separation_ns: 100,
            maximum_sample_span_ns: 100,
        });
        recovered.validate().unwrap();
        for case in 0..6 {
            let mut bad = recovered.clone();
            match case {
                0 => bad.schema_version = 4,
                1 => bad.recovery = None,
                2 => bad.profile.stopping_bound_ns = None,
                3 => bad.profile.stopping_bound_ns = Some(101),
                4 => bad.operative_settings = None,
                _ => bad.device_clock = None,
            }
            assert!(bad.validate().is_err(), "recovery case {case}");
        }
        let old_bytes = serde_json::to_vec(&recovered).unwrap();
        assert!(!String::from_utf8(old_bytes.clone())
            .unwrap()
            .contains("continuous_timing"));
        let decoded: HandyQualificationSubject = serde_json::from_slice(&old_bytes).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), old_bytes);
        assert!(decoded.continuous_timing.is_none());

        let mut continuous = recovered;
        continuous.schema_version = 6;
        continuous.continuous_timing = Some(HandyContinuousTimingQualification {
            model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
            timestamp_error_ns: 1,
            maximum_observation_age_ns: 100,
            maximum_sample_span_ns: 50,
            drift_ppm: 1,
        });
        continuous.validate().unwrap();
        let bytes = serde_json::to_vec(&continuous).unwrap();
        let decoded: HandyQualificationSubject = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, continuous);
        for case in 0..10 {
            let mut bad = continuous.clone();
            match case {
                0 => bad.schema_version = 5,
                1 => bad.continuous_timing = None,
                2 => bad.recovery = None,
                3 => bad.device_clock = None,
                4 => bad.server_clock = None,
                5 => bad.operative_settings = None,
                6 => bad.continuous_timing.as_mut().unwrap().timestamp_error_ns = 0,
                7 => bad.continuous_timing.as_mut().unwrap().timestamp_error_ns = 3,
                8 => {
                    bad.continuous_timing
                        .as_mut()
                        .unwrap()
                        .maximum_observation_age_ns = 0
                }
                _ => {
                    bad.continuous_timing
                        .as_mut()
                        .unwrap()
                        .maximum_sample_span_ns = 101
                }
            }
            assert!(
                bad.validate().is_err(),
                "continuous qualification case {case}"
            );
        }
    }
    #[test]
    fn qualification_server_clock_contract_is_explicit_versioned_and_bounded() {
        let old = subject();
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("server_clock"));
        serde_json::from_str::<HandyQualificationSubject>(&json)
            .unwrap()
            .validate()
            .unwrap();
        let mut s = old;
        s.server_clock = Some(HandyServerClockQualification {
            model: HandyServerClockModel::RequestBracketMonotonic,
            timestamp_error_ns: 1,
        });
        assert!(s.validate().is_err());
        s.schema_version = 3;
        s.operative_settings = Some(
            serde_json::from_str(include_str!(
                "../tests/fixtures/handy-operative-settings.json"
            ))
            .unwrap(),
        );
        s.validate().unwrap();
        s.server_clock.as_mut().unwrap().timestamp_error_ns = 2;
        assert!(s.validate().is_err());
        s.server_clock = None;
        assert!(s.validate().is_err());
    }
    fn subject() -> HandyQualificationSubject {
        HandyQualificationSubject {
            schema_version: 1,
            origin: HandyEvidenceOrigin::Synthetic,
            unit_label: "synthetic protocol fixture".into(),
            device_id: DeviceId::new("device").unwrap(),
            registration_revision: 1,
            registration_binding_sha256: "a".repeat(64),
            identity: HandyIdentity {
                firmware: "4.0.16".into(),
                model_number: 2,
                model_name: "H02".into(),
                model_variant: 0,
                session_id: "fixture".into(),
                firmware_status: 0,
            },
            context: HandyQualificationContext {
                schema_version: 1,
                control_image: ArtifactIdentity {
                    artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
                    sha256: "b".repeat(64),
                    byte_len: 1,
                },
                platform: "linux/fixture".into(),
                transport: ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap(),
            },
            operative_settings: None,
            server_clock: None,
            device_clock: None,
            recovery: None,
            continuous_timing: None,
            hsp: None,
            profile: HandyHsspQualificationProfile {
                dynamics: DynamicsProfile {
                    axes: vec![AxisDynamics {
                        axis: Axis::Stroke,
                        unit: "mm".into(),
                        minimum: 0.0,
                        maximum: 100.0,
                        max_velocity: 1.0,
                        max_acceleration: 1.0,
                        max_jerk: 1.0,
                    }],
                    coupled: vec![],
                    minimum_gain: 0.0,
                },
                minimum_segment_ms: 1,
                maximum_step_percent: 1,
                minimum_position_percent: 0,
                maximum_position_percent: 100,
                maximum_command_age_ns: 1,
                maximum_clock_uncertainty_ns: 1,
                drift_ppm: 1,
                stopping_bound_ns: None,
            },
        }
    }
    #[test]
    fn qualification_roles_digest_and_synthetic_acceptance_are_strict() {
        let evidence = HandyEvidenceKind::ALL
            .into_iter()
            .map(|kind| HandyQualificationEvidence {
                kind,
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("c".repeat(64)).unwrap(),
                    sha256: "c".repeat(64),
                    byte_len: 1,
                },
            })
            .collect();
        let manifest = HandyQualificationManifest {
            subject: subject(),
            evidence,
        };
        manifest.validate().unwrap();
        let id = manifest.identity().unwrap();
        let mut bad = manifest.clone();
        bad.evidence.swap(0, 1);
        assert!(bad.validate().is_err());
        bad = manifest.clone();
        bad.evidence.pop();
        assert!(bad.validate().is_err());
        let mut report = HandyQualificationReport {
            dossier_id: id,
            revision: 1,
            state: HandyQualificationState::Pending,
            manifest,
            decision: None,
        };
        report.validate().unwrap();
        report.manifest.subject.profile.maximum_step_percent = 2;
        assert!(report.validate().is_err());
        report.dossier_id = report.manifest.identity().unwrap();
        report.revision = 2;
        report.state = HandyQualificationState::Accepted;
        report.decision = Some(HandyQualificationDecision {
            revision: 2,
            state: HandyQualificationState::Accepted,
            request_id: RequestId::new("review").unwrap(),
            actor_session: SessionId::new("fixture").unwrap(),
            note: "synthetic fixture must not qualify".into(),
            recorded_at_unix_ms: 1,
        });
        assert!(report.validate().is_err());
    }
    #[test]
    fn qualification_review_requires_explicit_consents_and_incrementable_revision() {
        let mut r = Request::new(
            RequestId::new("review").unwrap(),
            Command::AcceptHandyQualification {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                expected_qualification_revision: 1,
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("session").unwrap(),
                note: "explicit operator review fixture".into(),
                acknowledge_physical_evidence: false,
                allow_online_inspection: true,
                owner_proof: "b".repeat(64),
            },
        );
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        if let Command::AcceptHandyQualification {
            acknowledge_physical_evidence,
            ..
        } = &mut r.command
        {
            *acknowledge_physical_evidence = true;
        }
        r.validate().unwrap();
        if let Command::AcceptHandyQualification {
            expected_qualification_revision,
            ..
        } = &mut r.command
        {
            *expected_qualification_revision = i64::MAX as u64;
        }
        assert!(r.validate().is_err());
        assert!(!Command::HandyQualificationContext {
            transport: HandyQualificationTransport::Hssp,
            owner_proof: "a".repeat(64)
        }
        .is_mutating());
    }
}

/// Selection of an accepted record, not an executable qualification grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyQualificationBinding {
    pub dossier_id: ArtifactId,
    pub revision: u64,
}
impl HandyQualificationBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.dossier_id.as_str())?;
        if self.revision < 2 || self.revision > i64::MAX as u64 {
            return Err(ProtocolError::invalid(
                "accepted qualification needs a valid review revision",
            ));
        }
        Ok(())
    }
}
