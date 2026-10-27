//! A user seed chosen from real, current preview observations; no inferred identity.
use anyhow::{ensure, Context, Result};
use eframe::egui;
use pulsar_protocol::*;

#[derive(Clone, PartialEq)]
struct Binding {
    context: FrameContext,
    artifact: ArtifactIdentity,
    view: Option<VideoView>,
    map: Option<ViewCoordinateMap>,
}
impl Binding {
    fn checked(result: &PreviewResult, view: Option<&VideoView>) -> Result<Self> {
        ensure!(
            result.annotated.is_none()
                && result.matches_request(&result.context)
                && matches!(result.analysis, AnalysisStatus::Available),
            "Pick requires current ordinary detector observations"
        );
        let frame = result.frame.as_ref().context("Preview pixels are absent")?;
        frame.validate(crate::preview::MAX_PREVIEW_BYTES)?;
        ensure!(
            result.context.transform == video_view_transform(view)?,
            "Selected view differs from the displayed transform"
        );
        match (view, result.coordinate_map.as_ref()) {
            (Some(view), Some(map)) => {
                map.validate()?;
                ensure!(
                    map.view() == view && map.output_dimensions() == (frame.width, frame.height),
                    "Preview geometry differs from the selected view"
                );
            }
            (None, None) => {}
            _ => anyhow::bail!("Preview view mapping is absent or unexpected"),
        }
        Ok(Self {
            context: result.context.clone(),
            artifact: frame.artifact.identity.clone(),
            view: view.cloned(),
            map: result.coordinate_map.clone(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct PickedTarget {
    binding: Binding,
    pub seed: AnalysisTargetSeed,
    bounds: CoordinateBox,
}
impl PickedTarget {
    pub fn context(&self) -> &FrameContext {
        &self.binding.context
    }
}
fn select(
    result: &PreviewResult,
    view: Option<&VideoView>,
    point: [f64; 2],
) -> Result<PickedTarget> {
    let binding = Binding::checked(result, view)?;
    let seed = AnalysisTargetSeed {
        source_frame_index: result.source_frame_index,
        x: point[0],
        y: point[1],
        class_id: None,
    };
    seed.validate()?;
    let mut matches = result.observations.iter().filter(|observation| {
        let b = observation.bounds();
        observation.matches_frame(&result.context)
            && observation.evidence() == EvidenceKind::Observed
            && b.space() == CoordinateSpace::ProjectionNormalized
            && b.x_min() <= point[0]
            && point[0] < b.x_max()
            && b.y_min() <= point[1]
            && point[1] < b.y_max()
    });
    let selected = matches
        .next()
        .context("No observed box contains that point")?;
    ensure!(
        matches.next().is_none(),
        "Multiple observed boxes contain that point; pick an unambiguous point"
    );
    Ok(PickedTarget {
        binding,
        seed,
        bounds: selected.bounds().clone(),
    })
}

#[derive(Default)]
pub(crate) struct TargetPicker {
    draft: Option<PickedTarget>,
    pressed: Option<(Binding, egui::Rect)>,
}
impl TargetPicker {
    pub fn clear(&mut self) {
        self.draft = None;
        self.pressed = None;
    }
    pub fn current_draft(
        &self,
        result: Option<&PreviewResult>,
        view: Option<&VideoView>,
    ) -> Result<PickedTarget> {
        let draft = self.draft.as_ref().context("Click an observed box first")?;
        let current = select(
            result.context("Preview is absent")?,
            view,
            [draft.seed.x, draft.seed.y],
        )?;
        ensure!(
            current.binding == draft.binding && current.bounds == draft.bounds,
            "Picked target belongs to a different preview observation"
        );
        Ok(draft.clone())
    }
    /// The caller has decoded these exact pixels and uses the letterboxed image rect.
    pub fn widget(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        result: &PreviewResult,
        view: Option<&VideoView>,
    ) -> Result<()> {
        let binding = match Binding::checked(result, view) {
            Ok(b) => b,
            Err(error) => {
                self.clear();
                return Err(error);
            }
        };
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.binding != binding)
        {
            self.clear();
        }
        let response = ui.interact(
            rect,
            ui.id().with("tracking-target-pick"),
            egui::Sense::click(),
        );
        if response.is_pointer_button_down_on()
            && ui
                .ctx()
                .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
            && self.pressed.is_none()
        {
            self.pressed = Some((binding.clone(), rect));
        }
        if response.clicked() {
            let (started_binding, started_rect) = self
                .pressed
                .take()
                .context("Pick began on a different or unavailable image")?;
            ensure!(
                started_binding == binding && started_rect == rect,
                "Displayed frame or layout changed during the click; pick again"
            );
            let pos = response
                .interact_pointer_pos()
                .context("Pick pointer is absent")?;
            ensure!(
                rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 && rect.contains(pos),
                "Pick inside the displayed image"
            );
            let point = [
                f64::from((pos.x - rect.left()) / rect.width()),
                f64::from((pos.y - rect.top()) / rect.height()),
            ];
            // A failed click is not a replacement and cannot activate an invalid seed.
            self.draft = Some(select(result, view, point)?);
        }
        if ui.ctx().input(|i| i.pointer.any_released()) {
            self.pressed = None;
        }
        if let Some(draft) = &self.draft {
            if self.current_draft(Some(result), view).is_err() {
                self.clear();
                return Ok(());
            }
            let point = rect.min
                + egui::vec2(
                    draft.seed.x as f32 * rect.width(),
                    draft.seed.y as f32 * rect.height(),
                );
            ui.painter().line_segment(
                [point - egui::vec2(6.0, 0.0), point + egui::vec2(6.0, 0.0)],
                egui::Stroke::new(2.0_f32, egui::Color32::YELLOW),
            );
            ui.painter().line_segment(
                [point - egui::vec2(0.0, 6.0), point + egui::vec2(0.0, 6.0)],
                egui::Stroke::new(2.0_f32, egui::Color32::YELLOW),
            );
            ui.painter().text(
                point + egui::vec2(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                "Target seed draft",
                egui::FontId::monospace(12.0),
                egui::Color32::YELLOW,
            );
        }
        Ok(())
    }
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
                path: crate::test_host_path("/checked/frame.raw").into(),
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
    #[test]
    fn picks_only_a_unique_observed_half_open_box() {
        let mut result = fixture();
        let picked = select(&result, None, [0.5, 0.5]).unwrap();
        assert_eq!(picked.seed.source_frame_index, 4);
        assert!(picked.seed.class_id.is_none());
        assert_eq!(picked.context(), &result.context);
        for point in [
            [0.75, 0.5],
            [0.5, 0.75],
            [1.0, 0.5],
            [f64::NAN, 0.5],
            [-0.1, 0.5],
        ] {
            assert!(select(&result, None, point).is_err());
        }
        assert!(select(&result, None, [0.25, 0.25]).is_ok());
        result.observations.push(result.observations[0].clone());
        assert!(select(&result, None, [0.5, 0.5]).is_err());
        for kind in [
            EvidenceKind::Predicted,
            EvidenceKind::Inferred,
            EvidenceKind::Synthesized,
        ] {
            result.observations =
                vec![observation(&result.context, [0.25, 0.25, 0.75, 0.75], kind)];
            assert!(select(&result, None, [0.5, 0.5]).is_err());
        }
        result.observations.clear();
        assert!(select(&result, None, [0.5, 0.5]).is_err());
    }
    #[test]
    fn freshness_and_observation_replacement_refuse() {
        let original = fixture();
        let picked = select(&original, None, [0.5, 0.5]).unwrap();
        let picker = TargetPicker {
            draft: Some(picked),
            ..Default::default()
        };
        assert!(picker.current_draft(Some(&original), None).is_ok());
        for index in 0..7 {
            let mut result = original.clone();
            match index {
                0 => result.context.request_generation += 1,
                1 => result.context.seek_generation += 1,
                2 => result.context.source_version = SourceVersionId::new("c".repeat(64)).unwrap(),
                3 => result.context.frame = FrameId::new(5),
                4 => result.frame.as_mut().unwrap().artifact.identity.sha256 = "d".repeat(64),
                5 => {
                    result.observations = vec![observation(
                        &result.context,
                        [0.3, 0.3, 0.6, 0.6],
                        EvidenceKind::Observed,
                    )]
                }
                _ => result.observations.push(result.observations[0].clone()),
            }
            assert!(picker.current_draft(Some(&result), None).is_err());
        }
        assert!(picker.current_draft(None, None).is_err());
    }
    #[test]
    fn declared_view_mapping_and_non_detector_results_refuse() {
        let mut result = fixture();
        let view = VideoView {
            layout: StereoLayout::SideBySideLeftRight,
            eye: StereoEye::Right,
            projection: VideoProjection::Flat,
        };
        assert!(select(&result, Some(&view), [0.5, 0.5]).is_err());
        result.context.transform = video_view_transform(Some(&view)).unwrap();
        result.observations = vec![observation(
            &result.context,
            [0.25, 0.25, 0.75, 0.75],
            EvidenceKind::Observed,
        )];
        result.coordinate_map = Some(ViewCoordinateMap::new(view.clone(), 32, 10, 16, 10).unwrap());
        assert!(select(&result, Some(&view), [0.5, 0.5]).is_ok());
        let other = VideoView {
            eye: StereoEye::Left,
            ..view.clone()
        };
        assert!(select(&result, Some(&other), [0.5, 0.5]).is_err());
        result.analysis = AnalysisStatus::Pending;
        assert!(select(&result, Some(&view), [0.5, 0.5]).is_err());
    }
    #[test]
    fn actual_widget_pick_is_a_draft_and_failed_click_preserves_it() {
        let result = fixture();
        let ctx = egui::Context::default();
        let mut picker = TargetPicker::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(160.0, 100.0));
        let mut run = |events| {
            let mut outcome = Ok(());
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, 480.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        outcome = picker.widget(ui, rect, &result, None);
                    });
                },
            );
            outcome
        };
        run(vec![]).unwrap();
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let point = rect.center();
        run(vec![egui::Event::PointerMoved(point), click(point, true)]).unwrap();
        run(vec![click(point, false)]).unwrap();
        let miss = rect.min + egui::vec2(1.0, 1.0);
        run(vec![egui::Event::PointerMoved(miss), click(miss, true)]).unwrap();
        assert!(run(vec![click(miss, false)]).is_err());
        let picked = picker.current_draft(Some(&result), None).unwrap();
        assert_eq!(picked.seed.x, 0.5);
        assert_eq!(picked.seed.y, 0.5);
        if let Some(path) = std::env::var_os("PULSAR_TEST_PICKED_SEED_OUTPUT") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            file.write_all(
                &serde_json::to_vec(
                    &serde_json::json!({"context":picked.context(),"seed":picked.seed}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    #[test]
    fn replacement_pixels_or_layout_during_click_cannot_select() {
        for with_draft in [false, true] {
            for change in 0..3 {
                let result = fixture();
                let ctx = egui::Context::default();
                let mut picker = TargetPicker::default();
                let rect =
                    egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(160.0, 100.0));
                let point = rect.center();
                let mut run = |result: &PreviewResult, rect, events| {
                    let mut outcome = Ok(());
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(640.0, 480.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                outcome = picker.widget(ui, rect, result, None);
                            });
                        },
                    );
                    outcome
                };
                let button = |pressed| egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                };
                run(&result, rect, vec![]).unwrap();
                if with_draft {
                    run(
                        &result,
                        rect,
                        vec![egui::Event::PointerMoved(point), button(true)],
                    )
                    .unwrap();
                    run(&result, rect, vec![button(false)]).unwrap();
                }
                run(
                    &result,
                    rect,
                    vec![egui::Event::PointerMoved(point), button(true)],
                )
                .unwrap();
                let mut changed = result.clone();
                let mut next_rect = rect;
                match change {
                    0 => {
                        changed.context.request_generation += 1;
                        changed.observations = vec![observation(
                            &changed.context,
                            [0.25, 0.25, 0.75, 0.75],
                            EvidenceKind::Observed,
                        )];
                    }
                    1 => changed.frame.as_mut().unwrap().artifact.identity.sha256 = "c".repeat(64),
                    _ => next_rect = rect.translate(egui::vec2(1.0, 0.0)),
                }
                run(&changed, next_rect, vec![]).unwrap();
                assert!(run(&changed, next_rect, vec![button(false)]).is_err());
                if with_draft && change == 2 {
                    let preserved = picker.current_draft(Some(&changed), None).unwrap();
                    assert_eq!([preserved.seed.x, preserved.seed.y], [0.5, 0.5]);
                    assert_eq!(preserved.seed.source_frame_index, 4);
                } else {
                    assert!(picker.current_draft(Some(&changed), None).is_err());
                }
            }
        }
    }
}
