//! Inert Start identities. This journal never grants authority or launches work.
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const CAPACITY: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub request_id: RequestId,
    pub engine_epoch: AttemptId,
    pub options: NativeImagePreparationOptions,
}
impl Intent {
    fn validate(&self) -> Result<()> {
        self.options.validate()?;
        ensure!(
            self.revision.value() < i64::MAX as u64,
            "Recovery revision exceeds admission"
        );
        Ok(())
    }
    fn check(&self, session: &SessionId, value: &NativeImagePreparationStatus) -> Result<()> {
        value.validate()?;
        let actual = &value.report.intent;
        ensure!(
            actual.owner == *session
                && actual.project == self.project
                && actual.revision == self.revision.value()
                && actual.original_request_id == self.request_id
                && actual.engine_epoch == self.engine_epoch
                && actual.options == self.options,
            "Preparation reply conflicts with saved Start identity"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u16,
    session: SessionId,
    credential: String,
    endpoint: String,
    entries: Vec<Intent>,
}
fn transaction<T>(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    credential: &str,
    action: impl FnOnce(&mut Vec<Intent>) -> Result<(T, bool)>,
) -> Result<T> {
    validate_sha256(credential)?;
    ensure!(endpoint.is_absolute(), "Recovery endpoint must be absolute");
    let endpoint = format!("{:x}", Sha256::digest(serde_json::to_vec(endpoint)?));
    crate::private_recovery::transaction(
        directory,
        "native-preparation-recovery",
        session,
        Journal {
            version: 1,
            session: session.clone(),
            credential: credential.into(),
            endpoint: endpoint.clone(),
            entries: vec![],
        },
        |journal| {
            ensure!(
                journal.version == 1
                    && journal.session == *session
                    && journal.credential == credential
                    && journal.endpoint == endpoint
                    && journal.entries.len() <= CAPACITY,
                "Recovery binding or layout changed; original preserved"
            );
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.validate()?;
                ensure!(
                    ids.insert(entry.request_id.clone()),
                    "Duplicate recovery request"
                );
            }
            action(&mut journal.entries)
        },
    )
}
pub(crate) fn load(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    credential: &str,
) -> Result<Vec<Intent>> {
    transaction(directory, endpoint, session, credential, |entries| {
        Ok((entries.clone(), false))
    })
}
fn prepare(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    credential: &str,
    intent: Intent,
) -> Result<()> {
    intent.validate()?;
    transaction(directory, endpoint, session, credential, |entries| {
        if let Some(old) = entries.iter().find(|e| e.request_id == intent.request_id) {
            ensure!(
                *old == intent,
                "Start request identity already has different frozen options"
            );
            return Ok(((), false));
        }
        ensure!(
            entries.len() < CAPACITY,
            "Recovery journal is full; dismiss a saved identity before starting"
        );
        entries.push(intent);
        Ok(((), true))
    })
}
pub(crate) fn dismiss(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    credential: &str,
    project: &ProjectId,
    request: &RequestId,
) -> Result<()> {
    transaction(directory, endpoint, session, credential, |entries| {
        let index = entries
            .iter()
            .position(|e| e.project == *project && e.request_id == *request)
            .ok_or_else(|| anyhow::anyhow!("Saved Start identity not found for this project"))?;
        entries.remove(index);
        Ok(((), true))
    })
}
pub(crate) fn execute(
    client: &mut crate::session::SessionClient,
    directory: &Path,
    endpoint: &Path,
    request_id: RequestId,
    command: Command,
    project: ProjectId,
    revision: RevisionId,
) -> Result<crate::motion::ResponseBody> {
    let session = client.session_id().clone();
    let credential = client.recovery_credential_binding();
    execute_inner(
        client,
        (directory, endpoint, &session, &credential),
        request_id,
        command,
        project,
        revision,
    )
}
fn execute_inner(
    client: &mut impl crate::session::EngineApi,
    store: (&Path, &Path, &SessionId, &str),
    request_id: RequestId,
    command: Command,
    project: ProjectId,
    revision: RevisionId,
) -> Result<crate::motion::ResponseBody> {
    use crate::motion::ResponseBody;
    let (directory, endpoint, session, credential) = store;
    // A durable record must exist before any Start can reach the engine.
    if let Command::StartNativeImagePreparation {
        engine_epoch,
        options,
    } = &command
    {
        prepare(
            directory,
            endpoint,
            session,
            credential,
            Intent {
                project: project.clone(),
                revision,
                request_id: request_id.clone(),
                engine_epoch: engine_epoch.clone(),
                options: *options,
            },
        )?;
    }
    let entries = if matches!(
        &command,
        Command::StartNativeImagePreparation { .. } | Command::ResolveNativeImagePreparation { .. }
    ) {
        load(directory, endpoint, session, credential)?
    } else {
        vec![]
    };
    let original = match &command {
        Command::StartNativeImagePreparation { .. } => Some(&request_id),
        Command::ResolveNativeImagePreparation {
            original_request_id,
        } => Some(original_request_id),
        _ => None,
    };
    let expected = original.and_then(|id| entries.iter().find(|e| e.request_id == *id));
    if let Some(expected) = expected {
        ensure!(
            expected.project == project,
            "Saved identity belongs to another project"
        );
    }
    let expected_original = original.cloned();
    let expected_operation = match &command {
        Command::NativeImagePreparationStatus { operation_id }
        | Command::CancelNativeImagePreparation { operation_id }
        | Command::ReleaseNativeImagePreparation { operation_id } => Some(operation_id.clone()),
        _ => None,
    };
    let expects_limits = matches!(&command, Command::NativeImagePreparationLimits);
    let response =
        client.execute_identified(request_id, command, Some(project.clone()), Some(revision))?;
    match &response {
        ResponseBody::NativeImagePreparation(value) if !expects_limits => {
            value.validate()?;
            let actual = &value.report.intent;
            ensure!(
                actual.owner == *session && actual.project == project,
                "Preparation reply belongs to another owner or project"
            );
            if let Some(id) = expected_original {
                ensure!(
                    actual.original_request_id == id,
                    "Preparation reply has a different original request"
                );
            }
            if let Some(id) = expected_operation {
                ensure!(
                    actual.operation_id == id,
                    "Preparation reply has a different operation"
                );
            }
            if let Some(expected) = expected {
                expected.check(session, value)?;
            }
        }
        ResponseBody::NativeImagePreparationLimits(value) if expects_limits => value.validate()?,
        _ => anyhow::bail!("Unexpected native preparation response"),
    }
    Ok(response)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn root() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }
    fn intent(id: &str) -> Intent {
        Intent {
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(0),
            request_id: RequestId::new(id).unwrap(),
            engine_epoch: AttemptId::new("epoch").unwrap(),
            options: NativeImagePreparationOptions {
                preparation_ms: 1000,
                retention_ms: 2000,
            },
        }
    }
    #[test]
    fn lost_reply_survives_reopen_without_starting_work() {
        let dir = root();
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let entry = intent("lost");
        prepare(
            dir.path(),
            Path::new("/engine.sock"),
            &session,
            &credential,
            entry.clone(),
        )
        .unwrap();
        assert_eq!(
            load(dir.path(), Path::new("/engine.sock"), &session, &credential).unwrap(),
            vec![entry]
        );
        assert!(load(dir.path(), Path::new("/other.sock"), &session, &credential).is_err());
        assert!(load(
            dir.path(),
            Path::new("/engine.sock"),
            &session,
            &"b".repeat(64)
        )
        .is_err());
        assert!(load(
            dir.path(),
            Path::new("/engine.sock"),
            &SessionId::new("other").unwrap(),
            &credential
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn frozen_intents_capacity_and_project_bound_dismissal() {
        let dir = root();
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let endpoint = Path::new("/engine.sock");
        let first = intent("request-0");
        prepare(dir.path(), endpoint, &session, &credential, first.clone()).unwrap();
        prepare(dir.path(), endpoint, &session, &credential, first.clone()).unwrap();
        let mut conflict = first.clone();
        conflict.options.retention_ms += 1;
        assert!(prepare(dir.path(), endpoint, &session, &credential, conflict).is_err());
        transaction(dir.path(), endpoint, &session, &credential, |entries| {
            entries.extend((1..CAPACITY).map(|n| intent(&format!("request-{n}"))));
            Ok(((), true))
        })
        .unwrap();
        assert!(prepare(
            dir.path(),
            endpoint,
            &session,
            &credential,
            intent("overflow")
        )
        .is_err());
        assert!(dismiss(
            dir.path(),
            endpoint,
            &session,
            &credential,
            &ProjectId::new("other").unwrap(),
            &first.request_id
        )
        .is_err());
        dismiss(
            dir.path(),
            endpoint,
            &session,
            &credential,
            &first.project,
            &first.request_id,
        )
        .unwrap();
        prepare(
            dir.path(),
            endpoint,
            &session,
            &credential,
            intent("overflow"),
        )
        .unwrap();
        assert_eq!(
            load(dir.path(), endpoint, &session, &credential)
                .unwrap()
                .len(),
            CAPACITY
        );
    }
    struct LostReply {
        calls: usize,
    }
    impl crate::session::EngineApi for LostReply {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<crate::motion::ResponseBody> {
            anyhow::bail!("unidentified request forbidden")
        }
        fn execute_identified(
            &mut self,
            _: RequestId,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<crate::motion::ResponseBody> {
            self.calls += 1;
            anyhow::bail!("simulated lost reply")
        }
    }
    #[test]
    fn durable_before_dispatch_and_refusal_prevents_dispatch() {
        let dir = root();
        let endpoint = Path::new("/engine.sock");
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let entry = intent("lost");
        let mut client = LostReply { calls: 0 };
        let command = Command::StartNativeImagePreparation {
            engine_epoch: entry.engine_epoch.clone(),
            options: entry.options,
        };
        assert!(execute_inner(
            &mut client,
            (dir.path(), endpoint, &session, &credential),
            entry.request_id.clone(),
            command.clone(),
            entry.project.clone(),
            entry.revision
        )
        .is_err());
        assert_eq!(client.calls, 1);
        assert_eq!(
            load(dir.path(), endpoint, &session, &credential).unwrap(),
            vec![entry.clone()]
        );
        let mut conflict = command;
        if let Command::StartNativeImagePreparation { options, .. } = &mut conflict {
            options.retention_ms += 1;
        }
        assert!(execute_inner(
            &mut client,
            (dir.path(), endpoint, &session, &credential),
            entry.request_id.clone(),
            conflict,
            entry.project.clone(),
            entry.revision
        )
        .is_err());
        assert_eq!(client.calls, 1);
        assert!(execute_inner(
            &mut client,
            (dir.path(), endpoint, &session, &credential),
            RequestId::new("resolve").unwrap(),
            Command::ResolveNativeImagePreparation {
                original_request_id: entry.request_id.clone()
            },
            entry.project.clone(),
            RevisionId::new(1)
        )
        .is_err());
        assert_eq!(client.calls, 2);
        assert_eq!(
            load(dir.path(), endpoint, &session, &credential).unwrap(),
            vec![entry]
        );
    }
    #[test]
    fn saved_identity_rejects_mismatched_authenticated_report() {
        let entry = intent("start");
        let owner = SessionId::new("owner").unwrap();
        let report = NativeImagePreparationReport::queued(NativeImagePreparationIntent {
            operation_id: AttemptId::new("operation").unwrap(),
            engine_epoch: entry.engine_epoch.clone(),
            owner: owner.clone(),
            project: entry.project.clone(),
            revision: entry.revision.value(),
            original_request_id: entry.request_id.clone(),
            options: entry.options,
            image_sha256: "a".repeat(64),
            owner_policy_sha256: "b".repeat(64),
            declared_bytes: 1,
        })
        .unwrap();
        let mut status = NativeImagePreparationStatus {
            report,
            available: false,
            producer_active: true,
            observed_elapsed_ms: Some(0),
        };
        entry.check(&owner, &status).unwrap();
        status.report.intent.engine_epoch = AttemptId::new("different-epoch").unwrap();
        assert!(entry.check(&owner, &status).is_err());
        status.report.intent.engine_epoch = entry.engine_epoch.clone();
        status.report.intent.options.retention_ms += 1;
        assert!(entry.check(&owner, &status).is_err());
    }
}
