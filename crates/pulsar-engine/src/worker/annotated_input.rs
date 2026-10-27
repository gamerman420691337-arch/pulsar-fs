//! One source-bound native plan shared by file generation and training.
use super::*;

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnnotatedInput {
    pub native_plan: training_views::Plan,
    pub transform: TransformId,
}

pub(super) fn plan(
    binding: &AnnotatedGenerationView,
    source: &SourceArtifact,
    dimensions: [u32; 2],
    memory_bytes: u64,
) -> Result<AnnotatedInput> {
    binding.validate()?;
    if binding.source_sha256 != source.identity.sha256
        || binding.declaration.source_dimensions != dimensions
    {
        bail!("annotated view source content or encoded dimensions differ");
    }
    let [width, height] = binding.declaration.analysis_dimensions;
    if media::frame_bytes(width, height)? as u64 > memory_bytes / 16 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "annotated intermediate view exceeds immutable attempt admission",
        )
        .into());
    }
    let native_plan = training_views::plan(binding.declaration.clone())?;
    let identity = serde_json::to_vec(&(ANNOTATED_TEMPORAL_RECIPE, binding, &native_plan))?;
    let transform = TransformId::new(format!("annotated-view-{:x}", Sha256::digest(identity)))?;
    Ok(AnnotatedInput {
        native_plan,
        transform,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> AnnotatedGenerationView {
        serde_json::from_value(serde_json::json!({
            "source_sha256":"a".repeat(64),
            "declaration":{"schema_version":1,"source_dimensions":[192,64],
                "analysis_dimensions":[96,64],"target_region":[8,4,32,24],"target_id":"authored",
                "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}
        })).unwrap()
    }
    fn source() -> SourceArtifact {
        SourceArtifact {
            source_version: SourceVersionId::new("authored").unwrap(),
            path: "/inert".into(),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
                sha256: "a".repeat(64),
                byte_len: 1,
            },
        }
    }
    #[test]
    fn generation_retains_native_plan_and_binds_each_transform() {
        let binding = binding();
        let original = plan(&binding, &source(), [192, 64], 128 * 1024 * 1024).unwrap();
        assert_eq!(
            serde_json::to_vec(&original.native_plan).unwrap(),
            serde_json::to_vec(&training_views::plan(binding.declaration.clone()).unwrap())
                .unwrap()
        );
        assert!(original
            .native_plan
            .spatial_filter
            .contains("crop=32:24:8:4:exact=1,scale=64:64:flags=neighbor"));
        for changed in ["region", "target", "source"] {
            let mut b = binding.clone();
            let mut s = source();
            match changed {
                "region" => b.declaration.target_region[0] += 1,
                "target" => b.declaration.target_id = "another".into(),
                _ => {
                    b.source_sha256 = "b".repeat(64);
                    s.identity.sha256 = b.source_sha256.clone();
                }
            }
            assert_ne!(
                original.transform,
                plan(&b, &s, [192, 64], 128 * 1024 * 1024)
                    .unwrap()
                    .transform
            );
        }
    }
    #[test]
    fn source_geometry_and_intermediate_memory_refuse_before_decoding() {
        assert!(plan(&binding(), &source(), [191, 64], 128 * 1024 * 1024).is_err());
        let mut b = binding();
        b.source_sha256 = "b".repeat(64);
        assert!(plan(&b, &source(), [192, 64], 128 * 1024 * 1024).is_err());
        assert!(plan(&binding(), &source(), [192, 64], 1024).is_err());
        let mut b = binding();
        b.declaration.analysis_dimensions = [96, 96];
        b.declaration.target_region = [0, 80, 16, 16];
        assert!(plan(&b, &source(), [192, 64], 128 * 1024 * 1024).is_err());
    }
}
