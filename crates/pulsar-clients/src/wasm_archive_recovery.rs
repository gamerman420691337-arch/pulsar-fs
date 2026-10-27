//! Private archive retry facts. Stored intent never grants authority or dispatches itself.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveIntent {
    pub(crate) project: ProjectId,
    pub(crate) request_id: RequestId,
    pub(crate) attempt: AttemptId,
    pub(crate) original_request: RequestId,
    pub(crate) observed_revision: RevisionId,
    pub(crate) observed_state: WasmEditState,
    pub(crate) candidate: CandidateId,
    pub(crate) receipt: WasmEditReceipt,
}
impl ArchiveIntent {
    pub(crate) fn capture(
        binding: &(ProjectId, RevisionId),
        report: &WasmEditReport,
    ) -> Result<Self> {
        report.validate()?;
        ensure!(
            report.project_id == binding.0 && report.state == WasmEditState::Completed,
            "Only an observed completed attempt can be archived"
        );
        Ok(Self {
            project: binding.0.clone(),
            request_id: crate::session::new_request_id(),
            attempt: report.attempt_id.clone(),
            original_request: report.request_id.clone(),
            observed_revision: report.base_revision,
            observed_state: report.state,
            candidate: report
                .candidate_id
                .clone()
                .expect("validated completed candidate"),
            receipt: report.receipt.clone().expect("validated completed receipt"),
        })
    }
}

impl ArchiveIntent {
    pub(crate) fn matches(&self, other: &Self) -> Result<bool> {
        Ok(serde_json::to_vec(self)? == serde_json::to_vec(other)?)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        ensure!(
            self.observed_revision.value() <= i64::MAX as u64
                && self.observed_state == WasmEditState::Completed,
            "Invalid archive observation"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 16 * 1024,
            "Archive metadata exceeds byte admission"
        );
        Ok(())
    }
    pub(crate) fn check_report(&self, report: &WasmEditReport) -> Result<()> {
        let request = Request::new(
            self.request_id.clone(),
            Command::ArchiveCompletedWasmEdit {
                attempt_id: self.attempt.clone(),
                original_request_id: self.original_request.clone(),
            },
        )
        .in_project(self.project.clone(), None);
        report.validate_archive_for(&request)?;
        ensure!(
            report.candidate_id.as_ref() == Some(&self.candidate)
                && serde_json::to_vec(&report.receipt)?
                    == serde_json::to_vec(&Some(&self.receipt))?,
            "Archive acknowledgement custody identity differs"
        );
        ensure!(
            report.base_revision == self.observed_revision,
            "Archive acknowledgement revision differs"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub intent: ArchiveIntent,
    pub acknowledged: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u16,
    session: SessionId,
    credential_binding: String,
    endpoint_binding: String,
    entries: Vec<Entry>,
}
fn transaction<T>(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    operation: impl FnOnce(&mut Journal) -> Result<(T, bool)>,
) -> Result<T> {
    validate_sha256(binding)?;
    ensure!(endpoint.is_absolute(), "Archive endpoint must be absolute");
    let endpoint_binding = format!("{:x}", Sha256::digest(serde_json::to_vec(endpoint)?));
    crate::private_recovery::transaction(
        directory,
        "desktop-wasm-archive",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            endpoint_binding: endpoint_binding.clone(),
            entries: vec![],
        },
        |journal| {
            ensure!(journal.schema_version==1 && journal.session==*session && journal.credential_binding==binding && journal.endpoint_binding==endpoint_binding && journal.entries.len()<=64,"Archive layout, session, endpoint or credential binding changed; original preserved");
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                ensure!(
                    ids.insert(entry.intent.request_id.clone()),
                    "Duplicate archive request"
                );
            }
            operation(journal)
        },
    )
}
pub(crate) fn load(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
) -> Result<Vec<Entry>> {
    transaction(directory, endpoint, session, binding, |j| {
        Ok((j.entries.clone(), false))
    })
}
pub(crate) fn prepare(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: &ArchiveIntent,
) -> Result<()> {
    intent.validate()?;
    transaction(directory, endpoint, session, binding, |j| {
        if let Some(entry) = j
            .entries
            .iter()
            .find(|e| e.intent.request_id == intent.request_id)
        {
            ensure!(
                entry.intent.matches(intent)?,
                "Archive intent changed; original preserved"
            );
            return Ok(((), false));
        }
        ensure!(
            !j.entries.iter().any(|e| !e.acknowledged
                && e.intent.project == intent.project
                && e.intent.attempt == intent.attempt),
            "Archive for this attempt is unresolved; retry its original request"
        );
        ensure!(
            j.entries.len() < 64,
            "Archive journal is full; dismiss acknowledged records before new archive"
        );
        j.entries.push(Entry {
            intent: intent.clone(),
            acknowledged: false,
        });
        Ok(((), true))
    })
}
pub(crate) fn acknowledge(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: &ArchiveIntent,
    report: &WasmEditReport,
) -> Result<()> {
    intent.check_report(report)?;
    transaction(directory, endpoint, session, binding, |j| {
        let entry = j
            .entries
            .iter_mut()
            .find(|e| e.intent.request_id == intent.request_id)
            .context("Archive entry absent")?;
        ensure!(
            entry.intent.matches(intent)?,
            "Archive acknowledgement intent changed"
        );
        let changed = !entry.acknowledged;
        entry.acknowledged = true;
        Ok(((), changed))
    })
}
pub(crate) fn dismiss(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
) -> Result<()> {
    transaction(directory, endpoint, session, binding, |j| {
        let index = j
            .entries
            .iter()
            .position(|e| &e.intent.request_id == id)
            .context("Archive entry absent")?;
        ensure!(
            j.entries[index].acknowledged,
            "Unknown archive cannot be dismissed; explicitly retry its original request"
        );
        j.entries.remove(index);
        Ok(((), true))
    })
}
#[cfg(all(test, unix))]
#[path = "wasm_archive_recovery_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn authored_report() -> WasmEditReport {
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("module").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 8,
    };
    let program = ProgramDescriptor {
        artifact_id: identity.artifact_id.clone(),
        sha256: identity.sha256.clone(),
        byte_len: 8,
        codec: MotionCodec::MotionProgramJsonV1,
        axes: vec![],
    };
    WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: RequestId::new("start").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        base_revision: RevisionId::new(0),
        engine_epoch: "epoch".into(),
        state: WasmEditState::Completed,
        candidate_id: Some(CandidateId::new("candidate").unwrap()),
        error: None,
        receipt: Some(WasmEditReceipt {
            schema_version: PROTOCOL_VERSION,
            request_sha256: "a".repeat(64),
            module: identity.clone(),
            worker: identity.clone(),
            base_program: program.clone(),
            input_sha256: "a".repeat(64),
            input_byte_len: 8,
            output_sha256: "a".repeat(64),
            output_byte_len: 8,
            result_program: program,
            edit_receipt: identity,
            fuel_consumed: 1,
            worker_peak_memory_bytes: 65536,
        }),
    }
}
