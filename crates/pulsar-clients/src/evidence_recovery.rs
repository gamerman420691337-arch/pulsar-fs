//! Non-authoritative removal intent and validated acknowledgement; no credentials.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
const ENTRIES: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RemovalIntent {
    pub binding: (ProjectId, RevisionId),
    pub epoch: AttemptId,
    pub manifest_sha256: String,
    pub request_id: RequestId,
}
impl RemovalIntent {
    pub fn recovery_command(&self) -> String {
        format!(
            "pulsar live-capture remove-evidence --request-id={} --confirm-removal -- {} {} {} {}",
            self.request_id,
            self.binding.0,
            self.binding.1.value(),
            self.epoch,
            self.manifest_sha256
        )
    }
    fn validate(&self) -> Result<()> {
        validate_sha256(&self.manifest_sha256)?;
        ensure!(
            self.binding.1.value() <= i64::MAX as u64,
            "Removal revision exceeds storage range"
        );
        Ok(())
    }
    pub fn validate_receipt(&self, receipt: &LiveCudaEvidenceRemoval) -> Result<()> {
        self.validate()?;
        receipt.validate()?;
        ensure!(
            receipt.project_id == self.binding.0
                && receipt.epoch == self.epoch
                && receipt.manifest.sha256 == self.manifest_sha256,
            "Capture removal receipt differs from intent"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub intent: RemovalIntent,
    pub receipt: Option<LiveCudaEvidenceRemoval>,
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
fn endpoint_binding(endpoint: &Path) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(endpoint)?)
    ))
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
    let endpoint_binding = endpoint_binding(endpoint)?;
    crate::private_recovery::transaction(
        directory,
        "desktop-evidence-recovery",
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            endpoint_binding: endpoint_binding.clone(),
            entries: vec![],
        },
        |journal| {
            ensure!(journal.schema_version==1&&journal.session==*session&&journal.credential_binding==binding&&journal.endpoint_binding==endpoint_binding&&journal.entries.len()<=ENTRIES,"Removal recovery session, endpoint, credential or layout changed; original journal preserved");
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                if let Some(receipt) = &entry.receipt {
                    entry.intent.validate_receipt(receipt)?;
                }
                ensure!(
                    ids.insert(entry.intent.request_id.clone()),
                    "Duplicate removal recovery request"
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
                entry.intent == *intent,
                "Removal recovery intent changed; original preserved"
            );
            return Ok(((), false));
        }
        ensure!(
            j.entries.len() < ENTRIES,
            "Removal recovery journal is full; dismiss acknowledged entries before new removals"
        );
        j.entries.push(Entry {
            intent: intent.clone(),
            receipt: None,
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
    receipt: &LiveCudaEvidenceRemoval,
) -> Result<()> {
    intent.validate_receipt(receipt)?;
    transaction(directory, endpoint, session, binding, |j| {
        let entry = j
            .entries
            .iter_mut()
            .find(|e| e.intent.request_id == intent.request_id)
            .context("Removal recovery entry absent")?;
        ensure!(
            entry.intent == *intent,
            "Removal acknowledgement intent changed"
        );
        if let Some(saved) = &entry.receipt {
            ensure!(
                serde_json::to_vec(saved)? == serde_json::to_vec(receipt)?,
                "Removal acknowledgement changed; original preserved"
            );
            return Ok(((), false));
        }
        entry.receipt = Some(receipt.clone());
        Ok(((), true))
    })
}
pub(crate) fn dismiss(
    directory: &Path,
    endpoint: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
) -> Result<Vec<Entry>> {
    transaction(directory, endpoint, session, binding, |j| {
        let index = j
            .entries
            .iter()
            .position(|e| e.intent.request_id == *id)
            .context("Removal recovery entry absent")?;
        ensure!(
            j.entries[index].receipt.is_some(),
            "Unacknowledged removal recovery cannot be dismissed"
        );
        j.entries.remove(index);
        Ok((j.entries.clone(), true))
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    fn fixture() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        SessionId,
        String,
        RemovalIntent,
    ) {
        let dir = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let endpoint = dir.path().join("engine.sock");
        (
            dir,
            endpoint,
            SessionId::new("session").unwrap(),
            "b".repeat(64),
            RemovalIntent {
                binding: (ProjectId::new("project").unwrap(), RevisionId::new(3)),
                epoch: AttemptId::new("capture").unwrap(),
                manifest_sha256: "a".repeat(64),
                request_id: RequestId::new("request").unwrap(),
            },
        )
    }
    fn path(dir: &Path, session: &SessionId) -> std::path::PathBuf {
        dir.join("desktop-evidence-recovery").join(format!(
            "{:x}.json",
            Sha256::digest(session.as_str().as_bytes())
        ))
    }
    fn receipt(intent: &RemovalIntent) -> LiveCudaEvidenceRemoval {
        LiveCudaEvidenceRemoval {
            schema_version: 1,
            project_id: intent.binding.0.clone(),
            epoch: intent.epoch.clone(),
            manifest: ArtifactIdentity {
                artifact_id: ArtifactId::new(intent.manifest_sha256.clone()).unwrap(),
                sha256: intent.manifest_sha256.clone(),
                byte_len: 10,
            },
            removed_original_bytes: 20,
            physical_erasure: false,
        }
    }
    #[test]
    fn exact_restore_conflicts_and_binding_changes_preserve_original_journal() {
        let (dir, endpoint, session, binding, intent) = fixture();
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        let p = path(dir.path(), &session);
        let before = fs::read(&p).unwrap();
        assert_eq!(
            load(dir.path(), &endpoint, &session, &binding).unwrap()[0].intent,
            intent
        );
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        assert_eq!(fs::read(&p).unwrap(), before);
        for changed in [
            RemovalIntent {
                binding: (intent.binding.0.clone(), RevisionId::new(4)),
                ..intent.clone()
            },
            RemovalIntent {
                manifest_sha256: "c".repeat(64),
                ..intent.clone()
            },
        ] {
            assert!(prepare(dir.path(), &endpoint, &session, &binding, &changed).is_err());
        }
        assert!(load(
            dir.path(),
            &dir.path().join("other.sock"),
            &session,
            &binding
        )
        .is_err());
        assert!(load(dir.path(), &endpoint, &session, &"c".repeat(64)).is_err());
        assert!(dismiss(
            dir.path(),
            &endpoint,
            &session,
            &binding,
            &intent.request_id
        )
        .is_err());
        assert_eq!(fs::read(&p).unwrap(), before);
        let r = receipt(&intent);
        let mut wrong = r.clone();
        wrong.epoch = AttemptId::new("other").unwrap();
        assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &wrong).is_err());
        acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &r).unwrap();
        let saved = fs::read(&p).unwrap();
        acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &r).unwrap();
        assert_eq!(fs::read(&p).unwrap(), saved);
        let mut changed = r.clone();
        changed.removed_original_bytes += 1;
        assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &changed).is_err());
        assert_eq!(fs::read(&p).unwrap(), saved);
        assert!(dismiss(
            dir.path(),
            &endpoint,
            &session,
            &binding,
            &intent.request_id
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn corrupt_and_nonprivate_storage_refuses_without_replacing_bytes() {
        let (dir, endpoint, session, binding, intent) = fixture();
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        let p = path(dir.path(), &session);
        let good = fs::read(&p).unwrap();
        for bad in [b"broken".to_vec(), vec![b'x'; 1048577]] {
            fs::write(&p, &bad).unwrap();
            assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
            assert_eq!(fs::read(&p).unwrap(), bad);
        }
        fs::write(&p, &good).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let target = dir.path().join("original");
        fs::rename(&p, &target).unwrap();
        symlink(&target, &p).unwrap();
        assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
        fs::remove_file(&p).unwrap();
        fs::hard_link(&target, &p).unwrap();
        assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
        assert_eq!(fs::read(&target).unwrap(), good);
    }
    #[test]
    #[ignore = "explicit owned child SIGKILL after durable preparation; run serially"]
    fn native_process_death_after_prepare_preserves_exact_unknown_intent() {
        if let Ok(root) = std::env::var("PULSAR_EVIDENCE_RECOVERY_KILL_CHILD") {
            let dir = Path::new(&root);
            let endpoint = dir.join("engine.sock");
            let session = SessionId::new("session").unwrap();
            let intent = RemovalIntent {
                binding: (ProjectId::new("project").unwrap(), RevisionId::new(3)),
                epoch: AttemptId::new("capture").unwrap(),
                manifest_sha256: "a".repeat(64),
                request_id: RequestId::new("request").unwrap(),
            };
            prepare(dir, &endpoint, &session, &"b".repeat(64), &intent).unwrap();
            fs::write(dir.join("prepared-marker"), b"ready").unwrap();
            loop {
                std::thread::park_timeout(std::time::Duration::from_secs(1));
            }
        }
        let (dir, endpoint, session, binding, intent) = fixture();
        let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","evidence_recovery::tests::native_process_death_after_prepare_preserves_exact_unknown_intent","--ignored","--test-threads=1"])
            .env("PULSAR_EVIDENCE_RECOVERY_KILL_CHILD",dir.path()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut ready = false;
        while std::time::Instant::now() < deadline {
            if dir.path().join("prepared-marker").exists() {
                ready = true;
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = child.kill();
        let status = child.wait().unwrap();
        assert!(ready, "owned child did not durably prepare");
        assert!(!status.success());
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
        let restored = load(dir.path(), &endpoint, &session, &binding).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].intent, intent);
        assert!(restored[0].receipt.is_none());
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        assert_eq!(
            load(dir.path(), &endpoint, &session, &binding)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn full_journal_preserves_unknown_requests_and_reclaims_only_acknowledged_entries() {
        let (dir, endpoint, session, binding, mut intent) = fixture();
        for i in 0..ENTRIES {
            intent.request_id = RequestId::new(format!("request-{i}")).unwrap();
            prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        }
        let p = path(dir.path(), &session);
        let before = fs::read(&p).unwrap();
        intent.request_id = RequestId::new("overflow").unwrap();
        assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
        assert_eq!(fs::read(&p).unwrap(), before);
        intent.request_id = RequestId::new("request-0").unwrap();
        acknowledge(
            dir.path(),
            &endpoint,
            &session,
            &binding,
            &intent,
            &receipt(&intent),
        )
        .unwrap();
        dismiss(
            dir.path(),
            &endpoint,
            &session,
            &binding,
            &intent.request_id,
        )
        .unwrap();
        intent.request_id = RequestId::new("overflow").unwrap();
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
        assert_eq!(
            load(dir.path(), &endpoint, &session, &binding)
                .unwrap()
                .len(),
            ENTRIES
        );
    }
}
