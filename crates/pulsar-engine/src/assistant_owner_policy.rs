//! Explicit private host configuration; never parsed from a client request.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    schema_version: u16,
    approve_inference: bool,
    worker_sha256: String,
    worker_bytes: u64,
    runtime_path: PathBuf,
    runtime: ArtifactIdentity,
    model_path: PathBuf,
    model: ArtifactIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projector: Option<ArtifactReference>,
    cpu_threads: u16,
    wall_time_ms: u64,
    worker_memory_bytes: u64,
}
impl Policy {
    fn validate(&self, config: &crate::EngineConfig) -> Result<()> {
        ensure!(
            matches!(self.schema_version, 1..=4) && self.approve_inference,
            "assistant owner policy requires a supported schema and explicit inference approval"
        );
        ensure!(
            self.projector.is_some() == matches!(self.schema_version, 3 | 4),
            "vision policy schema requires exactly one projector"
        );
        ensure!(
            self.cpu_threads <= config.max_cpu_threads,
            "assistant CPU declaration exceeds owner capacity"
        );
        if let Some(projector) = &self.projector {
            projector.validate()?;
        }
        let worker = ArtifactIdentity {
            artifact_id: ArtifactId::new("owner-policy")?,
            sha256: self.worker_sha256.clone(),
            byte_len: self.worker_bytes,
        };
        let profile = AssistantProfile {
            vision_projector: self.projector.as_ref().map(|p| p.identity.clone()),
            profile_id: AttemptId::new("policy-validation")?,
            worker,
            runtime: self.runtime.clone(),
            model: self.model.clone(),
            cpu_threads: self.cpu_threads,
            wall_time_ms: self.wall_time_ms,
            worker_memory_bytes: self.worker_memory_bytes,
        };
        profile.validate()?;
        ensure!(
            self.runtime_path.is_absolute() && self.model_path.is_absolute(),
            "assistant asset paths must be absolute"
        );
        ensure!(
            std::time::Duration::from_millis(self.wall_time_ms) <= config.worker_timeout,
            "assistant owner deadline exceeds Engine limit"
        );
        let images = self
            .worker_bytes
            .checked_add(self.runtime.byte_len)
            .and_then(|n| n.checked_add(self.model.byte_len))
            .and_then(|n| {
                n.checked_add(
                    self.projector
                        .as_ref()
                        .map_or(0, |p| p.identity.byte_len + 512 * 512 * 3),
                )
            })
            .context("assistant image accounting overflow")?;
        ensure!(
            images
                .checked_add(256 * 1024 * 1024)
                .is_some_and(|n| n <= self.worker_memory_bytes),
            "assistant child budget cannot hold mounts/native allowance"
        );
        ensure!(
            self.worker_memory_bytes
                .checked_add(images)
                .and_then(|n| n.checked_add(2 * 1024 * 1024))
                .is_some_and(|n| n <= config.max_ram_bytes),
            "assistant aggregate images/child budget exceeds Engine admission"
        );
        let mut assets = vec![
            (&self.runtime_path, &self.runtime, true),
            (&self.model_path, &self.model, false),
        ];
        if let Some(projector) = &self.projector {
            assets.push((&projector.path, &projector.identity, false));
        }
        for (path, identity, executable) in assets {
            let file = crate::artifacts::open_regular(path)?;
            let metadata = file.metadata()?;
            ensure!(
                metadata.len() == identity.byte_len
                    && (!executable || metadata.mode() & 0o111 != 0),
                "assistant declared asset metadata differs"
            );
        }
        Ok(())
    }
    pub(crate) fn match_worker(&self, worker: &ArtifactIdentity) -> Result<()> {
        ensure!(
            self.worker_sha256 == worker.sha256 && self.worker_bytes == worker.byte_len,
            "assistant owner policy worker differs from captured companion"
        );
        Ok(())
    }
    pub(crate) fn register(self, engine: &crate::Engine) -> Result<()> {
        let assets = crate::AssistantLaunchAssets {
            vision: None,
            runtime_path: self.runtime_path,
            model_path: self.model_path,
            worker_memory_bytes: self.worker_memory_bytes,
        };
        if let Some(projector) = self.projector {
            engine.register_vision_assistant_profile(
                assets,
                self.runtime,
                self.model,
                projector,
                self.cpu_threads,
                self.wall_time_ms,
            )?;
        } else {
            engine.register_assistant_profile(
                assets,
                self.runtime,
                self.model,
                self.cpu_threads,
                self.wall_time_ms,
            )?;
        }
        Ok(())
    }
}
fn read_private_policy(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        path.is_absolute(),
        "assistant owner policy path must be absolute"
    );
    let mut file = crate::artifacts::open_regular(path)?;
    let before = file.metadata()?;
    ensure!(
        before.uid() == unsafe { libc::geteuid() }
            && before.mode() & 0o077 == 0
            && before.nlink() == 1
            && before.len() > 0
            && before.len() <= 16384,
        "assistant policy requires private current-user single-link regular bytes <=16 KiB"
    );
    let mut bytes = Vec::new();
    file.by_ref().take(16385).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        bytes.len() <= 16384
            && before.len() == after.len()
            && before.mode() == after.mode()
            && before.nlink() == after.nlink()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "assistant policy changed during capture"
    );
    Ok(bytes)
}
pub(crate) fn load(path: &Path, config: &crate::EngineConfig) -> Result<Policy> {
    let bytes = read_private_policy(path)?;
    let policy: Policy = serde_json::from_slice(&bytes)?;
    let mut policy = policy;
    if matches!(policy.schema_version, 2 | 3) {
        let parent = path
            .parent()
            .context("assistant policy has no installation root")?;
        let root = parent.canonicalize()?;
        policy.runtime_path = resolve_asset(&root, &policy.runtime_path)?;
        policy.model_path = resolve_asset(&root, &policy.model_path)?;
        if let Some(projector) = &mut policy.projector {
            projector.path = resolve_asset(&root, &projector.path)?;
        }
    }
    policy.validate(config)?;
    Ok(policy)
}
// Portable schema 2/3 asset paths are relative to this explicitly selected policy, never
// the process cwd or a search/download location. Execution still captures
// and hashes the resolved bytes before executing inference.
fn resolve_asset(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "portable assistant asset requires a nonempty normal relative path"
    );
    let joined = root.join(relative);
    let resolved = joined.canonicalize()?;
    ensure!(
        resolved == joined,
        "portable assistant asset contains a symlink or escapes installation root"
    );
    Ok(resolved)
}
/// Trusted local host setup only. This is not a client RPC or project approval.
pub struct AssistantPolicySetup {
    pub output: PathBuf,
    pub runtime: PathBuf,
    pub model: PathBuf,
    pub projector: Option<PathBuf>,
    pub cpu_threads: u16,
    pub wall_time_ms: u64,
    pub worker_memory_bytes: u64,
    pub absolute_assets: bool,
    pub approve_host_inference: bool,
}

/// Measures local files and creates a new private policy without opening Engine state.
/// An uncertain write/sync leaves only the newly created private file for inspection.
pub fn create_assistant_policy(
    config: &crate::EngineConfig,
    options: AssistantPolicySetup,
) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    ensure!(
        options.approve_host_inference,
        "explicit assistant host inference approval is required"
    );
    ensure!(
        options.output.is_absolute()
            && options.runtime.is_absolute()
            && options.model.is_absolute()
            && options.projector.as_ref().is_none_or(|p| p.is_absolute()),
        "assistant setup paths must be absolute"
    );
    let parent = options
        .output
        .parent()
        .context("assistant policy parent is missing")?;
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)?;
    let metadata = directory.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
        "assistant policy parent must be owned and not writable by other users"
    );
    let root = parent.canonicalize()?;
    let asset_path = |path: &Path| -> Result<PathBuf> {
        if options.absolute_assets {
            return Ok(path.to_path_buf());
        }
        let relative = path
            .strip_prefix(&root)
            .context("portable assets must be inside policy folder")?;
        resolve_asset(&root, relative)?;
        Ok(relative.to_path_buf())
    };
    let runtime_path = asset_path(&options.runtime)?;
    let model_path = asset_path(&options.model)?;
    let worker = measured_identity(&config.worker_executable, config.max_ram_bytes, true)?;
    let runtime = measured_identity(&options.runtime, config.max_ram_bytes, true)?;
    let model = measured_identity(&options.model, config.max_ram_bytes, false)?;
    let projector = options
        .projector
        .as_ref()
        .map(|path| -> Result<ArtifactReference> {
            Ok(ArtifactReference {
                path: asset_path(path)?,
                identity: measured_identity(path, config.max_ram_bytes.min(2u64 << 30), false)?,
            })
        })
        .transpose()?;
    let policy = Policy {
        schema_version: match (options.projector.is_some(), options.absolute_assets) {
            (false, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
            (true, true) => 4,
        },
        projector,
        approve_inference: true,
        worker_sha256: worker.sha256,
        worker_bytes: worker.byte_len,
        runtime_path,
        runtime,
        model_path,
        model,
        cpu_threads: options.cpu_threads,
        wall_time_ms: options.wall_time_ms,
        worker_memory_bytes: options.worker_memory_bytes,
    };
    let mut checked = policy.clone();
    checked.runtime_path = options.runtime;
    checked.model_path = options.model;
    if let Some(projector) = &mut checked.projector {
        projector.path = options.projector.clone().expect("checked vision setup");
    }
    checked.validate(config)?;
    write_private_policy(&directory, &options.output, &policy)
}
fn write_private_policy(directory: &std::fs::File, output: &Path, policy: &Policy) -> Result<()> {
    use std::io::Write;
    use std::os::unix::{
        ffi::OsStrExt,
        io::{AsRawFd, FromRawFd},
    };
    let bytes = serde_json::to_vec_pretty(&policy)?;
    ensure!(
        bytes.len() <= 16384,
        "assistant policy exceeds capture bound"
    );
    let name = std::ffi::CString::new(
        output
            .file_name()
            .context("policy filename missing")?
            .as_bytes(),
    )?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    directory.sync_all()?;
    Ok(())
}
const STARTUP_POLICY: &str = "assistant-startup-policy.json";
pub(crate) fn startup_policy_path(
    state: &Path,
    explicit: Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    let path = state.join(STARTUP_POLICY);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
/// Bounded saved-choice inspection; does not register a profile or run inference.
pub struct AssistantSelectionInspection {
    pub policy_sha256: String,
    pub validation_error: Option<String>,
}
pub fn inspect_assistant_policy_selection(
    config: &crate::EngineConfig,
) -> Result<Option<AssistantSelectionInspection>> {
    use sha2::{Digest, Sha256};
    let Some(path) = startup_policy_path(&config.state_dir, None)? else {
        return Ok(None);
    };
    let bytes = read_private_policy(&path)?;
    let checked = load(&path, config).and_then(|policy| {
        policy.match_worker(&measured_identity(
            &config.worker_executable,
            config.max_ram_bytes,
            true,
        )?)
    });
    Ok(Some(AssistantSelectionInspection {
        policy_sha256: format!("{:x}", Sha256::digest(bytes)),
        validation_error: checked
            .err()
            .map(|error| error.to_string().chars().take(4096).collect()),
    }))
}
/// Explicit host configuration for subsequent Engine startup; no project effect.
pub fn select_assistant_policy(
    config: &crate::EngineConfig,
    source: &Path,
    approve: bool,
) -> Result<String> {
    ensure!(
        approve,
        "explicit assistant host inference approval is required"
    );
    ensure!(
        config.state_dir.is_absolute(),
        "assistant startup state directory must be absolute"
    );
    let mut policy = load(source, config)?;
    policy.match_worker(&measured_identity(
        &config.worker_executable,
        config.max_ram_bytes,
        true,
    )?)?;
    crate::artifacts::private_directory(&config.state_dir)?;
    let root = config.state_dir.canonicalize()?;
    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
    };
    if let (Some(runtime), Some(model)) =
        (relative(&policy.runtime_path), relative(&policy.model_path))
    {
        let projector = policy.projector.as_ref().map(|p| relative(&p.path));
        if projector.as_ref().is_none_or(Option::is_some) {
            policy.runtime_path = runtime;
            policy.model_path = model;
            if let Some(projector_path) = projector.flatten() {
                policy.projector.as_mut().expect("selected projector").path = projector_path;
            }
            policy.schema_version = if policy.projector.is_some() { 3 } else { 2 };
        } else {
            policy.schema_version = 4;
        }
    } else {
        policy.schema_version = if policy.projector.is_some() { 4 } else { 1 };
    }
    let path = config.state_dir.join(STARTUP_POLICY);
    use std::os::unix::fs::OpenOptionsExt;
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&config.state_dir)?;
    let metadata = directory.metadata()?;
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
        "saved assistant startup directory must be private and owned"
    );
    write_private_policy(&directory, &path, &policy)?;
    use sha2::{Digest, Sha256};
    Ok(format!("{:x}", Sha256::digest(read_private_policy(&path)?)))
}
/// Removes only the private saved selection with the explicitly supplied current digest.
pub fn disable_assistant_policy(
    config: &crate::EngineConfig,
    expected: &str,
    approve: bool,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    ensure!(approve, "explicit assistant disable approval is required");
    ensure!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid saved assistant policy digest"
    );
    ensure!(
        config.state_dir.is_absolute(),
        "assistant startup state directory must be absolute"
    );
    ensure!(
        config.state_dir.exists(),
        "saved assistant startup directory is missing"
    );
    crate::artifacts::private_directory(&config.state_dir)?;
    let path = config.state_dir.join(STARTUP_POLICY);
    let before = std::fs::symlink_metadata(&path)?;
    let bytes = read_private_policy(&path)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == expected,
        "saved assistant policy digest differs; retained"
    );
    let after = std::fs::symlink_metadata(&path)?;
    ensure!(
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "saved assistant policy identity changed; retained"
    );
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&config.state_dir)?;
    let mut selected = std::mem::MaybeUninit::<libc::stat>::uninit();
    let name = std::ffi::CString::new(STARTUP_POLICY)?;
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            selected.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let selected = unsafe { selected.assume_init() };
    ensure!(
        selected.st_dev == after.dev() && selected.st_ino == after.ino(),
        "saved assistant selection changed before removal; retained"
    );
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    directory.sync_all()?;
    Ok(())
}

fn measured_identity(path: &Path, limit: u64, executable: bool) -> Result<ArtifactIdentity> {
    use sha2::{Digest, Sha256};
    let mut file = crate::artifacts::open_regular(path)?;
    let before = file.metadata()?;
    ensure!(
        before.len() > 0 && before.len() <= limit && (!executable || before.mode() & 0o111 != 0),
        "assistant setup asset size or executable metadata is invalid"
    );
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .context("assistant asset size overflow")?;
        ensure!(total <= limit, "assistant setup asset exceeds admission");
        hash.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    ensure!(
        total == before.len()
            && before.len() == after.len()
            && before.mode() == after.mode()
            && before.nlink() == after.nlink()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "assistant asset changed during measurement"
    );
    let sha256 = format!("{:x}", hash.finalize());
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len: total,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn selected_fixture(root: &Path, inside: bool) -> (PathBuf, crate::EngineConfig) {
        let (source, mut policy, mut config) = portable_fixture(root);
        let worker =
            measured_identity(&config.worker_executable, config.max_ram_bytes, true).unwrap();
        policy.worker_sha256 = worker.sha256;
        policy.worker_bytes = worker.byte_len;
        write_policy(&source, &policy);
        if inside {
            config.state_dir = root.to_path_buf();
        }
        (source, config)
    }
    #[test]
    fn startup_choice_is_explicit_private_no_overwrite_and_digest_bound_disable() {
        let root = tempfile::tempdir().unwrap();
        let (source, config) = selected_fixture(root.path(), false);
        assert!(select_assistant_policy(&config, &source, false).is_err());
        assert!(!config.state_dir.exists());
        let digest = select_assistant_policy(&config, &source, true).unwrap();
        let saved = startup_policy_path(&config.state_dir, None)
            .unwrap()
            .unwrap();
        let bytes = std::fs::read(&saved).unwrap();
        assert_eq!(std::fs::metadata(&saved).unwrap().mode() & 0o777, 0o600);
        assert_eq!(load(&saved, &config).unwrap().schema_version, 1);
        assert!(select_assistant_policy(&config, &source, true).is_err());
        assert!(disable_assistant_policy(&config, &digest, false).is_err());
        assert!(disable_assistant_policy(&config, &"0".repeat(64), true).is_err());
        assert_eq!(std::fs::read(&saved).unwrap(), bytes);
        disable_assistant_policy(&config, &digest, true).unwrap();
        assert!(startup_policy_path(&config.state_dir, None)
            .unwrap()
            .is_none());
        assert!(source.exists());
    }
    #[test]
    fn startup_choice_inside_data_root_survives_relocation_and_explicit_override_wins() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("original");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (source, mut config) = selected_fixture(&root, true);
        select_assistant_policy(&config, &source, true).unwrap();
        assert_eq!(
            load(&root.join(STARTUP_POLICY), &config)
                .unwrap()
                .schema_version,
            2
        );
        let moved = outer.path().join("moved data with spaces");
        std::fs::rename(&root, &moved).unwrap();
        config.state_dir = moved.clone();
        let path = startup_policy_path(&moved, None).unwrap().unwrap();
        assert_eq!(
            load(&path, &config).unwrap().runtime_path,
            moved.join("assets/runtime")
        );
        let explicit = PathBuf::from("/explicit/missing-policy");
        assert_eq!(
            startup_policy_path(&moved, Some(explicit.clone())).unwrap(),
            Some(explicit)
        );
    }
    #[test]
    fn saved_corrupt_or_unsafe_choice_never_silently_disables_assistant() {
        let root = tempfile::tempdir().unwrap();
        let (source, config) = selected_fixture(root.path(), false);
        select_assistant_policy(&config, &source, true).unwrap();
        let path = config.state_dir.join(STARTUP_POLICY);
        std::fs::write(&path, b"truncated").unwrap();
        assert!(startup_policy_path(&config.state_dir, None)
            .unwrap()
            .is_some());
        assert!(load(&path, &config).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&path, &config).is_err());
        assert!(disable_assistant_policy(&config, &"0".repeat(64), true).is_err());
        assert!(path.exists());
    }
    #[test]
    fn startup_selection_refuses_changed_worker_and_missing_disable_has_no_state_effect() {
        let root = tempfile::tempdir().unwrap();
        let (source, mut config) = selected_fixture(root.path(), false);
        config.worker_executable = root.path().join("different-worker");
        std::fs::write(&config.worker_executable, b"different").unwrap();
        std::fs::set_permissions(
            &config.worker_executable,
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(select_assistant_policy(&config, &source, true).is_err());
        assert!(!config.state_dir.exists());
        assert!(disable_assistant_policy(&config, &"0".repeat(64), true).is_err());
        assert!(!config.state_dir.exists());
    }
    #[test]
    fn disable_never_removes_linked_or_symlinked_selection() {
        let root = tempfile::tempdir().unwrap();
        let (source, config) = selected_fixture(root.path(), false);
        let digest = select_assistant_policy(&config, &source, true).unwrap();
        let saved = config.state_dir.join(STARTUP_POLICY);
        let linked = config.state_dir.join("other-file");
        std::fs::hard_link(&saved, &linked).unwrap();
        assert!(disable_assistant_policy(&config, &digest, true).is_err());
        std::fs::remove_file(&saved).unwrap();
        std::os::unix::fs::symlink(&linked, &saved).unwrap();
        assert!(disable_assistant_policy(&config, &digest, true).is_err());
        assert!(linked.exists() && saved.is_symlink());
    }
    #[test]
    fn unsafe_policy_refuses_before_engine_state_creation() {
        let root = tempfile::tempdir().unwrap();
        let policy = root.path().join("policy.json");
        std::fs::write(
            &policy,
            b"{\"schema_version\":1,\"approve_inference\":false}",
        )
        .unwrap();
        std::fs::set_permissions(&policy, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut config =
            crate::EngineConfig::new(root.path().join("state"), PathBuf::from("/usr/bin/true"));
        config.assistant_owner_policy = Some(policy.clone());
        assert!(crate::Engine::open(config.clone()).is_err());
        assert!(!config.state_dir.exists());
        std::fs::set_permissions(&policy, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(&policy, &config).is_err());
        std::fs::set_permissions(&policy, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&policy, &alias).unwrap();
        assert!(load(&alias, &config).is_err());
        let linked = root.path().join("linked");
        std::fs::hard_link(&policy, &linked).unwrap();
        assert!(load(&policy, &config).is_err());
    }
    #[test]
    fn complete_policy_checks_explicit_approval_asset_metadata_and_aggregate_budget() {
        let root = tempfile::tempdir().unwrap();
        let model = root.path().join("model");
        std::fs::write(&model, b"x").unwrap();
        let bytes = std::fs::metadata("/usr/bin/true").unwrap().len();
        let artifact = |len| ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: len,
        };
        let config =
            crate::EngineConfig::new(root.path().join("state"), PathBuf::from("/usr/bin/true"));
        let mut policy = Policy {
            projector: None,
            schema_version: 1,
            approve_inference: true,
            worker_sha256: "a".repeat(64),
            worker_bytes: bytes,
            runtime_path: PathBuf::from("/usr/bin/true"),
            runtime: artifact(bytes),
            model_path: model,
            model: artifact(1),
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 * 1024 * 1024,
        };
        assert!(policy.validate(&config).is_ok());
        policy.approve_inference = false;
        assert!(policy.validate(&config).is_err());
        policy.approve_inference = true;
        policy.model.byte_len = 2;
        assert!(policy.validate(&config).is_err());
        policy.model.byte_len = 1;
        let mut small = config;
        small.max_ram_bytes = 512 * 1024 * 1024;
        assert!(policy.validate(&small).is_err());
    }
    fn portable_fixture(root: &Path) -> (PathBuf, Policy, crate::EngineConfig) {
        let assets = root.join("assets");
        std::fs::create_dir(&assets).unwrap();
        std::fs::write(assets.join("runtime"), b"x").unwrap();
        std::fs::set_permissions(
            assets.join("runtime"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(assets.join("model"), b"y").unwrap();
        let identity = || ArtifactIdentity {
            artifact_id: ArtifactId::new("portable-fixture").unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let policy = Policy {
            projector: None,
            schema_version: 2,
            approve_inference: true,
            worker_sha256: "a".repeat(64),
            worker_bytes: 1,
            runtime_path: "assets/runtime".into(),
            runtime: identity(),
            model_path: "assets/model".into(),
            model: identity(),
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 * 1024 * 1024,
        };
        let path = root.join("assistant.json");
        write_policy(&path, &policy);
        let worker = root.join("fixture-worker");
        std::fs::write(&worker, b"fixture worker; not executed").unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = crate::EngineConfig::new(root.join("state"), worker);
        (path, policy, config)
    }
    fn write_policy(path: &Path, policy: &Policy) {
        std::fs::write(path, serde_json::to_vec(policy).unwrap()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[test]
    fn portable_policy_resolves_only_selected_root_and_survives_relocation() {
        let outer = tempfile::tempdir().unwrap();
        let original = outer.path().join("installation");
        std::fs::create_dir(&original).unwrap();
        let (path, declaration, config) = portable_fixture(&original);
        let loaded = load(&path, &config).unwrap();
        assert_eq!(loaded.runtime_path, original.join("assets/runtime"));
        let moved = outer.path().join("moved installation");
        std::fs::rename(&original, &moved).unwrap();
        let loaded = load(&moved.join("assistant.json"), &config).unwrap();
        assert_eq!(loaded.runtime_path, moved.join("assets/runtime"));
        assert_eq!(loaded.model_path, moved.join("assets/model"));
        assert_eq!(loaded.runtime, declaration.runtime);
        assert_eq!(loaded.model, declaration.model);
        assert_eq!(loaded.worker_sha256, declaration.worker_sha256);
        assert!(!config.state_dir.exists());
    }
    #[test]
    fn portable_policy_refuses_traversal_absolute_missing_symlink_and_unapproved_assets() {
        let root = tempfile::tempdir().unwrap();
        let (path, mut policy, config) = portable_fixture(root.path());
        for bad in ["", "../model", "/usr/bin/true", "assets/missing"] {
            policy.model_path = bad.into();
            write_policy(&path, &policy);
            assert!(load(&path, &config).is_err(), "{bad}");
        }
        policy.model_path = "assets/model".into();
        std::os::unix::fs::symlink("assets", root.path().join("alias")).unwrap();
        policy.runtime_path = "alias/runtime".into();
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        std::os::unix::fs::symlink("model", root.path().join("assets/link")).unwrap();
        policy.runtime_path = "assets/runtime".into();
        policy.model_path = "assets/link".into();
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        policy.model_path = "assets/model".into();
        policy.approve_inference = false;
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        policy.approve_inference = true;
        policy.schema_version = 1;
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        policy.schema_version = 3;
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        assert!(!config.state_dir.exists());
    }
    #[test]
    fn host_setup_measures_private_portable_policy_without_state_and_never_overwrites() {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let (path, _, config) = portable_fixture(root.path());
        std::fs::remove_file(&path).unwrap();
        let options = || AssistantPolicySetup {
            projector: None,
            output: path.clone(),
            runtime: root.path().join("assets/runtime"),
            model: root.path().join("assets/model"),
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 * 1024 * 1024,
            absolute_assets: false,
            approve_host_inference: true,
        };
        create_assistant_policy(&config, options()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let declaration: Policy = serde_json::from_slice(&original).unwrap();
        assert_eq!(declaration.schema_version, 2);
        assert_eq!(declaration.runtime_path, Path::new("assets/runtime"));
        assert_eq!(
            declaration.model.sha256,
            format!("{:x}", Sha256::digest(b"y"))
        );
        let loaded = load(&path, &config).unwrap();
        let worker =
            measured_identity(&config.worker_executable, config.max_ram_bytes, true).unwrap();
        loaded.match_worker(&worker).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&path).unwrap().nlink(), 1);
        assert!(!config.state_dir.exists());
        assert!(create_assistant_policy(&config, options()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    #[test]
    fn host_setup_refuses_unapproved_unsafe_assets_and_budget_before_publication() {
        let root = tempfile::tempdir().unwrap();
        let (path, _, config) = portable_fixture(root.path());
        std::fs::remove_file(&path).unwrap();
        let options = || AssistantPolicySetup {
            projector: None,
            output: path.clone(),
            runtime: root.path().join("assets/runtime"),
            model: root.path().join("assets/model"),
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512 * 1024 * 1024,
            absolute_assets: false,
            approve_host_inference: true,
        };
        let mut bad = options();
        bad.approve_host_inference = false;
        assert!(create_assistant_policy(&config, bad).is_err());
        let mut bad = options();
        bad.worker_memory_bytes = 1;
        assert!(create_assistant_policy(&config, bad).is_err());
        let mut bad = options();
        bad.model = PathBuf::from("relative");
        assert!(create_assistant_policy(&config, bad).is_err());
        std::os::unix::fs::symlink("model", root.path().join("assets/link")).unwrap();
        let mut bad = options();
        bad.model = root.path().join("assets/link");
        assert!(create_assistant_policy(&config, bad).is_err());
        assert!(!path.exists());
        assert!(!config.state_dir.exists());
        let mut legacy = options();
        legacy.absolute_assets = true;
        create_assistant_policy(&config, legacy).unwrap();
        assert_eq!(load(&path, &config).unwrap().schema_version, 1);
        assert!(measured_identity(&config.worker_executable, 1, true).is_err());
    }
    fn vision_options(root: &Path, output: &Path, absolute_assets: bool) -> AssistantPolicySetup {
        AssistantPolicySetup {
            output: output.to_path_buf(),
            runtime: root.join("assets/runtime"),
            model: root.join("assets/model"),
            projector: Some(root.join("assets/projector")),
            cpu_threads: 1,
            wall_time_ms: 1000,
            worker_memory_bytes: 512u64 << 20,
            absolute_assets,
            approve_host_inference: true,
        }
    }
    #[test]
    fn portable_vision_setup_relocates_all_assets_and_refuses_projector_escape() {
        let root = tempfile::tempdir().unwrap();
        let (path, _, config) = portable_fixture(root.path());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(root.path().join("assets/projector"), b"projector fixture").unwrap();
        create_assistant_policy(&config, vision_options(root.path(), &path, false)).unwrap();
        let policy: Policy = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(policy.schema_version, 3);
        assert_eq!(
            policy.projector.as_ref().unwrap().path,
            Path::new("assets/projector")
        );
        let moved = tempfile::tempdir().unwrap();
        let relocated = moved.path().join("installation");
        std::fs::rename(root.path(), &relocated).unwrap();
        let loaded = load(&relocated.join("assistant.json"), &config).unwrap();
        assert_eq!(
            loaded.projector.as_ref().unwrap().path,
            relocated.join("assets/projector")
        );
        let mut forged = policy.clone();
        forged.projector.as_mut().unwrap().path = "../escape".into();
        write_policy(&relocated.join("assistant.json"), &forged);
        assert!(load(&relocated.join("assistant.json"), &config).is_err());
        write_policy(&relocated.join("assistant.json"), &policy);
        std::fs::remove_file(relocated.join("assets/projector")).unwrap();
        std::os::unix::fs::symlink("model", relocated.join("assets/projector")).unwrap();
        assert!(load(&relocated.join("assistant.json"), &config).is_err());
        assert!(!config.state_dir.exists());
    }
    #[test]
    fn vision_setup_refuses_unsafe_projector_and_budget_before_publication() {
        let root = tempfile::tempdir().unwrap();
        let (path, _, config) = portable_fixture(root.path());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(root.path().join("assets/projector"), b"z").unwrap();
        for case in 0..5 {
            let mut options = vision_options(root.path(), &path, false);
            match case {
                0 => options.projector = Some("relative".into()),
                1 => options.projector = Some(root.path().join("absent")),
                2 => {
                    let alias = root.path().join("assets/link");
                    std::os::unix::fs::symlink("projector", &alias).unwrap();
                    options.projector = Some(alias);
                }
                3 => options.worker_memory_bytes = 256u64 << 20,
                _ => options.cpu_threads = config.max_cpu_threads + 1,
            }
            assert!(
                create_assistant_policy(&config, options).is_err(),
                "case {case}"
            );
            assert!(!path.exists());
        }
        create_assistant_policy(&config, vision_options(root.path(), &path, true)).unwrap();
        let mut policy = load(&path, &config).unwrap();
        assert_eq!(policy.schema_version, 4);
        let original = std::fs::read(&path).unwrap();
        assert!(
            create_assistant_policy(&config, vision_options(root.path(), &path, true)).is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        policy.schema_version = 1;
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
        policy.schema_version = 4;
        policy.projector = None;
        write_policy(&path, &policy);
        assert!(load(&path, &config).is_err());
    }
}
