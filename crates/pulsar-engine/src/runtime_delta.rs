//! Single-file changed-payload transport bound to an exact retained base.
use super::*;
const MAGIC: &[u8; 12] = b"PULSARUP0001";
const MAX_HEADER: usize = 262144;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    base_manifest_sha256: String,
    desired_manifest: String,
    provided_paths: Vec<String>,
}
pub(super) struct DeltaArchive {
    file: File,
    before: std::fs::Metadata,
    provided: BTreeSet<String>,
}
impl DeltaArchive {
    pub(super) fn open(mut file: File, expected_base: &str) -> Result<(Self, Vec<u8>, Manifest)> {
        let before = file.metadata()?;
        ensure!(before.is_file(), "update archive must be regular");
        let mut magic = [0; 12];
        file.read_exact(&mut magic)
            .context("truncated update header")?;
        ensure!(&magic == MAGIC, "unsupported update archive magic/version");
        let mut length = [0; 4];
        file.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        ensure!(
            length > 0 && length <= MAX_HEADER,
            "update header exceeds admission"
        );
        let mut encoded = vec![0; length];
        file.read_exact(&mut encoded)
            .context("truncated update descriptor")?;
        let header: Header = serde_json::from_slice(&encoded)?;
        ensure!(
            header.base_manifest_sha256 == expected_base,
            "update archive base identity differs"
        );
        let bytes = header.desired_manifest.into_bytes();
        ensure!(
            !bytes.is_empty() && bytes.len() <= 65536,
            "update manifest exceeds admission"
        );
        let desired: Manifest = serde_json::from_slice(&bytes)?;
        desired.validate()?;
        let provided: BTreeSet<_> = header.provided_paths.iter().cloned().collect();
        let ordered: Vec<_> = desired
            .files
            .iter()
            .filter(|r| provided.contains(&r.path))
            .map(|r| r.path.clone())
            .collect();
        ensure!(
            ordered == header.provided_paths && provided.len() == ordered.len(),
            "update paths must be unique declared manifest order"
        );
        let payload = desired
            .files
            .iter()
            .filter(|r| provided.contains(&r.path))
            .try_fold(0u64, |total, r| {
                total
                    .checked_add(r.byte_len)
                    .context("update size overflow")
            })?;
        let expected = 16u64
            .checked_add(length as u64)
            .and_then(|n| n.checked_add(payload))
            .context("update size overflow")?;
        ensure!(
            before.len() == expected && stable(&before, &file.metadata()?),
            "update archive length/metadata differs"
        );
        Ok((
            Self {
                file,
                before,
                provided,
            },
            bytes,
            desired,
        ))
    }
    pub(super) fn provided(&self, path: &str) -> bool {
        self.provided.contains(path)
    }
    pub(super) fn copy_asset(
        &mut self,
        r: &Record,
        output: &mut File,
        deadline: Instant,
    ) -> Result<()> {
        archive::copy_verified(&mut self.file, r, output, deadline)
    }
    pub(super) fn finish(&mut self) -> Result<()> {
        let mut extra = [0];
        ensure!(
            self.file.read(&mut extra)? == 0 && stable(&self.before, &self.file.metadata()?),
            "update archive changed or has trailing bytes"
        );
        Ok(())
    }
}
/// Compare complete candidates and publish only changed payloads. Every source
/// payload is independently verified, including bytes omitted from the archive.
pub fn pack_runtime_update(
    source: &Path,
    base: &Path,
    destination: &Path,
    expected_base: &str,
) -> Result<update::RuntimeUpdateReport> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "runtime update packaging requires Linux x86-64"
    );
    ensure!(
        source.is_absolute() && base.is_absolute() && destination.is_absolute(),
        "update archive paths must be absolute"
    );
    let root = directory(source)?;
    let old = directory(base)?;
    let (bytes, desired) = manifest(&root)?;
    desired.validate()?;
    let (old_bytes, previous) = manifest(&old)?;
    previous.validate()?;
    let base_hash = format!("{:x}", Sha256::digest(&old_bytes));
    ensure!(base_hash == expected_base, "base manifest identity differs");
    generation::verify(&root, &desired.generation_packs)?;
    let previous: std::collections::BTreeMap<_, _> = previous
        .files
        .into_iter()
        .map(|r| (r.path, (r.byte_len, r.sha256)))
        .collect();
    let changed: BTreeSet<_> = desired
        .files
        .iter()
        .filter(|r| {
            !previous
                .get(&r.path)
                .is_some_and(|(n, h)| *n == r.byte_len && *h == r.sha256)
        })
        .map(|r| r.path.clone())
        .collect();
    let mut report = update::RuntimeUpdateReport {
        base_manifest_sha256: base_hash.clone(),
        manifest_sha256: format!("{:x}", Sha256::digest(&bytes)),
        provided_files: 0,
        reused_files: 0,
        provided_bytes: 0,
        reused_bytes: 0,
        qualified: false,
    };
    for r in &desired.files {
        if changed.contains(&r.path) {
            report.provided_files += 1;
            report.provided_bytes += r.byte_len;
        } else {
            report.reused_files += 1;
            report.reused_bytes += r.byte_len;
        }
    }
    let encoded = serde_json::to_vec(&Header {
        base_manifest_sha256: base_hash,
        desired_manifest: String::from_utf8(bytes.clone())?,
        provided_paths: desired
            .files
            .iter()
            .filter(|r| changed.contains(&r.path))
            .map(|r| r.path.clone())
            .collect(),
    })?;
    ensure!(
        encoded.len() <= MAX_HEADER,
        "update header exceeds admission"
    );
    let total = 16u64
        .checked_add(encoded.len() as u64)
        .and_then(|n| n.checked_add(report.provided_bytes))
        .context("update size overflow")?;
    archive::publish_archive(destination, total, |output, deadline| {
        output.write_all(MAGIC)?;
        output.write_all(&(encoded.len() as u32).to_le_bytes())?;
        output.write_all(&encoded)?;
        let mut sink = std::fs::OpenOptions::new().write(true).open("/dev/null")?;
        for r in &desired.files {
            let target = if changed.contains(&r.path) {
                &mut *output
            } else {
                &mut sink
            };
            archive::copy_open_asset(source_file(&root, &r.path)?, r, target, deadline)?;
        }
        ensure!(
            manifest(&root)?.0 == bytes && manifest(&old)?.0 == old_bytes,
            "update source/base manifest changed during capture"
        );
        Ok(())
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let base = super::super::tests::fixture(root);
        let desired = root.join("desired");
        install_runtime_candidate(&base, &desired).unwrap();
        let bytes = b"new application";
        std::fs::write(desired.join("pulsar"), bytes).unwrap();
        let path = desired.join("runtime-candidate.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let record = value["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["path"] == "pulsar")
            .unwrap();
        record["byte_len"] = serde_json::json!(bytes.len());
        record["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(bytes)));
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        let hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(base.join("runtime-candidate.json")).unwrap())
        );
        (base, desired, hash)
    }
    #[test]
    fn delta_archive_roundtrip_only_transports_changed_bytes_and_retains_exact_manifest() {
        let root = tempfile::tempdir().unwrap();
        let (base, desired, hash) = setup(root.path());
        let packed = root.path().join("delta");
        let dest = root.path().join("new");
        let report = pack_runtime_update(&desired, &base, &packed, &hash).unwrap();
        assert_eq!((report.provided_files, report.reused_files), (1, 7));
        let installed = update_runtime_candidate(&packed, &base, &dest, &hash).unwrap();
        assert_eq!(installed.json().unwrap(), report.json().unwrap());
        let raw = std::fs::read(&packed).unwrap();
        let length = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
        assert_eq!(raw.len(), 16 + length + b"new application".len());
        assert_eq!(&raw[16 + length..], b"new application");
        for r in manifest(&directory(&desired).unwrap()).unwrap().1.files {
            assert_eq!(
                std::fs::read(dest.join(&r.path)).unwrap(),
                std::fs::read(desired.join(&r.path)).unwrap()
            );
        }
        assert_eq!(
            std::fs::read(dest.join("runtime-candidate.json")).unwrap(),
            std::fs::read(desired.join("runtime-candidate.json")).unwrap()
        );
        assert!(pack_runtime_update(&desired, &base, &packed, &hash).is_err());
        assert_eq!(std::fs::read(&packed).unwrap(), raw);
        assert!(install_runtime_candidate(&packed, &root.path().join("unbound")).is_err());
    }
    #[test]
    fn delta_archive_refuses_corrupt_truncated_trailing_payload_wrong_base_and_damaged_reuse() {
        let root = tempfile::tempdir().unwrap();
        let (base, desired, hash) = setup(root.path());
        let packed = root.path().join("delta");
        pack_runtime_update(&desired, &base, &packed, &hash).unwrap();
        let original = std::fs::read(&packed).unwrap();
        let dest = root.path().join("new");
        for mode in 0..3 {
            let mut bytes = original.clone();
            match mode {
                0 => {
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                }
                1 => {
                    bytes.pop();
                }
                _ => bytes.push(0),
            };
            std::fs::write(&packed, bytes).unwrap();
            assert!(update_runtime_candidate(&packed, &base, &dest, &hash).is_err());
            assert!(!dest.exists());
        }
        std::fs::write(&packed, &original).unwrap();
        assert!(update_runtime_candidate(&packed, &base, &dest, &"0".repeat(64)).is_err());
        std::fs::write(base.join("runtimes/ffprobe"), b"bad").unwrap();
        assert!(update_runtime_candidate(&packed, &base, &dest, &hash).is_err());
        assert!(!dest.exists());
    }
    #[test]
    fn delta_descriptor_refuses_unknown_duplicate_reordered_omitted_changed_and_overlarge_headers()
    {
        let root = tempfile::tempdir().unwrap();
        let (base, desired, hash) = setup(root.path());
        let packed = root.path().join("delta");
        pack_runtime_update(&desired, &base, &packed, &hash).unwrap();
        let original = std::fs::read(&packed).unwrap();
        let length = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
        let header: serde_json::Value = serde_json::from_slice(&original[16..16 + length]).unwrap();
        for paths in [
            serde_json::json!(["pulsar", "pulsar"]),
            serde_json::json!(["foreign"]),
            serde_json::json!(["runtimes/ffmpeg", "pulsar"]),
            serde_json::json!([]),
        ] {
            let mut h = header.clone();
            h["provided_paths"] = paths;
            let encoded = serde_json::to_vec(&h).unwrap();
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
            bytes.extend(encoded);
            if h["provided_paths"] != serde_json::json!([]) {
                bytes.extend_from_slice(&original[16 + length..]);
            }
            std::fs::write(&packed, bytes).unwrap();
            let dest = root.path().join("refused");
            assert!(update_runtime_candidate(&packed, &base, &dest, &hash).is_err());
            assert!(!dest.exists());
        }
        let mut h = header;
        h["extra"] = serde_json::json!(true);
        let encoded = serde_json::to_vec(&h).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded);
        std::fs::write(&packed, bytes).unwrap();
        assert!(
            update_runtime_candidate(&packed, &base, &root.path().join("unknown"), &hash).is_err()
        );
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&((MAX_HEADER + 1) as u32).to_le_bytes());
        std::fs::write(&packed, bytes).unwrap();
        assert!(
            update_runtime_candidate(&packed, &base, &root.path().join("oversized"), &hash)
                .is_err()
        );
    }
    #[test]
    fn packing_verifies_omitted_source_assets_and_preserves_destination_on_refusal() {
        let root = tempfile::tempdir().unwrap();
        let (base, desired, hash) = setup(root.path());
        let packed = root.path().join("delta");
        std::fs::write(desired.join("runtimes/ffmpeg"), b"damaged omitted asset").unwrap();
        assert!(pack_runtime_update(&desired, &base, &packed, &hash).is_err());
        assert!(!packed.exists());
        assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pulsar-runtime-archive-")));
    }
}
