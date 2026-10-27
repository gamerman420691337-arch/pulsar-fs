//! Content-verified source ranges from one retained descriptor. No path or playback authority.
use pulsar_protocol::{ArtifactIdentity, ErrorCode, ProtocolError};
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

type PResult<T> = Result<T, ProtocolError>;

pub(super) const BLOCK_BYTES: usize = 64 * 1024;
const MAX_BLOCKS: u64 = 1_048_576;
const MAX_SOURCE_BYTES: u64 = MAX_BLOCKS * BLOCK_BYTES as u64;

/// The caller owns admission, source-retention holds and absolute lease expiry.
/// Retaining a descriptor alone does not make its inode immutable: every returned
/// range is authenticated against blocks captured during full-file verification.
pub(super) struct VerifiedSourceReader {
    #[cfg(unix)]
    file: File,
    #[cfg(unix)]
    byte_len: u64,
    #[cfg(unix)]
    blocks: Vec<[u8; 32]>,
}

#[cfg(unix)]
fn unavailable() -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, "source descriptor I/O failed")
}

#[cfg(unix)]
fn changed() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "source bytes differ from the retained content identity",
    )
}

fn exhausted() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::ResourceExhausted,
        "source verification exceeds bounded reader admission",
    )
}

#[cfg(unix)]
fn check_running(cancel: &AtomicBool, deadline: Instant) -> PResult<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "source read was cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "source read deadline expired",
        ));
    }
    Ok(())
}

fn block_count(byte_len: u64) -> PResult<usize> {
    if byte_len == 0 || byte_len > MAX_SOURCE_BYTES {
        return Err(exhausted());
    }
    usize::try_from(byte_len.div_ceil(BLOCK_BYTES as u64)).map_err(|_| exhausted())
}

#[cfg(unix)]
fn read_exact_at(
    file: &File,
    mut offset: u64,
    mut bytes: &mut [u8],
    cancel: &AtomicBool,
    deadline: Instant,
) -> PResult<()> {
    use std::os::unix::fs::FileExt;
    while !bytes.is_empty() {
        check_running(cancel, deadline)?;
        let count = match file.read_at(bytes, offset) {
            Ok(0) => return Err(changed()),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(unavailable()),
        };
        check_running(cancel, deadline)?;
        offset = offset
            .checked_add(count as u64)
            .ok_or_else(|| ProtocolError::invalid("source range overflow"))?;
        bytes = &mut bytes[count..];
    }
    Ok(())
}

#[cfg(unix)]
fn checked_metadata(
    file: &File,
    byte_len: u64,
    cancel: &AtomicBool,
    deadline: Instant,
) -> PResult<std::fs::Metadata> {
    check_running(cancel, deadline)?;
    let metadata = file.metadata().map_err(|_| unavailable())?;
    check_running(cancel, deadline)?;
    if !metadata.is_file() || metadata.len() != byte_len {
        return Err(changed());
    }
    Ok(metadata)
}

impl VerifiedSourceReader {
    /// Reserve this before capture. It covers the bounded digest index and two
    /// simultaneous 64 KiB reader scratch buffers (or the single capture buffer).
    /// The owner must separately admit each returned range and cap concurrent
    /// reads on this reader at two. This method neither allocates nor reads files.
    pub(super) fn reservation_bytes(byte_len: u64) -> PResult<u64> {
        let count = u64::try_from(block_count(byte_len)?).map_err(|_| exhausted())?;
        count
            .checked_mul(32)
            .and_then(|bytes| bytes.checked_add(2 * BLOCK_BYTES as u64))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>() as u64))
            .ok_or_else(exhausted)
    }

    /// Consume the descriptor opened by the authorized caller before its worker
    /// starts. No path is reopened. Success requires the complete expected hash
    /// and length, stable capture metadata, and a live cancellation/deadline check.
    #[cfg(unix)]
    pub(super) fn capture(
        file: File,
        identity: &ArtifactIdentity,
        cancel: &AtomicBool,
        deadline: Instant,
        progress: &AtomicU64,
    ) -> PResult<Self> {
        use std::os::unix::fs::MetadataExt;
        progress.store(0, Ordering::Release);
        check_running(cancel, deadline)?;
        identity.validate()?;
        let count = block_count(identity.byte_len)?;
        let before = checked_metadata(&file, identity.byte_len, cancel, deadline)?;
        let mut blocks = Vec::new();
        blocks.try_reserve_exact(count).map_err(|_| exhausted())?;
        let mut buffer = [0u8; BLOCK_BYTES];
        let mut hash = Sha256::new();
        let mut offset = 0u64;
        while offset < identity.byte_len {
            let length = (identity.byte_len - offset).min(BLOCK_BYTES as u64) as usize;
            let bytes = &mut buffer[..length];
            read_exact_at(&file, offset, bytes, cancel, deadline)?;
            hash.update(&*bytes);
            blocks.push(Sha256::digest(&*bytes).into());
            offset = offset.checked_add(length as u64).ok_or_else(exhausted)?;
            check_running(cancel, deadline)?;
            progress.store(offset, Ordering::Release);
        }
        let after = checked_metadata(&file, identity.byte_len, cancel, deadline)?;
        if blocks.len() != count
            || format!("{:x}", hash.finalize()) != identity.sha256
            || before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
        {
            return Err(changed());
        }
        check_running(cancel, deadline)?;
        Ok(Self {
            file,
            byte_len: identity.byte_len,
            blocks,
        })
    }

    #[cfg(not(unix))]
    pub(super) fn capture(
        _file: File,
        _identity: &ArtifactIdentity,
        _cancel: &AtomicBool,
        _deadline: Instant,
        progress: &AtomicU64,
    ) -> PResult<Self> {
        progress.store(0, Ordering::Release);
        Err(ProtocolError::unsupported(
            "verified source range reads require a supported descriptor platform",
        ))
    }

    /// Authenticate complete affected blocks before returning any requested
    /// bytes. A short range crossing a boundary authenticates both blocks. The
    /// returned allocation is at most 64 KiB; no global lock or shared seek cursor
    /// is used. Authorization and lease rechecks belong to the caller.
    #[cfg(unix)]
    pub(super) fn read_range(
        &self,
        offset: u64,
        byte_len: u32,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> PResult<Vec<u8>> {
        check_running(cancel, deadline)?;
        let end = offset
            .checked_add(u64::from(byte_len))
            .filter(|end| *end <= self.byte_len)
            .filter(|_| byte_len != 0 && byte_len as usize <= BLOCK_BYTES)
            .ok_or_else(|| ProtocolError::invalid("source range exceeds admitted bytes"))?;
        checked_metadata(&self.file, self.byte_len, cancel, deadline)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(byte_len as usize)
            .map_err(|_| exhausted())?;
        let mut buffer = [0u8; BLOCK_BYTES];
        let first = offset / BLOCK_BYTES as u64;
        let last = (end - 1) / BLOCK_BYTES as u64;
        for index in first..=last {
            let block_offset = index
                .checked_mul(BLOCK_BYTES as u64)
                .ok_or_else(|| ProtocolError::invalid("source block offset overflow"))?;
            let length = (self.byte_len - block_offset).min(BLOCK_BYTES as u64) as usize;
            let bytes = &mut buffer[..length];
            read_exact_at(&self.file, block_offset, bytes, cancel, deadline)?;
            let expected = self
                .blocks
                .get(usize::try_from(index).map_err(|_| exhausted())?)
                .ok_or_else(changed)?;
            let actual: [u8; 32] = Sha256::digest(&*bytes).into();
            if &actual != expected {
                return Err(changed());
            }
            let from = offset.saturating_sub(block_offset) as usize;
            let to = (end - block_offset).min(length as u64) as usize;
            result.extend_from_slice(&bytes[from..to]);
        }
        checked_metadata(&self.file, self.byte_len, cancel, deadline)?;
        check_running(cancel, deadline)?;
        if result.len() != byte_len as usize {
            return Err(changed());
        }
        Ok(result)
    }

    #[cfg(not(unix))]
    pub(super) fn read_range(
        &self,
        _offset: u64,
        _byte_len: u32,
        _cancel: &AtomicBool,
        _deadline: Instant,
    ) -> PResult<Vec<u8>> {
        Err(ProtocolError::unsupported(
            "verified source range reads require a supported descriptor platform",
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pulsar_protocol::ArtifactId;
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::FileExt,
        path::PathBuf,
        sync::Arc,
        time::Duration,
    };

    struct Fixture {
        _directory: tempfile::TempDir,
        path: PathBuf,
        writer: File,
        bytes: Vec<u8>,
        identity: ArtifactIdentity,
    }
    impl Fixture {
        fn new(length: usize) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("source");
            let bytes: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
            let mut writer = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            writer.write_all(&bytes).unwrap();
            writer.sync_all().unwrap();
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            let identity = ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: length as u64,
            };
            Self {
                _directory: directory,
                path,
                writer,
                bytes,
                identity,
            }
        }
        fn capture(&self) -> VerifiedSourceReader {
            let progress = AtomicU64::new(0);
            let reader = VerifiedSourceReader::capture(
                File::open(&self.path).unwrap(),
                &self.identity,
                &AtomicBool::new(false),
                deadline(),
                &progress,
            )
            .unwrap();
            assert_eq!(progress.load(Ordering::Acquire), self.identity.byte_len);
            reader
        }
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }
    fn read(reader: &VerifiedSourceReader, offset: u64, length: u32) -> PResult<Vec<u8>> {
        reader.read_range(offset, length, &AtomicBool::new(false), deadline())
    }

    #[test]
    fn ranges_retry_and_cross_block_reads_preserve_exact_captured_bytes() {
        let f = Fixture::new(2 * BLOCK_BYTES + 17);
        let reader = f.capture();
        for (offset, length) in [
            (0, 1),
            (17, 19),
            (0, BLOCK_BYTES),
            (BLOCK_BYTES - 7, BLOCK_BYTES),
            (2 * BLOCK_BYTES - 2, 19),
            (f.bytes.len() - 1, 1),
        ] {
            for _ in 0..2 {
                assert_eq!(
                    read(&reader, offset as u64, length as u32).unwrap(),
                    f.bytes[offset..offset + length]
                );
            }
        }
    }

    #[test]
    fn same_inode_changes_outside_requested_slice_still_fail_complete_block_hash() {
        let f = Fixture::new(2 * BLOCK_BYTES + 17);
        let reader = f.capture();
        f.writer.write_at(&[255], 20_000).unwrap();
        assert_eq!(
            read(&reader, 0, 3).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        f.writer.write_at(&f.bytes[20_000..20_001], 20_000).unwrap();
        f.writer
            .write_at(&[255], (BLOCK_BYTES + 20_000) as u64)
            .unwrap();
        assert_eq!(
            read(&reader, (BLOCK_BYTES - 3) as u64, 6).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert_eq!(read(&reader, 0, 3).unwrap(), f.bytes[..3]);
    }

    #[test]
    fn truncation_and_growth_fail_even_when_requested_prefix_survives() {
        let f = Fixture::new(BLOCK_BYTES + 7);
        let reader = f.capture();
        f.writer.set_len(BLOCK_BYTES as u64).unwrap();
        assert_eq!(
            read(&reader, 0, 1).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        f.writer.set_len((2 * BLOCK_BYTES) as u64).unwrap();
        assert_eq!(
            read(&reader, 0, 1).unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }

    #[test]
    fn capture_and_later_reads_keep_original_descriptor_after_path_replacement() {
        let f = Fixture::new(BLOCK_BYTES + 11);
        let opened = File::open(&f.path).unwrap();
        let prior = f.path.with_extension("prior");
        fs::rename(&f.path, &prior).unwrap();
        fs::write(&f.path, vec![255; f.bytes.len()]).unwrap();
        let reader = VerifiedSourceReader::capture(
            opened,
            &f.identity,
            &AtomicBool::new(false),
            deadline(),
            &AtomicU64::new(0),
        )
        .unwrap();
        fs::remove_file(&prior).unwrap();
        assert_eq!(
            read(&reader, (BLOCK_BYTES - 4) as u64, 15).unwrap(),
            f.bytes[BLOCK_BYTES - 4..]
        );
    }

    #[test]
    fn capture_refuses_wrong_full_digest_and_length_without_leaking_paths() {
        let f = Fixture::new(BLOCK_BYTES + 1);
        for change_length in [false, true] {
            let mut identity = f.identity.clone();
            if change_length {
                identity.byte_len += 1;
            } else {
                identity.sha256 = "0".repeat(64);
            }
            let error = VerifiedSourceReader::capture(
                File::open(&f.path).unwrap(),
                &identity,
                &AtomicBool::new(false),
                deadline(),
                &AtomicU64::new(0),
            )
            .err()
            .unwrap();
            assert_eq!(error.code, ErrorCode::DependencyMismatch);
            assert!(!error
                .message
                .contains(&f.path.to_string_lossy().to_string()));
        }
    }

    #[test]
    fn cancellation_and_deadline_refuse_capture_and_range_delivery() {
        let f = Fixture::new(BLOCK_BYTES + 1);
        let reader = f.capture();
        for cancelled in [true, false] {
            let cancel = AtomicBool::new(cancelled);
            let end = if cancelled {
                deadline()
            } else {
                Instant::now()
            };
            let code = if cancelled {
                ErrorCode::Cancelled
            } else {
                ErrorCode::Unavailable
            };
            let progress = AtomicU64::new(99);
            let error = VerifiedSourceReader::capture(
                File::open(&f.path).unwrap(),
                &f.identity,
                &cancel,
                end,
                &progress,
            )
            .err()
            .unwrap();
            assert_eq!(error.code, code);
            assert_eq!(progress.load(Ordering::Acquire), 0);
            assert_eq!(
                reader.read_range(0, 1, &cancel, end).unwrap_err().code,
                code
            );
        }
    }

    #[test]
    fn range_overflow_empty_oversized_and_out_of_bounds_are_refused() {
        let f = Fixture::new(BLOCK_BYTES + 1);
        let reader = f.capture();
        for (offset, length) in [
            (0, 0),
            (0, BLOCK_BYTES as u32 + 1),
            (u64::MAX, 1),
            (f.identity.byte_len, 1),
            (f.identity.byte_len - 1, 2),
        ] {
            assert_eq!(
                read(&reader, offset, length).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
    }

    #[test]
    fn index_admission_has_explicit_bounds_without_large_allocations() {
        assert!(VerifiedSourceReader::reservation_bytes(0).is_err());
        assert!(VerifiedSourceReader::reservation_bytes(MAX_SOURCE_BYTES + 1).is_err());
        assert!(VerifiedSourceReader::reservation_bytes(u64::MAX).is_err());
        assert_eq!(
            VerifiedSourceReader::reservation_bytes(BLOCK_BYTES as u64 + 1).unwrap()
                - VerifiedSourceReader::reservation_bytes(BLOCK_BYTES as u64).unwrap(),
            32
        );
        assert_eq!(
            VerifiedSourceReader::reservation_bytes(MAX_SOURCE_BYTES).unwrap(),
            MAX_BLOCKS * 32
                + 2 * BLOCK_BYTES as u64
                + std::mem::size_of::<VerifiedSourceReader>() as u64
        );
    }

    #[test]
    fn independent_concurrent_ranges_use_no_shared_seek_cursor() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<VerifiedSourceReader>();
        let f = Fixture::new(2 * BLOCK_BYTES + 1);
        let reader = Arc::new(f.capture());
        let first = reader.clone();
        let one = std::thread::spawn(move || read(&first, 5, BLOCK_BYTES as u32).unwrap());
        let two = std::thread::spawn(move || read(&reader, BLOCK_BYTES as u64 - 5, 23).unwrap());
        assert_eq!(one.join().unwrap(), f.bytes[5..BLOCK_BYTES + 5]);
        assert_eq!(
            two.join().unwrap(),
            f.bytes[BLOCK_BYTES - 5..BLOCK_BYTES + 18]
        );
    }
}
