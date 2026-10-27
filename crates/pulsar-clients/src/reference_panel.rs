//! Reference declarations and a read-only comparison; never edits project motion.
use crate::reference_set::{self, ReferenceEntry, ReferenceEntryInput, ReferenceViewInput};
use eframe::egui::{self, Color32, Stroke};
use pulsar_protocol::*;
use std::{path::PathBuf, thread::JoinHandle};

pub(crate) struct PreviewSelection {
    pub source: SourceVersionId,
    pub path: PathBuf,
    pub view: VideoView,
}

enum Reply {
    Entry(Box<ReferenceEntry>),
    Opened(String, Vec<ReferenceEntry>),
    Exported(PathBuf),
}

#[derive(Default)]
pub(crate) struct ReferencePanel {
    root: String,
    authorization: String,
    id: String,
    group: String,
    video: String,
    reference: String,
    view_path: String,
    attached_view: Option<AnnotatedGenerationView>,
    offset_ms: i64,
    entries: Vec<ReferenceEntry>,
    selected: Option<usize>,
    destination: String,
    open_path: String,
    message: String,
    job: Option<JoinHandle<Result<Reply, String>>>,
}

impl ReferencePanel {
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn launch(&mut self, work: impl FnOnce() -> anyhow::Result<Reply> + Send + 'static) {
        if self.busy() {
            return;
        }
        match std::thread::Builder::new()
            .name("pulsar-reference-set".into())
            .spawn(move || work().map_err(|e| format!("{e:#}")))
        {
            Ok(job) => {
                self.job = Some(job);
                self.message = "Checking files; this operation keeps its original inputs.".into();
            }
            Err(e) => self.message = format!("Could not start file operation: {e}"),
        }
    }

    pub(crate) fn poll(&mut self) {
        if !self.job.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        let outcome = self.job.take().expect("finished job").join();
        match outcome {
            Ok(Ok(Reply::Entry(entry))) => self.accept_entry(*entry),
            Ok(Ok(Reply::Opened(authorization, entries))) => {
                self.root = entries
                    .first()
                    .map(|e| e.input_root.display().to_string())
                    .unwrap_or_default();
                self.entries = entries;
                self.authorization = authorization;
                self.selected = (!self.entries.is_empty()).then_some(0);
                self.message = "Reference set reopened; source and script bytes rechecked. Alignment remains declared.".into();
            }
            Ok(Ok(Reply::Exported(path))) => {
                self.message = format!("Saved reference declarations to {}. Target planning and timing review remain separate.", path.display());
            }
            Ok(Err(e)) => self.message = e,
            Err(_) => self.message =
                "Reference operation panicked. Inspect the destination before retrying an export."
                    .into(),
        }
    }

    fn accept_entry(&mut self, entry: ReferenceEntry) {
        if self.entries.len() >= 64 {
            self.message = "A set can contain at most 64 entries.".into();
        } else if self
            .entries
            .iter()
            .any(|e| e.id == entry.id || e.source_sha256 == entry.source_sha256)
        {
            self.message = "This entry ID or source content is already in the set. Remove it before adding a replacement.".into();
        } else if self
            .entries
            .first()
            .is_some_and(|e| e.input_root != entry.input_root)
        {
            self.message = "All entries must use the same Live-Reference folder.".into();
        } else {
            self.root = entry.input_root.display().to_string();
            self.entries.push(entry);
            self.selected = Some(self.entries.len() - 1);
            self.message = "Source, script and target-view binding checked. Preview the timing before exporting.".into();
        }
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current_view: Option<AnnotatedGenerationView>,
        can_preview: bool,
    ) -> Option<PreviewSelection> {
        let mut selection = None;
        ui.heading("Reference sets");
        ui.label("Pair training videos with supplied scripts and a static target view.");
        ui.small("Timing offsets are declarations. Previewing or saving does not mark alignment reviewed. Sterile evaluation references use the separate evaluation workflow.");
        let busy = self.busy();
        ui.add_enabled_ui(!busy, |ui| {
            ui.collapsing("Open saved reference set", |ui| {
                field(ui, "Saved set folder", &mut self.open_path);
                if ui.add_enabled(!self.open_path.trim().is_empty(), egui::Button::new("Open reference set")).clicked() {
                    let path = PathBuf::from(self.open_path.trim());
                    self.launch(move || {
                        let (authorization, entries) = reference_set::load_set(&path)?;
                        Ok(Reply::Opened(authorization, entries))
                    });
                }
                ui.small("Opening replaces this panel's unsaved set after every input has passed validation.");
            });
            ui.separator();
            ui.add_enabled_ui(self.entries.is_empty(), |ui| field(ui, "Live-Reference folder", &mut self.root));
            field(ui, "Authorization record", &mut self.authorization);
            ui.collapsing("Add source / script pair", |ui| {
                field(ui, "Entry ID", &mut self.id);
                field(ui, "Source group", &mut self.group);
                field(ui, "Video path (relative to root or absolute)", &mut self.video);
                field(ui, "Reference script path", &mut self.reference);
                ui.small("Reference positions are used as supplied; the training reader ignores the funscript inverted flag.");
                field(ui, "Saved target-view path", &mut self.view_path);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(current_view.is_some(), egui::Button::new("Attach current target view")).clicked() {
                        self.attached_view = current_view.clone();
                    }
                    if ui.add_enabled(self.attached_view.is_some(), egui::Button::new("Use saved view path instead")).clicked() {
                        self.attached_view = None;
                    }
                });
                if let Some(view) = &self.attached_view {
                    ui.label(format!("Attached target {} · {:?}", view.declaration.target_id, view.declaration.target_region));
                    ui.small(format!("Source {}", view.source_sha256));
                } else {
                    ui.small("Draw a view in Generator / Studio, or select a saved view file.");
                }
                offset_control(ui, &mut self.offset_ms);
                if ui.add_enabled(self.entries.len() < 64, egui::Button::new("Check and add pair")).clicked() {
                    let root = PathBuf::from(self.root.trim());
                    let input = ReferenceEntryInput {
                        id: self.id.trim().to_owned(),
                        group: self.group.trim().to_owned(),
                        video: PathBuf::from(self.video.trim()),
                        reference: PathBuf::from(self.reference.trim()),
                        view: self.attached_view.clone().map(ReferenceViewInput::Declared)
                            .unwrap_or_else(|| ReferenceViewInput::File(PathBuf::from(self.view_path.trim()))),
                        offset_ms: self.offset_ms,
                    };
                    self.launch(move || reference_set::load_entry(&root, input).map(|e| Reply::Entry(Box::new(e))));
                }
            });
            ui.separator();
            ui.label(format!("{} / 64 pairs", self.entries.len()));
            for (index, entry) in self.entries.iter().enumerate() {
                if ui.selectable_label(self.selected == Some(index), format!("{} · {}", entry.id, entry.group)).clicked() {
                    self.selected = Some(index);
                }
            }
            if let Some(index) = self.selected.filter(|i| *i < self.entries.len()) {
                let entry = &mut self.entries[index];
                ui.label(format!("Video: {}", entry.video.display()));
                ui.label(format!("Script: {} ({} points)", entry.reference.display(), entry.curve.points().len()));
                ui.label(format!("Target: {} · {:?}", entry.view.declaration.target_id, entry.view.declaration.target_region));
                offset_control(ui, &mut entry.offset_ms);
                if ui.add_enabled(can_preview, egui::Button::new("Open source in preview")).clicked() {
                    match SourceVersionId::new(entry.source_sha256.clone()) {
                        Ok(source) => selection = Some(PreviewSelection {
                            source,
                            path: entry.input_root.join(&entry.video),
                            view: entry.view.declaration.video_view.clone(),
                        }),
                        Err(e) => self.message = e.to_string(),
                    }
                }
                ui.small("Uses this source's declared view; imports into the open project if needed.");
                if ui.button("Remove selected pair").clicked() {
                    self.entries.remove(index);
                    self.selected = (!self.entries.is_empty()).then_some(index.min(self.entries.len().saturating_sub(1)));
                }
            }
            ui.separator();
            field(ui, "New export folder", &mut self.destination);
            if ui.add_enabled(!self.entries.is_empty() && !self.authorization.trim().is_empty() && !self.destination.trim().is_empty(), egui::Button::new("Export new training set")).clicked() {
                let path = PathBuf::from(self.destination.trim());
                let authorization = self.authorization.clone();
                let entries = self.entries.clone();
                self.launch(move || reference_set::export_new(&path, &authorization, &entries).map(Reply::Exported));
            }
            ui.small("Exports manifest, views and an authoring record. Existing destinations are preserved. No media is copied or model trained.");
        });
        if busy {
            ui.spinner();
        }
        ui.label(&self.message);
        selection
    }

    pub(crate) fn comparison(
        &self,
        ui: &mut egui::Ui,
        preview: Option<&PreviewResult>,
    ) -> Option<f64> {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)) else {
            ui.label(
                "Select a reference pair to compare its script against the displayed source frame.",
            );
            return None;
        };
        ui.label(format!(
            "Reference: {} · offset {:+} ms · read-only",
            entry.id, entry.offset_ms
        ));
        let elapsed = comparison_time(&entry.source_sha256, &entry.view, preview);
        let sampled = elapsed
            .as_ref()
            .ok()
            .map(|time| entry.curve.sample_at(*time, entry.offset_ms));
        match &sampled {
            Some(Ok(Some(sample))) => {
                ui.label(format!(
                    "Displayed source {:.6} s → script {:.6} s → position {:.2}%",
                    seconds(*elapsed.as_ref().expect("sample has time")),
                    seconds(sample.script_time),
                    sample.display_position() * 100.0
                ));
            }
            Some(Ok(None)) => {
                ui.label("Outside reference coverage; no position supplied.");
            }
            Some(Err(error)) => {
                ui.colored_label(Color32::YELLOW, error.to_string());
            }
            None => {
                ui.colored_label(
                    Color32::YELLOW,
                    elapsed
                        .as_ref()
                        .err()
                        .copied()
                        .unwrap_or("Preview unavailable"),
                );
            }
        }
        let points = entry.curve.points();
        let start = f64::from(points[0].at_ms) / 1000.0 - entry.offset_ms as f64 / 1000.0;
        let end =
            f64::from(points[points.len() - 1].at_ms) / 1000.0 - entry.offset_ms as f64 / 1000.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(1.0), 105.0),
            egui::Sense::click(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, Color32::from_rgb(8, 11, 17));
        for fraction in [0.0, 0.5, 1.0] {
            let y = rect.bottom() - rect.height() * fraction;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0_f32, Color32::DARK_GRAY),
            );
        }
        // Limit paint cost by summarizing each bucket's extrema, retaining their
        // original order; the actual interpolation always uses every source knot.
        let budget = (rect.width().ceil() as usize).clamp(32, 2048);
        let mut line = Vec::new();
        let bucket = points.len().div_ceil(budget);
        for chunk in points.chunks(bucket.max(1)) {
            let min = chunk
                .iter()
                .enumerate()
                .min_by_key(|(_, p)| p.pos)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let max = chunk
                .iter()
                .enumerate()
                .max_by_key(|(_, p)| p.pos)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let mut indices = vec![0, min, max, chunk.len() - 1];
            indices.sort_unstable();
            indices.dedup();
            for i in indices {
                let p = chunk[i];
                let t = f64::from(p.at_ms) / 1000.0 - entry.offset_ms as f64 / 1000.0;
                line.push(egui::pos2(
                    rect.left() + ((t - start) / (end - start)) as f32 * rect.width(),
                    rect.bottom() - f32::from(p.pos) / 100.0 * rect.height(),
                ));
            }
        }
        painter.add(egui::Shape::line(
            line,
            Stroke::new(1.5_f32, Color32::from_rgb(69, 222, 230)),
        ));
        if let Ok(time) = elapsed {
            let fraction = (seconds(time) - start) / (end - start);
            if (0.0..=1.0).contains(&fraction) {
                let x = rect.left() + fraction as f32 * rect.width();
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1.5_f32, Color32::WHITE),
                );
            }
        }
        ui.small(format!(
            "Source elapsed {:.3}–{:.3} s. Click to seek; curve points cannot be edited here.",
            start, end
        ));
        response
            .clicked()
            .then(|| response.interact_pointer_pos())
            .flatten()
            .filter(|_| {
                preview.is_some_and(|result| result.placed.is_none())
                    && comparison_time(&entry.source_sha256, &entry.view, preview).is_ok()
            })
            .map(|p| {
                (start + f64::from((p.x - rect.left()) / rect.width()) * (end - start))
                    .clamp(0.0, 86400.0)
            })
    }
}

impl Drop for ReferencePanel {
    fn drop(&mut self) {
        // Desktop defers normal close until terminal polling has joined this
        // handle. Also retain custody if the owner is dropped by another caller.
        if let Some(job) = self.job.take() {
            let _ = job.join();
        }
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .char_limit(4096)
            .desired_width(f32::INFINITY),
    );
}
fn offset_control(ui: &mut egui::Ui, offset: &mut i64) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Script offset");
        ui.add(
            egui::DragValue::new(offset)
                .speed(1)
                .range(-86_400_000..=86_400_000)
                .suffix(" ms"),
        );
    });
    ui.small("Script time = elapsed source time + offset.");
}
fn seconds(time: ExactSeconds) -> f64 {
    time.numerator() as f64 / time.denominator() as f64
}

fn comparison_time(
    source: &str,
    view: &AnnotatedGenerationView,
    preview: Option<&PreviewResult>,
) -> Result<ExactSeconds, &'static str> {
    let result = preview.ok_or("Request a matching source frame to compare timing.")?;
    if result.context.source_version.to_string() != source || view.source_sha256 != source {
        return Err(
            "The displayed frame belongs to a different source. Open this pair in preview.",
        );
    }
    if !result.matches_request(&result.context)
        || result.annotated.is_some()
        || result.frame.is_none()
    {
        return Err("Comparison requires an accepted, uncropped source frame.");
    }
    let map = result
        .coordinate_map
        .as_ref()
        .ok_or("Preview has no declared view.")?;
    let [width, height] = view.declaration.source_dimensions;
    if map.validate().is_err()
        || map.view() != &view.declaration.video_view
        || map.source_dimensions() != (width, height)
        || video_view_transform(Some(&view.declaration.video_view))
            .ok()
            .as_ref()
            != Some(&result.context.transform)
    {
        return Err("Displayed view or source dimensions differ from this reference pair.");
    }
    let elapsed = result
        .elapsed_source_time
        .ok_or("Decoded source elapsed time is unavailable.")?;
    Ok(ExactSeconds::from_source(elapsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;

    fn fixture(parent: &std::path::Path) -> (ReferenceEntry, PreviewResult) {
        let root = parent.join("Live-Reference");
        fs::create_dir(&root).unwrap();
        let video = b"authored neutral source identity fixture; not decoded media";
        fs::write(root.join("neutral.mp4"), video).unwrap();
        fs::write(root.join("neutral.funscript"), br#"{"inverted":true,"actions":[{"at":0,"pos":20},{"at":1000,"pos":80},{"at":2000,"pos":40}]}"#).unwrap();
        let hash = format!("{:x}", Sha256::digest(video));
        let video_view = VideoView {
            layout: StereoLayout::SideBySideLeftRight,
            eye: StereoEye::Right,
            projection: VideoProjection::Flat,
        };
        let view = AnnotatedGenerationView {
            source_sha256: hash.clone(),
            declaration: AnnotatedTemporalView {
                schema_version: 1,
                source_dimensions: [192, 64],
                analysis_dimensions: [96, 64],
                video_view: video_view.clone(),
                target_region: [8, 4, 32, 24],
                target_id: "neutral-target".into(),
            },
        };
        let entry = reference_set::load_entry(
            &root,
            ReferenceEntryInput {
                id: "neutral".into(),
                group: "authored".into(),
                video: "neutral.mp4".into(),
                reference: "neutral.funscript".into(),
                view: ReferenceViewInput::Declared(view),
                offset_ms: 250,
            },
        )
        .unwrap();
        let context = FrameContext {
            source_version: SourceVersionId::new(hash).unwrap(),
            source_placement: SourcePlacementId::new("primary").unwrap(),
            frame: FrameId::new(4),
            transform: video_view_transform(Some(&video_view)).unwrap(),
            seek_generation: 1,
            request_generation: 2,
        };
        let result = PreviewResult {
            annotated: None,
            placed: None,
            time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
            elapsed_source_time: Some(SourceTimestamp::new(1, 2).unwrap()),
            source_origin: Some(SourceTimestamp::new(7, 1).unwrap()),
            coordinate_map: Some(ViewCoordinateMap::new(video_view, 192, 64, 96, 64).unwrap()),
            context,
            requested_source_time: SourceTimestamp::new(3, 4).unwrap(),
            source_time: SourceTimestamp::new(15, 2).unwrap(),
            source_frame_index: 4,
            observations: vec![],
            frame: Some(FrameArtifact {
                artifact: ArtifactReference {
                    path: root.join("frame.raw"),
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new("frame").unwrap(),
                        sha256: "b".repeat(64),
                        byte_len: 96 * 64 * 3,
                    },
                },
                width: 96,
                height: 64,
                stride_bytes: 96 * 3,
                format: PixelFormat::Rgb8,
            }),
            analysis: AnalysisStatus::Available,
        };
        (entry, result)
    }

    #[test]
    fn comparison_uses_actual_elapsed_frame_time_and_rejects_changed_binding() {
        let temp = tempfile::tempdir().unwrap();
        let (entry, result) = fixture(temp.path());
        let time = comparison_time(&entry.source_sha256, &entry.view, Some(&result)).unwrap();
        assert_eq!(time, ExactSeconds::new(1, 2).unwrap());
        let sample = entry
            .curve
            .sample_at(time, entry.offset_ms)
            .unwrap()
            .unwrap();
        assert_eq!(sample.script_time, ExactSeconds::new(3, 4).unwrap());
        assert_eq!(
            (sample.position_numerator, sample.position_denominator),
            (13, 20)
        );
        assert!(comparison_time(&entry.source_sha256, &entry.view, None).is_err());
        for change in 0..5 {
            let mut changed = result.clone();
            match change {
                0 => changed.context.source_version = SourceVersionId::new("c".repeat(64)).unwrap(),
                1 => changed.context.frame = FrameId::new(5),
                2 => changed.elapsed_source_time = None,
                3 => changed.frame = None,
                _ => {
                    changed.coordinate_map = Some(
                        ViewCoordinateMap::new(
                            entry.view.declaration.video_view.clone(),
                            384,
                            64,
                            96,
                            64,
                        )
                        .unwrap(),
                    )
                }
            }
            assert!(
                comparison_time(&entry.source_sha256, &entry.view, Some(&changed)).is_err(),
                "change {change}"
            );
        }
    }

    #[test]
    fn reference_panel_widget_paints_and_seeks_without_changing_supplied_points() {
        let temp = tempfile::tempdir().unwrap();
        let (entry, preview) = fixture(temp.path());
        let original = entry.curve.points().to_vec();
        let mut panel = ReferencePanel::default();
        panel.accept_entry(entry);
        let ctx = egui::Context::default();
        let mut seek = None;
        let mut text = Vec::new();
        for pressed in [None, Some(true), Some(false)] {
            let pos = egui::pos2(200., 80.);
            let events = pressed
                .map(|pressed| {
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ]
                })
                .unwrap_or_default();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640., 240.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        seek = panel.comparison(ui, Some(&preview));
                    });
                },
            );
            for shape in &output.shapes {
                if let egui::Shape::Text(t) = &shape.shape {
                    text.push(t.galley.job.text.clone());
                }
            }
        }
        assert!(seek.is_some_and(|t| (0.0..2.0).contains(&t)));
        assert_eq!(panel.entries[0].curve.points(), original);
        assert!(text.iter().any(|t| t.contains("65.00%")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("read-only")));
        let mut wrong = preview;
        wrong.context.source_version = SourceVersionId::new("d".repeat(64)).unwrap();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(panel.comparison(ui, Some(&wrong)).is_none());
            });
        });
    }

    #[test]
    fn retained_background_input_does_not_follow_later_form_changes() {
        let temp = tempfile::tempdir().unwrap();
        let (entry, _) = fixture(temp.path());
        let original = entry.id.clone();
        let mut panel = ReferencePanel::default();
        panel.launch(move || Ok(Reply::Entry(Box::new(entry))));
        panel.id = "new-form-id".into();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while panel.busy() && std::time::Instant::now() < deadline {
            panel.poll();
            std::thread::yield_now();
        }
        assert!(!panel.busy());
        assert_eq!(panel.entries[0].id, original);
        assert_eq!(panel.id, "new-form-id");
    }

    #[test]
    fn background_export_and_reopen_preserve_inputs_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let (entry, _) = fixture(temp.path());
        let path = temp.path().join("export");
        let mut panel = ReferencePanel::default();
        panel.accept_entry(entry);
        let entries = panel.entries.clone();
        let destination = path.clone();
        panel.launch(move || {
            reference_set::export_new(&destination, "Authored neutral software test", &entries)
                .map(Reply::Exported)
        });
        finish(&mut panel);
        assert!(panel.message.starts_with("Saved reference declarations"));
        panel.entries[0].offset_ms = -250;
        panel.launch(move || {
            let (authorization, entries) = reference_set::load_set(&path)?;
            Ok(Reply::Opened(authorization, entries))
        });
        finish(&mut panel);
        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].offset_ms, 250);
        assert_eq!(panel.authorization, "Authored neutral software test");
        panel.launch(|| anyhow::bail!("declared read failure"));
        finish(&mut panel);
        assert_eq!(panel.entries.len(), 1);
        assert_eq!(panel.entries[0].offset_ms, 250);
        assert_eq!(panel.message, "declared read failure");
    }

    fn finish(panel: &mut ReferencePanel) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while panel.busy() && std::time::Instant::now() < deadline {
            panel.poll();
            std::thread::yield_now();
        }
        assert!(!panel.busy());
    }
}
