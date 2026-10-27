//! Linux publishes only fully written receipts; handled failure removes the held name.
use super::*;

struct Pending<'a> {
    directory: &'a File,
    name: &'a OsStr,
    file: &'a File,
    armed: bool,
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if self.armed
            && private_regular(self.file, MAX_JOURNAL).is_ok()
            && named_matches(self.directory, self.name, self.file).is_ok()
            && unlinkat(self.directory, self.name, AtFlags::empty()).is_ok()
        {
            let _ = self.directory.sync_all();
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn create(
    directory: &File,
    name: &OsStr,
    bytes: &[u8],
    outer_parent: Option<&File>,
) -> Result<File> {
    ensure!(
        bytes.len() as u64 <= MAX_JOURNAL,
        "Receipt write exceeds local bound"
    );
    let file = File::from(openat(
        directory,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    let mut pending = Pending {
        directory,
        name,
        file: &file,
        armed: true,
    };
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
    let mut writer = &file;
    writer.write_all(bytes)?;
    file.sync_all()?;
    directory.sync_all()?;
    if let Some(parent) = outer_parent {
        parent.sync_all()?;
    }
    named_matches(directory, name, &file)?;
    private_regular(&file, MAX_JOURNAL)?;
    pending.armed = false;
    drop(pending);
    Ok(file)
}

#[cfg(target_os = "linux")]
fn unnamed(directory: &File) -> Result<File> {
    let file = File::from(
        openat(
            directory,
            ".",
            OFlags::RDWR | OFlags::TMPFILE | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .context("Atomic receipt creation requires filesystem O_TMPFILE support")?,
    );
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.nlink() == 0
            && m.uid() == rustix::process::geteuid().as_raw()
            && m.mode() & 0o077 == 0
            && m.len() == 0,
        "Unsafe unnamed receipt"
    );
    Ok(file)
}

#[cfg(target_os = "linux")]
pub(super) fn create(
    directory: &File,
    name: &OsStr,
    bytes: &[u8],
    outer_parent: Option<&File>,
) -> Result<File> {
    use std::os::fd::AsRawFd;
    ensure!(
        bytes.len() as u64 <= MAX_JOURNAL,
        "Receipt write exceeds local bound"
    );
    let file = unnamed(directory)?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
    (&file).write_all(bytes)?;
    file.sync_all()?;
    // The fd remains held across publication; procfs avoids requiring elevated
    // AT_EMPTY_PATH capability. linkat refuses every existing destination.
    let proc_fd = format!("/proc/self/fd/{}", file.as_raw_fd());
    rustix::fs::linkat(
        rustix::fs::CWD,
        proc_fd,
        directory,
        name,
        AtFlags::SYMLINK_FOLLOW,
    )
    .context("Atomic receipt publication requires procfs and no existing receipt")?;
    let mut pending = Pending {
        directory,
        name,
        file: &file,
        armed: true,
    };
    named_matches(directory, name, &file)?;
    private_regular(&file, MAX_JOURNAL)?;
    directory.sync_all()?;
    if let Some(parent) = outer_parent {
        parent.sync_all()?;
    }
    named_matches(directory, name, &file)?;
    private_regular(&file, MAX_JOURNAL)?;
    pending.armed = false;
    drop(pending);
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    #[cfg(target_os = "linux")]
    #[test]
    fn unnamed_partial_has_no_directory_entry_and_disappears_on_close() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        let file = unnamed(&dir).unwrap();
        (&file).write_all(b"{partial").unwrap();
        assert_eq!(file.metadata().unwrap().nlink(), 0);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        drop(file);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn invalid_parent_refuses_without_named_receipt() {
        let file = File::open("/dev/null").unwrap();
        assert!(create(&file, OsStr::new("receipt"), b"inert", None).is_err());
    }
    #[test]
    fn successful_receipt_is_private_durable_and_locked() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        let file = create(&dir, OsStr::new("receipt"), b"inert metadata", None).unwrap();
        assert_eq!(
            fs::read(root.path().join("receipt")).unwrap(),
            b"inert metadata"
        );
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
        let other = File::open(root.path().join("receipt")).unwrap();
        assert!(
            rustix::fs::flock(&other, rustix::fs::FlockOperation::NonBlockingLockExclusive)
                .is_err()
        );
        drop(file);
        rustix::fs::flock(&other, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    }
    #[test]
    fn existing_file_and_symlink_refuse_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        fs::write(root.path().join("original"), b"keep").unwrap();
        symlink(root.path().join("original"), root.path().join("linked")).unwrap();
        for name in ["original", "linked"] {
            assert!(create(&dir, OsStr::new(name), b"new", None).is_err());
        }
        assert_eq!(fs::read(root.path().join("original")).unwrap(), b"keep");
        assert!(root.path().join("linked").is_symlink());
    }
    #[test]
    fn outer_sync_failure_removes_only_held_receipt() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        let bad_parent = File::open("/dev/null").unwrap();
        fs::write(root.path().join("unrelated"), b"keep").unwrap();
        assert!(create(&dir, OsStr::new("receipt"), b"inert", Some(&bad_parent)).is_err());
        assert!(!root.path().join("receipt").exists());
        assert_eq!(fs::read(root.path().join("unrelated")).unwrap(), b"keep");
    }
    fn unfinished(root: &std::path::Path) -> File {
        File::from(
            openat(
                File::open(root).unwrap(),
                "receipt",
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::from_raw_mode(0o600),
            )
            .unwrap(),
        )
    }
    #[test]
    fn zero_and_partial_uncommitted_receipts_are_removed() {
        for bytes in [b"".as_slice(), b"{partial".as_slice()] {
            let root = tempfile::tempdir().unwrap();
            let dir = File::open(root.path()).unwrap();
            let file = unfinished(root.path());
            (&file).write_all(bytes).unwrap();
            {
                let _pending = Pending {
                    directory: &dir,
                    name: OsStr::new("receipt"),
                    file: &file,
                    armed: true,
                };
            }
            assert!(!root.path().join("receipt").exists());
        }
    }
    #[test]
    fn replaced_name_and_renamed_original_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        let file = unfinished(root.path());
        (&file).write_all(b"{partial").unwrap();
        let pending = Pending {
            directory: &dir,
            name: OsStr::new("receipt"),
            file: &file,
            armed: true,
        };
        fs::rename(
            root.path().join("receipt"),
            root.path().join("renamed-original"),
        )
        .unwrap();
        fs::write(root.path().join("receipt"), b"unrelated replacement").unwrap();
        drop(pending);
        assert_eq!(
            fs::read(root.path().join("receipt")).unwrap(),
            b"unrelated replacement"
        );
        assert_eq!(
            fs::read(root.path().join("renamed-original")).unwrap(),
            b"{partial"
        );
    }
    #[test]
    fn hard_link_or_public_mode_prevents_cleanup() {
        for hard_link in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let dir = File::open(root.path()).unwrap();
            let file = unfinished(root.path());
            (&file).write_all(b"keep").unwrap();
            let pending = Pending {
                directory: &dir,
                name: OsStr::new("receipt"),
                file: &file,
                armed: true,
            };
            if hard_link {
                fs::hard_link(root.path().join("receipt"), root.path().join("linked")).unwrap();
            } else {
                fs::set_permissions(
                    root.path().join("receipt"),
                    fs::Permissions::from_mode(0o644),
                )
                .unwrap();
            }
            drop(pending);
            assert_eq!(fs::read(root.path().join("receipt")).unwrap(), b"keep");
        }
    }
    #[test]
    fn receipt_bound_precedes_creation() {
        let root = tempfile::tempdir().unwrap();
        let dir = File::open(root.path()).unwrap();
        assert!(create(
            &dir,
            OsStr::new("receipt"),
            &vec![0; MAX_JOURNAL as usize + 1],
            None
        )
        .is_err());
        assert!(!root.path().join("receipt").exists());
    }
}
