//! Fresh host and named-file observations. This module grants no execution authority.
use super::*;
#[cfg(target_os = "linux")]
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileObservation {
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
}
#[cfg(target_os = "linux")]
impl FileObservation {
    fn of(metadata: &fs::Metadata) -> PResult<Self> {
        if !metadata.is_file() {
            return Err(ProtocolError::invalid(
                "inference environment requires regular named files",
            ));
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanos: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanos: metadata.ctime_nsec(),
        })
    }
}

#[cfg(target_os = "linux")]
struct VerifiedFile {
    path: PathBuf,
    file: fs::File,
    observation: FileObservation,
}
#[cfg(target_os = "linux")]
impl VerifiedFile {
    fn open(path: &Path, expected: &ArtifactIdentity, deadline: Instant) -> PResult<Self> {
        expected.validate()?;
        if expected.byte_len == 0 || expected.byte_len > MAX_NATIVE_LIBRARY_BYTES {
            return Err(ProtocolError::invalid(
                "inference environment file exceeds its byte bound",
            ));
        }
        let mut file = artifacts::open_regular(path).map_err(unavailable)?;
        let observation = FileObservation::of(&file.metadata().map_err(unavailable)?)?;
        if observation.length != expected.byte_len {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "named inference dependency length changed",
            ));
        }
        let digest = hash_opened(&mut file, expected.byte_len, deadline)?;
        if digest != expected.sha256 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "named inference dependency bytes changed",
            ));
        }
        let held = Self {
            path: path.to_path_buf(),
            file,
            observation,
        };
        held.recheck()?;
        Ok(held)
    }
    fn recheck(&self) -> PResult<()> {
        if FileObservation::of(&self.file.metadata().map_err(unavailable)?)? != self.observation
            || FileObservation::of(&fs::symlink_metadata(&self.path).map_err(unavailable)?)?
                != self.observation
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "named inference dependency changed during host observation",
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn observation_deadline(deadline: Instant) -> PResult<()> {
    if Instant::now() >= deadline {
        return Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "inference environment observation deadline elapsed",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn hash_opened(file: &mut fs::File, expected_length: u64, deadline: Instant) -> PResult<String> {
    observation_deadline(deadline)?;
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        observation_deadline(deadline)?;
        let count = file.read(&mut buffer).map_err(unavailable)?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .filter(|value| *value <= expected_length)
            .ok_or_else(|| {
                ProtocolError::invalid("inference dependency grew during observation")
            })?;
        hash.update(&buffer[..count]);
    }
    if length != expected_length {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "inference dependency length changed during observation",
        ));
    }
    observation_deadline(deadline)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// This descriptor follows only the kernel's fixed running-image magic link.
/// A configured worker path or a replaced on-disk Engine path cannot stand in
/// for the image actually executing this authority process.
#[cfg(target_os = "linux")]
struct LoadedControlImage {
    file: fs::File,
    observation: FileObservation,
    identity: ArtifactIdentity,
}
#[cfg(target_os = "linux")]
impl LoadedControlImage {
    fn open(deadline: Instant) -> PResult<Self> {
        observation_deadline(deadline)?;
        let mut file = fs::File::open("/proc/self/exe").map_err(unavailable)?;
        let observation = FileObservation::of(&file.metadata().map_err(unavailable)?)?;
        if observation.length == 0 || observation.length > 512 * 1024 * 1024 {
            return Err(ProtocolError::invalid(
                "unsupported loaded control image size",
            ));
        }
        let sha256 = hash_opened(&mut file, observation.length, deadline)?;
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).map_err(internal)?,
            sha256,
            byte_len: observation.length,
        };
        let held = Self {
            file,
            observation,
            identity,
        };
        held.recheck()?;
        observation_deadline(deadline)?;
        Ok(held)
    }
    fn recheck(&self) -> PResult<()> {
        let current = fs::File::open("/proc/self/exe").map_err(unavailable)?;
        if FileObservation::of(&self.file.metadata().map_err(unavailable)?)? != self.observation
            || FileObservation::of(&current.metadata().map_err(unavailable)?)? != self.observation
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "loaded control image changed during observation",
            ));
        }
        Ok(())
    }
}

/// Private lifetime-bound facts, not permission to capture, execute, retry, or
/// publish. A caller must separately check current owner acceptance and actual
/// captured-image correspondence. Borrowed reports remain historical reports.
#[cfg(target_os = "linux")]
pub(super) struct HeldInferenceEnvironment {
    report: InferenceEnvironmentReport,
    held: Vec<VerifiedFile>,
    control: LoadedControlImage,
    gpu: Option<(
        crate::cuda_devices::PinnedCudaDevices,
        InferenceNvidiaDriverObservation,
    )>,
    validity: ObservationValidity,
    // Retain the complete observation budget through descriptors and all future
    // rechecks, even when the caller moves this object out of the request scope.
    _memory: crate::resources::Reservation,
}
#[cfg(target_os = "linux")]
struct ObservationValidity {
    deadline: Instant,
    wall_floor: AtomicU64,
    invalidated: AtomicBool,
}
#[cfg(target_os = "linux")]
impl ObservationValidity {
    fn check(&self, observe: impl FnOnce() -> PResult<()>) -> PResult<()> {
        if self.invalidated.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "inference environment observation was invalidated",
            ));
        }
        let result = (|| {
            observation_deadline(self.deadline)?;
            self.advance_clock()?;
            observe()?;
            self.advance_clock()?;
            observation_deadline(self.deadline)?;
            if self.invalidated.load(Ordering::Acquire) {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "inference environment observation was invalidated",
                ));
            }
            Ok(())
        })();
        if result.is_err() {
            self.invalidated.store(true, Ordering::Release);
        }
        result
    }
    fn advance_clock(&self) -> PResult<()> {
        let now = wall_time()?;
        let previous = self.wall_floor.fetch_max(now, Ordering::AcqRel);
        if now < previous {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "inference environment observation wall clock moved backwards",
            ));
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
impl HeldInferenceEnvironment {
    /// Check only the original observation lifetime and clock watermark. This
    /// performs no file or hardware observation and cannot refresh either.
    pub(super) fn check_lifetime(&self) -> PResult<()> {
        self.validity.check(|| Ok(()))
    }
    pub(super) fn report(&self) -> &InferenceEnvironmentReport {
        &self.report
    }
    pub(super) fn environment(&self) -> &InferenceEquivalenceEnvironment {
        &self.report.environment
    }
    /// Reobserve applicability without changing the public report timestamp or
    /// extending its deadline. A failed check permanently invalidates this hold.
    pub(super) fn recheck(&self) -> PResult<()> {
        self.validity.check(|| {
            self.control.recheck()?;
            for file in &self.held {
                file.recheck()?;
            }
            observation_deadline(self.validity.deadline)?;
            let mut current =
                crate::inference_host_observation::observe_cpu().map_err(unavailable)?;
            if let Some((devices, driver)) = &self.gpu {
                let expected = devices.expectation();
                let fresh =
                    crate::cuda_devices::PinnedCudaDevices::open(expected).map_err(unavailable)?;
                let fresh_driver = crate::inference_host_observation::observe_nvidia_driver()
                    .map_err(unavailable)?;
                if encode(&devices.identities())? != encode(&fresh.identities())?
                    || encode(driver)? != encode(&fresh_driver)?
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "CUDA kernel observation changed",
                    ));
                }
                current = current
                    .with_gpu(InferenceGpuObservation {
                        device_uuid: expected.device_uuid.clone(),
                        pci_bus_address: expected.pci_bus_address.clone(),
                        device_minor: expected.device_minor,
                        kernel_driver: fresh_driver,
                    })
                    .map_err(unavailable)?;
            }
            if current.platform.identity()? != self.report.environment.platform_build
                || current.hardware.identity()? != self.report.environment.hardware
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "host applicability changed during observation",
                ));
            }
            self.control.recheck()?;
            for file in &self.held {
                file.recheck()?;
            }
            Ok(())
        })
    }
}

/// Freshly observe the named files selected by the immutable loaded policy.
/// These identities do not establish a closed namespace or successful loading.
#[cfg(target_os = "linux")]
fn observe_cpu_policy_libraries(
    owner: &crate::cpu_owner_policy::LoadedCpuOwnerPolicy,
    held: &mut Vec<VerifiedFile>,
    identities: &mut Vec<ArtifactIdentity>,
    total: &mut u64,
    deadline: Instant,
) -> PResult<()> {
    let image = owner.image().map_err(unavailable)?;
    for library in &owner.policy().native_dependencies.libraries {
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(library.sha256.clone()).map_err(internal)?,
            sha256: library.sha256.clone(),
            byte_len: library.byte_len,
        };
        *total = total
            .checked_add(identity.byte_len)
            .filter(|bytes| *bytes <= MAX_NATIVE_TOTAL_BYTES)
            .ok_or_else(|| {
                ProtocolError::invalid("named CPU dependencies exceed aggregate byte bound")
            })?;
        held.push(VerifiedFile::open(&library.path, &identity, deadline)?);
        identities.push(identity);
    }
    // Both were computed from exact retained policy bytes and canonical image
    // metadata. No fresh reading of a mutable policy pathname grants authority.
    identities.push(owner.identity().clone());
    identities.push(image.identity()?);
    Ok(())
}

#[cfg(target_os = "linux")]
fn wall_time() -> PResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .filter(|value| *value > 0 && *value <= i64::MAX as u64)
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "host observation wall clock unavailable",
            )
        })
}

impl Engine {
    pub(super) fn inspect_inference_environment(
        &self,
        request: &Request,
        actor: &SessionId,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, actor)?;
        }
        let Command::InspectInferenceEnvironment { profile, .. } = &request.command else {
            return Err(ProtocolError::unsupported(
                "inference environment inspection",
            ));
        };
        #[cfg(not(target_os = "linux"))]
        {
            let _ = profile;
            Err(ProtocolError::unsupported(
                "verified inference host observations require Linux",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(30))
                .ok_or_else(|| ProtocolError::invalid("host observation deadline overflow"))?;
            let held = self.observe_inference_environment(profile, deadline)?;
            {
                let db = self.db.lock().map_err(internal)?;
                self.authorize(&db, request, actor)?;
            }
            held.recheck()?;
            Ok(ResponseBody::InferenceEnvironment(Box::new(
                held.report().clone(),
            )))
        }
    }

    /// This internal observation performs no authorization or registry decision.
    /// Its hold lasts at most 30 seconds and never extends the caller's deadline.
    #[cfg(target_os = "linux")]
    pub(super) fn observe_inference_environment(
        &self,
        profile: &DeclaredInferenceProfile,
        deadline: Instant,
    ) -> PResult<HeldInferenceEnvironment> {
        profile.validate()?;
        let deadline = deadline.min(
            Instant::now()
                .checked_add(Duration::from_secs(30))
                .ok_or_else(|| ProtocolError::invalid("host observation deadline overflow"))?,
        );
        observation_deadline(deadline)?;
        let memory = self.pool.reserve_memory(128 * 1024 * 1024)?;
        let started = wall_time()?;
        let control = LoadedControlImage::open(deadline)?;
        let tools = self.tools.as_ref().map_err(unavailable)?;
        let runtime = self.selected_profile_runtime(profile)?;
        let mut host = crate::inference_host_observation::observe_cpu().map_err(unavailable)?;
        let mut held = vec![VerifiedFile::open(
            &self.config.worker_executable,
            &self.binary_identity,
            deadline,
        )?];
        let mut identities = Vec::new();
        let mut total = control
            .identity
            .byte_len
            .checked_add(self.binary_identity.byte_len)
            .filter(|bytes| *bytes <= MAX_NATIVE_TOTAL_BYTES)
            .ok_or_else(|| {
                ProtocolError::invalid("inference images exceed aggregate byte bound")
            })?;
        for file in [&tools.ffmpeg, &tools.ffprobe, &runtime] {
            total = total
                .checked_add(file.identity.byte_len)
                .filter(|n| *n <= MAX_NATIVE_TOTAL_BYTES)
                .ok_or_else(|| {
                    ProtocolError::invalid(
                        "named inference dependencies exceed aggregate byte bound",
                    )
                })?;
            held.push(VerifiedFile::open(&file.path, &file.identity, deadline)?);
            identities.push(file.identity.clone());
        }
        // GPU observation is attached below only after actual kernel identity
        // checks. An owner policy by itself is not a hardware observation.
        let gpu = match profile.provider {
            InferenceProvider::Cpu => {
                if let Some(owner) = &self.cpu_owner_policy {
                    observe_cpu_policy_libraries(
                        owner,
                        &mut held,
                        &mut identities,
                        &mut total,
                        deadline,
                    )?;
                }
                None
            }
            InferenceProvider::Cuda => {
                let owner = self.cuda_owner_policy.as_ref().ok_or_else(|| {
                    ProtocolError::unsupported(
                        "CUDA environment inspection requires the configured owner policy",
                    )
                })?;
                owner
                    .policy()
                    .match_runtime(Some(&runtime))
                    .map_err(unavailable)?;
                let resources = profile.cuda_resources.as_ref().ok_or_else(|| {
                    ProtocolError::invalid(
                        "CUDA inspection requires the exact resource/device declaration",
                    )
                })?;
                if resources.device_uuid != owner.policy().device.device_uuid {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "CUDA inspection device differs from owner policy",
                    ));
                }
                let devices = crate::cuda_devices::PinnedCudaDevices::open(&owner.policy().device)
                    .map_err(unavailable)?;
                for library in &owner.policy().native_dependencies.libraries {
                    let identity = ArtifactIdentity {
                        artifact_id: ArtifactId::new(library.sha256.clone()).map_err(internal)?,
                        sha256: library.sha256.clone(),
                        byte_len: library.byte_len,
                    };
                    {
                        total = total
                            .checked_add(identity.byte_len)
                            .filter(|n| *n <= MAX_NATIVE_TOTAL_BYTES)
                            .ok_or_else(|| {
                                ProtocolError::invalid(
                                    "named CUDA dependencies exceed aggregate byte bound",
                                )
                            })?;
                        held.push(VerifiedFile::open(&library.path, &identity, deadline)?);
                        identities.push(identity);
                    }
                }
                Some((
                    devices,
                    crate::inference_host_observation::observe_nvidia_driver()
                        .map_err(unavailable)?,
                ))
            }
            _ => {
                return Err(ProtocolError::unsupported(
                    "inference environment provider observation",
                ))
            }
        };
        identities.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        identities.dedup();
        if let Some((devices, driver)) = &gpu {
            let expected = devices.expectation();
            host = host
                .with_gpu(InferenceGpuObservation {
                    device_uuid: expected.device_uuid.clone(),
                    pci_bus_address: expected.pci_bus_address.clone(),
                    device_minor: expected.device_minor,
                    kernel_driver: driver.clone(),
                })
                .map_err(unavailable)?;
        }
        let observed_at_unix_ms = wall_time()?;
        if observed_at_unix_ms < started {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "host observation wall clock moved backwards",
            ));
        }
        let report = InferenceEnvironmentReport {
            schema_version: 1,
            declared_profile: profile.clone(),
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(profile).map_err(internal)?)
            ),
            environment: InferenceEquivalenceEnvironment {
                engine: control.identity.clone(),
                worker: self.binary_identity.clone(),
                os: host.platform.os.clone(),
                architecture: host.platform.architecture.clone(),
                platform_build: host.platform.identity()?,
                hardware: host.hardware.identity()?,
                dependencies: identities,
            },
            platform: host.platform,
            hardware: host.hardware,
            observed_at_unix_ms,
            report_only: true,
            runtime_dependency_closure_verified: false,
            qualified: false,
        };
        report.validate_for_profile(profile)?;
        let held = HeldInferenceEnvironment {
            report,
            held,
            control,
            gpu,
            validity: ObservationValidity {
                deadline,
                wall_floor: AtomicU64::new(observed_at_unix_ms),
                invalidated: AtomicBool::new(false),
            },
            _memory: memory,
        };
        held.recheck()?;
        Ok(held)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    fn identity(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    fn validity(deadline: Instant) -> ObservationValidity {
        ObservationValidity {
            deadline,
            wall_floor: AtomicU64::new(wall_time().unwrap()),
            invalidated: AtomicBool::new(false),
        }
    }
    #[test]
    fn held_observation_never_refreshes_an_expired_deadline() {
        let proof = validity(Instant::now());
        let reached = AtomicBool::new(false);
        assert!(proof
            .check(|| {
                reached.store(true, Ordering::Release);
                Ok(())
            })
            .is_err());
        assert!(!reached.load(Ordering::Acquire));
        assert!(proof.invalidated.load(Ordering::Acquire));
        assert!(LoadedControlImage::open(Instant::now()).is_err());
    }
    #[test]
    fn held_observation_failure_is_latched_before_later_rechecks() {
        let proof = validity(Instant::now() + Duration::from_secs(1));
        proof.check(|| Ok(())).unwrap();
        let old_deadline = proof.deadline;
        let error = proof
            .check(|| {
                Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "authored changed host observation",
                ))
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DependencyMismatch);
        let reached = AtomicBool::new(false);
        assert!(proof
            .check(|| {
                reached.store(true, Ordering::Release);
                Ok(())
            })
            .is_err());
        assert!(!reached.load(Ordering::Acquire));
        assert_eq!(proof.deadline, old_deadline);
    }
    #[test]
    fn held_observation_detects_clock_rollback_against_its_latest_private_watermark() {
        let proof = validity(Instant::now() + Duration::from_secs(1));
        proof.check(|| Ok(())).unwrap();
        proof.wall_floor.store(i64::MAX as u64, Ordering::Release);
        assert!(proof
            .check(|| Ok(()))
            .unwrap_err()
            .message
            .contains("backwards"));
        assert!(proof.invalidated.load(Ordering::Acquire));
    }
    #[test]
    fn bounded_opened_hash_observes_actual_bytes_and_refuses_length_and_time_changes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("authored-hash-input");
        fs::write(&path, b"authored bytes").unwrap();
        assert_eq!(
            hash_opened(
                &mut artifacts::open_regular(&path).unwrap(),
                14,
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
            identity(b"authored bytes").sha256
        );
        assert!(hash_opened(
            &mut artifacts::open_regular(&path).unwrap(),
            13,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
        assert!(hash_opened(
            &mut artifacts::open_regular(&path).unwrap(),
            15,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
        assert!(hash_opened(
            &mut artifacts::open_regular(&path).unwrap(),
            14,
            Instant::now()
        )
        .is_err());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    #[ignore = "requires actual Linux host observations and local media tools; hashes loaded Engine but performs no native loading"]
    fn held_environment_retains_memory_and_engine_identity_until_drop() {
        struct Shutdown(Arc<Engine>);
        impl Drop for Shutdown {
            fn drop(&mut self) {
                self.0
                    .finish_shutdown()
                    .expect("join observation fixture shutdown");
            }
        }
        let root = tempfile::tempdir().unwrap();
        let owner = cpu_policy(root.path());
        let worker = root.path().join("authored-worker-never-executed");
        fs::write(
            &worker,
            b"held observation worker identity; no executable code",
        )
        .unwrap();
        let mut config = EngineConfig::new(root.path().join("state"), worker);
        config.max_ram_bytes = transfers::VALIDATION_MEMORY;
        config.cpu_owner_policy = Some(root.path().join("cpu-policy.json"));
        let engine = Engine::open(config).unwrap();
        let _shutdown = Shutdown(engine.clone());
        let _other_memory = engine
            .pool
            .reserve_memory(transfers::VALIDATION_MEMORY - 128 * 1024 * 1024)
            .unwrap();
        let request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let WorkerOperation::Generate { settings, .. } = request.operation else {
            panic!("authored generate profile");
        };
        let mut profile = settings.inference_profile.unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        profile.runtime = owner.runtime_identity().unwrap();
        let held = engine
            .observe_inference_environment(&profile, Instant::now() + Duration::from_secs(30))
            .unwrap();
        held.report().validate_for_profile(&profile).unwrap();
        assert_eq!(
            held.environment().engine,
            handy_qualification::control_image().unwrap()
        );
        assert_eq!(held.environment().worker, engine.binary_identity);
        assert_ne!(held.environment().engine, held.environment().worker);
        assert!(
            held.report().report_only
                && !held.report().qualified
                && !held.report().runtime_dependency_closure_verified
        );
        assert!(held.environment().dependencies.contains(owner.identity()));
        assert!(held
            .environment()
            .dependencies
            .contains(&owner.image().unwrap().identity().unwrap()));
        assert!(engine.pool.reserve_memory(1).is_err());
        let report = held.report().canonical_bytes().unwrap();
        held.recheck().unwrap();
        assert_eq!(held.report().canonical_bytes().unwrap(), report);
        let runtime = owner.runtime_reference().unwrap();
        let original = fs::read(&runtime.path).unwrap();
        fs::write(&runtime.path, vec![b'x'; original.len()]).unwrap();
        assert_eq!(
            held.recheck().unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        fs::write(&runtime.path, original).unwrap();
        assert!(held.recheck().is_err());
        assert!(engine.pool.reserve_memory(1).is_err());
        drop(held);
        assert!(engine.pool.reserve_memory(128 * 1024 * 1024).is_ok());
    }

    #[cfg(target_arch = "x86_64")]
    fn cpu_policy(root: &Path) -> crate::cpu_owner_policy::LoadedCpuOwnerPolicy {
        use std::os::unix::fs::PermissionsExt;
        let libraries = CPU_RUNTIME_REQUIRED_ALIASES
            .iter()
            .enumerate()
            .map(|(index, alias)| {
                let bytes = format!("authored unexecuted CPU dependency {index}");
                let path = root.join(format!("dependency-{index}"));
                fs::write(&path, bytes.as_bytes()).unwrap();
                let expected = identity(bytes.as_bytes());
                NativeLibrarySpec {
                    path,
                    sha256: expected.sha256,
                    byte_len: expected.byte_len,
                    names: vec![(*alias).into()],
                }
            })
            .collect();
        let policy = crate::cpu_owner_policy::CpuOwnerPolicy {
            schema_version: 1,
            native_dependencies: NativeDependencySpec {
                schema_version: 1,
                libraries,
            },
        };
        let path = root.join("cpu-policy.json");
        fs::write(&path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        crate::cpu_owner_policy::LoadedCpuOwnerPolicy::load(&path).unwrap()
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn environment_cpu_policy_observes_every_selected_library_and_rejects_later_drift() {
        let root = tempfile::tempdir().unwrap();
        let owner = cpu_policy(root.path());
        let mut held = Vec::new();
        let mut identities = Vec::new();
        let mut total = 0;
        observe_cpu_policy_libraries(
            &owner,
            &mut held,
            &mut identities,
            &mut total,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(
            held.len(),
            owner.policy().native_dependencies.libraries.len()
        );
        assert_eq!(
            total,
            owner.policy().native_dependencies.validate().unwrap()
        );
        assert!(identities.contains(owner.identity()));
        assert!(identities.contains(&owner.image().unwrap().identity().unwrap()));
        assert!(identities.contains(&owner.runtime_identity().unwrap()));
        for file in &held {
            file.recheck().unwrap();
        }
        let runtime = owner.runtime_reference().unwrap();
        fs::write(
            &runtime.path,
            vec![b'x'; runtime.identity.byte_len as usize],
        )
        .unwrap();
        assert!(held.iter().any(|file| file.recheck().is_err()));
        let mut next = Vec::new();
        assert!(observe_cpu_policy_libraries(
            &owner,
            &mut next,
            &mut Vec::new(),
            &mut 0,
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn environment_cpu_policy_reads_keep_the_existing_byte_and_deadline_bounds() {
        let root = tempfile::tempdir().unwrap();
        let owner = cpu_policy(root.path());
        let mut total = MAX_NATIVE_TOTAL_BYTES;
        assert!(observe_cpu_policy_libraries(
            &owner,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut total,
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
        assert!(observe_cpu_policy_libraries(
            &owner,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut 0,
            Instant::now()
        )
        .is_err());
    }
    #[test]
    fn environment_named_file_observation_refuses_replaced_or_modified_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        fs::write(&path, b"original").unwrap();
        let held = VerifiedFile::open(
            &path,
            &identity(b"original"),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        held.recheck().unwrap();
        fs::write(dir.path().join("replacement"), b"original").unwrap();
        fs::rename(dir.path().join("replacement"), &path).unwrap();
        assert_eq!(
            held.recheck().unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert!(VerifiedFile::open(
            &path,
            &identity(b"different"),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
        let held = VerifiedFile::open(
            &path,
            &identity(b"original"),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        fs::write(&path, b"mutated!").unwrap();
        assert!(held.recheck().is_err());
    }
    #[test]
    fn environment_named_file_observation_refuses_symlinks_expired_budget_and_wrong_digest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        fs::write(&path, b"original").unwrap();
        std::os::unix::fs::symlink(&path, dir.path().join("alias")).unwrap();
        assert!(VerifiedFile::open(
            &dir.path().join("alias"),
            &identity(b"original"),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
        assert!(VerifiedFile::open(&path, &identity(b"original"), Instant::now()).is_err());
        assert!(VerifiedFile::open(
            &path,
            &identity(b"changed!"),
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
    }
}
