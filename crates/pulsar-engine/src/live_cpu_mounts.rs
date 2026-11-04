//! Captured CPU live namespace observations. A witness never grants admission.
#![cfg(target_os = "linux")]
use crate::{
    native_capture::SupervisedNativeDependencies,
    native_mounts::{MountedNativeAlias, PinnedNativeMounts},
    worker_retirement::{RetirementAssets, WorkerCustody},
    EngineConfig,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::{File, Metadata},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, MetadataExt},
        },
    },
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

const MAX_LIVE_MS: u64 = 30_000;
const MAX_MODULE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CLOSED_ROOTS: [&str; 3] = ["/usr/lib", "/lib", "/lib64"];
const WORKER_PATH: &str = "/pulsar";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MountedLiveCpuFile {
    pub(crate) path: PathBuf,
    pub(crate) identity: ArtifactIdentity,
    pub(crate) filesystem_device: u64,
    pub(crate) inode: u64,
    pub(crate) mode: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveCpuMountWitness {
    pub(crate) schema_version: u16,
    pub(crate) manifest: LiveCpuInferenceManifest,
    pub(crate) manifest_sha256: String,
    pub(crate) library_directory: PathBuf,
    pub(crate) aliases: Vec<MountedNativeAlias>,
    pub(crate) worker: MountedLiveCpuFile,
    pub(crate) model: MountedLiveCpuFile,
    pub(crate) deadline_ns: u64,
}

/// The caller has already staged these inputs and owns their resource charges.
/// Files must be read-only descriptors for private immutable staged snapshots.
pub(crate) struct LiveCpuMountInputs {
    pub(crate) image: SupervisedNativeDependencies,
    pub(crate) worker: File,
    pub(crate) model: File,
    pub(crate) assets: RetirementAssets,
}
struct OwnedLiveCpuAssets {
    // Pins close before their captured storage owner. Completion guards reside
    // in WorkerTree's dedicated last-drop retirement-assets field, not here.
    descriptors: Vec<File>,
    worker: File,
    model: File,
    image: SupervisedNativeDependencies,
}
pub(crate) struct OwnedLiveCpuMountCommand {
    command: Command,
    custody: WorkerCustody,
    witness: LiveCpuMountWitness,
}
impl OwnedLiveCpuMountCommand {
    pub(crate) fn prepare(
        config: &EngineConfig,
        manifest: &LiveCpuInferenceManifest,
        inputs: LiveCpuMountInputs,
        mut custody: WorkerCustody,
        deadline: Instant,
    ) -> Result<Self> {
        let LiveCpuMountInputs {
            image,
            worker,
            model,
            assets,
        } = inputs;
        // Both transfers are infallible. On every later error the original
        // custody retains snapshots, charges, image, pins and completion guards.
        custody.retain_retirement_assets(assets);
        let group = custody.group_handle();
        let held = custody.retain_asset(OwnedLiveCpuAssets {
            descriptors: Vec::new(),
            worker,
            model,
            image,
        });
        manifest.validate()?;
        let deadline_ns = retain_deadline(deadline, manifest.stream.budget.wall_time_ms)?;
        crate::inference_profiles::admit_tensor_adapter(
            &manifest.stream.profile,
            &manifest.stream.budget,
        )?;
        held.image.captured().verify_loader_ready()?;
        let mounts = PinnedNativeMounts::new_at(
            held.image.captured(),
            Path::new(LIVE_CPU_LIBRARY_DIRECTORY),
        )?;
        let worker = pin_module(
            &held.worker,
            Path::new(WORKER_PATH),
            &manifest.stream.worker,
            true,
            deadline,
        )?;
        let model = pin_module(
            &held.model,
            Path::new(LIVE_CPU_MODEL_PATH),
            &manifest.stream.model.identity,
            false,
            deadline,
        )?;
        let witness = LiveCpuMountWitness {
            schema_version: 1,
            manifest: manifest.clone(),
            manifest_sha256: manifest.identity_sha256()?,
            library_directory: mounts.guest_directory().to_path_buf(),
            aliases: mounts.witnesses(),
            worker,
            model,
            deadline_ns,
        };
        correspondence(&witness)?;
        ensure!(
            serde_json::to_vec(&witness)?.len() <= MAX_CONTROL_BYTES,
            "captured live CPU witness exceeds control frame"
        );
        let command = crate::confinement::live_cpu_image_command(
            config,
            crate::confinement::ConfinementLimits {
                budget: &manifest.stream.budget,
                address_space_bytes: manifest.stream.budget.memory_bytes,
            },
            &group,
            &mounts,
            (held.worker.as_raw_fd(), manifest.stream.worker.byte_len),
            (
                held.model.as_raw_fd(),
                manifest.stream.model.identity.byte_len,
            ),
        )?;
        held.descriptors = mounts.into_descriptors();
        ensure!(
            Instant::now() < deadline,
            "captured live CPU mount preparation exhausted original deadline"
        );
        Ok(Self {
            command,
            custody,
            witness,
        })
    }
    pub(crate) fn into_parts(self) -> (Command, WorkerCustody, LiveCpuMountWitness) {
        (self.command, self.custody, self.witness)
    }
}

// Sampling order only shortens the original parent's deadline. This never
// creates a fresh duration after staging, transport, or native initialization.
fn retain_deadline(end: Instant, budget_ms: u64) -> Result<u64> {
    ensure!(
        (1..=MAX_LIVE_MS).contains(&budget_ms),
        "captured live CPU budget exceeds original live bound"
    );
    crate::cuda_bootstrap::deadline_ns_in_execution(end, budget_ms)
}
fn inherited_deadline(end: u64, budget_ms: u64) -> Result<Instant> {
    ensure!(
        (1..=MAX_LIVE_MS).contains(&budget_ms),
        "captured live CPU budget exceeds original live bound"
    );
    crate::cuda_bootstrap::deadline_instant(end, budget_ms)
}
fn module_correspondence(
    module: &MountedLiveCpuFile,
    path: &Path,
    expected: &ArtifactIdentity,
    executable: bool,
) -> Result<()> {
    module.identity.validate()?;
    ensure!(
        module.path == path
            && module.identity == *expected
            && (1..=MAX_MODULE_BYTES).contains(&module.identity.byte_len)
            && module.filesystem_device != 0
            && module.inode != 0
            && module.mode & !0o777 == 0
            && module.mode & 0o222 == 0
            && module.mode & 0o400 != 0
            && (!executable || module.mode & 0o100 != 0),
        "captured live CPU module identity, guest path or mode differs"
    );
    Ok(())
}
fn correspondence(witness: &LiveCpuMountWitness) -> Result<()> {
    ensure!(
        witness.schema_version == 1,
        "captured live CPU witness schema differs"
    );
    witness.manifest.validate()?;
    ensure!(
        witness.manifest_sha256 == witness.manifest.identity_sha256()?
            && witness.library_directory == Path::new(LIVE_CPU_LIBRARY_DIRECTORY),
        "captured live CPU manifest identity or library coordinate differs"
    );
    crate::inference_profiles::admit_tensor_adapter(
        &witness.manifest.stream.profile,
        &witness.manifest.stream.budget,
    )?;
    let expected: BTreeSet<_> = witness
        .manifest
        .image
        .libraries
        .iter()
        .flat_map(|library| {
            library.names.iter().map(move |name| {
                (
                    name.as_str(),
                    library.identity.sha256.as_str(),
                    library.identity.byte_len,
                )
            })
        })
        .collect();
    let actual: BTreeSet<_> = witness
        .aliases
        .iter()
        .map(|alias| (alias.name.as_str(), alias.sha256.as_str(), alias.byte_len))
        .collect();
    ensure!(
        !witness.aliases.is_empty()
            && witness.aliases.len() <= 512
            && actual.len() == witness.aliases.len()
            && actual == expected
            && witness
                .aliases
                .iter()
                .all(|alias| alias.filesystem_device != 0 && alias.inode != 0),
        "captured live CPU aliases differ from exact image"
    );
    module_correspondence(
        &witness.worker,
        Path::new(WORKER_PATH),
        &witness.manifest.stream.worker,
        true,
    )?;
    module_correspondence(
        &witness.model,
        Path::new(LIVE_CPU_MODEL_PATH),
        &witness.manifest.stream.model.identity,
        false,
    )?;
    inherited_deadline(
        witness.deadline_ns,
        witness.manifest.stream.budget.wall_time_ms,
    )?;
    Ok(())
}

#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    len: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Stamp {
    fn of(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            len: meta.len(),
            mode: meta.mode(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}
fn verify_file(file: &File, module: &MountedLiveCpuFile, deadline: Instant) -> Result<Stamp> {
    ensure!(
        Instant::now() < deadline,
        "captured live CPU module verification deadline expired"
    );
    let meta = file.metadata()?;
    let before = Stamp::of(&meta);
    ensure!(
        meta.is_file()
            && meta.dev() == module.filesystem_device
            && meta.ino() == module.inode
            && meta.len() == module.identity.byte_len
            && meta.mode() & 0o7777 == module.mode,
        "captured live CPU held module identity differs"
    );
    let mut hash = Sha256::new();
    let mut offset = 0u64;
    let mut buffer = [0u8; 65_536];
    loop {
        ensure!(
            Instant::now() < deadline,
            "captured live CPU module hash deadline expired"
        );
        let count = file.read_at(&mut buffer, offset)?;
        if count == 0 {
            break;
        }
        offset = offset
            .checked_add(count as u64)
            .context("captured live CPU module length overflow")?;
        ensure!(
            offset <= module.identity.byte_len,
            "captured live CPU module grew beyond admission"
        );
        hash.update(&buffer[..count]);
    }
    ensure!(
        offset == module.identity.byte_len
            && format!("{:x}", hash.finalize()) == module.identity.sha256
            && Stamp::of(&file.metadata()?) == before,
        "captured live CPU module bytes changed"
    );
    ensure!(
        Instant::now() < deadline,
        "captured live CPU module verification exhausted original deadline"
    );
    Ok(before)
}
fn pin_module(
    file: &File,
    guest: &Path,
    identity: &ArtifactIdentity,
    executable: bool,
    deadline: Instant,
) -> Result<MountedLiveCpuFile> {
    let fd = file.as_raw_fd();
    let access = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    let descriptor = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    ensure!(
        (3..1024).contains(&fd)
            && access >= 0
            && access & libc::O_ACCMODE == libc::O_RDONLY
            && descriptor >= 0
            && descriptor & libc::FD_CLOEXEC != 0,
        "captured live CPU input descriptor must be read-only and CLOEXEC"
    );
    let meta = file.metadata()?;
    let module = MountedLiveCpuFile {
        path: guest.to_owned(),
        identity: identity.clone(),
        filesystem_device: meta.dev(),
        inode: meta.ino(),
        mode: meta.mode() & 0o7777,
    };
    module_correspondence(&module, guest, identity, executable)?;
    verify_file(file, &module, deadline)?;
    Ok(module)
}
fn verify_mounted_module(module: &MountedLiveCpuFile, deadline: Instant) -> Result<()> {
    require_readonly_mount(&module.path)?;
    let file = crate::artifacts::open_regular(&module.path)?;
    let before = verify_file(&file, module, deadline)?;
    ensure!(
        Stamp::of(&crate::artifacts::open_regular(&module.path)?.metadata()?) == before,
        "captured live CPU module name changed after verification"
    );
    Ok(())
}
fn require_readonly_mount(path: &Path) -> Result<()> {
    let name = CString::new(path.as_os_str().as_bytes())?;
    let mut status: libc::statvfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::statvfs(name.as_ptr(), &mut status) } == 0
            && status.f_flag & libc::ST_RDONLY != 0,
        "captured live CPU namespace contains an absent or writable runtime mount"
    );
    Ok(())
}
fn verify_witness(witness: &LiveCpuMountWitness, deadline: Instant) -> Result<()> {
    correspondence(witness)?;
    ensure!(
        Instant::now() < deadline,
        "captured live CPU bootstrap deadline expired"
    );
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1")
            && std::env::var_os("LD_LIBRARY_PATH").as_deref()
                == Some(witness.library_directory.as_os_str()),
        "captured live CPU confinement or descendant loader path differs"
    );
    require_readonly_mount(Path::new("/"))?;
    require_readonly_mount(&witness.library_directory)?;
    for root in CLOSED_ROOTS {
        require_readonly_mount(Path::new(root))?;
        crate::native_mounts::verify_mounted(Path::new(root), &witness.aliases)?;
    }
    ensure!(
        matches!(std::fs::symlink_metadata("/etc/ld.so.cache"), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "captured live CPU namespace exposes loader-cache fallback"
    );
    let loader =
        crate::artifacts::open_regular(&witness.library_directory.join("ld-linux-x86-64.so.2"))?;
    ensure!(
        loader.metadata()?.mode() & 0o7777 == 0o500,
        "captured live CPU loader mode differs"
    );
    crate::native_mount_bytes::verify(&witness.library_directory, &witness.aliases, deadline)?;
    verify_mounted_module(&witness.worker, deadline)?;
    verify_mounted_module(&witness.model, deadline)?;
    ensure!(
        Instant::now() < deadline,
        "captured live CPU bootstrap exhausted original deadline"
    );
    Ok(())
}

/// Called before reading the bounded live witness. It grants no launch authority.
pub(crate) fn clear_inherited_descriptors() -> Result<()> {
    crate::cpu_mounts::clear_inherited_descriptors()
}
/// Native observation tied to this borrowed witness and its original deadline.
/// Private fields and no Deserialize prevent metadata from minting this proof.
pub(crate) struct VerifiedLiveCpuBootstrap<'a> {
    witness: &'a LiveCpuMountWitness,
    deadline: Instant,
}
impl VerifiedLiveCpuBootstrap<'_> {
    pub(crate) fn manifest(&self) -> &LiveCpuInferenceManifest {
        &self.witness.manifest
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn verify_after_execution(&self) -> Result<()> {
        verify_witness(self.witness, self.deadline)
    }
}
pub(crate) fn verify_bootstrap(
    witness: &LiveCpuMountWitness,
) -> Result<VerifiedLiveCpuBootstrap<'_>> {
    let deadline = inherited_deadline(
        witness.deadline_ns,
        witness.manifest.stream.budget.wall_time_ms,
    )?;
    verify_witness(witness, deadline)?;
    Ok(VerifiedLiveCpuBootstrap { witness, deadline })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
    fn artifact(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 10,
        }
    }
    fn manifest() -> LiveCpuInferenceManifest {
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: artifact(3),
            libraries: CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(i, name)| CapturedNativeLibrary {
                    identity: artifact(i as u8 + 10),
                    names: vec![name.to_string()],
                })
                .collect(),
        };
        let model = artifact(1);
        let runtime = artifact(12);
        let identity = LiveIdentity {
            epoch: AttemptId::new("epoch").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(2),
            source: SourceVersionId::new("source").unwrap(),
        };
        let stream = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            clock: LivePresentationClock {
                epoch: identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            identity,
            model: ArtifactReference {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: model.clone(),
            },
            runtime: ArtifactReference {
                path: LIVE_CPU_RUNTIME_PATH.into(),
                identity: runtime.clone(),
            },
            worker: artifact(2),
            profile: DeclaredInferenceProfile {
                schema_version: 1,
                model,
                runtime,
                preprocessing: InferencePreprocessing {
                    input: ModelInputContract {
                        width: 64,
                        height: 64,
                        channels: 6,
                        class_count: 1,
                        layout: TensorLayout::Nchw,
                        decoder: DetectorDecoder::TemporalScriptPosition,
                    },
                    resize: InferenceResize::NearestStretch,
                    color: InferenceColor::RgbFramePair,
                    scale: vec![1.0 / 255.0; 6],
                    bias: vec![0.0; 6],
                    padding_byte: 0,
                },
                provider: InferenceProvider::Cpu,
                minimum_memory_bytes: 128 << 20,
                minimum_output_bytes: 4096,
                cpu_threads: 2,
                cuda_resources: None,
            },
            budget: ResourceBudget {
                memory_bytes: 256 << 20,
                output_bytes: 4096,
                wall_time_ms: 30_000,
                cpu_threads: 2,
            },
        };
        LiveCpuInferenceManifest::new(stream, image).unwrap()
    }

    fn witness() -> LiveCpuMountWitness {
        let manifest = manifest();
        let aliases = manifest
            .image
            .libraries
            .iter()
            .enumerate()
            .flat_map(|(index, library)| {
                library.names.iter().map(move |name| MountedNativeAlias {
                    name: name.clone(),
                    sha256: library.identity.sha256.clone(),
                    byte_len: library.identity.byte_len,
                    filesystem_device: 1,
                    inode: index as u64 + 1,
                })
            })
            .collect();
        LiveCpuMountWitness {
            schema_version: 1,
            manifest_sha256: manifest.identity_sha256().unwrap(),
            worker: MountedLiveCpuFile {
                path: WORKER_PATH.into(),
                identity: manifest.stream.worker.clone(),
                filesystem_device: 1,
                inode: 100,
                mode: 0o500,
            },
            model: MountedLiveCpuFile {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: manifest.stream.model.identity.clone(),
                filesystem_device: 1,
                inode: 101,
                mode: 0o400,
            },
            manifest,
            library_directory: LIVE_CPU_LIBRARY_DIRECTORY.into(),
            aliases,
            deadline_ns: retain_deadline(Instant::now() + Duration::from_secs(5), 30_000).unwrap(),
        }
    }
    #[test]
    fn distinct_live_witness_accepts_only_exact_correspondence() {
        let witness = witness();
        correspondence(&witness).unwrap();
        let raw = serde_json::to_vec(&witness).unwrap();
        let decoded: LiveCpuMountWitness = serde_json::from_slice(&raw).unwrap();
        correspondence(&decoded).unwrap();
        assert!(serde_json::from_slice::<crate::cpu_mounts::CpuMountWitness>(&raw).is_err());
    }
    #[test]
    fn live_witness_refuses_hash_provider_dependency_and_clock_rebinding() {
        let mut value = witness();
        value.manifest_sha256 = "0".repeat(64);
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.manifest.stream.profile.provider = InferenceProvider::Cuda;
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.manifest.dependencies.pop();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.manifest.stream.clock.epoch = AttemptId::new("different-epoch").unwrap();
        assert!(correspondence(&value).is_err());
    }
    #[test]
    fn live_witness_refuses_missing_extra_duplicate_and_rebound_aliases() {
        let mut value = witness();
        value.aliases.pop();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.aliases.push(value.aliases[0].clone());
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.aliases[0].name = "unlisted.so".into();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.aliases[0].sha256 = "0".repeat(64);
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.aliases[0].inode = 0;
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.library_directory = "/usr/lib".into();
        assert!(correspondence(&value).is_err());
    }
    #[test]
    fn live_witness_refuses_worker_model_path_identity_and_write_mode_changes() {
        let mut value = witness();
        value.worker.identity = value.model.identity.clone();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.model.path = "/out/model".into();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.worker.path = "/usr/bin/pulsar".into();
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.worker.mode = 0o400;
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.model.mode = 0o600;
        assert!(correspondence(&value).is_err());
        let mut value = witness();
        value.model.mode = 0o4400;
        assert!(correspondence(&value).is_err());
    }
    #[test]
    fn original_deadline_never_extends_or_accepts_a_fresh_overlong_budget() {
        let end = Instant::now() + Duration::from_secs(2);
        let clock = retain_deadline(end, 30_000).unwrap();
        assert!(inherited_deadline(clock, 30_000).unwrap() <= end);
        assert!(retain_deadline(Instant::now(), 30_000).is_err());
        assert!(retain_deadline(Instant::now() + Duration::from_secs(31), 30_000).is_err());
        assert!(retain_deadline(end, 30_001).is_err());
        assert!(inherited_deadline(0, 30_000).is_err());
        let mut value = witness();
        value.deadline_ns = 0;
        assert!(correspondence(&value).is_err());
    }
    fn file_identity(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    #[test]
    fn fd_pin_survives_supplier_name_replacement_but_replacement_cannot_match_mount_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model");
        let bytes = b"neutral staged model bytes";
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        let file = File::open(&path).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        let module = pin_module(
            &file,
            Path::new(LIVE_CPU_MODEL_PATH),
            &file_identity(bytes),
            false,
            end,
        )
        .unwrap();
        fs::rename(&path, root.path().join("held-original")).unwrap();
        fs::write(&path, bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        verify_file(&file, &module, end).unwrap();
        assert!(verify_file(&File::open(&path).unwrap(), &module, end).is_err());
    }
    #[test]
    fn byte_change_and_writable_descriptor_cannot_become_verified_module() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model");
        let bytes = b"neutral staged model bytes";
        fs::write(&path, bytes).unwrap();
        let writable = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        assert!(pin_module(
            &writable,
            Path::new(LIVE_CPU_MODEL_PATH),
            &file_identity(bytes),
            false,
            end
        )
        .is_err());
        drop(writable);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        let file = File::open(&path).unwrap();
        let module = pin_module(
            &file,
            Path::new(LIVE_CPU_MODEL_PATH),
            &file_identity(bytes),
            false,
            end,
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, b"different staged model xxx").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(verify_file(&file, &module, end).is_err());
        assert!(verify_file(&file, &module, Instant::now()).is_err());
    }
    #[test]
    fn metadata_roundtrip_cannot_substitute_for_verified_bootstrap() {
        let mut raw = serde_json::to_value(witness()).unwrap();
        raw["verified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveCpuMountWitness>(raw).is_err());
        // Calling the verifier outside the exact namespace must fail before a
        // native loader is invoked; no proof is returned by JSON deserialization.
        assert!(verify_bootstrap(&witness()).is_err());
    }
}
