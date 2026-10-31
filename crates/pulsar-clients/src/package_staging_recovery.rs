//! Reconcile only receipted, inactive local download stages after full recovery.
//! No lease replay, package release, import, or project mutation is performed.
#![cfg(unix)]
use crate::project_package::PackageDownloadControl;
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use rustix::fs::{openat, statat, unlinkat, AtFlags, Mode, OFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{FileExt, MetadataExt},
    },
};

pub(crate) const JOURNAL: &str = "recovery.json";
const MAX_JOURNAL: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DownloadBinding {
    pub credential_binding: String,
    pub project: ProjectId,
    pub operation: PackageOperationId,
    pub request: RequestId,
    pub artifact: PackageArtifactDescriptor,
}
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn of(file: &File) -> Result<Self> {
        let m = file.metadata()?;
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
        })
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u16,
    binding: DownloadBinding,
    destination_name: Vec<u8>,
    parent: Identity,
    directory: Identity,
    bundle: Identity,
}

/// Before any Begin submission or private package byte is received.
pub(crate) fn write_receipt(
    parent: &File,
    directory: &File,
    bundle: &File,
    destination: &OsStr,
    binding: DownloadBinding,
) -> Result<File> {
    binding.artifact.validate()?;
    ensure!(
        binding.project == binding.artifact.project_id,
        "Download receipt project mismatch"
    );
    let receipt = Receipt {
        schema: 1,
        binding,
        destination_name: destination.as_bytes().to_vec(),
        parent: Identity::of(parent)?,
        directory: Identity::of(directory)?,
        bundle: Identity::of(bundle)?,
    };
    let bytes = serde_json::to_vec(&receipt)?;
    ensure!(
        bytes.len() as u64 <= MAX_JOURNAL,
        "Download receipt exceeds local bound"
    );
    receipt_write::create(directory, OsStr::new(JOURNAL), &bytes, Some(parent))
}

fn private_regular(file: &File, limit: u64) -> Result<()> {
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.nlink() == 1
            && m.uid() == rustix::process::geteuid().as_raw()
            && m.mode() & 0o077 == 0
            && m.len() <= limit,
        "Unsafe private download recovery member"
    );
    Ok(())
}
fn named_matches(parent: &File, name: &OsStr, file: &File) -> Result<()> {
    let named = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let held = rustix::fs::fstat(file)?;
    ensure!(
        named.st_dev == held.st_dev && named.st_ino == held.st_ino,
        "Download recovery member was replaced"
    );
    Ok(())
}
fn unchanged(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
        && before.mode() == after.mode()
        && before.uid() == after.uid()
        && after.nlink() == 1
}
struct Retained {
    name: std::ffi::OsString,
    directory: File,
    journal: Option<(File, std::fs::Metadata)>,
    receipt: Receipt,
    cleanup_index: Option<usize>,
    bundle: Option<(File, std::fs::Metadata)>,
}

/// Caller holds the paired-stage lock and has independently verified the full
/// completed package and its script/capture association. Partial bytes are never
/// reused. Every candidate is validated before any local removal occurs.
pub(crate) fn reconcile(
    parent: &File,
    completed: &File,
    destination: &OsStr,
    expected: &DownloadBinding,
    control: &PackageDownloadControl,
) -> Result<usize> {
    expected.artifact.validate()?;
    let full_before = completed.metadata()?;
    ensure!(
        full_before.is_file()
            && full_before.nlink() == 1
            && full_before.len() == expected.artifact.byte_len,
        "Recovered package shape differs from bound receipt"
    );
    let mut records = cleanup::load(parent, destination, expected, control)?;
    let mut candidates = Vec::new();
    let mut entries = 0;
    for entry in rustix::fs::Dir::read_from(parent)? {
        control.check()?;
        let entry = entry?;
        entries += 1;
        ensure!(entries <= 512, "Paired recovery directory entry budget");
        let bytes = entry.file_name().to_bytes();
        if !bytes.starts_with(b".pulsar-package-") {
            continue;
        }
        ensure!(candidates.len() < 64, "Private download stage count budget");
        let name = OsStr::from_bytes(bytes).to_owned();
        let text = name.to_str().context("Noncanonical download stage name")?;
        let suffix = text
            .strip_prefix(".pulsar-package-")
            .context("Download stage prefix")?;
        ensure!(
            uuid::Uuid::parse_str(suffix)?.to_string() == suffix,
            "Noncanonical download stage identity"
        );
        let directory = File::from(openat(
            parent,
            &name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let meta = directory.metadata()?;
        ensure!(
            meta.is_dir()
                && meta.uid() == rustix::process::geteuid().as_raw()
                && meta.mode() & 0o077 == 0,
            "Unsafe private download recovery directory"
        );
        rustix::fs::flock(
            &directory,
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        )
        .context("Download stage is still owned by an active transfer")?;
        let mut member_count = 0;
        for member in rustix::fs::Dir::read_from(&directory)? {
            member_count += 1;
            ensure!(
                member_count <= 4,
                "Unexpected private download recovery member count"
            );
            ensure!(
                [
                    b".".as_slice(),
                    b"..".as_slice(),
                    JOURNAL.as_bytes(),
                    b"bundle".as_slice()
                ]
                .contains(&member?.file_name().to_bytes()),
                "Unknown private download member; nothing removed"
            );
        }
        let cleanup_index = records.iter().position(|r| r.stage_name() == text);
        let (receipt, journal) = match openat(
            &directory,
            JOURNAL,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => {
                let mut journal = File::from(fd);
                private_regular(&journal, MAX_JOURNAL)?;
                let before = journal.metadata()?;
                let mut bytes = Vec::new();
                (&mut journal)
                    .take(MAX_JOURNAL + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 <= MAX_JOURNAL,
                    "Download recovery journal grew"
                );
                let receipt: Receipt = serde_json::from_slice(&bytes)?;
                if let Some(index) = cleanup_index {
                    records[index].matches_journal(&receipt, &journal)?;
                }
                ensure!(
                    unchanged(&before, &journal.metadata()?),
                    "Download recovery journal changed"
                );
                named_matches(&directory, OsStr::new(JOURNAL), &journal)?;
                (receipt, Some((journal, before)))
            }
            Err(rustix::io::Errno::NOENT) => {
                let index =
                    cleanup_index.context("Unreceipted empty download stage; nothing removed")?;
                ensure!(
                    matches!(
                        statat(&directory, "bundle", AtFlags::SYMLINK_NOFOLLOW),
                        Err(rustix::io::Errno::NOENT)
                    ),
                    "Missing journal with retained bundle; nothing removed"
                );
                (records[index].download().clone(), None)
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            receipt.schema == 1
                && receipt.binding == *expected
                && receipt.destination_name == destination.as_bytes()
                && receipt.parent == Identity::of(parent)?
                && receipt.directory == Identity::of(&directory)?,
            "Download recovery journal differs from original intent/artifact/directory"
        );
        let bundle = match openat(
            &directory,
            "bundle",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => {
                let file = File::from(fd);
                private_regular(&file, expected.artifact.byte_len)?;
                ensure!(
                    Identity::of(&file)? == receipt.bundle,
                    "Download recovery bundle identity changed"
                );
                let before = file.metadata()?;
                let mut a = vec![0; MAX_ARTIFACT_CHUNK_BYTES];
                let mut b = vec![0; MAX_ARTIFACT_CHUNK_BYTES];
                let mut offset = 0;
                while offset < before.len() {
                    control.check()?;
                    let count = (before.len() - offset).min(a.len() as u64) as usize;
                    file.read_exact_at(&mut a[..count], offset)?;
                    completed.read_exact_at(&mut b[..count], offset)?;
                    ensure!(
                        a[..count] == b[..count],
                        "Retained package bytes differ from recovered immutable package"
                    );
                    offset += count as u64;
                }
                ensure!(
                    unchanged(&before, &file.metadata()?),
                    "Download partial changed during comparison"
                );
                Some((file, before))
            }
            Err(rustix::io::Errno::NOENT) => None, // only bound metadata remains; no data file is removed
            Err(error) => return Err(error.into()),
        };
        named_matches(parent, &name, &directory)?;
        candidates.push(Retained {
            name,
            directory,
            journal,
            receipt,
            cleanup_index,
            bundle,
        });
    }
    for (index, record) in records.iter().enumerate() {
        record.check(parent)?;
        if !candidates.iter().any(|c| c.cleanup_index == Some(index)) {
            ensure!(
                matches!(
                    statat(parent, record.stage_name(), AtFlags::SYMLINK_NOFOLLOW),
                    Err(rustix::io::Errno::NOENT)
                ),
                "Cleanup record's directory was not independently validated"
            );
        }
    }
    if !candidates.is_empty() || !records.is_empty() {
        let mut hasher = Sha256::new();
        let mut block = vec![0; MAX_ARTIFACT_CHUNK_BYTES];
        let mut offset = 0;
        while offset < full_before.len() {
            control.check()?;
            let count = (full_before.len() - offset).min(block.len() as u64) as usize;
            completed.read_exact_at(&mut block[..count], offset)?;
            hasher.update(&block[..count]);
            offset += count as u64;
        }
        ensure!(
            format!("{:x}", hasher.finalize()) == expected.artifact.sha256,
            "Held recovered package digest differs from immutable artifact"
        );
    }
    ensure!(
        unchanged(&full_before, &completed.metadata()?),
        "Recovered package changed during reconciliation"
    );
    ensure!(
        records.len()
            + candidates
                .iter()
                .filter(|c| c.cleanup_index.is_none())
                .count()
            <= 64,
        "Combined cleanup receipt count budget; nothing removed"
    );
    // Validate all candidates before recording or removing any local member.
    for c in &candidates {
        check_retained(parent, c)?;
    }
    for c in &mut candidates {
        control.check()?;
        if c.cleanup_index.is_none() {
            let (journal, _) = c.journal.as_ref().context("Cleanup journal missing")?;
            let record = cleanup::create(parent, &c.name, &c.receipt, journal)?;
            c.cleanup_index = Some(records.len());
            records.push(record);
        }
    }
    for c in &candidates {
        control.check()?;
        ensure!(
            unchanged(&full_before, &completed.metadata()?),
            "Recovered package changed before local retirement"
        );
        check_retained(parent, c)?;
        records[c.cleanup_index.context("Durable cleanup receipt missing")?].check(parent)?;
        if c.bundle.is_some() {
            unlinkat(&c.directory, "bundle", AtFlags::empty())?;
        }
        c.directory.sync_all()?;
        if c.journal.is_some() {
            unlinkat(&c.directory, JOURNAL, AtFlags::empty())?;
        }
        c.directory.sync_all()?;
        unlinkat(parent, &c.name, AtFlags::REMOVEDIR)?;
        parent.sync_all()?;
    }
    for record in &records {
        control.check()?;
        ensure!(
            unchanged(&full_before, &completed.metadata()?),
            "Recovered package changed before receipt retirement"
        );
        ensure!(
            matches!(
                statat(parent, record.stage_name(), AtFlags::SYMLINK_NOFOLLOW),
                Err(rustix::io::Errno::NOENT)
            ),
            "Download directory still exists; cleanup receipt preserved"
        );
        record.remove(parent)?;
    }
    Ok(candidates.len())
}

fn check_retained(parent: &File, c: &Retained) -> Result<()> {
    named_matches(parent, &c.name, &c.directory)?;
    for (count, member) in rustix::fs::Dir::read_from(&c.directory)?.enumerate() {
        ensure!(
            count < 4,
            "Download directory changed its member count before cleanup"
        );
        ensure!(
            [
                b".".as_slice(),
                b"..".as_slice(),
                JOURNAL.as_bytes(),
                b"bundle".as_slice()
            ]
            .contains(&member?.file_name().to_bytes()),
            "Unknown download member before cleanup; nothing removed"
        );
    }
    if let Some((file, before)) = &c.journal {
        named_matches(&c.directory, OsStr::new(JOURNAL), file)?;
        ensure!(
            unchanged(before, &file.metadata()?),
            "Download journal changed before cleanup"
        );
    } else {
        ensure!(
            matches!(
                statat(&c.directory, JOURNAL, AtFlags::SYMLINK_NOFOLLOW),
                Err(rustix::io::Errno::NOENT)
            ),
            "Journal reappeared before cleanup"
        );
    }
    if let Some((file, before)) = &c.bundle {
        named_matches(&c.directory, OsStr::new("bundle"), file)?;
        ensure!(
            unchanged(before, &file.metadata()?),
            "Download partial changed before cleanup"
        );
    } else {
        ensure!(
            matches!(
                statat(&c.directory, "bundle", AtFlags::SYMLINK_NOFOLLOW),
                Err(rustix::io::Errno::NOENT)
            ),
            "Bundle reappeared before cleanup"
        );
    }
    Ok(())
}
#[path = "package_cleanup_receipt.rs"]
mod cleanup;
#[path = "receipt_write.rs"]
mod receipt_write;

#[cfg(test)]
#[path = "package_staging_recovery_tests.rs"]
mod tests;
