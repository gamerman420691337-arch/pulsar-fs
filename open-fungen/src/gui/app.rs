//! Main FunGen desktop application window.

use anyhow::Context;
use crate::funscript::{diagnose_funscript, DoctorConfig, DoctorReport, Funscript};
use crate::gui::timeline::{show_timeline, TimelineState};
use eframe::egui::{
    self, Color32, Context as EguiContext, ProgressBar, RichText, TopBottomPanel, Ui,
};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

pub enum WorkerMessage {
    Progress { percent: f32, status: String },
    Done(Result<Funscript, String>),
}

pub struct FunGenApp {
    /// Currently loaded funscript
    pub script: Funscript,
    /// Path to the loaded funscript file
    pub script_path: Option<PathBuf>,
    /// Interactive timeline state (zoom, pan, selection, playhead)
    pub timeline_state: TimelineState,
    /// Latest Script Doctor quality audit report
    pub doctor_report: Option<DoctorReport>,

    // Generator settings
    pub video_path: Option<PathBuf>,
    pub model_path: Option<PathBuf>,
    pub target_fps: f64,
    pub neural_conf: f32,
    pub pov_mode: bool,
    pub vr_mode: bool,

    // Background generation task state
    pub is_generating: bool,
    pub generation_progress: f32,
    pub generation_status: String,
    worker_rx: Option<Receiver<WorkerMessage>>,
}

impl Default for FunGenApp {
    fn default() -> Self {
        let script = Funscript::new(vec![]);
        let doctor_report = Some(diagnose_funscript(&script, &DoctorConfig::default()));

        Self {
            script,
            script_path: None,
            timeline_state: TimelineState::default(),
            doctor_report,

            video_path: None,
            model_path: Some(PathBuf::from(
                "/home/brianklam/Desktop/Funscripts/FunGen/fungen-data/cache/models/FunGen-12n-pov-1.1.0.onnx",
            )),
            target_fps: 30.0,
            neural_conf: 0.35,
            pov_mode: false,
            vr_mode: false,

            is_generating: false,
            generation_progress: 0.0,
            generation_status: "Idle".to_string(),
            worker_rx: None,
        }
    }
}

impl eframe::App for FunGenApp {
    fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        // 1. Poll background worker messages
        self.poll_worker();

        // 2. Top Menu Bar
        TopBottomPanel::top("top_menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Funscript...").clicked() {
                        self.open_funscript_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Save Funscript").clicked() {
                        self.save_funscript();
                        ui.close_menu();
                    }
                    if ui.button("Save Funscript As...").clicked() {
                        self.save_funscript_as();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Select Video...").clicked() {
                        self.select_video_dialog();
                        ui.close_menu();
                    }
                    if ui.button("Select ONNX Model...").clicked() {
                        self.select_model_dialog();
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Zoom In (Scroll Up)").clicked() {
                        self.timeline_state.view_duration_ms =
                            (self.timeline_state.view_duration_ms * 0.75).max(500.0);
                        ui.close_menu();
                    }
                    if ui.button("Zoom Out (Scroll Down)").clicked() {
                        self.timeline_state.view_duration_ms =
                            (self.timeline_state.view_duration_ms * 1.33).min(3_600_000.0);
                        ui.close_menu();
                    }
                    if ui.button("Reset Zoom (Fit Script)").clicked() {
                        self.fit_timeline();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Audit", |ui| {
                    if ui.button("Run Script Doctor").clicked() {
                        self.run_doctor();
                        ui.close_menu();
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.is_generating {
                        ui.spinner();
                        ui.label(RichText::new(&self.generation_status).color(Color32::from_rgb(0, 200, 255)));
                    } else if let Some(ref path) = self.script_path {
                        ui.label(
                            RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
                                .color(Color32::from_rgb(180, 190, 205)),
                        );
                    }
                });
            });
        });

        // 3. Right Side Inspector Panel (Script Doctor & Selected Point Editor)
        egui::SidePanel::right("inspector_panel")
            .resizable(true)
            .default_width(280.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                self.render_selected_action_editor(ui);
                ui.separator();
                self.render_doctor_inspector(ui);
                ui.separator();
                self.render_generator_controls(ui);
            });

        // 4. Central Panel: Timeline Canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Interactive Keyframe Timeline");
            ui.label(
                RichText::new("Scroll: Zoom | Middle/Ctrl+Drag: Pan | Left Drag: Move Keyframe | Double-Click: Add | Right-Click: Delete")
                    .size(11.0)
                    .color(Color32::from_rgb(130, 140, 155)),
            );
            ui.add_space(4.0);

            show_timeline(
                ui,
                &mut self.script,
                &mut self.timeline_state,
                self.doctor_report.as_ref(),
            );
        });

        // Request repaint if generating
        if self.is_generating {
            ctx.request_repaint();
        }
    }
}

impl FunGenApp {
    fn poll_worker(&mut self) {
        let mut messages = Vec::new();
        if let Some(ref rx) = self.worker_rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }

        for msg in messages {
            match msg {
                WorkerMessage::Progress { percent, status } => {
                    self.generation_progress = percent;
                    self.generation_status = status;
                }
                WorkerMessage::Done(result) => {
                    self.is_generating = false;
                    self.worker_rx = None;
                    match result {
                        Ok(new_script) => {
                            self.script = new_script;
                            self.run_doctor();
                            self.fit_timeline();
                            self.generation_status = "Generation Complete!".to_string();
                        }
                        Err(e) => {
                            self.generation_status = format!("Error: {e}");
                        }
                    }
                }
            }
        }
    }

    fn render_selected_action_editor(&mut self, ui: &mut Ui) {
        ui.heading("Keyframe Editor");
        if let Some(idx) = self.timeline_state.selected_index {
            if idx < self.script.actions.len() {
                let action = &mut self.script.actions[idx];

                ui.horizontal(|ui| {
                    ui.label("Time (ms):");
                    ui.add(egui::DragValue::new(&mut action.at).speed(10.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Position:");
                    ui.add(egui::Slider::new(&mut action.pos, 0..=100));
                });

                if ui.button("Delete Keyframe").clicked() {
                    self.script.actions.remove(idx);
                    self.timeline_state.selected_index = None;
                    self.run_doctor();
                }
            }
        } else {
            ui.label(RichText::new("Click a keyframe node on the timeline to edit.").color(Color32::GRAY));
        }
    }

    fn render_doctor_inspector(&mut self, ui: &mut Ui) {
        ui.heading("Script Doctor");
        if let Some(ref rep) = self.doctor_report {
            ui.horizontal(|ui| {
                ui.label("Total Actions:");
                ui.strong(format!("{}", rep.total_actions));
            });
            ui.horizontal(|ui| {
                ui.label("Duration:");
                ui.strong(format!("{:.1}s", rep.duration_ms as f64 / 1000.0));
            });
            ui.horizontal(|ui| {
                ui.label("Avg Speed:");
                ui.strong(format!("{:.1} u/s", rep.avg_speed_units_per_sec));
            });
            ui.horizontal(|ui| {
                ui.label("Max Speed:");
                let color = if rep.max_speed_units_per_sec > 450.0 {
                    Color32::from_rgb(255, 80, 80)
                } else {
                    Color32::from_rgb(100, 220, 100)
                };
                ui.colored_label(color, format!("{:.1} u/s", rep.max_speed_units_per_sec));
            });
            ui.horizontal(|ui| {
                ui.label("Violations:");
                let color = if rep.speed_violations_count > 0 {
                    Color32::from_rgb(255, 80, 80)
                } else {
                    Color32::from_rgb(100, 220, 100)
                };
                ui.colored_label(color, format!("{}", rep.speed_violations_count));
            });

            if rep.speed_violations_count == 0 && rep.timing_inversions_count == 0 {
                ui.colored_label(Color32::from_rgb(80, 220, 120), "Status: Hardware Safe");
            }
        }
    }

    fn render_generator_controls(&mut self, ui: &mut Ui) {
        ui.heading("Video Generator");

        ui.horizontal(|ui| {
            if ui.button("Select Video...").clicked() {
                self.select_video_dialog();
            }
        });
        if let Some(ref p) = self.video_path {
            ui.label(RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).size(11.0));
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Select ONNX Model...").clicked() {
                self.select_model_dialog();
            }
        });
        if let Some(ref m) = self.model_path {
            ui.label(RichText::new(m.file_name().unwrap_or_default().to_string_lossy()).size(11.0));
        }

        ui.checkbox(&mut self.pov_mode, "POV Mode (Fixed Bottom Anchor)");
        ui.checkbox(&mut self.vr_mode, "VR 180 Stereo Mode");

        ui.add_space(6.0);
        if self.is_generating {
            ui.add(ProgressBar::new(self.generation_progress).text(&self.generation_status));
        } else {
            let can_run = self.video_path.is_some();
            if ui.add_enabled(can_run, egui::Button::new("Run Generation")).clicked() {
                self.start_generation_task();
            }
        }
    }

    fn start_generation_task(&mut self) {
        let video = match self.video_path.clone() {
            Some(v) => v,
            None => return,
        };
        let model = self.model_path.clone();
        let fps = self.target_fps;
        let pov = self.pov_mode;
        let vr = self.vr_mode;
        let conf = self.neural_conf;

        let (tx, rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = channel();
        self.worker_rx = Some(rx);
        self.is_generating = true;
        self.generation_progress = 0.0;
        self.generation_status = "Initializing generator...".to_string();

        thread::spawn(move || {
            let width = if model.is_some() { 640 } else { 256 };
            let height = if model.is_some() { 640 } else { 256 };

            let res = (|| -> anyhow::Result<Funscript> {
                let stream_cfg = crate::video::StreamConfig {
                    target_width: width,
                    target_height: height,
                    target_fps: fps,
                    vr_mode: vr,
                    is_rgb: model.is_some(),
                };

                let mut stream = crate::video::FrameStreamReader::new(&video, &stream_cfg)?;
                let mut all_samples = Vec::new();
                let mut neural_positions = Vec::new();

                let first = stream.next_frame()?.context("Zero video frames")?;
                let mut last_raw = first.0;

                let mut detector = if let Some(ref m) = model {
                    Some(crate::neural::NeuralDetector::new(m, conf)?)
                } else {
                    None
                };
                let mut tracker = if model.is_some() {
                    Some(crate::neural::tracker::AnatomicalTracker::new())
                } else {
                    None
                };

                let mut frame_count = 0;
                while let Some((curr_raw, ts)) = stream.next_frame()? {
                    frame_count += 1;
                    if frame_count % 30 == 0 {
                        let _ = tx.send(WorkerMessage::Progress {
                            percent: 0.5,
                            status: format!("Processing frame {}", frame_count),
                        });
                    }

                    let (last_gray, curr_gray) = if stream_cfg.is_rgb {
                        (
                            crate::video::rgb_to_gray(&last_raw),
                            crate::video::rgb_to_gray(&curr_raw),
                        )
                    } else {
                        (last_raw.clone(), curr_raw.clone())
                    };

                    let is_cut_diff = crate::tracking::detect_cut_photometric(&last_gray, &curr_gray, 30.0);
                    let flow = crate::tracking::compute_dense_flow(&last_gray, &curr_gray, width as usize, height as usize);
                    let is_cut = is_cut_diff || (flow.mean_magnitude() > 8.0);

                    let center = if pov {
                        ((width / 2) as f32, (height - 1) as f32)
                    } else {
                        let (cx, cy, _) = crate::tracking::find_divergence_center(&flow);
                        (cx, cy)
                    };

                    let radial_dot = crate::tracking::project_radial_motion(&flow, center, is_cut, pov, true);
                    all_samples.push((radial_dot, is_cut, ts));

                    if let (Some(ref mut det), Some(ref mut trk)) = (&mut detector, &mut tracker) {
                        let detections = det.detect(&curr_raw, width as usize, height as usize)?;
                        let pos = if let Some((p, _, _)) = trk.update_from_detections(&detections) {
                            p
                        } else {
                            trk.update_hybrid_fallback(radial_dot)
                        };
                        neural_positions.push((pos, ts));
                    }

                    last_raw = curr_raw;
                }

                let actions = if model.is_some() && !neural_positions.is_empty() {
                    let norm_values: Vec<f32> = neural_positions.iter().map(|(p, _)| *p).collect();
                    let timestamps: Vec<i64> = neural_positions.iter().map(|(_, ts)| *ts).collect();
                    crate::signal::extract_actions(&norm_values, &timestamps, true)
                } else {
                    let (cum, ts) = crate::signal::integrate_flow(&all_samples);
                    let norm = crate::signal::detrend_and_normalize(&cum, fps, 2.0, 3.0, 1000.0);
                    crate::signal::extract_actions(&norm, &ts, true)
                };

                let mut s = Funscript::new(actions);
                s.sanitize();
                Ok(s)
            })();

            let _ = tx.send(WorkerMessage::Done(res.map_err(|e| e.to_string())));
        });
    }

    fn open_funscript_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Funscript", &["funscript", "json"])
            .pick_file()
        {
            if let Ok(loaded) = Funscript::load(&path) {
                self.script = loaded;
                self.script_path = Some(path);
                self.run_doctor();
                self.fit_timeline();
            }
        }
    }

    fn save_funscript(&mut self) {
        if let Some(ref path) = self.script_path {
            let _ = self.script.save(path);
        } else {
            self.save_funscript_as();
        }
    }

    fn save_funscript_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Funscript", &["funscript"])
            .save_file()
        {
            if self.script.save(&path).is_ok() {
                self.script_path = Some(path);
            }
        }
    }

    fn select_video_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Video Files", &["mp4", "mkv", "mov", "avi", "webm"])
            .pick_file()
        {
            self.video_path = Some(path);
        }
    }

    fn select_model_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("ONNX Models", &["onnx"])
            .pick_file()
        {
            self.model_path = Some(path);
        }
    }

    fn fit_timeline(&mut self) {
        if let Some(last) = self.script.actions.last() {
            self.timeline_state.view_start_ms = 0.0;
            self.timeline_state.view_duration_ms = (last.at as f64).max(5000.0);
        }
    }

    fn run_doctor(&mut self) {
        self.doctor_report = Some(diagnose_funscript(&self.script, &DoctorConfig::default()));
    }
}
