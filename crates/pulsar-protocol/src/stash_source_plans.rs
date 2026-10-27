//! Source-bound batch intent. Resolving this contract does not create a
//! placement, authorize publication, or verify source-clock receipts.
use crate::*;
use std::collections::BTreeSet;

/// A separate entry for each selected Stash file. Placement identifiers and
/// revisions come from the Engine's acknowledged SetSourcePlacement response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StashBatchSourcePlan {
    pub scene_id: String,
    pub file_id: String,
    pub source_sha256: String,
    pub placement: Option<SourcePlacementSpec>,
    pub project_interval: Option<TimeRange>,
    /// Explicit original-source clock conversion for sibling publication.
    pub source_time: bool,
}

impl StashBatchSourcePlan {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if [&self.scene_id, &self.file_id].iter().any(|id| {
            id.is_empty()
                || id.len() > 256
                || id.trim() != id.as_str()
                || id.chars().any(char::is_control)
        }) {
            return Err(ProtocolError::invalid(
                "batch source plan requires exact bounded keys and lowercase source SHA-256",
            ));
        }
        crate::validate_sha256(&self.source_sha256)?;
        if let Some(spec) = &self.placement {
            spec.validate()
                .map_err(|e| ProtocolError::invalid(e.to_string()))?;
            if !self.source_time
                && !spec
                    .preserves_source_clock()
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?
            {
                return Err(ProtocolError::invalid(
                    "placed batch publication requires explicit source-time conversion",
                ));
            }
        } else if self.source_time {
            return Err(ProtocolError::invalid(
                "source-time conversion requires an explicit placement",
            ));
        }
        if self
            .project_interval
            .is_some_and(|range| range.start() < ProjectTime::ZERO)
        {
            return Err(ProtocolError::invalid(
                "batch project interval must be nonnegative",
            ));
        }
        Ok(())
    }
}

/// Validate the complete template and catalogs before source/model capture.
/// Discovery eligibility and exact selected-source coverage remain Engine checks.
pub fn validate_stash_batch_source_plans(
    settings: &GenerationSettings,
    views: &[StashBatchAnnotatedView],
    plans: &[StashBatchSourcePlan],
) -> Result<(), ProtocolError> {
    validate_stash_batch_settings(settings, views)?;
    if plans.is_empty() {
        return Ok(());
    }
    if settings.raw_frame_pair.is_some() {
        return Err(ProtocolError::unsupported(
            "batch source plans require indexed finite video",
        ));
    }
    if plans.len() > 100
        || settings.source_placement.is_some()
        || settings.project_interval.is_some()
    {
        return Err(ProtocolError::invalid(
            "batch source plans require at most 100 entries and no shared placement or interval",
        ));
    }
    let mut keys = BTreeSet::new();
    for plan in plans {
        plan.validate()?;
        if !keys.insert((&plan.scene_id, &plan.file_id)) {
            return Err(ProtocolError::invalid("duplicate batch source plan"));
        }
        let mut resolved = settings.clone();
        resolved.project_interval = plan.project_interval;
        if !views.is_empty() {
            let view = views
                .iter()
                .find(|v| v.scene_id == plan.scene_id && v.file_id == plan.file_id)
                .ok_or_else(|| {
                    ProtocolError::invalid("source plan has no matching annotated view")
                })?;
            if view.view.source_sha256 != plan.source_sha256 {
                return Err(ProtocolError::invalid(
                    "source plan and annotated view identify different source bytes",
                ));
            }
            resolved.annotated_view = Some(view.view.clone());
        }
        // No fabricated placement ID is used to validate a not-yet-created
        // Engine placement. Check its only additional settings constraint here.
        if resolved.target_seed.is_some()
            && plan
                .placement
                .as_ref()
                .is_some_and(|p| p.rate_numerator < 0)
        {
            return Err(ProtocolError::unsupported(
                "target seed requires forward source placement",
            ));
        }
        resolved.validate()?;
    }
    if !views.is_empty() && views.len() != plans.len() {
        return Err(ProtocolError::invalid(
            "annotated views and source plans must cover identical files",
        ));
    }
    Ok(())
}

/// Resolve one captured source using an Engine-acknowledged active placement.
/// The caller must verify active membership and project revision before dispatch.
pub fn resolve_stash_batch_source_settings(
    settings: &GenerationSettings,
    views: &[StashBatchAnnotatedView],
    plans: &[StashBatchSourcePlan],
    capture: &StashSourceCapture,
    active: Option<&SourcePlacement>,
) -> Result<GenerationSettings, ProtocolError> {
    validate_stash_batch_source_plans(settings, views, plans)?;
    let plan = plans
        .iter()
        .find(|p| p.scene_id == capture.scene_id && p.file_id == capture.file_id)
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "captured Stash source has no source plan",
            )
        })?;
    if plan.source_sha256 != capture.identity.sha256
        || plan
            .placement
            .as_ref()
            .is_some_and(|p| p.source_version != capture.source_version)
        || plan.placement.as_ref() != active.map(|p| &p.spec)
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "batch source bytes or acknowledged placement differ from source plan",
        ));
    }
    let mut resolved = settings.clone();
    resolved.project_interval = plan.project_interval;
    resolved.source_placement = active.cloned();
    if !views.is_empty() {
        resolved.annotated_view = Some(
            views
                .iter()
                .find(|v| v.scene_id == plan.scene_id && v.file_id == plan.file_id)
                .ok_or_else(|| ProtocolError::invalid("captured source annotated view absent"))?
                .view
                .clone(),
        );
    }
    resolved.validate()?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (StashBatchSourcePlan, StashSourceCapture, SourcePlacement) {
        let source = SourceVersionId::new("captured-source").unwrap();
        let spec = SourcePlacementSpec {
            source_version: source.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(10, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(5_000_000_000),
            rate_numerator: 2,
            rate_denominator: 1,
        };
        let placement = SourcePlacement {
            placement_id: SourcePlacementId::new("acknowledged").unwrap(),
            created_revision: RevisionId::new(1),
            spec: spec.clone(),
        };
        let plan = StashBatchSourcePlan {
            scene_id: "scene".into(),
            file_id: "file".into(),
            source_sha256: "a".repeat(64),
            placement: Some(spec),
            project_interval: Some(
                TimeRange::new(
                    ProjectTime::from_nanos(6_000_000_000),
                    ProjectTime::from_nanos(8_000_000_000),
                )
                .unwrap(),
            ),
            source_time: true,
        };
        let capture = StashSourceCapture {
            endpoint: None,
            source_version: source,
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("artifact").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 1,
            },
            scene_id: "scene".into(),
            file_id: "file".into(),
            server_path: "/stash/source.mkv".into(),
            local_source: "/authored/source.mkv".into(),
            script_destination: "/authored/source.funscript".into(),
        };
        (plan, capture, placement)
    }

    #[test]
    fn resolved_settings_preserve_exact_acknowledged_placement_and_half_open_interval() {
        let (plan, capture, placement) = fixture();
        let settings = GenerationSettings::default();
        let resolved = resolve_stash_batch_source_settings(
            &settings,
            &[],
            std::slice::from_ref(&plan),
            &capture,
            Some(&placement),
        )
        .unwrap();
        assert_eq!(resolved.source_placement, Some(placement));
        assert_eq!(resolved.project_interval, plan.project_interval);
        let range = resolved.project_interval.unwrap();
        assert!(range.contains(range.start()));
        assert!(!range.contains(range.end()));
        assert!(settings.source_placement.is_none());
        assert!(settings.project_interval.is_none());
    }

    #[test]
    fn source_key_hash_version_and_placement_substitution_refuse() {
        let (plan, capture, placement) = fixture();
        for mutation in 0..5 {
            let mut capture = capture.clone();
            let mut active = placement.clone();
            match mutation {
                0 => capture.file_id = "other".into(),
                1 => capture.identity.sha256 = "b".repeat(64),
                2 => capture.source_version = SourceVersionId::new("other").unwrap(),
                3 => active.spec.rate_numerator = 3,
                _ => active.spec.project_anchor = ProjectTime::ZERO,
            }
            assert_eq!(
                resolve_stash_batch_source_settings(
                    &GenerationSettings::default(),
                    &[],
                    std::slice::from_ref(&plan),
                    &capture,
                    Some(&active)
                )
                .unwrap_err()
                .code,
                ErrorCode::DependencyMismatch
            );
        }
        assert!(resolve_stash_batch_source_settings(
            &GenerationSettings::default(),
            &[],
            &[plan],
            &capture,
            None
        )
        .is_err());
    }

    #[test]
    fn clock_conversion_intent_cannot_be_implicit_or_unplaced() {
        let (mut plan, capture, placement) = fixture();
        plan.source_time = false;
        assert!(plan.validate().is_err());
        plan.placement.as_mut().unwrap().rate_numerator = 1;
        plan.placement.as_mut().unwrap().project_anchor = ProjectTime::ZERO;
        plan.validate().unwrap();
        plan.placement = None;
        plan.source_time = true;
        assert!(plan.validate().is_err());
        plan.source_time = false;
        let resolved = resolve_stash_batch_source_settings(
            &GenerationSettings::default(),
            &[],
            &[plan.clone()],
            &capture,
            None,
        )
        .unwrap();
        assert!(resolved.source_placement.is_none());
        assert!(resolve_stash_batch_source_settings(
            &GenerationSettings::default(),
            &[],
            &[plan],
            &capture,
            Some(&placement)
        )
        .is_err());
    }

    #[test]
    fn catalogs_are_bounded_unique_and_refuse_shared_single_source_settings() {
        let (plan, _, placement) = fixture();
        let settings = GenerationSettings::default();
        assert!(
            validate_stash_batch_source_plans(&settings, &[], &[plan.clone(), plan.clone()])
                .is_err()
        );
        assert!(
            validate_stash_batch_source_plans(&settings, &[], &vec![plan.clone(); 101]).is_err()
        );
        let mut settings = settings;
        settings.project_interval = plan.project_interval;
        assert!(
            validate_stash_batch_source_plans(&settings, &[], std::slice::from_ref(&plan)).is_err()
        );
        settings.project_interval = None;
        settings.source_placement = Some(placement);
        assert!(validate_stash_batch_source_plans(&settings, &[], &[plan]).is_err());
    }

    #[test]
    fn strict_wire_requires_explicit_clock_intent_and_valid_half_open_ranges() {
        let (plan, _, _) = fixture();
        let mut wire = serde_json::to_value(&plan).unwrap();
        wire.as_object_mut().unwrap().remove("source_time");
        assert!(serde_json::from_value::<StashBatchSourcePlan>(wire).is_err());
        let mut wire = serde_json::to_value(&plan).unwrap();
        wire["unexpected"] = true.into();
        assert!(serde_json::from_value::<StashBatchSourcePlan>(wire).is_err());
        let mut wire = serde_json::to_value(&plan).unwrap();
        wire["project_interval"]["end"] = wire["project_interval"]["start"].clone();
        assert!(serde_json::from_value::<StashBatchSourcePlan>(wire).is_err());
        let mut bad = plan;
        bad.source_sha256 = "A".repeat(64);
        assert!(bad.validate().is_err());
    }

    #[test]
    fn annotated_catalog_and_placement_resolve_together_without_cross_source_fallback() {
        let (plan, capture, placement) = fixture();
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        let identity = capture.identity.clone();
        let mut settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            model_input: Some(input.clone()),
            ..GenerationSettings::default()
        };
        settings.inference_profile = Some(DeclaredInferenceProfile {
            schema_version: 1,
            model: identity.clone(),
            runtime: identity,
            preprocessing: InferencePreprocessing {
                input,
                resize: InferenceResize::AnnotatedTemporalView,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        });
        let view=StashBatchAnnotatedView { scene_id:plan.scene_id.clone(),file_id:plan.file_id.clone(),
            view:AnnotatedGenerationView { source_sha256:plan.source_sha256.clone(),declaration:serde_json::from_value(serde_json::json!({
                "schema_version":1,"source_dimensions":[64,64],"analysis_dimensions":[64,64],
                "target_region":[0,0,64,64],"target_id":"authored","video_view":{"layout":"mono","eye":"left","projection":{"kind":"flat"}}
            })).unwrap() } };
        let resolved = resolve_stash_batch_source_settings(
            &settings,
            std::slice::from_ref(&view),
            std::slice::from_ref(&plan),
            &capture,
            Some(&placement),
        )
        .unwrap();
        assert_eq!(
            resolved.annotated_view.unwrap().source_sha256,
            plan.source_sha256
        );
        assert_eq!(resolved.source_placement, Some(placement));
        assert!(settings.annotated_view.is_none());
        let mut wrong = view.clone();
        wrong.view.source_sha256 = "b".repeat(64);
        assert!(validate_stash_batch_source_plans(
            &settings,
            &[wrong],
            std::slice::from_ref(&plan)
        )
        .is_err());
        let mut extra = view.clone();
        extra.file_id = "other".into();
        assert!(validate_stash_batch_source_plans(
            &settings,
            &[view, extra.clone()],
            std::slice::from_ref(&plan)
        )
        .is_err());
        assert!(validate_stash_batch_source_plans(&settings, &[extra], &[plan]).is_err());
    }

    #[test]
    fn reverse_seed_and_negative_project_interval_refuse_before_capture() {
        let (mut plan, _, _) = fixture();
        plan.placement.as_mut().unwrap().rate_numerator = -1;
        let settings = GenerationSettings {
            target_seed: Some(AnalysisTargetSeed {
                source_frame_index: 0,
                x: 0.5,
                y: 0.5,
                class_id: None,
            }),
            model_input: Some(ModelInputContract {
                width: 320,
                height: 180,
                channels: 3,
                layout: TensorLayout::Nchw,
                class_count: 1,
                decoder: DetectorDecoder::YoloChannelMajor,
            }),
            ..GenerationSettings::default()
        };
        assert_eq!(
            validate_stash_batch_source_plans(&settings, &[], &[plan.clone()])
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        plan.project_interval =
            Some(TimeRange::new(ProjectTime::from_nanos(-1), ProjectTime::ZERO).unwrap());
        assert!(plan.validate().is_err());
    }

    #[test]
    fn finite_source_plans_refuse_raw_pair_interpretation() {
        let (mut plan, _, _) = fixture();
        plan.project_interval = None;
        let settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            model_input: Some(ModelInputContract {
                width: 64,
                height: 64,
                channels: 6,
                layout: TensorLayout::Nchw,
                class_count: 1,
                decoder: DetectorDecoder::TemporalScriptPosition,
            }),
            raw_frame_pair: Some(RawFramePair {
                source_epoch: AttemptId::new("authored").unwrap(),
                previous_frame: FrameId::new(0),
                current_frame: FrameId::new(1),
                previous_time: SourceTimestamp::new(0, 1).unwrap(),
                current_time: SourceTimestamp::new(1, 10).unwrap(),
                presentation_clock: None,
            }),
            ..GenerationSettings::default()
        };
        settings.validate().unwrap();
        assert_eq!(
            validate_stash_batch_source_plans(&settings, &[], &[plan])
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
}
