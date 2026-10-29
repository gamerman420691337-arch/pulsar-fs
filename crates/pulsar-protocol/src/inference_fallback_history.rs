//! Inert, original-namespace fallback history. Neither parsing nor validation
//! recreates acceptance, resources, provider observations or execution leases.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MAX_INFERENCE_FALLBACK_HISTORY_ATTEMPTS: usize = 128;
pub const MAX_INFERENCE_FALLBACK_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedInferenceFallbackAttempt {
    pub attempt_id: AttemptId,
    pub manifest_sha256: String,
    pub profile_sha256: String,
    pub state: String,
    pub trace: JobAttemptFallbackTrace,
    pub observed_environment: InferenceEquivalenceEnvironment,
    /// Missing means unknown. Terminal state never implies native retirement.
    pub outcome: Option<JobAttemptOutcome>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceFallbackHistory {
    pub schema_version: u16,
    pub history_only: bool,
    pub project_id: ProjectId,
    pub job_id: JobId,
    pub base_revision: RevisionId,
    pub source_version: SourceVersionId,
    pub initial_attempt: AttemptId,
    pub current_attempt: AttemptId,
    pub selection: InferenceFallbackSelection,
    /// Original predecessor order, from initial attempt to captured current.
    pub attempts: Vec<ArchivedInferenceFallbackAttempt>,
}

fn check(value: bool, message: &str) -> Result<(), ProtocolError> {
    if value {
        Ok(())
    } else {
        Err(ProtocolError::invalid(message))
    }
}
fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    serde_json::to_vec(value).map_err(|e| ProtocolError::invalid(e.to_string()))
}
fn hash<T: Serialize>(value: &T) -> Result<String, ProtocolError> {
    Ok(format!("{:x}", Sha256::digest(bytes(value)?)))
}
fn same<T: Serialize>(a: &T, b: &T) -> Result<bool, ProtocolError> {
    Ok(bytes(a)? == bytes(b)?)
}
fn profile(request: &WorkerRequest) -> Result<&DeclaredInferenceProfile, ProtocolError> {
    match &request.operation {
        WorkerOperation::Generate {
            settings,
            model: Some(_),
            ..
        } => settings.inference_profile.as_ref(),
        _ => None,
    }
    .ok_or_else(|| ProtocolError::invalid("fallback history requires explicit model generation"))
}

impl InferenceFallbackHistory {
    /// Validate bounded metadata and complete original predecessor membership.
    /// Worker bytes must additionally pass `validate_with_manifests` before use.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1 && self.history_only,
            "fallback archive version or inert-history declaration differs",
        )?;
        check(
            !self.attempts.is_empty()
                && self.attempts.len() <= MAX_INFERENCE_FALLBACK_HISTORY_ATTEMPTS,
            "fallback archive attempt count differs",
        )?;
        self.selection.validate()?;
        let selected = self.selection.identity()?;
        let mut ids = BTreeSet::new();
        let mut profiles = BTreeSet::new();
        let mut size = bytes(&self.selection)?.len();
        for (i, attempt) in self.attempts.iter().enumerate() {
            validate_sha256(&attempt.manifest_sha256)?;
            validate_sha256(&attempt.profile_sha256)?;
            attempt.trace.validate_for_attempt(&attempt.attempt_id)?;
            attempt.observed_environment.validate()?;
            check(
                ids.insert(&attempt.attempt_id),
                "fallback archive has duplicate attempts",
            )?;
            check(
                matches!(
                    attempt.state.as_str(),
                    "queued"
                        | "running"
                        | "completed"
                        | "failed"
                        | "cancelled"
                        | "interrupted"
                        | "assistant_paused"
                ),
                "fallback archive attempt state differs",
            )?;
            check(
                attempt.trace.selection == selected
                    && attempt.trace.ordinal <= self.selection.maximum_fallbacks
                    && attempt.trace.observed_environment_sha256
                        == attempt.observed_environment.identity_sha256()?,
                "fallback archive selection, ordinal or environment differs",
            )?;
            let mut matching_references = 0;
            for reference in &self.selection.equivalents {
                if same(reference, &attempt.trace.reference)? {
                    matching_references += 1;
                }
            }
            check(
                matching_references == 1,
                "fallback archive reference is not selected",
            )?;
            if let Some(outcome) = &attempt.outcome {
                outcome.validate()?;
                check(
                    outcome.manifest_sha256 == attempt.manifest_sha256,
                    "fallback archive outcome manifest differs",
                )?;
            }
            if attempt.trace.kind != JobAttemptFallbackKind::Resume {
                check(
                    profiles.insert(&attempt.profile_sha256),
                    "fallback archive profile cycle",
                )?;
            }
            if i == 0 {
                check(
                    attempt.attempt_id == self.initial_attempt
                        && attempt.trace.kind == JobAttemptFallbackKind::Initial,
                    "fallback archive initial attempt differs",
                )?;
            } else {
                let previous = &self.attempts[i - 1];
                check(
                    attempt.trace.predecessor.as_ref() == Some(&previous.attempt_id)
                        && attempt.trace.admitted_at_unix_ms >= previous.trace.admitted_at_unix_ms,
                    "fallback archive predecessor or clock order differs",
                )?;
                match attempt.trace.kind {
                    JobAttemptFallbackKind::Initial => {
                        return Err(ProtocolError::invalid(
                            "fallback archive has a second initial attempt",
                        ))
                    }
                    JobAttemptFallbackKind::Resume => check(
                        matches!(
                            previous.state.as_str(),
                            "interrupted" | "assistant_paused" | "cancelled"
                        ) && previous.profile_sha256 == attempt.profile_sha256
                            && previous.trace.ordinal == attempt.trace.ordinal,
                        "fallback archive resume basis differs",
                    )?,
                    JobAttemptFallbackKind::Fallback => {
                        check(
                            previous.state == "failed"
                                && previous.profile_sha256 != attempt.profile_sha256
                                && previous.trace.ordinal.checked_add(1)
                                    == Some(attempt.trace.ordinal),
                            "fallback archive failed predecessor or ordinal differs",
                        )?;
                        check(
                            previous
                                .outcome
                                .as_ref()
                                .and_then(|o| o.worker_retirement.as_ref())
                                .is_some_and(|r| r.provider_unavailable.is_some()),
                            "fallback archive lacks the original provider-unavailable settlement",
                        )?;
                    }
                }
            }
            let entry_bytes = bytes(attempt)?.len();
            check(
                entry_bytes <= MAX_ENTRY_BYTES,
                "fallback archive entry exceeds byte admission",
            )?;
            size = size
                .checked_add(entry_bytes)
                .ok_or_else(|| ProtocolError::invalid("fallback archive size overflow"))?;
            check(
                size <= MAX_INFERENCE_FALLBACK_HISTORY_BYTES,
                "fallback archive exceeds byte admission",
            )?;
        }
        check(
            self.attempts
                .last()
                .is_some_and(|a| a.attempt_id == self.current_attempt),
            "fallback archive current attempt differs",
        )?;
        check(
            bytes(self)?.len() <= MAX_INFERENCE_FALLBACK_HISTORY_BYTES,
            "fallback archive encoding exceeds byte admission",
        )
    }

    /// Resolve only original immutable manifests, from a DB snapshot or checked
    /// archive objects. The resolver never supplies current execution authority.
    pub fn validate_with_manifests(
        &self,
        mut resolve: impl FnMut(&AttemptId) -> Result<WorkerRequest, ProtocolError>,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        let selected = self.selection.identity()?;
        let mut previous: Option<WorkerRequest> = None;
        for entry in &self.attempts {
            let request = resolve(&entry.attempt_id)?;
            request.validate()?;
            check(
                bytes(&request)?.len() <= 1024 * 1024,
                "fallback archive worker manifest exceeds byte admission",
            )?;
            let declared = profile(&request)?;
            check(
                request.attempt_id == entry.attempt_id
                    && request.project_id == self.project_id
                    && request.job_id == self.job_id
                    && request.base_revision == self.base_revision
                    && request.source.source_version == self.source_version
                    && hash(&request)? == entry.manifest_sha256
                    && hash(declared)? == entry.profile_sha256,
                "fallback archive immutable worker identity differs",
            )?;
            check(
                self.selection.matches_worker_request(&request)?
                    && request
                        .dependencies
                        .iter()
                        .filter(|d| {
                            d.artifact_id
                                .as_str()
                                .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
                        })
                        .count()
                        == 1
                    && request.dependencies.contains(&selected)
                    && request
                        .dependencies
                        .contains(&entry.observed_environment.worker)
                    && entry
                        .observed_environment
                        .dependencies
                        .contains(&declared.runtime),
                "fallback archive policy or environment worker/runtime differs",
            )?;
            if let Some(outcome) = &entry.outcome {
                outcome.validate_for(&request)?;
            }
            if let Some(old) = &previous {
                match entry.trace.kind {
                    JobAttemptFallbackKind::Fallback => {
                        validate_inference_fallback_successor(old, &request)?
                    }
                    JobAttemptFallbackKind::Resume => check(
                        old.source.identity == request.source.identity
                            && same(&old.independent_audio, &request.independent_audio)?
                            && same(&old.operation, &request.operation)?
                            && same(&old.budget, &request.budget)?
                            && old.tools.ffmpeg.identity == request.tools.ffmpeg.identity
                            && old.tools.ffprobe.identity == request.tools.ffprobe.identity,
                        "fallback archive resume changed its original generation basis",
                    )?,
                    JobAttemptFallbackKind::Initial => {
                        return Err(ProtocolError::invalid(
                            "fallback archive has a second initial attempt",
                        ))
                    }
                }
            }
            previous = Some(request);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        bytes(self)
    }

    /// The byte limit is checked before parsing. Unknown fields and alternate
    /// encodings refuse; callers still verify worker/object correspondence.
    pub fn from_canonical_bytes(raw: &[u8]) -> Result<Self, ProtocolError> {
        check(
            !raw.is_empty() && raw.len() <= MAX_INFERENCE_FALLBACK_HISTORY_BYTES,
            "fallback archive input exceeds byte admission",
        )?;
        let history: Self =
            serde_json::from_slice(raw).map_err(|e| ProtocolError::invalid(e.to_string()))?;
        check(
            history.canonical_bytes()? == raw,
            "fallback archive encoding is not canonical",
        )?;
        Ok(history)
    }
}
