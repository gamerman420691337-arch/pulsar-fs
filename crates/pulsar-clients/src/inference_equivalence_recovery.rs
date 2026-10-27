//! Non-authoritative owner-decision recovery. Never stores bearer or owner proofs.
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const ENTRIES: usize = 64;
const NAMESPACE: &str = "inference-equivalence-recovery";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Intent {
    Import {
        path: String,
        manifest_sha256: String,
        expected_dossier_id: ArtifactId,
    },
    Accept {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        operator: String,
    },
    Withdraw {
        dossier_id: ArtifactId,
        expected_record_revision: u64,
        reason: String,
    },
}
impl Intent {
    /// Read and bind exact manifest bytes once; recovery does not reopen mutable input.
    pub fn import(path: String, manifest_sha256: String) -> Result<Self> {
        use std::{fs::OpenOptions, io::Read};
        validate_path(&path)?;
        validate_sha256(&manifest_sha256)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            );
        }
        let file = options
            .open(&path)
            .context("Read inference equivalence manifest")?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && (1..=INFERENCE_EQUIVALENCE_MANIFEST_LIMIT).contains(&metadata.len()),
            "Equivalence manifest must be a bounded regular file"
        );
        let mut bytes = Vec::new();
        file.take(INFERENCE_EQUIVALENCE_MANIFEST_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == metadata.len() && hash(&bytes) == manifest_sha256,
            "Equivalence manifest bytes differ from supplied digest"
        );
        let input: InferenceEquivalenceInput = serde_json::from_slice(&bytes)?;
        input.validate()?;
        let mut evidence: Vec<_> = input
            .evidence
            .into_iter()
            .map(|item| {
                Ok(InferenceEquivalenceEvidence {
                    kind: item.kind,
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(item.sha256.clone())?,
                        sha256: item.sha256,
                        byte_len: item.byte_len,
                    },
                })
            })
            .collect::<Result<_>>()?;
        evidence.sort_by_key(|item| item.kind);
        let expected_dossier_id = InferenceEquivalenceManifest {
            subject: input.subject,
            evidence,
        }
        .id()?;
        let intent = Self::Import {
            path,
            manifest_sha256,
            expected_dossier_id,
        };
        intent.validate()?;
        Ok(intent)
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Import {
                path,
                manifest_sha256,
                expected_dossier_id,
            } => {
                validate_path(path)?;
                validate_sha256(manifest_sha256)?;
                validate_sha256(expected_dossier_id.as_str())?;
            }
            Self::Accept {
                dossier_id,
                expected_record_revision,
                operator,
            } => {
                validate_revision(dossier_id, *expected_record_revision)?;
                validate_inference_equivalence_note(operator)?;
                ensure!(
                    operator.len() <= 256,
                    "Equivalence operator exceeds 256 bytes"
                );
            }
            Self::Withdraw {
                dossier_id,
                expected_record_revision,
                reason,
            } => {
                validate_revision(dossier_id, *expected_record_revision)?;
                validate_inference_equivalence_note(reason)?;
            }
        }
        Ok(())
    }
    fn command(&self, owner_proof: String) -> Command {
        match self {
            Self::Import {
                path,
                manifest_sha256,
                ..
            } => Command::ImportInferenceEquivalence {
                path: path.clone(),
                manifest_sha256: manifest_sha256.clone(),
                owner_proof,
            },
            Self::Accept {
                dossier_id,
                expected_record_revision,
                operator,
            } => Command::AcceptInferenceEquivalence {
                dossier_id: dossier_id.clone(),
                expected_record_revision: *expected_record_revision,
                operator: operator.clone(),
                acknowledge_measured_equivalence: true,
                owner_proof,
            },
            Self::Withdraw {
                dossier_id,
                expected_record_revision,
                reason,
            } => Command::WithdrawInferenceEquivalence {
                dossier_id: dossier_id.clone(),
                expected_record_revision: *expected_record_revision,
                reason: reason.clone(),
                owner_proof,
            },
        }
    }
    fn validate_receipt(
        &self,
        session: &SessionId,
        id: &RequestId,
        receipt: &InferenceEquivalenceReceipt,
    ) -> Result<()> {
        self.validate()?;
        receipt.validate()?;
        ensure!(
            receipt.request_id == *id,
            "Equivalence receipt has another request ID; original intent retained"
        );
        let report = &receipt.report;
        match self {
            Self::Import {
                manifest_sha256,
                expected_dossier_id,
                ..
            } => ensure!(
                receipt.input_sha256.as_ref() == Some(manifest_sha256)
                    && report.dossier_id == *expected_dossier_id,
                "Equivalence import receipt differs from exact retained input"
            ),
            Self::Accept {
                dossier_id,
                expected_record_revision,
                operator,
            } => {
                validate_decision_receipt(
                    receipt,
                    session,
                    id,
                    dossier_id,
                    *expected_record_revision,
                    InferenceEquivalenceState::Accepted,
                )?;
                ensure!(
                    report.decision.as_ref().and_then(|d| d.operator.as_ref()) == Some(operator),
                    "Equivalence receipt operator differs"
                );
            }
            Self::Withdraw {
                dossier_id,
                expected_record_revision,
                reason,
            } => {
                validate_decision_receipt(
                    receipt,
                    session,
                    id,
                    dossier_id,
                    *expected_record_revision,
                    InferenceEquivalenceState::Withdrawn,
                )?;
                ensure!(
                    report.decision.as_ref().is_some_and(|d| &d.note == reason),
                    "Equivalence receipt withdrawal reason differs"
                );
            }
        }
        Ok(())
    }
}
fn validate_path(path: &str) -> Result<()> {
    ensure!(
        Path::new(path).is_absolute()
            && path.len() <= 4096
            && !path.contains('\0')
            && !Path::new(path)
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
        "Equivalence manifest needs an absolute bounded path without traversal"
    );
    Ok(())
}
fn validate_revision(id: &ArtifactId, revision: u64) -> Result<()> {
    validate_sha256(id.as_str())?;
    ensure!(
        revision > 0 && revision < i64::MAX as u64,
        "Equivalence decision revision cannot advance"
    );
    Ok(())
}
fn validate_decision_receipt(
    receipt: &InferenceEquivalenceReceipt,
    session: &SessionId,
    id: &RequestId,
    dossier: &ArtifactId,
    revision: u64,
    state: InferenceEquivalenceState,
) -> Result<()> {
    let report = &receipt.report;
    ensure!(receipt.input_sha256.is_none() && report.dossier_id == *dossier && report.revision == revision + 1 && report.state == state && report.decision.as_ref().is_some_and(|d| d.request_id == *id && d.actor_session == *session), "Equivalence decision receipt differs from original actor, request, dossier, state or revision");
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub request_id: RequestId,
    pub intent: Intent,
    /// Exact validated historical receipt hash; not current authority or eligibility.
    pub acknowledged_receipt_sha256: Option<String>,
    owner_binding: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u16,
    session: SessionId,
    credential_binding: String,
    entries: Vec<Entry>,
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
        NAMESPACE,
        session,
        Journal {
            schema_version: 1,
            session: session.clone(),
            credential_binding: binding.into(),
            entries: vec![],
        },
        |journal| {
            ensure!(journal.schema_version == 1 && journal.session == *session && journal.credential_binding == binding && journal.entries.len() <= ENTRIES, "Equivalence recovery session, credential binding or schema differs; original journal preserved");
            let mut ids = std::collections::BTreeSet::new();
            for entry in &journal.entries {
                entry.intent.validate()?;
                validate_sha256(&entry.owner_binding)?;
                if let Some(digest) = &entry.acknowledged_receipt_sha256 {
                    validate_sha256(digest)?;
                }
                ensure!(
                    ids.insert(entry.request_id.clone()),
                    "Duplicate equivalence recovery request"
                );
            }
            operation(journal)
        },
    )
}
pub fn load(directory: &Path, session: &SessionId, binding: &str) -> Result<Vec<Entry>> {
    transaction(directory, session, binding, |journal| {
        Ok((journal.entries.clone(), false))
    })
}
fn prepare(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
    intent: &Intent,
    proof: &str,
) -> Result<()> {
    intent.validate()?;
    ensure!(!proof.is_empty(), "Equivalence owner capability is absent");
    transaction(directory, session, binding, |journal| {
        let owner_binding = hash(proof.as_bytes());
        if let Some(entry) = journal.entries.iter().find(|e| e.request_id == *id) {
            ensure!(entry.intent == *intent && entry.owner_binding == owner_binding, "Equivalence request intent or owner capability changed; original recovery entry preserved");
            return Ok(((), false));
        }
        ensure!(journal.entries.len() < ENTRIES, "Equivalence recovery journal is full; dismiss an acknowledged entry before a new mutation");
        journal.entries.push(Entry {
            request_id: id.clone(),
            intent: intent.clone(),
            acknowledged_receipt_sha256: None,
            owner_binding,
        });
        Ok(((), true))
    })
}
fn acknowledge(
    directory: &Path,
    session: &SessionId,
    binding: &str,
    id: &RequestId,
    intent: &Intent,
    proof: &str,
    receipt: &InferenceEquivalenceReceipt,
) -> Result<()> {
    intent.validate_receipt(session, id, receipt)?;
    let digest = hash(&serde_json::to_vec(receipt)?);
    transaction(directory, session, binding, |journal| {
        let entry = journal
            .entries
            .iter_mut()
            .find(|e| e.request_id == *id)
            .context("Equivalence recovery entry is absent")?;
        ensure!(
            entry.intent == *intent && entry.owner_binding == hash(proof.as_bytes()),
            "Equivalence acknowledgment binding differs"
        );
        if let Some(retained) = &entry.acknowledged_receipt_sha256 {
            ensure!(
                *retained == digest,
                "Equivalence replay receipt differs from acknowledged original"
            );
            return Ok(((), false));
        }
        entry.acknowledged_receipt_sha256 = Some(digest);
        Ok(((), true))
    })
}
pub fn dismiss(directory: &Path, session: &SessionId, binding: &str, id: &RequestId) -> Result<()> {
    transaction(directory, session, binding, |journal| {
        let index = journal
            .entries
            .iter()
            .position(|e| e.request_id == *id)
            .context("Equivalence recovery entry is absent")?;
        ensure!(journal.entries[index].acknowledged_receipt_sha256.is_some(), "Unacknowledged equivalence intent cannot be dismissed; explicitly retry its original identity");
        journal.entries.remove(index);
        Ok(((), true))
    })
}
/// Durably retain an explicit intent before dispatch; verify its exact receipt before acknowledgment.
pub fn execute(
    directory: &Path,
    client: &mut crate::session::SessionClient,
    id: RequestId,
    intent: Intent,
) -> Result<crate::motion::ResponseBody> {
    let proof =
        crate::project_package::read_local_owner_proof(directory, "model-pack-owner.token")?;
    let session = client.session_id().clone();
    let binding = client.recovery_credential_binding();
    execute_bound(directory, client, &session, &binding, id, intent, proof)
}
fn execute_bound(
    directory: &Path,
    api: &mut impl crate::session::EngineApi,
    session: &SessionId,
    binding: &str,
    id: RequestId,
    intent: Intent,
    proof: String,
) -> Result<crate::motion::ResponseBody> {
    prepare(directory, session, binding, &id, &intent, &proof)?;
    let response = api.execute_identified(id.clone(), intent.command(proof.clone()), None, None)?;
    let crate::motion::ResponseBody::InferenceEquivalenceSaved(receipt) = &response else {
        bail!("Unexpected equivalence response; original recovery intent retained");
    };
    acknowledge(directory, session, binding, &id, &intent, &proof, receipt).with_context(|| format!("Engine returned a response for {id}; exact receipt acknowledgment failed and original intent remains retained"))?;
    Ok(response)
}
/// Explicit retry only. Loading an intent never dispatches it or refreshes its authority.
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
    .find(|e| e.request_id == *id)
    .context("Retained equivalence request is absent")?;
    execute(directory, client, id.clone(), entry.intent)
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use crate::{motion::ResponseBody, session::EngineApi};
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    fn identity(seed: u8) -> ArtifactIdentity {
        let digest = format!("{seed:02x}").repeat(32);
        ArtifactIdentity {
            artifact_id: ArtifactId::new(digest.clone()).unwrap(),
            sha256: digest,
            byte_len: 1,
        }
    }
    pub(crate) fn synthetic_manifest() -> InferenceEquivalenceManifest {
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity(1),
            runtime: identity(2),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 32,
                    height: 32,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let mut to = profile.clone();
        to.cpu_threads = 2;
        let environment = InferenceEquivalenceEnvironment {
            engine: identity(3),
            worker: identity(4),
            os: "synthetic-linux".into(),
            architecture: "synthetic-x86_64".into(),
            platform_build: identity(5),
            hardware: identity(6),
            dependencies: vec![identity(2)],
        };
        let manifest = InferenceEquivalenceManifest {
            subject: InferenceEquivalenceSubject { schema_version: 1, origin: InferenceEquivalenceOrigin::Synthetic, from_profile: profile, to_profile: to, from_environment: environment.clone(), to_environment: environment,
                scope: InferenceEquivalenceScope { tier: InferenceEquivalenceTier::Fast, recipe: InferenceEquivalenceRecipe::YoloTracking, axes: vec![Axis::Stroke], source_policy: identity(7), view_policy: identity(8), limitations: "Authored synthetic transport fixture; no measured equivalence or acceptance".into() }, not_before_unix_ms: 1, expires_unix_ms: 1000 },
            evidence: InferenceEquivalenceEvidenceKind::ALL.into_iter().enumerate().map(|(i, kind)| InferenceEquivalenceEvidence { kind, identity: identity(10 + i as u8) }).collect(),
        };
        manifest.validate().unwrap();
        manifest
    }
    pub(crate) fn pending_report() -> InferenceEquivalenceReport {
        let manifest = synthetic_manifest();
        InferenceEquivalenceReport {
            dossier_id: manifest.id().unwrap(),
            revision: 1,
            state: InferenceEquivalenceState::Pending,
            manifest,
            decision: None,
            report_only: true,
        }
    }
    fn withdrawal(session: &SessionId, id: &RequestId) -> (Intent, InferenceEquivalenceReceipt) {
        let mut report = pending_report();
        let intent = Intent::Withdraw {
            dossier_id: report.dossier_id.clone(),
            expected_record_revision: 1,
            reason: "Withdraw authored synthetic fixture".into(),
        };
        report.revision = 2;
        report.state = InferenceEquivalenceState::Withdrawn;
        report.decision = Some(InferenceEquivalenceDecision {
            revision: 2,
            state: report.state,
            request_id: id.clone(),
            actor_session: session.clone(),
            operator: None,
            note: "Withdraw authored synthetic fixture".into(),
            recorded_at_unix_ms: None,
            review_identity: None,
        });
        let receipt = InferenceEquivalenceReceipt {
            input_sha256: None,
            request_id: id.clone(),
            report,
        };
        receipt.validate().unwrap();
        (intent, receipt)
    }
    fn fixture() -> (tempfile::TempDir, SessionId, String, RequestId) {
        (
            tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap(),
            SessionId::new("equivalence-recovery-fixture").unwrap(),
            "b".repeat(64),
            RequestId::new("original-equivalence-withdrawal").unwrap(),
        )
    }
    fn journal_path(dir: &Path, session: &SessionId) -> PathBuf {
        dir.join(NAMESPACE)
            .join(format!("{}.json", hash(session.as_str().as_bytes())))
    }
    struct FixtureApi<F>(F);
    impl<F: FnMut(RequestId, Command) -> Result<ResponseBody>> EngineApi for FixtureApi<F> {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            bail!("Unidentified dispatch forbidden in recovery fixture")
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
                "Equivalence must be global owner scope"
            );
            (self.0)(id, command)
        }
    }
    #[test]
    fn transport_loss_retains_original_intent_and_exact_retry_acknowledges_before_dismissal() {
        let (dir, session, binding, id) = fixture();
        let (intent, receipt) = withdrawal(&session, &id);
        let proof = "authored-owner-capability-never-stored";
        let mut failed = FixtureApi(|seen: RequestId, command: Command| {
            let entries = load(dir.path(), &session, &binding)?;
            ensure!(
                entries.len() == 1
                    && entries[0].request_id == seen
                    && entries[0].intent == intent
                    && entries[0].acknowledged_receipt_sha256.is_none(),
                "Intent must be durable before dispatch"
            );
            ensure!(
                serde_json::to_vec(&command)? == serde_json::to_vec(&intent.command(proof.into()))?,
                "Dispatched intent changed"
            );
            bail!("Authored transport loss after dispatch")
        });
        assert!(execute_bound(
            dir.path(),
            &mut failed,
            &session,
            &binding,
            id.clone(),
            intent.clone(),
            proof.into()
        )
        .is_err());
        let before = fs::read(journal_path(dir.path(), &session)).unwrap();
        assert!(!String::from_utf8_lossy(&before).contains(proof));
        assert!(dismiss(dir.path(), &session, &binding, &id).is_err());
        let restored = load(dir.path(), &session, &binding).unwrap().remove(0);
        assert_eq!(restored.intent, intent);
        let mut success = FixtureApi(|seen: RequestId, command: Command| {
            assert_eq!(seen, id);
            assert_eq!(
                serde_json::to_value(command).unwrap(),
                serde_json::to_value(intent.command(proof.into())).unwrap()
            );
            Ok(ResponseBody::InferenceEquivalenceSaved(Box::new(
                receipt.clone(),
            )))
        });
        execute_bound(
            dir.path(),
            &mut success,
            &session,
            &binding,
            id.clone(),
            restored.intent,
            proof.into(),
        )
        .unwrap();
        let acknowledged = load(dir.path(), &session, &binding).unwrap();
        assert_eq!(
            acknowledged[0].acknowledged_receipt_sha256,
            Some(hash(&serde_json::to_vec(&receipt).unwrap()))
        );
        execute_bound(
            dir.path(),
            &mut success,
            &session,
            &binding,
            id.clone(),
            intent.clone(),
            proof.into(),
        )
        .unwrap();
        dismiss(dir.path(), &session, &binding, &id).unwrap();
        assert!(load(dir.path(), &session, &binding).unwrap().is_empty());
    }
    #[test]
    fn changed_actor_intent_capability_or_receipt_never_overwrites_original() {
        let (dir, session, binding, id) = fixture();
        let (intent, receipt) = withdrawal(&session, &id);
        prepare(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "original-proof",
        )
        .unwrap();
        let before = fs::read(journal_path(dir.path(), &session)).unwrap();
        let mut changed = intent.clone();
        if let Intent::Withdraw { reason, .. } = &mut changed {
            reason.push_str(" changed");
        }
        assert!(prepare(
            dir.path(),
            &session,
            &binding,
            &id,
            &changed,
            "original-proof"
        )
        .is_err());
        let mut never_called = FixtureApi(|_: RequestId, _: Command| -> Result<ResponseBody> {
            panic!("Changed owner capability must fail before dispatch")
        });
        assert!(execute_bound(
            dir.path(),
            &mut never_called,
            &session,
            &binding,
            id.clone(),
            intent.clone(),
            "different-proof".into()
        )
        .is_err());
        assert!(load(dir.path(), &session, &"c".repeat(64)).is_err());
        let mut altered = receipt.clone();
        altered.report.decision.as_mut().unwrap().actor_session =
            SessionId::new("other-actor").unwrap();
        assert!(acknowledge(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "original-proof",
            &altered
        )
        .is_err());
        assert_eq!(
            fs::read(journal_path(dir.path(), &session)).unwrap(),
            before
        );
        acknowledge(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "original-proof",
            &receipt,
        )
        .unwrap();
        let after = fs::read(journal_path(dir.path(), &session)).unwrap();
        let mut altered = receipt.clone();
        altered
            .report
            .decision
            .as_mut()
            .unwrap()
            .recorded_at_unix_ms = Some(2);
        assert!(acknowledge(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "original-proof",
            &altered
        )
        .is_err());
        assert_eq!(fs::read(journal_path(dir.path(), &session)).unwrap(), after);
    }
    #[test]
    fn receipt_must_match_original_decision_and_import_content() {
        let (_, session, _, id) = fixture();
        let (intent, receipt) = withdrawal(&session, &id);
        let mutations: [fn(&mut InferenceEquivalenceReceipt); 4] = [
            |r: &mut InferenceEquivalenceReceipt| {
                r.request_id = RequestId::new("other-request").unwrap()
            },
            |r: &mut InferenceEquivalenceReceipt| r.input_sha256 = Some("a".repeat(64)),
            |r: &mut InferenceEquivalenceReceipt| {
                r.report.revision = 3;
                r.report.decision.as_mut().unwrap().revision = 3;
            },
            |r: &mut InferenceEquivalenceReceipt| {
                r.report.decision.as_mut().unwrap().note = "different reason".into()
            },
        ];
        for mutate in mutations {
            let mut changed = receipt.clone();
            mutate(&mut changed);
            assert!(intent.validate_receipt(&session, &id, &changed).is_err());
        }
        let report = pending_report();
        let import = Intent::Import {
            path: "/authored/manifest.json".into(),
            manifest_sha256: "a".repeat(64),
            expected_dossier_id: report.dossier_id.clone(),
        };
        let mut receipt = InferenceEquivalenceReceipt {
            input_sha256: Some("a".repeat(64)),
            request_id: id.clone(),
            report,
        };
        import.validate_receipt(&session, &id, &receipt).unwrap();
        receipt
            .report
            .manifest
            .subject
            .scope
            .limitations
            .push_str(" substituted");
        receipt.report.dossier_id = receipt.report.manifest.id().unwrap();
        receipt.validate().unwrap();
        assert!(import.validate_receipt(&session, &id, &receipt).is_err());
    }
    #[test]
    fn bounded_import_binds_manifest_before_dispatch_but_recovery_needs_no_source_file() {
        let (dir, session, binding, id) = fixture();
        let manifest = synthetic_manifest();
        let input = InferenceEquivalenceInput {
            subject: manifest.subject.clone(),
            evidence: manifest
                .evidence
                .iter()
                .rev()
                .map(|e| InferenceEquivalenceInputEvidence {
                    kind: e.kind,
                    path: "/authored/evidence.json".into(),
                    sha256: e.identity.sha256.clone(),
                    byte_len: e.identity.byte_len,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&input).unwrap();
        let path = dir.path().join("input.json");
        fs::write(&path, &bytes).unwrap();
        let intent = Intent::import(path.to_str().unwrap().into(), hash(&bytes)).unwrap();
        assert!(
            matches!(&intent, Intent::Import { expected_dossier_id, .. } if *expected_dossier_id == manifest.id().unwrap())
        );
        assert!(Intent::import(path.to_str().unwrap().into(), "f".repeat(64)).is_err());
        prepare(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "fixture-proof",
        )
        .unwrap();
        fs::remove_file(&path).unwrap();
        let retained = load(dir.path(), &session, &binding).unwrap().remove(0);
        prepare(
            dir.path(),
            &session,
            &binding,
            &id,
            &retained.intent,
            "fixture-proof",
        )
        .unwrap();
        assert_eq!(retained.intent, intent);
        fs::write(
            &path,
            vec![b' '; INFERENCE_EQUIVALENCE_MANIFEST_LIMIT as usize + 1],
        )
        .unwrap();
        assert!(Intent::import(path.to_str().unwrap().into(), "f".repeat(64)).is_err());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(journal_path(dir.path(), &session), &path).unwrap();
        assert!(Intent::import(path.to_str().unwrap().into(), "f".repeat(64)).is_err());
    }
    #[test]
    fn refused_or_malformed_reply_remains_unacknowledged_and_cannot_be_dismissed() {
        let (dir, session, binding, id) = fixture();
        let (intent, mut receipt) = withdrawal(&session, &id);
        receipt.report.decision.as_mut().unwrap().note = "unrelated reason".into();
        let mut api = FixtureApi(|_: RequestId, _: Command| {
            Ok(ResponseBody::InferenceEquivalenceSaved(Box::new(
                receipt.clone(),
            )))
        });
        assert!(execute_bound(
            dir.path(),
            &mut api,
            &session,
            &binding,
            id.clone(),
            intent,
            "fixture-proof".into()
        )
        .is_err());
        assert!(load(dir.path(), &session, &binding).unwrap()[0]
            .acknowledged_receipt_sha256
            .is_none());
        assert!(dismiss(dir.path(), &session, &binding, &id).is_err());
    }
    #[test]
    fn incompatible_journal_and_capacity_refuse_without_eviction_or_dispatch() {
        let (dir, session, binding, id) = fixture();
        let (intent, _) = withdrawal(&session, &id);
        prepare(
            dir.path(),
            &session,
            &binding,
            &id,
            &intent,
            "fixture-proof",
        )
        .unwrap();
        let path = journal_path(dir.path(), &session);
        let good = fs::read(&path).unwrap();
        let mut changed: serde_json::Value = serde_json::from_slice(&good).unwrap();
        changed["schema_version"] = serde_json::json!(2);
        let unsupported = serde_json::to_vec(&changed).unwrap();
        fs::write(&path, &unsupported).unwrap();
        assert!(load(dir.path(), &session, &binding).is_err());
        assert_eq!(fs::read(&path).unwrap(), unsupported);
        fs::write(&path, &good).unwrap();
        for index in 1..ENTRIES {
            prepare(
                dir.path(),
                &session,
                &binding,
                &RequestId::new(format!("request-{index}")).unwrap(),
                &intent,
                "fixture-proof",
            )
            .unwrap();
        }
        let full = fs::read(&path).unwrap();
        assert!(prepare(
            dir.path(),
            &session,
            &binding,
            &RequestId::new("overflow").unwrap(),
            &intent,
            "fixture-proof"
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), full);
        assert_eq!(load(dir.path(), &session, &binding).unwrap().len(), ENTRIES);
    }
}
