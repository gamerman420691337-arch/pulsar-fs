//! Request-bound captured CPU mounts. Byte observations are not qualification.
#![cfg(target_os = "linux")]
use crate::{
    generation_progress::{self, Envelope},
    native_capture::SupervisedNativeDependencies,
    native_mounts::{MountedNativeAlias, PinnedNativeMounts},
    worker_retirement::WorkerCustody,
    EngineConfig,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::File,
    io::Read,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use crate::native_mount_bytes as bytes;

pub(crate) const CPU_LIBRARY_DIRECTORY: &str = "/pulsar-cpu-libraries";
const RESUME_GUEST: &str = "/pulsar-generation-resume.json";
const MAX_DEADLINE_MS: u64 = 86_400_000;
const CLOSED_ROOTS: [&str; 3] = ["/usr/lib", "/lib", "/lib64"];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CpuMountWitness {
    pub(crate) schema_version: u16,
    pub(crate) request: WorkerRequest,
    pub(crate) request_sha256: String,
    pub(crate) image: CpuRuntimeImage,
    pub(crate) library_directory: PathBuf,
    pub(crate) aliases: Vec<MountedNativeAlias>,
    pub(crate) progress: Option<Envelope>,
    pub(crate) deadline_ns: u64,
}

fn request_hash(request: &WorkerRequest) -> Result<String> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(
        bytes.len() <= MAX_CONTROL_BYTES,
        "CPU request exceeds control bound"
    );
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path != Path::new("/")
        && !path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        && !path.as_os_str().as_bytes().contains(&0)
}
fn progress_correspondence(
    request: &WorkerRequest,
    progress: Option<&Envelope>,
    mounted: bool,
) -> Result<()> {
    ensure!(
        progress.is_some() == generation_progress::eligible(request),
        "CPU progress framing differs from request"
    );
    let Some(envelope) = progress else {
        return Ok(());
    };
    ensure!(
        serde_json::to_vec(&envelope.request)? == serde_json::to_vec(request)?,
        "CPU progress request differs from mount request"
    );
    if let Some(binding) = &envelope.resume {
        binding.record.validate(request, true)?;
        binding.body.identity.validate()?;
        let expected = if mounted {
            PathBuf::from(RESUME_GUEST)
        } else {
            request.output_dir.join("resume-progress.json")
        };
        ensure!(
            binding.body.path == expected
                && binding.body.identity.artifact_id.as_str() == binding.record.body_sha256
                && binding.body.identity.sha256 == binding.record.body_sha256
                && binding.body.identity.byte_len == binding.record.byte_len,
            "CPU resume member differs from retained progress boundary"
        );
    }
    Ok(())
}
fn alias_correspondence(image: &CpuRuntimeImage, aliases: &[MountedNativeAlias]) -> Result<()> {
    let expected = image
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
        .collect::<BTreeSet<_>>();
    let actual = aliases
        .iter()
        .map(|alias| (alias.name.as_str(), alias.sha256.as_str(), alias.byte_len))
        .collect::<BTreeSet<_>>();
    ensure!(
        !aliases.is_empty()
            && aliases.len() <= 512
            && expected == actual
            && actual.len() == aliases.len(),
        "CPU mounted aliases differ from image"
    );
    Ok(())
}
fn correspondence(witness: &CpuMountWitness) -> Result<()> {
    ensure!(
        witness.schema_version == 1,
        "unsupported CPU mount witness schema"
    );
    witness.request.validate()?;
    ensure!(
        witness.request.cuda_execution.is_none()
            && witness.request_sha256 == request_hash(&witness.request)?,
        "CPU mount request identity or provider differs"
    );
    let profile = crate::inference_profiles::resolve_profile(&witness.request)?
        .context("closed CPU mounts require inference")?;
    ensure!(
        profile.provider == InferenceProvider::Cpu,
        "closed CPU mounts require CPU profile"
    );
    witness.image.validate()?;
    ensure!(
        witness.library_directory == Path::new(CPU_LIBRARY_DIRECTORY)
            && normalized_absolute(&witness.request.output_dir)
            && !witness
                .library_directory
                .starts_with(&witness.request.output_dir)
            && !witness
                .request
                .output_dir
                .starts_with(&witness.library_directory)
            && witness.request.output_dir != Path::new("/pulsar")
            && !CLOSED_ROOTS.iter().any(|root| Path::new(root)
                .starts_with(&witness.request.output_dir)
                || witness.request.output_dir.starts_with(root)),
        "invalid captured CPU library directory"
    );
    let runtime = witness
        .request
        .tools
        .onnx_runtime
        .as_ref()
        .context("CPU runtime missing")?;
    let captured_runtime = witness
        .image
        .libraries
        .iter()
        .find(|library| {
            library
                .names
                .iter()
                .any(|name| name == "libonnxruntime.so.1")
        })
        .context("captured CPU ONNX runtime alias missing")?;
    ensure!(
        runtime.path == witness.library_directory.join("libonnxruntime.so.1")
            && runtime.identity == captured_runtime.identity
            && profile.runtime == captured_runtime.identity,
        "CPU runtime path or identity differs from image"
    );
    ensure!(
        witness
            .request
            .dependencies
            .contains(&witness.image.identity()?)
            && witness
                .request
                .dependencies
                .contains(&witness.image.owner_policy)
            && witness
                .image
                .libraries
                .iter()
                .all(|library| witness.request.dependencies.contains(&library.identity)),
        "CPU request omits captured image dependencies"
    );
    alias_correspondence(&witness.image, &witness.aliases)?;
    progress_correspondence(&witness.request, witness.progress.as_ref(), true)?;
    retained_deadline(witness.deadline_ns, witness.request.budget.wall_time_ms)?;
    Ok(())
}

/// Sample the relay clock first, then local remaining time. Sampling latency can
/// shorten the parent's original deadline; it cannot create a new wall budget.
fn deadline_ns(end: Instant, budget_ms: u64) -> Result<u64> {
    ensure!(
        (1..=MAX_DEADLINE_MS).contains(&budget_ms),
        "CPU wall budget exceeds one day"
    );
    let before = crate::cuda_bootstrap::now_ns()?;
    let remaining = end
        .checked_duration_since(Instant::now())
        .context("CPU parent deadline expired")?;
    ensure!(
        !remaining.is_zero() && remaining <= Duration::from_millis(budget_ms),
        "CPU parent deadline exceeds request budget"
    );
    before
        .checked_add(u64::try_from(remaining.as_nanos()).context("CPU deadline overflow")?)
        .context("CPU deadline overflow")
}
fn retained_deadline(end: u64, budget_ms: u64) -> Result<Instant> {
    ensure!(
        (1..=MAX_DEADLINE_MS).contains(&budget_ms),
        "CPU wall budget exceeds one day"
    );
    crate::cuda_bootstrap::deadline_instant(end, budget_ms)
}

struct OwnedCpuAssets {
    // Descriptor pins close before the image storage owner is released.
    descriptors: Vec<File>,
    image: SupervisedNativeDependencies,
}
pub(crate) struct OwnedCpuMountCommand {
    command: Command,
    custody: WorkerCustody,
    witness: CpuMountWitness,
}
impl OwnedCpuMountCommand {
    pub(crate) fn prepare(
        config: &EngineConfig,
        request: &WorkerRequest,
        image: SupervisedNativeDependencies,
        descriptor: CpuRuntimeImage,
        mut custody: WorkerCustody,
        mut progress: Option<Envelope>,
        deadline: Instant,
    ) -> Result<Self> {
        let group = custody.group_handle();
        // No fallible work precedes retention: setup errors retire the same
        // custody, including image storage and any already-held input staging.
        let assets = custody.retain_asset(OwnedCpuAssets {
            descriptors: Vec::new(),
            image,
        });
        assets.image.captured().verify_loader_ready()?;
        let mounts =
            PinnedNativeMounts::new_at(assets.image.captured(), Path::new(CPU_LIBRARY_DIRECTORY))?;
        let library_directory = mounts.guest_directory().to_path_buf();
        progress_correspondence(request, progress.as_ref(), false)?;
        let mut bindings = Vec::new();
        let model = match &request.operation {
            WorkerOperation::Generate { model, .. } | WorkerOperation::Preview { model, .. } => {
                model.as_ref()
            }
            _ => None,
        }
        .context("closed CPU worker model missing")?;
        for path in std::iter::once(&request.source.path)
            .chain(std::iter::once(&model.path))
            .chain(request.independent_audio.iter().map(|audio| &audio.path))
            .chain([&request.tools.ffmpeg.path, &request.tools.ffprobe.path])
        {
            ensure!(
                normalized_absolute(path) && path.canonicalize()? == *path,
                "CPU worker input path must be canonical"
            );
            ensure!(
                !CLOSED_ROOTS.iter().any(|root| path.starts_with(root))
                    && !path.starts_with(&library_directory)
                    && !path.starts_with(&request.output_dir)
                    && path != Path::new("/pulsar")
                    && path != Path::new(RESUME_GUEST)
                    && path != Path::new("/etc/ld.so.cache"),
                "CPU input overlaps reserved runtime or output path"
            );
            // A declared input must never expose a whole host directory.
            crate::artifacts::open_regular(path)?;
            bindings.push((path.clone(), path.clone()));
        }
        if let Some(binding) = progress
            .as_mut()
            .and_then(|envelope| envelope.resume.as_mut())
        {
            ensure!(
                normalized_absolute(&binding.body.path)
                    && binding.body.path.canonicalize()? == binding.body.path,
                "CPU resume path must be canonical"
            );
            verify_resume_bytes(&binding.body.path, &binding.body.identity, deadline)?;
            let guest = PathBuf::from(RESUME_GUEST);
            bindings.push((binding.body.path.clone(), guest.clone()));
            binding.body.path = guest;
        }
        let witness = CpuMountWitness {
            schema_version: 1,
            request: request.clone(),
            request_sha256: request_hash(request)?,
            image: descriptor,
            library_directory,
            aliases: mounts.witnesses(),
            progress,
            deadline_ns: deadline_ns(deadline, request.budget.wall_time_ms)?,
        };
        correspondence(&witness)?;
        ensure!(
            serde_json::to_vec(&witness)?.len() <= MAX_CONTROL_BYTES,
            "CPU mount witness exceeds control bound"
        );
        let command = crate::confinement::cpu_image_command(
            config,
            crate::confinement::ConfinementLimits {
                budget: &request.budget,
                address_space_bytes: request.budget.memory_bytes,
            },
            &group,
            &bindings,
            &request.output_dir,
            &mounts,
        )?;
        assets.descriptors = mounts.into_descriptors();
        ensure!(
            Instant::now() < deadline,
            "CPU mount setup exhausted parent deadline"
        );
        Ok(Self {
            command,
            custody,
            witness,
        })
    }
    pub(crate) fn into_parts(self) -> (Command, WorkerCustody, CpuMountWitness) {
        (self.command, self.custody, self.witness)
    }
}

/// Erase descriptor-based paths before request intake and input/native loading.
pub(crate) fn clear_inherited_descriptors() -> Result<()> {
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "CPU bootstrap requires parent confinement"
    );
    ensure!(
        unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0,
        "CPU bootstrap cannot erase inherited descriptors: {}",
        std::io::Error::last_os_error()
    );
    for fd in 3..1024 {
        ensure!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EBADF),
            "CPU bootstrap inherited descriptor remains open"
        );
    }
    Ok(())
}

/// A live observation of this namespace and retained deadline, never a scheduling lease.
pub(crate) struct VerifiedCpuBootstrap<'a> {
    witness: &'a CpuMountWitness,
    deadline: Instant,
}
impl VerifiedCpuBootstrap<'_> {
    pub(crate) fn request(&self) -> &WorkerRequest {
        &self.witness.request
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn verify_after_execution(&self) -> Result<()> {
        verify_witness(self.witness, self.deadline)
    }
}
pub(crate) fn verify_bootstrap(witness: &CpuMountWitness) -> Result<VerifiedCpuBootstrap<'_>> {
    let deadline = retained_deadline(witness.deadline_ns, witness.request.budget.wall_time_ms)?;
    verify_witness(witness, deadline)?;
    Ok(VerifiedCpuBootstrap { witness, deadline })
}
fn verify_resume_bytes(path: &Path, identity: &ArtifactIdentity, deadline: Instant) -> Result<()> {
    identity.validate()?;
    let mut file = crate::artifacts::open_regular(path)?;
    let stamp = |metadata: &std::fs::Metadata| {
        (
            metadata.dev(),
            metadata.ino(),
            metadata.len(),
            metadata.mode(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    };
    let before = stamp(&file.metadata()?);
    ensure!(before.2 == identity.byte_len, "CPU resume length differs");
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65_536];
    loop {
        ensure!(
            Instant::now() < deadline,
            "CPU resume verification deadline expired"
        );
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .context("CPU resume size overflow")?;
        ensure!(
            count <= identity.byte_len,
            "CPU resume grew beyond retained bound"
        );
        hash.update(&buffer[..read]);
    }
    ensure!(
        count == identity.byte_len
            && format!("{:x}", hash.finalize()) == identity.sha256
            && stamp(&file.metadata()?) == before
            && stamp(&crate::artifacts::open_regular(path)?.metadata()?) == before,
        "CPU resume bytes changed"
    );
    ensure!(
        Instant::now() < deadline,
        "CPU resume verification deadline expired"
    );
    Ok(())
}

fn require_readonly_mount(path: &Path) -> Result<()> {
    let name = CString::new(path.as_os_str().as_bytes())?;
    let mut status: libc::statvfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::statvfs(name.as_ptr(), &mut status) } == 0,
        "CPU namespace mount status unavailable: {}",
        std::io::Error::last_os_error()
    );
    ensure!(
        status.f_flag & libc::ST_RDONLY != 0,
        "CPU runtime mount is writable"
    );
    Ok(())
}
fn verify_witness(witness: &CpuMountWitness, deadline: Instant) -> Result<()> {
    correspondence(witness)?;
    ensure!(Instant::now() < deadline, "CPU bootstrap deadline expired");
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "CPU bootstrap requires confinement"
    );
    ensure!(
        std::env::var_os("LD_LIBRARY_PATH").as_deref()
            == Some(witness.library_directory.as_os_str()),
        "CPU descendant loader path differs from captured image"
    );
    require_readonly_mount(Path::new("/"))?;
    require_readonly_mount(&witness.library_directory)?;
    for root in CLOSED_ROOTS {
        require_readonly_mount(Path::new(root))?;
        crate::native_mounts::verify_mounted(Path::new(root), &witness.aliases)?;
    }
    ensure!(
        matches!(std::fs::symlink_metadata("/etc/ld.so.cache"), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "CPU namespace exposes a loader cache"
    );
    let loader =
        crate::artifacts::open_regular(&witness.library_directory.join("ld-linux-x86-64.so.2"))?;
    ensure!(
        loader.metadata()?.permissions().mode() & 0o7777 == 0o500,
        "CPU captured loader mode differs"
    );
    bytes::verify(&witness.library_directory, &witness.aliases, deadline)?;
    if let Some(binding) = witness
        .progress
        .as_ref()
        .and_then(|progress| progress.resume.as_ref())
    {
        require_readonly_mount(&binding.body.path)?;
        verify_resume_bytes(&binding.body.path, &binding.body.identity, deadline)?;
    }
    ensure!(
        Instant::now() < deadline,
        "CPU bootstrap exhausted parent deadline"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(index: u8) -> ArtifactIdentity {
        let sha256 = format!("{index:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }
    fn fixture() -> CpuMountWitness {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        request.cuda_execution = None;
        let directory = PathBuf::from(CPU_LIBRARY_DIRECTORY);
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries: CPU_RUNTIME_REQUIRED_ALIASES
                .iter()
                .enumerate()
                .map(|(index, alias)| CapturedNativeLibrary {
                    identity: identity(index as u8 + 1),
                    names: vec![alias.to_string()],
                })
                .collect(),
        };
        let runtime = image
            .libraries
            .iter()
            .find(|library| {
                library
                    .names
                    .iter()
                    .any(|name| name == "libonnxruntime.so.1")
            })
            .unwrap()
            .identity
            .clone();
        request.tools.onnx_runtime = Some(ArtifactReference {
            path: directory.join("libonnxruntime.so.1"),
            identity: runtime.clone(),
        });
        let WorkerOperation::Generate {
            settings, model, ..
        } = &mut request.operation
        else {
            panic!("fixture generate");
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        profile.runtime = runtime;
        request.dependencies = vec![
            model.as_ref().unwrap().identity.clone(),
            image.owner_policy.clone(),
            image.identity().unwrap(),
        ];
        request.dependencies.extend(
            image
                .libraries
                .iter()
                .map(|library| library.identity.clone()),
        );
        let aliases = image
            .libraries
            .iter()
            .flat_map(|library| {
                library.names.iter().map(move |name| MountedNativeAlias {
                    name: name.clone(),
                    sha256: library.identity.sha256.clone(),
                    byte_len: library.identity.byte_len,
                    filesystem_device: 1,
                    inode: 1,
                })
            })
            .collect();
        CpuMountWitness {
            schema_version: 1,
            request_sha256: request_hash(&request).unwrap(),
            deadline_ns: deadline_ns(
                Instant::now() + Duration::from_secs(10),
                request.budget.wall_time_ms,
            )
            .unwrap(),
            progress: Some(Envelope {
                request: request.clone(),
                resume: None,
            }),
            request,
            image,
            library_directory: directory,
            aliases,
        }
    }
    fn rebind(witness: &mut CpuMountWitness) {
        witness.request_sha256 = request_hash(&witness.request).unwrap();
        if let Some(progress) = &mut witness.progress {
            progress.request = witness.request.clone();
        }
    }
    #[test]
    fn witness_binds_request_image_runtime_and_all_declared_aliases() {
        correspondence(&fixture()).unwrap();
        for change in [
            |w: &mut CpuMountWitness| w.schema_version = 2,
            |w: &mut CpuMountWitness| w.request_sha256 = "0".repeat(64),
            |w: &mut CpuMountWitness| w.aliases.pop().map(drop).unwrap(),
            |w: &mut CpuMountWitness| w.aliases.push(w.aliases[0].clone()),
            |w: &mut CpuMountWitness| w.aliases[0].sha256 = "0".repeat(64),
            |w: &mut CpuMountWitness| w.aliases[0].name = "../injected.so".into(),
            |w: &mut CpuMountWitness| w.library_directory = "/usr/lib".into(),
            |w: &mut CpuMountWitness| w.library_directory = "/authored/../image".into(),
            |w: &mut CpuMountWitness| w.image.owner_policy = identity(10),
            |w: &mut CpuMountWitness| {
                w.request.dependencies.pop();
                rebind(w);
            },
            |w: &mut CpuMountWitness| {
                w.request.tools.onnx_runtime.as_mut().unwrap().path = "/host/runtime.so".into();
                rebind(w);
            },
            |w: &mut CpuMountWitness| {
                w.request.tools.onnx_runtime.as_mut().unwrap().identity = identity(30);
                rebind(w);
            },
        ] {
            let mut bad = fixture();
            change(&mut bad);
            assert!(correspondence(&bad).is_err());
        }
        let mut json = serde_json::to_value(fixture()).unwrap();
        json["qualified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CpuMountWitness>(json).is_err());
    }
    #[test]
    fn non_cpu_provider_and_absent_inference_refuse_even_after_rehash() {
        let mut bad = fixture();
        let WorkerOperation::Generate { settings, .. } = &mut bad.request.operation else {
            panic!()
        };
        settings.inference_profile.as_mut().unwrap().provider = InferenceProvider::Cuda;
        rebind(&mut bad);
        assert!(correspondence(&bad).is_err());
        let mut bad = fixture();
        bad.request.operation = WorkerOperation::ProbeMedia;
        bad.request.tools.onnx_runtime = None;
        bad.progress = None;
        rebind(&mut bad);
        assert!(correspondence(&bad).is_err());
    }
    #[test]
    fn progress_and_resume_cannot_rebind_another_request_or_member() {
        let mut bad = fixture();
        bad.progress = None;
        assert!(correspondence(&bad).is_err());
        let mut bad = fixture();
        bad.progress.as_mut().unwrap().request.attempt_id =
            AttemptId::new("another-attempt").unwrap();
        assert!(correspondence(&bad).is_err());
        let mut witness = fixture();
        let record = generation_progress::Record {
            schema: 1,
            sequence: 1,
            attempt: AttemptId::new("previous-attempt").unwrap(),
            job: witness.request.job_id.clone(),
            basis_sha256: generation_progress::basis(&witness.request).unwrap(),
            next_source_frame: 1,
            analysed_samples: 1,
            body_sha256: identity(50).sha256,
            byte_len: 1,
        };
        witness.progress.as_mut().unwrap().resume = Some(generation_progress::Binding {
            body: ArtifactReference {
                path: RESUME_GUEST.into(),
                identity: identity(50),
            },
            record,
        });
        correspondence(&witness).unwrap();
        witness
            .progress
            .as_mut()
            .unwrap()
            .resume
            .as_mut()
            .unwrap()
            .body
            .path = "/unbound-resume.json".into();
        assert!(correspondence(&witness).is_err());
        witness
            .progress
            .as_mut()
            .unwrap()
            .resume
            .as_mut()
            .unwrap()
            .body
            .path = RESUME_GUEST.into();
        witness
            .progress
            .as_mut()
            .unwrap()
            .resume
            .as_mut()
            .unwrap()
            .record
            .basis_sha256 = "0".repeat(64);
        assert!(correspondence(&witness).is_err());
    }
    #[test]
    fn original_parent_deadline_is_never_renewed() {
        let parent = Instant::now() + Duration::from_secs(1);
        let relay = deadline_ns(parent, 1_000).unwrap();
        let child = retained_deadline(relay, 1_000).unwrap();
        assert!(child <= parent);
        assert!(deadline_ns(Instant::now(), 1_000).is_err());
        assert!(deadline_ns(Instant::now() + Duration::from_secs(2), 1_000).is_err());
        assert!(retained_deadline(0, 1_000).is_err());
        assert!(retained_deadline(u64::MAX, 1_000).is_err());
        assert!(deadline_ns(parent, MAX_DEADLINE_MS + 1).is_err());
        assert!(retained_deadline(relay, MAX_DEADLINE_MS + 1).is_err());
    }
    #[test]
    fn witness_refuses_expired_and_overbudget_deadlines() {
        let mut witness = fixture();
        witness.deadline_ns = 0;
        assert!(correspondence(&witness).is_err());
        witness.deadline_ns = u64::MAX;
        assert!(correspondence(&witness).is_err());
    }
    #[test]
    fn resume_hash_requires_original_length_bytes_and_live_deadline() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        let content = b"authored checkpoint";
        std::fs::write(&path, content).unwrap();
        let hash = format!("{:x}", Sha256::digest(content));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: content.len() as u64,
        };
        verify_resume_bytes(&path, &identity, Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(verify_resume_bytes(&path, &identity, Instant::now()).is_err());
        std::fs::write(&path, b"different checkpoint").unwrap();
        assert!(
            verify_resume_bytes(&path, &identity, Instant::now() + Duration::from_secs(1)).is_err()
        );
        assert!(require_readonly_mount(root.path()).is_err());
    }

    #[test]
    #[ignore = "requires pinned native worker and delegated cgroups; run serially"]
    fn failed_cpu_mount_setup_retains_capture_and_budget_until_transport_retirement() {
        use crate::resources::ResourcePool;
        use std::sync::{atomic::AtomicBool, Arc};
        let worker = std::env::var_os("PULSAR_BENCHMARK_WORKER").expect("pin actual worker");
        let root = tempfile::tempdir().unwrap();
        let config = EngineConfig::new(root.path().join("unused-state"), worker.into());
        let capture_pool = ResourcePool::new(1024 * 1024 * 1024, 1);
        let worker_pool = ResourcePool::new(1024 * 1024 * 1024, 1);
        let mut spec = NativeDependencySpec {
            schema_version: 1,
            libraries: Vec::new(),
        };
        for (index, alias) in CPU_RUNTIME_REQUIRED_ALIASES.iter().enumerate() {
            let content = vec![index as u8; index + 1];
            let path = root.path().join(format!("supplier-{index}"));
            std::fs::write(&path, &content).unwrap();
            spec.libraries.push(NativeLibrarySpec {
                path,
                sha256: format!("{:x}", Sha256::digest(&content)),
                byte_len: content.len() as u64,
                names: vec![alias.to_string()],
            });
        }
        let image = crate::native_capture::capture(
            &config,
            &capture_pool,
            &spec,
            &root.path().join("capture"),
            crate::native_capture::preparation_budget(spec.validate().unwrap(), 10_000).unwrap(),
            1536 * 1024 * 1024,
            &AtomicBool::new(false),
        )
        .unwrap();
        let library_directory = image.captured().library_directory();
        let mut witness = fixture();
        let input = root.path().join("input");
        std::fs::write(&input, b"authored input").unwrap();
        witness.request.source.path = input.clone();
        let WorkerOperation::Generate { model, .. } = &mut witness.request.operation else {
            panic!()
        };
        model.as_mut().unwrap().path = input.clone();
        witness.request.tools.ffmpeg.path = input.clone();
        witness.request.tools.ffprobe.path = input;
        let progress = Some(Envelope {
            request: witness.request.clone(),
            resume: None,
        });
        let bytes = witness.request.budget.memory_bytes;
        let reservation = Arc::new(
            worker_pool
                .reserve_worker(bytes, witness.request.budget.cpu_threads)
                .unwrap(),
        );
        let group = crate::cgroup::AttemptCgroup::create(
            "closed-cpu-failed-setup",
            bytes,
            witness.request.budget.cpu_threads,
        )
        .unwrap();
        let group_path = group.path().to_owned();
        let mut custody = WorkerCustody::prepare(group, reservation, None).unwrap();
        let (release, waiting) = std::sync::mpsc::channel();
        custody.retain_thread(std::thread::spawn(move || {
            let _ = waiting.recv_timeout(Duration::from_secs(10));
        }));
        let result = OwnedCpuMountCommand::prepare(
            &config,
            &witness.request,
            image,
            witness.image,
            custody,
            progress,
            Instant::now(),
        );
        assert!(result.is_err());
        assert!(library_directory.exists());
        assert!(capture_pool.reserved_storage() > 0);
        assert!(worker_pool.reserve(bytes).is_err());
        assert!(group_path.exists());
        release.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while capture_pool.reserved_storage() != 0
            || worker_pool.reserve(bytes).is_err()
            || group_path.exists()
            || library_directory.exists()
        {
            assert!(
                Instant::now() < end,
                "failed CPU mount setup custody did not retire"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
