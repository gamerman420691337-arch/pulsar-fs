//! Request-bound GPU mount assembly. Callers still own authorization and retirement.
#![cfg(target_os = "linux")]
use crate::{
    cuda_devices::{DeviceNodeIdentity, PinnedCudaDevices},
    native_dependencies::CapturedNativeDependencies,
    native_mounts::{MountedNativeAlias, PinnedNativeMounts},
    EngineConfig,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::io::Read;
use std::{
    fs::File,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileExt, MetadataExt},
            process::CommandExt,
        },
    },
    process::Command,
    time::{Duration, Instant},
};

use crate::native_mount_bytes as bytes;
#[path = "live_cuda_mounts.rs"]
mod live;
pub use live::run_live_checker;
pub(crate) use live::{
    verify_live_bootstrap, LiveCudaMountWitness, OwnedLiveCudaMountCommand,
    VerifiedLiveCudaBootstrap,
};

pub(crate) struct CudaMounts<'a> {
    pub(crate) image: PinnedNativeMounts<'a>,
    pub(crate) devices: &'a PinnedCudaDevices,
    modules: Vec<(File, MountedCudaModule)>,
}
const MODULE_PATHS: [&str; 2] = [
    "/sys/module/nvidia/initstate",
    "/sys/module/nvidia_uvm/initstate",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedCudaModule {
    pub guest_path: String,
    pub filesystem_device: u64,
    pub inode: u64,
}
fn module_ready(file: &File) -> Result<()> {
    ensure!(
        file.metadata()?.is_file(),
        "CUDA module state must be regular"
    );
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } == 0
            && filesystem.f_type == libc::SYSFS_MAGIC,
        "CUDA module state must come from sysfs"
    );
    let mut bytes = [0u8; 17];
    let count = file.read_at(&mut bytes, 0)?;
    ensure!(
        &bytes[..count] == b"live\n",
        "CUDA kernel module is not live"
    );
    Ok(())
}
impl CudaMounts<'_> {
    pub(crate) fn append_module_bindings(&self, command: &mut Command) -> Result<()> {
        let mut descriptors = Vec::new();
        for (file, witness) in &self.modules {
            module_ready(file)?;
            let actual = file.metadata()?;
            ensure!(
                actual.dev() == witness.filesystem_device && actual.ino() == witness.inode,
                "held CUDA module state changed"
            );
            let fd = file.as_raw_fd();
            ensure!(
                (3..1024).contains(&fd),
                "CUDA module descriptor outside child bound"
            );
            descriptors.push(fd);
            command
                .arg("--ro-bind")
                .arg(format!("/proc/self/fd/{fd}"))
                .arg(&witness.guest_path);
        }
        unsafe {
            command.pre_exec(move || {
                for fd in &descriptors {
                    let flags = libc::fcntl(*fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaMountWitness {
    pub request: WorkerRequest,
    pub request_sha256: String,
    pub aliases: Vec<MountedNativeAlias>,
    pub devices: Vec<DeviceNodeIdentity>,
    pub module_states: Vec<MountedCudaModule>,
    /// Optional finite-inference cap; checker and memory roles keep legacy framing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_deadline_ns: Option<u64>,
}
pub(crate) fn request_hash(request: &WorkerRequest) -> Result<String> {
    let bytes = serde_json::to_vec(request)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "CUDA mount request exceeds control bound"
    );
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn correspondence(witness: &CudaMountWitness) -> Result<()> {
    witness.request.validate()?;
    if let Some(end) = witness.execution_deadline_ns {
        crate::cuda_bootstrap::execution_deadline_instant(
            end,
            witness.request.budget.wall_time_ms,
        )?;
    }
    ensure!(
        witness.request_sha256 == request_hash(&witness.request)?,
        "CUDA mount request identity differs"
    );
    let execution = witness
        .request
        .cuda_execution
        .as_ref()
        .context("CUDA mount declaration missing")?;
    mount_correspondence(
        execution,
        &witness.aliases,
        &witness.devices,
        &witness.module_states,
    )
}
fn mount_correspondence(
    execution: &WorkerCudaExecution,
    aliases: &[MountedNativeAlias],
    devices: &[DeviceNodeIdentity],
    module_states: &[MountedCudaModule],
) -> Result<()> {
    let expected = execution
        .libraries
        .iter()
        .flat_map(|lib| {
            lib.names.iter().map(move |name| {
                (
                    name.as_str(),
                    lib.identity.sha256.as_str(),
                    lib.identity.byte_len,
                )
            })
        })
        .collect::<std::collections::BTreeSet<_>>();
    let actual = aliases
        .iter()
        .map(|a| (a.name.as_str(), a.sha256.as_str(), a.byte_len))
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        expected == actual && actual.len() == aliases.len(),
        "CUDA mounted aliases differ from request"
    );
    ensure!(
        devices.len() == 3 && devices[1].minor == execution.device_minor,
        "CUDA mounted device minor differs from request"
    );
    ensure!(
        module_states.len() == 2
            && module_states
                .iter()
                .zip(MODULE_PATHS)
                .all(|(state, path)| state.guest_path == path),
        "invalid CUDA module state table"
    );
    Ok(())
}
/// Holds image descriptors, borrowed device/capture custody and a private cgroup.
/// This does not own a GPU/RAM reservation or validate owner admission. The caller
/// must retain it until the complete child tree is confirmed retired; dropping a
/// Command or this assembly is not retirement evidence.
pub struct CudaMountCommand<'a> {
    command: Command,
    group: crate::cgroup::AttemptCgroup,
    _mounts: CudaMounts<'a>,
    witness: CudaMountWitness,
}
impl<'a> CudaMountCommand<'a> {
    /// A read/hash-only checker uses the same mount builder intended for launch.
    /// No device IO, native library loading or inference is performed by its role.
    pub fn inspection(
        config: &EngineConfig,
        request: &WorkerRequest,
        capture: &'a CapturedNativeDependencies,
        devices: &'a PinnedCudaDevices,
    ) -> Result<Self> {
        Self::prepare(config, request, capture, devices, "cuda-mount-check", &[])
    }
    /// Prototype worker assembly; this is not engine job admission. The caller
    /// owns input snapshots, reservations, independent deadlines and retirement.
    pub fn worker(
        config: &EngineConfig,
        request: &WorkerRequest,
        capture: &'a CapturedNativeDependencies,
        devices: &'a PinnedCudaDevices,
    ) -> Result<Self> {
        capture.verify_loader_ready()?;
        let mut bindings = vec![(request.source.path.clone(), request.source.path.clone())];
        if let Some(audio) = &request.independent_audio {
            bindings.push((audio.path.clone(), audio.path.clone()));
        }
        let model = match &request.operation {
            WorkerOperation::Generate { model, .. } | WorkerOperation::Preview { model, .. } => {
                model.as_ref()
            }
            _ => None,
        }
        .context("CUDA worker model missing")?;
        bindings.push((model.path.clone(), model.path.clone()));
        for tool in [&request.tools.ffmpeg, &request.tools.ffprobe] {
            bindings.push((tool.path.clone(), tool.path.clone()));
        }
        Self::prepare(config, request, capture, devices, "cuda-worker", &bindings)
    }
    fn prepare(
        config: &EngineConfig,
        request: &WorkerRequest,
        capture: &'a CapturedNativeDependencies,
        devices: &'a PinnedCudaDevices,
        role: &'static str,
        bindings: &[(std::path::PathBuf, std::path::PathBuf)],
    ) -> Result<Self> {
        let (mounts, witness, address_space) = mount_plan(request, capture, devices)?;
        let group = crate::cgroup::AttemptCgroup::create(
            request.attempt_id.as_str(),
            request.budget.memory_bytes,
            request.budget.cpu_threads,
        )?;
        let command = crate::confinement::cuda_command(
            config,
            crate::confinement::ConfinementLimits {
                budget: &request.budget,
                address_space_bytes: address_space,
            },
            &group,
            bindings,
            &request.output_dir,
            role,
            &mounts,
        )?;
        Ok(Self {
            command,
            group,
            _mounts: mounts,
            witness,
        })
    }
    pub fn command_mut(&mut self) -> &mut Command {
        &mut self.command
    }
    pub fn witness(&self) -> &CudaMountWitness {
        &self.witness
    }
    pub fn kill_tree(&self) {
        self.group.kill();
    }
    pub fn tree_is_empty(&self) -> std::io::Result<bool> {
        self.group.is_empty()
    }
}

fn mount_plan<'a>(
    request: &WorkerRequest,
    capture: &'a CapturedNativeDependencies,
    devices: &'a PinnedCudaDevices,
) -> Result<(CudaMounts<'a>, CudaMountWitness, u64)> {
    request.validate()?;
    let execution = request
        .cuda_execution
        .as_ref()
        .context("CUDA mount declaration missing")?;
    let mounts = pin_mounts(execution, capture, devices)?;
    let witness = CudaMountWitness {
        request: request.clone(),
        request_sha256: request_hash(request)?,
        aliases: mounts.image.witnesses(),
        devices: devices.identities().into_iter().cloned().collect(),
        module_states: mounts.modules.iter().map(|(_, w)| w.clone()).collect(),
        execution_deadline_ns: None,
    };
    correspondence(&witness)?;
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        _ => None,
    }
    .context("CUDA mount profile missing")?;
    crate::inference_profiles::admit_tensor_adapter(profile, &request.budget)?;
    let address_space = profile
        .cuda_resources
        .as_ref()
        .context("CUDA resources missing")?
        .address_space_bytes;
    Ok((mounts, witness, address_space))
}

fn pin_mounts<'a>(
    execution: &WorkerCudaExecution,
    capture: &'a CapturedNativeDependencies,
    devices: &'a PinnedCudaDevices,
) -> Result<CudaMounts<'a>> {
    ensure!(
        execution.library_directory == capture.library_directory(),
        "CUDA mount capture path differs"
    );
    let device = devices.expectation();
    ensure!(
        execution.device_uuid == device.device_uuid
            && execution.pci_bus_address == device.pci_bus_address
            && execution.device_minor == device.device_minor,
        "CUDA held device differs from request"
    );
    let image = PinnedNativeMounts::new(capture)?;
    let mut modules = Vec::new();
    for path in MODULE_PATHS {
        let file = crate::artifacts::open_regular(std::path::Path::new(path))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.uid() == 0,
            "CUDA module state must be kernel-owned"
        );
        module_ready(&file)?;
        modules.push((
            file,
            MountedCudaModule {
                guest_path: path.into(),
                filesystem_device: metadata.dev(),
                inode: metadata.ino(),
            },
        ));
    }
    Ok(CudaMounts {
        image,
        devices,
        modules,
    })
}

/// Descriptor/image ownership for an admitted group. Construction installs all
/// owners before fallible mount/command setup. No new cgroup is created here.
struct OwnedCudaAssets {
    // Close descriptor pins before image cleanup can release its storage claim.
    descriptors: Vec<File>,
    devices: PinnedCudaDevices,
    image: crate::native_capture::SupervisedNativeDependencies,
}
pub(crate) struct OwnedCudaMountCommand {
    command: Command,
    pub(crate) custody: crate::worker_retirement::WorkerCustody,
    pub(crate) witness: CudaMountWitness,
}
impl OwnedCudaMountCommand {
    /// Read/hash-only launch through existing admitted custody. This does not
    /// authorize inference or provide a fresh GPU memory observation.
    #[cfg(test)]
    pub(crate) fn inspection(
        config: &EngineConfig,
        request: &WorkerRequest,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
    ) -> Result<Self> {
        Self::prepare(config, request, image, devices, custody, "cuda-mount-check")
    }
    pub(crate) fn memory_observer(
        config: &EngineConfig,
        request: &WorkerRequest,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
    ) -> Result<Self> {
        Self::prepare(
            config,
            request,
            image,
            devices,
            custody,
            "cuda-memory-observer",
        )
    }
    pub(crate) fn inference(
        config: &EngineConfig,
        request: &WorkerRequest,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
    ) -> Result<Self> {
        Self::prepare(config, request, image, devices, custody, "cuda-worker")
    }
    pub(crate) fn into_parts(
        self,
    ) -> (
        Command,
        crate::worker_retirement::WorkerCustody,
        CudaMountWitness,
    ) {
        (self.command, self.custody, self.witness)
    }
    fn prepare(
        config: &EngineConfig,
        request: &WorkerRequest,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        mut custody: crate::worker_retirement::WorkerCustody,
        role: &'static str,
    ) -> Result<Self> {
        let group = custody.group_handle();
        let assets = custody.retain_asset(OwnedCudaAssets {
            image,
            devices,
            descriptors: Vec::new(),
        });
        if matches!(role, "cuda-memory-observer" | "cuda-worker") {
            assets.image.captured().verify_loader_ready()?;
        }
        let (mounts, witness, address_space) =
            mount_plan(request, assets.image.captured(), &assets.devices)?;
        let mut bindings = Vec::new();
        if role == "cuda-worker" {
            let model = match &request.operation {
                WorkerOperation::Generate { model, .. }
                | WorkerOperation::Preview { model, .. } => model.as_ref(),
                _ => None,
            }
            .context("CUDA worker model missing")?;
            for path in std::iter::once(&request.source.path)
                .chain(std::iter::once(&model.path))
                .chain(request.independent_audio.iter().map(|audio| &audio.path))
                .chain([&request.tools.ffmpeg.path, &request.tools.ffprobe.path])
            {
                ensure!(
                    path.is_absolute() && path.canonicalize()? == *path,
                    "CUDA worker input path must be canonical"
                );
                bindings.push((path.clone(), path.clone()));
            }
        }
        let command = crate::confinement::cuda_command(
            config,
            crate::confinement::ConfinementLimits {
                budget: &request.budget,
                address_space_bytes: address_space,
            },
            &group,
            &bindings,
            &request.output_dir,
            role,
            &mounts,
        )?;
        assets.descriptors = mounts.image.into_descriptors();
        assets
            .descriptors
            .extend(mounts.modules.into_iter().map(|(file, _)| file));
        Ok(Self {
            command,
            custody,
            witness,
        })
    }
    pub(crate) fn retire_with_image(
        self,
        deadline: Instant,
    ) -> std::io::Result<(
        crate::native_capture::SupervisedNativeDependencies,
        PinnedCudaDevices,
    )> {
        let assets = self
            .custody
            .retire_into_asset::<OwnedCudaAssets>(deadline)?;
        drop(assets.descriptors);
        Ok((assets.image, assets.devices))
    }
    pub(crate) fn spawn(&mut self) -> std::io::Result<()> {
        self.custody.spawn(&mut self.command)
    }
}

#[cfg(test)]
#[path = "owned_cuda_mount_tests.rs"]
mod owned_tests;

/// Dedicated confinement checker. It does not authorize subsequent execution.
pub fn run_checker() -> Result<()> {
    clear_inherited_descriptors()?;
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "CUDA mount checker requires parent confinement"
    );
    let witness: CudaMountWitness = read_message(&mut std::io::stdin().lock())?;
    let deadline = Instant::now()
        .checked_add(Duration::from_millis(witness.request.budget.wall_time_ms))
        .context("mount checker deadline overflow")?;
    verify_witness(&witness, deadline)?;
    println!(
        "{}",
        serde_json::json!({"state":"passed","request_sha256":witness.request_sha256,
        "aliases":witness.aliases.len(),"device_nodes":witness.devices.len(),"native_execution":false,"gpu_admission":false,
        "inherited_descriptors_erased":true})
    );
    Ok(())
}

/// Called only in a fresh dedicated child role, before reading its request or
/// opening inputs. Inherited O_PATH descriptors must not bypass read-only mounts.
pub(crate) fn clear_inherited_descriptors() -> Result<()> {
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1"),
        "CUDA bootstrap requires parent confinement"
    );
    ensure!(
        unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0,
        "CUDA bootstrap cannot erase inherited descriptors: {}",
        std::io::Error::last_os_error()
    );
    for fd in 3..1024 {
        ensure!(
            unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EBADF),
            "CUDA bootstrap inherited descriptor remains open"
        );
    }
    Ok(())
}

/// A private proof of this exact bootstrap check; it cannot authorize scheduling.
pub(crate) struct VerifiedCudaBootstrap<'a> {
    witness: &'a CudaMountWitness,
    deadline: Instant,
}
impl VerifiedCudaBootstrap<'_> {
    pub(crate) fn request(&self) -> &WorkerRequest {
        &self.witness.request
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn verify_after(&self) -> Result<()> {
        verify_witness(self.witness, self.deadline)
    }
}
fn inherited_deadline(witness: &CudaMountWitness, deadline: Instant) -> Result<Instant> {
    match witness.execution_deadline_ns {
        Some(end) => Ok(
            deadline.min(crate::cuda_bootstrap::execution_deadline_instant(
                end,
                witness.request.budget.wall_time_ms,
            )?),
        ),
        None => Ok(deadline),
    }
}

pub(crate) fn verify_worker_bootstrap_at(
    witness: &CudaMountWitness,
    deadline: Instant,
) -> Result<VerifiedCudaBootstrap<'_>> {
    let deadline = inherited_deadline(witness, deadline)?;
    crate::inference_profiles::admit_cuda_worker_profile(&witness.request)?;
    verify_witness(witness, deadline)?;
    Ok(VerifiedCudaBootstrap { witness, deadline })
}
pub(crate) fn verify_memory_bootstrap_at(
    witness: &CudaMountWitness,
    deadline: Instant,
) -> Result<VerifiedCudaBootstrap<'_>> {
    let deadline = inherited_deadline(witness, deadline)?;
    ensure!(
        witness
            .request
            .cuda_execution
            .as_ref()
            .is_some_and(|execution| execution
                .libraries
                .iter()
                .any(|library| library.names.iter().any(|name| name == "libnvidia-ml.so.1"))),
        "CUDA memory observation requires captured NVML"
    );
    verify_witness(witness, deadline)?;
    Ok(VerifiedCudaBootstrap { witness, deadline })
}
fn verify_witness(witness: &CudaMountWitness, deadline: Instant) -> Result<()> {
    let deadline = inherited_deadline(witness, deadline)?;
    correspondence(witness)?;
    verify_mount_bytes(
        witness.request.cuda_execution.as_ref().unwrap(),
        &witness.aliases,
        &witness.devices,
        &witness.module_states,
        deadline,
    )
}
fn verify_mount_bytes(
    execution: &WorkerCudaExecution,
    aliases: &[MountedNativeAlias],
    devices: &[DeviceNodeIdentity],
    module_states: &[MountedCudaModule],
    deadline: Instant,
) -> Result<()> {
    for module in module_states {
        let file = crate::artifacts::open_regular(std::path::Path::new(&module.guest_path))?;
        let actual = file.metadata()?;
        ensure!(
            actual.dev() == module.filesystem_device && actual.ino() == module.inode,
            "mounted CUDA module state differs"
        );
        module_ready(&file)?;
    }
    crate::cuda_devices::verify_mounted_nodes(devices)?;
    bytes::verify(&execution.library_directory, aliases, deadline)?;
    crate::cuda_devices::verify_mounted_nodes(devices)?;
    Ok(())
}

#[cfg(test)]
pub(crate) use live::tests::witness as live_test_witness;

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn witness() -> CudaMountWitness {
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let execution = request.cuda_execution.as_ref().unwrap();
        let aliases = execution
            .libraries
            .iter()
            .flat_map(|lib| {
                lib.names.iter().map(move |name| MountedNativeAlias {
                    name: name.clone(),
                    sha256: lib.identity.sha256.clone(),
                    byte_len: lib.identity.byte_len,
                    filesystem_device: 1,
                    inode: 1,
                })
            })
            .collect();
        let devices = vec![
            DeviceNodeIdentity {
                guest_path: "/dev/nvidiactl".into(),
                major: 195,
                minor: 255,
                filesystem_device: 1,
                inode: 1,
            },
            DeviceNodeIdentity {
                guest_path: "/dev/nvidia0".into(),
                major: 195,
                minor: 0,
                filesystem_device: 1,
                inode: 2,
            },
            DeviceNodeIdentity {
                guest_path: "/dev/nvidia-uvm".into(),
                major: 235,
                minor: 0,
                filesystem_device: 1,
                inode: 3,
            },
        ];
        CudaMountWitness {
            request_sha256: request_hash(&request).unwrap(),
            execution_deadline_ns: None,
            request,
            aliases,
            devices,
            module_states: MODULE_PATHS
                .into_iter()
                .map(|path| MountedCudaModule {
                    guest_path: path.into(),
                    filesystem_device: 1,
                    inode: 1,
                })
                .collect(),
        }
    }
    #[test]
    fn optional_execution_cap_preserves_legacy_wire_and_bounds_verified_deadline() {
        let mut witness = witness();
        let legacy = serde_json::to_value(&witness).unwrap();
        assert!(legacy.get("execution_deadline_ns").is_none());
        let decoded: CudaMountWitness = serde_json::from_value(legacy).unwrap();
        assert!(decoded.execution_deadline_ns.is_none());
        let cap = Instant::now() + Duration::from_secs(1);
        witness.execution_deadline_ns = Some(
            crate::cuda_bootstrap::deadline_ns_in_execution(
                cap,
                witness.request.budget.wall_time_ms,
            )
            .unwrap(),
        );
        correspondence(&witness).unwrap();
        let bounded =
            inherited_deadline(&witness, Instant::now() + Duration::from_secs(60)).unwrap();
        assert!(bounded <= cap && bounded > Instant::now());
        let earlier = Instant::now() + Duration::from_millis(100);
        assert_eq!(inherited_deadline(&witness, earlier).unwrap(), earlier);
        witness.execution_deadline_ns = Some(0);
        assert!(correspondence(&witness).is_err());
        assert!(inherited_deadline(&witness, Instant::now() + Duration::from_secs(1)).is_err());
    }

    #[test]
    fn mount_correspondence_requires_exact_request_aliases_and_device_minor() {
        correspondence(&witness()).unwrap();
        let mut bad = witness();
        bad.request_sha256 = "0".repeat(64);
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.aliases[0].sha256 = "0".repeat(64);
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.aliases.pop();
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.aliases.push(bad.aliases[0].clone());
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.devices[1].minor = 1;
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.devices.clear();
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.request.attempt_id = AttemptId::new("other-attempt").unwrap();
        assert!(correspondence(&bad).is_err());
    }
    #[test]
    fn mount_witness_refuses_unknown_fields_without_changing_worker_contract() {
        let witness = witness();
        let original = serde_json::to_vec(&witness.request).unwrap();
        let mut encoded = serde_json::to_value(witness).unwrap();
        encoded["approved"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CudaMountWitness>(encoded).is_err());
        let decoded: WorkerRequest = serde_json::from_slice(&original).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), original);
    }
    #[test]
    fn cuda_profile_identity_checks_do_not_enable_ordinary_worker_admission() {
        let witness = witness();
        crate::inference_profiles::admit_cuda_worker_profile(&witness.request).unwrap();
        assert!(crate::inference_profiles::resolve_profile(&witness.request).is_err());
        let mut changed = witness.request.clone();
        changed.cuda_execution = None;
        assert!(crate::inference_profiles::admit_cuda_worker_profile(&changed).is_err());
        let mut changed = witness.request.clone();
        if let WorkerOperation::Generate { settings, .. } = &mut changed.operation {
            settings.inference_profile.as_mut().unwrap().model.sha256 = "0".repeat(64);
        }
        assert!(crate::inference_profiles::admit_cuda_worker_profile(&changed).is_err());
        let result = crate::worker::execute_manifest(&witness.request);
        assert!(result.result.is_err() && result.cuda_artifacts.is_none());
    }
}
