//! Explicit geometry authored against an accepted, uncropped viewer frame.
use anyhow::{ensure, Context, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq)]
struct Binding {
    context: FrameContext,
    artifact: ArtifactIdentity,
    map: ViewCoordinateMap,
}
impl Binding {
    fn checked(result: &PreviewResult, view: &VideoView) -> Result<Self> {
        ensure!(
            result.annotated.is_none() && result.matches_request(&result.context),
            "Target drawing requires a checked, uncropped preview"
        );
        let frame = result.frame.as_ref().context("Preview has no pixels")?;
        frame.validate(crate::preview::MAX_PREVIEW_BYTES)?;
        let map = result
            .coordinate_map
            .as_ref()
            .context("Declare the eye and projection before drawing")?;
        map.validate()?;
        ensure!(
            result.context.transform == video_view_transform(Some(view))?,
            "Preview transform does not match the selected view"
        );
        ensure!(
            frame.width <= PREVIEW_FRAME_MAX_DIMENSIONS[0]
                && frame.height <= PREVIEW_FRAME_MAX_DIMENSIONS[1],
            "Preview exceeds the desktop admission"
        );
        ensure!(
            map.view() == view && map.output_dimensions() == (frame.width, frame.height),
            "Displayed view does not match the declaration"
        );
        Ok(Self {
            context: result.context.clone(),
            artifact: frame.artifact.identity.clone(),
            map: map.clone(),
        })
    }
    fn proposal(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        target_id: &str,
    ) -> Result<AnnotatedGenerationView> {
        ensure!(
            a.into_iter()
                .chain(b)
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
            "Draw entirely inside the displayed image"
        );
        ensure!(
            a[0] != b[0] && a[1] != b[1],
            "Target rectangle must have area"
        );
        let (w, h) = self.map.output_dimensions();
        let left = (a[0].min(b[0]) * f64::from(w)).floor() as u32;
        let top = (a[1].min(b[1]) * f64::from(h)).floor() as u32;
        let right = (a[0].max(b[0]) * f64::from(w)).ceil() as u32;
        let bottom = (a[1].max(b[1]) * f64::from(h)).ceil() as u32;
        let (sw, sh) = self.map.source_dimensions();
        let proposal = AnnotatedGenerationView {
            source_sha256: self.context.source_version.to_string(),
            declaration: AnnotatedTemporalView {
                schema_version: 1,
                source_dimensions: [sw, sh],
                analysis_dimensions: PREVIEW_FRAME_MAX_DIMENSIONS,
                video_view: self.map.view().clone(),
                target_region: [left, top, right - left, bottom - top],
                target_id: target_id.to_owned(),
            },
        };
        proposal.validate()?;
        Ok(proposal)
    }
}

#[derive(Default)]
pub(crate) struct TargetViewAuthor {
    binding: Option<Binding>,
    start: Option<(egui::Rect, [f64; 2])>,
    draft: Option<AnnotatedGenerationView>,
}
impl TargetViewAuthor {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn current_draft(
        &self,
        result: Option<&PreviewResult>,
        view: Option<&VideoView>,
        target_id: &str,
    ) -> Result<AnnotatedGenerationView> {
        let binding = Binding::checked(
            result.context("Preview is absent")?,
            view.context("View is absent")?,
        )?;
        ensure!(
            self.binding.as_ref() == Some(&binding),
            "Draft belongs to a different preview"
        );
        self.draft(target_id)
    }
    fn draft(&self, target_id: &str) -> Result<AnnotatedGenerationView> {
        let mut draft = self
            .draft
            .clone()
            .context("Draw a target rectangle first")?;
        draft.declaration.target_id = target_id.to_owned();
        draft.validate()?;
        Ok(draft)
    }
    /// Called only after the desktop decoded and accepted this frame's pixels.
    pub(crate) fn widget(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        result: &PreviewResult,
        view: &VideoView,
    ) -> Result<()> {
        let binding = match Binding::checked(result, view) {
            Ok(binding) => binding,
            Err(error) => {
                self.clear();
                return Err(error);
            }
        };
        if self.binding.as_ref() != Some(&binding) {
            self.clear();
            self.binding = Some(binding);
        }
        let response = ui.interact(rect, ui.id().with("target-view-drag"), egui::Sense::drag());
        let point = |pos: egui::Pos2| -> Result<[f64; 2]> {
            ensure!(
                rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 && rect.contains(pos),
                "Draw entirely inside the displayed image"
            );
            Ok([
                f64::from((pos.x - rect.left()) / rect.width()),
                f64::from((pos.y - rect.top()) / rect.height()),
            ])
        };
        if response.drag_started() {
            self.start = None;
            let origin = ui
                .ctx()
                .input(|i| i.pointer.press_origin())
                .context("Pointer origin is absent")?;
            self.start = Some((rect, point(origin)?));
        }
        if let Some((started_rect, _)) = self.start {
            if started_rect != rect {
                self.start = None;
                anyhow::bail!("Viewer layout changed; draw again");
            }
        }
        if response.drag_stopped() {
            if let Some((_, start)) = self.start.take() {
                let end = ui
                    .ctx()
                    .input(|i| i.pointer.interact_pos())
                    .context("Pointer endpoint is absent")?;
                let proposal = self
                    .binding
                    .as_ref()
                    .context("Preview binding is absent")?
                    .proposal(start, point(end)?, "selected-target")?;
                self.draft = Some(proposal);
            }
        }
        if let Some(draft) = &self.draft {
            let [x, y, w, h] = draft.declaration.target_region;
            let (vw, vh) = self
                .binding
                .as_ref()
                .expect("draft retains a binding")
                .map
                .output_dimensions();
            let bounds = egui::Rect::from_min_max(
                rect.min
                    + egui::vec2(
                        x as f32 / vw as f32 * rect.width(),
                        y as f32 / vh as f32 * rect.height(),
                    ),
                rect.min
                    + egui::vec2(
                        (x + w) as f32 / vw as f32 * rect.width(),
                        (y + h) as f32 / vh as f32 * rect.height(),
                    ),
            );
            ui.painter().rect_stroke(
                bounds,
                0.0,
                egui::Stroke::new(2.0_f32, egui::Color32::YELLOW),
                egui::StrokeKind::Inside,
            );
        }
        Ok(())
    }
}

pub(crate) fn save_new(path: &Path, view: &AnnotatedGenerationView) -> Result<PathBuf> {
    view.validate()?;
    let mut bytes = serde_json::to_vec_pretty(view)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= 4096,
        "Target view exceeds planner input bound"
    );
    let mut stage =
        crate::project_package::DestinationStage::new(path).context("Create new target view")?;
    stage.file_mut().write_all(&bytes)?;
    stage.publish().map_err(|error| {
        // The shared publisher's recovery receipt is useful here, but its
        // package-specific Display promises an engine export that does not exist.
        if let Some(receipt) =
            error.downcast_ref::<crate::project_package::PackagePublicationError>()
        {
            anyhow::anyhow!(
                "Target view publication {:?} at {}: {}; recovery staging: {:?}",
                receipt.outcome,
                receipt.destination.display(),
                receipt.detail,
                receipt.recovery_staging_path
            )
        } else {
            error.context("Publish target view")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PreviewResult, VideoView) {
        let view = VideoView {
            layout: StereoLayout::SideBySideLeftRight,
            eye: StereoEye::Right,
            projection: VideoProjection::Flat,
        };
        let map = ViewCoordinateMap::new(view.clone(), 192, 64, 96, 64).unwrap();
        let context = FrameContext {
            source_version: SourceVersionId::new("a".repeat(64)).unwrap(),
            source_placement: SourcePlacementId::new("primary").unwrap(),
            frame: FrameId::new(4),
            transform: video_view_transform(Some(&view)).unwrap(),
            seek_generation: 1,
            request_generation: 2,
        };
        let frame = FrameArtifact {
            artifact: ArtifactReference {
                path: PathBuf::from(crate::test_host_path("/checked/frame.raw")),
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new("displayed").unwrap(),
                    sha256: "b".repeat(64),
                    byte_len: 96 * 64 * 3,
                },
            },
            width: 96,
            height: 64,
            stride_bytes: 96 * 3,
            format: PixelFormat::Rgb8,
        };
        (
            PreviewResult {
                annotated: None,
                placed: None,
                time_basis: PreviewTimeBasis::SourcePts,
                elapsed_source_time: None,
                source_origin: None,
                coordinate_map: Some(map),
                context,
                requested_source_time: SourceTimestamp::new(1, 2).unwrap(),
                source_time: SourceTimestamp::new(1, 2).unwrap(),
                source_frame_index: 4,
                observations: vec![],
                frame: Some(frame),
                analysis: AnalysisStatus::Available,
            },
            view,
        )
    }
    #[test]
    fn declared_geometry_and_invalid_rectangles() {
        let (result, view) = fixture();
        let binding = Binding::checked(&result, &view).unwrap();
        let proposal = binding
            .proposal([0.5, 0.5], [0.125, 0.25], "chosen")
            .unwrap();
        assert_eq!(proposal.source_sha256, "a".repeat(64));
        assert_eq!(proposal.declaration.source_dimensions, [192, 64]);
        assert_eq!(
            proposal.declaration.analysis_dimensions,
            PREVIEW_FRAME_MAX_DIMENSIONS
        );
        assert_eq!(proposal.declaration.video_view, view);
        assert_eq!(proposal.declaration.target_region, [12, 16, 36, 16]);
        assert_eq!(
            binding
                .proposal([0., 0.], [1., 1.], "chosen")
                .unwrap()
                .declaration
                .target_region,
            [0, 0, 96, 64]
        );
        for (a, b) in [
            ([0., 0.], [0., 1.]),
            ([-0.1, 0.], [1., 1.]),
            ([0., 0.], [1.1, 1.]),
            ([f64::NAN, 0.], [1., 1.]),
        ] {
            assert!(binding.proposal(a, b, "chosen").is_err());
        }
        assert!(binding.proposal([0., 0.], [1., 1.], "not a token").is_err());
        let mut wrong = result.clone();
        wrong.source_frame_index = 5;
        assert!(Binding::checked(&wrong, &view).is_err());
        let mut wrong = view.clone();
        wrong.eye = StereoEye::Left;
        assert!(Binding::checked(&result, &wrong).is_err());
    }
    fn draw(
        author: &mut TargetViewAuthor,
        ctx: &egui::Context,
        result: &PreviewResult,
        view: &VideoView,
        rect: egui::Rect,
        events: Vec<egui::Event>,
    ) -> Result<()> {
        let mut output = Ok(());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640., 480.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    output = author.widget(ui, rect, result, view);
                });
            },
        );
        output
    }
    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn actual_widget_drag_and_freshness() {
        let (mut result, view) = fixture();
        let ctx = egui::Context::default();
        let mut author = TargetViewAuthor::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100., 100.), egui::vec2(192., 128.));
        let a = egui::pos2(124., 132.);
        let b = egui::pos2(196., 164.);
        draw(&mut author, &ctx, &result, &view, rect, vec![]).unwrap();
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![egui::Event::PointerMoved(a), pointer(a, true)],
        )
        .unwrap();
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![egui::Event::PointerMoved(b)],
        )
        .unwrap();
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![pointer(b, false)],
        )
        .unwrap();
        assert_eq!(
            author.draft("chosen").unwrap().declaration.target_region,
            [12, 16, 36, 16]
        );
        assert!(author.draft("invalid name").is_err());
        assert!(author.draft("chosen").is_ok());
        if let Some(path) = std::env::var_os("PULSAR_TEST_AUTHORED_VIEW") {
            save_new(Path::new(&path), &author.draft("chosen").unwrap()).unwrap();
        }
        let original = result.clone();
        for changed in 0..4 {
            let mut altered = original.clone();
            match changed {
                0 => altered.context.source_version = SourceVersionId::new("c".repeat(64)).unwrap(),
                1 => altered.context.frame = FrameId::new(99),
                2 => altered.frame.as_mut().unwrap().artifact.identity.sha256 = "d".repeat(64),
                _ => altered.context.seek_generation += 1,
            }
            assert!(author
                .current_draft(Some(&altered), Some(&view), "chosen")
                .is_err());
        }
        result.context.request_generation += 1;
        draw(&mut author, &ctx, &result, &view, rect, vec![]).unwrap();
        assert!(author.draft("chosen").is_err());
    }
    #[test]
    fn widget_reflow_abandons_gesture() {
        let (result, view) = fixture();
        let ctx = egui::Context::default();
        let mut author = TargetViewAuthor::default();
        let rect = egui::Rect::from_min_size(egui::pos2(100., 100.), egui::vec2(192., 128.));
        let a = egui::pos2(124., 132.);
        let b = egui::pos2(196., 164.);
        draw(&mut author, &ctx, &result, &view, rect, vec![]).unwrap();
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![egui::Event::PointerMoved(a), pointer(a, true)],
        )
        .unwrap();
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![egui::Event::PointerMoved(b)],
        )
        .unwrap();
        assert!(draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect.translate(egui::vec2(1., 0.)),
            vec![]
        )
        .is_err());
        draw(
            &mut author,
            &ctx,
            &result,
            &view,
            rect,
            vec![pointer(b, false)],
        )
        .unwrap();
        assert!(author.draft("chosen").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn saved_declaration_interoperates_without_overwrite() {
        let (result, view) = fixture();
        let proposal = Binding::checked(&result, &view)
            .unwrap()
            .proposal([0., 0.], [1., 1.], "chosen")
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("view.json");
        save_new(&path, &proposal).unwrap();
        let read = crate::inference::read_annotated_view(&path).unwrap();
        assert_eq!(
            serde_json::to_value(read).unwrap(),
            serde_json::to_value(&proposal).unwrap()
        );
        let bytes = std::fs::read(&path).unwrap();
        assert!(save_new(&path, &proposal).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(save_new(&link, &proposal).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
