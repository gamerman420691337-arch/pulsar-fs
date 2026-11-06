//! Presentation-side loading only. The engine admits identities and resources.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Read, path::Path};

/// Choose dimensions only when authoring a new request. Persisted requests keep
/// their concrete dimensions; the worker still owns view fitting and validation.
pub(crate) fn generation_analysis_dimensions(
    input: Option<&ModelInputContract>,
    width: Option<u32>,
    height: Option<u32>,
) -> (u32, u32) {
    let defaults = match input {
        Some(input) if input.decoder == DetectorDecoder::TemporalScriptPosition => (64, 64),
        Some(input) => (input.width, input.height),
        None => (320, 180),
    };
    (width.unwrap_or(defaults.0), height.unwrap_or(defaults.1))
}

#[cfg(test)]
mod generation_dimension_tests {
    use super::*;

    #[test]
    fn new_requests_use_declared_dimensions_and_preserve_partial_overrides() {
        let mut input = ModelInputContract {
            width: 640,
            height: 384,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 80,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        assert_eq!(generation_analysis_dimensions(None, None, None), (320, 180));
        assert_eq!(
            generation_analysis_dimensions(Some(&input), None, None),
            (640, 384)
        );
        assert_eq!(
            generation_analysis_dimensions(Some(&input), Some(200), None),
            (200, 384)
        );
        assert_eq!(
            generation_analysis_dimensions(Some(&input), None, Some(100)),
            (640, 100)
        );
        assert_eq!(
            generation_analysis_dimensions(Some(&input), Some(320), Some(180)),
            (320, 180)
        );
        input.decoder = DetectorDecoder::TemporalScriptPosition;
        assert_eq!(
            generation_analysis_dimensions(Some(&input), None, None),
            (64, 64)
        );
    }

    #[test]
    fn explicit_invalid_dimensions_reach_validation_without_silent_repair() {
        for (width, height) in [(0, 180), (320, 0), (16385, 180), (320, 16385)] {
            let dimensions = generation_analysis_dimensions(None, Some(width), Some(height));
            assert_eq!(dimensions, (width, height));
            assert!(GenerationSettings {
                analysis_width: dimensions.0,
                analysis_height: dimensions.1,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
    }
}

pub fn read_profile(path: &Path) -> Result<DeclaredInferenceProfile> {
    let bytes = read_bounded_profile(path)?;
    let profile: DeclaredInferenceProfile =
        serde_json::from_slice(&bytes).context("parse inference profile")?;
    profile.validate()?;
    Ok(profile)
}

/// Load an explicit source-content binding; never infer a target from model metadata.
pub fn read_annotated_view(path: &Path) -> Result<AnnotatedGenerationView> {
    let binding: AnnotatedGenerationView = serde_json::from_slice(&read_bounded_profile(path)?)
        .context("parse annotated generation view")?;
    binding.validate()?;
    Ok(binding)
}
pub fn read_stash_annotated_views(path: &Path) -> Result<Vec<StashBatchAnnotatedView>> {
    let entries: Vec<StashBatchAnnotatedView> =
        serde_json::from_slice(&read_bounded_profile(path)?)
            .context("parse Stash annotated view catalog")?;
    ensure!(
        !entries.is_empty() && entries.len() <= 100,
        "Stash annotated catalog requires 1..=100 entries"
    );
    for entry in &entries {
        entry.view.validate()?;
    }
    Ok(entries)
}
pub fn read_stash_source_plans(path: &Path) -> Result<Vec<StashBatchSourcePlan>> {
    let entries: Vec<StashBatchSourcePlan> = serde_json::from_slice(&read_bounded_profile(path)?)
        .context("parse Stash source plan catalog")?;
    ensure!(
        !entries.is_empty() && entries.len() <= 100,
        "Stash source plans require 1..=100 entries"
    );
    for entry in &entries {
        entry.validate()?;
    }
    Ok(entries)
}

pub(crate) fn read_bounded_profile(path: &Path) -> Result<Vec<u8>> {
    const LIMIT: u64 = 64 * 1024;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    let file = options.open(path).context("open profile")?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= LIMIT,
        "profile must be a regular file of at most 64 KiB"
    );
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= LIMIT, "profile exceeds 64 KiB");
    Ok(bytes)
}

pub fn custom_profile(
    catalog: &InferenceCatalog,
    model: &Path,
    width: u32,
    height: u32,
    class_count: u32,
) -> Result<DeclaredInferenceProfile> {
    ensure!(
        catalog.schema_version == 1 && catalog.providers.contains(&InferenceProvider::Cpu),
        "engine does not expose the CPU adapter"
    );
    ensure!(
        width <= catalog.maximum_input_dimension
            && height <= catalog.maximum_input_dimension
            && class_count <= catalog.maximum_class_count,
        "model input exceeds engine adapter limits"
    );
    let runtime = catalog
        .runtime
        .clone()
        .context("engine has no pinned ONNX runtime")?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    let mut file = options.open(model)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= catalog.maximum_model_bytes,
        "model exceeds engine regular-file snapshot bound"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut byte_len = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        byte_len += n as u64;
        ensure!(
            byte_len <= metadata.len(),
            "model changed during profile creation"
        );
        hash.update(&buffer[..n]);
    }
    ensure!(
        byte_len == metadata.len(),
        "model changed during profile creation"
    );
    let sha256 = format!("{:x}", hash.finalize());
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len,
        },
        runtime,
        preprocessing: InferencePreprocessing {
            input: ModelInputContract {
                width,
                height,
                channels: 3,
                layout: TensorLayout::Nchw,
                class_count,
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
        minimum_output_bytes: 1024 * 1024,
        cpu_threads: catalog.cpu_threads,
        cuda_resources: None,
    };
    profile.admit(&ResourceBudget {
        memory_bytes: catalog.memory_bytes,
        output_bytes: catalog.output_bytes,
        cpu_threads: catalog.cpu_threads,
        wall_time_ms: 1,
    })?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_custom_profile_binds_engine_runtime_and_model_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.onnx");
        std::fs::write(&model, b"fixture bytes; not a trained model").unwrap();
        let mut catalog = InferenceCatalog {
            schema_version: 1,
            worker_available: true,
            runtime: Some(ArtifactIdentity {
                artifact_id: ArtifactId::new("runtime").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 1,
            }),
            providers: vec![InferenceProvider::Cpu],
            maximum_input_dimension: 1024,
            maximum_class_count: 256,
            maximum_model_bytes: 1024,
            cpu_threads: 2,
            memory_bytes: 1536 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            qualified_profiles: vec![],
            qualified_fallback_available: false,
            limitation: "custom".into(),
        };
        let profile = custom_profile(&catalog, &model, 64, 64, 1).unwrap();
        assert_eq!(profile.runtime, catalog.runtime.clone().unwrap());
        assert_eq!(
            profile.model.sha256,
            format!("{:x}", Sha256::digest(std::fs::read(&model).unwrap()))
        );
        let selected = dir.path().join("profile.json");
        std::fs::write(&selected, serde_json::to_vec(&profile).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(read_profile(&selected).unwrap()).unwrap(),
            serde_json::to_value(&profile).unwrap()
        );
        assert!(custom_profile(&catalog, &model, 1025, 64, 1).is_err());
        catalog.runtime = None;
        assert!(custom_profile(&catalog, &model, 64, 64, 1).is_err());
    }

    #[test]
    fn annotated_view_loader_preserves_binding_and_rejects_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("view.json");
        let mut value = serde_json::json!({"source_sha256":"a".repeat(64),"declaration":{"schema_version":1,
            "source_dimensions":[192,64],"analysis_dimensions":[96,64],"target_region":[16,0,64,64],"target_id":"authored",
            "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}});
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(read_annotated_view(&path).unwrap()).unwrap(),
            value
        );
        value["unexpected"] = true.into();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_annotated_view(&path).is_err());
        std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_annotated_view(&path).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read_annotated_view(&link).is_err());
        }
    }

    #[test]
    fn stash_source_plan_loader_preserves_explicit_intent_and_refuses_unsafe_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plans.json");
        let entry = serde_json::json!({"scene_id":"s1","file_id":"f1","source_sha256":"a".repeat(64),
            "placement":null,"project_interval":{"start":0,"end":1000000000},"source_time":false});
        std::fs::write(&path, serde_json::to_vec(&vec![entry.clone()]).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(read_stash_source_plans(&path).unwrap()).unwrap(),
            serde_json::json!([entry])
        );
        std::fs::write(&path, b"[]").unwrap();
        assert!(read_stash_source_plans(&path).is_err());
        std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_stash_source_plans(&path).is_err());
        assert!(read_stash_source_plans(dir.path()).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read_stash_source_plans(&link).is_err());
        }
    }

    #[test]
    fn rejects_oversized_and_nonregular_profiles() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_profile(dir.path()).is_err());
        let path = dir.path().join("profile.json");
        std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_profile(&path).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(read_profile(&link).is_err());
        }
    }
}
