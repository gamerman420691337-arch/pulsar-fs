//! Versioned, uncompressed, bounded offline runtime candidate archive.
use super::*;
const MAGIC: &[u8; 12] = b"PULSARRT0001";
const HEADER_BYTES: u64 = 16;
pub(super) enum Input {
    Directory(File),
    Update(super::update::UpdateInput),
    Archive {
        file: File,
        before: std::fs::Metadata,
    },
}
impl Input {
    pub(super) fn open(path: &Path) -> Result<(Self, Vec<u8>, Manifest)> {
        ensure!(path.is_absolute(), "runtime source must be absolute");
        let file = open_source(path)?;
        let before = file.metadata()?;
        if before.is_dir() {
            let (bytes, manifest) = manifest(&file)?;
            manifest.validate()?;
            return Ok((Self::Directory(file), bytes, manifest));
        }
        ensure!(
            before.is_file(),
            "runtime source must be a regular archive or directory"
        );
        let mut file = file;
        let mut magic = [0; 12];
        file.read_exact(&mut magic)
            .context("truncated runtime archive header")?;
        ensure!(&magic == MAGIC, "unsupported runtime archive magic/version");
        let mut length = [0; 4];
        file.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        ensure!(
            length > 0 && length <= 65536,
            "runtime archive manifest exceeds admission"
        );
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)
            .context("truncated runtime archive manifest")?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        let total = manifest.validate()?;
        let expected = HEADER_BYTES
            .checked_add(length as u64)
            .and_then(|n| n.checked_add(total))
            .context("runtime archive size overflow")?;
        ensure!(
            before.len() == expected && stable(&before, &file.metadata()?),
            "runtime archive length/metadata differs"
        );
        Ok((Self::Archive { file, before }, bytes, manifest))
    }
    pub(super) fn copy_asset(
        &mut self,
        r: &Record,
        output: &mut File,
        deadline: Instant,
    ) -> Result<()> {
        match self {
            Self::Directory(root) => {
                copy_open_asset(source_file(root, &r.path)?, r, output, deadline)?
            }
            Self::Update(input) => input.copy_asset(r, output, deadline)?,
            Self::Archive { file, .. } => copy_verified(file, r, output, deadline)?,
        }
        Ok(())
    }
    pub(super) fn finish(&mut self) -> Result<()> {
        if let Self::Update(input) = self {
            input.finish()?;
        }
        if let Self::Archive { file, before } = self {
            let mut extra = [0];
            ensure!(
                file.read(&mut extra)? == 0 && stable(before, &file.metadata()?),
                "runtime archive changed or has trailing bytes"
            );
        }
        Ok(())
    }
}
pub(super) fn open_source(path: &Path) -> Result<File> {
    ensure!(path.is_absolute(), "runtime source must be absolute");
    let file = if let Some(name) = path.file_name() {
        child(
            &directory(path.parent().context("runtime source parent missing")?)?,
            name,
            false,
        )?
    } else {
        directory(path)?
    };
    Ok(file)
}
pub(super) fn copy_open_asset(
    mut input: File,
    r: &Record,
    output: &mut File,
    deadline: Instant,
) -> Result<()> {
    let before = input.metadata()?;
    ensure!(before.len() == r.byte_len, "candidate file length differs");
    copy_verified(&mut input, r, output, deadline)?;
    let mut extra = [0];
    ensure!(
        input.read(&mut extra)? == 0 && stable(&before, &input.metadata()?),
        "candidate file changed during capture"
    );
    Ok(())
}
pub(super) fn copy_verified(
    input: &mut File,
    r: &Record,
    output: &mut File,
    deadline: Instant,
) -> Result<()> {
    let mut remaining = r.byte_len;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    while remaining > 0 {
        ensure!(
            Instant::now() < deadline,
            "runtime archive/copy deadline exceeded"
        );
        let length = remaining.min(buffer.len() as u64) as usize;
        let n = input.read(&mut buffer[..length])?;
        ensure!(n > 0, "truncated runtime candidate payload: {}", r.path);
        remaining -= n as u64;
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n])?;
    }
    ensure!(
        format!("{:x}", hash.finalize()) == r.sha256,
        "candidate file integrity differs: {}",
        r.path
    );
    Ok(())
}
/// Create a new private single-file archive without Engine startup or asset activation.
pub fn pack_runtime_candidate(source: &Path, destination: &Path) -> Result<String> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "runtime candidate packaging requires Linux x86-64"
    );
    ensure!(
        source.is_absolute() && destination.is_absolute(),
        "runtime archive paths must be absolute"
    );
    let root = directory(source)?;
    let (bytes, manifest) = manifest(&root)?;
    let total = manifest.validate()?;
    generation::verify(&root, &manifest.generation_packs)?;
    publish_archive(
        destination,
        total + HEADER_BYTES + bytes.len() as u64,
        |output, deadline| {
            output.write_all(MAGIC)?;
            output.write_all(&(bytes.len() as u32).to_le_bytes())?;
            output.write_all(&bytes)?;
            let mut input = Input::Directory(root);
            for r in &manifest.files {
                input.copy_asset(r, output, deadline)?;
            }
            input.finish()?;
            Ok(())
        },
    )?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

pub(super) fn publish_archive(
    destination: &Path,
    total: u64,
    write: impl FnOnce(&mut File, Instant) -> Result<()>,
) -> Result<()> {
    let parent = directory(destination.parent().context("archive parent missing")?)?;
    let metadata = parent.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
        "archive parent must be owned and not writable by other users"
    );
    let name = destination
        .file_name()
        .context("archive basename missing")?;
    let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd()));
    ensure!(
        std::fs::symlink_metadata(anchored.join(name))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "archive destination already exists or is inaccessible"
    );
    ensure!(
        fs2::available_space(&anchored)? > total + 1024 * 1024,
        "insufficient runtime archive space"
    );
    let staged = tempfile::Builder::new()
        .prefix(archive_recovery::PREFIX)
        .tempdir_in(&anchored)?;
    std::fs::set_permissions(staged.path(), std::fs::Permissions::from_mode(0o700))?;
    let stage_dir = File::open(staged.path())?;
    let _marker = archive_recovery::write_marker(staged.path(), name, total)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(staged.path().join(archive_recovery::PAYLOAD))?;
    output.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    let deadline = Instant::now() + Duration::from_secs(600);
    write(&mut output, deadline)?;
    output.sync_all()?;
    ensure!(
        Instant::now() < deadline,
        "runtime archive deadline exceeded before publication"
    );
    let from = CString::new(archive_recovery::PAYLOAD)?;
    let to = CString::new(name.as_bytes())?;
    // SAFETY: held parent and two single components; atomic no-overwrite publication.
    let published = unsafe {
        libc::renameat2(
            stage_dir.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    ensure!(
        published == 0,
        "cannot publish runtime archive: {}",
        std::io::Error::last_os_error()
    );
    stage_dir
        .sync_all()
        .context("runtime archive published but stage sync failed; inspect before retry")?;
    parent
        .sync_all()
        .context("runtime archive published but parent sync failed; inspect before retry")?;
    staged.close().context(
        "runtime archive published but construction-stage cleanup failed; inspect before retry",
    )?;
    parent
        .sync_all()
        .context("runtime archive published but cleanup sync failed; inspect before retry")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_roundtrip_preserves_original_manifest_and_every_payload() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let archive = root.path().join("runtime.pulsar-runtime");
        let dest = root.path().join("installed");
        let hash = pack_runtime_candidate(&source, &archive).unwrap();
        assert_eq!(install_runtime_candidate(&archive, &dest).unwrap(), hash);
        assert_eq!(
            std::fs::read(source.join("runtime-candidate.json")).unwrap(),
            std::fs::read(dest.join("runtime-candidate.json")).unwrap()
        );
        for n in BASE {
            assert_eq!(
                std::fs::read(source.join(n)).unwrap(),
                std::fs::read(dest.join(n)).unwrap()
            );
        }
        let captured = std::fs::read(&archive).unwrap();
        assert!(pack_runtime_candidate(&source, &archive).is_err());
        assert_eq!(std::fs::read(archive).unwrap(), captured);
    }
    #[test]
    fn truncated_trailing_wrong_header_and_late_corrupt_archives_never_publish() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let archive = root.path().join("runtime.pulsar-runtime");
        pack_runtime_candidate(&source, &archive).unwrap();
        let original = std::fs::read(&archive).unwrap();
        for kind in [
            "truncated",
            "trailing",
            "header",
            "late-payload",
            "manifest-length",
        ] {
            let mut bytes = original.clone();
            match kind {
                "truncated" => {
                    bytes.pop();
                }
                "trailing" => bytes.push(0),
                "header" => bytes[0] ^= 1,
                "manifest-length" => bytes[12..16].copy_from_slice(&65537u32.to_le_bytes()),
                _ => {
                    let end = bytes.len() - 1;
                    bytes[end] ^= 1;
                }
            }
            let input = root.path().join(kind);
            std::fs::write(&input, bytes).unwrap();
            let dest = root.path().join(format!("out-{kind}"));
            assert!(install_runtime_candidate(&input, &dest).is_err(), "{kind}");
            assert!(!dest.exists());
            assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".pulsar-runtime-stage-")));
        }
    }
    #[test]
    fn pack_corruption_and_symlink_source_do_not_publish_archive() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let target = root.path().join("out");
        let last = source.join(BASE[7]);
        let original = std::fs::read(&last).unwrap();
        std::fs::write(last, vec![b'x'; original.len()]).unwrap();
        assert!(pack_runtime_candidate(&source, &target).is_err());
        assert!(!target.exists());
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(pack_runtime_candidate(&link, &target).is_err());
        assert!(!target.exists());
    }
}
