//! Ephemeral checked preview references; no effect or target identity authority.
use super::*;
use crate::resources::Reservation;
use std::collections::VecDeque;
#[derive(Default)]
pub(super) struct Registry {
    slots: VecDeque<(AssistantTargetCatalog, Reservation)>,
}
impl Registry {
    pub(super) fn replace(
        &mut self,
        catalog: AssistantTargetCatalog,
        pool: &ResourcePool,
    ) -> PResult<()> {
        catalog.validate()?;
        self.slots.retain(|(old, _)| {
            old.project_id != catalog.project_id || old.session_id != catalog.session_id
        });
        if catalog.targets.is_empty() {
            return Ok(());
        }
        if self.slots.len() == 32 {
            self.slots.pop_front();
        }
        let reservation = pool.reserve_memory(512 * 1024)?;
        self.slots.push_back((catalog, reservation));
        Ok(())
    }
    pub(super) fn get(
        &self,
        project: &ProjectId,
        owner: &SessionId,
        revision: RevisionId,
    ) -> AssistantTargetCatalog {
        self.slots
            .iter()
            .find(|(c, _)| {
                &c.project_id == project && &c.session_id == owner && c.revision == revision
            })
            .map(|(c, _)| c.clone())
            .unwrap_or_else(|| AssistantTargetCatalog {
                project_id: project.clone(),
                session_id: owner.clone(),
                revision,
                targets: Vec::new(),
            })
    }
}
pub(super) fn capture(
    project_id: ProjectId,
    session_id: SessionId,
    revision: RevisionId,
    preview: &PreviewResult,
    model: Option<&ArtifactIdentity>,
    view: Option<&VideoView>,
    current: bool,
) -> PResult<AssistantTargetCatalog> {
    let mut catalog = AssistantTargetCatalog {
        project_id,
        session_id,
        revision,
        targets: Vec::new(),
    };
    let Some(frame) = preview.frame.as_ref() else {
        return Ok(catalog);
    };
    let Some(model) = model else {
        return Ok(catalog);
    };
    if !current
        || preview.annotated.is_some()
        || preview.placed.is_some()
        || !preview.matches_request(&preview.context)
        || !matches!(preview.analysis, AnalysisStatus::Available)
        || preview.observations.len() > 4096
        || preview.context.transform != video_view_transform(view)?
    {
        return Ok(catalog);
    }
    match (view, preview.coordinate_map.as_ref()) {
        (Some(view), Some(map))
            if map.view() == view && map.output_dimensions() == (frame.width, frame.height) =>
        {
            map.validate()
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
        }
        (None, None) => {}
        _ => return Ok(catalog),
    }
    let eligible = |o: &&DetectionObservation| {
        o.matches_frame(&preview.context)
            && o.evidence() == EvidenceKind::Observed
            && o.bounds().space() == CoordinateSpace::ProjectionNormalized
    };
    for observation in preview.observations.iter().filter(eligible) {
        let (x, y) = observation.bounds().center();
        let containing = preview
            .observations
            .iter()
            .filter(eligible)
            .filter(|o| {
                let b = o.bounds();
                b.x_min() <= x && x < b.x_max() && b.y_min() <= y && y < b.y_max()
            })
            .take(2)
            .count();
        if containing != 1 {
            continue;
        }
        let mut target = AssistantObservedTarget {
            target_id: String::new(),
            context: preview.context.clone(),
            frame: frame.artifact.identity.clone(),
            model: model.clone(),
            observation: observation.clone(),
            source_frame_index: preview.source_frame_index,
            video_view: view.cloned(),
        };
        target.target_id = target.identity()?;
        target.validate()?;
        if catalog
            .targets
            .iter()
            .any(|old| old.target_id == target.target_id)
        {
            continue;
        }
        catalog.targets.push(target);
        if catalog.targets.len() == 64 {
            break;
        }
    }
    catalog.validate()?;
    if serde_json::to_vec(&catalog).map_err(internal)?.len() > 128 * 1024 {
        return Err(ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "target catalog exceeds capture bound",
        ));
    }
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> PreviewResult {
        let context = FrameContext {
            source_version: SourceVersionId::new("a".repeat(64)).unwrap(),
            source_placement: SourcePlacementId::new("primary").unwrap(),
            frame: FrameId::new(4),
            transform: video_view_transform(None).unwrap(),
            seek_generation: 2,
            request_generation: 3,
        };
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path: "/checked/frame.raw".into(),
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("displayed").unwrap(),
                    sha256: "b".repeat(64),
                    byte_len: 16 * 10 * 3,
                },
            },
            width: 16,
            height: 10,
            stride_bytes: 16 * 3,
            format: PixelFormat::Rgb8,
        };
        PreviewResult {
            annotated: None,
            placed: None,
            time_basis: PreviewTimeBasis::SourcePts,
            elapsed_source_time: None,
            source_origin: None,
            coordinate_map: None,
            context: context.clone(),
            requested_source_time: SourceTimestamp::new(1, 2).unwrap(),
            source_time: SourceTimestamp::new(1, 2).unwrap(),
            source_frame_index: 4,
            frame: Some(frame),
            analysis: AnalysisStatus::Available,
            observations: vec![observation(
                &context,
                [0.25, 0.25, 0.75, 0.75],
                EvidenceKind::Observed,
            )],
        }
    }
    fn observation(
        context: &FrameContext,
        b: [f64; 4],
        evidence: EvidenceKind,
    ) -> DetectionObservation {
        DetectionObservation::new(
            context.clone(),
            CoordinateBox::new(
                CoordinateSpace::ProjectionNormalized,
                b[0],
                b[1],
                b[2],
                b[3],
            )
            .unwrap(),
            None,
            evidence,
            Confidence::new(0.6, None).unwrap(),
        )
        .unwrap()
    }

    fn model() -> ArtifactIdentity {
        ArtifactIdentity {
            artifact_id: ArtifactId::new("model").unwrap(),
            sha256: "c".repeat(64),
            byte_len: 12,
        }
    }
    fn catalog(preview: &PreviewResult, current: bool) -> AssistantTargetCatalog {
        capture(
            ProjectId::new("project").unwrap(),
            SessionId::new("owner").unwrap(),
            RevisionId::new(1),
            preview,
            Some(&model()),
            None,
            current,
        )
        .unwrap()
    }
    #[test]
    fn checked_capture_binds_actual_frame_model_and_seed() {
        let c = catalog(&fixture(), true);
        assert_eq!(c.targets.len(), 1);
        let t = &c.targets[0];
        assert_eq!(t.seed().source_frame_index, 4);
        assert_eq!(
            (t.seed().x, t.seed().y, t.seed().class_id),
            (0.5, 0.5, None)
        );
        let mut changed = t.clone();
        changed.model.sha256 = "d".repeat(64);
        assert!(changed.validate().is_err());
        changed = t.clone();
        changed.source_frame_index += 1;
        assert!(changed.validate().is_err());
    }
    #[test]
    fn ambiguous_centres_and_predictions_are_excluded() {
        let mut p = fixture();
        p.observations.push(p.observations[0].clone());
        assert!(catalog(&p, true).targets.is_empty());
        p.observations = vec![observation(
            &p.context,
            [0.25, 0.25, 0.75, 0.75],
            EvidenceKind::Predicted,
        )];
        assert!(catalog(&p, true).targets.is_empty());
    }
    #[test]
    fn stale_missing_model_pixels_and_wrong_transform_are_excluded() {
        let p = fixture();
        assert!(catalog(&p, false).targets.is_empty());
        let mut p = fixture();
        p.frame = None;
        assert!(catalog(&p, true).targets.is_empty());
        let mut p = fixture();
        p.context.request_generation += 1;
        assert!(catalog(&p, true).targets.is_empty());
        let c = capture(
            ProjectId::new("project").unwrap(),
            SessionId::new("owner").unwrap(),
            RevisionId::new(1),
            &fixture(),
            None,
            None,
            true,
        )
        .unwrap();
        assert!(c.targets.is_empty());
    }
    #[test]
    fn owner_revision_restart_and_new_preview_clear_visibility() {
        let mut registry = Registry::default();
        let pool = ResourcePool::new(4 * 1024 * 1024, 1);
        let c = catalog(&fixture(), true);
        let project = c.project_id.clone();
        let owner = c.session_id.clone();
        registry.replace(c.clone(), &pool).unwrap();
        assert_eq!(registry.get(&project, &owner, c.revision).targets.len(), 1);
        assert!(registry
            .get(&project, &SessionId::new("other").unwrap(), c.revision)
            .targets
            .is_empty());
        assert!(registry
            .get(&project, &owner, RevisionId::new(2))
            .targets
            .is_empty());
        assert!(Registry::default()
            .get(&project, &owner, c.revision)
            .targets
            .is_empty());
        registry.replace(catalog(&fixture(), false), &pool).unwrap();
        assert!(registry
            .get(&project, &owner, c.revision)
            .targets
            .is_empty());
        assert!(pool.reserve_memory(4 * 1024 * 1024).is_ok());
    }
    #[test]
    fn metadata_exhaustion_releases_previous_catalog() {
        let mut registry = Registry::default();
        let pool = ResourcePool::new(512 * 1024, 1);
        let c = catalog(&fixture(), true);
        registry.replace(c.clone(), &pool).unwrap();
        registry.replace(c, &pool).unwrap();
        let mut c = catalog(&fixture(), true);
        c.project_id = ProjectId::new("other").unwrap();
        assert!(registry.replace(c, &pool).is_err());
        assert_eq!(registry.slots.len(), 1);
    }
    #[test]
    fn catalog_bounds_unknown_fields_and_duplicate_ids_refused() {
        let mut c = catalog(&fixture(), true);
        c.targets.push(c.targets[0].clone());
        assert!(c.validate().is_err());
        let mut value = serde_json::to_value(catalog(&fixture(), true)).unwrap();
        value["authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<AssistantTargetCatalog>(value).is_err());
    }
    #[test]
    fn catalogue_limits_and_fifo_eviction_are_bounded() {
        let mut p = fixture();
        p.observations = vec![p.observations[0].clone(); 4097];
        assert!(catalog(&p, true).targets.is_empty());
        let pool = ResourcePool::new(32 * 512 * 1024, 1);
        let mut registry = Registry::default();
        for index in 0..33 {
            let mut c = catalog(&fixture(), true);
            c.project_id = ProjectId::new(format!("project-{index}")).unwrap();
            registry.replace(c, &pool).unwrap();
        }
        assert_eq!(registry.slots.len(), 32);
        assert!(registry
            .get(
                &ProjectId::new("project-0").unwrap(),
                &SessionId::new("owner").unwrap(),
                RevisionId::new(1)
            )
            .targets
            .is_empty());
        assert_eq!(
            registry
                .get(
                    &ProjectId::new("project-32").unwrap(),
                    &SessionId::new("owner").unwrap(),
                    RevisionId::new(1)
                )
                .targets
                .len(),
            1
        );
    }
}
