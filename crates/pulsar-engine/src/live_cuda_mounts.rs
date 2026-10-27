//! Live-specific mount custody. No offline job or inference authority is created.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LiveCudaMountWitness {
    pub(crate) manifest: LiveCudaInferenceManifest,
    pub(crate) manifest_sha256: String,
    pub(crate) deadline_ns: u64,
    pub(crate) aliases: Vec<MountedNativeAlias>,
    pub(crate) devices: Vec<DeviceNodeIdentity>,
    pub(crate) module_states: Vec<MountedCudaModule>,
}
fn correspondence(witness: &LiveCudaMountWitness) -> Result<()> {
    ensure!(
        witness.manifest_sha256 == witness.manifest.identity_sha256()?,
        "CUDA live mount manifest identity differs"
    );
    crate::inference_profiles::admit_tensor_adapter(
        &witness.manifest.stream.profile,
        &witness.manifest.stream.budget,
    )?;
    crate::cuda_bootstrap::deadline_instant(
        witness.deadline_ns,
        witness.manifest.stream.budget.wall_time_ms,
    )?;
    mount_correspondence(
        &witness.manifest.execution,
        &witness.aliases,
        &witness.devices,
        &witness.module_states,
    )
}

/// Owns only mounted assets and admitted process custody. A declaration is not
/// a fresh GPU observation or reservation. Production stream launch is separate.
pub(crate) struct OwnedLiveCudaMountCommand {
    command: Command,
    pub(crate) custody: crate::worker_retirement::WorkerCustody,
    pub(crate) witness: LiveCudaMountWitness,
}
/// Captured image and device pins, moved into custody before fallible setup.
struct LiveMountInputs {
    image: crate::native_capture::SupervisedNativeDependencies,
    devices: PinnedCudaDevices,
}
enum LiveMountRole {
    #[cfg(test)]
    Inspection,
    Memory,
    Stream,
}
impl OwnedLiveCudaMountCommand {
    #[cfg(test)]
    pub(crate) fn inspection(
        config: &EngineConfig,
        manifest: &LiveCudaInferenceManifest,
        deadline_ns: u64,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
    ) -> Result<Self> {
        Self::prepare(
            config,
            manifest,
            deadline_ns,
            LiveMountInputs { image, devices },
            custody,
            LiveMountRole::Inspection,
            &[],
        )
    }
    pub(crate) fn memory_observer(
        config: &EngineConfig,
        manifest: &LiveCudaInferenceManifest,
        deadline_ns: u64,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
    ) -> Result<Self> {
        Self::prepare(
            config,
            manifest,
            deadline_ns,
            LiveMountInputs { image, devices },
            custody,
            LiveMountRole::Memory,
            &[],
        )
    }
    pub(crate) fn stream(
        config: &EngineConfig,
        manifest: &LiveCudaInferenceManifest,
        deadline_ns: u64,
        image: crate::native_capture::SupervisedNativeDependencies,
        devices: PinnedCudaDevices,
        custody: crate::worker_retirement::WorkerCustody,
        model: &std::path::Path,
    ) -> Result<Self> {
        Self::prepare(
            config,
            manifest,
            deadline_ns,
            LiveMountInputs { image, devices },
            custody,
            LiveMountRole::Stream,
            &[(model.to_owned(), manifest.stream.model.path.clone())],
        )
    }
    fn prepare(
        config: &EngineConfig,
        manifest: &LiveCudaInferenceManifest,
        deadline_ns: u64,
        inputs: LiveMountInputs,
        mut custody: crate::worker_retirement::WorkerCustody,
        role: LiveMountRole,
        bindings: &[(std::path::PathBuf, std::path::PathBuf)],
    ) -> Result<Self> {
        let LiveMountInputs { image, devices } = inputs;
        let group = custody.group_handle();
        // Install owners before validation or any other fallible mount setup.
        let assets = custody.retain_asset(OwnedCudaAssets {
            image,
            devices,
            descriptors: Vec::new(),
        });
        manifest.validate()?;
        match role {
            #[cfg(test)]
            LiveMountRole::Inspection => {}
            _ => assets.image.captured().verify_loader_ready()?,
        }
        let mounts = pin_mounts(
            &manifest.execution,
            assets.image.captured(),
            &assets.devices,
        )?;
        let witness = LiveCudaMountWitness {
            manifest: manifest.clone(),
            manifest_sha256: manifest.identity_sha256()?,
            deadline_ns,
            aliases: mounts.image.witnesses(),
            devices: assets.devices.identities().into_iter().cloned().collect(),
            module_states: mounts
                .modules
                .iter()
                .map(|(_, value)| value.clone())
                .collect(),
        };
        correspondence(&witness)?;
        let address_space = manifest
            .stream
            .profile
            .cuda_resources
            .as_ref()
            .context("CUDA live resources missing")?
            .address_space_bytes;
        let command = crate::confinement::cuda_command(
            config,
            crate::confinement::ConfinementLimits {
                budget: &manifest.stream.budget,
                address_space_bytes: address_space,
            },
            &group,
            bindings,
            &manifest.output_dir,
            match role {
                #[cfg(test)]
                LiveMountRole::Inspection => "cuda-live-mount-check",
                LiveMountRole::Memory => "cuda-live-memory-observer",
                LiveMountRole::Stream => "cuda-live-worker",
            },
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
    pub(crate) fn into_parts(
        self,
    ) -> (
        Command,
        crate::worker_retirement::WorkerCustody,
        LiveCudaMountWitness,
    ) {
        (self.command, self.custody, self.witness)
    }
    pub(crate) fn spawn(&mut self) -> std::io::Result<()> {
        self.custody.spawn(&mut self.command)
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
}

/// Proof of fresh mounted-byte verification, never scheduling authority.
pub(crate) struct VerifiedLiveCudaBootstrap<'a> {
    witness: &'a LiveCudaMountWitness,
    deadline: Instant,
}
impl VerifiedLiveCudaBootstrap<'_> {
    pub(crate) fn manifest(&self) -> &LiveCudaInferenceManifest {
        &self.witness.manifest
    }
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn verify_after(&self) -> Result<()> {
        correspondence(self.witness)?;
        verify_mount_bytes(
            &self.witness.manifest.execution,
            &self.witness.aliases,
            &self.witness.devices,
            &self.witness.module_states,
            self.deadline,
        )?;
        ensure!(
            Instant::now() < self.deadline,
            "CUDA live bootstrap deadline expired"
        );
        Ok(())
    }
}
pub(crate) fn verify_live_bootstrap(
    witness: &LiveCudaMountWitness,
) -> Result<VerifiedLiveCudaBootstrap<'_>> {
    let deadline = crate::cuda_bootstrap::deadline_instant(
        witness.deadline_ns,
        witness.manifest.stream.budget.wall_time_ms,
    )?;
    let proof = VerifiedLiveCudaBootstrap { witness, deadline };
    proof.verify_after()?;
    Ok(proof)
}

/// Hash-only inspection in a fresh confined child. It never loads native code,
/// opens a GPU for IO, admits a stream, or returns motion.
pub fn run_live_checker() -> Result<()> {
    clear_inherited_descriptors()?;
    let witness: LiveCudaMountWitness = read_message(&mut std::io::stdin().lock())?;
    correspondence(&witness)?;
    let deadline = crate::cuda_bootstrap::deadline_instant(
        witness.deadline_ns,
        witness.manifest.stream.budget.wall_time_ms,
    )?;
    verify_mount_bytes(
        &witness.manifest.execution,
        &witness.aliases,
        &witness.devices,
        &witness.module_states,
        deadline,
    )?;
    ensure!(
        Instant::now() < deadline,
        "CUDA live mount verification deadline expired"
    );
    println!(
        "{}",
        serde_json::json!({"state":"passed", "manifest_sha256":witness.manifest_sha256,
        "deadline_ns":witness.deadline_ns, "aliases":witness.aliases.len(), "device_nodes":witness.devices.len(),
        "native_execution":false,"gpu_admission":false,"inherited_descriptors_erased":true})
    );
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn witness() -> LiveCudaMountWitness {
        let offline = super::super::tests::witness();
        let mut stream: LiveInferenceManifest = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
        ))
        .unwrap();
        let profile = match offline.request.operation {
            WorkerOperation::Generate { settings, .. } => settings.inference_profile.unwrap(),
            _ => unreachable!(),
        };
        stream.model.identity = profile.model;
        stream.runtime.identity = profile.runtime.clone();
        stream.profile.model = stream.model.identity.clone();
        stream.profile.runtime = profile.runtime;
        stream.profile.schema_version = 2;
        stream.profile.provider = InferenceProvider::Cuda;
        stream.profile.cuda_resources = profile.cuda_resources;
        stream.budget = offline.request.budget;
        stream.profile.cpu_threads = stream.budget.cpu_threads;
        let mut execution = offline.request.cuda_execution.unwrap();
        let nvml = ArtifactIdentity {
            artifact_id: ArtifactId::new("e".repeat(64)).unwrap(),
            sha256: "e".repeat(64),
            byte_len: 1,
        };
        execution.libraries.push(CapturedNativeLibrary {
            identity: nvml.clone(),
            names: vec!["libnvidia-ml.so.1".into()],
        });
        stream.runtime.path = execution.library_directory.join("libonnxruntime.so.1");
        let mut dependencies = offline.request.dependencies;
        dependencies.extend([
            stream.model.identity.clone(),
            stream.runtime.identity.clone(),
            stream.worker.clone(),
            nvml.clone(),
        ]);
        let manifest = LiveCudaInferenceManifest {
            schema_version: 1,
            stream,
            execution,
            dependencies,
            output_dir: "/work".into(),
        };
        let mut aliases = offline.aliases;
        aliases.push(MountedNativeAlias {
            name: "libnvidia-ml.so.1".into(),
            sha256: nvml.sha256,
            byte_len: 1,
            filesystem_device: 1,
            inode: 1,
        });
        LiveCudaMountWitness {
            manifest_sha256: manifest.identity_sha256().unwrap(),
            deadline_ns: crate::cuda_bootstrap::deadline_ns(manifest.stream.budget.wall_time_ms)
                .unwrap(),
            manifest,
            aliases,
            devices: offline.devices,
            module_states: offline.module_states,
        }
    }
    #[test]
    fn live_mount_correspondence_binds_manifest_mounts_and_inherited_deadline() {
        correspondence(&witness()).unwrap();
        let mut bad = witness();
        bad.manifest_sha256 = "0".repeat(64);
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.manifest.stream.attempt_id = AttemptId::new("changed").unwrap();
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.aliases[0].sha256 = "0".repeat(64);
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.aliases.push(bad.aliases[0].clone());
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.devices[1].minor += 1;
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.module_states.pop();
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.deadline_ns = crate::cuda_bootstrap::now_ns().unwrap() - 1;
        assert!(correspondence(&bad).is_err());
        let mut bad = witness();
        bad.deadline_ns = crate::cuda_bootstrap::deadline_ns(86_400_000).unwrap();
        assert!(correspondence(&bad).is_err());
    }
    #[test]
    fn live_mount_witness_cannot_enter_offline_or_ordinary_cpu_contracts() {
        let witness = witness();
        assert!(witness.manifest.stream.validate().is_err());
        let bytes = serde_json::to_vec(&witness).unwrap();
        assert!(serde_json::from_slice::<CudaMountWitness>(&bytes).is_err());
        let decoded: LiveCudaMountWitness = serde_json::from_slice(&bytes).unwrap();
        correspondence(&decoded).unwrap();
        let mut value = serde_json::to_value(decoded).unwrap();
        value["approved"] = serde_json::json!(true);
        assert!(serde_json::from_value::<LiveCudaMountWitness>(value).is_err());
    }
}
