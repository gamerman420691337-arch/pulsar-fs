//! Explicit component update into a complete new installation; base stays intact.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, serde::Serialize)]
pub struct RuntimeUpdateReport {
    pub base_manifest_sha256: String,
    pub manifest_sha256: String,
    pub provided_files: usize,
    pub reused_files: usize,
    pub provided_bytes: u64,
    pub reused_bytes: u64,
    pub qualified: bool,
}
impl RuntimeUpdateReport {
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
enum UpdateSource {
    Directory(File),
    Archive(Box<super::delta::DeltaArchive>),
}
pub(super) struct UpdateInput {
    source: UpdateSource,
    base: File,
    source_bytes: Vec<u8>,
    base_bytes: Vec<u8>,
    base_records: BTreeMap<String, (u64, String)>,
    report: RuntimeUpdateReport,
}
impl UpdateInput {
    pub(super) fn copy_asset(
        &mut self,
        record: &Record,
        output: &mut File,
        deadline: Instant,
    ) -> Result<()> {
        let supplied = match &mut self.source {
            UpdateSource::Archive(archive) => {
                if archive.provided(&record.path) {
                    archive.copy_asset(record, output, deadline)?;
                    true
                } else {
                    false
                }
            }
            UpdateSource::Directory(root) => match source_file(root, &record.path) {
                Ok(file) => {
                    archive::copy_open_asset(file, record, output, deadline)?;
                    true
                }
                Err(error)
                    if error.chain().any(|cause| {
                        cause
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    }) =>
                {
                    false
                }
                Err(error) => {
                    return Err(error.context("update asset unavailable; fallback refused"))
                }
            },
        };
        let reused = !supplied;
        if reused {
            ensure!(
                self.base_records
                    .get(&record.path)
                    .is_some_and(|(len, hash)| *len == record.byte_len && *hash == record.sha256),
                "missing changed update asset: {}",
                record.path
            );
            archive::copy_open_asset(
                source_file(&self.base, &record.path)?,
                record,
                output,
                deadline,
            )?;
        }
        if reused {
            self.report.reused_files += 1;
            self.report.reused_bytes += record.byte_len;
        } else {
            self.report.provided_files += 1;
            self.report.provided_bytes += record.byte_len;
        }
        Ok(())
    }
    pub(super) fn finish(&mut self) -> Result<()> {
        match &mut self.source {
            UpdateSource::Directory(root) => ensure!(
                manifest(root)?.0 == self.source_bytes,
                "update source manifest changed during capture"
            ),
            UpdateSource::Archive(archive) => archive.finish()?,
        }
        ensure!(
            manifest(&self.base)?.0 == self.base_bytes,
            "update source/base manifest changed during capture"
        );
        Ok(())
    }
}

/// SOURCE is a directory or update archive containing the desired manifest and changed
/// assets. Missing assets must match the explicitly pinned base manifest and are
/// independently copied/verified. No shared hardlinks or automatic activation.
pub fn update_runtime_candidate(
    source: &Path,
    base: &Path,
    destination: &Path,
    expected_base: &str,
) -> Result<RuntimeUpdateReport> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "runtime candidate update requires Linux x86-64"
    );
    ensure!(
        source.is_absolute() && base.is_absolute() && destination.is_absolute(),
        "update paths must be absolute"
    );
    ensure!(
        expected_base.len() == 64
            && expected_base
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "expected base manifest must be lowercase SHA-256"
    );
    let base_root = directory(base)?;
    let (base_bytes, previous) = manifest(&base_root)?;
    previous.validate()?;
    let base_hash = format!("{:x}", Sha256::digest(&base_bytes));
    ensure!(base_hash == expected_base, "base manifest identity differs");
    let file = archive::open_source(source)?;
    let (source, source_bytes, desired) = if file.metadata()?.is_dir() {
        let (bytes, desired) = manifest(&file)?;
        desired.validate()?;
        (UpdateSource::Directory(file), bytes, desired)
    } else {
        let (input, bytes, desired) = super::delta::DeltaArchive::open(file, expected_base)?;
        (UpdateSource::Archive(Box::new(input)), bytes, desired)
    };
    let report = RuntimeUpdateReport {
        base_manifest_sha256: base_hash,
        manifest_sha256: format!("{:x}", Sha256::digest(&source_bytes)),
        provided_files: 0,
        reused_files: 0,
        provided_bytes: 0,
        reused_bytes: 0,
        qualified: false,
    };
    let mut input = archive::Input::Update(UpdateInput {
        source,
        base: base_root,
        source_bytes: source_bytes.clone(),
        base_bytes,
        base_records: previous
            .files
            .into_iter()
            .map(|r| (r.path, (r.byte_len, r.sha256)))
            .collect(),
        report,
    });
    publish_runtime_input(&mut input, source_bytes, desired, destination)?;
    let archive::Input::Update(input) = input else {
        unreachable!("update input remains typed")
    };
    Ok(input.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn delta(root: &Path, base: &Path) -> std::path::PathBuf {
        let path = root.join("delta");
        std::fs::create_dir(&path).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(base.join("runtime-candidate.json")).unwrap())
                .unwrap();
        let bytes = b"updated authored application fixture";
        std::fs::write(path.join("pulsar"), bytes).unwrap();
        let record = value["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["path"] == "pulsar")
            .unwrap();
        record["byte_len"] = serde_json::json!(bytes.len());
        record["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(bytes)));
        std::fs::write(
            path.join("runtime-candidate.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        path
    }
    #[test]
    fn component_update_copies_verified_base_and_preserves_independent_old_installation() {
        let root = tempfile::tempdir().unwrap();
        let base = super::super::tests::fixture(root.path());
        let source = delta(root.path(), &base);
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(base.join("runtime-candidate.json")).unwrap())
        );
        let before = std::fs::read(base.join("pulsar")).unwrap();
        let dest = root.path().join("new");
        let report = update_runtime_candidate(&source, &base, &dest, &hash).unwrap();
        assert_eq!(report.provided_files, 1);
        assert_eq!(report.reused_files, 7);
        assert!(!report.qualified);
        assert_eq!(std::fs::read(base.join("pulsar")).unwrap(), before);
        assert_eq!(
            std::fs::read(dest.join("runtime-candidate.json")).unwrap(),
            std::fs::read(source.join("runtime-candidate.json")).unwrap()
        );
        let original = base.join("runtimes/ffmpeg");
        let copied = dest.join("runtimes/ffmpeg");
        assert_ne!(
            std::fs::metadata(&original).unwrap().ino(),
            std::fs::metadata(&copied).unwrap().ino()
        );
        let preserved = std::fs::read(&original).unwrap();
        std::fs::write(copied, b"new-install mutation").unwrap();
        assert_eq!(std::fs::read(original).unwrap(), preserved);
        assert!(update_runtime_candidate(&source, &base, &dest, &hash).is_err());
        assert_eq!(
            std::fs::read(dest.join("runtimes/ffmpeg")).unwrap(),
            b"new-install mutation"
        );
    }
    #[test]
    fn damaged_reused_base_never_publishes_and_owner_files_remain() {
        let root = tempfile::tempdir().unwrap();
        let base = super::super::tests::fixture(root.path());
        let source = delta(root.path(), &base);
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(base.join("runtime-candidate.json")).unwrap())
        );
        let asset = base.join("runtimes/ffprobe");
        let mut bytes = std::fs::read(&asset).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&asset, &bytes).unwrap();
        std::fs::write(base.join("owner-file"), b"keep").unwrap();
        let dest = root.path().join("new");
        assert!(update_runtime_candidate(&source, &base, &dest, &hash).is_err());
        assert!(!dest.exists());
        assert_eq!(std::fs::read(asset).unwrap(), bytes);
        assert_eq!(std::fs::read(base.join("owner-file")).unwrap(), b"keep");
    }
    #[test]
    fn wrong_base_missing_changed_corrupt_or_symlink_delta_never_publishes() {
        let root = tempfile::tempdir().unwrap();
        let base = super::super::tests::fixture(root.path());
        let source = delta(root.path(), &base);
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(base.join("runtime-candidate.json")).unwrap())
        );
        let dest = root.path().join("new");
        assert!(update_runtime_candidate(&source, &base, &dest, &"0".repeat(64)).is_err());
        let app = source.join("pulsar");
        std::fs::remove_file(&app).unwrap();
        assert!(update_runtime_candidate(&source, &base, &dest, &hash).is_err());
        std::fs::write(&app, b"corrupt supplied asset").unwrap();
        assert!(update_runtime_candidate(&source, &base, &dest, &hash).is_err());
        std::fs::remove_file(&app).unwrap();
        std::os::unix::fs::symlink(base.join("pulsar"), &app).unwrap();
        assert!(update_runtime_candidate(&source, &base, &dest, &hash).is_err());
        assert!(!dest.exists());
        assert!(std::fs::read_dir(root.path()).unwrap().all(|p| !p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pulsar-runtime-stage-")));
    }
}
