//! Private cleanup retry facts. Stored intent never grants authority or dispatches itself.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RemovalIntent {
    pub(crate) project: ProjectId,
    pub(crate) request_id: RequestId,
    pub(crate) attempt: AttemptId,
    pub(crate) original_request: RequestId,
    pub(crate) observed_revision: RevisionId,
    pub(crate) observed_state: WasmEditState,
}
impl RemovalIntent {
    pub(crate) fn capture(
        binding: &(ProjectId, RevisionId),
        report: &WasmEditReport,
    ) -> Result<Self> {
        report.validate()?;
        ensure!(
            report.project_id == binding.0 && report.state.is_unsuccessful_terminal(),
            "Only an observed unsuccessful terminal attempt can be removed"
        );
        Ok(Self {
            project: binding.0.clone(),
            request_id: crate::session::new_request_id(),
            attempt: report.attempt_id.clone(),
            original_request: report.request_id.clone(),
            observed_revision: report.base_revision,
            observed_state: report.state,
        })
    }
}

impl RemovalIntent {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.observed_revision.value() <= i64::MAX as u64
                && self.observed_state.is_unsuccessful_terminal(),
            "Invalid cleanup observation"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 16 * 1024,
            "Cleanup metadata exceeds byte admission"
        );
        Ok(())
    }
    pub(crate) fn check_report(&self, report: &WasmEditReport) -> Result<()> {
        let request = Request::new(
            self.request_id.clone(),
            Command::RemoveUnsuccessfulWasmEdit {
                attempt_id: self.attempt.clone(),
                original_request_id: self.original_request.clone(),
                expected_state: self.observed_state,
            },
        )
        .in_project(self.project.clone(), None);
        report.validate_removal_for(&request)?;
        ensure!(
            report.base_revision == self.observed_revision,
            "Cleanup acknowledgement revision differs"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub intent: RemovalIntent,
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
    ensure!(endpoint.is_absolute(), "Cleanup endpoint must be absolute");
    let endpoint_binding = format!("{:x}", Sha256::digest(serde_json::to_vec(endpoint)?));
    crate::private_recovery::transaction(
        directory,
        "desktop-wasm-cleanup",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            endpoint_binding: endpoint_binding.clone(),
            entries: vec![],
        },
        |journal| {
            ensure!(journal.schema_version==1 && journal.session==*session && journal.credential_binding==binding && journal.endpoint_binding==endpoint_binding && journal.entries.len()<=64,"Cleanup layout, session, endpoint or credential binding changed; original preserved");
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                ensure!(
                    ids.insert(entry.intent.request_id.clone()),
                    "Duplicate cleanup request"
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
    intent: &RemovalIntent,
) -> Result<()> {
    intent.validate()?;
    transaction(directory, endpoint, session, binding, |j| {
        if let Some(entry) = j
            .entries
            .iter()
            .find(|e| e.intent.request_id == intent.request_id)
        {
            ensure!(
                &entry.intent == intent,
                "Cleanup intent changed; original preserved"
            );
            return Ok(((), false));
        }
        ensure!(
            !j.entries.iter().any(|e| !e.acknowledged
                && e.intent.project == intent.project
                && e.intent.attempt == intent.attempt),
            "Cleanup for this attempt is unresolved; retry its original request"
        );
        ensure!(
            j.entries.len() < 64,
            "Cleanup journal is full; dismiss acknowledged records before new cleanup"
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
    intent: &RemovalIntent,
    report: &WasmEditReport,
) -> Result<()> {
    intent.check_report(report)?;
    transaction(directory, endpoint, session, binding, |j| {
        let entry = j
            .entries
            .iter_mut()
            .find(|e| e.intent.request_id == intent.request_id)
            .context("Cleanup entry absent")?;
        ensure!(
            &entry.intent == intent,
            "Cleanup acknowledgement intent changed"
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
            .context("Cleanup entry absent")?;
        ensure!(
            j.entries[index].acknowledged,
            "Unknown cleanup cannot be dismissed; explicitly retry its original request"
        );
        j.entries.remove(index);
        Ok(((), true))
    })
}
#[cfg(all(test, unix))]
#[path = "wasm_cleanup_tests.rs"]
mod tests;
