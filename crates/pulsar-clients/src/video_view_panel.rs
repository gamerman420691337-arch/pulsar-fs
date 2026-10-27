//! Shared explicit view authoring for single-source and retained folder consumers.
use eframe::egui;
use pulsar_protocol::{PerspectiveView, StereoEye, StereoLayout, VideoProjection, VideoView};

/// Edit form values only. Source geometry is checked by the shared media path.
pub(crate) fn show(ui: &mut egui::Ui, selection: &mut Option<VideoView>) {
    let mut enabled = selection.is_some();
    if ui
        .checkbox(&mut enabled, "Declare stereo / spherical source view")
        .changed()
    {
        *selection = enabled.then_some(VideoView {
            layout: StereoLayout::Mono,
            eye: StereoEye::Left,
            projection: VideoProjection::Flat,
        });
    }
    if let Some(view) = selection {
        egui::ComboBox::from_label("Source layout")
            .selected_text(format!("{:?}", view.layout))
            .show_ui(ui, |ui| {
                for (layout, label) in [
                    (StereoLayout::Mono, "Mono"),
                    (
                        StereoLayout::SideBySideLeftRight,
                        "Side by side: left / right",
                    ),
                    (
                        StereoLayout::SideBySideRightLeft,
                        "Side by side: right / left",
                    ),
                    (
                        StereoLayout::TopBottomLeftRight,
                        "Top / bottom: left / right",
                    ),
                    (
                        StereoLayout::TopBottomRightLeft,
                        "Top / bottom: right / left",
                    ),
                ] {
                    ui.selectable_value(&mut view.layout, layout, label);
                }
            });
        if view.layout == StereoLayout::Mono {
            view.eye = StereoEye::Left;
        } else {
            egui::ComboBox::from_label("Analysis eye")
                .selected_text(format!("{:?}", view.eye))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut view.eye, StereoEye::Left, "Left");
                    ui.selectable_value(&mut view.eye, StereoEye::Right, "Right");
                });
        }
        let mut kind = match view.projection {
            VideoProjection::Flat => 0,
            VideoProjection::Equirectangular360(_) => 1,
            VideoProjection::HalfEquirectangular180(_) => 2,
        };
        let previous = kind;
        egui::ComboBox::from_label("Projection")
            .selected_text(["Flat", "Equirectangular 360", "Half equirectangular VR180"][kind])
            .show_ui(ui, |ui| {
                for (index, label) in ["Flat", "Equirectangular 360", "Half equirectangular VR180"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut kind, index, *label);
                }
            });
        if kind != previous {
            let viewport = PerspectiveView {
                yaw_degrees: 0.0,
                pitch_degrees: 0.0,
                roll_degrees: 0.0,
                horizontal_fov_degrees: 90.0,
                vertical_fov_degrees: 90.0,
            };
            view.projection = match kind {
                1 => VideoProjection::Equirectangular360(viewport),
                2 => VideoProjection::HalfEquirectangular180(viewport),
                _ => VideoProjection::Flat,
            };
        }
        if let VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v) =
            &mut view.projection
        {
            ui.add(egui::Slider::new(&mut v.yaw_degrees, -180.0..=180.0).text("Yaw degrees"));
            ui.add(egui::Slider::new(&mut v.pitch_degrees, -90.0..=90.0).text("Pitch degrees"));
            ui.add(egui::Slider::new(&mut v.roll_degrees, -180.0..=180.0).text("Roll degrees"));
            ui.add(
                egui::Slider::new(&mut v.horizontal_fov_degrees, 1.0..=170.0)
                    .text("Horizontal field of view"),
            );
            ui.add(
                egui::Slider::new(&mut v.vertical_fov_degrees, 1.0..=170.0)
                    .text("Vertical field of view"),
            );
        }
        ui.small("Analysis uses the selected view. Eye order and projection are explicit; stereo depth and six-axis reconstruction remain unavailable.");
    }
}
