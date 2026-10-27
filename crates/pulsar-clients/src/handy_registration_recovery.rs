//! Session-bound local intent. Restoration neither dispatches nor grants authority.
use crate::{
    handy_registration,
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const CAPACITY: usize = 32;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryEntry {
    pub id: RequestId,
    pub intent: HandyRegistrationIntent,
    pub owner_binding: String,
    pub resolution_id: Option<RequestId>,
    pub outcome: Option<HandyRegistrationOutcome>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u16,
    session: SessionId,
    credential_binding: String,
    entries: Vec<RecoveryEntry>,
}
fn transaction<T>(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    action: impl FnOnce(&mut Journal) -> Result<(T, bool)>,
) -> Result<T> {
    validate_sha256(binding)?;
    crate::private_recovery::transaction(
        directory,
        "desktop-handy-recovery",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            entries: vec![],
        },
        |journal| {
            ensure!(
                journal.schema_version == 1
                    && journal.session == *session
                    && journal.credential_binding == binding
                    && journal.entries.len() <= CAPACITY,
                "Handy recovery namespace or layout changed; journal preserved"
            );
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                validate_sha256(&entry.owner_binding)?;
                ensure!(
                    ids.insert(entry.id.clone()),
                    "duplicate Handy recovery identity"
                );
                if let Some(id) = &entry.resolution_id {
                    ensure!(
                        ids.insert(id.clone()),
                        "duplicate Handy resolution identity"
                    );
                }
                if let Some(outcome) = &entry.outcome {
                    HandyRegistrationResolution {
                        request_id: entry.id.clone(),
                        intent: entry.intent.clone(),
                        outcome: outcome.clone(),
                    }
                    .validate()?;
                }
            }
            action(journal)
        },
    )
}
pub fn load(directory: &Path, session: &SessionId, binding: &str) -> Result<Vec<RecoveryEntry>> {
    transaction(directory, session, binding, |journal| {
        Ok((journal.entries.clone(), false))
    })
}
fn prepare(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
    intent: &HandyRegistrationIntent,
    proof: &str,
) -> Result<()> {
    intent.validate()?;
    let owner_binding = format!(
        "{:x}",
        Sha256::digest(format!("pulsar-handy-owner-v1:{proof}"))
    );
    transaction(directory, session, binding, |journal| {
        if let Some(entry) = journal.entries.iter().find(|entry| entry.id == *id) {
            ensure!(
                entry.intent == *intent && entry.owner_binding == owner_binding,
                "Handy recovery intent or owner binding changed; original preserved"
            );
            return Ok(((), false));
        }
        ensure!(
            journal.entries.len() < CAPACITY,
            "Handy recovery is full; dismiss a settled record before new changes"
        );
        ensure!(
            !journal
                .entries
                .iter()
                .any(|e| e.resolution_id.as_ref() == Some(id)),
            "request ID is already a resolution identity"
        );
        journal.entries.push(RecoveryEntry {
            id: id.clone(),
            intent: intent.clone(),
            owner_binding,
            resolution_id: None,
            outcome: None,
        });
        Ok(((), true))
    })
}
fn settle(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    resolution: &HandyRegistrationResolution,
) -> Result<()> {
    resolution.validate()?;
    transaction(directory, session, binding, |journal| {
        let entry = journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == resolution.request_id)
            .context("original Handy intent is absent")?;
        ensure!(
            entry.intent == resolution.intent,
            "Handy settlement changed its intent"
        );
        ensure!(
            entry
                .outcome
                .as_ref()
                .is_none_or(|prior| prior == &resolution.outcome),
            "Handy settlement conflicts with retained outcome"
        );
        entry.outcome = Some(resolution.outcome.clone());
        Ok(((), true))
    })
}
pub fn dismiss(directory: &Path, session: &SessionId, binding: &str, id: &RequestId) -> Result<()> {
    transaction(directory, session, binding, |journal| {
        let index = journal
            .entries
            .iter()
            .position(|entry| entry.id == *id)
            .context("Handy recovery entry is absent")?;
        ensure!(
            journal.entries[index].outcome.is_some(),
            "unsettled Handy recovery cannot be dismissed"
        );
        journal.entries.remove(index);
        Ok(((), true))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Change {
        id: RequestId,
        intent: HandyRegistrationIntent,
    },
    Resolve {
        id: RequestId,
        intent: HandyRegistrationIntent,
    },
    Dismiss(RequestId),
    Inspect(HandyRegistration),
}
pub fn execute(
    directory: &Path,
    client: &mut SessionClient,
    action: &Action,
) -> Result<ResponseBody> {
    let session = client.session_id().clone();
    let binding = client.recovery_credential_binding();
    execute_bound(directory, client, &session, &binding, action)
}
fn execute_bound(
    directory: &Path,
    client: &mut impl EngineApi,
    session: &SessionId,
    binding: &str,
    action: &Action,
) -> Result<ResponseBody> {
    match action {
        Action::Refresh => handy_registration::list(client, directory)
            .map(|c| ResponseBody::HandyRegistrations(Box::new(c))),
        Action::Inspect(registration) => {
            handy_registration::inspect(client, directory, registration, true)
                .map(|r| ResponseBody::HandyRegisteredInspection(Box::new(r)))
        }
        Action::Dismiss(id) => {
            dismiss(directory, session, binding, id)?;
            Ok(ResponseBody::Ack)
        }
        Action::Change { id, intent } | Action::Resolve { id, intent } => {
            intent.validate()?;
            let proof =
                crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
            prepare(directory, session, binding, id, intent, &proof)?;
            if matches!(action, Action::Change { .. }) {
                let receipt =
                    handy_registration::change_with_proof(client, id.clone(), intent, proof)?;
                settle(
                    directory,
                    session,
                    binding,
                    &HandyRegistrationResolution {
                        request_id: id.clone(),
                        intent: intent.clone(),
                        outcome: HandyRegistrationOutcome::Applied(Box::new(receipt.clone())),
                    },
                )?;
                Ok(ResponseBody::HandyRegistrationSaved(Box::new(receipt)))
            } else {
                let resolution_id = transaction(directory, session, binding, |journal| {
                    let index = journal
                        .entries
                        .iter()
                        .position(|entry| entry.id == *id)
                        .context("Handy recovery entry is absent")?;
                    if let Some(id) = &journal.entries[index].resolution_id {
                        return Ok((id.clone(), false));
                    }
                    let fresh = RequestId::new(uuid::Uuid::new_v4().to_string())?;
                    ensure!(
                        !journal.entries.iter().any(|entry| entry.id == fresh
                            || entry.resolution_id.as_ref() == Some(&fresh)),
                        "Handy resolution identity collision"
                    );
                    journal.entries[index].resolution_id = Some(fresh.clone());
                    Ok((fresh, true))
                })?;
                let resolution = handy_registration::resolve_with_proof(
                    client,
                    resolution_id,
                    id.clone(),
                    intent,
                    proof,
                )?;
                settle(directory, session, binding, &resolution)?;
                Ok(ResponseBody::HandyRegistrationResolution(Box::new(
                    resolution,
                )))
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "handy_registration_recovery_tests.rs"]
mod tests;
