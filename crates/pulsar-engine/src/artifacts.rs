//! Engine-owned durable artifacts. No client or worker can choose a project store path.
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "linux", target_os = "android"))]
mod pinned_export;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub use pinned_export::{
    check_publication_directory, preflight_pinned_export, publication_directory_identity,
    publish_export_pinned,
};
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn publication_directory_identity(
    _: &Path,
) -> Result<pulsar_protocol::PublicationDirectoryIdentity> {
    bail!("Pinned directory publication unavailable on this platform")
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn check_publication_directory(
    _: &Path,
    _: &pulsar_protocol::PublicationDirectoryIdentity,
) -> Result<()> {
    bail!("Pinned directory publication unavailable on this platform")
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn preflight_pinned_export(
    _: &Path,
    _: &pulsar_protocol::PublicationDirectoryIdentity,
) -> Result<()> {
    bail!("Pinned directory publication unavailable on this platform")
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn publish_export_pinned(
    _: &Path,
    _: &[u8],
    _: &pulsar_protocol::PublicationDirectoryIdentity,
) -> Result<()> {
    bail!("Pinned directory publication unavailable on this platform")
}

pub fn digest_file(path: &Path, limit: u64) -> Result<(String, u64)> {
    let mut file = open_regular(path)?;
    digest_opened(&mut file, limit)
}

fn digest_opened(file: &mut File, limit: u64) -> Result<(String, u64)> {
    if file.metadata()?.len() > limit {
        bail!("artifact exceeds admitted byte limit");
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .context("artifact size overflow")?;
        if total > limit {
            bail!("artifact grew beyond admitted byte limit");
        }
        hash.update(&buf[..n]);
    }
    Ok((format!("{:x}", hash.finalize()), total))
}

fn snapshot_unchanged(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if before.len() != after.len()
        || !matches!((before.modified(), after.modified()), (Ok(a), Ok(b)) if a == b)
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (before.ctime(), before.ctime_nsec()) != (after.ctime(), after.ctime_nsec()) {
            return false;
        }
    }
    true
}

pub fn open_regular(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::{
            os::windows::fs::OpenOptionsExt,
            path::{Component, Prefix},
        };
        let filesystem_prefix = match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => true,
                Prefix::UNC(_, share) | Prefix::VerbatimUNC(_, share) => {
                    !share.as_encoded_bytes().eq_ignore_ascii_case(b"pipe")
                }
                _ => false,
            },
            _ => false,
        };
        anyhow::ensure!(
            path.is_absolute() && filesystem_prefix,
            "artifact input requires an absolute filesystem path, not a device or pipe namespace"
        );
        const FILE_SHARE_READ: u32 = 1;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const SECURITY_ANONYMOUS: u32 = 0;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(SECURITY_ANONYMOUS);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("expected a regular artifact file");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        anyhow::ensure!(
            metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "artifact input must not be a reparse point"
        );
    }
    Ok(file)
}

#[cfg(unix)]
pub fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
pub fn sync_dir(_: &Path) -> Result<()> {
    bail!("checked artifact directory synchronization is unavailable on this platform")
}

/// Copies from an opened source, rejecting an observable concurrent modification.
/// The resulting content hash, not a mutable original path, is its identity.
pub fn snapshot(source: &Path, dir: &Path, max_bytes: u64) -> Result<(PathBuf, String, u64)> {
    snapshot_reserved(source, dir, max_bytes, 0)
}

/// Snapshot an executable while reusing independently verified existing bytes.
/// Reuse reads both files afresh; it never trusts a name or a cached digest.
pub(crate) fn snapshot_worker(
    source: &Path,
    dir: &Path,
    max_bytes: u64,
) -> Result<(PathBuf, String, u64)> {
    #[cfg(not(unix))]
    private_directory(dir)?;
    // A fresh Engine has no candidate: preserve its single capture/hash pass.
    if fs::read_dir(dir)?.next().transpose()?.is_none() {
        return snapshot(source, dir, max_bytes);
    }
    let canonical = source.canonicalize()?;
    let mut input = open_regular(&canonical)?;
    let before = input.metadata()?;
    let size = before.len();
    if size > max_bytes {
        bail!("source exceeds configured snapshot limit");
    }
    let reserve = size
        .checked_add(16 * 1024 * 1024)
        .context("storage reservation overflow")?;
    if fs2::available_space(dir)? < reserve {
        bail!("insufficient storage for immutable source snapshot");
    }
    let (hash, actual_size) = digest_opened(&mut input, max_bytes)?;
    let unchanged = |after: &fs::Metadata| snapshot_unchanged(&before, after);
    if actual_size != size || !unchanged(&input.metadata()?) {
        bail!("source changed while snapshotting");
    }
    let destination = dir.join(&hash);
    match open_regular(&destination) {
        Ok(mut existing) => {
            let existing_before = existing.metadata()?;
            let (existing_hash, existing_size) = digest_opened(&mut existing, max_bytes)?;
            let existing_after = existing.metadata()?;
            if existing_hash != hash
                || existing_size != size
                || !snapshot_unchanged(&existing_before, &existing_after)
            {
                bail!("existing snapshot failed content identity check");
            }
            if !unchanged(&input.metadata()?) {
                bail!("source changed while snapshotting");
            }
            sync_dir(dir)?;
            Ok((destination, hash, size))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            let captured = snapshot_opened_reserved(&mut input, dir, max_bytes, 0)?;
            if captured.1 != hash || captured.2 != size || !unchanged(&input.metadata()?) {
                bail!("source changed while snapshotting");
            }
            Ok(captured)
        }
        Err(error) => Err(error),
    }
}
pub fn snapshot_reserved(
    source: &Path,
    dir: &Path,
    max_bytes: u64,
    held_storage: u64,
) -> Result<(PathBuf, String, u64)> {
    #[cfg(not(unix))]
    private_directory(dir)?;
    let canonical = source.canonicalize()?;
    let mut input = open_regular(&canonical)?;
    snapshot_opened_reserved(&mut input, dir, max_bytes, held_storage)
}

/// Capture the already-authorized descriptor, without reopening a mutable path.
/// Callers own selection and namespace validation before passing the descriptor.
pub(crate) fn snapshot_opened_reserved(
    input: &mut File,
    dir: &Path,
    max_bytes: u64,
    held_storage: u64,
) -> Result<(PathBuf, String, u64)> {
    #[cfg(not(unix))]
    private_directory(dir)?;
    use std::io::{Seek, SeekFrom};
    input.seek(SeekFrom::Start(0))?;
    let before = input.metadata()?;
    if !before.is_file() {
        bail!("expected a regular snapshot input");
    }
    let size = before.len();
    if size > max_bytes {
        bail!("source exceeds configured snapshot limit");
    }
    let reserve = size
        .checked_add(16 * 1024 * 1024)
        .and_then(|size| size.checked_add(held_storage))
        .context("storage reservation overflow")?;
    if fs2::available_space(dir)? < reserve {
        bail!("insufficient storage for immutable source snapshot");
    }
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(not(target_os = "linux"))]
    let cloned = false;
    // FICLONE creates a copy-on-write extent snapshot; failure takes the copy path.
    #[cfg(target_os = "linux")]
    let cloned = {
        use std::os::fd::AsRawFd;
        (unsafe {
            libc::ioctl(
                temp.as_file().as_raw_fd(),
                0x40049409 as libc::c_ulong,
                input.as_raw_fd(),
            )
        }) == 0
    };
    if !cloned {
        let count = std::io::copy(
            &mut Read::by_ref(input).take(size.saturating_add(1)),
            &mut temp,
        )?;
        if count != size {
            bail!("source changed while snapshotting");
        }
    }
    let after = input.metadata()?;
    if !snapshot_unchanged(&before, &after) {
        bail!("source changed while snapshotting");
    }
    temp.as_file().sync_all()?;
    let (hash, actual_size) = digest_file(temp.path(), max_bytes)?;
    if actual_size != size {
        bail!("snapshot size differs from captured source");
    }
    let destination = dir.join(&hash);
    match temp.persist_noclobber(&destination) {
        Ok(file) => {
            let mut permissions = file.metadata()?.permissions();
            permissions.set_readonly(true);
            file.set_permissions(permissions)?;
            file.sync_all()?;
        }
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let (existing, existing_size) = digest_file(&destination, max_bytes)?;
            if existing != hash || existing_size != size {
                bail!("existing snapshot failed content identity check");
            }
        }
        Err(error) => return Err(error.error.into()),
    }
    sync_dir(dir)?;
    Ok((destination, hash, size))
}

/// A standard export never overwrites an existing user file.
#[cfg(not(unix))]
pub fn publish_export(_: &Path, _: &[u8]) -> Result<()> {
    bail!("checked artifact export publication and directory durability are unavailable on this platform")
}

#[cfg(unix)]
pub fn publish_export(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize()?;
    let name = path.file_name().context("export requires a filename")?;
    let destination = parent.join(name);
    if destination.exists() {
        bail!("export target already exists; choose a new path");
    }
    if fs2::available_space(&parent)? < bytes.len() as u64 + 1024 * 1024 {
        bail!("insufficient export storage");
    }
    let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(destination).map_err(|e| e.error)?;
    sync_dir(&parent)
}

#[cfg(not(unix))]
pub fn private_directory(_: &Path) -> Result<()> {
    bail!("checked private artifact directory creation and ownership are unavailable on this platform")
}

#[cfg(unix)]
pub fn private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
        }
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!("engine state path must be a real private directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            bail!("engine state directory must be owned by current user with mode 0700");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn same_length_rewrite_with_restored_mtime_is_not_unchanged() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::write(&source, b"first").unwrap();
        let before = fs::metadata(&source).unwrap();
        assert!(snapshot_unchanged(&before, &fs::metadata(&source).unwrap()));
        // Allow coarse filesystem timestamp resolution without counting a
        // fixture that failed to expose an observable change as a pass.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let after = loop {
            fs::write(&source, b"other").unwrap();
            OpenOptions::new()
                .write(true)
                .open(&source)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap()))
                .unwrap();
            let after = fs::metadata(&source).unwrap();
            if (before.ctime(), before.ctime_nsec()) != (after.ctime(), after.ctime_nsec()) {
                break after;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture has no observable change timestamp"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_eq!(fs::read(&source).unwrap(), b"other");
        assert!(
            !snapshot_unchanged(&before, &after),
            "same-length rewrite with restored mtime must refuse reuse/capture"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_artifact_privacy_and_sync_refuse_without_creating_or_changing_paths() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("new-state");
        let marker = root.path().join("marker");
        fs::write(&marker, b"original").unwrap();
        assert!(private_directory(&missing).is_err());
        assert!(!missing.exists());
        assert!(private_directory(root.path()).is_err());
        assert!(sync_dir(root.path()).is_err());
        assert_eq!(fs::read(&marker).unwrap(), b"original");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn windows_artifact_publication_refuses_before_snapshot_or_atomic_write() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let store = root.path().join("store");
        let destination = root.path().join("output");
        fs::write(&source, b"authored input").unwrap();
        assert!(snapshot(&source, &store, 1024).is_err());
        assert!(!store.exists());
        assert!(publish_export(&destination, b"new data").is_err());
        assert!(!destination.exists());
        fs::create_dir(&store).unwrap();
        assert!(snapshot_worker(&source, &store, 1024).is_err());
        assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
        assert_eq!(fs::read(&source).unwrap(), b"authored input");
    }
    #[cfg(windows)]
    #[test]
    fn windows_artifact_reads_hold_exact_bytes_and_refuse_writer_reparse_and_namespace_inputs() {
        use std::os::windows::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let alias = root.path().join("alias");
        let link = root.path().join("link");
        let bytes = b"authored input";
        fs::write(&source, bytes).unwrap();
        fs::hard_link(&source, &alias).unwrap();
        let writer = OpenOptions::new().write(true).open(&source).unwrap();
        assert!(open_regular(&source).is_err());
        drop(writer);
        let mut retained = open_regular(&source).unwrap();
        assert!(OpenOptions::new().write(true).open(&alias).is_err());
        assert!(fs::remove_file(&source).is_err());
        assert!(fs::rename(&source, root.path().join("replacement")).is_err());
        let mut content = Vec::new();
        retained.read_to_end(&mut content).unwrap();
        assert_eq!(content, bytes);
        drop(retained);
        assert_eq!(
            digest_file(&source, 1024).unwrap(),
            (format!("{:x}", Sha256::digest(bytes)), bytes.len() as u64)
        );
        assert!(digest_file(&source, 1).is_err());
        std::os::windows::fs::symlink_file(&source, &link).unwrap();
        assert_ne!(
            fs::symlink_metadata(&link).unwrap().file_attributes() & 0x400,
            0
        );
        assert_eq!(fs::read(&link).unwrap(), bytes);
        assert!(open_regular(&link).is_err());
        assert!(open_regular(root.path()).is_err());
        for path in [
            r"\\.\pipe\pulsar-unused",
            r"\\localhost\PIPE\pulsar-unused",
            r"\\?\GLOBALROOT\Device\unused",
            "relative",
        ] {
            assert!(open_regular(Path::new(path))
                .err()
                .unwrap()
                .to_string()
                .contains("filesystem path"));
        }
        fs::write(&source, b"released").unwrap();
        assert_eq!(fs::read(&alias).unwrap(), b"released");
    }

    #[test]
    fn worker_reuse_rehashes_source_and_rejects_corrupted_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("worker");
        let snapshots = dir.path().join("snapshots");
        fs::create_dir(&snapshots).unwrap();
        fs::write(&source, b"first").unwrap();
        let first = snapshot_worker(&source, &snapshots, 100).unwrap();
        fs::write(&source, b"other").unwrap();
        let second = snapshot_worker(&source, &snapshots, 100).unwrap();
        assert_ne!(first.1, second.1);
        assert_eq!(fs::read(&first.0).unwrap(), b"first");
        assert_eq!(fs::read(&second.0).unwrap(), b"other");
        #[cfg(windows)]
        {
            let mut permissions = fs::metadata(&second.0).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&second.0, permissions).unwrap();
        }
        fs::remove_file(&second.0).unwrap();
        fs::write(&second.0, b"wrong").unwrap();
        let error = snapshot_worker(&source, &snapshots, 100).unwrap_err();
        assert!(error
            .to_string()
            .contains("existing snapshot failed content identity check"));
        assert_eq!(fs::read(&second.0).unwrap(), b"wrong");
        assert!(snapshot_worker(&source, &snapshots, 4).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn worker_reuse_refuses_symlink_at_content_address() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("worker");
        let snapshots = dir.path().join("snapshots");
        fs::create_dir(&snapshots).unwrap();
        fs::write(&source, b"worker").unwrap();
        let first = snapshot_worker(&source, &snapshots, 100).unwrap();
        fs::remove_file(&first.0).unwrap();
        std::os::unix::fs::symlink(&source, &first.0).unwrap();
        assert!(snapshot_worker(&source, &snapshots, 100).is_err());
        assert!(fs::symlink_metadata(&first.0)
            .unwrap()
            .file_type()
            .is_symlink());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn repeated_worker_snapshot_creates_no_redundant_temporary_copy() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("worker");
        let snapshots = dir.path().join("snapshots");
        fs::create_dir(&snapshots).unwrap();
        fs::write(&source, b"authored executable identity").unwrap();
        let first = snapshot_worker(&source, &snapshots, 100).unwrap();
        let descriptor = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        assert!(descriptor >= 0);
        let watcher = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let path = std::ffi::CString::new(snapshots.as_os_str().as_bytes()).unwrap();
        assert!(
            unsafe { libc::inotify_add_watch(watcher.as_raw_fd(), path.as_ptr(), libc::IN_CREATE) }
                >= 0
        );
        assert_eq!(snapshot_worker(&source, &snapshots, 100).unwrap(), first);
        let mut events = [0u8; 4096];
        let count = unsafe {
            libc::read(
                watcher.as_raw_fd(),
                events.as_mut_ptr().cast(),
                events.len(),
            )
        };
        assert_eq!(
            count, -1,
            "repeated snapshot created another temporary copy"
        );
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    #[test]
    fn immutable_copy_and_non_destructive_export() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("original");
        fs::write(&source, b"source").unwrap();
        let snapshots = dir.path().join("snapshots");
        fs::create_dir(&snapshots).unwrap();
        let (copy, hash, size) = snapshot(&source, &snapshots, 100).unwrap();
        fs::write(&source, b"changed").unwrap();
        assert_eq!(fs::read(&copy).unwrap(), b"source");
        assert_eq!(size, 6);
        assert_eq!(hash.len(), 64);
        let export = dir.path().join("out");
        publish_export(&export, b"first").unwrap();
        assert!(publish_export(&export, b"second").is_err());
        assert_eq!(fs::read(export).unwrap(), b"first");
    }
    #[test]
    fn descriptor_capture_does_not_reopen_a_replaced_source_path() {
        use std::io::{Seek, SeekFrom};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source");
        fs::write(&path, b"selected bytes").unwrap();
        let mut input = open_regular(&path).unwrap();
        input.seek(SeekFrom::Start(3)).unwrap();
        fs::rename(&path, directory.path().join("original")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        let (snapshot, _, bytes) =
            snapshot_opened_reserved(&mut input, directory.path(), 100, 0).unwrap();
        assert_eq!(bytes, 14);
        assert_eq!(fs::read(snapshot).unwrap(), b"selected bytes");
    }
    #[test]
    fn refuses_snapshot_budget() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("source");
        fs::write(&p, b"123").unwrap();
        assert!(snapshot(&p, dir.path(), 2).is_err());
    }
}
