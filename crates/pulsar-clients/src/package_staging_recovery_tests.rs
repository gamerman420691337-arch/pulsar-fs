use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::PathBuf,
};

struct Fixture {
    root: tempfile::TempDir,
    parent: File,
    completed: File,
    binding: DownloadBinding,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let parent = File::open(root.path()).unwrap();
        let path = root.path().join("project.pulsar");
        fs::write(&path, vec![7; 4096]).unwrap();
        let completed = File::open(path).unwrap();
        let binding = DownloadBinding {
            credential_binding: "hashed-credential-binding".into(),
            project: ProjectId::new("project").unwrap(),
            operation: PackageOperationId::new("operation").unwrap(),
            request: RequestId::new("original-download").unwrap(),
            artifact: PackageArtifactDescriptor {
                format_version: PROJECT_PACKAGE_FORMAT_VERSION,
                sha256: format!("{:x}", Sha256::digest([7; 4096])),
                byte_len: 4096,
                manifest_sha256: "b".repeat(64),
                project_id: ProjectId::new("project").unwrap(),
                captured_revision: RevisionId::new(3),
                captured_event_cursor: 7,
                object_count: 1,
                verification: PackageReadyVerification::AllDeclaredObjectsVerified,
            },
        };
        Self {
            root,
            parent,
            completed,
            binding,
        }
    }
    fn stage(&self) -> (PathBuf, File) {
        let path = self
            .root
            .path()
            .join(format!(".pulsar-package-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = File::open(&path).unwrap();
        rustix::fs::flock(
            &directory,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .unwrap();
        let mut bundle = File::from(
            openat(
                &directory,
                "bundle",
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::from_raw_mode(0o600),
            )
            .unwrap(),
        );
        bundle.write_all(&[7; 1024]).unwrap();
        write_receipt(
            &self.parent,
            &directory,
            &bundle,
            OsStr::new("project.pulsar"),
            self.binding.clone(),
        )
        .unwrap();
        (path, directory)
    }
    fn recover(&self) -> Result<usize> {
        reconcile(
            &self.parent,
            &self.completed,
            OsStr::new("project.pulsar"),
            &self.binding,
            &PackageDownloadControl::default(),
        )
    }
    fn preserved(&self, path: &std::path::Path) {
        assert!(path.join(JOURNAL).is_file());
        assert_eq!(
            fs::read(self.root.path().join("project.pulsar")).unwrap(),
            vec![7; 4096]
        );
    }
}

#[test]
fn inactive_receipted_prefix_removed_without_changing_complete_package() {
    let f = Fixture::new();
    fs::write(f.root.path().join("intent.json"), b"original intent").unwrap();
    let (path, guard) = f.stage();
    // Declare this fixture's completed transfer inactive explicitly. Parallel
    // process-spawn tests may temporarily inherit the open-file description.
    rustix::fs::flock(&guard, rustix::fs::FlockOperation::Unlock).unwrap();
    drop(guard);
    assert_eq!(f.recover().unwrap(), 1);
    assert!(!path.exists());
    assert_eq!(
        fs::read(f.root.path().join("intent.json")).unwrap(),
        b"original intent"
    );
    assert_eq!(f.recover().unwrap(), 0);
    assert_eq!(
        fs::read(f.root.path().join("project.pulsar")).unwrap(),
        vec![7; 4096]
    );
}
#[test]
fn active_stage_lock_refuses_cleanup() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    assert!(format!("{:#}", f.recover().unwrap_err()).contains("active transfer"));
    f.preserved(&path);
    assert!(path.join("bundle").exists());
    drop(guard);
    assert_eq!(f.recover().unwrap(), 1);
}
#[test]
fn mismatched_bound_intent_refuses_all_variants() {
    for i in 0..7 {
        let mut f = Fixture::new();
        let (path, guard) = f.stage();
        drop(guard);
        match i {
            0 => f.binding.credential_binding.push('x'),
            1 => f.binding.project = ProjectId::new("other").unwrap(),
            2 => f.binding.operation = PackageOperationId::new("other").unwrap(),
            3 => f.binding.request = RequestId::new("other").unwrap(),
            4 => f.binding.artifact.sha256 = "c".repeat(64),
            5 => f.binding.artifact.captured_revision = RevisionId::new(4),
            _ => f.binding.artifact.manifest_sha256 = "c".repeat(64),
        }
        assert!(f.recover().is_err(), "variant {i}");
        f.preserved(&path);
        assert!(path.join("bundle").exists());
    }
}
#[test]
fn foreign_parent_or_destination_refuses_cleanup() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    assert!(reconcile(
        &f.parent,
        &f.completed,
        OsStr::new("other.pulsar"),
        &f.binding,
        &PackageDownloadControl::default()
    )
    .is_err());
    f.preserved(&path);
    let other = tempfile::tempdir().unwrap();
    let moved = other.path().join(path.file_name().unwrap());
    fs::rename(&path, &moved).unwrap();
    assert!(reconcile(
        &File::open(other.path()).unwrap(),
        &f.completed,
        OsStr::new("project.pulsar"),
        &f.binding,
        &PackageDownloadControl::default()
    )
    .is_err());
    assert!(moved.join("bundle").exists());
}
#[test]
fn changed_prefix_refuses_cleanup() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    fs::write(path.join("bundle"), vec![8; 1024]).unwrap();
    assert!(format!("{:#}", f.recover().unwrap_err()).contains("bytes differ"));
    f.preserved(&path);
}
#[test]
fn replaced_bundle_with_identical_bytes_refuses_cleanup() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    fs::rename(path.join("bundle"), f.root.path().join("old-bundle")).unwrap();
    fs::write(path.join("bundle"), vec![7; 1024]).unwrap();
    fs::set_permissions(path.join("bundle"), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(f.recover().is_err());
    f.preserved(&path);
    assert!(f.root.path().join("old-bundle").exists());
}
#[test]
fn symlink_hardlink_or_public_member_refuses_cleanup() {
    for i in 0..3 {
        let f = Fixture::new();
        let (path, guard) = f.stage();
        drop(guard);
        match i {
            0 => {
                fs::remove_file(path.join("bundle")).unwrap();
                symlink(f.root.path().join("project.pulsar"), path.join("bundle")).unwrap();
            }
            1 => fs::hard_link(path.join("bundle"), f.root.path().join("linked")).unwrap(),
            _ => {
                fs::set_permissions(path.join("bundle"), fs::Permissions::from_mode(0o644)).unwrap()
            }
        }
        assert!(f.recover().is_err());
        f.preserved(&path);
        assert!(path.join("bundle").exists());
    }
}
#[test]
fn unknown_member_or_missing_journal_never_removed() {
    for missing in [false, true] {
        let f = Fixture::new();
        let (path, guard) = f.stage();
        drop(guard);
        if missing {
            fs::remove_file(path.join(JOURNAL)).unwrap();
        } else {
            fs::write(path.join("unrelated"), b"keep").unwrap();
        }
        assert!(f.recover().is_err());
        assert!(path.join("bundle").exists());
        if !missing {
            assert_eq!(fs::read(path.join("unrelated")).unwrap(), b"keep");
        }
    }
}
#[test]
fn every_candidate_validated_before_any_removal() {
    let f = Fixture::new();
    let (good, guard) = f.stage();
    drop(guard);
    let (bad, guard) = f.stage();
    drop(guard);
    fs::write(bad.join("unknown"), b"keep").unwrap();
    assert!(f.recover().is_err());
    for path in [good, bad] {
        f.preserved(&path);
        assert!(path.join("bundle").exists());
    }
}
#[test]
fn cancellation_preserves_receipted_stage() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    let control = PackageDownloadControl::default();
    control.cancel();
    assert!(reconcile(
        &f.parent,
        &f.completed,
        OsStr::new("project.pulsar"),
        &f.binding,
        &control
    )
    .is_err());
    f.preserved(&path);
    assert!(path.join("bundle").exists());
}
#[test]
fn bound_metadata_only_stage_can_be_reconciled() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    fs::remove_file(path.join("bundle")).unwrap();
    assert_eq!(f.recover().unwrap(), 1);
    assert!(!path.exists());
}

#[test]
fn incorrect_complete_package_digest_preserves_partial() {
    let f = Fixture::new();
    let (path, guard) = f.stage();
    drop(guard);
    let mut changed = vec![7; 4096];
    changed[4095] = 9; // Outside the retained 1024-byte prefix: exercise the full digest guard.
    fs::write(f.root.path().join("project.pulsar"), changed).unwrap();
    assert!(format!("{:#}", f.recover().unwrap_err()).contains("digest differs"));
    assert!(path.join("bundle").exists());
    assert!(path.join(JOURNAL).exists());
}
#[test]
fn unknown_receipt_schema_or_fields_and_linked_journal_preserve_partial() {
    for i in 0..3 {
        let f = Fixture::new();
        let (path, guard) = f.stage();
        drop(guard);
        if i == 2 {
            fs::hard_link(path.join(JOURNAL), f.root.path().join("journal-link")).unwrap();
        } else {
            let mut receipt: serde_json::Value =
                serde_json::from_slice(&fs::read(path.join(JOURNAL)).unwrap()).unwrap();
            if i == 0 {
                receipt["schema"] = 2.into();
            } else {
                receipt["unexpected"] = true.into();
            }
            fs::write(path.join(JOURNAL), serde_json::to_vec(&receipt).unwrap()).unwrap();
        }
        assert!(f.recover().is_err());
        f.preserved(&path);
        assert!(path.join("bundle").exists());
    }
}

fn cleanup_record(f: &Fixture, stage: &std::path::Path) -> (cleanup::Record, PathBuf) {
    let journal = File::open(stage.join(JOURNAL)).unwrap();
    let receipt: Receipt = serde_json::from_slice(&fs::read(stage.join(JOURNAL)).unwrap()).unwrap();
    let record =
        cleanup::create(&f.parent, stage.file_name().unwrap(), &receipt, &journal).unwrap();
    let suffix = stage
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix(".pulsar-package-")
        .unwrap();
    (
        record,
        f.root
            .path()
            .join(format!(".pulsar-download-cleanup-{suffix}.json")),
    )
}
#[test]
fn every_cleanup_retirement_boundary_recovers_without_replaying_download() {
    for phase in 0..4 {
        let f = Fixture::new();
        let (stage, guard) = f.stage();
        let (record, path) = cleanup_record(&f, &stage);
        drop(guard);
        if phase >= 1 {
            fs::remove_file(stage.join("bundle")).unwrap();
        }
        if phase >= 2 {
            fs::remove_file(stage.join(JOURNAL)).unwrap();
        }
        if phase >= 3 {
            fs::remove_dir(&stage).unwrap();
        }
        drop(record);
        assert_eq!(f.recover().unwrap(), usize::from(phase < 3));
        assert!(!stage.exists() && !path.exists());
        assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), 1);
        assert_eq!(f.recover().unwrap(), 0);
    }
}
#[test]
fn empty_directory_without_parent_receipt_remains_protected() {
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    drop(guard);
    fs::remove_file(stage.join("bundle")).unwrap();
    fs::remove_file(stage.join(JOURNAL)).unwrap();
    assert!(f.recover().is_err());
    assert!(stage.is_dir());
}
#[test]
fn cleanup_receipt_lock_and_cancellation_preserve_empty_directory() {
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    let (record, path) = cleanup_record(&f, &stage);
    drop(guard);
    fs::remove_file(stage.join("bundle")).unwrap();
    fs::remove_file(stage.join(JOURNAL)).unwrap();
    assert!(f.recover().is_err());
    assert!(path.exists() && stage.exists());
    drop(record);
    let control = PackageDownloadControl::default();
    control.cancel();
    assert!(reconcile(
        &f.parent,
        &f.completed,
        OsStr::new("project.pulsar"),
        &f.binding,
        &control
    )
    .is_err());
    assert!(path.exists() && stage.exists());
    assert_eq!(f.recover().unwrap(), 1);
}
#[cfg(target_os = "linux")]
#[test]
fn ended_cleanup_owner_releases_lock_while_inherited_child_descriptor_remains() {
    use std::{
        os::fd::OwnedFd,
        os::unix::net::UnixStream,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    let (record, path) = cleanup_record(&f, &stage);
    drop(guard);
    fs::remove_file(stage.join("bundle")).unwrap();
    fs::remove_file(stage.join(JOURNAL)).unwrap();
    let (mut control, child_control) = UnixStream::pair().unwrap();
    control
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut child = OwnedChild(
        Command::new("/bin/bash")
            .env_clear()
            .args(["--noprofile", "--norc", "-c", "read -r -u 1 release"])
            .stdin(Stdio::from(record.held_file().try_clone().unwrap()))
            .stdout(Stdio::from(OwnedFd::from(child_control)))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(format!("{:#}", f.recover().unwrap_err())
        .contains("Cleanup receipt is owned by an active helper"));
    drop(record);
    // The child never reads the receipt and remains alive across owner retirement.
    let recovery = f.recover();
    let retained_child_alive = child.0.try_wait().unwrap().is_none();
    control.write_all(b"release\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.0.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "owned inherited-descriptor child did not retire"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(retained_child_alive);
    assert_eq!(recovery.unwrap(), 1);
    assert!(!stage.exists() && !path.exists());
}
#[test]
fn replacement_empty_directory_cannot_inherit_cleanup_receipt() {
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    let (record, path) = cleanup_record(&f, &stage);
    drop(record);
    drop(guard);
    let moved = f.root.path().join("unrelated-preserved");
    fs::rename(&stage, &moved).unwrap();
    fs::create_dir(&stage).unwrap();
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(f.recover().is_err());
    assert!(path.exists() && stage.is_dir());
    assert!(moved.join("bundle").exists() && moved.join(JOURNAL).exists());
}
#[test]
fn malformed_or_foreign_cleanup_receipt_refuses_before_removal() {
    for i in 0..4 {
        let mut f = Fixture::new();
        let (stage, guard) = f.stage();
        let (record, path) = cleanup_record(&f, &stage);
        drop(record);
        drop(guard);
        match i {
            0 => f.binding.request = RequestId::new("foreign").unwrap(),
            1 => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["schema"] = 2.into();
                fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            2 => {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["unexpected"] = true.into();
                fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => fs::write(&path, b"{partial").unwrap(),
        }
        assert!(f.recover().is_err());
        assert!(path.exists());
        f.preserved(&stage);
        assert!(stage.join("bundle").exists());
    }
}
#[test]
fn linked_or_public_cleanup_receipt_never_removed() {
    for i in 0..3 {
        let f = Fixture::new();
        let (stage, guard) = f.stage();
        let (record, path) = cleanup_record(&f, &stage);
        drop(record);
        drop(guard);
        match i {
            0 => {
                fs::rename(&path, f.root.path().join("original-record")).unwrap();
                symlink(f.root.path().join("original-record"), &path).unwrap();
            }
            1 => fs::hard_link(&path, f.root.path().join("linked-record")).unwrap(),
            _ => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
        }
        assert!(f.recover().is_err());
        assert!(path.exists());
        f.preserved(&stage);
    }
}
#[test]
fn orphan_cleanup_record_still_requires_complete_artifact_digest() {
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    let (record, path) = cleanup_record(&f, &stage);
    drop(record);
    drop(guard);
    fs::remove_file(stage.join("bundle")).unwrap();
    fs::remove_file(stage.join(JOURNAL)).unwrap();
    fs::remove_dir(stage).unwrap();
    fs::write(f.root.path().join("project.pulsar"), vec![9; 4096]).unwrap();
    assert!(f.recover().is_err());
    assert!(path.exists());
}
#[test]
fn invalid_candidate_preserves_already_valid_orphan_cleanup_record() {
    let f = Fixture::new();
    let (stage, guard) = f.stage();
    let (record, path) = cleanup_record(&f, &stage);
    drop(record);
    drop(guard);
    fs::remove_file(stage.join("bundle")).unwrap();
    fs::remove_file(stage.join(JOURNAL)).unwrap();
    fs::remove_dir(stage).unwrap();
    let (bad, guard) = f.stage();
    drop(guard);
    fs::write(bad.join("unknown"), b"keep").unwrap();
    assert!(f.recover().is_err());
    assert!(path.exists() && bad.join("unknown").exists());
    f.preserved(&bad);
}

#[test]
fn combined_receipt_budget_refuses_before_creating_or_removing_members() {
    let f = Fixture::new();
    let mut paths = Vec::new();
    for _ in 0..64 {
        let (stage, guard) = f.stage();
        let (record, path) = cleanup_record(&f, &stage);
        drop(record);
        drop(guard);
        fs::remove_file(stage.join("bundle")).unwrap();
        fs::remove_file(stage.join(JOURNAL)).unwrap();
        fs::remove_dir(stage).unwrap();
        paths.push(path);
    }
    let (stage, guard) = f.stage();
    drop(guard);
    let before = fs::read_dir(f.root.path()).unwrap().count();
    assert!(
        format!("{:#}", f.recover().unwrap_err()).contains("Combined cleanup receipt count budget")
    );
    assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), before);
    assert!(paths.iter().all(|p| p.exists()));
    f.preserved(&stage);
    assert!(stage.join("bundle").exists());
}
