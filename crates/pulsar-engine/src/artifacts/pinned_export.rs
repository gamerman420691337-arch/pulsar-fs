//! Existing-directory publication from an anonymous inode, with no replace or temp-name race.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::PublicationDirectoryIdentity;
use rustix::fs::{linkat, open, openat, statat, AtFlags, Mode, OFlags, CWD};
use std::{
    fs::File,
    io::Write,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};

fn identity(file: &File) -> Result<PublicationDirectoryIdentity> {
    let meta = file.metadata()?;
    ensure!(meta.is_dir(), "Publication directory is not a directory");
    Ok(PublicationDirectoryIdentity {
        device: meta.dev(),
        inode: meta.ino(),
    })
}

fn directory(path: &Path) -> Result<File> {
    Ok(File::from(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}

pub fn publication_directory_identity(path: &Path) -> Result<PublicationDirectoryIdentity> {
    identity(&directory(path)?)
}

fn checked_directory(path: &Path, expected: &PublicationDirectoryIdentity) -> Result<File> {
    let dir = directory(path)?;
    ensure!(
        identity(&dir)? == *expected,
        "Publication directory identity changed"
    );
    Ok(dir)
}

pub fn check_publication_directory(
    path: &Path,
    expected: &PublicationDirectoryIdentity,
) -> Result<()> {
    checked_directory(path, expected).map(|_| ())
}

pub fn preflight_pinned_export(path: &Path, expected: &PublicationDirectoryIdentity) -> Result<()> {
    let parent = path.parent().context("Export parent absent")?;
    let dir = checked_directory(parent, expected)?;
    let name = path.file_name().context("Export filename absent")?;
    match statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => anyhow::bail!("Export leaf already exists; nothing overwritten"),
    }
}

pub fn publish_export_pinned(
    path: &Path,
    bytes: &[u8],
    expected: &PublicationDirectoryIdentity,
) -> Result<()> {
    let parent = path.parent().context("Export parent absent")?;
    let name = path.file_name().context("Export filename absent")?;
    let dir = checked_directory(parent, expected)?;
    let mut file = File::from(openat(
        &dir,
        ".",
        OFlags::WRONLY | OFlags::TMPFILE | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    file.write_all(bytes)?;
    file.sync_all()?;
    check_publication_directory(parent, expected)?;
    // /proc/self/fd refers to this still-open anonymous inode. The link operation
    // fails if any target entry exists; no pathname-selected temporary is renamed.
    let opened = format!("/proc/self/fd/{}", file.as_raw_fd());
    linkat(CWD, opened.as_str(), &dir, name, AtFlags::SYMLINK_FOLLOW)?;
    dir.sync_all()?;
    check_publication_directory(parent, expected)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_export_preserves_existing_leaves_and_refuses_replaced_directory() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("bundle");
        std::fs::create_dir(&destination).unwrap();
        let id = publication_directory_identity(&destination).unwrap();
        let protected = destination.join("stroke.funscript");
        std::fs::write(&protected, b"protected").unwrap();
        assert!(preflight_pinned_export(&protected, &id).is_err());
        assert!(publish_export_pinned(&protected, b"replacement", &id).is_err());
        publish_export_pinned(&destination.join("roll.funscript"), b"new", &id).unwrap();
        assert_eq!(std::fs::read(&protected).unwrap(), b"protected");
        let retained = root.path().join("retained");
        std::fs::rename(&destination, &retained).unwrap();
        std::fs::create_dir(&destination).unwrap();
        assert!(
            publish_export_pinned(&destination.join("pitch.funscript"), b"wrong", &id).is_err()
        );
        assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
        assert_eq!(std::fs::read_dir(&retained).unwrap().count(), 2);
    }
}
