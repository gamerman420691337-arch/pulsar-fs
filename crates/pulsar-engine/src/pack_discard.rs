// Included in the Linux pack adapter: private descriptors and manifest admission.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscardIntent {
    schema_version: u16,
    key: String,
    report: ModelPackDiscardReport,
    manifest: ModelPackManifest,
    dev: u64,
    ino: u64,
}
struct Tree {
    files: Vec<(String, u64, u64, u64)>,
    dirs: Vec<(String, u64, u64)>,
}
fn discard_owned(meta: &std::fs::Metadata, mode: u32) -> Result<()> {
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o777 == mode,
        "untrusted discard ownership/mode"
    );
    Ok(())
}
fn discard_walk(
    dir: &File,
    prefix: &str,
    allowed: &std::collections::BTreeMap<String, u64>,
    tree: &mut Tree,
) -> Result<()> {
    let meta = dir.metadata()?;
    discard_owned(&meta, 0o700)?;
    tree.dirs.push((prefix.into(), meta.dev(), meta.ino()));
    ensure!(
        tree.dirs.len() <= 7681,
        "discard directory admission exceeded"
    );
    let names = std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))?
        .take(68)
        .map(|r| r.map(|r| r.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(names.len() <= 67, "discard entry admission exceeded");
    for name in names {
        let name = name.to_str().context("nonportable discard entry")?;
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(limit) = allowed.get(&path) {
            let file = open_child(dir, name, false)?;
            let meta = file.metadata()?;
            discard_owned(&meta, 0o600)?;
            ensure!(
                meta.is_file() && meta.nlink() == 1 && meta.len() <= *limit,
                "unsafe staged discard file"
            );
            tree.files.push((path, meta.dev(), meta.ino(), meta.len()));
            ensure!(tree.files.len() <= 67, "discard file admission exceeded");
        } else {
            ensure!(
                allowed.keys().any(|a| a.starts_with(&(path.clone() + "/"))),
                "unlisted discard entry"
            );
            discard_walk(&open_child(dir, name, true)?, &path, allowed, tree)?;
        }
    }
    Ok(())
}
fn discard_allowed(manifest: &ModelPackManifest) -> std::collections::BTreeMap<String, u64> {
    let mut allowed = manifest
        .assets
        .iter()
        .map(|a| (a.path.clone(), a.byte_len))
        .collect::<std::collections::BTreeMap<_, _>>();
    allowed.insert(STAGE_MARKER.into(), 8192);
    allowed.insert("manifest.json".into(), PACK_MANIFEST_LIMIT as u64);
    allowed.insert("receipt.json".into(), 8192);
    allowed
}
fn discard_directory(root: &File, path: &str) -> Result<File> {
    let mut current = root.try_clone()?;
    if !path.is_empty() {
        for name in path.split('/') {
            current = open_child(&current, name, true)?;
        }
    }
    Ok(current)
}
fn discard_optional(root: &File, name: &str) -> Result<Option<File>> {
    match open_child(root, name, true) {
        Ok(file) => Ok(Some(file)),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
fn discard_unlink(parent: &File, name: &str, directory: bool) -> Result<()> {
    let name = CString::new(name)?;
    // SAFETY: owned live directory descriptor, one NUL-terminated component.
    let result = unsafe {
        libc::unlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            if directory { libc::AT_REMOVEDIR } else { 0 },
        )
    };
    ensure!(
        result == 0,
        "discard unlink failed: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
fn discard_marker_matches(tree: &Tree, marker: &File) -> Result<()> {
    let metadata = marker.metadata()?;
    ensure!(
        tree.files
            .iter()
            .any(|f| f.0 == STAGE_MARKER && (f.1, f.2) == (metadata.dev(), metadata.ino())),
        "locked discard marker was replaced"
    );
    Ok(())
}
fn discard_not_published(root: &File, stage: &File) -> Result<()> {
    let identity = stage.metadata()?;
    let entries = std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))?
        .take(129)
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        entries.len() <= 128,
        "discard store inventory exceeds admission"
    );
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            bail!("nonportable store entry")
        };
        if name.len() == 64
            && name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            let published = open_child(root, name, true)?.metadata()?;
            ensure!(
                (published.dev(), published.ino()) != (identity.dev(), identity.ino()),
                "discard directory is a published pack"
            );
        }
    }
    Ok(())
}
pub(super) fn discard_stage(
    state: &Path,
    basename: &str,
    expected: &str,
    key: &str,
) -> Result<ModelPackDiscardReport> {
    validate_stage_basename(basename)?;
    for digest in [expected, key] {
        ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid discard digest"
        );
    }
    let root = store(state, false)?;
    let records =
        crate::project_package_storage::unix::directory(&state.join("model-pack-discards"))
            .map_err(|e| anyhow::anyhow!(e))?;
    let record_path = std::path::PathBuf::from(format!("/proc/self/fd/{}", records.as_raw_fd()));
    let intent_name = format!("{key}.json");
    let done_name = format!("{key}.done.json");
    let quarantine = format!(".pack-discard-{key}");
    let mut marker_guard = None;
    let intent_file = match asset(&records, &intent_name) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            let stage = open_child(&root, basename, true)?;
            discard_owned(&stage.metadata()?, 0o700)?;
            discard_not_published(&root, &stage)?;
            let marker = asset(&stage, STAGE_MARKER)?;
            discard_owned(&marker.metadata()?, 0o600)?;
            fs2::FileExt::try_lock_exclusive(&marker)?;
            let mut data = Vec::new();
            (&marker).take(8193).read_to_end(&mut data)?;
            ensure!(data.len() <= 8192, "discard marker exceeds admission");
            let saved: StageMarker = serde_json::from_slice(&data)?;
            let (_, manifest, hash) = manifest(&stage)?;
            ensure!(
                saved.schema_version == 1 && saved.manifest_sha256 == hash && hash == expected,
                "staged discard identity mismatch"
            );
            let mut tree = Tree {
                files: Vec::new(),
                dirs: Vec::new(),
            };
            discard_walk(&stage, "", &discard_allowed(&manifest), &mut tree)?;
            discard_marker_matches(&tree, &marker)?;
            let metadata = stage.metadata()?;
            let report = ModelPackDiscardReport {
                schema_version: 1,
                basename: basename.into(),
                manifest_sha256: hash,
                outcome: ModelPackDiscardOutcome::Removed,
                planned_file_count: tree.files.len().try_into()?,
                planned_bytes: tree.files.iter().map(|f| f.3).sum(),
                native_execution_or_activation: false,
            };
            report.validate()?;
            let intent = DiscardIntent {
                schema_version: 1,
                key: key.into(),
                report,
                manifest,
                dev: metadata.dev(),
                ino: metadata.ino(),
            };
            ensure!(
                std::fs::read_dir(&record_path)?.take(257).count() < 256,
                "discard receipt capacity reached"
            );
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(record_path.join(&intent_name))?;
            fs2::FileExt::lock_exclusive(&file)?;
            file.write_all(&serde_json::to_vec(&intent)?)?;
            file.sync_all()?;
            records.sync_all()?;
            marker_guard = Some(marker);
            file.seek(SeekFrom::Start(0))?;
            file
        }
        Err(error) => return Err(error),
    };
    discard_owned(&intent_file.metadata()?, 0o600)?;
    fs2::FileExt::try_lock_exclusive(&intent_file)?;
    let mut data = Vec::new();
    (&intent_file)
        .take((PACK_MANIFEST_LIMIT + 8193) as u64)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= PACK_MANIFEST_LIMIT + 8192,
        "discard intent exceeds admission"
    );
    let intent: DiscardIntent = serde_json::from_slice(&data)?;
    intent.report.validate()?;
    intent.manifest.validate()?;
    ensure!(
        intent.report.outcome == ModelPackDiscardOutcome::Removed
            && intent.schema_version == 1
            && intent.key == key
            && intent.report.basename == basename
            && intent.report.manifest_sha256 == expected,
        "discard intent differs"
    );
    match read_bounded(&records, &done_name, 8192) {
        Ok(data) => {
            let report: ModelPackDiscardReport = serde_json::from_slice(&data)?;
            report.validate()?;
            ensure!(
                serde_json::to_vec(&report)? == serde_json::to_vec(&intent.report)?,
                "discard completion differs"
            );
            return Ok(report);
        }
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
        Err(e) => return Err(e),
    }
    let original = discard_optional(&root, basename)?;
    let quarantined = discard_optional(&root, &quarantine)?;
    ensure!(
        original.is_none() || quarantined.is_none(),
        "discard source and quarantine both exist"
    );
    let Some(stage) = original.as_ref().or(quarantined.as_ref()) else {
        let mut report = intent.report;
        report.outcome = ModelPackDiscardOutcome::AbsentAfterIntent;
        return Ok(report);
    };
    let metadata = stage.metadata()?;
    discard_owned(&metadata, 0o700)?;
    discard_not_published(&root, stage)?;
    ensure!(
        (metadata.dev(), metadata.ino()) == (intent.dev, intent.ino),
        "discard directory was replaced"
    );
    if marker_guard.is_none() {
        match asset(stage, STAGE_MARKER) {
            Ok(marker) => {
                discard_owned(&marker.metadata()?, 0o600)?;
                fs2::FileExt::try_lock_exclusive(&marker)?;
                marker_guard = Some(marker);
            }
            Err(e)
                if original.is_none()
                    && e.downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    let mut tree = Tree {
        files: Vec::new(),
        dirs: Vec::new(),
    };
    discard_walk(stage, "", &discard_allowed(&intent.manifest), &mut tree)?;
    if let Some(marker) = &marker_guard {
        discard_marker_matches(&tree, marker)?;
    }
    if original.is_some() {
        let (_, _, hash) = manifest(stage)?;
        ensure!(hash == expected, "discard manifest changed");
        let from = CString::new(basename)?;
        let to = CString::new(quarantine.as_str())?;
        // SAFETY: anchored private store and validated one-component names; no clobber.
        let result = unsafe {
            libc::renameat2(
                root.as_raw_fd(),
                from.as_ptr(),
                root.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        ensure!(result == 0, "discard quarantine rename failed");
        root.sync_all()?;
        let found = open_child(&root, &quarantine, true)?.metadata()?;
        ensure!(
            (found.dev(), found.ino()) == (intent.dev, intent.ino),
            "quarantined directory differs"
        );
    }
    tree.files.sort_by_key(|f| f.0 == STAGE_MARKER);
    for (path, dev, ino, len) in &tree.files {
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        let directory = discard_directory(stage, parent)?;
        let found = open_child(&directory, name, false)?.metadata()?;
        ensure!(
            (found.dev(), found.ino(), found.len()) == (*dev, *ino, *len),
            "staged file changed during discard"
        );
        discard_unlink(&directory, name, false)?;
    }
    tree.dirs
        .sort_by_key(|d| std::cmp::Reverse(d.0.split('/').count()));
    for (path, dev, ino) in &tree.dirs {
        if path.is_empty() {
            continue;
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        let directory = discard_directory(stage, parent)?;
        let found = open_child(&directory, name, true)?.metadata()?;
        ensure!(
            (found.dev(), found.ino()) == (*dev, *ino),
            "staged directory changed during discard"
        );
        discard_unlink(&directory, name, true)?;
    }
    discard_unlink(&root, &quarantine, true)?;
    root.sync_all()?;
    let mut completion = tempfile::NamedTempFile::new_in(&record_path)?;
    completion.write_all(&serde_json::to_vec(&intent.report)?)?;
    completion.as_file().sync_all()?;
    let from = CString::new(
        completion
            .path()
            .file_name()
            .context("missing completion name")?
            .as_bytes(),
    )?;
    let to = CString::new(done_name)?;
    // SAFETY: anchored receipt directory and single validated basenames.
    let result = unsafe {
        libc::renameat2(
            records.as_raw_fd(),
            from.as_ptr(),
            records.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    ensure!(result == 0, "discard completion publication failed");
    records.sync_all()?;
    drop(marker_guard);
    Ok(intent.report)
}

#[cfg(test)]
mod discard_tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, String, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let root = store(state, true).unwrap();
        let name = ".pack-stage-authored".to_owned();
        let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
        std::fs::create_dir(anchored.join(&name)).unwrap();
        std::fs::set_permissions(anchored.join(&name), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let assets = [
            ("model.bin", PackAssetRole::Model),
            ("runtime.bin", PackAssetRole::Runtime),
            ("profile.json", PackAssetRole::Profile),
            ("LICENSE", PackAssetRole::License),
        ]
        .into_iter()
        .map(|(path, role)| PackAsset {
            path: path.into(),
            role,
            byte_len: 8,
            sha256: "a".repeat(64),
        })
        .collect();
        let manifest = ModelPackManifest {
            schema_version: 1,
            pack_id: ArtifactId::new("authored").unwrap(),
            target_os: "linux".into(),
            target_arch: "x86_64".into(),
            ort_api_version: 24,
            labels: vec!["authored".into()],
            assets,
            license_path: "LICENSE".into(),
            source_description: "authored incomplete stage".into(),
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        for (filename, data) in [
            ("manifest.json", bytes),
            (
                STAGE_MARKER,
                serde_json::to_vec(&StageMarker {
                    schema_version: 1,
                    manifest_sha256: hash.clone(),
                })
                .unwrap(),
            ),
            ("model.bin", b"part".to_vec()),
        ] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(anchored.join(&name).join(filename))
                .unwrap();
            file.write_all(&data).unwrap();
        }
        (temp, name, hash, "b".repeat(64))
    }
    #[test]
    fn removed_receipt_and_unacknowledged_absence_are_distinct() {
        let (temp, name, hash, key) = fixture();
        let removed = discard_stage(temp.path(), &name, &hash, &key).unwrap();
        assert_eq!(removed.outcome, ModelPackDiscardOutcome::Removed);
        let repeated = discard_stage(temp.path(), &name, &hash, &key).unwrap();
        assert_eq!(
            serde_json::to_vec(&removed).unwrap(),
            serde_json::to_vec(&repeated).unwrap()
        );
        std::fs::remove_file(
            temp.path()
                .join("model-pack-discards")
                .join(format!("{key}.done.json")),
        )
        .unwrap();
        let absent = discard_stage(temp.path(), &name, &hash, &key).unwrap();
        assert_eq!(absent.outcome, ModelPackDiscardOutcome::AbsentAfterIntent);
        assert!(!absent.native_execution_or_activation);
    }
    #[test]
    fn recorded_quarantine_resumes_partial_deletion_and_fences_changed_identity() {
        let (temp, name, hash, key) = fixture();
        let root = temp.path().join("model-packs");
        let quarantine = root.join(format!(".pack-discard-{key}"));
        std::fs::create_dir(&quarantine).unwrap();
        assert!(discard_stage(temp.path(), &name, &hash, &key).is_err());
        assert!(root.join(&name).exists());
        std::fs::remove_dir(&quarantine).unwrap();
        std::fs::rename(root.join(&name), &quarantine).unwrap();
        std::fs::remove_file(quarantine.join(STAGE_MARKER)).unwrap();
        std::fs::remove_file(quarantine.join("manifest.json")).unwrap();
        let removed = discard_stage(temp.path(), &name, &hash, &key).unwrap();
        assert_eq!(removed.outcome, ModelPackDiscardOutcome::Removed);
        assert!(!quarantine.exists());
        assert!(discard_stage(temp.path(), &name, &"c".repeat(64), &key).is_err());
    }
}
