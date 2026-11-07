//! Bounded platform and hardware observations for inference correspondence.
//! Identities describe exact observations; they do not qualify a model/provider,
//! authenticate measurements, or establish a complete runtime dependency closure.
use crate::{
    validate_cuda_device_identity, validate_sha256, ArtifactId, ArtifactIdentity,
    DeclaredInferenceProfile, InferenceEquivalenceEnvironment, InferenceProvider, ProtocolError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const INFERENCE_PLATFORM_OBSERVATION_LIMIT: usize = 4096;
pub const INFERENCE_HARDWARE_OBSERVATION_LIMIT: usize = 512 * 1024;
pub const INFERENCE_ENVIRONMENT_REPORT_LIMIT: usize = 768 * 1024;
const CPU_MODEL_LIMIT: usize = 64 * 1024;

fn check(condition: bool, message: &str) -> Result<(), ProtocolError> {
    if condition {
        Ok(())
    } else {
        Err(ProtocolError::invalid(message))
    }
}

fn text(value: &str, limit: usize) -> Result<(), ProtocolError> {
    check(
        !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control),
        "inference observation text is empty, oversized or contains controls",
    )
}

fn encode<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, ProtocolError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| ProtocolError::invalid("inference observation encoding failed"))?;
    check(
        bytes.len() <= limit,
        "inference observation canonical encoding exceeds its bound",
    )?;
    Ok(bytes)
}

fn identity(bytes: &[u8]) -> Result<ArtifactIdentity, ProtocolError> {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())
            .map_err(|e| ProtocolError::invalid(e.to_string()))?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferencePlatformObservation {
    pub schema_version: u16,
    pub os: String,
    pub architecture: String,
    pub kernel_release: String,
    pub kernel_version: String,
}

impl InferencePlatformObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported platform observation schema",
        )?;
        if self.os != "linux" || self.architecture != "x86_64" {
            return Err(ProtocolError::unsupported("inference observation platform"));
        }
        for (value, limit) in [
            (self.kernel_release.as_str(), 128),
            (self.kernel_version.as_str(), 512),
        ] {
            text(value, limit)?;
            check(
                !value.contains(['/', '\\']),
                "inference platform observation must not contain path separators",
            )?;
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        encode(self, INFERENCE_PLATFORM_OBSERVATION_LIMIT)
    }

    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        identity(&self.canonical_bytes()?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCpuModelObservation {
    pub vendor_id: String,
    pub family: u32,
    pub model: u32,
    pub model_name: String,
    pub stepping: u32,
    pub microcode: u64,
    pub flags: Vec<String>,
}

impl InferenceCpuModelObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        text(&self.vendor_id, 32)?;
        text(&self.model_name, 256)?;
        check(self.vendor_id.is_ascii(), "CPU vendor must be ASCII")?;
        check(
            self.family <= 65535 && self.model <= 65535 && self.stepping <= 65535,
            "CPU model scalar exceeds its bound",
        )?;
        check(
            !self.flags.is_empty() && self.flags.len() <= 512,
            "CPU feature count is empty or exceeds its bound",
        )?;
        for flag in &self.flags {
            check(
                !flag.is_empty()
                    && flag.len() <= 64
                    && flag
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                "CPU feature must be a bounded ASCII token",
            )?;
        }
        check(
            self.flags.windows(2).all(|pair| pair[0] < pair[1]),
            "CPU features must be uniquely sorted",
        )
    }

    /// The hardware model table is ordered by these exact serialized bytes.
    pub fn canonical_key(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        encode(self, CPU_MODEL_LIMIT)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceLogicalCpuObservation {
    pub processor_id: u32,
    pub package_id: u32,
    pub core_id: u32,
    pub model_index: u16,
}

impl InferenceLogicalCpuObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.processor_id <= 65535 && self.package_id <= 65535 && self.core_id <= 65535,
            "logical CPU identifier exceeds its bound",
        )?;
        check(
            self.model_index < 64,
            "logical CPU model index exceeds its bound",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCpuPackageObservation {
    pub package_id: u32,
    pub reported_logical_cpus: u32,
    pub reported_physical_cores: u32,
}

impl InferenceCpuPackageObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.package_id <= 65535
                && (1..=65536).contains(&self.reported_logical_cpus)
                && self.reported_physical_cores > 0
                && self.reported_physical_cores <= self.reported_logical_cpus,
            "reported CPU package topology is invalid",
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceNvidiaModuleFlavor {
    Open,
    Proprietary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceNvidiaDriverObservation {
    pub version: String,
    pub module_flavor: InferenceNvidiaModuleFlavor,
    /// Identity of the observed driver version document, without its raw text.
    pub build: ArtifactIdentity,
}

impl InferenceNvidiaDriverObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            !self.version.is_empty()
                && self.version.len() <= 64
                && self
                    .version
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
            "NVIDIA driver version must contain bounded numeric components",
        )?;
        self.build.validate()?;
        check(
            self.build.byte_len > 0
                && self.build.byte_len <= i64::MAX as u64
                && self.build.artifact_id.as_str() == self.build.sha256,
            "NVIDIA driver build must have a positive content-derived identity",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceGpuObservation {
    pub device_uuid: String,
    pub pci_bus_address: String,
    pub device_minor: u32,
    pub kernel_driver: InferenceNvidiaDriverObservation,
}

impl InferenceGpuObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        // Zero is used only to invoke the UUID syntax checker. A device minor
        // is not a CUDA ordinal, and this observation contains no ordinal map.
        validate_cuda_device_identity(0, &self.device_uuid)?;
        check(
            self.device_minor <= 63
                && self
                    .device_uuid
                    .bytes()
                    .skip(4)
                    .all(|b| !b.is_ascii_uppercase()),
            "GPU UUID or device minor is not canonical",
        )?;
        let pci = self.pci_bus_address.as_bytes();
        check(
            pci.len() == 12
                && pci.iter().enumerate().all(|(index, byte)| match index {
                    4 | 7 => *byte == b':',
                    10 => *byte == b'.',
                    _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
                }),
            "GPU PCI bus address is not canonical",
        )?;
        check(
            u8::from_str_radix(&self.pci_bus_address[8..10], 16).is_ok_and(|v| v <= 31)
                && u8::from_str_radix(&self.pci_bus_address[11..12], 16).is_ok_and(|v| v <= 7),
            "GPU PCI device or function exceeds its bound",
        )?;
        self.kernel_driver.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceHardwareObservation {
    pub schema_version: u16,
    pub architecture: String,
    pub cpu_models: Vec<InferenceCpuModelObservation>,
    pub logical_cpus: Vec<InferenceLogicalCpuObservation>,
    pub cpu_packages: Vec<InferenceCpuPackageObservation>,
    pub allowed_cpu_ids: Vec<u32>,
    pub physical_memory_bytes: u64,
    pub gpu: Option<InferenceGpuObservation>,
}

impl InferenceHardwareObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1,
            "unsupported hardware observation schema",
        )?;
        if self.architecture != "x86_64" {
            return Err(ProtocolError::unsupported(
                "inference hardware architecture",
            ));
        }
        check(
            !self.cpu_models.is_empty()
                && self.cpu_models.len() <= 64
                && !self.logical_cpus.is_empty()
                && self.logical_cpus.len() <= 4096
                && !self.cpu_packages.is_empty()
                && self.cpu_packages.len() <= 4096
                && !self.allowed_cpu_ids.is_empty()
                && self.allowed_cpu_ids.len() <= 4096,
            "inference hardware collection count is empty or exceeds its bound",
        )?;
        check(
            self.physical_memory_bytes > 0 && self.physical_memory_bytes <= 1u64 << 60,
            "physical memory observation exceeds its bound",
        )?;
        let mut previous_model = None;
        for model in &self.cpu_models {
            let key = model.canonical_key()?;
            check(
                previous_model
                    .as_ref()
                    .is_none_or(|previous| previous < &key),
                "CPU models must be uniquely ordered by canonical bytes",
            )?;
            previous_model = Some(key);
        }
        check(
            self.cpu_packages
                .windows(2)
                .all(|pair| pair[0].package_id < pair[1].package_id),
            "CPU packages must be uniquely sorted",
        )?;
        let mut packages = BTreeMap::new();
        for package in &self.cpu_packages {
            package.validate()?;
            packages.insert(package.package_id, (package, 0u32, BTreeSet::new()));
        }
        check(
            self.logical_cpus
                .windows(2)
                .all(|pair| pair[0].processor_id < pair[1].processor_id),
            "logical CPUs must be uniquely sorted",
        )?;
        let mut referenced_models = vec![false; self.cpu_models.len()];
        for cpu in &self.logical_cpus {
            cpu.validate()?;
            let model = referenced_models
                .get_mut(usize::from(cpu.model_index))
                .ok_or_else(|| ProtocolError::invalid("logical CPU references an absent model"))?;
            *model = true;
            let (_, observed_logical, observed_cores) =
                packages.get_mut(&cpu.package_id).ok_or_else(|| {
                    ProtocolError::invalid("logical CPU references an absent package")
                })?;
            *observed_logical += 1;
            observed_cores.insert(cpu.core_id);
        }
        check(
            referenced_models.iter().all(|value| *value),
            "CPU model table contains an unreferenced model",
        )?;
        for (package, observed_logical, observed_cores) in packages.values() {
            check(
                *observed_logical > 0
                    && *observed_logical <= package.reported_logical_cpus
                    && observed_cores.len() <= package.reported_physical_cores as usize,
                "observed CPU topology contradicts reported package counts",
            )?;
        }
        check(
            self.allowed_cpu_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "CPU affinity must be uniquely sorted",
        )?;
        for allowed in &self.allowed_cpu_ids {
            check(
                *allowed <= 65535
                    && self
                        .logical_cpus
                        .binary_search_by_key(allowed, |cpu| cpu.processor_id)
                        .is_ok(),
                "CPU affinity references an unobserved CPU",
            )?;
        }
        if let Some(gpu) = &self.gpu {
            gpu.validate()?;
        }
        encode(self, INFERENCE_HARDWARE_OBSERVATION_LIMIT)?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        encode(self, INFERENCE_HARDWARE_OBSERVATION_LIMIT)
    }

    pub fn identity(&self) -> Result<ArtifactIdentity, ProtocolError> {
        identity(&self.canonical_bytes()?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceEnvironmentReport {
    pub schema_version: u16,
    pub declared_profile: DeclaredInferenceProfile,
    pub profile_sha256: String,
    pub environment: InferenceEquivalenceEnvironment,
    pub platform: InferencePlatformObservation,
    pub hardware: InferenceHardwareObservation,
    pub observed_at_unix_ms: u64,
    pub runtime_dependency_closure_verified: bool,
    pub qualified: bool,
    pub report_only: bool,
}

impl InferenceEnvironmentReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        check(
            self.schema_version == 1
                && self.report_only
                && !self.qualified
                && !self.runtime_dependency_closure_verified,
            "inference environment report cannot claim qualification or dependency closure",
        )?;
        check(
            self.observed_at_unix_ms > 0 && self.observed_at_unix_ms <= i64::MAX as u64,
            "inference observation time must be positive and fit signed storage",
        )?;
        self.declared_profile.validate()?;
        validate_sha256(&self.profile_sha256)?;
        let profile = encode(&self.declared_profile, INFERENCE_ENVIRONMENT_REPORT_LIMIT)?;
        check(
            self.profile_sha256 == format!("{:x}", Sha256::digest(&profile)),
            "inference environment profile digest mismatch",
        )?;
        self.environment.validate()?;
        self.platform.validate()?;
        self.hardware.validate()?;
        check(
            self.environment.os == self.platform.os
                && self.environment.architecture == self.platform.architecture
                && self.environment.architecture == self.hardware.architecture,
            "inference environment platform correspondence differs",
        )?;
        check(
            self.environment.platform_build == self.platform.identity()?
                && self.environment.hardware == self.hardware.identity()?,
            "inference environment observation identities differ",
        )?;
        check(
            self.environment
                .dependencies
                .contains(&self.declared_profile.runtime),
            "inference environment does not contain the declared runtime identity",
        )?;
        match self.declared_profile.provider {
            InferenceProvider::Cpu => {}
            InferenceProvider::Cuda => {
                let cuda = self
                    .declared_profile
                    .cuda_resources
                    .as_ref()
                    .ok_or_else(|| {
                        ProtocolError::invalid(
                            "CUDA environment requires explicit resource declaration",
                        )
                    })?;
                let gpu = self.hardware.gpu.as_ref().ok_or_else(|| {
                    ProtocolError::invalid("CUDA environment requires an observed GPU")
                })?;
                check(
                    self.declared_profile.schema_version == 2
                        && cuda.device_uuid == gpu.device_uuid,
                    "CUDA environment UUID differs from the declared profile",
                )?;
            }
            _ => return Err(ProtocolError::unsupported("inference environment provider")),
        }
        encode(self, INFERENCE_ENVIRONMENT_REPORT_LIMIT)?;
        Ok(())
    }

    /// Request/response correspondence only; this does not grant execution.
    pub fn validate_for_profile(
        &self,
        expected: &DeclaredInferenceProfile,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        expected.validate()?;
        check(
            encode(&self.declared_profile, INFERENCE_ENVIRONMENT_REPORT_LIMIT)?
                == encode(expected, INFERENCE_ENVIRONMENT_REPORT_LIMIT)?,
            "inference environment response profile differs from the request",
        )
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        self.validate()?;
        encode(self, INFERENCE_ENVIRONMENT_REPORT_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CudaResourceLimits, DetectorDecoder, InferenceColor, InferencePreprocessing,
        InferenceResize, ModelInputContract, TensorLayout,
    };

    fn artifact(bytes: &[u8]) -> ArtifactIdentity {
        identity(bytes).unwrap()
    }

    fn platform() -> InferencePlatformObservation {
        InferencePlatformObservation {
            schema_version: 1,
            os: "linux".into(),
            architecture: "x86_64".into(),
            kernel_release: "6.12.0-fixture".into(),
            kernel_version: "#1 SMP test observation".into(),
        }
    }

    fn model() -> InferenceCpuModelObservation {
        InferenceCpuModelObservation {
            vendor_id: "GenuineIntel".into(),
            family: 6,
            model: 85,
            model_name: "Fixture CPU".into(),
            stepping: 4,
            microcode: 0x2000065,
            flags: vec!["avx".into(), "sse2".into()],
        }
    }

    fn hardware() -> InferenceHardwareObservation {
        InferenceHardwareObservation {
            schema_version: 1,
            architecture: "x86_64".into(),
            cpu_models: vec![model()],
            logical_cpus: vec![
                InferenceLogicalCpuObservation {
                    processor_id: 0,
                    package_id: 0,
                    core_id: 0,
                    model_index: 0,
                },
                InferenceLogicalCpuObservation {
                    processor_id: 1,
                    package_id: 0,
                    core_id: 1,
                    model_index: 0,
                },
            ],
            cpu_packages: vec![InferenceCpuPackageObservation {
                package_id: 0,
                reported_logical_cpus: 4,
                reported_physical_cores: 2,
            }],
            allowed_cpu_ids: vec![0, 1],
            physical_memory_bytes: 16 << 30,
            gpu: None,
        }
    }

    fn gpu() -> InferenceGpuObservation {
        InferenceGpuObservation {
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            pci_bus_address: "0000:01:00.0".into(),
            device_minor: 0,
            kernel_driver: InferenceNvidiaDriverObservation {
                version: "550.120".into(),
                module_flavor: InferenceNvidiaModuleFlavor::Open,
                build: artifact(b"synthetic driver observation"),
            },
        }
    }

    fn profile() -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: artifact(b"declared model, not installed"),
            runtime: artifact(b"observed fixture runtime"),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 320,
                    height: 180,
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
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }

    fn report() -> InferenceEnvironmentReport {
        let declared_profile = profile();
        let platform = platform();
        let hardware = hardware();
        InferenceEnvironmentReport {
            schema_version: 1,
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&declared_profile).unwrap())
            ),
            environment: InferenceEquivalenceEnvironment {
                engine: artifact(b"fixture engine"),
                worker: artifact(b"fixture worker"),
                os: "linux".into(),
                architecture: "x86_64".into(),
                platform_build: platform.identity().unwrap(),
                hardware: hardware.identity().unwrap(),
                dependencies: vec![declared_profile.runtime.clone()],
            },
            declared_profile,
            platform,
            hardware,
            observed_at_unix_ms: 1,
            runtime_dependency_closure_verified: false,
            qualified: false,
            report_only: true,
        }
    }

    fn refresh_bindings(report: &mut InferenceEnvironmentReport) {
        report.profile_sha256 = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&report.declared_profile).unwrap())
        );
        report.environment.platform_build = report.platform.identity().unwrap();
        report.environment.hardware = report.hardware.identity().unwrap();
    }

    #[test]
    fn fixture_observations_have_exact_content_identities_and_no_model_installation_claim() {
        let report = report();
        report.validate_for_profile(&profile()).unwrap();
        let platform_bytes = report.platform.canonical_bytes().unwrap();
        let hardware_bytes = report.hardware.canonical_bytes().unwrap();
        assert_eq!(report.environment.platform_build, artifact(&platform_bytes));
        assert_eq!(report.environment.hardware, artifact(&hardware_bytes));
        assert!(!report
            .environment
            .dependencies
            .contains(&report.declared_profile.model));
        let bytes = report.canonical_bytes().unwrap();
        let decoded: InferenceEnvironmentReport = serde_json::from_slice(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.canonical_bytes().unwrap(), bytes);
        assert!(!decoded.qualified);
        assert!(!decoded.runtime_dependency_closure_verified);
    }

    #[test]
    fn changing_observed_hardware_or_platform_changes_identity() {
        let original = hardware();
        let identity = original.identity().unwrap();
        for change in [
            |value: &mut InferenceHardwareObservation| value.physical_memory_bytes += 4096,
            |value: &mut InferenceHardwareObservation| value.allowed_cpu_ids = vec![0],
            |value: &mut InferenceHardwareObservation| value.cpu_models[0].microcode += 1,
            |value: &mut InferenceHardwareObservation| value.logical_cpus[1].core_id = 0,
            |value: &mut InferenceHardwareObservation| value.gpu = Some(gpu()),
        ] {
            let mut changed = original.clone();
            change(&mut changed);
            assert_ne!(changed.identity().unwrap(), identity);
        }
        let mut changed = platform();
        let before = changed.identity().unwrap();
        changed.kernel_version.push_str(" rebuilt");
        assert_ne!(changed.identity().unwrap(), before);
    }

    #[test]
    fn noncanonical_collections_and_unreferenced_topology_are_refused() {
        for change in [
            |value: &mut InferenceHardwareObservation| value.cpu_models[0].flags.reverse(),
            |value: &mut InferenceHardwareObservation| {
                value.cpu_models[0].flags.push("sse2".into())
            },
            |value: &mut InferenceHardwareObservation| value.cpu_models.push(model()),
            |value: &mut InferenceHardwareObservation| value.logical_cpus.reverse(),
            |value: &mut InferenceHardwareObservation| value.logical_cpus[1].processor_id = 0,
            |value: &mut InferenceHardwareObservation| value.logical_cpus[1].model_index = 1,
            |value: &mut InferenceHardwareObservation| value.logical_cpus[1].package_id = 1,
            |value: &mut InferenceHardwareObservation| {
                value.cpu_packages.push(value.cpu_packages[0].clone())
            },
            |value: &mut InferenceHardwareObservation| {
                value.cpu_packages.push(InferenceCpuPackageObservation {
                    package_id: 1,
                    reported_logical_cpus: 1,
                    reported_physical_cores: 1,
                })
            },
            |value: &mut InferenceHardwareObservation| value.allowed_cpu_ids.reverse(),
            |value: &mut InferenceHardwareObservation| value.allowed_cpu_ids.push(1),
            |value: &mut InferenceHardwareObservation| value.allowed_cpu_ids.push(2),
        ] {
            let mut bad = hardware();
            change(&mut bad);
            assert!(bad.validate().is_err());
        }
        let mut two_models = hardware();
        let mut other = model();
        other.model_name.push_str(" second");
        two_models.cpu_models.push(other);
        two_models
            .cpu_models
            .sort_by_cached_key(|entry| entry.canonical_key().unwrap());
        two_models.logical_cpus[1].model_index = 1;
        two_models.validate().unwrap();
        two_models.cpu_models.reverse();
        assert!(two_models.validate().is_err());
        two_models.cpu_models.reverse();
        two_models.logical_cpus[1].model_index = 0;
        assert!(two_models.validate().is_err());
    }

    #[test]
    fn observed_subset_can_be_offline_but_cannot_exceed_reported_topology() {
        hardware().validate().unwrap();
        for change in [
            |value: &mut InferenceHardwareObservation| {
                value.cpu_packages[0].reported_logical_cpus = 1
            },
            |value: &mut InferenceHardwareObservation| {
                value.cpu_packages[0].reported_physical_cores = 1
            },
            |value: &mut InferenceHardwareObservation| {
                value.cpu_packages[0].reported_physical_cores = 5
            },
            |value: &mut InferenceHardwareObservation| value.logical_cpus[1].core_id = 65536,
            |value: &mut InferenceHardwareObservation| value.physical_memory_bytes = 0,
            |value: &mut InferenceHardwareObservation| {
                value.physical_memory_bytes = (1u64 << 60) + 1
            },
            |value: &mut InferenceHardwareObservation| value.allowed_cpu_ids.clear(),
        ] {
            let mut bad = hardware();
            change(&mut bad);
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn platform_text_and_cpu_tokens_refuse_paths_controls_and_oversized_values() {
        for text in ["", "/proc/version", "C:\\version", "kernel\nversion"] {
            let mut bad = platform();
            bad.kernel_version = text.into();
            assert!(bad.validate().is_err());
        }
        let mut bad = platform();
        bad.kernel_release = "x".repeat(129);
        assert!(bad.validate().is_err());
        for token in ["", "has space", "slash/token", "avx\n", "é"] {
            let mut bad = model();
            bad.flags = vec![token.into()];
            assert!(bad.validate().is_err());
        }
        let mut bad = model();
        bad.vendor_id = "v".repeat(33);
        assert!(bad.validate().is_err());
        bad = model();
        bad.model_name = "x".repeat(257);
        assert!(bad.validate().is_err());
    }

    #[test]
    fn gpu_uuid_pci_and_driver_identity_are_canonical_and_bounded() {
        gpu().validate().unwrap();
        for change in [
            |value: &mut InferenceGpuObservation| {
                value.device_uuid = value.device_uuid.to_uppercase()
            },
            |value: &mut InferenceGpuObservation| value.device_minor = 64,
            |value: &mut InferenceGpuObservation| value.pci_bus_address = "0000:AB:00.0".into(),
            |value: &mut InferenceGpuObservation| value.pci_bus_address = "0000:01:20.0".into(),
            |value: &mut InferenceGpuObservation| value.pci_bus_address = "0000:01:00.8".into(),
            |value: &mut InferenceGpuObservation| value.pci_bus_address = "0000:01:é.0".into(),
            |value: &mut InferenceGpuObservation| value.kernel_driver.version = "550..120".into(),
            |value: &mut InferenceGpuObservation| {
                value.kernel_driver.version = "550.120-foo".into()
            },
            |value: &mut InferenceGpuObservation| value.kernel_driver.build.byte_len = 0,
            |value: &mut InferenceGpuObservation| {
                value.kernel_driver.build.artifact_id =
                    ArtifactId::new("not-content-addressed").unwrap()
            },
        ] {
            let mut bad = gpu();
            change(&mut bad);
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn report_refuses_changed_bindings_wrong_request_and_authority_claims() {
        for change in [
            |value: &mut InferenceEnvironmentReport| value.profile_sha256 = "0".repeat(64),
            |value: &mut InferenceEnvironmentReport| value.hardware.physical_memory_bytes += 4096,
            |value: &mut InferenceEnvironmentReport| {
                value.platform.kernel_version.push_str(" changed")
            },
            |value: &mut InferenceEnvironmentReport| value.environment.os = "other".into(),
            |value: &mut InferenceEnvironmentReport| {
                value.environment.architecture = "other".into()
            },
            |value: &mut InferenceEnvironmentReport| {
                value.environment.dependencies = vec![artifact(b"other runtime")]
            },
            |value: &mut InferenceEnvironmentReport| {
                value.environment.worker.sha256 = "invalid".into()
            },
            |value: &mut InferenceEnvironmentReport| value.environment.engine.byte_len = 0,
            |value: &mut InferenceEnvironmentReport| value.observed_at_unix_ms = 0,
            |value: &mut InferenceEnvironmentReport| value.qualified = true,
            |value: &mut InferenceEnvironmentReport| {
                value.runtime_dependency_closure_verified = true
            },
            |value: &mut InferenceEnvironmentReport| value.report_only = false,
        ] {
            let mut bad = report();
            change(&mut bad);
            assert!(bad.validate().is_err());
        }
        let mut expected = profile();
        expected.cpu_threads = 1;
        assert!(report().validate_for_profile(&expected).is_err());
        let mut changed = report();
        changed.declared_profile.model = artifact(b"other declaration");
        refresh_bindings(&mut changed);
        changed.validate().unwrap();
        assert!(changed.validate_for_profile(&profile()).is_err());
    }

    #[test]
    fn cuda_requires_observed_uuid_but_does_not_invent_an_ordinal_mapping() {
        let mut cpu = report();
        cpu.hardware.gpu = Some(gpu());
        refresh_bindings(&mut cpu);
        cpu.validate().unwrap();
        let mut cuda = cpu.clone();
        cuda.declared_profile.schema_version = 2;
        cuda.declared_profile.provider = InferenceProvider::Cuda;
        cuda.declared_profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 3,
            device_uuid: gpu().device_uuid,
            address_space_bytes: 16 << 30,
            device_reservation_bytes: 1 << 30,
            arena_limit_bytes: 1 << 29,
        });
        refresh_bindings(&mut cuda);
        cuda.validate().unwrap();
        let mut bad = cuda.clone();
        bad.hardware.gpu = None;
        refresh_bindings(&mut bad);
        assert!(bad.validate().is_err());
        let mut bad = cuda.clone();
        bad.declared_profile
            .cuda_resources
            .as_mut()
            .unwrap()
            .device_uuid = "GPU-0790118a-a6d7-4eaf-fcac-dcacac5f4351".into();
        refresh_bindings(&mut bad);
        assert!(bad.validate().is_err());
        let mut bad = cuda;
        bad.declared_profile.schema_version = 1;
        bad.declared_profile.cuda_resources = None;
        refresh_bindings(&mut bad);
        assert!(bad.validate().is_err());
        let mut bad = report();
        bad.declared_profile.provider = InferenceProvider::Rocm;
        refresh_bindings(&mut bad);
        assert_eq!(
            bad.validate().unwrap_err().code,
            crate::ErrorCode::Unsupported
        );
    }

    #[test]
    fn canonical_hardware_encoding_has_an_aggregate_bound() {
        let mut value = hardware();
        value.cpu_models = (0..64)
            .map(|index| {
                let mut entry = model();
                entry.model_name = format!("Fixture CPU {index:02}");
                entry.flags = (0..512)
                    .map(|flag| format!("flag_{flag:03}_{}", "x".repeat(50)))
                    .collect();
                entry
            })
            .collect();
        value
            .cpu_models
            .sort_by_cached_key(|entry| entry.canonical_key().unwrap());
        value.logical_cpus = (0..64)
            .map(|processor_id| InferenceLogicalCpuObservation {
                processor_id,
                package_id: 0,
                core_id: processor_id,
                model_index: processor_id as u16,
            })
            .collect();
        value.cpu_packages[0].reported_logical_cpus = 64;
        value.cpu_packages[0].reported_physical_cores = 64;
        assert!(serde_json::to_vec(&value).unwrap().len() > INFERENCE_HARDWARE_OBSERVATION_LIMIT);
        assert!(value.validate().is_err());
        assert!(value.identity().is_err());
    }

    #[test]
    fn wire_observations_reject_unknown_fields() {
        let mut value = serde_json::to_value(report()).unwrap();
        value["runtime_path"] = serde_json::json!("/private/runtime");
        assert!(serde_json::from_value::<InferenceEnvironmentReport>(value).is_err());
        let mut value = serde_json::to_value(hardware()).unwrap();
        value["gpu"] = serde_json::to_value(gpu()).unwrap();
        value["gpu"]["kernel_driver"]["raw_text"] = serde_json::json!("unretained");
        assert!(serde_json::from_value::<InferenceHardwareObservation>(value).is_err());
    }
}
