//! Immutable native executable bytes for one WASM attempt; no runtime closure or grant.
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
    pub fn open(path: &Path) -> Result<Self, ProtocolError> {
        let file =
            crate::artifacts::open_regular(path).map_err(|e| failure(ErrorCode::Unavailable, e))?;
        let metadata = file.metadata().map_err(io_error)?;
        if !super::executable_size_admitted(metadata.len()) || metadata.mode() & 0o111 == 0 {
            return Err(failure(
                ErrorCode::InvalidRequest,
                "WASM native executable size or mode refused",
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
                c"pulsar-wasm-executable".as_ptr(),
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
                    "WASM executable grew during capture",
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
                "WASM executable changed during capture",
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
                "WASM sealed executable descriptor refused",
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
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;
    fn source(root: &Path) -> PathBuf {
        let path = root.join("worker");
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[test]
    fn sealed_image_survives_source_rewrite_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = source(root.path());
        let bytes = std::fs::read(&path).unwrap();
        let opened = Source::open(&path).unwrap();
        let mut image = opened
            .capture(
                Instant::now() + std::time::Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        std::fs::rename(&path, root.path().join("original")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        std::fs::write(root.path().join("original"), b"later rewrite").unwrap();
        let mut actual = Vec::new();
        image.file.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        assert_eq!(image.sha256, super::super::digest(&bytes));
        assert_eq!(image.byte_len, bytes.len() as u64);
        assert!(image.file.write_all(b"bad").is_err());
        assert!(image.file.set_len(0).is_err());
        assert_eq!(
            unsafe { libc::fcntl(image.file.as_raw_fd(), libc::F_GET_SEALS) },
            SEALS
        );
    }
    #[test]
    fn changed_source_and_invalid_inputs_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = source(root.path());
        let opened = Source::open(&path).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert!(opened
            .capture(
                Instant::now() + std::time::Duration::from_secs(2),
                &AtomicBool::new(false)
            )
            .is_err());
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(Source::open(&link).is_err());
        assert!(Source::open(root.path()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(Source::open(&path).is_err());
        let path = source(root.path());
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(crate::wasm_launch::MAX_EXECUTABLE_BYTES + 1)
            .unwrap();
        assert!(Source::open(&path).is_err());
    }
    #[test]
    fn capture_obeys_cancellation_and_original_deadline() {
        let root = tempfile::tempdir().unwrap();
        let path = source(root.path());
        assert_eq!(
            Source::open(&path)
                .unwrap()
                .capture(Instant::now(), &AtomicBool::new(false))
                .err()
                .unwrap()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(
            Source::open(&path)
                .unwrap()
                .capture(
                    Instant::now() + std::time::Duration::from_secs(2),
                    &AtomicBool::new(true)
                )
                .err()
                .unwrap()
                .code,
            ErrorCode::Cancelled
        );
    }
}
