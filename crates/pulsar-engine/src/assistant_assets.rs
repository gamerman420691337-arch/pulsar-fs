//! Sealed owned worker/runtime/model bytes. Caller owns admission and policy.
use super::{check, failure};
use pulsar_protocol::{ErrorCode, ProtocolError};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::Path,
    process::Command,
    sync::atomic::AtomicBool,
    time::Instant,
};
const SEALS: i32 = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
pub(super) struct Source {
    file: File,
    metadata: std::fs::Metadata,
}
pub(super) struct Image {
    file: File,
    pub sha256: String,
    pub byte_len: u64,
}
impl Source {
    pub fn open(
        path: &Path,
        expected: &pulsar_protocol::ArtifactIdentity,
        executable: bool,
    ) -> Result<Self, ProtocolError> {
        expected.validate()?;
        let file =
            crate::artifacts::open_regular(path).map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let metadata = file.metadata().map_err(io_error)?;
        let maximum = if executable {
            128 * 1024 * 1024
        } else {
            2 * 1024 * 1024 * 1024
        };
        if metadata.len() == 0
            || metadata.len() > maximum
            || metadata.len() != expected.byte_len
            || (executable && metadata.mode() & 0o111 == 0)
        {
            return Err(failure(
                ErrorCode::InvalidRequest,
                "assistant native executable size or mode refused",
            ));
        }
        Ok(Self { file, metadata })
    }
    pub fn byte_len(&self) -> u64 {
        self.metadata.len()
    }
    /// The caller reserves this full length before allocating the anonymous image.
    /// Deadline checks bound work between reads; filesystem syscalls are not preemptible.
    pub fn capture(
        mut self,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<Image, ProtocolError> {
        check(deadline, cancel)?;
        let fd = unsafe {
            libc::memfd_create(
                c"pulsar-assistant-asset".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let mut hash = sha2::Sha256::new();
        use sha2::Digest;
        let mut copied = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            check(deadline, cancel)?;
            let n = self.file.read(&mut buffer).map_err(io_error)?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > self.byte_len() {
                return Err(failure(
                    ErrorCode::DependencyMismatch,
                    "assistant executable grew during capture",
                ));
            }
            file.write_all(&buffer[..n]).map_err(io_error)?;
            hash.update(&buffer[..n]);
        }
        let after = self.file.metadata().map_err(io_error)?;
        if copied != self.byte_len()
            || after.len() != self.byte_len()
            || after.mtime() != self.metadata.mtime()
            || after.mtime_nsec() != self.metadata.mtime_nsec()
            || after.ctime() != self.metadata.ctime()
            || after.ctime_nsec() != self.metadata.ctime_nsec()
            || after.mode() != self.metadata.mode()
        {
            return Err(failure(
                ErrorCode::DependencyMismatch,
                "assistant executable changed during capture",
            ));
        }
        if unsafe { libc::fchmod(fd, 0o500) } != 0
            || unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, SEALS) } != 0
        {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        check(deadline, cancel)?;
        Ok(Image {
            file,
            sha256: format!("{:x}", hash.finalize()),
            byte_len: copied,
        })
    }
}
fn io_error(e: std::io::Error) -> ProtocolError {
    failure(ErrorCode::Unavailable, e)
}
impl Image {
    pub fn fd(&self) -> i32 {
        self.file.as_raw_fd()
    }
    pub fn inherit(&self, command: &mut Command) -> Result<(), ProtocolError> {
        let fd = self.file.as_raw_fd();
        if fd >= 128 || unsafe { libc::fcntl(fd, libc::F_GET_SEALS) } != SEALS {
            return Err(failure(
                ErrorCode::Unavailable,
                "assistant sealed executable descriptor refused",
            ));
        }
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::{ArtifactId, ArtifactIdentity};
    use std::{
        os::unix::fs::{symlink, PermissionsExt},
        time::Duration,
    };
    fn source(root: &Path) -> (std::path::PathBuf, ArtifactIdentity) {
        let path = root.join("asset");
        std::fs::write(&path, b"fixture immutable bytes").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        use sha2::Digest;
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture").unwrap(),
            sha256: format!("{:x}", sha2::Sha256::digest(b"fixture immutable bytes")),
            byte_len: 23,
        };
        (path, identity)
    }
    #[test]
    fn captured_bytes_survive_replacement_and_are_write_and_resize_sealed() {
        let root = tempfile::tempdir().unwrap();
        let (path, identity) = source(root.path());
        let mut image = Source::open(&path, &identity, true)
            .unwrap()
            .capture(
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        std::fs::rename(&path, root.path().join("prior")).unwrap();
        std::fs::write(&path, b"other").unwrap();
        std::fs::write(root.path().join("prior"), b"changed original").unwrap();
        let mut bytes = Vec::new();
        image.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"fixture immutable bytes");
        assert!(image.file.write_all(b"bad").is_err());
        assert!(image.file.set_len(0).is_err());
        assert_eq!(unsafe { libc::fcntl(image.fd(), libc::F_GET_SEALS) }, SEALS);
    }
    #[test]
    fn refusal_covers_path_kind_identity_metadata_cancel_and_deadline() {
        let root = tempfile::tempdir().unwrap();
        let (path, mut identity) = source(root.path());
        let alias = root.path().join("alias");
        symlink(&path, &alias).unwrap();
        assert!(Source::open(&alias, &identity, true).is_err());
        identity.byte_len += 1;
        assert!(Source::open(&path, &identity, true).is_err());
        identity.byte_len -= 1;
        let opened = Source::open(&path, &identity, true).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert!(opened
            .capture(
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false)
            )
            .is_err());
        let (path, identity) = source(root.path());
        assert_eq!(
            Source::open(&path, &identity, true)
                .unwrap()
                .capture(
                    Instant::now() + Duration::from_secs(2),
                    &AtomicBool::new(true)
                )
                .err()
                .unwrap()
                .code,
            ErrorCode::Cancelled
        );
        assert!(Source::open(&path, &identity, true)
            .unwrap()
            .capture(Instant::now(), &AtomicBool::new(false))
            .is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Source::open(&path, &identity, true).is_err());
        assert!(Source::open(&path, &identity, false).is_ok());
    }
}
