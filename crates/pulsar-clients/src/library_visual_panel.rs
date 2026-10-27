//! Direct folder recipe authoring; filesystem reads run in the background controller.
use crate::cli::LibraryGenerationOptions;
use anyhow::{ensure, Result};
use eframe::egui;
use std::path::PathBuf;

pub struct LibraryVisualPanel {
    pub preset: String,
    pub model: String,
    pub use_model: bool,
    pub use_pack: bool,
    pub pack: String,
    pub profile: String,
    pub use_profile: bool,
    pub class_count: u32,
    pub width: u32,
    pub height: u32,
    pub override_dimensions: bool,
    pub threshold: f32,
    pub override_threshold: bool,
    pub stride: u32,
    pub video_view: Option<pulsar_protocol::VideoView>,
}
impl Default for LibraryVisualPanel {
    fn default() -> Self {
        Self {
            preset: "default".into(),
            model: String::new(),
            use_model: false,
            use_pack: false,
            pack: String::new(),
            profile: String::new(),
            use_profile: false,
            class_count: 1,
            width: 640,
            height: 640,
            override_dimensions: false,
            threshold: 0.25,
            override_threshold: false,
            stride: 1,
            video_view: None,
        }
    }
}
fn path(text: &str) -> Result<PathBuf> {
    let p = PathBuf::from(text.trim());
    ensure!(
        p.is_absolute()
            && p.as_os_str().as_encoded_bytes().len() <= 32768
            && !p.as_os_str().as_encoded_bytes().contains(&0)
            && !p
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "Choose a bounded absolute path without parent traversal"
    );
    Ok(p)
}
impl LibraryVisualPanel {
    /// Freeze form values only; never read model/profile files on the frame thread.
    pub fn capture(&self) -> Result<LibraryGenerationOptions> {
        ensure!(
            ["default", "fast", "maximum", "quality"].contains(&self.preset.as_str()),
            "Unsupported visual preset"
        );
        if let Some(view) = &self.video_view {
            view.validate()?;
        }
        ensure!(self.stride > 0, "Frame sampling stride must be positive");
        ensure!(
            !(self.use_model && self.use_pack),
            "Choose a file model or selected pack"
        );
        let pack = self.pack.trim();
        ensure!(
            !self.use_pack
                || (pack.len() == 64
                    && pack
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
            "Selected pack digest must be 64 lowercase hexadecimal characters"
        );
        let model = self.use_model.then(|| path(&self.model)).transpose()?;
        let inference_profile = (self.use_model && self.use_profile)
            .then(|| path(&self.profile))
            .transpose()?;
        ensure!(
            !self.use_model || self.use_profile || self.class_count > 0,
            "Declare a positive model class count"
        );
        ensure!(
            !self.override_dimensions
                || (self.width > 0
                    && self.height > 0
                    && self.width <= 16384
                    && self.height <= 16384),
            "Analysis dimensions must be 1..=16384"
        );
        ensure!(
            !(self.use_model || self.use_pack)
                || !self.override_threshold
                || (self.threshold.is_finite() && (0.0..=1.0).contains(&self.threshold)),
            "Detector threshold must be finite and within 0..=1"
        );
        Ok(LibraryGenerationOptions {
            audio_bundle_recipe: None,
            model,
            model_pack: self.use_pack.then(|| pack.to_owned()),
            model_class_count: (self.use_model && !self.use_profile).then_some(self.class_count),
            inference_profile,
            conf: ((self.use_model || self.use_pack) && self.override_threshold)
                .then_some(self.threshold),
            width: self.override_dimensions.then_some(self.width),
            height: self.override_dimensions.then_some(self.height),
            video_view: None,
            preset: self.preset.clone(),
            source_sample_stride: self.stride,
        })
    }
    pub fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut refresh = false;
        egui::ComboBox::from_label("Visual preset")
            .selected_text(&self.preset)
            .show_ui(ui, |ui| {
                for preset in ["default", "fast", "maximum", "quality"] {
                    ui.selectable_value(&mut self.preset, preset.into(), preset);
                }
            });
        if ui
            .checkbox(&mut self.use_pack, "Use current selected pack")
            .changed()
            && self.use_pack
        {
            self.use_model = false;
        }
        if self.use_pack {
            refresh = ui.button("Load current selected pack").clicked();
            ui.label("Exact selected manifest SHA-256");
            ui.add(egui::TextEdit::singleline(&mut self.pack).char_limit(64));
            ui.small("Capture reads the current approved selection and profile. Import or approve a pack in Generator first.");
        } else {
            ui.checkbox(&mut self.use_model, "Use a file model");
        }
        if self.use_model {
            ui.label("Absolute model path");
            ui.add(egui::TextEdit::singleline(&mut self.model).char_limit(4096));
            ui.checkbox(&mut self.use_profile, "Use a declared inference profile");
            if self.use_profile {
                ui.label("Absolute profile JSON path");
                ui.add(egui::TextEdit::singleline(&mut self.profile).char_limit(4096));
            } else {
                ui.horizontal(|ui| {
                    ui.label("Declared YOLO class count (NCHW, RGB)");
                    ui.add(egui::DragValue::new(&mut self.class_count).range(1..=u32::MAX));
                });
                ui.small("Without a profile, the model input uses the declared dimensions below (640 × 640 if no override).");
            }
        }
        if self.use_model || self.use_pack {
            ui.checkbox(&mut self.override_threshold, "Set detector score threshold");
            if self.override_threshold {
                ui.add(egui::DragValue::new(&mut self.threshold).range(0.0..=1.0));
            }
        }
        ui.checkbox(&mut self.override_dimensions, "Set analysis dimensions");
        if self.override_dimensions {
            ui.horizontal(|ui| {
                ui.label("Width / height");
                ui.add(egui::DragValue::new(&mut self.width).range(1..=16384));
                ui.add(egui::DragValue::new(&mut self.height).range(1..=16384));
            });
        }
        ui.horizontal(|ui| {
            ui.label("Analyze every Nth original frame");
            ui.add(egui::DragValue::new(&mut self.stride).range(1..=u32::MAX));
        });
        crate::video_view_panel::show(ui, &mut self.video_view);
        ui.small("Source timing is preserved. The declared view applies to every captured input; use separate batches for different layouts or projections.");
        refresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_capture_freezes_digest_and_ignores_inactive_file_contract() {
        let editor = LibraryVisualPanel {
            use_pack: true,
            pack: format!(" {} ", "a".repeat(64)),
            model: "ignored relative model".into(),
            profile: "ignored profile".into(),
            use_profile: true,
            class_count: 0,
            override_threshold: true,
            threshold: 0.6,
            stride: 3,
            ..Default::default()
        };
        let options = editor.capture().unwrap();
        assert_eq!(options.model_pack, Some("a".repeat(64)));
        assert!(
            options.model.is_none()
                && options.model_class_count.is_none()
                && options.inference_profile.is_none()
        );
        assert_eq!(options.conf, Some(0.6));
        assert_eq!(options.source_sample_stride, 3);
        let (settings, model) = crate::library_generation::configure(&options).unwrap();
        assert!(model.is_none() && settings.model_input.is_none());
        let mut changed = editor;
        changed.pack = "b".repeat(64);
        assert_eq!(options.model_pack, Some("a".repeat(64)));
        changed.use_model = true;
        assert!(changed.capture().is_err());
    }
    #[test]
    fn pack_controls_refuse_bad_digest_and_threshold_without_filesystem_reads() {
        let mut editor = LibraryVisualPanel {
            use_pack: true,
            ..Default::default()
        };
        for digest in [
            String::new(),
            "A".repeat(64),
            "a".repeat(63),
            "g".repeat(64),
        ] {
            editor.pack = digest;
            assert!(editor.capture().is_err());
        }
        editor.pack = "a".repeat(64);
        editor.override_threshold = true;
        editor.threshold = f32::NAN;
        assert!(editor.capture().is_err());
        editor.threshold = 0.6;
        assert!(editor.capture().is_ok());
    }
    #[test]
    fn capture_refuses_invalid_values_without_reading_files() {
        let mut editor = LibraryVisualPanel::default();
        assert!(editor.capture().is_ok());
        editor.use_model = true;
        editor.model = "relative.onnx".into();
        assert!(editor.capture().is_err());
        editor.model = "/explicit/not-present.onnx".into();
        assert!(editor.capture().is_ok());
        editor.class_count = 0;
        assert!(editor.capture().is_err());
        editor.class_count = 1;
        editor.override_threshold = true;
        editor.threshold = f32::NAN;
        assert!(editor.capture().is_err());
        editor.override_threshold = false;
        editor.override_dimensions = true;
        editor.width = 0;
        assert!(editor.capture().is_err());
        editor.width = 64;
        editor.height = 64;
        editor.stride = 0;
        assert!(editor.capture().is_err());
        editor.stride = 2;
        let options = editor.capture().unwrap();
        let (settings, model) = crate::library_generation::configure(&options).unwrap();
        assert_eq!(settings.analysis_width, 64);
        assert_eq!(settings.source_sample_stride, 2);
        assert_eq!(model.unwrap(), PathBuf::from("/explicit/not-present.onnx"));
    }
    #[test]
    fn view_capture_checks_shared_numeric_and_half_sphere_geometry_without_files() {
        use pulsar_protocol::{
            PerspectiveView, StereoEye, StereoLayout, VideoProjection, VideoView,
        };
        let mut editor = LibraryVisualPanel::default();
        let mut view = VideoView {
            layout: StereoLayout::Mono,
            eye: StereoEye::Left,
            projection: VideoProjection::HalfEquirectangular180(PerspectiveView {
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
                roll_degrees: 0.0,
                horizontal_fov_degrees: 60.0,
                vertical_fov_degrees: 60.0,
            }),
        };
        editor.video_view = Some(view.clone());
        assert!(editor.capture().is_ok());
        if let VideoProjection::HalfEquirectangular180(v) = &mut view.projection {
            v.yaw_degrees = 70.0;
        }
        editor.video_view = Some(view.clone());
        assert!(editor.capture().is_err());
        if let VideoProjection::HalfEquirectangular180(v) = &mut view.projection {
            v.yaw_degrees = f64::NAN;
        }
        editor.video_view = Some(view);
        assert!(editor.capture().is_err());
        editor.video_view = Some(VideoView {
            layout: StereoLayout::Mono,
            eye: StereoEye::Right,
            projection: VideoProjection::Flat,
        });
        assert!(editor.capture().is_err());
        editor.video_view = None;
        assert!(editor.capture().unwrap().video_view.is_none());
    }
}
