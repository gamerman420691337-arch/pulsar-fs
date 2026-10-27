//! Private owner startup policy. Expected bytes and accounting are not execution.
use crate::{
    cuda_devices::CudaDeviceExpectation,
    native_dependencies::NativeDependencySpec,
    resources::{GpuDeviceBudget, GpuPriority, ResourcePool},
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};

const MAX_POLICY_BYTES: usize = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaOwnerPolicy {
    pub schema_version: u16,
    pub device: CudaDeviceExpectation,
    pub accounting: GpuDeviceBudget,
    pub native_dependencies: NativeDependencySpec,
}
impl CudaOwnerPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported CUDA owner policy version"
        );
        self.device.validate()?;
        ensure!(
            self.device.device_uuid == self.accounting.device_uuid,
            "CUDA owner kernel and accounting UUID differ"
        );
        ResourcePool::new_with_gpu_devices(1, 1, vec![self.accounting.clone()])?;
        self.native_dependencies.validate()?;
        for library in &self.native_dependencies.libraries {
            ensure!(
                !library
                    .path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir)),
                "CUDA owner library path contains traversal"
            );
        }
        for role in [
            "ld-linux-x86-64.so.2",
            "libcuda.so.1",
            "libonnxruntime.so.1",
            "libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_shared.so",
            "libcudnn.so.9",
        ] {
            self.library(role)?;
        }
        Ok(())
    }
    fn library(&self, alias: &str) -> Result<&crate::native_dependencies::NativeLibrarySpec> {
        self.native_dependencies
            .libraries
            .iter()
            .find(|lib| lib.names.iter().any(|name| name == alias))
            .with_context(|| format!("CUDA owner policy lacks required alias {alias}"))
    }
    pub fn runtime_identity(&self) -> Result<ArtifactIdentity> {
        let runtime = self.library("libonnxruntime.so.1")?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(runtime.sha256.clone())?,
            sha256: runtime.sha256.clone(),
            byte_len: runtime.byte_len,
        })
    }
    pub fn match_runtime(&self, runtime: Option<&ArtifactReference>) -> Result<()> {
        ensure!(
            runtime.is_some_and(|r| self
                .runtime_identity()
                .is_ok_and(|identity| identity == r.identity)),
            "CUDA owner policy differs from the engine's pinned runtime"
        );
        Ok(())
    }
    /// Pure intent binding only. The aggregate pool, fresh observations, captured
    /// bytes, mounted devices and worker attempt must all be checked separately.
    pub fn authorize_profile(
        &self,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
    ) -> Result<()> {
        self.validate()?;
        profile.admit(budget)?;
        ensure!(
            profile.provider == InferenceProvider::Cuda,
            "CUDA owner policy cannot authorize another provider"
        );
        let limits = profile
            .cuda_resources
            .as_ref()
            .context("CUDA owner policy requires version-2 resource limits")?;
        ensure!(
            limits.device_uuid == self.accounting.device_uuid
                && limits.device_ordinal == self.accounting.device_ordinal,
            "CUDA profile device differs from owner policy"
        );
        ensure!(
            profile.runtime == self.runtime_identity()?,
            "CUDA profile runtime differs from owner policy"
        );
        let ceiling = match priority {
            GpuPriority::Interactive => self.accounting.maximum_reserved_bytes,
            GpuPriority::Offline => {
                self.accounting.maximum_reserved_bytes - self.accounting.interactive_reserved_bytes
            }
        };
        ensure!(
            limits.device_reservation_bytes <= ceiling,
            "CUDA intent exceeds owner priority ceiling"
        );
        Ok(())
    }
}

pub struct LoadedCudaOwnerPolicy {
    policy: CudaOwnerPolicy,
    identity: ArtifactIdentity,
}
#[derive(Clone, Debug, Serialize)]
pub struct CudaOwnerPolicyStatus {
    pub source_identity: ArtifactIdentity,
    pub device_uuid: String,
    pub pci_bus_address: String,
    pub device_minor: u32,
    pub accounting: GpuDeviceBudget,
    pub runtime: ArtifactIdentity,
    pub native_dependency_set_sha256: String,
    pub native_declared_bytes: u64,
    pub execution_enabled: bool,
    /// None for a loaded declaration; engine status fills actual shared usage.
    pub pool_usage: Option<crate::resources::GpuUsage>,
}
impl LoadedCudaOwnerPolicy {
    /// Read one bounded private current-owner regular file through a nofollow
    /// descriptor. Later pathname changes do not change this loaded policy.
    pub fn load(path: &Path) -> Result<Self> {
        ensure!(
            path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)),
            "CUDA owner policy path must be absolute without traversal"
        );
        let mut file = crate::artifacts::open_regular(path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o077 == 0
                && metadata.len() > 0
                && metadata.len() <= MAX_POLICY_BYTES as u64,
            "CUDA owner policy requires private current-owner storage within size limit"
        );
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_POLICY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == metadata.len() && file.metadata()?.len() == metadata.len(),
            "CUDA owner policy length changed during read"
        );
        let policy: CudaOwnerPolicy = serde_json::from_slice(&bytes)?;
        policy.validate()?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        Ok(Self {
            policy,
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone())?,
                sha256,
                byte_len: bytes.len() as u64,
            },
        })
    }
    pub fn policy(&self) -> &CudaOwnerPolicy {
        &self.policy
    }
    /// Build transport metadata only from this loaded policy and its verified
    /// private capture. The caller still owns reservations, mounted-device
    /// checks, durable attempt admission and independent worker supervision.
    pub fn worker_execution(
        &self,
        captured: &crate::native_dependencies::CapturedNativeDependencies,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
    ) -> Result<WorkerCudaExecution> {
        self.authorize_capture(captured, profile, budget, priority)?;
        captured.verify()?;
        self.execution_metadata(captured, profile, budget)
    }
    /// Build metadata from a completed supervised copy without rehashing image
    /// contents in the authority process. This borrows, but does not transfer,
    /// image/storage custody. It is not launch admission or byte immutability:
    /// the confined CUDA worker must freshly hash its mounted aliases.
    pub fn worker_execution_supervised(
        &self,
        owner: &crate::native_capture::SupervisedNativeDependencies,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
    ) -> Result<WorkerCudaExecution> {
        let captured = owner.captured();
        self.authorize_capture(captured, profile, budget, priority)?;
        ensure!(
            owner.completion().declared_bytes == captured.specification().validate()?,
            "CUDA supervised capture completion length differs"
        );
        captured.verify_metadata()?;
        self.execution_metadata(captured, profile, budget)
    }
    fn authorize_capture(
        &self,
        captured: &crate::native_dependencies::CapturedNativeDependencies,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        priority: GpuPriority,
    ) -> Result<()> {
        self.policy.authorize_profile(profile, budget, priority)?;
        ensure!(
            serde_json::to_vec(captured.specification())?
                == serde_json::to_vec(&self.policy.native_dependencies)?,
            "CUDA captured image differs from loaded owner policy"
        );
        Ok(())
    }
    fn execution_metadata(
        &self,
        captured: &crate::native_dependencies::CapturedNativeDependencies,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
    ) -> Result<WorkerCudaExecution> {
        let execution = WorkerCudaExecution {
            schema_version: 1,
            owner_policy: self.identity.clone(),
            device_uuid: self.policy.device.device_uuid.clone(),
            pci_bus_address: self.policy.device.pci_bus_address.clone(),
            device_minor: self.policy.device.device_minor,
            device_ordinal: self.policy.accounting.device_ordinal,
            library_directory: captured.library_directory(),
            libraries: captured
                .specification()
                .libraries
                .iter()
                .map(|library| {
                    Ok(CapturedNativeLibrary {
                        identity: ArtifactIdentity {
                            artifact_id: ArtifactId::new(library.sha256.clone())?,
                            sha256: library.sha256.clone(),
                            byte_len: library.byte_len,
                        },
                        names: library.names.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        };
        let mut dependencies = vec![execution.owner_policy.clone()];
        dependencies.extend(execution.libraries.iter().map(|lib| lib.identity.clone()));
        execution.validate(profile, budget, &dependencies)?;
        Ok(execution)
    }
    pub fn status(&self) -> CudaOwnerPolicyStatus {
        let policy = &self.policy;
        CudaOwnerPolicyStatus {
            source_identity: self.identity.clone(),
            device_uuid: policy.device.device_uuid.clone(),
            pci_bus_address: policy.device.pci_bus_address.clone(),
            device_minor: policy.device.device_minor,
            accounting: policy.accounting.clone(),
            runtime: policy.runtime_identity().expect("validated runtime alias"),
            native_dependency_set_sha256: format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&policy.native_dependencies).expect("serializable policy")
                )
            ),
            native_declared_bytes: policy
                .native_dependencies
                .validate()
                .expect("validated dependency set"),
            execution_enabled: false,
            pool_usage: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_dependencies::NativeLibrarySpec;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    pub(super) fn fixture() -> CudaOwnerPolicy {
        let uuid = "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".to_string();
        CudaOwnerPolicy {
            schema_version: 1,
            device: CudaDeviceExpectation {
                device_uuid: uuid.clone(),
                pci_bus_address: "0000:01:00.0".into(),
                device_minor: 0,
            },
            accounting: GpuDeviceBudget {
                device_uuid: uuid,
                device_ordinal: 0,
                maximum_reserved_bytes: 100,
                interactive_reserved_bytes: 40,
                control_reserve_bytes: 10,
            },
            native_dependencies: NativeDependencySpec {
                schema_version: 1,
                libraries: [
                    "ld-linux-x86-64.so.2",
                    "libcuda.so.1",
                    "libonnxruntime.so.1",
                    "libonnxruntime_providers_cuda.so",
                    "libonnxruntime_providers_shared.so",
                    "libcudnn.so.9",
                ]
                .iter()
                .enumerate()
                .map(|(i, name)| NativeLibrarySpec {
                    path: format!("/declared/{name}").into(),
                    sha256: format!("{i:064x}"),
                    byte_len: 1,
                    names: vec![name.to_string()],
                })
                .collect(),
            },
        }
    }
    fn profile(policy: &CudaOwnerPolicy) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 2,
            model: policy.runtime_identity().unwrap(),
            runtime: policy.runtime_identity().unwrap(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cuda,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: Some(CudaResourceLimits {
                device_uuid: policy.accounting.device_uuid.clone(),
                device_ordinal: 0,
                address_space_bytes: 1024,
                device_reservation_bytes: 60,
                arena_limit_bytes: 10,
            }),
        }
    }
    #[test]
    fn policy_roles_identity_and_priority_are_required() {
        let p = fixture();
        p.validate().unwrap();
        let f = profile(&p);
        let b = ResourceBudget {
            memory_bytes: 1,
            output_bytes: 1,
            wall_time_ms: 1,
            cpu_threads: 1,
        };
        p.authorize_profile(&f, &b, GpuPriority::Offline).unwrap();
        let mut bad = f.clone();
        bad.cuda_resources
            .as_mut()
            .unwrap()
            .device_reservation_bytes = 61;
        assert!(p.authorize_profile(&bad, &b, GpuPriority::Offline).is_err());
        p.authorize_profile(&bad, &b, GpuPriority::Interactive)
            .unwrap();
        let mut bad = f.clone();
        bad.runtime.sha256 = "f".repeat(64);
        assert!(p.authorize_profile(&bad, &b, GpuPriority::Offline).is_err());
        let mut bad = f;
        bad.cuda_resources.as_mut().unwrap().device_ordinal = 1;
        assert!(p.authorize_profile(&bad, &b, GpuPriority::Offline).is_err());
        let mut bad = p.clone();
        bad.native_dependencies.libraries.remove(1);
        assert!(bad.validate().is_err());
        let mut bad = p.clone();
        bad.accounting.device_uuid = "GPU-00000000-0000-0000-0000-000000000000".into();
        assert!(bad.validate().is_err());
        let mut bad = p;
        bad.native_dependencies.libraries[0].path = "/declared/../loader".into();
        assert!(bad.validate().is_err());
    }
    #[test]
    fn private_bounded_snapshot_survives_path_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        let bytes = serde_json::to_vec(&fixture()).unwrap();
        fs::write(&path, &bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let loaded = LoadedCudaOwnerPolicy::load(&path).unwrap();
        let first = loaded.status();
        assert!(!first.execution_enabled);
        assert_eq!(
            first.source_identity.sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        fs::rename(&path, dir.path().join("original.json")).unwrap();
        fs::write(&path, b"changed").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(LoadedCudaOwnerPolicy::load(&path).is_err());
        assert_eq!(loaded.status().source_identity, first.source_identity);
        let link = dir.path().join("link");
        symlink(dir.path().join("original.json"), &link).unwrap();
        assert!(LoadedCudaOwnerPolicy::load(&link).is_err());
        let original = dir.path().join("original.json");
        fs::set_permissions(&original, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(LoadedCudaOwnerPolicy::load(&original).is_err());
        fs::write(&path, vec![b' '; MAX_POLICY_BYTES + 1]).unwrap();
        assert!(LoadedCudaOwnerPolicy::load(&path).is_err());
    }
    #[test]
    fn worker_image_is_bound_to_loaded_policy_and_verified_capture() {
        let dir = tempfile::tempdir().unwrap();
        let mut policy = fixture();
        for (i, library) in policy.native_dependencies.libraries.iter_mut().enumerate() {
            let bytes = vec![i as u8; i + 1];
            library.path = dir.path().join(format!("library-{i}"));
            fs::write(&library.path, &bytes).unwrap();
            library.sha256 = format!("{:x}", Sha256::digest(&bytes));
            library.byte_len = bytes.len() as u64;
        }
        let path = dir.path().join("policy.json");
        fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let loaded = LoadedCudaOwnerPolicy::load(&path).unwrap();
        let captured = crate::native_dependencies::CapturedNativeDependencies::capture(
            &policy.native_dependencies,
            &dir.path().join("captures"),
        )
        .unwrap();
        let profile = profile(&policy);
        let budget = ResourceBudget {
            memory_bytes: 1,
            output_bytes: 1,
            wall_time_ms: 1,
            cpu_threads: 1,
        };
        let execution = loaded
            .worker_execution(&captured, &profile, &budget, GpuPriority::Offline)
            .unwrap();
        assert_eq!(execution.owner_policy, loaded.status().source_identity);
        assert_eq!(execution.library_directory, captured.library_directory());
        assert_eq!(execution.libraries.len(), 6);
        // Supplier pathname changes do not affect the held private capture.
        fs::write(
            &policy.native_dependencies.libraries[0].path,
            b"replacement",
        )
        .unwrap();
        loaded
            .worker_execution(&captured, &profile, &budget, GpuPriority::Offline)
            .unwrap();
        let mut foreign = policy.native_dependencies.clone();
        foreign.libraries[0].path = dir.path().join("foreign");
        fs::write(&foreign.libraries[0].path, [0u8]).unwrap();
        let foreign = crate::native_dependencies::CapturedNativeDependencies::capture(
            &foreign,
            &dir.path().join("captures"),
        )
        .unwrap();
        assert!(loaded
            .worker_execution(&foreign, &profile, &budget, GpuPriority::Offline)
            .is_err());
        let alias = captured.library_directory().join("libcuda.so.1");
        fs::set_permissions(&alias, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(alias, b"tampered").unwrap();
        assert!(loaded
            .worker_execution(&captured, &profile, &budget, GpuPriority::Offline)
            .is_err());
        foreign.close().unwrap();
        captured.close().unwrap();
    }
    #[test]
    #[ignore = "requires pinned native worker and writable delegated cgroups; run explicitly serially"]
    fn supervised_copy_builds_bound_metadata_without_parent_content_hashing() {
        use crate::{native_capture, resources::ResourcePool, EngineConfig};
        use std::sync::atomic::AtomicBool;
        let worker = std::env::var_os("PULSAR_BENCHMARK_WORKER").expect("pin actual native worker");
        let root = tempfile::tempdir().unwrap();
        let mut policy = fixture();
        for (i, library) in policy.native_dependencies.libraries.iter_mut().enumerate() {
            let bytes = vec![i as u8; i + 1];
            library.path = root.path().join(format!("supplier-{i}"));
            fs::write(&library.path, &bytes).unwrap();
            library.sha256 = format!("{:x}", Sha256::digest(&bytes));
            library.byte_len = bytes.len() as u64;
        }
        let path = root.path().join("policy.json");
        fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let loaded = LoadedCudaOwnerPolicy::load(&path).unwrap();
        let pool = ResourcePool::new(1024 * 1024 * 1024, 1);
        let config = EngineConfig::new(root.path().join("unused-state"), worker.into());
        let capture_budget = ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: policy.native_dependencies.validate().unwrap() + 16 * 1024 * 1024,
            wall_time_ms: 10_000,
            cpu_threads: 1,
        };
        let owner = native_capture::capture(
            &config,
            &pool,
            &policy.native_dependencies,
            &root.path().join("captures"),
            capture_budget,
            1536 * 1024 * 1024,
            &AtomicBool::new(false),
        )
        .unwrap();
        let profile = profile(&policy);
        let budget = ResourceBudget {
            memory_bytes: 1,
            output_bytes: 1,
            wall_time_ms: 1,
            cpu_threads: 1,
        };
        let build = |profile: &DeclaredInferenceProfile| {
            loaded.worker_execution_supervised(&owner, profile, &budget, GpuPriority::Offline)
        };
        let execution = build(&profile).unwrap();
        let previous = loaded
            .worker_execution(owner.captured(), &profile, &budget, GpuPriority::Offline)
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&execution).unwrap(),
            serde_json::to_vec(&previous).unwrap()
        );
        let held = pool.reserved_storage();
        assert!(held > 0);
        let mut bad = profile.clone();
        bad.cuda_resources
            .as_mut()
            .unwrap()
            .device_reservation_bytes = 61;
        assert!(build(&bad).is_err());
        bad = profile.clone();
        bad.runtime.sha256 = "f".repeat(64);
        assert!(build(&bad).is_err());
        let mut foreign_policy = policy.clone();
        foreign_policy.native_dependencies.libraries[0].path = root.path().join("foreign");
        fs::write(&path, serde_json::to_vec(&foreign_policy).unwrap()).unwrap();
        let foreign = LoadedCudaOwnerPolicy::load(&path).unwrap();
        assert!(foreign
            .worker_execution_supervised(&owner, &profile, &budget, GpuPriority::Offline)
            .is_err());
        fs::write(
            &policy.native_dependencies.libraries[0].path,
            b"changed supplier",
        )
        .unwrap();
        build(&profile).unwrap();
        // Same-size in-place edits with restored readonly metadata cannot be
        // detected by this builder. Fresh mounted-byte verification is required.
        let alias = owner.captured().library_directory().join("libcuda.so.1");
        fs::set_permissions(&alias, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&alias, [9u8, 9]).unwrap();
        assert!(build(&profile).is_err(), "writable image accepted");
        fs::set_permissions(&alias, fs::Permissions::from_mode(0o400)).unwrap();
        build(&profile).unwrap();
        assert!(
            owner.captured().verify().is_err(),
            "content tamper missed by independent hash"
        );
        fs::remove_file(&alias).unwrap();
        assert!(build(&profile).is_err(), "missing alias accepted");
        assert_eq!(
            pool.reserved_storage(),
            held,
            "metadata builder released image storage"
        );
        owner.close().unwrap();
        assert_eq!(pool.reserved_storage(), 0);
        eprintln!("supervised CUDA metadata: real confined copy, policy/profile/priority binding, retained storage, metadata refusals, and fresh-byte-verification boundary passed; no GPU launch");
    }
    #[test]
    fn invalid_owner_policy_refuses_before_state_creation() {
        let dir = tempfile::tempdir().unwrap();
        let policy = dir.path().join("bad.json");
        fs::write(&policy, b"{}").unwrap();
        fs::set_permissions(&policy, fs::Permissions::from_mode(0o600)).unwrap();
        let state = dir.path().join("new-state");
        let mut config = crate::EngineConfig::new(state.clone(), "/absent-worker".into());
        config.cuda_owner_policy = Some(policy);
        assert!(crate::Engine::open(config).is_err());
        assert!(!state.exists());
    }
}
