//! Local owner inspection/discard of recognized inactive installation stages.
use super::*;
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::PathBuf};

pub(super) const MARKER: &str = ".pulsar-runtime-stage.json";
const MARKER_LIMIT: u64 = 512 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StageMarker {
    schema_version: u16,
    manifest_sha256: String,
    manifest_json: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStageState {
    Locked,
    Discardable,
}
#[derive(Debug, Serialize)]
pub struct RuntimeStageInspection {
    pub state: RuntimeStageState,
    pub manifest_sha256: Option<String>,
    pub present_files: Option<usize>,
    pub present_bytes: Option<u64>,
}
impl RuntimeStageInspection {
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
pub(super) fn write_marker(stage: &Path, bytes: &[u8]) -> Result<File> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(stage.join(MARKER))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    fs2::FileExt::lock_exclusive(&file)?;
    let value = StageMarker {
        schema_version: 1,
        manifest_sha256: format!("{:x}", Sha256::digest(bytes)),
        manifest_json: String::from_utf8(bytes.to_vec())?,
    };
    let encoded = serde_json::to_vec(&value)?;
    ensure!(
        encoded.len() as u64 <= MARKER_LIMIT,
        "runtime stage marker exceeds admission"
    );
    file.write_all(&encoded)?;
    file.sync_all()?;
    File::open(stage)?.sync_all()?;
    Ok(file)
}
struct HeldStage {
    parent: File,
    name: OsString,
    directory: File,
    marker: File,
}
fn open_stage(path: &Path) -> Result<HeldStage> {
    let name = path.file_name().context("runtime stage basename missing")?;
    let text = name
        .to_str()
        .context("runtime stage basename is not portable")?;
    ensure!(
        text.starts_with(".pulsar-runtime-stage-")
            && text.len() > 22
            && text.len() <= 128
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)),
        "not a runtime installation stage basename"
    );
    let parent = directory(path.parent().context("runtime stage parent missing")?)?;
    let parent_meta = parent.metadata()?;
    ensure!(
        parent_meta.uid() == unsafe { libc::geteuid() } && parent_meta.mode() & 0o022 == 0,
        "runtime stage parent must be owned and not writable by other users"
    );
    let directory = child(&parent, name, true)?;
    let metadata = directory.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o777 == 0o700,
        "untrusted runtime stage directory"
    );
    let marker = child(&directory, std::ffi::OsStr::new(MARKER), false)?;
    let metadata = marker.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.nlink() == 1
            && metadata.mode() & 0o777 == 0o600
            && metadata.len() <= MARKER_LIMIT,
        "untrusted runtime stage marker"
    );
    Ok(HeldStage {
        parent,
        name: name.to_owned(),
        directory,
        marker,
    })
}
fn capture_marker(file: &File) -> Result<(StageMarker, Manifest)> {
    let mut file = file.try_clone()?;
    let before = file.metadata()?;
    ensure!(
        before.len() > 0 && before.len() <= MARKER_LIMIT,
        "runtime stage marker size differs"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MARKER_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == before.len() && stable(&before, &file.metadata()?),
        "runtime stage marker changed during capture"
    );
    let marker: StageMarker = serde_json::from_slice(&bytes)?;
    ensure!(
        marker.schema_version == 1
            && marker.manifest_json.len() <= 65536
            && format!("{:x}", Sha256::digest(marker.manifest_json.as_bytes()))
                == marker.manifest_sha256,
        "runtime stage marker identity differs"
    );
    let manifest: Manifest = serde_json::from_str(&marker.manifest_json)?;
    manifest.validate()?;
    Ok((marker, manifest))
}
struct Entry {
    parent: File,
    name: OsString,
    device: u64,
    inode: u64,
    directory: bool,
    bytes: u64,
}
pub(super) fn list(directory: &File, limit: usize) -> Result<Vec<OsString>> {
    let path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let mut entries = std::fs::read_dir(path)?
        .take(limit + 1)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() <= limit,
        "runtime stage inventory exceeds admission"
    );
    entries.sort();
    Ok(entries)
}
fn inventory(stage: &HeldStage, marker: &StageMarker, manifest: &Manifest) -> Result<Vec<Entry>> {
    let device = stage.directory.metadata()?.dev();
    let mut entries = Vec::new();
    let mut directories = Vec::new();
    // Preflight every declared level before deleting anything; held parents
    // retain identity and directories are removed after descendants.
    let mut pending = vec![(stage.directory.try_clone()?, String::new())];
    while let Some((parent, prefix)) = pending.pop() {
        for name in list(&parent, manifest.files.len() + 2)? {
            let text = name.to_str().context("nonportable runtime stage entry")?;
            if prefix.is_empty() && text == MARKER {
                continue;
            }
            let relative = format!("{prefix}{text}");
            let subtree_prefix = format!("{relative}/");
            if manifest
                .files
                .iter()
                .any(|r| r.path.starts_with(&subtree_prefix))
            {
                let directory = child(&parent, &name, true)?;
                let metadata = directory.metadata()?;
                ensure!(
                    metadata.dev() == device
                        && metadata.uid() == unsafe { libc::geteuid() }
                        && metadata.mode() & 0o777 == 0o700,
                    "untrusted runtime stage subtree"
                );
                directories.push(Entry {
                    parent: parent.try_clone()?,
                    name,
                    device: metadata.dev(),
                    inode: metadata.ino(),
                    directory: true,
                    bytes: 0,
                });
                pending.push((directory, subtree_prefix));
            } else {
                let (limit, mode) = if relative == "runtime-candidate.json" {
                    (marker.manifest_json.len() as u64, 0o644)
                } else {
                    let record = manifest
                        .files
                        .iter()
                        .find(|r| r.path == relative)
                        .context("unknown runtime stage entry")?;
                    (
                        record.byte_len,
                        if executable(&relative) { 0o755 } else { 0o644 },
                    )
                };
                entries.push(checked_entry(&parent, name, device, limit, mode)?);
            }
        }
    }
    entries.extend(directories.into_iter().rev());
    Ok(entries)
}
fn checked_entry(
    parent: &File,
    name: OsString,
    device: u64,
    limit: u64,
    mode: u32,
) -> Result<Entry> {
    let file = child(parent, &name, false)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.dev() == device
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.nlink() == 1
            && metadata.mode() & 0o777 == mode
            && metadata.len() <= limit,
        "untrusted runtime stage file"
    );
    Ok(Entry {
        parent: parent.try_clone()?,
        name,
        device: metadata.dev(),
        inode: metadata.ino(),
        directory: false,
        bytes: metadata.len(),
    })
}
pub(super) fn same_entry(
    parent: &File,
    name: &std::ffi::OsStr,
    device: u64,
    inode: u64,
    is_directory: bool,
) -> Result<()> {
    let file = child(parent, name, is_directory)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.dev() == device && metadata.ino() == inode && metadata.is_dir() == is_directory,
        "runtime stage entry binding changed"
    );
    Ok(())
}
pub(super) fn remove(parent: &File, name: &std::ffi::OsStr, directory: bool) -> Result<()> {
    let name = CString::new(name.as_bytes())?;
    // SAFETY: live held parent, one component and explicit file/directory removal flag.
    let result = unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    };
    ensure!(
        result == 0,
        "cannot discard runtime stage entry: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
pub fn inspect_runtime_stage(path: &Path) -> Result<RuntimeStageInspection> {
    let stage = open_stage(path)?;
    match fs2::FileExt::try_lock_exclusive(&stage.marker) {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            return Ok(RuntimeStageInspection {
                state: RuntimeStageState::Locked,
                manifest_sha256: None,
                present_files: None,
                present_bytes: None,
            })
        }
        Err(error) => return Err(error.into()),
        Ok(()) => {}
    }
    let (marker, manifest) = capture_marker(&stage.marker)?;
    let entries = inventory(&stage, &marker, &manifest)?;
    Ok(RuntimeStageInspection {
        state: RuntimeStageState::Discardable,
        manifest_sha256: Some(marker.manifest_sha256),
        present_files: Some(entries.iter().filter(|e| !e.directory).count()),
        present_bytes: Some(entries.iter().map(|e| e.bytes).sum()),
    })
}
/// Explicit local owner deletion, restricted to a recognized inactive stage and
/// an exact inspection identity. Unknown entries are never recursively removed.
pub fn discard_runtime_stage(
    path: &Path,
    expected_manifest_sha256: &str,
    approve_discard: bool,
) -> Result<()> {
    ensure!(
        approve_discard,
        "explicit runtime stage discard approval required"
    );
    ensure!(
        expected_manifest_sha256.len() == 64
            && expected_manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid expected runtime stage identity"
    );
    let stage = open_stage(path)?;
    fs2::FileExt::try_lock_exclusive(&stage.marker)
        .context("runtime stage is locked by another operation")?;
    let (marker, manifest) = capture_marker(&stage.marker)?;
    ensure!(
        marker.manifest_sha256 == expected_manifest_sha256,
        "runtime stage inspection identity differs"
    );
    let bound_marker = stage.marker.metadata()?;
    same_entry(
        &stage.directory,
        std::ffi::OsStr::new(MARKER),
        bound_marker.dev(),
        bound_marker.ino(),
        false,
    )?;
    let entries = inventory(&stage, &marker, &manifest)?;
    let root_meta = stage.directory.metadata()?;
    let marker_meta = stage.marker.metadata()?;
    same_entry(
        &stage.parent,
        &stage.name,
        root_meta.dev(),
        root_meta.ino(),
        true,
    )?;
    for entry in entries {
        same_entry(
            &entry.parent,
            &entry.name,
            entry.device,
            entry.inode,
            entry.directory,
        )?;
        remove(&entry.parent, &entry.name, entry.directory)?;
        entry.parent.sync_all()?;
    }
    same_entry(
        &stage.directory,
        std::ffi::OsStr::new(MARKER),
        marker_meta.dev(),
        marker_meta.ino(),
        false,
    )?;
    remove(&stage.directory, std::ffi::OsStr::new(MARKER), false)?;
    stage.directory.sync_all()?;
    same_entry(
        &stage.parent,
        &stage.name,
        root_meta.dev(),
        root_meta.ino(),
        true,
    )?;
    remove(&stage.parent, &stage.name, true)?;
    stage
        .parent
        .sync_all()
        .context("runtime stage removed but parent sync failed; inspect before retry")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stage(root: &Path) -> (PathBuf, File, String) {
        let source = super::super::tests::fixture(root);
        let bytes = std::fs::read(source.join("runtime-candidate.json")).unwrap();
        let path = root.join(".pulsar-runtime-stage-owned");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let marker = write_marker(&path, &bytes).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        (path, marker, hash)
    }
    #[test]
    fn live_marker_lock_is_observed_and_blocks_discard() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock, hash) = stage(root.path());
        let report = inspect_runtime_stage(&path).unwrap();
        assert_eq!(report.state, RuntimeStageState::Locked);
        assert!(report.manifest_sha256.is_none());
        assert!(discard_runtime_stage(&path, &hash, true).is_err());
        assert!(path.exists());
        drop(lock);
        let report = inspect_runtime_stage(&path).unwrap();
        assert_eq!(report.state, RuntimeStageState::Discardable);
        assert_eq!(report.manifest_sha256.as_deref(), Some(hash.as_str()));
        assert_eq!(report.present_files, Some(0));
        discard_runtime_stage(&path, &hash, true).unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn approval_stale_identity_and_unknown_entries_refuse_before_any_deletion() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock, hash) = stage(root.path());
        drop(lock);
        let file = path.join("pulsar");
        std::fs::write(&file, b"partial").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(discard_runtime_stage(&path, &hash, false).is_err());
        assert!(discard_runtime_stage(&path, &"0".repeat(64), true).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"partial");
        std::fs::write(path.join("owner-file"), b"must remain").unwrap();
        assert!(discard_runtime_stage(&path, &hash, true).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"partial");
        assert_eq!(
            std::fs::read(path.join("owner-file")).unwrap(),
            b"must remain"
        );
        std::fs::remove_file(path.join("owner-file")).unwrap();
        let report = inspect_runtime_stage(&path).unwrap();
        assert_eq!(report.present_files, Some(1));
        assert_eq!(report.present_bytes, Some(7));
        discard_runtime_stage(&path, &hash, true).unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn managed_subtree_removal_preserves_external_target_and_refuses_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock, hash) = stage(root.path());
        drop(lock);
        std::fs::create_dir(path.join("runtimes")).unwrap();
        std::fs::set_permissions(
            path.join("runtimes"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let external = root.path().join("external");
        std::fs::write(&external, b"keep").unwrap();
        std::os::unix::fs::symlink(&external, path.join("runtimes/ffmpeg")).unwrap();
        assert!(discard_runtime_stage(&path, &hash, true).is_err());
        assert_eq!(std::fs::read(&external).unwrap(), b"keep");
        std::fs::remove_file(path.join("runtimes/ffmpeg")).unwrap();
        std::fs::write(path.join("runtimes/ffmpeg"), b"partial").unwrap();
        std::fs::set_permissions(
            path.join("runtimes/ffmpeg"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        discard_runtime_stage(&path, &hash, true).unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read(&external).unwrap(), b"keep");
    }
    #[test]
    fn corrupt_missing_markers_and_published_basename_are_not_discardable() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock, hash) = stage(root.path());
        drop(lock);
        std::fs::write(path.join(MARKER), b"{}").unwrap();
        assert!(inspect_runtime_stage(&path).is_err());
        assert!(discard_runtime_stage(&path, &hash, true).is_err());
        assert!(path.exists());
        std::fs::remove_file(path.join(MARKER)).unwrap();
        assert!(discard_runtime_stage(&path, &hash, true).is_err());
        let other = root.path().join("installed");
        std::fs::rename(&path, &other).unwrap();
        assert!(discard_runtime_stage(&other, &hash, true).is_err());
        assert!(other.exists());
    }
}
