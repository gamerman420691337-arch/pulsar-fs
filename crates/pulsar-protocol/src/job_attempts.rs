//! Local attempt history. Observations never grant retry or fallback authority.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAttemptDiagnostic {
    pub code: ErrorCode,
    pub message: String,
    pub message_sha256: String,
    pub message_truncated: bool,
    pub retryable: bool,
}
impl JobAttemptDiagnostic {
    pub fn from_error(error: &ProtocolError) -> Self {
        let mut end = error.message.len().min(4096);
        while !error.message.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            code: error.code,
            message: error.message[..end].to_owned(),
            message_sha256: format!("{:x}", Sha256::digest(error.message.as_bytes())),
            message_truncated: end < error.message.len(),
            retryable: error.retryable,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.message_sha256)?;
        if self.message.len() > 4096
            || (!self.message_truncated
                && self.message_sha256 != format!("{:x}", Sha256::digest(self.message.as_bytes())))
        {
            return Err(ProtocolError::invalid(
                "attempt diagnostic length or digest differs",
            ));
        }
        Ok(())
    }
}

/// A historical observation that the confined worker tree retired. This is not
/// a live reservation, a permission, or evidence that a device stopped.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRetirementObservation {
    pub worker_error: Option<JobAttemptDiagnostic>,
    pub provider_unavailable: Option<InferenceProviderUnavailable>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAttemptOutcome {
    pub schema_version: u16,
    pub manifest_sha256: String,
    pub engine_epoch: AttemptId,
    /// Validation of the returned execution/result, independent of whether the
    /// current grant/state allowed candidate publication.
    pub validation_error: Option<JobAttemptDiagnostic>,
    /// None is unknown; cancellation or terminal state never implies retirement.
    pub worker_retirement: Option<WorkerRetirementObservation>,
}
impl JobAttemptOutcome {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1 {
            return Err(ProtocolError::invalid(
                "unsupported attempt outcome version",
            ));
        }
        validate_sha256(&self.manifest_sha256)?;
        if let Some(error) = &self.validation_error {
            error.validate()?;
        }
        if let Some(retired) = &self.worker_retirement {
            if let Some(error) = &retired.worker_error {
                error.validate()?;
                if self.validation_error.as_ref() != Some(error) {
                    return Err(ProtocolError::invalid(
                        "settled worker failure differs from attempt validation error",
                    ));
                }
            }
            if retired.provider_unavailable.is_some()
                && !retired
                    .worker_error
                    .as_ref()
                    .is_some_and(|e| e.code == ErrorCode::Unavailable && !e.retryable)
            {
                return Err(ProtocolError::invalid(
                    "provider absence requires a nonretryable settled worker failure",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_for(&self, request: &WorkerRequest) -> Result<(), ProtocolError> {
        self.validate()?;
        request.validate()?;
        let bytes =
            serde_json::to_vec(request).map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if self.manifest_sha256 != format!("{:x}", Sha256::digest(&bytes)) {
            return Err(ProtocolError::invalid(
                "attempt outcome manifest binding differs",
            ));
        }
        if let Some(absence) = self
            .worker_retirement
            .as_ref()
            .and_then(|r| r.provider_unavailable.as_ref())
        {
            let diagnostic = self
                .worker_retirement
                .as_ref()
                .and_then(|r| r.worker_error.as_ref())
                .ok_or_else(|| {
                    ProtocolError::invalid("provider absence lacks original worker error")
                })?;
            // Reuse the wire correspondence validator over retained values.
            // Constructing this view of history does not establish retirement.
            WorkerResult {
                cuda_artifacts: None,
                provider_unavailable: Some(absence.clone()),
                version: request.version,
                attempt_id: request.attempt_id.clone(),
                job_id: request.job_id.clone(),
                result: Err(ProtocolError {
                    code: diagnostic.code,
                    message: diagnostic.message.clone(),
                    retryable: diagnostic.retryable,
                }),
            }
            .validate_provider_unavailable_for(request)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobAttemptFallbackKind {
    Initial,
    Resume,
    Fallback,
}

/// Historical provenance from retained admission metadata. This does not grant
/// another attempt or prove that the referenced decision is still accepted.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAttemptFallbackTrace {
    pub schema_version: u16,
    pub selection: ArtifactIdentity,
    pub kind: JobAttemptFallbackKind,
    pub ordinal: u8,
    pub predecessor: Option<AttemptId>,
    pub reference: InferenceFallbackReference,
    pub observed_environment_sha256: String,
    pub admitted_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub history_only: bool,
}
impl JobAttemptFallbackTrace {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.selection.validate()?;
        self.reference.validate()?;
        validate_sha256(self.reference.dossier_id.as_str())?;
        validate_sha256(&self.observed_environment_sha256)?;
        if self.schema_version != 1
            || !self.history_only
            || usize::from(self.ordinal) > MAX_INFERENCE_FALLBACKS
            || self.selection.artifact_id.as_str()
                != format!(
                    "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{}",
                    self.selection.sha256
                )
            || self.selection.byte_len == 0
            || self.selection.byte_len > INFERENCE_EQUIVALENCE_MANIFEST_LIMIT
            || self.admitted_at_unix_ms == 0
            || self.admitted_at_unix_ms >= self.expires_at_unix_ms
            || self.expires_at_unix_ms > i64::MAX as u64
        {
            return Err(ProtocolError::invalid(
                "fallback attempt history identity, bounds or clocks differ",
            ));
        }
        let coherent = match self.kind {
            JobAttemptFallbackKind::Initial => self.ordinal == 0 && self.predecessor.is_none(),
            JobAttemptFallbackKind::Resume => self.predecessor.is_some(),
            JobAttemptFallbackKind::Fallback => self.ordinal > 0 && self.predecessor.is_some(),
        };
        if !coherent {
            return Err(ProtocolError::invalid(
                "fallback attempt history kind, ordinal or predecessor differs",
            ));
        }
        Ok(())
    }
    pub fn validate_for_attempt(&self, attempt: &AttemptId) -> Result<(), ProtocolError> {
        self.validate()?;
        if self.predecessor.as_ref() == Some(attempt) {
            return Err(ProtocolError::invalid(
                "fallback attempt history cannot name itself as predecessor",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAttemptRecord {
    pub attempt_id: AttemptId,
    pub state: String,
    pub manifest_sha256: String,
    /// An explicit admitted declaration, not proof of executed node placement.
    /// Historical implicit defaults are deliberately not reconstructed.
    pub declared_profile: Option<DeclaredInferenceProfile>,
    pub declared_profile_sha256: Option<String>,
    pub outcome: Option<JobAttemptOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<JobAttemptFallbackTrace>,
}
impl JobAttemptRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.manifest_sha256)?;
        if !matches!(
            self.state.as_str(),
            "queued"
                | "running"
                | "completed"
                | "failed"
                | "cancelled"
                | "interrupted"
                | "assistant_paused"
        ) {
            return Err(ProtocolError::invalid("unknown job attempt state"));
        }
        match (&self.declared_profile, &self.declared_profile_sha256) {
            (None, None) => {}
            (Some(profile), Some(hash)) => {
                profile.validate()?;
                validate_sha256(hash)?;
                let bytes = serde_json::to_vec(profile)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                if *hash != format!("{:x}", Sha256::digest(bytes)) {
                    return Err(ProtocolError::invalid(
                        "attempt declared profile digest differs",
                    ));
                }
            }
            _ => {
                return Err(ProtocolError::invalid(
                    "attempt declared profile/hash presence differs",
                ))
            }
        }
        if let Some(fallback) = &self.fallback {
            fallback.validate_for_attempt(&self.attempt_id)?;
            if self.declared_profile.is_none() {
                return Err(ProtocolError::invalid(
                    "fallback attempt history requires its explicit admitted profile",
                ));
            }
        }
        if let Some(outcome) = &self.outcome {
            outcome.validate()?;
            if outcome.manifest_sha256 != self.manifest_sha256 {
                return Err(ProtocolError::invalid("attempt history manifest differs"));
            }
            if let Some(absence) = outcome
                .worker_retirement
                .as_ref()
                .and_then(|r| r.provider_unavailable.as_ref())
            {
                let profile = self.declared_profile.as_ref().ok_or_else(|| {
                    ProtocolError::invalid("provider history requires an explicit profile")
                })?;
                absence.validate_for(
                    profile,
                    self.declared_profile_sha256.as_deref().ok_or_else(|| {
                        ProtocolError::invalid("provider history profile hash missing")
                    })?,
                )?;
            }
        }
        Ok(())
    }
}

/// Bounded lexical-ID page, not chronological ordering or restored authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobAttemptPage {
    pub schema_version: u16,
    pub project_id: ProjectId,
    pub job_id: JobId,
    pub current_attempt: AttemptId,
    pub attempts: Vec<JobAttemptRecord>,
    pub next_after: Option<AttemptId>,
    pub history_only: bool,
}
impl JobAttemptPage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != 1 || !self.history_only || self.attempts.len() > 16 {
            return Err(ProtocolError::invalid(
                "job attempt page bounds or history scope differs",
            ));
        }
        for (index, record) in self.attempts.iter().enumerate() {
            record.validate()?;
            if index > 0
                && self.attempts[index - 1].attempt_id.as_str() >= record.attempt_id.as_str()
            {
                return Err(ProtocolError::invalid(
                    "job attempt page order or identity differs",
                ));
            }
        }
        if self
            .next_after
            .as_ref()
            .is_some_and(|id| self.attempts.last().map(|r| &r.attempt_id) != Some(id))
        {
            return Err(ProtocolError::invalid("job attempt next cursor differs"));
        }
        if serde_json::to_vec(self)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            .len()
            > 512 * 1024
        {
            return Err(ProtocolError::invalid(
                "job attempt page exceeds byte admission",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> DeclaredInferenceProfile {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact.clone(),
            runtime: artifact,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }
    fn fallback() -> JobAttemptFallbackTrace {
        let sha256 = "b".repeat(64);
        JobAttemptFallbackTrace {
            schema_version: 1,
            selection: ArtifactIdentity {
                artifact_id: ArtifactId::new(format!(
                    "{INFERENCE_FALLBACK_SELECTION_ID_PREFIX}{sha256}"
                ))
                .unwrap(),
                sha256,
                byte_len: 100,
            },
            kind: JobAttemptFallbackKind::Initial,
            ordinal: 0,
            predecessor: None,
            reference: InferenceFallbackReference {
                dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
                expected_revision: 2,
                expected_report_sha256: "d".repeat(64),
            },
            observed_environment_sha256: "e".repeat(64),
            admitted_at_unix_ms: 1,
            expires_at_unix_ms: 10,
            history_only: true,
        }
    }
    fn fallback_record() -> JobAttemptRecord {
        let profile = profile();
        JobAttemptRecord {
            attempt_id: AttemptId::new("attempt").unwrap(),
            state: "queued".into(),
            manifest_sha256: "f".repeat(64),
            declared_profile_sha256: Some(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&profile).unwrap())
            )),
            declared_profile: Some(profile),
            outcome: None,
            fallback: Some(fallback()),
        }
    }

    #[test]
    fn fallback_history_is_optional_without_changing_legacy_serialization() {
        let mut record = fallback_record();
        record.fallback = None;
        let legacy = serde_json::to_value(&record).unwrap();
        assert!(legacy.get("fallback").is_none());
        let decoded: JobAttemptRecord = serde_json::from_value(legacy.clone()).unwrap();
        decoded.validate().unwrap();
        assert!(decoded.fallback.is_none());
        assert_eq!(serde_json::to_value(decoded).unwrap(), legacy);
        let encoded = serde_json::to_value(fallback_record()).unwrap();
        let decoded: JobAttemptRecord = serde_json::from_value(encoded.clone()).unwrap();
        decoded.validate().unwrap();
        assert!(decoded.fallback.unwrap().history_only);
        let mut permission = encoded;
        permission["fallback"]["permission"] = true.into();
        assert!(serde_json::from_value::<JobAttemptRecord>(permission).is_err());
    }

    #[test]
    fn fallback_history_kind_clocks_tags_and_self_predecessor_are_checked() {
        fallback_record().validate().unwrap();
        for mutate in [
            |t: &mut JobAttemptFallbackTrace| t.schema_version = 2,
            |t: &mut JobAttemptFallbackTrace| t.history_only = false,
            |t: &mut JobAttemptFallbackTrace| t.ordinal = 9,
            |t: &mut JobAttemptFallbackTrace| t.ordinal = 1,
            |t: &mut JobAttemptFallbackTrace| t.kind = JobAttemptFallbackKind::Resume,
            |t: &mut JobAttemptFallbackTrace| t.kind = JobAttemptFallbackKind::Fallback,
            |t: &mut JobAttemptFallbackTrace| t.selection.byte_len = 0,
            |t: &mut JobAttemptFallbackTrace| {
                t.selection.byte_len = INFERENCE_EQUIVALENCE_MANIFEST_LIMIT + 1
            },
            |t: &mut JobAttemptFallbackTrace| {
                t.selection.artifact_id = ArtifactId::new(t.selection.sha256.clone()).unwrap()
            },
            |t: &mut JobAttemptFallbackTrace| {
                t.reference.dossier_id = ArtifactId::new("unbound-dossier").unwrap()
            },
            |t: &mut JobAttemptFallbackTrace| t.observed_environment_sha256 = "not-a-hash".into(),
            |t: &mut JobAttemptFallbackTrace| t.admitted_at_unix_ms = 0,
            |t: &mut JobAttemptFallbackTrace| t.admitted_at_unix_ms = t.expires_at_unix_ms,
            |t: &mut JobAttemptFallbackTrace| t.expires_at_unix_ms = i64::MAX as u64 + 1,
        ] {
            let mut trace = fallback();
            mutate(&mut trace);
            assert!(trace.validate().is_err());
        }
        let mut record = fallback_record();
        let trace = record.fallback.as_mut().unwrap();
        trace.kind = JobAttemptFallbackKind::Resume;
        trace.predecessor = Some(AttemptId::new("previous").unwrap());
        record.validate().unwrap();
        let trace = record.fallback.as_mut().unwrap();
        trace.kind = JobAttemptFallbackKind::Fallback;
        trace.ordinal = 1;
        record.validate().unwrap();
        record.fallback.as_mut().unwrap().predecessor = Some(record.attempt_id.clone());
        assert!(record.validate().is_err());
        record.fallback = Some(fallback());
        record.declared_profile = None;
        record.declared_profile_sha256 = None;
        assert!(record.validate().is_err());
    }

    #[test]
    fn fallback_trace_counts_toward_bounded_history_pages_and_keeps_the_cursor_exact() {
        let mut page = JobAttemptPage {
            schema_version: 1,
            project_id: ProjectId::new("project").unwrap(),
            job_id: JobId::new("job").unwrap(),
            current_attempt: AttemptId::new("attempt-15").unwrap(),
            attempts: (0..16)
                .map(|index| {
                    let mut record = fallback_record();
                    record.attempt_id = AttemptId::new(format!("attempt-{index:02}")).unwrap();
                    record
                })
                .collect(),
            next_after: Some(AttemptId::new("attempt-15").unwrap()),
            history_only: true,
        };
        page.validate().unwrap();
        let traced = serde_json::to_vec(&page).unwrap().len();
        assert!(traced <= 512 * 1024);
        let mut legacy = page.clone();
        for record in &mut legacy.attempts {
            record.fallback = None;
        }
        assert!(serde_json::to_vec(&legacy).unwrap().len() < traced);
        page.next_after = Some(AttemptId::new("wrong-cursor").unwrap());
        assert!(page.validate().is_err());
        page.next_after = None;
        let mut extra = fallback_record();
        extra.attempt_id = AttemptId::new("attempt-16").unwrap();
        page.attempts.push(extra);
        assert!(page.validate().is_err());
    }

    #[test]
    fn diagnostics_bound_utf8_without_losing_error_code_or_full_message_identity() {
        let error = ProtocolError::new(ErrorCode::DependencyMismatch, "界".repeat(2000));
        let diagnostic = JobAttemptDiagnostic::from_error(&error);
        diagnostic.validate().unwrap();
        assert!(diagnostic.message_truncated);
        assert_eq!(diagnostic.message.len(), 4095);
        assert_eq!(diagnostic.code, ErrorCode::DependencyMismatch);
        assert_eq!(
            diagnostic.message_sha256,
            format!("{:x}", Sha256::digest(error.message.as_bytes()))
        );
        let mut false_complete = diagnostic;
        false_complete.message_truncated = false;
        assert!(false_complete.validate().is_err());
    }
    #[test]
    fn absence_of_retirement_stays_unknown_and_settled_errors_cannot_change_cause() {
        let mut outcome = JobAttemptOutcome {
            schema_version: 1,
            manifest_sha256: "a".repeat(64),
            engine_epoch: AttemptId::new("epoch").unwrap(),
            validation_error: Some(JobAttemptDiagnostic::from_error(&ProtocolError::new(
                ErrorCode::Cancelled,
                "cancelled",
            ))),
            worker_retirement: None,
        };
        outcome.validate().unwrap();
        assert!(outcome.worker_retirement.is_none());
        outcome.worker_retirement = Some(WorkerRetirementObservation {
            worker_error: Some(JobAttemptDiagnostic::from_error(&ProtocolError::new(
                ErrorCode::Unavailable,
                "different cause",
            ))),
            provider_unavailable: None,
        });
        assert!(outcome.validate().is_err());
    }
}
