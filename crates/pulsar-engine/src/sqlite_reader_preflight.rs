//! Negative-only reader-version inspection before SQLite or daemon effects.
//! WAL layout/checksums: https://www.sqlite.org/fileformat2.html#walformat
//! Success never admits a schema or replaces SQLite and marker validation.
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

const DATABASE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const WAL_FORMAT: u32 = 3_007_000;

#[derive(PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl Stamp {
    fn of(metadata: &fs::Metadata) -> Result<Self> {
        ensure!(
            metadata.is_file(),
            "reader preflight requires regular files"
        );
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}
struct ObservedFile {
    path: PathBuf,
    file: File,
    stamp: Stamp,
}
fn optional_file(path: &Path) -> Result<Option<ObservedFile>> {
    match crate::artifacts::open_regular(path) {
        Ok(file) => Ok(Some(ObservedFile {
            path: path.into(),
            stamp: Stamp::of(&file.metadata()?)?,
            file,
        })),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn unchanged(path: &Path, observed: Option<&ObservedFile>) -> Result<()> {
    let current = optional_file(path)?;
    match (observed, current) {
        (None, None) => Ok(()),
        (Some(before), Some(after)) => {
            ensure!(before.path == after.path && before.stamp == after.stamp
                && before.stamp == Stamp::of(&before.file.metadata()?)?,
                "database changed during reader preflight; retry without modifying original state");
            Ok(())
        }
        _ => anyhow::bail!("database files changed during reader preflight; retry without modifying original state"),
    }
}
fn be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four-byte field"))
}
fn checksum(bytes: &[u8], big_endian: bool, mut state: [u32; 2]) -> [u32; 2] {
    debug_assert_eq!(bytes.len() % 8, 0);
    for pair in bytes.as_chunks::<8>().0 {
        let word = |part: &[u8]| {
            if big_endian {
                be(part)
            } else {
                u32::from_le_bytes(part.try_into().expect("four-byte word"))
            }
        };
        state[0] = state[0]
            .wrapping_add(word(&pair[..4]))
            .wrapping_add(state[1]);
        state[1] = state[1]
            .wrapping_add(word(&pair[4..]))
            .wrapping_add(state[0]);
    }
    state
}
fn database_version(header: &[u8]) -> Result<u32> {
    ensure!(
        header.len() >= 100 && &header[..16] == DATABASE_MAGIC,
        "invalid project database header; original was not modified"
    );
    Ok(be(&header[60..64]))
}
fn permitted(version: u32, maximum: i64) -> Result<()> {
    let version = i64::from(version);
    ensure!(version == 0 || (2..=maximum).contains(&version),
        "project database schema requires an explicit recoverable migration; original was not modified");
    Ok(())
}

/// Stream the original file-length bound with at most one 64-KiB page buffer.
/// A bad/torn frame ends the valid WAL prefix, just as SQLite recovery does.
/// Only a checksum-valid commit publishes the latest preceding page-one value.
fn committed_wal_version(reader: &mut impl Read, length: u64) -> Result<Option<u32>> {
    if length < 32 {
        return Ok(None);
    }
    let mut header = [0u8; 32];
    reader
        .read_exact(&mut header)
        .context("read WAL header without SQLite")?;
    let magic = be(&header[..4]);
    if magic != 0x377f0682 && magic != 0x377f0683 {
        return Ok(None);
    }
    let big_endian = magic == 0x377f0683;
    let mut sum = checksum(&header[..24], big_endian, [0, 0]);
    if sum != [be(&header[24..28]), be(&header[28..32])] {
        return Ok(None);
    }
    ensure!(
        be(&header[4..8]) == WAL_FORMAT,
        "unsupported WAL format; original was not modified"
    );
    let page_size = be(&header[8..12]);
    ensure!(
        (512..=65536).contains(&page_size) && page_size.is_power_of_two(),
        "invalid WAL page size; original was not modified"
    );
    let mut page = vec![0u8; page_size as usize];
    let mut frame = [0u8; 24];
    let count = (length - 32) / (u64::from(page_size) + 24);
    let mut pending_page_one = None;
    let mut committed = None;
    for _ in 0..count {
        reader
            .read_exact(&mut frame)
            .context("read WAL frame header")?;
        if frame[8..16] != header[16..24] || be(&frame[..4]) == 0 {
            break;
        }
        reader
            .read_exact(&mut page)
            .context("read WAL frame page")?;
        let next = checksum(&page, big_endian, checksum(&frame[..8], big_endian, sum));
        if next != [be(&frame[16..20]), be(&frame[20..24])] {
            break;
        }
        sum = next;
        if be(&frame[..4]) == 1 {
            let mut first = [0u8; 100];
            first.copy_from_slice(&page[..100]);
            pending_page_one = Some(first);
        }
        if be(&frame[4..8]) != 0 {
            if let Some(first) = &pending_page_one {
                committed = Some(database_version(first)?);
            }
        }
    }
    Ok(committed)
}

pub(crate) fn check(state: &Path, maximum: i64) -> Result<()> {
    let main_path = state.join("projects.sqlite3");
    let wal_path = state.join("projects.sqlite3-wal");
    let mut main = optional_file(&main_path)?;
    let mut main_header = None;
    if let Some(observed) = &mut main {
        if observed.stamp.length != 0 {
            let mut header = [0u8; 100];
            observed
                .file
                .read_exact(&mut header)
                .context("read existing project database header")?;
            permitted(database_version(&header)?, maximum)?;
            main_header = Some(header);
        }
    }
    let mut wal = optional_file(&wal_path)?;
    let version = match &mut wal {
        Some(observed) => committed_wal_version(&mut observed.file, observed.stamp.length)?,
        None => None,
    };
    unchanged(&main_path, main.as_ref())?;
    unchanged(&wal_path, wal.as_ref())?;
    if let (Some(observed), Some(before)) = (&mut main, main_header) {
        let mut after = [0u8; 100];
        observed.file.seek(SeekFrom::Start(0))?;
        observed.file.read_exact(&mut after)?;
        ensure!(
            before == after,
            "database header changed during reader preflight; original was not modified"
        );
    }
    if let Some(version) = version {
        permitted(version, maximum)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "sqlite_reader_preflight_tests.rs"]
mod tests;
