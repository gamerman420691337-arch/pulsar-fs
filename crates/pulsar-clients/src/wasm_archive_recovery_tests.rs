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
    ArchiveIntent,
) {
    let dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let endpoint = dir.path().join("engine.sock");
    let session = SessionId::new("session").unwrap();
    let intent = ArchiveIntent::capture(
        &(ProjectId::new("project").unwrap(), RevisionId::new(0)),
        &authored_report(),
    )
    .unwrap();
    (dir, endpoint, session, "a".repeat(64), intent)
}
fn path(dir: &Path, session: &SessionId) -> std::path::PathBuf {
    dir.join("desktop-wasm-archive").join(format!(
        "{:x}.json",
        Sha256::digest(session.as_str().as_bytes())
    ))
}
fn report(intent: &ArchiveIntent) -> WasmEditReport {
    let mut report = authored_report();
    report.attempt_id = intent.attempt.clone();
    report.request_id = intent.original_request.clone();
    report.project_id = intent.project.clone();
    report.base_revision = intent.observed_revision;
    report.state = intent.observed_state;
    report.candidate_id = Some(intent.candidate.clone());
    report.receipt = Some(intent.receipt.clone());
    report
}
#[test]
fn restored_archive_is_inert_and_bound_to_exact_original_request_and_namespace() {
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
    assert!(entries[0].intent.matches(&intent).unwrap());
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
fn only_checked_acknowledgement_can_settle_and_dismiss_archive() {
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
    intent.observed_state = WasmEditState::Running;
    assert!(prepare(dir.path(), &endpoint, &session, &binding, &intent).is_err());
    intent.observed_state = WasmEditState::Completed;
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
fn capacity_preserves_unknown_archive_and_acknowledged_dismissal_reclaims_a_slot() {
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
fn concurrent_windows_merge_distinct_archive_requests() {
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
                    intent.attempt = AttemptId::new(format!("attempt-{window}-{i}")).unwrap();
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
