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
    let session = SessionId::new("session").unwrap();
    let intent = RemovalIntent {
        project: ProjectId::new("project").unwrap(),
        request_id: RequestId::new("removal").unwrap(),
        attempt: AttemptId::new("attempt").unwrap(),
        original_request: RequestId::new("start").unwrap(),
        observed_revision: RevisionId::new(0),
        observed_state: WasmEditState::Failed,
    };
    (dir, endpoint, session, "a".repeat(64), intent)
}
fn path(dir: &Path, session: &SessionId) -> std::path::PathBuf {
    dir.join("desktop-wasm-cleanup").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ))
}
fn report(intent: &RemovalIntent) -> WasmEditReport {
    WasmEditReport {
        attempt_id: intent.attempt.clone(),
        request_id: intent.original_request.clone(),
        project_id: intent.project.clone(),
        base_revision: intent.observed_revision,
        engine_epoch: "epoch".into(),
        state: intent.observed_state,
        candidate_id: None,
        receipt: None,
        error: Some(ProtocolError::new(
            ErrorCode::Unavailable,
            "authored failure",
        )),
    }
}
#[test]
fn restored_cleanup_is_inert_and_bound_to_exact_original_request_and_namespace() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let file = path(dir.path(), &session);
    let original = fs::read(&file).unwrap();
    let text = String::from_utf8(original.clone()).unwrap();
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
    assert_eq!(entries[0].intent, intent);
    assert!(!entries[0].acknowledged);
    assert!(load(dir.path(), &endpoint, &session, &"b".repeat(64)).is_err());
    assert!(load(
        dir.path(),
        &dir.path().join("other.sock"),
        &session,
        &binding
    )
    .is_err());
    let mut changed = intent.clone();
    changed.original_request = RequestId::new("other-start").unwrap();
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &changed).is_err());
    let mut competing = intent.clone();
    competing.request_id = RequestId::new("other-removal").unwrap();
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &competing).is_err());
    assert!(dismiss(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent.request_id
    )
    .is_err());
    assert_eq!(fs::read(file).unwrap(), original);
}
#[test]
fn only_checked_acknowledgement_can_settle_and_dismiss_cleanup() {
    let (dir, endpoint, session, binding, intent) = fixture();
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let mut wrong = report(&intent);
    wrong.base_revision = RevisionId::new(1);
    assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &wrong).is_err());
    let mut wrong = report(&intent);
    wrong.request_id = RequestId::new("wrong-start").unwrap();
    assert!(acknowledge(dir.path(), &endpoint, &session, &binding, &intent, &wrong).is_err());
    assert!(!load(dir.path(), &endpoint, &session, &binding).unwrap()[0].acknowledged);
    acknowledge(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &report(&intent),
    )
    .unwrap();
    let before = fs::read(path(dir.path(), &session)).unwrap();
    acknowledge(
        dir.path(),
        &endpoint,
        &session,
        &binding,
        &intent,
        &report(&intent),
    )
    .unwrap();
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
fn invalid_state_corrupt_unsafe_oversize_and_linked_journal_refuse_without_repair() {
    let (dir, endpoint, session, binding, mut intent) = fixture();
    intent.observed_state = WasmEditState::Completed;
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
    intent.observed_state = WasmEditState::Failed;
    prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    let file = path(dir.path(), &session);
    let original = fs::read(&file).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    assert_eq!(fs::read(&file).unwrap(), original);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
    json["schema_version"] = 99.into();
    let changed = serde_json::to_vec(&json).unwrap();
    fs::write(&file, &changed).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    assert_eq!(fs::read(&file).unwrap(), changed);
    fs::write(&file, vec![0; 1024 * 1024 + 1]).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    fs::remove_file(&file).unwrap();
    let target = dir.path().join("target");
    fs::write(&target, &original).unwrap();
    symlink(&target, &file).unwrap();
    assert!(load(dir.path(), &endpoint, &session, &binding).is_err());
    assert_eq!(fs::read(&target).unwrap(), original);
}
#[test]
fn capacity_preserves_unknown_cleanup_and_acknowledged_dismissal_reclaims_a_slot() {
    let (dir, endpoint, session, binding, mut intent) = fixture();
    for i in 0..64 {
        intent.request_id = RequestId::new(format!("request-{i}")).unwrap();
        intent.attempt = AttemptId::new(format!("attempt-{i}")).unwrap();
        prepare(dir.path(), &endpoint, &session, &binding, &intent).unwrap();
    }
    let file = path(dir.path(), &session);
    let before = fs::read(&file).unwrap();
    intent.request_id = RequestId::new("overflow").unwrap();
    intent.attempt = AttemptId::new("overflow").unwrap();
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
    assert_eq!(fs::read(&file).unwrap(), before);
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
fn concurrent_windows_merge_distinct_cleanup_requests() {
    let (dir, endpoint, session, binding, intent) = fixture();
    let start = std::sync::Barrier::new(4);
    // This bounds test supervision, not the production nonblocking lock. Each
    // holder must finish durable writes, and competing writers have no FIFO guarantee.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    std::thread::scope(|scope| {
        let threads = (0..4)
            .map(|window| {
                let dir = dir.path();
                let endpoint = &endpoint;
                let session = &session;
                let binding = &binding;
                let start = &start;
                let mut intent = intent.clone();
                scope.spawn(move || {
                    start.wait();
                    for i in 0..4 {
                        intent.request_id = RequestId::new(format!("window-{window}-{i}")).unwrap();
                        intent.attempt = AttemptId::new(format!("attempt-{window}-{i}")).unwrap();
                        loop {
                            match prepare(dir, endpoint, session, binding, &intent) {
                                Ok(()) => break,
                                Err(e) => {
                                    assert!(e.to_string().contains("busy"), "{e}");
                                    assert!(
                                        std::time::Instant::now() < deadline,
                                        "journal remained busy for window {window}, request {i}: {e}"
                                    );
                                    std::thread::sleep(std::time::Duration::from_millis(2));
                                }
                            }
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        let results = threads
            .into_iter()
            .map(|thread| thread.join())
            .collect::<Vec<_>>();
        assert!(
            results.iter().all(|result| result.is_ok()),
            "{} cleanup workers failed; all workers joined",
            results.iter().filter(|result| result.is_err()).count()
        );
    });
    let entries = load(dir.path(), &endpoint, &session, &binding).unwrap();
    assert_eq!(entries.len(), 16);
    assert!(entries.iter().all(|entry| !entry.acknowledged));
    let actual = entries
        .into_iter()
        .map(|entry| (entry.intent.request_id.clone(), entry.intent))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut expected = std::collections::BTreeMap::new();
    for window in 0..4 {
        for i in 0..4 {
            let mut expected_intent = intent.clone();
            expected_intent.request_id = RequestId::new(format!("window-{window}-{i}")).unwrap();
            expected_intent.attempt = AttemptId::new(format!("attempt-{window}-{i}")).unwrap();
            expected.insert(expected_intent.request_id.clone(), expected_intent);
        }
    }
    assert_eq!(actual, expected);
}
