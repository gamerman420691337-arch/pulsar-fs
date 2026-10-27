//! Explicit boot-bound connection control. No motion or default network consent.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;
fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
pub fn list(client: &mut impl EngineApi, directory: &Path) -> Result<HandyConnectionCatalog> {
    let ResponseBody::HandySessions(catalog) = client.execute(
        Command::HandySessions {
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("Handy connection catalog response mismatch");
    };
    catalog.validate()?;
    Ok(*catalog)
}
pub struct OpenConnection {
    pub epoch: DeviceSessionId,
    pub device: DeviceId,
    pub revision: u64,
    pub binding_sha256: String,
}
pub fn open(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    binding: &OpenConnection,
    consent: bool,
) -> Result<HandyConnectionSession> {
    validate_sha256(&binding.binding_sha256)?;
    ensure!(
        consent && binding.revision > 0 && binding.revision <= i64::MAX as u64,
        "Opening requires explicit inspection consent and a saved registration revision"
    );
    let ResponseBody::HandySession(report) = client.execute_identified(
        id.clone(),
        Command::OpenHandySession {
            engine_epoch: binding.epoch.clone(),
            device_id: binding.device.clone(),
            expected_registration_revision: binding.revision,
            binding_sha256: binding.binding_sha256.clone(),
            allow_online_inspection: true,
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("Handy open response mismatch");
    };
    report.validate()?;
    ensure!(
        report.engine_epoch == binding.epoch
            && report.open_request_id == id
            && report.registration.device_id == binding.device
            && report.registration.revision == binding.revision
            && report.registration.binding_sha256 == binding.binding_sha256,
        "Handy open response changed captured intent"
    );
    Ok(*report)
}
pub fn close(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    epoch: DeviceSessionId,
    device_session: DeviceSessionId,
) -> Result<HandyConnectionSession> {
    let ResponseBody::HandySession(report) = client.execute_identified(
        id,
        Command::CloseHandySession {
            engine_epoch: epoch.clone(),
            device_session: device_session.clone(),
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("Handy close response mismatch");
    };
    report.validate()?;
    ensure!(
        report.engine_epoch == epoch
            && report.device_session == device_session
            && report.state == HandyConnectionState::Retired,
        "Handy close response mismatch"
    );
    Ok(*report)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        reply: Option<ResponseBody>,
        calls: Vec<(RequestId, Command)>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unexpected uncaptured request")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            ensure!(p.is_none() && r.is_none(), "scope");
            self.calls.push((id, command));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("lost reply"))
        }
    }
    fn binding() -> OpenConnection {
        OpenConnection {
            epoch: DeviceSessionId::new("boot").unwrap(),
            device: DeviceId::new("device").unwrap(),
            revision: 1,
            binding_sha256: "a".repeat(64),
        }
    }
    fn report() -> HandyConnectionSession {
        HandyConnectionSession {
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("host-session").unwrap(),
            open_request_id: RequestId::new("open").unwrap(),
            registration: HandyRegistration {
                device_id: DeviceId::new("device").unwrap(),
                revision: 1,
                binding_sha256: "a".repeat(64),
                settings: HandyRegistrationSettings {
                    label: "saved".into(),
                    api_key_env: "HANDY_API".into(),
                    connection_key_env: "HANDY_CONNECTION".into(),
                },
                disabled: false,
            },
            state: HandyConnectionState::Retired,
            inspection: None,
        }
    }
    fn owner() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let p = d.path().join("handy-owner.token");
        std::fs::write(&p, "b".repeat(64)).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600)).unwrap();
        d
    }
    #[test]
    fn handy_session_client_retries_exact_pins_without_current_catalog() {
        let d = owner();
        let mut api = Api {
            reply: None,
            calls: vec![],
        };
        let id = RequestId::new("open").unwrap();
        let b = binding();
        assert!(open(&mut api, d.path(), id.clone(), &b, true).is_err());
        api.reply = Some(ResponseBody::HandySession(Box::new(report())));
        assert_eq!(
            open(&mut api, d.path(), id, &b, true).unwrap().state,
            HandyConnectionState::Retired
        );
        assert_eq!(api.calls[0].0, api.calls[1].0);
        assert_eq!(
            serde_json::to_value(&api.calls[0].1).unwrap(),
            serde_json::to_value(&api.calls[1].1).unwrap()
        );
        let before = api.calls.len();
        assert!(open(
            &mut api,
            d.path(),
            RequestId::new("denied").unwrap(),
            &b,
            false
        )
        .is_err());
        assert_eq!(api.calls.len(), before);
    }
    #[test]
    fn handy_session_client_refuses_wrong_epoch_and_unsettled_close() {
        let d = owner();
        let mut wrong = report();
        wrong.engine_epoch = DeviceSessionId::new("wrong-boot").unwrap();
        let mut api = Api {
            reply: Some(ResponseBody::HandySession(Box::new(wrong))),
            calls: vec![],
        };
        assert!(open(
            &mut api,
            d.path(),
            RequestId::new("open").unwrap(),
            &binding(),
            true
        )
        .is_err());
        let mut pending = report();
        pending.state = HandyConnectionState::Opening;
        api.reply = Some(ResponseBody::HandySession(Box::new(pending)));
        assert!(close(
            &mut api,
            d.path(),
            RequestId::new("close").unwrap(),
            binding().epoch,
            DeviceSessionId::new("host-session").unwrap()
        )
        .is_err());
    }
}
