//! Inert session-bound recovery facts. No credentials, executable bytes or approval are stored.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
const ENTRIES: usize = 64;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub request_id: RequestId,
    pub manifest: ExtensionManifest,
    pub worker: ArtifactIdentity,
    pub guest_memory_bytes: u64,
    pub label: String,
}
impl Intent {
    pub(crate) fn capture(
        project: ProjectId,
        revision: RevisionId,
        id: RequestId,
        spec: &WasmEditSpec,
    ) -> Result<Self> {
        spec.validate()?;
        ensure!(
            spec.manifest.artifact.sha256 == format!("{:x}", Sha256::digest(&spec.module)),
            "Recovery module bytes differ from declared identity"
        );
        let intent = Self {
            project,
            revision,
            request_id: id,
            manifest: spec.manifest.clone(),
            worker: spec.expected_worker.clone(),
            guest_memory_bytes: spec.guest_memory_bytes,
            label: spec.label.clone(),
        };
        intent.validate()?;
        Ok(intent)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.revision.value() <= i64::MAX as u64 && self.manifest.role == ExtensionRole::Editor,
            "Invalid recovery revision or editor role"
        );
        self.worker.validate()?;
        ensure!(
            !self.label.trim().is_empty()
                && self.label.len() <= 256
                && !self.label.chars().any(char::is_control),
            "Invalid recovery label"
        );
        ensure!(
            (1..=WASM_MODULE_MAX_BYTES as u64).contains(&self.manifest.artifact.byte_len),
            "Recovery module exceeds byte admission"
        );
        // Only exercise the shared shape/budget contract. These dummy bytes are
        // never stored, compiled, dispatched or accepted as the actual module.
        WasmProcessingRequest {
            manifest: self.manifest.clone(),
            module: vec![0; self.manifest.artifact.byte_len as usize],
            input: vec![],
            guest_memory_bytes: self.guest_memory_bytes,
        }
        .validate()?;
        ensure!(
            serde_json::to_vec(self)?.len() <= 16 * 1024,
            "Recovery metadata exceeds byte admission"
        );
        Ok(())
    }
    pub(crate) fn matches(&self, other: &Self) -> Result<bool> {
        Ok(serde_json::to_vec(self)? == serde_json::to_vec(other)?)
    }
    pub(crate) fn reference(&self) -> Result<WasmStartReference> {
        self.validate()?;
        Ok(WasmStartReference {
            original_request_id: self.request_id.clone(),
            base_revision: self.revision,
            manifest: self.manifest.clone(),
            worker: self.worker.clone(),
            guest_memory_bytes: self.guest_memory_bytes,
            label: self.label.clone(),
        })
    }
    fn check_report(&self, report: &WasmEditReport) -> Result<()> {
        report.validate()?;
        ensure!(
            report.project_id == self.project
                && report.base_revision == self.revision
                && report.request_id == self.request_id,
            "Recovery acknowledgement scope differs"
        );
        if let Some(receipt) = &report.receipt {
            ensure!(
                receipt.module == self.manifest.artifact && receipt.worker == self.worker,
                "Recovery acknowledgement artifact identity differs"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub intent: Intent,
    pub attempt: Option<AttemptId>,
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
    ensure!(endpoint.is_absolute(), "Recovery endpoint must be absolute");
    let endpoint_binding = format!("{:x}", Sha256::digest(serde_json::to_vec(endpoint)?));
    crate::private_recovery::transaction(
        directory,
        "desktop-wasm-recovery",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            endpoint_binding: endpoint_binding.clone(),
            entries: vec![],
        },
        |journal| {
            ensure!(journal.schema_version==1 && journal.session==*session && journal.credential_binding==binding && journal.endpoint_binding==endpoint_binding && journal.entries.len()<=ENTRIES,"Recovery layout, session, endpoint or credential binding changed; original preserved");
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                ensure!(
                    ids.insert(entry.intent.request_id.clone()),
                    "Duplicate recovery request"
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
    intent: &Intent,
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
                "Recovery request intent changed; original preserved"
            );
            return Ok(((), false));
        }
        ensure!(
            j.entries.len() < ENTRIES,
            "WASM recovery journal is full; dismiss acknowledged records before new invocations"
        );
        j.entries.push(Entry {
            intent: intent.clone(),
            attempt: None,
        });
        Ok(((), true))
    })
}
pub(crate) fn acknowledge(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: &Intent,
    report: &WasmEditReport,
) -> Result<()> {
    intent.check_report(report)?;
    transaction(directory, endpoint, session, binding, |j| {
        let entry = j
            .entries
            .iter_mut()
            .find(|e| e.intent.request_id == intent.request_id)
            .context("Recovery entry absent")?;
        ensure!(
            entry.intent.matches(intent)?,
            "Recovery acknowledgement intent changed"
        );
        if let Some(attempt) = &entry.attempt {
            ensure!(
                attempt == &report.attempt_id,
                "Recovery attempt identity changed"
            );
            return Ok(((), false));
        }
        entry.attempt = Some(report.attempt_id.clone());
        Ok(((), true))
    })
}
pub(crate) fn reconcile(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    catalog: &WasmEditCatalog,
) -> Result<()> {
    catalog.validate()?;
    transaction(directory, endpoint, session, binding, |j| {
        let mut changed = false;
        for entry in &mut j.entries {
            if entry.intent.project != catalog.project_id {
                continue;
            }
            if let Some(report) = catalog
                .operations
                .iter()
                .find(|r| r.request_id == entry.intent.request_id)
            {
                entry.intent.check_report(report)?;
                if let Some(attempt) = &entry.attempt {
                    ensure!(
                        attempt == &report.attempt_id,
                        "Recovered attempt identity changed"
                    );
                } else {
                    entry.attempt = Some(report.attempt_id.clone());
                    changed = true;
                }
            }
        }
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
            .position(|e| e.intent.request_id == *id)
            .context("Recovery entry absent")?;
        ensure!(
            j.entries[index].attempt.is_some(),
            "Unknown invocation cannot be dismissed; reconcile it first"
        );
        j.entries.remove(index);
        Ok(((), true))
    })
}

/// An authenticated withdrawal acknowledgement permits deleting only exact unpublished facts.
pub(crate) fn withdraw(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    intent: &Intent,
    resolution: &WasmStartResolution,
) -> Result<()> {
    let reference = intent.reference()?;
    let request = Request::new(
        reference.resolution_request_id(&intent.project)?,
        Command::ResolveWasmStart { reference },
    )
    .in_project(intent.project.clone(), None);
    resolution.validate_for(&request)?;
    ensure!(
        matches!(resolution.outcome, WasmStartOutcome::Withdrawn),
        "Accepted work cannot be discarded as unpublished"
    );
    transaction(directory, endpoint, session, binding, |j| {
        let Some(index) = j
            .entries
            .iter()
            .position(|e| e.intent.request_id == intent.request_id)
        else {
            return Ok(((), false));
        };
        ensure!(
            j.entries[index].intent.matches(intent)? && j.entries[index].attempt.is_none(),
            "Withdrawal differs from unresolved original facts"
        );
        j.entries.remove(index);
        Ok(((), true))
    })
}

#[cfg(all(test, unix))]
#[path = "wasm_recovery_tests.rs"]
mod tests;
