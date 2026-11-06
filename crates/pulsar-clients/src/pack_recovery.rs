//! Non-authoritative recovery intent. No bearer or owner capability is stored.
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[cfg(test)]
const LIMIT: u64 = 1024 * 1024;
const ENTRIES: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PackRecoveryIntent {
    Import(PathBuf),
    Select {
        manifest_sha256: String,
        expected_revision: u64,
    },
    Deactivate {
        expected_revision: u64,
    },
    Withdraw {
        choice: ModelPackChoice,
        expected_revision: u64,
        include_unclassified_generation: bool,
    },
    CatalogRefresh {
        source: PathBuf,
        bootstrap: PathBuf,
        bootstrap_sha256: String,
        expected_revision: u64,
    },
    CatalogSelect {
        manifest_sha256: String,
        binding: ModelCatalogBinding,
        expected_revision: u64,
    },
}
impl PackRecoveryIntent {
    pub fn command(&self, owner_proof: String) -> Command {
        match self {
            Self::CatalogRefresh {
                source,
                bootstrap,
                bootstrap_sha256,
                expected_revision,
            } => Command::RefreshModelCatalog {
                source: source.clone(),
                bootstrap: bootstrap.clone(),
                bootstrap_sha256: bootstrap_sha256.clone(),
                expected_catalog_revision: *expected_revision,
                approve_bootstrap: true,
                owner_proof,
            },
            Self::CatalogSelect {
                manifest_sha256,
                binding,
                expected_revision,
            } => Command::SelectCatalogModelPack {
                manifest_sha256: manifest_sha256.clone(),
                binding: binding.clone(),
                expected_selection_revision: *expected_revision,
                approve_unqualified_native_execution: true,
                owner_proof,
            },
            Self::Import(source) => Command::ImportModelPack {
                source: source.clone(),
                owner_proof,
            },
            Self::Select {
                manifest_sha256,
                expected_revision,
            } => Command::SelectModelPack {
                manifest_sha256: Some(manifest_sha256.clone()),
                expected_selection_revision: *expected_revision,
                approve_unqualified_native_execution: true,
                owner_proof,
            },
            Self::Withdraw {
                choice,
                expected_revision,
                include_unclassified_generation,
            } => Command::WithdrawModelPackApproval {
                choice: choice.clone(),
                expected_selection_revision: *expected_revision,
                include_unclassified_generation: *include_unclassified_generation,
                owner_proof,
            },
            Self::Deactivate { expected_revision } => Command::SelectModelPack {
                manifest_sha256: None,
                expected_selection_revision: *expected_revision,
                approve_unqualified_native_execution: false,
                owner_proof,
            },
        }
    }
    fn is_catalog(&self) -> bool {
        matches!(
            self,
            Self::CatalogRefresh { .. } | Self::CatalogSelect { .. }
        )
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Import(path) => ensure!(
                path.is_absolute() && path.as_os_str().len() <= 4096,
                "Invalid retained pack directory"
            ),
            Self::Select {
                manifest_sha256,
                expected_revision,
            } => {
                validate_sha256(manifest_sha256)?;
                ensure!(
                    *expected_revision <= i64::MAX as u64,
                    "Invalid retained revision"
                );
            }
            Self::Withdraw {
                choice,
                expected_revision,
                ..
            } => {
                choice.validate()?;
                ensure!(
                    *expected_revision < i64::MAX as u64
                        && choice.selection_revision <= *expected_revision,
                    "Invalid retained withdrawal fence"
                );
            }
            Self::Deactivate { expected_revision } => ensure!(
                *expected_revision <= i64::MAX as u64,
                "Invalid retained revision"
            ),
            Self::CatalogRefresh {
                source,
                bootstrap,
                bootstrap_sha256,
                expected_revision,
            } => {
                ensure!(
                    source.is_absolute()
                        && bootstrap.is_absolute()
                        && source.as_os_str().len() <= 4096
                        && bootstrap.as_os_str().len() <= 4096
                        && *expected_revision < i64::MAX as u64,
                    "Invalid retained catalog intake"
                );
                validate_sha256(bootstrap_sha256)?;
            }
            Self::CatalogSelect {
                manifest_sha256,
                binding,
                expected_revision,
            } => {
                validate_sha256(manifest_sha256)?;
                binding.validate()?;
                ensure!(
                    *expected_revision < i64::MAX as u64,
                    "Invalid retained catalog selection revision"
                );
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackRecoveryEntry {
    pub id: RequestId,
    pub intent: PackRecoveryIntent,
    pub acknowledged: bool,
    owner_binding: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u16,
    session: SessionId,
    credential_binding: String,
    entries: Vec<PackRecoveryEntry>,
}
fn owner_binding(proof: &str) -> String {
    format!("{:x}", Sha256::digest(proof.as_bytes()))
}
fn captured(command: &Command) -> Option<(PackRecoveryIntent, &str)> {
    match command {
        Command::WithdrawModelPackApproval {
            choice,
            expected_selection_revision,
            include_unclassified_generation,
            owner_proof,
        } => Some((
            PackRecoveryIntent::Withdraw {
                choice: choice.clone(),
                expected_revision: *expected_selection_revision,
                include_unclassified_generation: *include_unclassified_generation,
            },
            owner_proof,
        )),
        Command::RefreshModelCatalog {
            source,
            bootstrap,
            bootstrap_sha256,
            expected_catalog_revision,
            approve_bootstrap: true,
            owner_proof,
        } => Some((
            PackRecoveryIntent::CatalogRefresh {
                source: source.clone(),
                bootstrap: bootstrap.clone(),
                bootstrap_sha256: bootstrap_sha256.clone(),
                expected_revision: *expected_catalog_revision,
            },
            owner_proof,
        )),
        Command::SelectCatalogModelPack {
            manifest_sha256,
            binding,
            expected_selection_revision,
            approve_unqualified_native_execution: true,
            owner_proof,
        } => Some((
            PackRecoveryIntent::CatalogSelect {
                manifest_sha256: manifest_sha256.clone(),
                binding: binding.clone(),
                expected_revision: *expected_selection_revision,
            },
            owner_proof,
        )),
        Command::ImportModelPack {
            source,
            owner_proof,
        } => Some((PackRecoveryIntent::Import(source.clone()), owner_proof)),
        Command::SelectModelPack {
            manifest_sha256,
            expected_selection_revision,
            approve_unqualified_native_execution,
            owner_proof,
        } if *approve_unqualified_native_execution == manifest_sha256.is_some() => Some((
            match manifest_sha256 {
                Some(digest) => PackRecoveryIntent::Select {
                    manifest_sha256: digest.clone(),
                    expected_revision: *expected_selection_revision,
                },
                None => PackRecoveryIntent::Deactivate {
                    expected_revision: *expected_selection_revision,
                },
            },
            owner_proof,
        )),
        _ => None,
    }
}
/// The caller's authenticated session supplies the namespace and non-secret binding.
pub fn load(
    directory: &Path,
    session: &SessionId,
    binding: &str,
) -> Result<Vec<PackRecoveryEntry>> {
    transaction(directory, session, binding, |journal| {
        Ok((journal.entries.clone(), false))
    })
}
/// Persist before dispatch. Exact replay never changes an existing intent or owner binding.
pub fn prepare(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
    command: &Command,
) -> Result<bool> {
    let Some((intent, proof)) = captured(command) else {
        return Ok(false);
    };
    intent.validate()?;
    transaction(directory, session, binding, |journal| {
        let digest = owner_binding(proof);
        if let Some(entry) = journal.entries.iter().find(|entry| entry.id == *id) {
            ensure!(entry.intent==intent&&entry.owner_binding==digest,"Retained request intent or owner capability changed; original recovery record preserved");
            return Ok((true, false));
        }
        ensure!(journal.entries.len()<ENTRIES,"Pack recovery journal is full; explicitly dismiss an acknowledged entry before new mutations");
        journal.entries.push(PackRecoveryEntry {
            id: id.clone(),
            intent,
            acknowledged: false,
            owner_binding: digest,
        });
        Ok((true, true))
    })
}
/// An engine response is an operation receipt, never current selection authority.
pub fn acknowledge(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
) -> Result<()> {
    transaction(directory, session, binding, |journal| {
        let entry = journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == *id)
            .context("Recovery entry is absent")?;
        entry.acknowledged = true;
        Ok(((), true))
    })
}
pub fn dismiss(directory: &Path, session: &SessionId, binding: &str, id: &RequestId) -> Result<()> {
    transaction(directory, session, binding, |journal| {
        let index = journal
            .entries
            .iter()
            .position(|entry| entry.id == *id)
            .context("Recovery entry is absent")?;
        ensure!(
            journal.entries[index].acknowledged,
            "Unacknowledged recovery intent cannot be dismissed"
        );
        journal.entries.remove(index);
        Ok(((), true))
    })
}
fn transaction<T>(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    operation: impl FnOnce(&mut Journal) -> Result<(T, bool)>,
) -> Result<T> {
    validate_sha256(binding)?;
    crate::private_recovery::transaction(
        directory,
        "desktop-pack-recovery",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            entries: vec![],
        },
        |journal| {
            ensure!(
                matches!(journal.schema_version, 1..=3)
                    && journal.session == *session
                    && journal.credential_binding == binding
                    && journal.entries.len() <= ENTRIES,
                "Recovery session/credential binding or layout changed; original journal preserved"
            );
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                validate_sha256(&entry.owner_binding)?;
                ensure!(
                    ids.insert(entry.id.clone()),
                    "Duplicate retained recovery request"
                );
            }
            ensure!(
                journal.schema_version >= 2
                    || !journal.entries.iter().any(|e| e.intent.is_catalog()),
                "Catalog recovery intent lacks reader barrier"
            );
            ensure!(
                journal.schema_version >= 3
                    || !journal
                        .entries
                        .iter()
                        .any(|e| matches!(e.intent, PackRecoveryIntent::Withdraw { .. })),
                "Withdrawal journal reader barrier missing"
            );
            let (value, changed) = operation(journal)?;
            if journal
                .entries
                .iter()
                .any(|e| matches!(e.intent, PackRecoveryIntent::Withdraw { .. }))
            {
                journal.schema_version = 3;
            } else if journal.entries.iter().any(|e| e.intent.is_catalog())
                && journal.schema_version < 2
            {
                journal.schema_version = 2;
            }
            Ok((value, changed))
        },
    )
}

/// Shared desktop/native-fixture path: persist intent, dispatch, then retain acknowledgement.
pub fn execute(
    directory: &Path,
    client: &mut crate::session::SessionClient,
    id: RequestId,
    command: Command,
) -> Result<crate::motion::ResponseBody> {
    use crate::{motion::ResponseBody, session::EngineApi};
    let import = matches!(command, Command::ImportModelPack { .. });
    let catalog = matches!(command, Command::RefreshModelCatalog { .. });
    let withdrawal = matches!(command, Command::WithdrawModelPackApproval { .. });
    let binding = client.recovery_credential_binding();
    ensure!(
        prepare(directory, client.session_id(), &binding, &id, &command)?,
        "Not a journalled pack mutation"
    );
    let response = client.execute_identified(id.clone(), command, None, None)?;
    match &response {
        ResponseBody::ModelPack(report) if import => report.validate()?,
        ResponseBody::ModelCatalog(report) if catalog => report.validate()?,
        ResponseBody::ModelPackWithdrawal(report) if withdrawal => report.validate()?,
        ResponseBody::ModelPackSelection(report) if !import && !catalog && !withdrawal => {
            report.validate()?
        }
        _ => bail!("Unexpected pack receipt; recovery intent retained"),
    }
    acknowledge(directory, client.session_id(), &binding, &id).map_err(|error| {
        error.context(format!(
            "Engine returned receipt for {id}; recovery acknowledgement could not be persisted"
        ))
    })?;
    Ok(response)
}
/// Explicit staging never dispatches or promotes a restored choice.
pub fn stage(
    directory: &Path,
    client: &crate::session::SessionClient,
    id: &RequestId,
    intent: &PackRecoveryIntent,
) -> Result<()> {
    let proof =
        crate::project_package::read_local_owner_proof(directory, "model-pack-owner.token")?;
    ensure!(
        prepare(
            directory,
            client.session_id(),
            &client.recovery_credential_binding(),
            id,
            &intent.command(proof)
        )?,
        "Not a pack mutation"
    );
    Ok(())
}
/// Resolve current capability, but retain the original request and intent.
pub fn replay(
    directory: &Path,
    client: &mut crate::session::SessionClient,
    id: &RequestId,
) -> Result<crate::motion::ResponseBody> {
    let entry = load(
        directory,
        client.session_id(),
        &client.recovery_credential_binding(),
    )?
    .into_iter()
    .find(|entry| entry.id == *id)
    .context("Retained request is absent")?;
    let proof =
        crate::project_package::read_local_owner_proof(directory, "model-pack-owner.token")?;
    execute(directory, client, id.clone(), entry.intent.command(proof))
}

#[cfg(all(test, unix))]
mod tests {

    #[test]
    fn withdrawal_recovery_retains_exact_scope_without_capability_and_uses_reader_barrier() {
        let directory = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new("withdrawal-window").unwrap();
        let binding = "b".repeat(64);
        let id = RequestId::new("original-withdrawal").unwrap();
        let command = Command::WithdrawModelPackApproval {
            choice: ModelPackChoice {
                manifest_sha256: "c".repeat(64),
                selection_revision: 1,
            },
            expected_selection_revision: 3,
            include_unclassified_generation: true,
            owner_proof: "private-fixture-proof".into(),
        };
        assert!(prepare(directory.path(), &session, &binding, &id, &command).unwrap());
        let entries = load(directory.path(), &session, &binding).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].acknowledged);
        assert_eq!(
            serde_json::to_value(entries[0].intent.command("private-fixture-proof".into()))
                .unwrap(),
            serde_json::to_value(&command).unwrap()
        );
        let path = journal(directory.path(), &session);
        let before = fs::read(&path).unwrap();
        assert!(!std::str::from_utf8(&before)
            .unwrap()
            .contains("private-fixture-proof"));
        let mut value: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(value["schema_version"], 3);
        value["schema_version"] = serde_json::json!(2);
        let downgraded = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &downgraded).unwrap();
        assert!(load(directory.path(), &session, &binding).is_err());
        assert_eq!(fs::read(&path).unwrap(), downgraded);
        fs::write(&path, before).unwrap();
        let mut changed = command.clone();
        if let Command::WithdrawModelPackApproval {
            include_unclassified_generation,
            ..
        } = &mut changed
        {
            *include_unclassified_generation = false;
        }
        assert!(prepare(directory.path(), &session, &binding, &id, &changed).is_err());
        assert_eq!(load(directory.path(), &session, &binding).unwrap().len(), 1);
    }
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    fn fixture() -> (tempfile::TempDir, SessionId, String, Command) {
        let dir = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        (
            dir,
            SessionId::new("fixture-session").unwrap(),
            "b".repeat(64),
            Command::SelectModelPack {
                manifest_sha256: None,
                expected_selection_revision: 0,
                approve_unqualified_native_execution: false,
                owner_proof: "fixture-capability-never-stored".into(),
            },
        )
    }
    fn journal(dir: &Path, session: &SessionId) -> PathBuf {
        dir.join("desktop-pack-recovery").join(format!(
            "{:x}.json",
            Sha256::digest(session.as_str().as_bytes())
        ))
    }
    #[test]
    fn catalog_intents_upgrade_reader_barrier_and_preserve_exact_original_intent() {
        let (dir, session, binding, old) = fixture();
        let old_id = RequestId::new("old-intent").unwrap();
        prepare(dir.path(), &session, &binding, &old_id, &old).unwrap();
        let command = PackRecoveryIntent::CatalogRefresh {
            source: "/authored/catalog".into(),
            bootstrap: "/authored/bootstrap.json".into(),
            bootstrap_sha256: "c".repeat(64),
            expected_revision: 2,
        }
        .command("fixture-capability-never-stored".into());
        let id = RequestId::new("catalog-recovery").unwrap();
        prepare(dir.path(), &session, &binding, &id, &command).unwrap();
        let path = journal(dir.path(), &session);
        let before = fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(!String::from_utf8_lossy(&before).contains("fixture-capability-never-stored"));
        assert_eq!(load(dir.path(), &session, &binding).unwrap().len(), 2);
        let mut changed = command.clone();
        if let Command::RefreshModelCatalog {
            expected_catalog_revision,
            ..
        } = &mut changed
        {
            *expected_catalog_revision = 3;
        }
        assert!(prepare(dir.path(), &session, &binding, &id, &changed).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        acknowledge(dir.path(), &session, &binding, &id).unwrap();
        dismiss(dir.path(), &session, &binding, &id).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(load(dir.path(), &session, &binding).unwrap().len(), 1);
    }
    #[test]
    fn durable_intent_replays_exactly_refuses_changed_bindings_and_requires_ack_for_dismissal() {
        let (dir, session, binding, command) = fixture();
        let id = RequestId::new("lost-ack").unwrap();
        prepare(dir.path(), &session, &binding, &id, &command).unwrap();
        let path = journal(dir.path(), &session);
        let before = fs::read(&path).unwrap();
        assert!(!String::from_utf8(before.clone())
            .unwrap()
            .contains("fixture-capability-never-stored"));
        let restored = load(dir.path(), &session, &binding).unwrap();
        assert_eq!(restored[0].id, id);
        assert!(!restored[0].acknowledged);
        assert!(dismiss(dir.path(), &session, &binding, &id).is_err());
        prepare(dir.path(), &session, &binding, &id, &command).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        let mut changed = command.clone();
        if let Command::SelectModelPack {
            expected_selection_revision,
            ..
        } = &mut changed
        {
            *expected_selection_revision = 1;
        }
        assert!(prepare(dir.path(), &session, &binding, &id, &changed).is_err());
        if let Command::SelectModelPack { owner_proof, .. } = &mut changed {
            *owner_proof = "changed capability".into();
        }
        assert!(prepare(dir.path(), &session, &binding, &id, &changed).is_err());
        assert!(load(dir.path(), &session, &"c".repeat(64)).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        acknowledge(dir.path(), &session, &binding, &id).unwrap();
        dismiss(dir.path(), &session, &binding, &id).unwrap();
        assert!(load(dir.path(), &session, &binding).unwrap().is_empty());
    }
    #[test]
    fn corrupt_oversized_nonprivate_symlink_and_hardlink_journals_refuse_without_replacing_bytes() {
        let (dir, session, binding, command) = fixture();
        let id = RequestId::new("fixture").unwrap();
        prepare(dir.path(), &session, &binding, &id, &command).unwrap();
        let path = journal(dir.path(), &session);
        let good = fs::read(&path).unwrap();
        for bad in [b"invalid journal".to_vec(), vec![b'x'; LIMIT as usize + 1]] {
            fs::write(&path, &bad).unwrap();
            assert!(prepare(dir.path(), &session, &binding, &id, &command).is_err());
            assert_eq!(fs::read(&path).unwrap(), bad);
        }
        fs::write(&path, &good).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(dir.path(), &session, &binding).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let target = dir.path().join("preserved");
        fs::rename(&path, &target).unwrap();
        symlink(&target, &path).unwrap();
        assert!(load(dir.path(), &session, &binding).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(load(dir.path(), &session, &binding).is_err());
        assert_eq!(fs::read(&target).unwrap(), good);
    }
    #[test]
    fn concurrent_windows_merge_distinct_requests_under_the_same_session() {
        let (dir, session, binding, command) = fixture();
        let threads = (0..4)
            .map(|window| {
                let path = dir.path().to_path_buf();
                let session = session.clone();
                let binding = binding.clone();
                let command = command.clone();
                std::thread::spawn(move || {
                    let mut completed = vec![];
                    let mut refused = vec![];
                    for index in 0..8 {
                        let id = RequestId::new(format!("window-{window}-{index}")).unwrap();
                        match prepare(&path, &session, &binding, &id, &command) {
                            Ok(journalled) => {
                                assert!(journalled);
                                completed.push(id);
                            }
                            Err(error) => {
                                assert!(error.to_string().contains("busy"), "{error}");
                                refused.push(id);
                            }
                        }
                    }
                    (completed, refused)
                })
            })
            .collect::<Vec<_>>();
        let mut completed = std::collections::BTreeSet::new();
        let mut refused = vec![];
        for thread in threads {
            let (successes, busy) = thread.join().unwrap();
            for id in successes {
                assert!(completed.insert(id));
            }
            refused.extend(busy);
        }
        let before_retry = load(dir.path(), &session, &binding).unwrap();
        assert_eq!(
            before_retry
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            completed
        );
        assert_eq!(before_retry.len(), completed.len());
        for id in &refused {
            assert!(!completed.contains(id));
            assert!(prepare(dir.path(), &session, &binding, id, &command).unwrap());
        }
        let entries = load(dir.path(), &session, &binding).unwrap();
        let expected = (0..4)
            .flat_map(|window| {
                (0..8).map(move |index| RequestId::new(format!("window-{window}-{index}")).unwrap())
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(entries.len(), 32);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        for entry in entries {
            assert_eq!(
                entry.intent,
                PackRecoveryIntent::Deactivate {
                    expected_revision: 0
                }
            );
            assert!(!entry.acknowledged);
            assert_eq!(
                entry.owner_binding,
                owner_binding("fixture-capability-never-stored")
            );
        }
    }

    #[test]
    fn held_journal_lock_refuses_without_mutation_then_retries_original_identity() {
        use rustix::fs::{flock, open, FlockOperation, Mode, OFlags};
        let (dir, session, binding, command) = fixture();
        let existing = RequestId::new("existing-request").unwrap();
        let waiting = RequestId::new("busy-original-request").unwrap();
        prepare(dir.path(), &session, &binding, &existing, &command).unwrap();
        let path = journal(dir.path(), &session);
        let before = fs::read(&path).unwrap();
        let lock = open(
            path.with_extension("lock"),
            OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .unwrap();
        flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
        let error = prepare(dir.path(), &session, &binding, &waiting, &command).unwrap_err();
        assert!(error.to_string().contains("busy"));
        assert!(
            error
                .chain()
                .any(|cause| cause.downcast_ref::<rustix::io::Errno>()
                    == Some(&rustix::io::Errno::WOULDBLOCK)),
            "{error:#}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        drop(lock);
        assert!(prepare(dir.path(), &session, &binding, &waiting, &command).unwrap());
        assert!(prepare(dir.path(), &session, &binding, &waiting, &command).unwrap());
        let entries = load(dir.path(), &session, &binding).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.id == existing));
        assert!(entries.iter().any(|entry| entry.id == waiting));
        assert!(entries.iter().all(|entry| !entry.acknowledged));
    }
    #[test]
    fn full_journal_preserves_unknown_intent_until_explicit_acknowledged_dismissal() {
        let (dir, session, binding, command) = fixture();
        for index in 0..ENTRIES {
            prepare(
                dir.path(),
                &session,
                &binding,
                &RequestId::new(format!("entry-{index}")).unwrap(),
                &command,
            )
            .unwrap();
        }
        let path = journal(dir.path(), &session);
        let before = fs::read(&path).unwrap();
        let next = RequestId::new("next").unwrap();
        assert!(prepare(dir.path(), &session, &binding, &next, &command).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let first = RequestId::new("entry-0").unwrap();
        acknowledge(dir.path(), &session, &binding, &first).unwrap();
        dismiss(dir.path(), &session, &binding, &first).unwrap();
        prepare(dir.path(), &session, &binding, &next, &command).unwrap();
        assert_eq!(load(dir.path(), &session, &binding).unwrap().len(), ENTRIES);
    }
}
