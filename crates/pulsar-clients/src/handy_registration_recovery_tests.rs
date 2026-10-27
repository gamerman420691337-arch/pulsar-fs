use super::*;
use std::os::unix::fs::PermissionsExt;
struct Api<'a> {
    directory: &'a Path,
    session: SessionId,
    binding: String,
    calls: Vec<(RequestId, Command)>,
    reply: Option<ResponseBody>,
    fail_ack: bool,
}
impl EngineApi for Api<'_> {
    fn execute(
        &mut self,
        _: Command,
        _: Option<ProjectId>,
        _: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        anyhow::bail!("unexpected read");
    }
    fn execute_identified(
        &mut self,
        id: RequestId,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        ensure!(
            project.is_none() && revision.is_none(),
            "global scope required"
        );
        let entries = load(self.directory, &self.session, &self.binding)?;
        match &command {
            Command::SetHandyRegistration { .. } | Command::DisableHandyRegistration { .. } => {
                ensure!(
                    entries.iter().any(|e| e.id == id
                        && Some(&e.intent) == command.handy_registration_intent().as_ref()),
                    "intent was not durable before dispatch"
                )
            }
            Command::ResolveHandyRegistration {
                original_request_id,
                ..
            } => ensure!(
                entries
                    .iter()
                    .any(|e| e.id == *original_request_id && e.resolution_id.as_ref() == Some(&id)),
                "resolution ID was not durable before dispatch"
            ),
            _ => anyhow::bail!("unexpected command"),
        }
        self.calls.push((id, command));
        let reply = self.reply.take().context("synthetic lost reply");
        if reply.is_ok() && self.fail_ack {
            std::fs::set_permissions(
                self.directory.join("desktop-handy-recovery"),
                std::fs::Permissions::from_mode(0o500),
            )?;
        }
        reply
    }
}
fn setup() -> (
    tempfile::TempDir,
    SessionId,
    String,
    HandyRegistrationIntent,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let proof = dir.path().join("handy-owner.token");
    std::fs::write(&proof, "a".repeat(64)).unwrap();
    std::fs::set_permissions(proof, std::fs::Permissions::from_mode(0o600)).unwrap();
    let intent = HandyRegistrationIntent::Save {
        device_id: DeviceId::new("handy").unwrap(),
        expected_revision: 0,
        settings: HandyRegistrationSettings {
            label: "Handy".into(),
            api_key_env: "API_KEY".into(),
            connection_key_env: "CONNECTION_KEY".into(),
        },
        disabled: false,
    };
    (
        dir,
        SessionId::new("session").unwrap(),
        "b".repeat(64),
        intent,
    )
}
#[test]
fn unknown_intent_is_durable_inert_and_resolves_only_through_original_ids() {
    let (dir, session, binding, intent) = setup();
    let id = RequestId::new("original").unwrap();
    let mut api = Api {
        directory: dir.path(),
        session: session.clone(),
        binding: binding.clone(),
        calls: vec![],
        reply: None,
        fail_ack: false,
    };
    let change = Action::Change {
        id: id.clone(),
        intent: intent.clone(),
    };
    assert!(execute_bound(dir.path(), &mut api, &session, &binding, &change).is_err());
    let restored = load(dir.path(), &session, &binding).unwrap();
    assert_eq!(restored.len(), 1);
    assert!(restored[0].outcome.is_none());
    assert_eq!(api.calls.len(), 1);
    assert!(dismiss(dir.path(), &session, &binding, &id).is_err());
    let resolve = Action::Resolve {
        id: id.clone(),
        intent: intent.clone(),
    };
    assert!(execute_bound(dir.path(), &mut api, &session, &binding, &resolve).is_err());
    let resolution_id = load(dir.path(), &session, &binding).unwrap()[0]
        .resolution_id
        .clone()
        .unwrap();
    api.reply = Some(ResponseBody::HandyRegistrationResolution(Box::new(
        HandyRegistrationResolution {
            request_id: id.clone(),
            intent: intent.clone(),
            outcome: HandyRegistrationOutcome::Withdrawn,
        },
    )));
    execute_bound(dir.path(), &mut api, &session, &binding, &resolve).unwrap();
    assert_eq!(api.calls[1].0, resolution_id);
    assert_eq!(api.calls[2].0, resolution_id);
    assert_eq!(
        serde_json::to_value(&api.calls[1].1).unwrap(),
        serde_json::to_value(&api.calls[2].1).unwrap()
    );
    assert!(matches!(
        load(dir.path(), &session, &binding).unwrap()[0].outcome,
        Some(HandyRegistrationOutcome::Withdrawn)
    ));
    dismiss(dir.path(), &session, &binding, &id).unwrap();
    assert!(load(dir.path(), &session, &binding).unwrap().is_empty());
}
#[test]
fn changed_owner_namespace_or_intent_refuses_before_dispatch() {
    let (dir, session, binding, intent) = setup();
    let id = RequestId::new("original").unwrap();
    let mut api = Api {
        directory: dir.path(),
        session: session.clone(),
        binding: binding.clone(),
        calls: vec![],
        reply: None,
        fail_ack: false,
    };
    assert!(execute_bound(
        dir.path(),
        &mut api,
        &session,
        &binding,
        &Action::Change {
            id: id.clone(),
            intent: intent.clone()
        }
    )
    .is_err());
    assert!(load(dir.path(), &session, &"c".repeat(64)).is_err());
    let mut changed = intent.clone();
    if let HandyRegistrationIntent::Save { settings, .. } = &mut changed {
        settings.label = "changed".into();
    }
    assert!(execute_bound(
        dir.path(),
        &mut api,
        &session,
        &binding,
        &Action::Change {
            id: id.clone(),
            intent: changed
        }
    )
    .is_err());
    assert_eq!(api.calls.len(), 1);
    std::fs::write(dir.path().join("handy-owner.token"), "d".repeat(64)).unwrap();
    assert!(execute_bound(
        dir.path(),
        &mut api,
        &session,
        &binding,
        &Action::Resolve { id, intent }
    )
    .is_err());
    assert_eq!(api.calls.len(), 1);
    let restored = load(dir.path(), &session, &binding).unwrap();
    assert_eq!(restored.len(), 1);
    assert!(restored[0].outcome.is_none());
}

#[test]
fn accepted_reply_with_failed_local_ack_keeps_the_original_retry() {
    let (dir, session, binding, intent) = setup();
    let id = RequestId::new("ack-failure").unwrap();
    let HandyRegistrationIntent::Save {
        device_id,
        settings,
        disabled,
        ..
    } = &intent
    else {
        unreachable!()
    };
    let receipt = HandyRegistrationReceipt {
        request_id: id.clone(),
        registration: HandyRegistration {
            device_id: device_id.clone(),
            revision: 1,
            settings: settings.clone(),
            disabled: *disabled,
            binding_sha256: "c".repeat(64),
        },
    };
    let action = Action::Change {
        id: id.clone(),
        intent: intent.clone(),
    };
    let mut api = Api {
        directory: dir.path(),
        session: session.clone(),
        binding: binding.clone(),
        calls: vec![],
        reply: Some(ResponseBody::HandyRegistrationSaved(Box::new(
            receipt.clone(),
        ))),
        fail_ack: true,
    };
    let failed = execute_bound(dir.path(), &mut api, &session, &binding, &action);
    std::fs::set_permissions(
        dir.path().join("desktop-handy-recovery"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(failed.is_err());
    assert!(load(dir.path(), &session, &binding).unwrap()[0]
        .outcome
        .is_none());
    assert!(dismiss(dir.path(), &session, &binding, &id).is_err());
    api.fail_ack = false;
    api.reply = Some(ResponseBody::HandyRegistrationSaved(Box::new(receipt)));
    execute_bound(dir.path(), &mut api, &session, &binding, &action).unwrap();
    assert_eq!(api.calls.len(), 2);
    assert!(api.calls.iter().all(|(sent, _)| sent == &id));
    assert!(matches!(
        load(dir.path(), &session, &binding).unwrap()[0].outcome,
        Some(HandyRegistrationOutcome::Applied(_))
    ));
}
