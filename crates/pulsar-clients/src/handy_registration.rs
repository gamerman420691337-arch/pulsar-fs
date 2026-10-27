//! Shared CLI/desktop registration requests. No credential values or playback grants.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::path::Path;

pub use pulsar_protocol::HandyRegistrationIntent as RegistrationChange;
fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
pub fn change(
    client: &mut impl EngineApi,
    directory: &Path,
    request_id: RequestId,
    change: &RegistrationChange,
) -> Result<HandyRegistrationReceipt> {
    change.validate()?;
    change_with_proof(client, request_id, change, proof(directory)?)
}
pub(crate) fn change_with_proof(
    client: &mut impl EngineApi,
    request_id: RequestId,
    change: &RegistrationChange,
    owner_proof: String,
) -> Result<HandyRegistrationReceipt> {
    change.validate()?;
    let command = change.command(owner_proof);
    let response=client.execute_identified(request_id.clone(),command,None,None)
        .with_context(||format!("Handy registration request {request_id}; retry only this original ID and captured settings"))?;
    let ResponseBody::HandyRegistrationSaved(receipt) = response else {
        anyhow::bail!("Handy registration returned an unexpected response");
    };
    receipt.validate()?;
    ensure!(
        receipt.request_id == request_id
            && &receipt.registration.device_id == change.device()
            && receipt.registration.revision == change.expected_revision() + 1,
        "Handy registration receipt correspondence mismatch"
    );
    match change {
        RegistrationChange::Save {
            settings, disabled, ..
        } => ensure!(
            receipt.registration.settings == *settings
                && receipt.registration.disabled == *disabled,
            "Handy registration receipt changed the captured configuration"
        ),
        RegistrationChange::Disable { .. } => ensure!(
            receipt.registration.disabled,
            "Handy disable receipt did not disable lookup"
        ),
    }
    Ok(*receipt)
}
pub fn list(client: &mut impl EngineApi, directory: &Path) -> Result<HandyRegistrationCatalog> {
    let ResponseBody::HandyRegistrations(catalog) = client.execute(
        Command::ListHandyRegistrations {
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("Handy registration catalog response mismatch");
    };
    catalog.validate()?;
    Ok(*catalog)
}
pub fn inspect(
    client: &mut impl EngineApi,
    directory: &Path,
    registration: &HandyRegistration,
    consent: bool,
) -> Result<HandyRegisteredInspection> {
    registration.validate()?;
    ensure!(consent, "Handy online inspection requires explicit consent");
    ensure!(!registration.disabled, "Handy registration is disabled");
    let response = client.execute(
        Command::InspectHandyRegistration {
            device_id: registration.device_id.clone(),
            expected_registration_revision: registration.revision,
            binding_sha256: registration.binding_sha256.clone(),
            allow_online_inspection: true,
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?;
    let ResponseBody::HandyRegisteredInspection(report) = response else {
        anyhow::bail!("Handy registered inspection response mismatch");
    };
    report.validate()?;
    ensure!(
        &report.registration == registration,
        "Handy registered inspection changed its selected binding"
    );
    Ok(*report)
}

pub fn resolve(
    client: &mut impl EngineApi,
    directory: &Path,
    resolution_id: RequestId,
    original_id: RequestId,
    intent: &RegistrationChange,
) -> Result<HandyRegistrationResolution> {
    intent.validate()?;
    ensure!(
        resolution_id != original_id,
        "resolution needs a distinct request ID"
    );
    resolve_with_proof(
        client,
        resolution_id,
        original_id,
        intent,
        proof(directory)?,
    )
}
pub(crate) fn resolve_with_proof(
    client: &mut impl EngineApi,
    resolution_id: RequestId,
    original_id: RequestId,
    intent: &RegistrationChange,
    owner_proof: String,
) -> Result<HandyRegistrationResolution> {
    intent.validate()?;
    ensure!(
        resolution_id != original_id,
        "resolution needs a distinct request ID"
    );
    let command = Command::ResolveHandyRegistration {
        original_request_id: original_id.clone(),
        intent: intent.clone(),
        owner_proof,
    };
    let response = client.execute_identified(resolution_id.clone(), command, None, None)
        .with_context(|| format!("Handy resolution {resolution_id} for {original_id}; retain both original identities"))?;
    let ResponseBody::HandyRegistrationResolution(resolution) = response else {
        anyhow::bail!("Handy resolution response mismatch");
    };
    resolution.validate()?;
    ensure!(
        resolution.request_id == original_id && resolution.intent == *intent,
        "Handy resolution changed the original intent"
    );
    Ok(*resolution)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        calls: Vec<(Option<RequestId>, Command)>,
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(project.is_none() && revision.is_none());
            self.calls.push((None, command));
            self.reply.take().context("synthetic lost reply")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            assert!(project.is_none() && revision.is_none());
            self.calls.push((Some(id), command));
            self.reply.take().context("synthetic lost reply")
        }
    }
    fn owner() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let p = d.path().join("handy-owner.token");
        std::fs::write(&p, "a".repeat(64)).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
        d
    }
    fn registration() -> HandyRegistration {
        HandyRegistration {
            device_id: DeviceId::new("handy").unwrap(),
            revision: 1,
            settings: HandyRegistrationSettings {
                label: "Handy".into(),
                api_key_env: "HANDY_API_KEY".into(),
                connection_key_env: "HANDY_CONNECTION_KEY".into(),
            },
            disabled: false,
            binding_sha256: "b".repeat(64),
        }
    }
    #[test]
    fn lost_reply_retains_original_identity_and_mismatched_receipt_refuses() {
        let dir = owner();
        let record = registration();
        let id = RequestId::new("original").unwrap();
        let change = RegistrationChange::Save {
            device_id: record.device_id.clone(),
            expected_revision: 0,
            settings: record.settings.clone(),
            disabled: false,
        };
        let mut api = Api {
            calls: vec![],
            reply: None,
        };
        assert!(super::change(&mut api, dir.path(), id.clone(), &change)
            .unwrap_err()
            .to_string()
            .contains("original"));
        api.reply = Some(ResponseBody::HandyRegistrationSaved(Box::new(
            HandyRegistrationReceipt {
                request_id: id.clone(),
                registration: record.clone(),
            },
        )));
        super::change(&mut api, dir.path(), id.clone(), &change).unwrap();
        assert_eq!(api.calls.len(), 2);
        assert!(api.calls.iter().all(|(sent, _)| sent.as_ref() == Some(&id)));
        assert_eq!(
            serde_json::to_value(&api.calls[0].1).unwrap(),
            serde_json::to_value(&api.calls[1].1).unwrap()
        );
        let mut bad = record;
        bad.disabled = true;
        api.reply = Some(ResponseBody::HandyRegistrationSaved(Box::new(
            HandyRegistrationReceipt {
                request_id: id.clone(),
                registration: bad,
            },
        )));
        assert!(super::change(&mut api, dir.path(), id, &change).is_err());
    }
    #[test]
    fn inspection_requires_explicit_consent_and_exact_current_selection() {
        let dir = owner();
        let mut record = registration();
        let mut api = Api {
            calls: vec![],
            reply: None,
        };
        assert!(inspect(&mut api, dir.path(), &record, false).is_err());
        assert!(api.calls.is_empty());
        record.disabled = true;
        assert!(inspect(&mut api, dir.path(), &record, true).is_err());
        assert!(api.calls.is_empty());
        record.disabled = false;
        let mut changed = record.clone();
        changed.binding_sha256 = "c".repeat(64);
        api.reply = Some(ResponseBody::HandyRegisteredInspection(Box::new(
            HandyRegisteredInspection {
                registration: changed,
                inspection: HandyInspection {
                    connected: false,
                    identity: None,
                    observed_at_unix_ms: 1,
                },
            },
        )));
        assert!(inspect(&mut api, dir.path(), &record, true).is_err());
        let (
            _,
            Command::InspectHandyRegistration {
                binding_sha256,
                expected_registration_revision,
                ..
            },
        ) = &api.calls[0]
        else {
            panic!("bound inspection");
        };
        assert_eq!(binding_sha256, &record.binding_sha256);
        assert_eq!(*expected_registration_revision, 1);
    }
}
