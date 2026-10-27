//! Shared profile admission for production workers and qualification campaigns.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};

fn model_contract(
    manifest: &WorkerRequest,
) -> Result<Option<(&SourceArtifact, &ModelInputContract)>> {
    let (model, input) = match &manifest.operation {
        WorkerOperation::Generate {
            model: Some(model),
            settings,
            ..
        } => (model, settings.model_input.as_ref()),
        WorkerOperation::Preview {
            model: Some(model),
            model_input,
            ..
        } => (model, model_input.as_ref()),
        _ => return Ok(None),
    };
    Ok(Some((
        model,
        input.context("model-backed operation requires explicit input contract")?,
    )))
}

pub fn admit_profile(profile: &DeclaredInferenceProfile, manifest: &WorkerRequest) -> Result<()> {
    admit_profile_identity(profile, manifest)?;
    ensure!(
        profile.provider == InferenceProvider::Cpu,
        "unsupported pinned provider; no automatic substitution"
    );
    if matches!(
        profile.preprocessing.resize,
        InferenceResize::AnnotatedTemporalView
    ) {
        // Only this source-bound file worker path implements the new recipe.
        return admit_tensor_adapter(profile, &manifest.budget);
    }
    admit_adapter(profile, &manifest.budget)
}

fn admit_profile_identity(
    profile: &DeclaredInferenceProfile,
    manifest: &WorkerRequest,
) -> Result<()> {
    profile.admit(&manifest.budget)?;
    let (model, input) =
        model_contract(manifest)?.context("profile requires model-backed operation")?;
    let runtime = manifest
        .tools
        .onnx_runtime
        .as_ref()
        .context("profile requires pinned runtime")?;
    ensure!(
        profile.model == model.identity && profile.runtime == runtime.identity,
        "profile model/runtime identity mismatch"
    );
    ensure!(
        serde_json::to_value(input)? == serde_json::to_value(&profile.preprocessing.input)?,
        "profile input contract mismatch"
    );
    if matches!(
        profile.preprocessing.resize,
        InferenceResize::AnnotatedTemporalView
    ) {
        let bound = match &manifest.operation {
            WorkerOperation::Generate { settings, .. } => settings.annotated_view.is_some(),
            WorkerOperation::Preview {
                annotated: Some(spec),
                ..
            } => {
                spec.validate_profile(profile)?;
                spec.view.source_sha256 == manifest.source.identity.sha256
            }
            _ => false,
        };
        ensure!(
            bound,
            "annotated profile requires source-bound indexed generation or preview settings"
        );
    }
    Ok(())
}

/// Metadata/profile correspondence only: used by the Engine with its capture
/// owner and by the private verified bootstrap. It does not authorize execution;
/// ordinary public profile admission and worker entry points remain CPU-only.
pub(crate) fn admit_cuda_worker_profile(manifest: &WorkerRequest) -> Result<()> {
    let selected = match &manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        _ => None,
    }
    .context("CUDA worker requires an explicit model profile")?;
    ensure!(
        selected.provider == InferenceProvider::Cuda && manifest.cuda_execution.is_some(),
        "CUDA worker requires its execution declaration"
    );
    admit_profile_identity(selected, manifest)?;
    admit_tensor_adapter(selected, &manifest.budget)
}

/// Immutable request/profile correspondence only. CUDA launch still requires
/// the verified private bootstrap; public worker entry points use resolve_profile.
pub(crate) fn resolve_bound_profile(
    manifest: &WorkerRequest,
) -> Result<Option<DeclaredInferenceProfile>> {
    manifest.validate()?;
    if manifest.cuda_execution.is_none() {
        return resolve_profile(manifest);
    }
    admit_cuda_worker_profile(manifest)?;
    Ok(match &manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.clone(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref().cloned(),
        _ => None,
    })
}

/// Checks the implemented adapter without loading supplier native code.
pub(crate) fn admit_adapter(
    profile: &DeclaredInferenceProfile,
    budget: &ResourceBudget,
) -> Result<()> {
    profile.admit(budget)?;
    ensure!(
        !matches!(
            profile.preprocessing.resize,
            InferenceResize::AnnotatedTemporalView
        ),
        "annotated preprocessing requires source-bound file worker admission"
    );
    ensure!(
        profile.provider == InferenceProvider::Cpu,
        "unsupported pinned provider; no automatic substitution"
    );
    admit_tensor_adapter(profile, budget)
}

/// Shared implemented tensor preprocessing only; this does not admit a provider.
pub(crate) fn admit_tensor_adapter(
    profile: &DeclaredInferenceProfile,
    budget: &ResourceBudget,
) -> Result<()> {
    profile.admit(budget)?;
    ensure!(
        profile.cpu_threads == budget.cpu_threads,
        "profile thread count differs from execution"
    );
    let p = &profile.preprocessing;
    ensure!(
        ((matches!(
            p.resize,
            InferenceResize::Stretch | InferenceResize::Letterbox
        ) && matches!(p.color, InferenceColor::Rgb)
            && matches!(p.input.decoder, DetectorDecoder::YoloChannelMajor)
            && p.input.channels == 3)
            || (matches!(
                p.resize,
                InferenceResize::NearestStretch | InferenceResize::AnnotatedTemporalView
            ) && matches!(p.color, InferenceColor::RgbFramePair)
                && matches!(p.input.decoder, DetectorDecoder::TemporalScriptPosition)
                && p.input.channels == 6))
            && matches!(p.input.layout, TensorLayout::Nchw)
            && p.input.width <= 1024
            && p.input.height <= 1024
            && p.input.class_count <= 256
            && p.scale.iter().all(|v| *v == 1.0f32 / 255.0)
            && p.bias.iter().all(|v| *v == 0.0),
        "preprocessing unsupported by implemented model adapter"
    );
    Ok(())
}

/// Explicit legacy/default adapter profile. Admission envelope is not measured
/// peak memory or quality qualification. Additional backends remain unsupported.
pub fn resolve_profile(manifest: &WorkerRequest) -> Result<Option<DeclaredInferenceProfile>> {
    let selected = match &manifest.operation {
        WorkerOperation::Generate { settings, .. } => settings.inference_profile.as_ref(),
        WorkerOperation::Preview {
            inference_profile, ..
        } => inference_profile.as_deref(),
        WorkerOperation::GenerateInput { .. } | WorkerOperation::ProbeMedia => None,
    };
    if let Some(profile) = selected {
        admit_profile(profile, manifest)?;
        return Ok(Some(profile.clone()));
    }
    let Some((model, input)) = model_contract(manifest)? else {
        return Ok(None);
    };
    let runtime = manifest
        .tools
        .onnx_runtime
        .as_ref()
        .context("model requires pinned runtime")?;
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: model.identity.clone(),
        runtime: runtime.identity.clone(),
        preprocessing: InferencePreprocessing {
            input: input.clone(),
            resize: if input.decoder == DetectorDecoder::TemporalScriptPosition {
                InferenceResize::NearestStretch
            } else {
                InferenceResize::Stretch
            },
            color: if input.decoder == DetectorDecoder::TemporalScriptPosition {
                InferenceColor::RgbFramePair
            } else {
                InferenceColor::Rgb
            },
            scale: vec![1.0 / 255.0; input.channels as usize],
            bias: vec![0.0; input.channels as usize],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 128 * 1024 * 1024,
        minimum_output_bytes: 1024 * 1024,
        cpu_threads: manifest.budget.cpu_threads,
        cuda_resources: None,
    };
    admit_profile(&profile, manifest)?;
    Ok(Some(profile))
}

pub fn profile_identity(profile: &DeclaredInferenceProfile) -> Result<ArtifactIdentity> {
    profile.validate()?;
    let bytes = serde_json::to_vec(profile)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len: bytes.len() as u64,
    })
}

/// Structural resource calculation only. Adapter admission precedes launch;
/// these declarations do not authorize GPU device access or native dependencies.
pub(crate) fn address_space_limit(
    profile: Option<&DeclaredInferenceProfile>,
    budget: &ResourceBudget,
) -> Result<u64> {
    budget.validate()?;
    let Some(profile) = profile else {
        return Ok(budget.memory_bytes);
    };
    profile.admit(budget)?;
    match profile.provider {
        InferenceProvider::Cpu => Ok(budget.memory_bytes),
        InferenceProvider::Cuda => Ok(profile
            .cuda_resources
            .as_ref()
            .context("CUDA address space requires an explicit version-2 resource declaration")?
            .address_space_bytes),
        _ => anyhow::bail!("unsupported address-space adapter"),
    }
}

#[cfg(test)]
mod memory_limit_tests {
    use super::*;
    #[test]
    fn annotated_cuda_correspondence_does_not_admit_an_ordinary_worker_or_unbound_profile() {
        let mut request: WorkerRequest =
            serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json"))
                .unwrap();
        let view: AnnotatedGenerationView = serde_json::from_value(serde_json::json!({
            "source_sha256": request.source.identity.sha256,
            "declaration": {"schema_version":1,"source_dimensions":[64,64],
                "analysis_dimensions":[64,64],"target_region":[0,0,64,64],
                "target_id":"authored","video_view":{"layout":"mono","eye":"left",
                    "projection":{"kind":"flat"}}}
        }))
        .unwrap();
        let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
            panic!()
        };
        settings.annotated_view = Some(view);
        settings
            .inference_profile
            .as_mut()
            .unwrap()
            .preprocessing
            .resize = InferenceResize::AnnotatedTemporalView;
        request.validate().unwrap();
        resolve_bound_profile(&request).unwrap().unwrap();
        assert!(resolve_profile(&request).is_err());
        let mut unbound = request.clone();
        unbound.cuda_execution = None;
        assert!(resolve_bound_profile(&unbound).is_err());
        for changed in ["runtime", "recipe", "resources", "provider", "source"] {
            let mut bad = request.clone();
            let WorkerOperation::Generate { settings, .. } = &mut bad.operation else {
                panic!()
            };
            let profile = settings.inference_profile.as_mut().unwrap();
            match changed {
                "runtime" => profile.runtime.byte_len += 1,
                "recipe" => profile.preprocessing.padding_byte = 1,
                "resources" => {
                    profile.schema_version = 1;
                    profile.cuda_resources = None;
                }
                "provider" => {
                    profile.schema_version = 1;
                    profile.cuda_resources = None;
                    profile.provider = InferenceProvider::TensorRt;
                }
                "source" => {
                    settings.annotated_view.as_mut().unwrap().source_sha256 = "b".repeat(64)
                }
                _ => unreachable!(),
            }
            assert!(resolve_bound_profile(&bad).is_err(), "{changed}");
        }
    }
    #[test]
    fn virtual_limit_is_separate_and_gpu_declaration_does_not_admit_execution() {
        let budget = ResourceBudget {
            memory_bytes: 1536 * 1024 * 1024,
            output_bytes: 1024 * 1024,
            wall_time_ms: 1,
            cpu_threads: 2,
        };
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let mut profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        };
        profile.preprocessing.resize = InferenceResize::Letterbox;
        profile.preprocessing.padding_byte = 114;
        admit_adapter(&profile, &budget).unwrap();
        for changed in ["color", "decoder", "scale", "layout"] {
            let mut bad = profile.clone();
            match changed {
                "color" => bad.preprocessing.color = InferenceColor::Bgr,
                "decoder" => bad.preprocessing.input.decoder = DetectorDecoder::YoloEndToEnd,
                "scale" => bad.preprocessing.scale[0] = 1.0,
                "layout" => bad.preprocessing.input.layout = TensorLayout::Nhwc,
                _ => unreachable!(),
            }
            assert!(admit_adapter(&bad, &budget).is_err(), "{changed}");
        }
        assert_eq!(
            address_space_limit(None, &budget).unwrap(),
            budget.memory_bytes
        );
        assert_eq!(
            address_space_limit(Some(&profile), &budget).unwrap(),
            budget.memory_bytes
        );
        profile.provider = InferenceProvider::Cuda;
        assert!(address_space_limit(Some(&profile), &budget).is_err());
        profile.schema_version = 2;
        profile.cuda_resources = Some(CudaResourceLimits {
            device_ordinal: 0,
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            address_space_bytes: 16 * 1024u64.pow(3),
            device_reservation_bytes: 1024u64.pow(3),
            arena_limit_bytes: 64 * 1024u64.pow(2),
        });
        assert_eq!(
            address_space_limit(Some(&profile), &budget).unwrap(),
            16 * 1024u64.pow(3)
        );
        assert!(admit_adapter(&profile, &budget)
            .unwrap_err()
            .to_string()
            .contains("unsupported pinned provider"));
    }
}
