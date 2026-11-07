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
    Intent,
) {
    let dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let endpoint = dir.path().join("engine.sock");
    let session = SessionId::new("session").unwrap();
    let module = b"\0asm\x01\0\0\0".to_vec();
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("module").unwrap(),
        sha256: format!("{:x}", Sha256::digest(&module)),
        byte_len: module.len() as u64,
    };
    let spec = WasmEditSpec {
        manifest: ExtensionManifest {
            schema_version: PROTOCOL_VERSION,
            name: "fixture".into(),
            role: ExtensionRole::Editor,
            artifact: identity.clone(),
            abi: WASM_PROCESSING_ABI.into(),
            requested_scopes: vec![],
            confinement: ConfinementRequirement::Wasm,
            budget: ResourceBudget {
                memory_bytes: 256 * 1024 * 1024,
                output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                wall_time_ms: 5000,
                cpu_threads: 2,
            },
        },
        module,
        guest_memory_bytes: 65536,
        expected_worker: identity,
        approve_execution: true,
        label: "proposal".into(),
    };
    let intent = Intent::capture(
        ProjectId::new("project").unwrap(),
        RevisionId::new(0),
        RequestId::new("request").unwrap(),
        &spec,
    )
    .unwrap();
    (dir, endpoint, session, "a".repeat(64), intent)
}
fn path(dir: &Path, session: &SessionId) -> std::path::PathBuf {
    dir.join("desktop-wasm-recovery").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ))
}
fn report(intent: &Intent) -> WasmEditReport {
    WasmEditReport {
        attempt_id: AttemptId::new("attempt").unwrap(),
        request_id: intent.request_id.clone(),
        project_id: intent.project.clone(),
        base_revision: intent.revision,
        engine_epoch: "epoch".into(),
        state: WasmEditState::Running,
        candidate_id: None,
        receipt: None,
        error: None,
    }
}
#[test]
fn persisted_facts_restore_no_bytes_credentials_or_execution_approval() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let bytes = fs::read(path(dir.path(), &session)).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for forbidden in [
        "approve_execution",
        "auth_token",
        "owner_proof",
        "module_bytes",
        "input_bytes",
    ] {
        assert!(!text.contains(forbidden));
    }
    let entries = load(dir.path(), &endpoint, &session, &binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].attempt.is_none());
    assert!(load(dir.path(), &endpoint, &session, &"b".repeat(64)).is_err());
    assert_eq!(fs::read(path(dir.path(), &session)).unwrap(), bytes);
    assert!(load(
        dir.path(),
        &dir.path().join("other.sock"),
        &session,
        &binding
    )
    .is_err());
    assert!(dismiss(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent.request_id
    )
    .is_err());
    let mut changed = intent.clone();
    changed.label = "different".into();
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &changed).is_err());
    assert_eq!(fs::read(path(dir.path(), &session)).unwrap(), bytes);
}
#[test]
fn acknowledgement_reconciliation_and_dismissal_bind_original_attempt() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let mut wrong = report(&intent);
    wrong.base_revision = RevisionId::new(3);
    assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &wrong).is_err());
    reconcile(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &WasmEditCatalog {
            project_id: intent.project.clone(),
            operations: vec![],
        },
    )
    .unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).unwrap()[0]
        .attempt
        .is_none());
    let correct = report(&intent);
    reconcile(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &WasmEditCatalog {
            project_id: intent.project.clone(),
            operations: vec![correct.clone()],
        },
    )
    .unwrap();
    acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &correct).unwrap();
    let before = fs::read(path(dir.path(), &session)).unwrap();
    let mut changed = correct;
    changed.attempt_id = AttemptId::new("other").unwrap();
    assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &changed).is_err());
    assert_eq!(fs::read(path(dir.path(), &session)).unwrap(), before);
    dismiss(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent.request_id,
    )
    .unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding)
        .unwrap()
        .is_empty());
}
#[test]
fn invalid_layout_symlink_nonregular_and_oversize_preserve_original_storage() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let file = path(dir.path(), &session);
    let original = fs::read(&file).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
    json["schema_version"] = 99.into();
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    let changed = fs::read(&file).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    assert_eq!(fs::read(&file).unwrap(), changed);
    fs::write(&file, vec![0; 1024 * 1024 + 1]).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    fs::remove_file(&file).unwrap();
    let target = dir.path().join("target");
    fs::write(&target, &original).unwrap();
    symlink(&target, &file).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
}
#[test]
fn full_journal_retains_unknown_intents_and_requires_acknowledged_dismissal() {
    let (dir, endpoint, session, binding, mut intent) = fixture();
    for i in 0..64 {
        intent.request_id = RequestId::new(format!("request-{i}")).unwrap();
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    }
    let file = path(dir.path(), &session);
    let original = fs::read(&file).unwrap();
    intent.request_id = RequestId::new("overflow").unwrap();
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    let first = load(dir.path(), &endpoint, &session, &binding).unwrap()[0]
        .intent
        .clone();
    acknowledge(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &first,
        &report(&first),
    )
    .unwrap();
    dismiss(dir.path(), &endpoint, &session, &binding, &first.request_id).unwrap();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    assert_eq!(
        load(dir.path(), &endpoint, &session, &binding)
            .unwrap()
            .len(),
        64
    );
}
#[test]
fn concurrent_windows_merge_without_losing_request_identities() {
    let (dir, endpoint, session, binding, intent) = fixture();
    let threads = (0..4)
        .map(|window| {
            let dir = dir.path().to_path_buf();
            let endpoint = endpoint.clone();
            let session = session.clone();
            let binding = binding.clone();
            let mut intent = intent.clone();
            std::thread::spawn(move || {
                for i in 0..4 {
                    intent.request_id = RequestId::new(format!("window-{window}-{i}")).unwrap();
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    loop {
                        match prepare(&dir, &endpoint, &session, &binding, &intent) {
                            Ok(()) => break,
                            Err(e) => {
                                assert!(e.to_string().contains("busy"), "{e}");
                                assert!(
                                    std::time::Instant::now() < deadline,
                                    "journal remained busy"
                                );
                                std::thread::sleep(std::time::Duration::from_millis(2));
                            }
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        load(dir.path(), &endpoint, &session, &binding)
            .unwrap()
            .len(),
        16
    );
}

#[test]
fn only_checked_unpublished_withdrawal_reclaims_unknown_facts_and_survives_ack_gap() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let resolution = WasmStartResolution {
        project_id: intent.project.clone(),
        reference: intent.reference().unwrap(),
        outcome: WasmStartOutcome::Withdrawn,
    };
    let file = path(dir.path(), &session);
    let original = fs::read(&file).unwrap();
    let mut wrong = resolution.clone();
    wrong.reference.label = "wrong".into();
    assert!(withdraw(dir.path(), &endpoint, &session, &binding, &intent, &wrong).is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(withdraw(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &resolution
    )
    .is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let restored = load(dir.path(), &endpoint, &session, &binding).unwrap();
    assert_eq!(restored.len(), 1);
    assert!(restored[0].attempt.is_none());
    let id = intent
        .reference()
        .unwrap()
        .resolution_request_id(&intent.project)
        .unwrap();
    assert_eq!(
        id,
        restored[0]
            .intent
            .reference()
            .unwrap()
            .resolution_request_id(&intent.project)
            .unwrap()
    );
    withdraw(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &resolution,
    )
    .unwrap();
    withdraw(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &resolution,
    )
    .unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding)
        .unwrap()
        .is_empty());
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    acknowledge(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &report(&intent),
    )
    .unwrap();
    assert!(withdraw(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &resolution
    )
    .is_err());
}
