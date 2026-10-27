//! Bounded observations of the actual host. No inference loading, device
//! admission, dependency-closure verification, or scientific qualification.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

const MAX_CPUINFO_BYTES: usize = 16 * 1024 * 1024;
const MAX_MEMINFO_BYTES: usize = 64 * 1024;
const MAX_DRIVER_BYTES: usize = 8192;
const MAX_CPUS: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) struct HostObservation {
    pub(crate) platform: InferencePlatformObservation,
    pub(crate) hardware: InferenceHardwareObservation,
}
impl HostObservation {
    /// The caller obtains the device identity from its actual pinned devices.
    /// This attachment is still an observation, never proof of usable CUDA.
    pub(crate) fn with_gpu(mut self, gpu: InferenceGpuObservation) -> Result<Self> {
        ensure!(
            self.hardware.gpu.is_none(),
            "GPU observation already attached"
        );
        gpu.validate()?;
        self.hardware.gpu = Some(gpu);
        self.platform.validate()?;
        self.hardware.validate()?;
        Ok(self)
    }
}

fn unsupported(message: &str) -> anyhow::Error {
    ProtocolError::unsupported(message).into()
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) fn observe_cpu() -> Result<HostObservation> {
    // The second bounded sample detects topology, microcode, feature, memory,
    // affinity or kernel changes during observation. Volatile fields are never
    // included. This is not an atomic hotplug lock or a future-use reservation.
    let first = observe_once()?;
    let second = observe_once()?;
    require_unchanged(&first, &second)?;
    Ok(second)
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub(crate) fn observe_cpu() -> Result<HostObservation> {
    Err(unsupported(
        "native inference environment observation requires Linux x86_64",
    ))
}

fn require_unchanged(first: &HostObservation, second: &HostObservation) -> Result<()> {
    ensure!(
        first.platform.canonical_bytes()? == second.platform.canonical_bytes()?
            && first.hardware.canonical_bytes()? == second.hardware.canonical_bytes()?,
        "host environment changed during observation"
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn observe_once() -> Result<HostObservation> {
    let platform = observe_platform()?;
    let affinity_before = observe_affinity()?;
    let cpuinfo = kernel_text(std::path::Path::new("/proc/cpuinfo"), MAX_CPUINFO_BYTES)?;
    let meminfo = kernel_text(std::path::Path::new("/proc/meminfo"), MAX_MEMINFO_BYTES)?;
    let affinity_after = observe_affinity()?;
    ensure!(
        affinity_before == affinity_after,
        "CPU affinity changed during observation"
    );
    let hardware = parse_cpuinfo(
        &platform.architecture,
        &cpuinfo,
        affinity_after,
        parse_memory(&meminfo)?,
    )?;
    Ok(HostObservation { platform, hardware })
}

#[cfg(target_os = "linux")]
fn kernel_text(path: &std::path::Path, limit: usize) -> Result<String> {
    use std::{
        io::Read,
        os::unix::{fs::MetadataExt, io::AsRawFd},
    };
    let mut file = crate::artifacts::open_regular(path)?;
    let before = file.metadata()?;
    ensure!(
        before.uid() == 0 && before.mode() & 0o022 == 0,
        "host information must be root-owned and not externally writable"
    );
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } == 0,
        "cannot establish host information filesystem"
    );
    ensure!(
        filesystem.f_type == libc::PROC_SUPER_MAGIC,
        "host information must come from procfs"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= limit,
        "host information exceeds byte admission"
    );
    let after = file.metadata()?;
    ensure!(
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.uid() == after.uid()
            && before.mode() == after.mode(),
        "host information descriptor changed during observation"
    );
    String::from_utf8(bytes).context("host information is not UTF-8")
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn observe_platform() -> Result<InferencePlatformObservation> {
    let mut data: libc::utsname = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::uname(&mut data) } == 0,
        "cannot observe running kernel"
    );
    fn field(raw: &[libc::c_char]) -> Result<String> {
        let end = raw
            .iter()
            .position(|byte| *byte == 0)
            .context("kernel identity lacks terminator")?;
        let bytes: Vec<u8> = raw[..end].iter().map(|byte| *byte as u8).collect();
        String::from_utf8(bytes).context("kernel identity is not UTF-8")
    }
    // nodename/domainname are deliberately never read, hashed or returned.
    if field(&data.sysname)? != "Linux" || field(&data.machine)? != "x86_64" {
        return Err(unsupported(
            "native inference environment parser supports Linux x86_64 only",
        ));
    }
    let observed = InferencePlatformObservation {
        schema_version: 1,
        os: "linux".into(),
        architecture: "x86_64".into(),
        kernel_release: field(&data.release)?,
        kernel_version: field(&data.version)?,
    };
    observed.validate()?;
    Ok(observed)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn observe_affinity() -> Result<Vec<u32>> {
    // Linux accepts a dynamically sized bitmask; a fixed cpu_set_t would silently
    // limit support to its compile-time CPU count. The bound is 65536 CPU IDs.
    let word_bytes = std::mem::size_of::<libc::c_ulong>();
    for bytes in [128usize, 256, 512, 1024, 2048, 4096, 8192] {
        let mut words = vec![0 as libc::c_ulong; bytes / word_bytes];
        let rc = unsafe {
            libc::sched_getaffinity(0, bytes, words.as_mut_ptr().cast::<libc::cpu_set_t>())
        };
        if rc == 0 {
            let mut ids = Vec::new();
            for (index, word) in words.into_iter().enumerate() {
                for bit in 0..libc::c_ulong::BITS {
                    if word & ((1 as libc::c_ulong) << bit) != 0 {
                        ids.push((index * libc::c_ulong::BITS as usize + bit as usize) as u32);
                        ensure!(
                            ids.len() <= MAX_CPUS,
                            "CPU affinity exceeds logical-CPU admission"
                        );
                    }
                }
            }
            ensure!(!ids.is_empty(), "CPU affinity is empty");
            return Ok(ids);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINVAL) {
            return Err(error).context("cannot observe current CPU affinity");
        }
    }
    Err(unsupported(
        "CPU affinity exceeds the bounded environment parser",
    ))
}

fn decimal(value: &str) -> Result<u32> {
    ensure!(
        !value.is_empty() && value.len() <= 10 && value.bytes().all(|b| b.is_ascii_digit()),
        "CPU numeric field is not a bounded unsigned decimal"
    );
    let value: u32 = value.parse()?;
    ensure!(value <= 65535, "CPU numeric field exceeds admission");
    Ok(value)
}

fn microcode(value: &str) -> Result<u64> {
    let hex = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .context("CPU microcode lacks hexadecimal representation")?;
    ensure!(
        !hex.is_empty() && hex.len() <= 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "CPU microcode exceeds admission"
    );
    Ok(u64::from_str_radix(hex, 16)?)
}

fn required<'a>(fields: &BTreeMap<&str, &'a str>, key: &str) -> Result<&'a str> {
    fields
        .get(key)
        .copied()
        .with_context(|| format!("required x86 CPU field missing: {key}"))
}

/// Parse selected documented x86 cpuinfo fields. Unknown fields, including
/// serials, volatile MHz and cache-performance values, are never retained.
fn parse_cpuinfo(
    architecture: &str,
    text: &str,
    allowed_cpu_ids: Vec<u32>,
    physical_memory_bytes: u64,
) -> Result<InferenceHardwareObservation> {
    if architecture != "x86_64" {
        return Err(unsupported("CPU environment parser supports x86_64 only"));
    }
    ensure!(
        !text.is_empty() && text.len() <= MAX_CPUINFO_BYTES,
        "CPU information exceeds byte admission"
    );
    let mut models = BTreeMap::<Vec<u8>, InferenceCpuModelObservation>::new();
    let mut logical = BTreeMap::<u32, (u32, u32, Vec<u8>)>::new();
    let mut packages = BTreeMap::<u32, InferenceCpuPackageObservation>::new();
    let mut fields = BTreeMap::new();
    let mut consume = |fields: &mut BTreeMap<&str, &str>| -> Result<()> {
        if fields.is_empty() {
            return Ok(());
        }
        ensure!(
            logical.len() < MAX_CPUS,
            "CPU topology exceeds logical-CPU admission"
        );
        let processor = decimal(required(fields, "processor")?)?;
        let package = decimal(required(fields, "physical id")?)?;
        let core = decimal(required(fields, "core id")?)?;
        let mut flags = Vec::new();
        let mut unique_flags = BTreeSet::new();
        for flag in required(fields, "flags")?.split_ascii_whitespace() {
            ensure!(unique_flags.insert(flag), "duplicate CPU feature field");
            flags.push(flag.to_owned());
            ensure!(flags.len() <= 512, "CPU feature count exceeds admission");
        }
        flags.sort();
        let model = InferenceCpuModelObservation {
            vendor_id: required(fields, "vendor_id")?.to_owned(),
            family: decimal(required(fields, "cpu family")?)?,
            model: decimal(required(fields, "model")?)?,
            model_name: required(fields, "model name")?
                .split_ascii_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            stepping: decimal(required(fields, "stepping")?)?,
            microcode: microcode(required(fields, "microcode")?)?,
            flags,
        };
        model.validate()?;
        let key = model.canonical_key()?;
        models.entry(key.clone()).or_insert(model);
        ensure!(models.len() <= 64, "CPU model count exceeds admission");
        let package_observation = InferenceCpuPackageObservation {
            package_id: package,
            reported_logical_cpus: decimal(required(fields, "siblings")?)?,
            reported_physical_cores: decimal(required(fields, "cpu cores")?)?,
        };
        package_observation.validate()?;
        if let Some(existing) = packages.get(&package) {
            ensure!(
                existing.reported_logical_cpus == package_observation.reported_logical_cpus
                    && existing.reported_physical_cores
                        == package_observation.reported_physical_cores,
                "conflicting CPU package topology"
            );
        } else {
            packages.insert(package, package_observation);
        }
        ensure!(
            logical.insert(processor, (package, core, key)).is_none(),
            "duplicate logical CPU identity"
        );
        fields.clear();
        Ok(())
    };
    for line in text.lines() {
        ensure!(
            line.len() <= 65536,
            "CPU information line exceeds admission"
        );
        if line.trim().is_empty() {
            consume(&mut fields)?;
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .context("CPU information field lacks separator")?;
        let key = key.trim();
        if matches!(
            key,
            "processor"
                | "vendor_id"
                | "cpu family"
                | "model"
                | "model name"
                | "stepping"
                | "microcode"
                | "physical id"
                | "siblings"
                | "core id"
                | "cpu cores"
                | "flags"
        ) {
            ensure!(
                fields.insert(key, value.trim()).is_none(),
                "duplicate CPU identity field"
            );
        }
    }
    consume(&mut fields)?;
    ensure!(
        !logical.is_empty(),
        "CPU information contains no supported logical processors"
    );
    let model_indexes: BTreeMap<Vec<u8>, u16> = models
        .keys()
        .enumerate()
        .map(|(index, key)| (key.clone(), index as u16))
        .collect();
    let logical_cpus = logical
        .into_iter()
        .map(|(processor_id, (package_id, core_id, key))| {
            Ok(InferenceLogicalCpuObservation {
                processor_id,
                package_id,
                core_id,
                model_index: *model_indexes
                    .get(&key)
                    .context("CPU model correspondence disappeared")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let observation = InferenceHardwareObservation {
        schema_version: 1,
        architecture: architecture.into(),
        cpu_models: models.into_values().collect(),
        logical_cpus,
        cpu_packages: packages.into_values().collect(),
        allowed_cpu_ids,
        physical_memory_bytes,
        gpu: None,
    };
    observation.validate()?;
    Ok(observation)
}

fn parse_memory(text: &str) -> Result<u64> {
    ensure!(
        !text.is_empty() && text.len() <= MAX_MEMINFO_BYTES,
        "memory information exceeds admission"
    );
    let mut total = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != "MemTotal" {
            continue;
        }
        ensure!(total.is_none(), "duplicate total physical memory field");
        let parts: Vec<_> = value.split_ascii_whitespace().collect();
        ensure!(
            parts.len() == 2
                && parts[1] == "kB"
                && !parts[0].is_empty()
                && parts[0].len() <= 20
                && parts[0].bytes().all(|b| b.is_ascii_digit()),
            "total physical memory representation differs"
        );
        let kib: u64 = parts[0].parse()?;
        let bytes = kib
            .checked_mul(1024)
            .context("total physical memory overflow")?;
        ensure!(
            bytes > 0 && bytes <= 1 << 60,
            "total physical memory exceeds admission"
        );
        total = Some(bytes);
    }
    total.context("total physical memory field missing")
}

/// Optional, separate from CPU observation. It neither opens a GPU node nor
/// loads driver/runtime libraries. Root binds the result to pinned device facts.
#[cfg(target_os = "linux")]
pub(crate) fn observe_nvidia_driver() -> Result<InferenceNvidiaDriverObservation> {
    let first = kernel_text(
        std::path::Path::new("/proc/driver/nvidia/version"),
        MAX_DRIVER_BYTES,
    )?;
    let second = kernel_text(
        std::path::Path::new("/proc/driver/nvidia/version"),
        MAX_DRIVER_BYTES,
    )?;
    ensure!(
        first == second,
        "kernel NVIDIA driver changed during observation"
    );
    parse_nvidia_driver(&second)
}
#[cfg(not(target_os = "linux"))]
pub(crate) fn observe_nvidia_driver() -> Result<InferenceNvidiaDriverObservation> {
    Err(unsupported(
        "kernel NVIDIA driver observation requires Linux procfs",
    ))
}
fn parse_nvidia_driver(text: &str) -> Result<InferenceNvidiaDriverObservation> {
    use sha2::{Digest, Sha256};
    ensure!(
        !text.is_empty()
            && text.len() <= MAX_DRIVER_BYTES
            && !text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "kernel NVIDIA driver information exceeds admission"
    );
    let mut fields = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line
            .split_once(':')
            .context("kernel NVIDIA driver field lacks separator")?;
        let key = key.trim();
        ensure!(
            matches!(key, "NVRM version" | "GCC version"),
            "unsupported kernel NVIDIA driver field"
        );
        ensure!(
            fields.insert(key, value.trim()).is_none(),
            "duplicate kernel NVIDIA driver field"
        );
    }
    let nvrm = fields
        .get("NVRM version")
        .context("kernel NVIDIA version field missing")?;
    let module_flavor = if nvrm.starts_with("NVIDIA UNIX Open Kernel Module for x86_64 ") {
        InferenceNvidiaModuleFlavor::Open
    } else if nvrm.starts_with("NVIDIA UNIX x86_64 Kernel Module ") {
        InferenceNvidiaModuleFlavor::Proprietary
    } else {
        return Err(unsupported(
            "unrecognized x86_64 NVIDIA kernel module declaration",
        ));
    };
    let versions: Vec<_> = nvrm
        .split_ascii_whitespace()
        .filter(|part| {
            let components: Vec<_> = part.split('.').collect();
            (2..=4).contains(&components.len())
                && components.iter().all(|component| {
                    !component.is_empty()
                        && component.len() <= 10
                        && component.bytes().all(|b| b.is_ascii_digit())
                })
        })
        .collect();
    ensure!(
        versions.len() == 1,
        "kernel NVIDIA driver version is ambiguous or missing"
    );
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let observation = InferenceNvidiaDriverObservation {
        version: versions[0].to_owned(),
        module_flavor,
        // No raw build text, path, hostname, serial or compiler text is returned.
        build: ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone())?,
            sha256: hash,
            byte_len: text.len() as u64,
        },
    };
    observation.validate()?;
    Ok(observation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(processor: u32, core: u32, flags: &str) -> String {
        format!("processor : {processor}\nvendor_id : GenuineIntel\ncpu family : 6\nmodel : 85\nmodel name : Authored CPU Model\nstepping : 7\nmicrocode : 0x2f\nphysical id : 0\nsiblings : 4\ncore id : {core}\ncpu cores : 2\nflags : {flags}\ncpu MHz : 3200.000\nSerial : never-retained-machine-serial\n\n")
    }
    fn hardware(text: &str) -> InferenceHardwareObservation {
        parse_cpuinfo("x86_64", text, vec![0, 1], 8 * 1024 * 1024 * 1024).unwrap()
    }
    fn platform() -> InferencePlatformObservation {
        InferencePlatformObservation {
            schema_version: 1,
            os: "linux".into(),
            architecture: "x86_64".into(),
            kernel_release: "6.8.0-authored".into(),
            kernel_version: "#1 SMP Authored kernel build".into(),
        }
    }
    fn sample() -> String {
        cpu(0, 0, "sse2 avx xsave") + &cpu(1, 0, "sse2 avx xsave")
    }

    #[test]
    fn cpu_parser_canonicalizes_model_feature_and_processor_order() {
        let canonical = hardware(&sample());
        let reordered = hardware(&(cpu(1, 0, "xsave sse2 avx") + &cpu(0, 0, "avx xsave sse2")));
        assert_eq!(
            canonical.canonical_bytes().unwrap(),
            reordered.canonical_bytes().unwrap()
        );
        assert_eq!(canonical.cpu_models.len(), 1);
        assert_eq!(canonical.cpu_models[0].flags, ["avx", "sse2", "xsave"]);
        assert_eq!(canonical.logical_cpus[0].processor_id, 0);
        assert_eq!(canonical.logical_cpus[1].processor_id, 1);
    }

    #[test]
    fn volatile_fields_and_serials_never_affect_or_enter_hardware_identity() {
        let first = hardware(&sample());
        let changed = sample()
            .replace("3200.000", "800.000")
            .replace("never-retained-machine-serial", "another-private-serial");
        let second = hardware(&changed);
        assert_eq!(first.identity().unwrap(), second.identity().unwrap());
        let output = String::from_utf8(first.canonical_bytes().unwrap()).unwrap();
        for absent in [
            "MHz",
            "3200",
            "Serial",
            "never-retained",
            "/proc",
            "uptime",
            "boot_id",
        ] {
            assert!(!output.contains(absent));
        }
    }

    #[test]
    fn missing_duplicate_conflicting_or_unsupported_cpu_fields_refuse() {
        let original = sample();
        for bad in [
            original.replace("microcode : 0x2f\n", ""),
            original.replace(
                "flags : sse2 avx xsave",
                "flags : sse2 avx xsave\nflags : sse2",
            ),
            original.replace("flags : sse2 avx xsave", "flags : sse2 avx avx"),
            original.replace("microcode : 0x2f", "microcode : unknown"),
            cpu(0, 0, "avx") + &cpu(0, 0, "avx"),
            cpu(0, 0, "avx") + &cpu(1, 0, "avx").replace("siblings : 4", "siblings : 8"),
            original.replace("processor : 1", "processor : 65536"),
        ] {
            assert!(parse_cpuinfo("x86_64", &bad, vec![0, 1], 1024).is_err());
        }
        let error = parse_cpuinfo("aarch64", &original, vec![0, 1], 1024).unwrap_err();
        assert_eq!(
            error.downcast_ref::<ProtocolError>().unwrap().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn affinity_and_memory_are_actual_observations_not_reconstructed_defaults() {
        for allowed in [vec![], vec![1, 0], vec![0, 0], vec![0, 3]] {
            assert!(parse_cpuinfo("x86_64", &sample(), allowed, 1024).is_err());
        }
        assert!(parse_cpuinfo("x86_64", &sample(), vec![0], 0).is_err());
        let a = hardware(&sample());
        let b = parse_cpuinfo("x86_64", &sample(), vec![0], a.physical_memory_bytes).unwrap();
        assert_ne!(a.identity().unwrap(), b.identity().unwrap());
    }

    #[test]
    fn changed_model_microcode_features_memory_or_topology_changes_identity() {
        let baseline = hardware(&sample());
        for changed in [
            sample().replace("Authored CPU Model", "Different Authored Model"),
            sample().replace("microcode : 0x2f", "microcode : 0x30"),
            sample().replace("sse2 avx xsave", "sse2 xsave"),
            cpu(0, 0, "sse2 avx xsave") + &cpu(1, 1, "sse2 avx xsave"),
        ] {
            assert_ne!(
                baseline.identity().unwrap(),
                hardware(&changed).identity().unwrap()
            );
        }
        let changed_memory = parse_cpuinfo(
            "x86_64",
            &sample(),
            vec![0, 1],
            baseline.physical_memory_bytes + 1024,
        )
        .unwrap();
        assert_ne!(
            baseline.identity().unwrap(),
            changed_memory.identity().unwrap()
        );
    }

    #[test]
    fn model_groups_are_canonical_and_topology_does_not_invent_offline_cpus() {
        let text =
            cpu(0, 0, "sse2 avx") + &cpu(1, 1, "sse2 avx").replace("stepping : 7", "stepping : 8");
        let observation = hardware(&text);
        assert_eq!(observation.cpu_models.len(), 2);
        assert_eq!(observation.logical_cpus.len(), 2);
        assert_eq!(observation.cpu_packages[0].reported_logical_cpus, 4);
        assert_eq!(observation.allowed_cpu_ids, [0, 1]);
        assert!(observation
            .cpu_models
            .windows(2)
            .all(|pair| pair[0].canonical_key().unwrap() < pair[1].canonical_key().unwrap()));
    }

    #[test]
    fn stable_two_pass_comparison_refuses_hardware_affinity_or_kernel_change() {
        let first = HostObservation {
            platform: platform(),
            hardware: hardware(&sample()),
        };
        require_unchanged(&first, &first).unwrap();
        let mut second = first.clone();
        second.hardware.allowed_cpu_ids = vec![0];
        assert!(require_unchanged(&first, &second).is_err());
        second = first.clone();
        second.platform.kernel_release = "6.8.1-authored".into();
        assert!(require_unchanged(&first, &second).is_err());
        second = first.clone();
        second.hardware = hardware(&(sample() + &cpu(2, 1, "sse2 avx xsave")));
        assert!(require_unchanged(&first, &second).is_err());
    }

    #[test]
    fn total_memory_parser_ignores_free_memory_and_refuses_ambiguous_units() {
        assert_eq!(
            parse_memory("MemTotal: 1024 kB\nMemFree: 100 kB\n").unwrap(),
            1024 * 1024
        );
        assert_eq!(
            parse_memory("MemTotal: 1024 kB\nMemFree: 999 kB\n").unwrap(),
            1024 * 1024
        );
        for bad in [
            "MemFree: 100 kB",
            "MemTotal: 1 MB",
            "MemTotal: 0 kB",
            "MemTotal: 1 kB\nMemTotal: 1 kB",
            "MemTotal: 18446744073709551615 kB",
            "MemTotal: +1 kB",
        ] {
            assert!(parse_memory(bad).is_err());
        }
    }

    fn driver() -> &'static str {
        "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  580.105.08  Release Build  Thu Oct  1 00:00:00 UTC 2026\nGCC version: gcc version 13.3.0 (Authored distribution)\n"
    }
    #[test]
    fn driver_observation_retains_typed_version_and_exact_build_identity_only() {
        let observed = parse_nvidia_driver(driver()).unwrap();
        assert_eq!(observed.version, "580.105.08");
        assert!(matches!(
            observed.module_flavor,
            InferenceNvidiaModuleFlavor::Open
        ));
        assert_eq!(observed.build.byte_len, driver().len() as u64);
        let changed = parse_nvidia_driver(&driver().replace("13.3.0", "13.3.1")).unwrap();
        assert_ne!(observed.build, changed.build);
        let encoded = serde_json::to_string(&observed).unwrap();
        assert!(!encoded.contains("Authored distribution") && !encoded.contains("/proc"));
        let proprietary = parse_nvidia_driver(&driver().replace(
            "NVIDIA UNIX Open Kernel Module for x86_64",
            "NVIDIA UNIX x86_64 Kernel Module",
        ))
        .unwrap();
        assert!(matches!(
            proprietary.module_flavor,
            InferenceNvidiaModuleFlavor::Proprietary
        ));
    }

    #[test]
    fn unknown_or_ambiguous_driver_declarations_are_not_guessed() {
        for bad in [
            driver().replace("x86_64", "aarch64"),
            driver().replace("580.105.08", "unknown"),
            driver().replace("580.105.08", "580.105.08 581.0"),
            driver().to_owned() + "NVRM version: duplicate\n",
            driver().to_owned() + "Machine Serial: private\n",
        ] {
            assert!(parse_nvidia_driver(&bad).is_err());
        }
    }

    #[test]
    fn gpu_attachment_preserves_common_observations_and_does_not_replace_gpu() {
        let before = HostObservation {
            platform: platform(),
            hardware: hardware(&sample()),
        };
        let gpu = InferenceGpuObservation {
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            pci_bus_address: "0000:01:00.0".into(),
            device_minor: 0,
            kernel_driver: parse_nvidia_driver(driver()).unwrap(),
        };
        let after = before.clone().with_gpu(gpu.clone()).unwrap();
        assert_eq!(
            after.platform.identity().unwrap(),
            before.platform.identity().unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&after.hardware.logical_cpus).unwrap(),
            serde_json::to_vec(&before.hardware.logical_cpus).unwrap()
        );
        assert_eq!(
            after.hardware.physical_memory_bytes,
            before.hardware.physical_memory_bytes
        );
        assert_ne!(
            after.hardware.identity().unwrap(),
            before.hardware.identity().unwrap()
        );
        assert!(after.with_gpu(gpu).is_err());
    }
}
