//! Persistent verified runtime custody before a transient distribution exits.
use super::*;
use std::{collections::BTreeMap, path::PathBuf};

pub struct PreparedRuntime {
    pub directory: PathBuf,
    pub manifest_sha256: String,
    pub reused: bool,
}

/// Install once into a manifest-addressed private directory, then verify every
/// cached byte before launch. Never replace or delete a cache entry.
pub fn prepare_runtime_launch(source: &Path, cache_root: &Path) -> Result<PreparedRuntime> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "runtime launch requires Linux x86-64"
    );
    let (_, bytes, manifest) = archive::Input::open(source)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let cache = open_cache(cache_root, true)?;
    let _lock = lock_cache(&cache, true)?;
    let destination = cache_root.join(&hash);
    let anchored = PathBuf::from(format!("/proc/self/fd/{}", cache.as_raw_fd())).join(&hash);
    let reused = match std::fs::symlink_metadata(&anchored) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    ensure_same_directory(cache_root, &cache)?;
    if !reused {
        ensure!(
            install_runtime_candidate(source, &destination)? == hash,
            "runtime source manifest changed during first-use installation"
        );
    }
    let installed = child(&cache, std::ffi::OsStr::new(&hash), true)?;
    verify_cached(&installed, &bytes, &manifest)?;
    ensure_same_directory(cache_root, &cache)?;
    let path_meta = directory(&destination)?.metadata()?;
    let held_meta = installed.metadata()?;
    ensure!(
        path_meta.dev() == held_meta.dev() && path_meta.ino() == held_meta.ino(),
        "runtime cache entry changed during verification"
    );
    Ok(PreparedRuntime {
        directory: destination,
        manifest_sha256: hash,
        reused,
    })
}

/// Verify an explicitly selected retained runtime without its supplier bundle.
/// Never creates a missing cache/lock, repairs assets or chooses a latest version.
pub fn prepare_cached_runtime(cache_root: &Path, manifest_sha256: &str) -> Result<PreparedRuntime> {
    with_cached_runtime(cache_root, manifest_sha256, |directory, _| {
        Ok(PreparedRuntime {
            directory: directory.to_owned(),
            manifest_sha256: manifest_sha256.to_owned(),
            reused: true,
        })
    })
}

pub fn inspect_cached_generation_packs(
    cache_root: &Path,
    manifest_sha256: &str,
) -> Result<generation::BundledGenerationReport> {
    with_cached_runtime(cache_root, manifest_sha256, |directory, manifest| {
        Ok(generation::report(
            directory,
            manifest_sha256,
            &manifest.generation_packs,
        ))
    })
}

fn with_cached_runtime<T>(
    cache_root: &Path,
    manifest_sha256: &str,
    finish: impl FnOnce(&Path, &Manifest) -> Result<T>,
) -> Result<T> {
    ensure!(
        cfg!(target_arch = "x86_64"),
        "cached runtime launch requires Linux x86-64"
    );
    ensure!(
        manifest_sha256.len() == 64
            && manifest_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "cached runtime identity must be 64 lowercase hexadecimal characters"
    );
    let cache = open_cache(cache_root, false)?;
    let _lock = lock_cache(&cache, false)?;
    let installed = child(&cache, std::ffi::OsStr::new(manifest_sha256), true)?;
    let (bytes, manifest) = super::manifest(&installed)?;
    manifest.validate()?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == manifest_sha256,
        "cached runtime manifest identity differs; preserved for inspection"
    );
    verify_cached(&installed, &bytes, &manifest)?;
    ensure_same_directory(cache_root, &cache)?;
    let destination = cache_root.join(manifest_sha256);
    ensure_same_directory(&destination, &installed)?;
    finish(&destination, &manifest)
}

fn lock_cache(cache: &File, create: bool) -> Result<File> {
    let lock_name = std::ffi::OsStr::new(".pulsar-runtime-cache.lock");
    let lock_path = PathBuf::from(format!("/proc/self/fd/{}", cache.as_raw_fd())).join(lock_name);
    let (lock, created) = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .mode(0o600)
        .open(&lock_path)
    {
        Ok(file) => (file, create),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&lock_path)?,
            false,
        ),
        Err(e) => return Err(e.into()),
    };
    if created {
        lock.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        lock.sync_all()?;
        cache.sync_all()?;
    }
    let metadata = lock.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.nlink() == 1
            && metadata.mode() & 0o777 == 0o600
            && metadata.len() == 0,
        "untrusted runtime cache lock"
    );
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("runtime cache preparation is busy; retry after the current preparation")?;
    Ok(lock)
}

fn ensure_same_directory(path: &Path, held: &File) -> Result<()> {
    let a = directory(path)?.metadata()?;
    let b = held.metadata()?;
    ensure!(
        a.dev() == b.dev() && a.ino() == b.ino(),
        "runtime cache path changed"
    );
    Ok(())
}

fn open_cache(path: &Path, create: bool) -> Result<File> {
    ensure!(
        path.is_absolute() && path.file_name().is_some(),
        "runtime cache root must be an absolute directory"
    );
    let mut current = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let text = CString::new(name.as_bytes())?;
                // SAFETY: held parent and one component; no recursive path traversal.
                let made = if create {
                    unsafe { libc::mkdirat(current.as_raw_fd(), text.as_ptr(), 0o700) }
                } else {
                    -1
                };
                let created = create && made == 0;
                if create && !created {
                    ensure!(
                        std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
                        "cannot create runtime cache component: {}",
                        std::io::Error::last_os_error()
                    );
                }
                if created {
                    // O_PATH admits a newly created mode000 directory under an
                    // umask0777. chmod through its held descriptor, never a name.
                    let fd = unsafe {
                        libc::openat(
                            current.as_raw_fd(),
                            text.as_ptr(),
                            libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    ensure!(fd >= 0, "cannot hold created runtime cache directory");
                    let held = unsafe { File::from_raw_fd(fd) };
                    std::fs::set_permissions(
                        format!("/proc/self/fd/{}", held.as_raw_fd()),
                        std::fs::Permissions::from_mode(0o700),
                    )?;
                }
                let next = child(&current, name, true)?;
                let metadata = next.metadata()?;
                ensure!(
                    (metadata.uid() == unsafe { libc::geteuid() } || metadata.uid() == 0)
                        && (metadata.mode() & 0o022 == 0
                            || (metadata.uid() == 0 && metadata.mode() & 0o1000 != 0)),
                    "runtime cache ancestors must be trusted and not writable by other users except root-owned sticky directories"
                );
                if created {
                    next.sync_all()?;
                    current.sync_all()?;
                }
                current = next;
            }
            _ => anyhow::bail!("runtime cache path must contain normal components"),
        }
    }
    let metadata = current.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o777 == 0o700,
        "runtime cache root must be private and owned by the current user"
    );
    Ok(current)
}

fn verify_cached(root: &File, bytes: &[u8], manifest: &Manifest) -> Result<()> {
    let metadata = root.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o777 == 0o700,
        "untrusted installed runtime directory"
    );
    let device = metadata.dev();
    let (actual, _) = super::manifest(root)?;
    ensure!(
        actual == bytes,
        "cached runtime manifest differs; preserved for inspection"
    );
    let mut expected = BTreeMap::<String, BTreeSet<String>>::new();
    let mut top = BTreeSet::from([
        "runtime-candidate.json".to_owned(),
        recovery::MARKER.to_owned(),
    ]);
    for record in &manifest.files {
        let parts = record.path.split('/').collect::<Vec<_>>();
        top.insert(parts[0].to_owned());
        for depth in 1..parts.len() {
            expected
                .entry(parts[..depth].join("/"))
                .or_default()
                .insert(parts[depth].to_owned());
        }
    }
    check_names(root, &top)?;
    for (name, names) in expected {
        let mut subtree = root.try_clone()?;
        for part in name.split('/') {
            subtree = child(&subtree, std::ffi::OsStr::new(part), true)?;
        }
        let m = subtree.metadata()?;
        ensure!(
            m.dev() == device && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o777 == 0o700,
            "untrusted installed runtime subtree"
        );
        check_names(&subtree, &names)?;
    }
    check_file(
        root,
        "runtime-candidate.json",
        bytes.len() as u64,
        0o644,
        device,
    )?;
    let mut marker = check_file(root, recovery::MARKER, 512 * 1024, 0o600, device)?;
    let before = marker.metadata()?;
    let mut captured = Vec::new();
    (&mut marker)
        .take(512 * 1024 + 1)
        .read_to_end(&mut captured)?;
    ensure!(
        captured.len() <= 512 * 1024
            && captured.len() as u64 == before.len()
            && stable(&before, &marker.metadata()?),
        "cached runtime receipt changed or exceeded admission"
    );
    let receipt: serde_json::Value = serde_json::from_slice(&captured)?;
    let fields = receipt
        .as_object()
        .context("invalid cached runtime receipt")?;
    ensure!(
        fields.len() == 3
            && receipt["schema_version"] == 1
            && receipt["manifest_json"].as_str() == Some(std::str::from_utf8(bytes)?)
            && receipt["manifest_sha256"].as_str()
                == Some(format!("{:x}", Sha256::digest(bytes)).as_str()),
        "cached runtime receipt identity differs"
    );
    let deadline = Instant::now() + Duration::from_secs(600);
    for record in &manifest.files {
        let file = check_file(
            root,
            &record.path,
            record.byte_len,
            if executable(&record.path) {
                0o755
            } else {
                0o644
            },
            device,
        )?;
        ensure!(
            file.metadata()?.len() == record.byte_len,
            "cached runtime payload length differs"
        );
        // Reuse the directory capture path: fixed buffer, exact EOF and stable
        // metadata. No inference, model loading or byte-sized allocation.
        let mut input = archive::Input::Directory(root.try_clone()?);
        input.copy_asset(
            record,
            &mut File::options().write(true).open("/dev/null")?,
            deadline,
        )?;
    }
    generation::verify(root, &manifest.generation_packs)?;
    Ok(())
}
fn check_names(root: &File, expected: &BTreeSet<String>) -> Result<()> {
    let path = PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
    let actual = std::fs::read_dir(path)?
        .take(expected.len() + 1)
        .map(|e| {
            let e = e?;
            e.file_name()
                .into_string()
                .map_err(|_| std::io::Error::other("nonportable cached runtime entry"))
        })
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    ensure!(
        &actual == expected,
        "cached runtime inventory differs; preserved for inspection"
    );
    Ok(())
}
fn check_file(root: &File, path: &str, limit: u64, mode: u32, device: u64) -> Result<File> {
    let file = source_file(root, path)?;
    let m = file.metadata()?;
    ensure!(
        m.dev() == device
            && m.uid() == unsafe { libc::geteuid() }
            && m.nlink() == 1
            && m.mode() & 0o777 == mode
            && m.len() <= limit,
        "untrusted cached runtime file"
    );
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_return_survives_failed_update_and_missing_supplier() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let cache = root.path().join("cache");
        let old = prepare_runtime_launch(&source, &cache).unwrap();
        let before = std::fs::metadata(old.directory.join("pulsar")).unwrap();
        let path = source.join("runtime-candidate.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let payload = b"authored updated executable";
        std::fs::write(source.join("pulsar"), payload).unwrap();
        let record = manifest["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["path"] == "pulsar")
            .unwrap();
        record["byte_len"] = payload.len().into();
        record["sha256"] = format!("{:x}", Sha256::digest(payload)).into();
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let updated = prepare_runtime_launch(&source, &cache).unwrap();
        assert_ne!(old.manifest_sha256, updated.manifest_sha256);
        std::fs::write(updated.directory.join("pulsar-worker"), b"damaged update").unwrap();
        std::fs::remove_dir_all(&source).unwrap();
        assert!(prepare_cached_runtime(&cache, &updated.manifest_sha256).is_err());
        let retained = prepare_cached_runtime(&cache, &old.manifest_sha256).unwrap();
        assert!(retained.reused);
        assert_eq!(retained.directory, old.directory);
        let after = std::fs::metadata(old.directory.join("pulsar")).unwrap();
        assert!(stable(&before, &after));
        assert_eq!(before.ino(), after.ino());
        assert_eq!(
            std::fs::read(updated.directory.join("pulsar-worker")).unwrap(),
            b"damaged update"
        );
    }
    #[test]
    fn retained_selection_never_creates_missing_cache_lock_or_entry() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("missing/cache");
        let valid = "a".repeat(64);
        for id in [
            "../escape".to_owned(),
            "A".repeat(64),
            "a".repeat(63),
            valid.clone(),
        ] {
            assert!(prepare_cached_runtime(&cache, &id).is_err());
            assert!(!root.path().join("missing").exists());
        }
        let source = super::super::tests::fixture(root.path());
        let retained = prepare_runtime_launch(&source, &cache).unwrap();
        assert!(prepare_cached_runtime(&cache, &valid).is_err());
        assert!(!cache.join(&valid).exists());
        let lock = cache.join(".pulsar-runtime-cache.lock");
        std::fs::remove_file(&lock).unwrap();
        assert!(prepare_cached_runtime(&cache, &retained.manifest_sha256).is_err());
        assert!(!lock.exists());
        assert!(retained.directory.exists());
    }
    #[test]
    fn first_use_installs_and_exact_reuse_preserves_bytes() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let cache = root.path().join("new/cache");
        let a = prepare_runtime_launch(&source, &cache).unwrap();
        assert!(!a.reused);
        let b = prepare_runtime_launch(&source, &cache).unwrap();
        assert!(b.reused);
        assert_eq!(a.directory, b.directory);
    }
    #[test]
    fn cached_damage_unknown_files_and_symlinks_are_preserved_and_refused() {
        for kind in ["payload", "unknown", "symlink", "receipt"] {
            let root = tempfile::tempdir().unwrap();
            let source = super::super::tests::fixture(root.path());
            let cache = root.path().join("cache");
            let a = prepare_runtime_launch(&source, &cache).unwrap();
            match kind {
                "payload" => std::fs::write(a.directory.join("pulsar"), b"damage").unwrap(),
                "unknown" => std::fs::write(a.directory.join("owner-file"), b"preserve").unwrap(),
                "receipt" => std::fs::write(a.directory.join(recovery::MARKER), b"{}").unwrap(),
                _ => {
                    std::fs::remove_file(a.directory.join("pulsar")).unwrap();
                    std::os::unix::fs::symlink(source.join("pulsar"), a.directory.join("pulsar"))
                        .unwrap();
                }
            }
            assert!(prepare_runtime_launch(&source, &cache).is_err());
            assert!(prepare_cached_runtime(&cache, &a.manifest_sha256).is_err());
            assert!(a.directory.exists());
            if kind == "unknown" {
                assert_eq!(
                    std::fs::read(a.directory.join("owner-file")).unwrap(),
                    b"preserve"
                );
            }
        }
    }
    #[test]
    fn cache_symlink_shared_root_and_active_lock_refuse() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let cache = root.path().join("cache");
        let installed = prepare_runtime_launch(&source, &cache).unwrap();
        let lock = File::open(cache.join(".pulsar-runtime-cache.lock")).unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        assert!(prepare_runtime_launch(&source, &cache).is_err());
        assert!(prepare_cached_runtime(&cache, &installed.manifest_sha256).is_err());
        drop(lock);
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&cache, &link).unwrap();
        assert!(prepare_runtime_launch(&source, &link).is_err());
        assert!(prepare_cached_runtime(&link, &installed.manifest_sha256).is_err());
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_runtime_launch(&source, &cache).is_err());
        assert!(prepare_cached_runtime(&cache, &installed.manifest_sha256).is_err());
    }
    #[test]
    fn writable_nonsticky_cache_ancestor_refuses_without_installation() {
        let root = tempfile::tempdir().unwrap();
        let source = super::super::tests::fixture(root.path());
        let shared = root.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
        let cache = shared.join("cache");
        assert!(prepare_runtime_launch(&source, &cache).is_err());
        assert!(!cache.exists());
    }
}
