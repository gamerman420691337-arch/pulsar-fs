//! CUDA attempt declarations. These carry expected identities, not permission,
//! byte verification, a resource reservation or proof of native execution.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Component, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedNativeLibrary {
    pub identity: ArtifactIdentity,
    pub names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity(n: u8) -> ArtifactIdentity {
        ArtifactIdentity {
            artifact_id: ArtifactId::new(format!("artifact-{n}")).unwrap(),
            sha256: format!("{n:064x}"),
            byte_len: 1,
        }
    }
    fn fixture() -> WorkerRequest {
        let model = SourceArtifact {
            source_version: SourceVersionId::new("model").unwrap(),
            path: crate::test_host_path("/captured/model").into(),
            identity: identity(9),
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        let uuid = "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351";
        let profile = DeclaredInferenceProfile {
            schema_version: 2,
            model: model.identity.clone(),
            runtime: identity(2),
            preprocessing: InferencePreprocessing {
                input: input.clone(),
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
                device_uuid: uuid.into(),
                device_ordinal: 0,
                address_space_bytes: 1024,
                device_reservation_bytes: 100,
                arena_limit_bytes: 50,
            }),
        };
        let execution = WorkerCudaExecution {
            schema_version: 1,
            owner_policy: identity(8),
            device_uuid: uuid.into(),
            pci_bus_address: "0000:01:00.0".into(),
            device_minor: 0,
            device_ordinal: 0,
            library_directory: crate::test_host_path("/captured/libraries").into(),
            libraries: [
                "ld-linux-x86-64.so.2",
                "libcuda.so.1",
                "libonnxruntime.so.1",
                "libonnxruntime_providers_cuda.so",
                "libonnxruntime_providers_shared.so",
                "libcudnn.so.9",
            ]
            .into_iter()
            .enumerate()
            .map(|(n, name)| CapturedNativeLibrary {
                identity: identity(n as u8),
                names: vec![name.into()],
            })
            .collect(),
        };
        let mut dependencies = vec![execution.owner_policy.clone(), model.identity.clone()];
        dependencies.extend(execution.libraries.iter().map(|lib| lib.identity.clone()));
        let tool = ArtifactReference {
            identity: identity(10),
            path: crate::test_host_path("/captured/tool").into(),
        };
        WorkerRequest {
            cuda_execution: Some(execution),
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(0),
            source: SourceArtifact {
                source_version: SourceVersionId::new("source").unwrap(),
                path: crate::test_host_path("/captured/source").into(),
                identity: identity(11),
            },
            independent_audio: None,
            output_dir: crate::test_host_path("/output").into(),
            operation: WorkerOperation::Generate {
                preset: "fast".into(),
                model: Some(model),
                settings: (GenerationSettings {
                    analysis_width: 64,
                    analysis_height: 64,
                    model_input: Some(input),
                    inference_profile: Some(profile),
                    ..Default::default()
                })
                .into(),
            },
            budget: ResourceBudget {
                memory_bytes: 1,
                output_bytes: 1,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies,
            tools: WorkerTools {
                ffmpeg: tool.clone(),
                ffprobe: tool,
                onnx_runtime: Some(ArtifactReference {
                    identity: identity(2),
                    path: crate::test_host_path("/captured/runtime").into(),
                }),
            },
        }
    }
    #[test]
    fn complete_cuda_request_roundtrips_and_requires_all_bindings() {
        let request = fixture();
        request.validate().unwrap();
        let bytes = serde_json::to_vec(&request).unwrap();
        let decoded: WorkerRequest = serde_json::from_slice(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
        let mut bad = request.clone();
        bad.cuda_execution = None;
        assert!(bad.validate().is_err());
        let mut bad = request.clone();
        bad.dependencies.remove(0);
        assert!(bad.validate().is_err());
        let mut bad = request.clone();
        bad.dependencies.pop();
        assert!(bad.validate().is_err());
        let mut bad = request.clone();
        bad.tools.onnx_runtime.as_mut().unwrap().identity = identity(12);
        assert!(bad.validate().is_err());
        let mut bad = request.clone();
        if let WorkerOperation::Generate { model, .. } = &mut bad.operation {
            model.as_mut().unwrap().identity = identity(12);
        }
        assert!(bad.validate().is_err());
        let mut bad = request.clone();
        if let WorkerOperation::Generate { settings, .. } = &mut bad.operation {
            settings.model_input.as_mut().unwrap().class_count = 2;
        }
        assert!(bad.validate().is_err());
        let mut bad = request;
        bad.cuda_execution.as_mut().unwrap().device_ordinal = 1;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn image_refuses_ambiguous_aliases_unbounded_bytes_and_paths() {
        let request = fixture();
        for mutate in [
            |e: &mut WorkerCudaExecution| e.libraries[0].names.push("../escape".into()),
            |e: &mut WorkerCudaExecution| e.libraries[0].names.push("libcuda.so.1".into()),
            |e: &mut WorkerCudaExecution| {
                e.libraries[0].identity.byte_len = 2 * 1024 * 1024 * 1024 + 1
            },
            |e: &mut WorkerCudaExecution| e.libraries[0].names = vec!["missing-loader".into()],
            |e: &mut WorkerCudaExecution| e.libraries[0].identity = e.libraries[1].identity.clone(),
            |e: &mut WorkerCudaExecution| {
                e.library_directory = crate::test_host_path("/capture/../supplier").into()
            },
            |e: &mut WorkerCudaExecution| e.pci_bus_address = "0000:01:20.0".into(),
            |e: &mut WorkerCudaExecution| e.pci_bus_address = "0000:01:00.8".into(),
            |e: &mut WorkerCudaExecution| e.pci_bus_address = "0000:01:é.0".into(),
            |e: &mut WorkerCudaExecution| e.owner_policy.byte_len = 1024 * 1024 + 1,
        ] {
            let mut bad = request.clone();
            mutate(bad.cuda_execution.as_mut().unwrap());
            assert!(bad.validate().is_err());
        }
        let mut bad = request.clone();
        bad.cuda_execution.as_mut().unwrap().libraries =
            vec![bad.cuda_execution.as_ref().unwrap().libraries[0].clone(); 129];
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(request).unwrap();
        value["cuda_execution"]["enable_gpu"] = true.into();
        assert!(serde_json::from_value::<WorkerRequest>(value).is_err());
    }
    #[test]
    fn cpu_request_omits_new_field_and_rejects_cuda_envelope() {
        let mut request = fixture();
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            let p = settings.inference_profile.as_mut().unwrap();
            p.schema_version = 1;
            p.provider = InferenceProvider::Cpu;
            p.cuda_resources = None;
        }
        assert!(request.validate().is_err());
        request.cuda_execution = None;
        request.validate().unwrap();
        let bytes = serde_json::to_vec(&request).unwrap();
        assert!(!String::from_utf8(bytes.clone())
            .unwrap()
            .contains("cuda_execution"));
        let decoded: WorkerRequest = serde_json::from_slice(&bytes).unwrap();
        assert!(decoded.cuda_execution.is_none());
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCudaExecution {
    pub schema_version: u16,
    /// Identity of the exact private policy bytes read at engine startup.
    pub owner_policy: ArtifactIdentity,
    pub device_uuid: String,
    pub pci_bus_address: String,
    /// Kernel character-device minor, independent of CUDA logical ordinal.
    pub device_minor: u32,
    pub device_ordinal: u32,
    /// Engine-owned captured directory; never an original supplier directory.
    pub library_directory: PathBuf,
    pub libraries: Vec<CapturedNativeLibrary>,
}

impl WorkerCudaExecution {
    /// Pure correspondence only. Callers must additionally hold reservations,
    /// verify captured bytes and mounted nodes, and enforce worker confinement.
    pub fn validate(
        &self,
        profile: &DeclaredInferenceProfile,
        budget: &ResourceBudget,
        dependencies: &[ArtifactIdentity],
    ) -> Result<(), ProtocolError> {
        let invalid = || ProtocolError::invalid("invalid CUDA worker execution declaration");
        profile.admit(budget)?;
        self.owner_policy.validate()?;
        validate_cuda_device_identity(self.device_ordinal, &self.device_uuid)?;
        let limits = profile.cuda_resources.as_ref().ok_or_else(invalid)?;
        let pci = self.pci_bus_address.as_bytes();
        if self.schema_version != 1
            || profile.provider != InferenceProvider::Cuda
            || self.owner_policy.byte_len == 0
            || self.owner_policy.byte_len > 1024 * 1024
            || self.device_minor > 63
            || limits.device_uuid != self.device_uuid
            || limits.device_ordinal != self.device_ordinal
            || pci.len() != 12
            || !pci.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b':',
                10 => *b == b'.',
                _ => b.is_ascii_hexdigit(),
            })
            || u8::from_str_radix(&self.pci_bus_address[8..10], 16).map_or(true, |v| v > 31)
            || u8::from_str_radix(&self.pci_bus_address[11..12], 16).map_or(true, |v| v > 7)
            || !self.library_directory.is_absolute()
            || self
                .library_directory
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            || self.libraries.is_empty()
            || self.libraries.len() > 128
            || !dependencies.contains(&self.owner_policy)
        {
            return Err(invalid());
        }
        let mut hashes = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for library in &self.libraries {
            library.identity.validate()?;
            if library.identity.byte_len == 0
                || library.identity.byte_len > 2 * 1024 * 1024 * 1024
                || !hashes.insert(&library.identity.sha256)
                || !identities.insert(library.identity.artifact_id.as_str())
                || !dependencies.contains(&library.identity)
                || library.names.is_empty()
                || library.names.len() > 4
            {
                return Err(invalid());
            }
            for name in &library.names {
                if name.is_empty()
                    || name.len() > 128
                    || name == "."
                    || name == ".."
                    || !name.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+')
                    })
                    || !names.insert(name.as_str())
                {
                    return Err(invalid());
                }
            }
            total = total
                .checked_add(library.identity.byte_len)
                .ok_or_else(invalid)?;
            if total > 8 * 1024 * 1024 * 1024 {
                return Err(invalid());
            }
        }
        for role in [
            "ld-linux-x86-64.so.2",
            "libcuda.so.1",
            "libonnxruntime.so.1",
            "libonnxruntime_providers_cuda.so",
            "libonnxruntime_providers_shared.so",
            "libcudnn.so.9",
        ] {
            if !names.contains(role) {
                return Err(invalid());
            }
        }
        if !self.libraries.iter().any(|lib| {
            lib.identity == profile.runtime
                && lib.names.iter().any(|name| name == "libonnxruntime.so.1")
        }) {
            return Err(invalid());
        }
        Ok(())
    }
}
