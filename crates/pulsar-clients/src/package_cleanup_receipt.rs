//! Durable parent receipts survive journal removal and directory retirement.
use super::*;
const PREFIX: &str = ".pulsar-download-cleanup-";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanupReceipt {
    schema: u16,
    stage_name: String,
    download: Receipt,
    journal: Identity,
}
pub(super) struct Record {
    name: String,
    file: File,
    before: std::fs::Metadata,
    receipt: CleanupReceipt,
}
impl Drop for Record {
    fn drop(&mut self) {
        // Closing only this descriptor leaves the lock held by an inherited
        // open-file description. The Record owns the helper lifetime, so end
        // that lifetime explicitly even while an unrelated child retains a fd.
        let _ = rustix::fs::flock(&self.file, rustix::fs::FlockOperation::Unlock);
    }
}
impl Record {
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn held_file(&self) -> &File {
        &self.file
    }
    pub(super) fn stage_name(&self) -> &str {
        &self.receipt.stage_name
    }
    pub(super) fn download(&self) -> &Receipt {
        &self.receipt.download
    }
    pub(super) fn matches_journal(&self, receipt: &Receipt, journal: &File) -> Result<()> {
        ensure!(
            *receipt == self.receipt.download && Identity::of(journal)? == self.receipt.journal,
            "Cleanup receipt differs from held original journal"
        );
        Ok(())
    }
    pub(super) fn check(&self, parent: &File) -> Result<()> {
        named_matches(parent, OsStr::new(&self.name), &self.file)?;
        private_regular(&self.file, MAX_JOURNAL)?;
        ensure!(
            unchanged(&self.before, &self.file.metadata()?),
            "Cleanup receipt changed"
        );
        Ok(())
    }
    pub(super) fn remove(&self, parent: &File) -> Result<()> {
        self.check(parent)?;
        unlinkat(parent, &self.name, AtFlags::empty())?;
        parent.sync_all()?;
        Ok(())
    }
}
fn canonical_stage(name: &str) -> Result<&str> {
    let suffix = name
        .strip_prefix(".pulsar-package-")
        .context("Cleanup stage prefix")?;
    ensure!(
        uuid::Uuid::parse_str(suffix)?.to_string() == suffix,
        "Noncanonical cleanup stage"
    );
    Ok(suffix)
}
pub(super) fn create(
    parent: &File,
    stage: &OsStr,
    download: &Receipt,
    journal: &File,
) -> Result<Record> {
    let stage_name = stage.to_str().context("Cleanup stage name")?.to_owned();
    let name = format!("{PREFIX}{}.json", canonical_stage(&stage_name)?);
    let receipt = CleanupReceipt {
        schema: 1,
        stage_name,
        download: download.clone(),
        journal: Identity::of(journal)?,
    };
    let bytes = serde_json::to_vec(&receipt)?;
    ensure!(
        bytes.len() as u64 <= MAX_JOURNAL,
        "Cleanup receipt size bound"
    );
    let file = receipt_write::create(parent, OsStr::new(&name), &bytes, None)?;
    // The guarded writer syncs the file and directory before nested retirement.
    let before = file.metadata()?;
    Ok(Record {
        name,
        file,
        before,
        receipt,
    })
}
pub(super) fn load(
    parent: &File,
    destination: &OsStr,
    expected: &DownloadBinding,
    control: &PackageDownloadControl,
) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    for (entry_count, entry) in rustix::fs::Dir::read_from(parent)?.enumerate() {
        control.check()?;
        ensure!(entry_count < 512, "Cleanup receipt directory entry budget");
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if !bytes.starts_with(PREFIX.as_bytes()) {
            continue;
        }
        ensure!(records.len() < 64, "Cleanup receipt count budget");
        let name = std::str::from_utf8(bytes)?.to_owned();
        let suffix = name
            .strip_prefix(PREFIX)
            .and_then(|s| s.strip_suffix(".json"))
            .context("Noncanonical cleanup receipt name")?;
        ensure!(
            uuid::Uuid::parse_str(suffix)?.to_string() == suffix,
            "Noncanonical cleanup receipt UUID"
        );
        let mut file = File::from(openat(
            parent,
            &name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        private_regular(&file, MAX_JOURNAL)?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .context("Cleanup receipt is owned by an active helper")?;
        let before = file.metadata()?;
        let mut bytes = Vec::new();
        (&mut file).take(MAX_JOURNAL + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= MAX_JOURNAL, "Cleanup receipt grew");
        let receipt: CleanupReceipt = serde_json::from_slice(&bytes)?;
        ensure!(
            receipt.schema == 1
                && receipt.download.schema == 1
                && canonical_stage(&receipt.stage_name)? == suffix
                && receipt.download.binding == *expected
                && receipt.download.destination_name == destination.as_bytes()
                && receipt.download.parent == Identity::of(parent)?,
            "Cleanup receipt differs from original intent/artifact/parent"
        );
        let record = Record {
            name,
            file,
            before,
            receipt,
        };
        record.check(parent)?;
        records.push(record);
    }
    Ok(records)
}
