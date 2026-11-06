//! Private startup selection for a captured CPU library image.
//! Policy intent does not prove execution, closure completeness or qualification.
use crate::native_dependencies::CapturedNativeDependencies;
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuOwnerPolicy {
    pub schema_version: u16,
    pub native_dependencies: NativeDependencySpec,
}

impl CpuOwnerPolicy {
    pub fn validate(&self) -> Result<()> {
        supported_platform()?;
        ensure!(
            self.schema_version == 1,
            "unsupported CPU owner policy version"
        );
        self.native_dependencies.validate()?;
        for library in &self.native_dependencies.libraries {
            ensure!(
                !library
                    .path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir)),
                "CPU owner library path contains traversal"
            );
        }
        for alias in CPU_RUNTIME_REQUIRED_ALIASES {
            self.library(alias)?;
        }
        Ok(())
    }

    fn library(&self, alias: &str) -> Result<&NativeLibrarySpec> {
        self.native_dependencies
            .libraries
            .iter()
            .find(|library| library.names.iter().any(|name| name == alias))
            .with_context(|| format!("CPU owner policy lacks required alias {alias}"))
    }

    pub fn runtime_identity(&self) -> Result<ArtifactIdentity> {
        self.validate()?;
        library_identity(self.library("libonnxruntime.so.1")?)
    }

    /// Bind only the declared provider and runtime. Model, preprocessing,
    /// reservation and execution admission remain their existing owners' work.
    pub fn match_profile(&self, profile: &DeclaredInferenceProfile) -> Result<()> {
        self.validate()?;
        profile.validate()?;
        ensure!(
            profile.provider == InferenceProvider::Cpu,
            "CPU owner policy requires the CPU provider"
        );
        ensure!(
            profile.runtime == self.runtime_identity()?,
            "CPU profile runtime differs from owner policy"
        );
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct LoadedCpuOwnerPolicy {
    policy: CpuOwnerPolicy,
    identity: ArtifactIdentity,
}

impl LoadedCpuOwnerPolicy {
    /// Read a bounded private regular file once through a nofollow descriptor.
    /// Later changes to its pathname do not change this in-memory selection.
    pub fn load(path: &Path) -> Result<Self> {
        supported_platform()?;
        ensure!(
            path.is_absolute()
                && !path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir)),
            "CPU owner policy path must be absolute without traversal"
        );
        #[cfg(target_os = "linux")]
        {
            use std::{io::Read, os::unix::fs::MetadataExt};
            let mut file = crate::artifacts::open_regular(path)?;
            let before = file.metadata()?;
            let current_owner = unsafe { libc::geteuid() };
            ensure!(
                private_current_owner(before.uid(), before.mode(), before.len(), current_owner),
                "CPU owner policy requires a private current-owner file within the size limit"
            );
            let mut bytes = Vec::with_capacity(before.len() as usize);
            file.by_ref()
                .take(MAX_CPU_OWNER_POLICY_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            let after = file.metadata()?;
            ensure!(
                bytes.len() as u64 == before.len()
                    && private_current_owner(after.uid(), after.mode(), after.len(), current_owner)
                    && (
                        before.dev(),
                        before.ino(),
                        before.len(),
                        before.mode(),
                        before.uid()
                    ) == (
                        after.dev(),
                        after.ino(),
                        after.len(),
                        after.mode(),
                        after.uid()
                    )
                    && (
                        before.mtime(),
                        before.mtime_nsec(),
                        before.ctime(),
                        before.ctime_nsec()
                    ) == (
                        after.mtime(),
                        after.mtime_nsec(),
                        after.ctime(),
                        after.ctime_nsec()
                    ),
                "CPU owner policy changed during read"
            );
            let policy: CpuOwnerPolicy =
                serde_json::from_slice(&bytes).context("invalid CPU owner policy JSON")?;
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
        #[cfg(not(target_os = "linux"))]
        {
            Err(ProtocolError::unsupported("captured CPU libraries require Linux x86_64").into())
        }
    }

    pub fn policy(&self) -> &CpuOwnerPolicy {
        &self.policy
    }

    pub fn identity(&self) -> &ArtifactIdentity {
        &self.identity
    }

    pub fn runtime_identity(&self) -> Result<ArtifactIdentity> {
        self.policy.runtime_identity()
    }

    /// The configured supplier coordinate, not a verified or captured handle.
    pub fn runtime_reference(&self) -> Result<ArtifactReference> {
        Ok(ArtifactReference {
            identity: self.runtime_identity()?,
            path: self.policy.library("libonnxruntime.so.1")?.path.clone(),
        })
    }

    pub fn image(&self) -> Result<CpuRuntimeImage> {
        self.policy.validate()?;
        image_for_spec(&self.identity, &self.policy.native_dependencies)
    }

    pub fn match_profile(&self, profile: &DeclaredInferenceProfile) -> Result<()> {
        self.policy.match_profile(profile)
    }

    /// Match capture declarations and metadata without rehashing libraries in
    /// the authority process. The capture child authenticates copied bytes;
    /// each confined worker must authenticate its mounted bytes again.
    pub fn captured_image(&self, captured: &CapturedNativeDependencies) -> Result<CpuRuntimeImage> {
        let expected = self.image()?;
        let actual = image_for_spec(&self.identity, captured.specification())?;
        ensure!(
            expected.identity()? == actual.identity()?,
            "CPU captured image differs from loaded owner policy"
        );
        captured.verify_metadata()?;
        Ok(actual)
    }

    #[cfg(target_os = "linux")]
    pub fn captured_image_supervised(
        &self,
        owner: &crate::native_capture::SupervisedNativeDependencies,
    ) -> Result<CpuRuntimeImage> {
        ensure!(
            owner.completion().declared_bytes == owner.captured().specification().validate()?,
            "CPU supervised capture completion length differs"
        );
        self.captured_image(owner.captured())
    }
}

fn supported_platform() -> Result<()> {
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        ProtocolError::unsupported("captured CPU libraries require Linux x86_64")
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn private_current_owner(owner: u32, mode: u32, length: u64, current_owner: u32) -> bool {
    owner == current_owner
        && mode & 0o7777 & !0o600 == 0
        && length > 0
        && length <= MAX_CPU_OWNER_POLICY_BYTES as u64
}

fn library_identity(library: &NativeLibrarySpec) -> Result<ArtifactIdentity> {
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(library.sha256.clone())?,
        sha256: library.sha256.clone(),
        byte_len: library.byte_len,
    })
}

fn image_for_spec(
    owner_policy: &ArtifactIdentity,
    spec: &NativeDependencySpec,
) -> Result<CpuRuntimeImage> {
    spec.validate()?;
    let mut libraries = spec
        .libraries
        .iter()
        .map(|library| {
            let mut names = library.names.clone();
            names.sort();
            Ok(CapturedNativeLibrary {
                identity: library_identity(library)?,
                names,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    libraries.sort_by(|left, right| left.identity.sha256.cmp(&right.identity.sha256));
    let image = CpuRuntimeImage {
        schema_version: 1,
        owner_policy: owner_policy.clone(),
        libraries,
    };
    image.validate()?;
    Ok(image)
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };

    fn fixture(root: &Path) -> CpuOwnerPolicy {
        CpuOwnerPolicy {
            schema_version: 1,
            native_dependencies: NativeDependencySpec {
                schema_version: 1,
                libraries: CPU_RUNTIME_REQUIRED_ALIASES
                    .iter()
                    .enumerate()
                    .map(|(index, alias)| {
                        let bytes = format!("authored library {index}");
                        let path = root.join(format!("library-{index}"));
                        fs::write(&path, &bytes).unwrap();
                        NativeLibrarySpec {
                            path,
                            sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
                            byte_len: bytes.len() as u64,
                            names: vec![(*alias).into()],
                        }
                    })
                    .collect(),
            },
        }
    }

    fn save(root: &Path, policy: &CpuOwnerPolicy) -> std::path::PathBuf {
        let path = root.join("cpu-policy.json");
        fs::write(&path, serde_json::to_vec(policy).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn profile(runtime: ArtifactIdentity) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: runtime.clone(),
            runtime,
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
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }

    #[test]
    fn loaded_identity_binds_exact_bytes_and_survives_path_replacement() {
        let root = tempfile::tempdir().unwrap();
        let policy = fixture(root.path());
        let path = save(root.path(), &policy);
        let bytes = fs::read(&path).unwrap();
        let loaded = LoadedCpuOwnerPolicy::load(&path).unwrap();
        assert_eq!(
            loaded.identity().sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        assert_eq!(loaded.identity().byte_len, bytes.len() as u64);
        assert_eq!(
            loaded.identity().artifact_id.as_str(),
            loaded.identity().sha256
        );
        assert_eq!(
            loaded.runtime_reference().unwrap().path,
            policy.library("libonnxruntime.so.1").unwrap().path
        );
        let before = loaded.image().unwrap().identity().unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
        let reloaded = LoadedCpuOwnerPolicy::load(&path).unwrap();
        assert_ne!(loaded.identity(), reloaded.identity());
        assert_ne!(before, reloaded.image().unwrap().identity().unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(loaded.image().unwrap().identity().unwrap(), before);
    }

    #[test]
    fn policy_private_owner_file_and_length_guards_are_strict() {
        assert!(private_current_owner(7, 0o100600, 1, 7));
        assert!(private_current_owner(
            7,
            0o100400,
            MAX_CPU_OWNER_POLICY_BYTES as u64,
            7
        ));
        for (uid, mode, bytes) in [
            (8, 0o600, 1),
            (7, 0o640, 1),
            (7, 0o604, 1),
            (7, 0o700, 1),
            (7, 0o4600, 1),
            (7, 0o600, 0),
            (7, 0o600, MAX_CPU_OWNER_POLICY_BYTES as u64 + 1),
        ] {
            assert!(!private_current_owner(uid, mode, bytes, 7));
        }
        let root = tempfile::tempdir().unwrap();
        let path = save(root.path(), &fixture(root.path()));
        let link = root.path().join("linked-policy.json");
        symlink(&path, &link).unwrap();
        assert!(LoadedCpuOwnerPolicy::load(&link).is_err());
        assert!(LoadedCpuOwnerPolicy::load(Path::new("relative-policy.json")).is_err());
        assert!(LoadedCpuOwnerPolicy::load(root.path()).is_err());
        for mode in [0o640, 0o700, 0o1600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(LoadedCpuOwnerPolicy::load(&path).is_err());
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, vec![b' '; MAX_CPU_OWNER_POLICY_BYTES + 1]).unwrap();
        assert!(LoadedCpuOwnerPolicy::load(&path).is_err());
    }

    #[test]
    fn strict_policy_schema_and_existing_dependency_limits_apply() {
        let root = tempfile::tempdir().unwrap();
        let policy = fixture(root.path());
        let path = save(root.path(), &policy);
        for payload in [
            b"{}".to_vec(),
            b"{\"schema_version\":1,\"schema_version\":1,\"native_dependencies\":{}}".to_vec(),
            {
                let mut value = serde_json::to_value(&policy).unwrap();
                value["qualified"] = true.into();
                serde_json::to_vec(&value).unwrap()
            },
        ] {
            fs::write(&path, payload).unwrap();
            assert!(LoadedCpuOwnerPolicy::load(&path).is_err());
        }
        for mutate in [
            |policy: &mut CpuOwnerPolicy| policy.schema_version = 2,
            |policy: &mut CpuOwnerPolicy| policy.native_dependencies.libraries[0].names.clear(),
            |policy: &mut CpuOwnerPolicy| {
                policy.native_dependencies.libraries[0].names = vec!["other.so".into()]
            },
            |policy: &mut CpuOwnerPolicy| {
                policy.native_dependencies.libraries[0].path = "/a/../b".into()
            },
            |policy: &mut CpuOwnerPolicy| {
                policy.native_dependencies.libraries[0].byte_len = MAX_NATIVE_LIBRARY_BYTES + 1
            },
        ] {
            let mut changed = policy.clone();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
        }
    }

    #[test]
    fn profile_binds_only_valid_cpu_and_exact_runtime_identity() {
        let root = tempfile::tempdir().unwrap();
        let loaded = LoadedCpuOwnerPolicy::load(&save(root.path(), &fixture(root.path()))).unwrap();
        let cpu = profile(loaded.runtime_identity().unwrap());
        loaded.match_profile(&cpu).unwrap();
        for mutate in [
            |profile: &mut DeclaredInferenceProfile| profile.provider = InferenceProvider::Cuda,
            |profile: &mut DeclaredInferenceProfile| profile.runtime.byte_len += 1,
            |profile: &mut DeclaredInferenceProfile| profile.runtime.sha256 = "a".repeat(64),
            |profile: &mut DeclaredInferenceProfile| {
                profile.runtime.artifact_id = ArtifactId::new("different").unwrap()
            },
            |profile: &mut DeclaredInferenceProfile| profile.cpu_threads = 0,
        ] {
            let mut changed = cpu.clone();
            mutate(&mut changed);
            assert!(loaded.match_profile(&changed).is_err());
        }
    }

    #[test]
    fn image_canonicalizes_order_and_matches_captured_bytes_not_supplier_coordinates() {
        let root = tempfile::tempdir().unwrap();
        let mut policy = fixture(root.path());
        policy.native_dependencies.libraries[0]
            .names
            .push("aaa-loader.so".into());
        let loaded = LoadedCpuOwnerPolicy::load(&save(root.path(), &policy)).unwrap();
        let image = loaded.image().unwrap();
        image.validate().unwrap();
        let mut reordered = policy.native_dependencies.clone();
        reordered.libraries.reverse();
        for library in &mut reordered.libraries {
            library.names.reverse();
        }
        assert_eq!(
            image.identity().unwrap(),
            image_for_spec(loaded.identity(), &reordered)
                .unwrap()
                .identity()
                .unwrap()
        );
        let mut relocated = reordered.clone();
        for (index, library) in relocated.libraries.iter_mut().enumerate() {
            library.path = root.path().join(format!("other-coordinate-{index}"));
        }
        assert_eq!(
            image.identity().unwrap(),
            image_for_spec(loaded.identity(), &relocated)
                .unwrap()
                .identity()
                .unwrap()
        );
        let captured =
            CapturedNativeDependencies::capture(&reordered, &root.path().join("captures")).unwrap();
        assert_eq!(
            loaded
                .captured_image(&captured)
                .unwrap()
                .identity()
                .unwrap(),
            image.identity().unwrap()
        );
        for mutate in [
            |policy: &mut CpuOwnerPolicy| {
                policy.native_dependencies.libraries[0]
                    .names
                    .push("unexpected.so".into())
            },
            |policy: &mut CpuOwnerPolicy| policy.native_dependencies.libraries[0].byte_len += 1,
            |policy: &mut CpuOwnerPolicy| {
                policy.native_dependencies.libraries[0].sha256 = "a".repeat(64)
            },
        ] {
            let mut changed_policy = policy.clone();
            mutate(&mut changed_policy);
            let changed = LoadedCpuOwnerPolicy::load(&save(root.path(), &changed_policy)).unwrap();
            assert!(changed.captured_image(&captured).is_err());
        }
        let alias = captured.library_directory().join("libc.so.6");
        fs::remove_file(alias).unwrap();
        assert!(loaded.captured_image(&captured).is_err());
        captured.close().unwrap();
    }
}
