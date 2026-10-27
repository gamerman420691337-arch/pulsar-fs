//! Explicit recovery for recognized inactive archive construction directories.
use super::*;
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
pub(super) const PREFIX: &str = ".pulsar-runtime-archive-stage-";
pub(super) const MARKER: &str = ".pulsar-runtime-archive.json";
pub(super) const PAYLOAD: &str = "archive.part";
const LIMIT: u64 = 16384;
const MAX_ARCHIVE: u64 = 64 * 1024 * 1024 * 1024 + 512 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema_version: u16,
    stage_basename: String,
    destination_basename: String,
    expected_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct RuntimeArchiveStageInspection {
    pub state: RuntimeStageState,
    pub stage_sha256: Option<String>,
    pub destination_basename: Option<String>,
    pub expected_bytes: Option<u64>,
    pub present_bytes: Option<u64>,
    pub qualified: bool,
}
impl RuntimeArchiveStageInspection {
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
pub(super) fn write_marker(path: &Path, destination: &OsStr, total: u64) -> Result<File> {
    ensure!(
        (16..=MAX_ARCHIVE).contains(&total),
        "archive stage size exceeds admission"
    );
    let marker = Marker {
        schema_version: 1,
        stage_basename: path
            .file_name()
            .context("archive stage basename missing")?
            .to_str()
            .context("archive stage basename not UTF-8")?
            .to_owned(),
        destination_basename: destination
            .to_str()
            .context("archive destination basename not UTF-8")?
            .to_owned(),
        expected_bytes: total,
    };
    let encoded = serde_json::to_vec(&marker)?;
    ensure!(
        encoded.len() as u64 <= LIMIT,
        "archive stage marker exceeds admission"
    );
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path.join(MARKER))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    fs2::FileExt::lock_exclusive(&file)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    File::open(path)?.sync_all()?;
    Ok(file)
}
struct Held {
    parent: File,
    name: OsString,
    root: File,
    marker: File,
}
fn owned(file: &File, device: u64, mode: u32, limit: u64) -> Result<std::fs::Metadata> {
    let m = file.metadata()?;
    ensure!(
        m.is_file()
            && m.dev() == device
            && m.uid() == unsafe { libc::geteuid() }
            && m.nlink() == 1
            && m.mode() & 0o777 == mode
            && m.len() <= limit,
        "untrusted archive stage file"
    );
    Ok(m)
}
fn open(path: &Path) -> Result<Held> {
    let name = path.file_name().context("archive stage basename missing")?;
    let text = name.to_str().context("archive stage basename not UTF-8")?;
    ensure!(
        text.strip_prefix(PREFIX)
            .is_some_and(|suffix| !suffix.is_empty())
            && text.len() <= 128
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)),
        "not an archive stage basename"
    );
    let parent = directory(path.parent().context("archive stage parent missing")?)?;
    let meta = parent.metadata()?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o022 == 0,
        "archive stage parent must be owned and not writable by other users"
    );
    let root = child(&parent, name, true)?;
    let meta = root.metadata()?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o777 == 0o700,
        "untrusted archive stage directory"
    );
    let marker = child(&root, OsStr::new(MARKER), false)?;
    owned(&marker, meta.dev(), 0o600, LIMIT)?;
    Ok(Held {
        parent,
        name: name.to_owned(),
        root,
        marker,
    })
}
fn capture(held: &Held) -> Result<(Marker, String, std::fs::Metadata)> {
    let before = held.marker.metadata()?;
    ensure!(
        before.len() > 0 && before.len() <= LIMIT,
        "archive stage marker size differs"
    );
    let mut file = held.marker.try_clone()?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == before.len() && stable(&before, &file.metadata()?),
        "archive stage marker changed during capture"
    );
    let marker: Marker = serde_json::from_slice(&bytes)?;
    ensure!(
        marker.schema_version == 1
            && OsStr::new(&marker.stage_basename) == held.name
            && (16..=MAX_ARCHIVE).contains(&marker.expected_bytes)
            && !marker.destination_basename.is_empty()
            && marker.destination_basename.len() <= 1024
            && !marker.destination_basename.contains(['/', '\0']),
        "archive stage marker declaration differs"
    );
    Ok((marker, format!("{:x}", Sha256::digest(&bytes)), before))
}
fn inventory(held: &Held, marker: &Marker) -> Result<Option<std::fs::Metadata>> {
    let entries = recovery::list(&held.root, 2)?;
    ensure!(
        entries.iter().all(|n| n == MARKER || n == PAYLOAD),
        "unknown archive stage entry"
    );
    if entries.iter().any(|n| n == PAYLOAD) {
        Ok(Some(owned(
            &child(&held.root, OsStr::new(PAYLOAD), false)?,
            held.root.metadata()?.dev(),
            0o600,
            marker.expected_bytes,
        )?))
    } else {
        Ok(None)
    }
}
fn inspection_hash(
    held: &Held,
    marker_hash: &str,
    marker: &std::fs::Metadata,
    payload: Option<&std::fs::Metadata>,
) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"pulsar-archive-stage-inspection-v1\0");
    hash.update(marker_hash.as_bytes());
    let root = held.root.metadata()?;
    for n in [root.dev(), root.ino(), marker.dev(), marker.ino()] {
        hash.update(n.to_le_bytes());
    }
    hash.update([u8::from(payload.is_some())]);
    if let Some(m) = payload {
        for n in [
            m.dev(),
            m.ino(),
            m.len(),
            m.uid() as u64,
            m.mode() as u64,
            m.nlink(),
            m.mtime() as u64,
            m.mtime_nsec() as u64,
            m.ctime() as u64,
            m.ctime_nsec() as u64,
        ] {
            hash.update(n.to_le_bytes());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn inspect_runtime_archive_stage(path: &Path) -> Result<RuntimeArchiveStageInspection> {
    let held = open(path)?;
    match fs2::FileExt::try_lock_exclusive(&held.marker) {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            return Ok(RuntimeArchiveStageInspection {
                state: RuntimeStageState::Locked,
                stage_sha256: None,
                destination_basename: None,
                expected_bytes: None,
                present_bytes: None,
                qualified: false,
            })
        }
        Err(e) => return Err(e.into()),
        Ok(()) => {}
    }
    let (marker, marker_hash, marker_meta) = capture(&held)?;
    let payload = inventory(&held, &marker)?;
    let hash = inspection_hash(&held, &marker_hash, &marker_meta, payload.as_ref())?;
    Ok(RuntimeArchiveStageInspection {
        state: RuntimeStageState::Discardable,
        stage_sha256: Some(hash),
        destination_basename: Some(marker.destination_basename),
        expected_bytes: Some(marker.expected_bytes),
        present_bytes: Some(payload.map_or(0, |m| m.len())),
        qualified: false,
    })
}
/// Local explicit deletion of known entries only; no automatic scavenging.
pub fn discard_runtime_archive_stage(
    path: &Path,
    expected_stage_sha256: &str,
    approve_discard: bool,
) -> Result<()> {
    ensure!(
        approve_discard,
        "explicit archive stage discard approval required"
    );
    ensure!(
        expected_stage_sha256.len() == 64
            && expected_stage_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid expected archive stage identity"
    );
    let held = open(path)?;
    fs2::FileExt::try_lock_exclusive(&held.marker)
        .context("archive stage is locked by another operation")?;
    let (marker, marker_hash, marker_meta) = capture(&held)?;
    let payload = inventory(&held, &marker)?;
    let hash = inspection_hash(&held, &marker_hash, &marker_meta, payload.as_ref())?;
    ensure!(
        hash == expected_stage_sha256,
        "archive stage inspection identity differs"
    );
    let root_meta = held.root.metadata()?;
    recovery::same_entry(
        &held.parent,
        &held.name,
        root_meta.dev(),
        root_meta.ino(),
        true,
    )?;
    recovery::same_entry(
        &held.root,
        OsStr::new(MARKER),
        marker_meta.dev(),
        marker_meta.ino(),
        false,
    )?;
    if let Some(meta) = payload {
        let current = owned(
            &child(&held.root, OsStr::new(PAYLOAD), false)?,
            root_meta.dev(),
            0o600,
            marker.expected_bytes,
        )?;
        ensure!(
            current.ino() == meta.ino() && stable(&meta, &current),
            "archive stage payload changed"
        );
        recovery::remove(&held.root, OsStr::new(PAYLOAD), false)?;
        held.root.sync_all()?;
    }
    let entries = recovery::list(&held.root, 2)?;
    ensure!(
        entries == vec![OsString::from(MARKER)],
        "archive stage changed before marker discard"
    );
    let current = owned(
        &child(&held.root, OsStr::new(MARKER), false)?,
        root_meta.dev(),
        0o600,
        LIMIT,
    )?;
    ensure!(
        current.ino() == marker_meta.ino() && stable(&marker_meta, &current),
        "archive stage marker changed before discard"
    );
    recovery::remove(&held.root, OsStr::new(MARKER), false)?;
    held.root.sync_all()?;
    recovery::same_entry(
        &held.parent,
        &held.name,
        root_meta.dev(),
        root_meta.ino(),
        true,
    )?;
    recovery::remove(&held.parent, &held.name, true)?;
    held.parent
        .sync_all()
        .context("archive stage removed but parent sync failed; inspect before retry")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stage(root: &Path) -> (std::path::PathBuf, File) {
        let path = root.join(format!("{PREFIX}authored"));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let lock = write_marker(&path, OsStr::new("new archive"), 128).unwrap();
        (path, lock)
    }
    #[test]
    fn active_writer_is_locked_and_inactive_partial_or_empty_stage_requires_exact_approval() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock) = stage(root.path());
        assert_eq!(
            inspect_runtime_archive_stage(&path).unwrap().state,
            RuntimeStageState::Locked
        );
        assert!(discard_runtime_archive_stage(&path, &"0".repeat(64), true).is_err());
        drop(lock);
        let report = inspect_runtime_archive_stage(&path).unwrap();
        assert_eq!(report.present_bytes, Some(0));
        let hash = report.stage_sha256.unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, false).is_err());
        assert!(discard_runtime_archive_stage(&path, &"0".repeat(64), true).is_err());
        std::fs::write(path.join(PAYLOAD), b"partial").unwrap();
        std::fs::set_permissions(path.join(PAYLOAD), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        assert_eq!(
            inspect_runtime_archive_stage(&path).unwrap().present_bytes,
            Some(7)
        );
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        let current = inspect_runtime_archive_stage(&path)
            .unwrap()
            .stage_sha256
            .unwrap();
        discard_runtime_archive_stage(&path, &current, true).unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn unknown_symlink_hardlink_wrong_mode_and_oversize_refuse_without_deleting_payload() {
        let root = tempfile::tempdir().unwrap();
        let (path, lock) = stage(root.path());
        drop(lock);
        let hash = inspect_runtime_archive_stage(&path)
            .unwrap()
            .stage_sha256
            .unwrap();
        let payload = path.join(PAYLOAD);
        std::fs::write(&payload, b"partial").unwrap();
        std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(path.join("owner"), b"keep").unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        assert_eq!(std::fs::read(&payload).unwrap(), b"partial");
        std::fs::remove_file(path.join("owner")).unwrap();
        let extra = root.path().join("link");
        std::fs::hard_link(&payload, &extra).unwrap();
        assert!(inspect_runtime_archive_stage(&path).is_err());
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        std::fs::remove_file(extra).unwrap();
        std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        std::fs::remove_file(&payload).unwrap();
        std::os::unix::fs::symlink(path.join(MARKER), &payload).unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        std::fs::remove_file(&payload).unwrap();
        std::fs::write(&payload, vec![0; 129]).unwrap();
        std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        assert_eq!(std::fs::metadata(payload).unwrap().len(), 129);
    }
    #[test]
    fn legacy_opaque_files_changed_markers_and_copied_stage_identity_are_not_discarded() {
        let root = tempfile::tempdir().unwrap();
        let opaque = root.path().join(".pulsar-runtime-archive-legacy");
        std::fs::write(&opaque, b"old").unwrap();
        assert!(inspect_runtime_archive_stage(&opaque).is_err());
        assert_eq!(std::fs::read(opaque).unwrap(), b"old");
        let (path, lock) = stage(root.path());
        drop(lock);
        let hash = inspect_runtime_archive_stage(&path)
            .unwrap()
            .stage_sha256
            .unwrap();
        let marker = path.join(MARKER);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
        value["destination_basename"] = serde_json::json!("different");
        std::fs::write(&marker, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(discard_runtime_archive_stage(&path, &hash, true).is_err());
        let new = root.path().join(format!("{PREFIX}other"));
        std::fs::rename(&path, &new).unwrap();
        assert!(inspect_runtime_archive_stage(&new).is_err());
        assert!(new.join(MARKER).exists());
    }
}
