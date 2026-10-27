//! Narrow provider-list observations. No retry, fallback or qualification authority.
use pulsar_protocol::*;

#[derive(Debug)]
struct ProviderNotListed {
    provider: InferenceProvider,
}
impl std::fmt::Display for ProviderNotListed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "selected {:?} provider is not listed by the initialized runtime",
            self.provider
        )
    }
}
impl std::error::Error for ProviderNotListed {}

/// Call only after the existing driver checks and pinned ORT initialization.
#[cfg(all(target_os = "linux", feature = "cuda"))]
pub(super) fn require_cuda_available(provider: &ort::ep::CUDA) -> anyhow::Result<()> {
    use ort::ep::ExecutionProvider;
    observe_availability(InferenceProvider::Cuda, provider.is_available())
}

#[cfg(any(test, all(target_os = "linux", feature = "cuda")))]
fn observe_availability<E: std::fmt::Display>(
    provider: InferenceProvider,
    result: std::result::Result<bool, E>,
) -> anyhow::Result<()> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(ProviderNotListed { provider }.into()),
        Err(error) => Err(anyhow::anyhow!(
            "runtime provider enumeration failed: {error}"
        )),
    }
}

pub(super) fn worker_failure(
    request: &WorkerRequest,
    error: anyhow::Error,
) -> (ProtocolError, Option<InferenceProviderUnavailable>) {
    let failure = error
        .downcast_ref::<ProtocolError>()
        .cloned()
        .unwrap_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                format!("worker attempt failed: {error:#}"),
            )
        });
    let evidence = bound_observation(request, &error, &failure);
    (failure, evidence)
}

fn bound_observation(
    request: &WorkerRequest,
    error: &anyhow::Error,
    failure: &ProtocolError,
) -> Option<InferenceProviderUnavailable> {
    let unavailable = error.downcast_ref::<ProviderNotListed>()?;
    let profile = match &request.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    }?;
    if unavailable.provider != profile.provider {
        return None;
    }
    let profile_identity = crate::inference_profiles::profile_identity(profile).ok()?;
    let evidence = InferenceProviderUnavailable {
        schema_version: 1,
        provider: unavailable.provider,
        profile_sha256: profile_identity.sha256,
        check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
    };
    let response = WorkerResult {
        cuda_artifacts: None,
        provider_unavailable: Some(evidence.clone()),
        version: PROTOCOL_VERSION,
        attempt_id: request.attempt_id.clone(),
        job_id: request.job_id.clone(),
        result: Err(failure.clone()),
    };
    response.validate_provider_unavailable_for(request).ok()?;
    Some(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> WorkerRequest {
        // Pure declaration only. No runtime, source, model or device is opened.
        let path = |leaf: &str| {
            if cfg!(windows) {
                std::path::PathBuf::from(format!("C:\\fixture\\{leaf}"))
            } else {
                std::path::PathBuf::from(format!("/fixture/{leaf}"))
            }
        };
        let identity = |name: &str| ArtifactIdentity {
            artifact_id: ArtifactId::new(name).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 12,
        };
        let artifact = |name: &str| ArtifactReference {
            path: path(name),
            identity: identity(name),
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity("model"),
            runtime: identity("runtime"),
            preprocessing: InferencePreprocessing {
                input: input.clone(),
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let source = |name: &str| SourceArtifact {
            source_version: SourceVersionId::new(name).unwrap(),
            path: path(name),
            identity: identity(name),
        };
        let request = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("attempt").unwrap(),
            job_id: JobId::new("job").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            base_revision: RevisionId::new(0),
            source: source("source"),
            output_dir: path("output"),
            operation: WorkerOperation::Generate {
                preset: "default".into(),
                model: Some(source("model")),
                settings: Box::new(GenerationSettings {
                    model_input: Some(input),
                    inference_profile: Some(profile),
                    ..GenerationSettings::default()
                }),
            },
            budget: ResourceBudget {
                memory_bytes: 4096,
                output_bytes: 1024,
                wall_time_ms: 1000,
                cpu_threads: 1,
            },
            dependencies: vec![identity("model")],
            tools: WorkerTools {
                ffmpeg: artifact("ffmpeg"),
                ffprobe: artifact("ffprobe"),
                onnx_runtime: Some(artifact("runtime")),
            },
        };
        request.validate().unwrap();
        request
    }

    #[test]
    fn only_a_successful_negative_provider_query_creates_the_typed_error() {
        assert!(observe_availability(InferenceProvider::Cuda, Ok::<_, &str>(true)).is_ok());
        let absent =
            observe_availability(InferenceProvider::Cuda, Ok::<_, &str>(false)).unwrap_err();
        assert_eq!(
            absent.downcast_ref::<ProviderNotListed>().unwrap().provider,
            InferenceProvider::Cuda
        );
        let failed = observe_availability(
            InferenceProvider::Cuda,
            Err::<bool, _>("not listed by runtime"),
        )
        .unwrap_err();
        assert!(failed.downcast_ref::<ProviderNotListed>().is_none());
    }

    #[test]
    fn cpu_api_never_supplies_a_negative_runtime_list_observation() {
        use ort::ep::ExecutionProvider;
        // ort's CPU override returns Ok(true) without loading a runtime.
        assert!(ort::ep::CPU::default().is_available().unwrap());
    }

    #[test]
    fn typed_error_routing_retains_context_but_never_matches_error_text() {
        let request = fixture();
        // Authored routing input, not an actual CPU availability observation.
        let typed = anyhow::Error::new(ProviderNotListed {
            provider: InferenceProvider::Cpu,
        })
        .context("authored outer context");
        let same_text = format!("{typed:#}");
        let (failure, evidence) = worker_failure(&request, typed);
        assert_eq!(failure.code, ErrorCode::Unavailable);
        assert!(!failure.retryable);
        let evidence = evidence.expect("exact typed authored observation");
        let WorkerOperation::Generate { settings, .. } = &request.operation else {
            unreachable!()
        };
        let profile = settings.inference_profile.as_ref().unwrap();
        assert_eq!(
            evidence.profile_sha256,
            crate::inference_profiles::profile_identity(profile)
                .unwrap()
                .sha256
        );
        for text in [
            same_text.as_str(),
            "runtime initialization failed",
            "provider registration failed",
            "model shape mismatch",
            "resource exhausted",
            "dependency mismatch",
        ] {
            assert!(worker_failure(&request, anyhow::anyhow!("{text}"))
                .1
                .is_none());
        }
        let known = ProtocolError::new(ErrorCode::DependencyMismatch, "not listed by runtime");
        let (failure, evidence) = worker_failure(&request, known.clone().into());
        assert_eq!(failure, known);
        assert!(evidence.is_none());
    }

    #[test]
    fn implicit_profiles_and_mismatched_provider_or_dependencies_remain_unclassified() {
        let mut request = fixture();
        let failure = || {
            anyhow::Error::new(ProviderNotListed {
                provider: InferenceProvider::Cpu,
            })
        };
        assert!(worker_failure(
            &request,
            anyhow::Error::new(ProviderNotListed {
                provider: InferenceProvider::Cuda
            })
        )
        .1
        .is_none());
        request.tools.onnx_runtime.as_mut().unwrap().identity.sha256 = "b".repeat(64);
        assert!(worker_failure(&request, failure()).1.is_none());
        let mut request = fixture();
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            settings.inference_profile = None;
        }
        request.validate().unwrap();
        assert!(worker_failure(&request, failure()).1.is_none());
    }
}
