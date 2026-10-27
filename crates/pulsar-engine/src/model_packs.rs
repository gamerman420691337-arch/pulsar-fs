//! Offline inert installation. No native loading, subprocess, network or activation.
use anyhow::{bail, ensure, Context, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::OsStrExt,
                fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            },
        },
        path::Component,
    };

    const STAGE_MARKER: &str = ".pulsar-stage.json";
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StageMarker {
        schema_version: u16,
        manifest_sha256: String,
    }
    include!("pack_discard.rs");
    include!("pack_catalog.rs");

    pub(super) fn staging_status(state: &Path) -> Result<ModelPackStagingReport> {
        let mut observations = Vec::new();
        let path = state.join("model-packs");
        let root = match store(state, false) {
            Ok(root) => Some(root),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    && !path.try_exists()? =>
            {
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(root) = root {
            let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
            let mut names = std::fs::read_dir(anchored)?
                .take(129)
                .map(|e| e.map(|e| e.file_name()))
                .collect::<std::io::Result<Vec<_>>>()?;
            ensure!(names.len() <= 128, "pack store inventory exceeds admission");
            names.sort();
            for name in names {
                let Some(name) = name.to_str() else {
                    bail!("nonportable pack store entry")
                };
                if !name.starts_with(".pack-stage-") {
                    continue;
                }
                ensure!(
                    name.len() <= 128
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b)),
                    "invalid staging basename"
                );
                let mut observation = ModelPackStageObservation {
                    basename: name.into(),
                    state: ModelPackStageState::Unrecognized,
                    manifest_sha256: None,
                };
                let inspect = || -> Result<Option<String>> {
                    let directory = open_child(&root, name, true)?;
                    let metadata = directory.metadata()?;
                    ensure!(
                        metadata.uid() == unsafe { libc::geteuid() }
                            && metadata.mode() & 0o777 == 0o700,
                        "untrusted stage directory"
                    );
                    let marker = asset(&directory, STAGE_MARKER)?;
                    let metadata = marker.metadata()?;
                    ensure!(
                        metadata.uid() == unsafe { libc::geteuid() }
                            && metadata.mode() & 0o777 == 0o600
                            && metadata.len() <= 8192,
                        "untrusted stage marker"
                    );
                    match fs2::FileExt::try_lock_exclusive(&marker) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            return Ok(None)
                        }
                        Err(error) => return Err(error.into()),
                    }
                    let mut data = Vec::new();
                    (&marker).take(8193).read_to_end(&mut data)?;
                    ensure!(data.len() <= 8192, "stage marker grew");
                    let record: StageMarker = serde_json::from_slice(&data)?;
                    ensure!(record.schema_version == 1, "unsupported stage marker");
                    let (_, _, hash) = manifest(&directory)?;
                    ensure!(
                        record.manifest_sha256 == hash,
                        "stage manifest identity differs"
                    );
                    Ok(Some(hash))
                };
                match inspect() {
                    Ok(Some(hash)) => {
                        observation.state = ModelPackStageState::UnlockedRecognized;
                        observation.manifest_sha256 = Some(hash);
                    }
                    Ok(None) => observation.state = ModelPackStageState::Locked,
                    Err(_) => {}
                }
                observations.push(observation);
            }
        }
        let report = ModelPackStagingReport {
            schema_version: 1,
            observed_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .try_into()?,
            stages: observations,
            observation_only: true,
        };
        report.validate()?;
        Ok(report)
    }

    fn open_child(parent: &File, name: &str, directory: bool) -> Result<File> {
        let name = CString::new(name)?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 };
        // SAFETY: live directory descriptor and one validated NUL-terminated component.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful open returned one uniquely owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn source_directory(path: &Path) -> Result<File> {
        ensure!(path.is_absolute(), "pack source must be absolute");
        let mut current = File::open("/")?;
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    let name = CString::new(name.as_bytes())?;
                    // SAFETY: live parent directory and NUL-terminated single component.
                    let fd = unsafe {
                        libc::openat(
                            current.as_raw_fd(),
                            name.as_ptr(),
                            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                        )
                    };
                    if fd < 0 {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    current = unsafe { File::from_raw_fd(fd) };
                }
                _ => bail!("unsupported pack source path"),
            }
        }
        Ok(current)
    }
    fn asset(root: &File, path: &str) -> Result<File> {
        let mut dir = root.try_clone()?;
        let parts = path.split('/').collect::<Vec<_>>();
        for part in &parts[..parts.len() - 1] {
            dir = open_child(&dir, part, true)?;
        }
        let file = open_child(&dir, parts[parts.len() - 1], false)?;
        let meta = file.metadata()?;
        ensure!(
            meta.is_file() && meta.nlink() == 1,
            "pack assets must be regular, single-link files"
        );
        Ok(file)
    }
    fn read_bounded(root: &File, path: &str, limit: usize) -> Result<Vec<u8>> {
        let file = asset(root, path)?;
        ensure!(
            file.metadata()?.len() <= limit as u64,
            "pack document exceeds limit"
        );
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= limit, "pack document exceeds limit");
        Ok(bytes)
    }
    fn manifest(root: &File) -> Result<(Vec<u8>, ModelPackManifest, String)> {
        let bytes = read_bounded(root, "manifest.json", PACK_MANIFEST_LIMIT)?;
        let value: ModelPackManifest = serde_json::from_slice(&bytes)?;
        value.validate()?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        Ok((bytes, value, hash))
    }
    fn check_profile(root: &File, manifest: &ModelPackManifest) -> Result<()> {
        let profile_asset = manifest
            .assets
            .iter()
            .find(|a| a.role == PackAssetRole::Profile)
            .context("missing profile")?;
        let bytes = read_bounded(root, &profile_asset.path, 1024 * 1024)?;
        ensure!(
            bytes.len() as u64 == profile_asset.byte_len
                && format!("{:x}", Sha256::digest(&bytes)) == profile_asset.sha256,
            "parsed profile identity differs from pack"
        );
        let profile: DeclaredInferenceProfile = serde_json::from_slice(&bytes)?;
        profile.validate()?;
        for (role, identity) in [
            (PackAssetRole::Model, &profile.model),
            (PackAssetRole::Runtime, &profile.runtime),
        ] {
            let item = manifest
                .assets
                .iter()
                .find(|a| a.role == role)
                .context("missing identity asset")?;
            ensure!(
                identity.sha256 == item.sha256 && identity.byte_len == item.byte_len,
                "profile identity does not bind listed pack bytes"
            );
        }
        ensure!(
            profile.preprocessing.input.class_count as usize == manifest.labels.len(),
            "profile output count differs from ordered labels"
        );
        let model_asset = manifest
            .assets
            .iter()
            .find(|a| a.role == PackAssetRole::Model)
            .context("missing model")?;
        let mut model = asset(root, &model_asset.path)?;
        let before = model.metadata()?;
        let stamp = |m: &std::fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            )
        };
        ensure!(
            before.len() == model_asset.byte_len,
            "parsed model length differs from pack"
        );
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut count = 0u64;
        loop {
            let n = model.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count = count.checked_add(n as u64).context("model size overflow")?;
            ensure!(count <= model_asset.byte_len, "parsed model grew");
            hash.update(&buffer[..n]);
        }
        ensure!(
            count == model_asset.byte_len && format!("{:x}", hash.finalize()) == model_asset.sha256,
            "parsed model identity differs from pack"
        );
        model.seek(SeekFrom::Start(0))?;
        if matches!(
            profile.preprocessing.resize,
            InferenceResize::AnnotatedTemporalView
        ) {
            crate::pack_onnx::validate_annotated(
                &mut model,
                model_asset.byte_len,
                &profile.preprocessing.input,
            )?;
        } else {
            crate::pack_onnx::validate(
                &mut model,
                model_asset.byte_len,
                &profile.preprocessing.input,
            )?;
        }
        ensure!(
            stamp(&before) == stamp(&model.metadata()?),
            "parsed model changed during admission"
        );
        Ok(())
    }
    fn verify_assets(
        root: &File,
        manifest: &ModelPackManifest,
        copy_to: Option<&Path>,
    ) -> Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        for item in &manifest.assets {
            let mut source = asset(root, &item.path)?;
            ensure!(
                source.metadata()?.len() == item.byte_len,
                "pack asset length mismatch"
            );
            let mut output = if let Some(target) = copy_to {
                let path = target.join(&item.path);
                std::fs::create_dir_all(path.parent().context("missing asset parent")?)?;
                let mut parent = path.parent();
                while let Some(dir) = parent {
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
                    if dir == target {
                        break;
                    }
                    parent = dir.parent();
                }
                Some(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(path)?,
                )
            } else {
                None
            };
            let mut hash = Sha256::new();
            let mut count = 0u64;
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                count = count
                    .checked_add(read as u64)
                    .context("asset size overflow")?;
                ensure!(count <= item.byte_len, "pack asset grew during capture");
                hash.update(&buffer[..read]);
                if let Some(file) = output.as_mut() {
                    file.write_all(&buffer[..read])?;
                }
            }
            ensure!(
                count == item.byte_len && format!("{:x}", hash.finalize()) == item.sha256,
                "pack asset integrity mismatch"
            );
            if let Some(file) = output {
                file.sync_all()?;
            }
        }
        Ok(())
    }
    fn report(manifest: &ModelPackManifest, hash: String) -> Result<ModelPackReport> {
        let value = ModelPackReport {
            manifest_sha256: hash,
            pack_id: manifest.pack_id.clone(),
            asset_count: manifest.assets.len() as u32,
            total_bytes: manifest.assets.iter().map(|a| a.byte_len).sum(),
            declared_target_matches_host: manifest.target_os == std::env::consts::OS
                && manifest.target_arch == std::env::consts::ARCH,
            verified_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .try_into()?,
            integrity_verified: true,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
            active: false,
        };
        value.validate()?;
        Ok(value)
    }
    fn store(state: &Path, create: bool) -> Result<File> {
        if !create {
            let root = source_directory(&state.join("model-packs"))?;
            let meta = root.metadata()?;
            ensure!(
                meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o777 == 0o700,
                "pack store must be private and current-user owned"
            );
            return Ok(root);
        }
        crate::project_package_storage::unix::directory(&state.join("model-packs"))
            .map_err(|e| anyhow::anyhow!(e))
    }
    pub(super) fn status(state: &Path, sha: &str) -> Result<ModelPackReport> {
        ensure!(
            sha.len() == 64
                && sha
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "invalid pack digest"
        );
        let root = store(state, false)?;
        let installed = open_child(&root, sha, true)?;
        let (_, manifest, hash) = manifest(&installed)?;
        ensure!(hash == sha, "installed manifest identity mismatch");
        verify_assets(&installed, &manifest, None)?;
        check_profile(&installed, &manifest)?;
        let receipt: ModelPackReport =
            serde_json::from_slice(&read_bounded(&installed, "receipt.json", 8192)?)?;
        receipt.validate()?;
        ensure!(
            receipt.manifest_sha256 == hash
                && receipt.pack_id == manifest.pack_id
                && receipt.asset_count == manifest.assets.len() as u32
                && receipt.total_bytes == manifest.assets.iter().map(|a| a.byte_len).sum::<u64>(),
            "installed receipt identity mismatch"
        );
        report(&manifest, hash)
    }
    pub(super) fn execution_input(state: &Path, sha: &str) -> Result<PackExecutionInput> {
        // status checks the entire installed asset/receipt closure. The second
        // descriptor-bound capture independently binds the bytes actually used.
        status(state, sha)?;
        let root = store(state, false)?;
        let installed = open_child(&root, sha, true)?;
        let (manifest_bytes, manifest, hash) = manifest(&installed)?;
        ensure!(hash == sha, "selected manifest changed");
        ensure!(
            manifest.target_os == std::env::consts::OS
                && manifest.target_arch == std::env::consts::ARCH,
            "selected pack target differs from host"
        );
        ensure!(
            !manifest
                .assets
                .iter()
                .any(|a| a.role == PackAssetRole::NativeDependency),
            "supplier native dependency execution is unsupported"
        );
        let declared = manifest
            .assets
            .iter()
            .find(|a| a.role == PackAssetRole::Profile)
            .context("missing pack profile")?;
        let profile_bytes = read_bounded(&installed, &declared.path, 1024 * 1024)?;
        ensure!(
            profile_bytes.len() as u64 == declared.byte_len
                && format!("{:x}", Sha256::digest(&profile_bytes)) == declared.sha256,
            "selected profile changed"
        );
        let profile: DeclaredInferenceProfile = serde_json::from_slice(&profile_bytes)?;
        profile.validate()?;
        for (role, identity) in [
            (PackAssetRole::Model, &profile.model),
            (PackAssetRole::Runtime, &profile.runtime),
        ] {
            let item = manifest
                .assets
                .iter()
                .find(|a| a.role == role)
                .context("missing pack asset")?;
            ensure!(
                identity.sha256 == item.sha256
                    && identity.byte_len == item.byte_len
                    && identity.artifact_id.as_str() == item.sha256,
                "selected profile requires canonical snapshot identities"
            );
        }
        let model_asset = manifest
            .assets
            .iter()
            .find(|a| a.role == PackAssetRole::Model)
            .context("missing pack model")?;
        let model = asset(&installed, &model_asset.path)?;
        Ok(PackExecutionInput {
            model,
            profile,
            manifest_bytes,
            profile_bytes,
        })
    }
    pub(super) fn install(state: &Path, source: &Path) -> Result<ModelPackReport> {
        let source = source_directory(source)?;
        let (bytes, manifest, sha) = manifest(&source)?;
        let root = store(state, true)?;
        match open_child(&root, &sha, true) {
            Ok(_) => return status(state, &sha),
            Err(error) => {
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_none_or(|e| e.kind() != std::io::ErrorKind::NotFound)
                {
                    return Err(error);
                }
            }
        }
        let anchored = std::path::PathBuf::from(format!("/proc/self/fd/{}", root.as_raw_fd()));
        ensure!(
            std::fs::read_dir(&anchored)?.take(129).count() < 128,
            "offline pack storage capacity reached"
        );
        let total = manifest.assets.iter().map(|a| a.byte_len).sum::<u64>();
        ensure!(
            fs2::available_space(&anchored)? > total + 1024 * 1024,
            "insufficient pack storage space"
        );
        let stage = tempfile::Builder::new()
            .prefix(".pack-stage-")
            .tempdir_in(&anchored)?;
        let mut marker = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(stage.path().join(STAGE_MARKER))?;
        fs2::FileExt::lock_exclusive(&marker)?;
        marker.write_all(&serde_json::to_vec(&StageMarker {
            schema_version: 1,
            manifest_sha256: sha.clone(),
        })?)?;
        marker.sync_all()?;
        let mut manifest_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(stage.path().join("manifest.json"))?;
        manifest_file.write_all(&bytes)?;
        manifest_file.sync_all()?;
        File::open(stage.path())?.sync_all()?;
        verify_assets(&source, &manifest, Some(stage.path()))?;
        // Validate captured profile bytes, never a second read from the supplier path.
        let captured = File::open(stage.path())?;
        check_profile(&captured, &manifest)?;
        let receipt = report(&manifest, sha.clone())?;
        {
            let (name, data) = ("receipt.json", serde_json::to_vec(&receipt)?);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(stage.path().join(name))?;
            file.write_all(&data)?;
            file.sync_all()?;
        }
        // Flush nested directory entries before publishing the whole pack.
        for item in &manifest.assets {
            let mut path = stage.path().join(&item.path);
            while let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
                if parent == stage.path() {
                    break;
                }
                path = parent.to_path_buf();
            }
        }
        captured.sync_all()?;
        let from = CString::new(
            stage
                .path()
                .file_name()
                .context("missing stage basename")?
                .as_bytes(),
        )?;
        let to = CString::new(sha.as_str())?;
        // SAFETY: anchored store descriptor, engine-created basenames, no-clobber atomic publication.
        let renamed = unsafe {
            libc::renameat2(
                root.as_raw_fd(),
                from.as_ptr(),
                root.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if renamed < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                return status(state, &sha);
            }
            return Err(error.into());
        }
        root.sync_all()?;
        // TempDir now names a nonexistent staging entry; it cannot delete the installed pack.
        Ok(receipt)
    }
}
pub(crate) fn install(state: &Path, source: &Path) -> Result<ModelPackReport> {
    #[cfg(target_os = "linux")]
    {
        linux::install(state, source)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, source);
        bail!("offline pack intake requires a qualified platform storage adapter")
    }
}
pub(crate) fn status(state: &Path, sha: &str) -> Result<ModelPackReport> {
    #[cfg(target_os = "linux")]
    {
        linux::status(state, sha)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, sha);
        bail!("offline pack intake requires a qualified platform storage adapter")
    }
}

pub(crate) fn staging_status(state: &Path) -> Result<ModelPackStagingReport> {
    #[cfg(target_os = "linux")]
    {
        linux::staging_status(state)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        bail!("offline pack staging observation requires a qualified platform storage adapter")
    }
}

pub(crate) fn discard_stage(
    state: &Path,
    basename: &str,
    expected: &str,
    key: &str,
) -> Result<ModelPackDiscardReport> {
    #[cfg(target_os = "linux")]
    {
        linux::discard_stage(state, basename, expected, key)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, basename, expected, key);
        bail!("pack discard requires qualified descriptor-relative storage")
    }
}

/// Descriptor-bound input; the caller snapshots and compares the model before
/// admission. Pack runtimes are never loaded by this selection path.
pub(crate) struct PackExecutionInput {
    pub model: File,
    pub profile: DeclaredInferenceProfile,
    pub manifest_bytes: Vec<u8>,
    pub profile_bytes: Vec<u8>,
}
pub(crate) fn execution_input(state: &Path, sha: &str) -> Result<PackExecutionInput> {
    #[cfg(target_os = "linux")]
    {
        linux::execution_input(state, sha)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, sha);
        bail!("model-pack execution intake is not implemented on this platform")
    }
}

pub(crate) fn catalog_command(
    state: &Path,
    request: &Request,
    session: &SessionId,
    fingerprint: &str,
) -> Result<ModelCatalogReport> {
    #[cfg(target_os = "linux")]
    {
        linux::catalog_command(state, request, session, fingerprint)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, request, session, fingerprint);
        bail!("offline catalog intake requires a descriptor-relative platform adapter")
    }
}

pub(crate) fn catalog_pack_check(
    state: &Path,
    bootstrap: &str,
    manifest: &str,
    revision: u64,
) -> Result<ModelPackCatalogReport> {
    #[cfg(target_os = "linux")]
    {
        linux::catalog_pack_check(state, bootstrap, manifest, revision)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, bootstrap, manifest, revision);
        bail!("catalog pack association requires a descriptor-relative platform adapter")
    }
}

pub(crate) fn catalog_binding_check(
    state: &Path,
    manifest: &str,
    binding: &ModelCatalogBinding,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::catalog_binding_check(state, manifest, binding)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (state, manifest, binding);
        bail!("catalog-bound generation requires a descriptor-relative platform adapter")
    }
}
