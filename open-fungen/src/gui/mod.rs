//! GUI module for Open-FunGen.

pub mod app;
pub mod timeline;

use app::FunGenApp;
use eframe::egui::vec2;

/// Launch the interactive GUI application
pub fn run_gui() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Open-FunGen - Funscript Generator & Editor")
            .with_inner_size(vec2(1280.0, 800.0))
            .with_min_inner_size(vec2(800.0, 500.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Open-FunGen",
        native_options,
        Box::new(|_cc| Ok(Box::new(FunGenApp::default()))),
    )
}
