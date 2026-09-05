//! Main Funscript Hub desktop application window.

use anyhow::Context;
use crate::audio::AudioWaveform;
use crate::funscript::{
    diagnose_funscript, AxisChannel, DoctorConfig, DoctorReport, Funscript, InfillPattern,
    MultiAxisScript,
};
use crate::gui::timeline::{show_timeline, TimelineState};
use eframe::egui::{
    self, Align, Color32, Context as EguiContext, Layout, ProgressBar, RichText, ScrollArea,
    Slider, TopBottomPanel, Ui,
};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubTab {
    Studio,       // F1: Studio Editor (Video Canvas + Keyframe Inspector + Timeline + Waveform)
    Cinema,       // F2: Cinema Player (Full-window Video Canvas + Floating HUD)
    Generator,    // F3: Generator Studio
    ScriptDoctor, // F4: Script Doctor & Repair
    DeviceSync,   // F5: Device & Hardware Sync
}

pub enum WorkerMessage {
    Progress { percent: f32, status: String },
    Done(Result<MultiAxisScript, String>),
}

pub struct FunGenApp {
    /// Active interface tab
    pub active_tab: HubTab,

    /// Currently loaded funscript
    pub script: Funscript,
    /// Multi-axis script collection (6-DOF + Suction)
    pub multi_axis: MultiAxisScript,
    /// Currently selected active editing channel
    pub active_axis: AxisChannel,
    /// Whether to show faint ghost curves of inactive channels on timeline
    pub show_ghost_axes: bool,

    /// Infill generator modal and parameters
    pub infill_dialog_open: bool,
    pub infill_pattern: InfillPattern,
    pub infill_freq_hz: f32,
    pub infill_min_pos: i32,
    pub infill_max_pos: i32,
    pub infill_start_ms: i64,
    pub infill_end_ms: i64,

    /// Path to the loaded funscript file
    pub script_path: Option<PathBuf>,
    /// Interactive timeline state (zoom, pan, selection, playhead)
    pub timeline_state: TimelineState,
    /// Latest Script Doctor quality audit report
    pub doctor_report: Option<DoctorReport>,
    /// User notification or repair action message
    pub status_message: Option<(String, Instant)>,

    // Media & Waveform
    pub audio_waveform: Option<AudioWaveform>,
    waveform_rx: Option<Receiver<AudioWaveform>>,
    pub video_texture: Option<egui::TextureHandle>,
    pub last_video_frame_ts: i64,

    // Playback simulation
    pub is_playing: bool,
    pub playback_speed: f64,
    pub last_frame_time: Option<Instant>,

    // Generator settings
    pub video_path: Option<PathBuf>,
    pub video_info: Option<String>,
    pub model_path: Option<PathBuf>,
    pub target_fps: f64,
    pub neural_conf: f32,
    pub pov_mode: bool,
    pub vr_mode: bool,
    pub detrend_window: f64,
    pub norm_window: f64,
    pub use_neural: bool,
    pub generate_multi_axis: bool,

    // Background generation task state
    pub is_generating: bool,
    pub generation_progress: f32,
    pub generation_status: String,
    worker_rx: Option<Receiver<WorkerMessage>>,

    // Device & Sync state
    pub sync_udp_address: String,
    pub sync_udp_enabled: bool,
}

impl Default for FunGenApp {
    fn default() -> Self {
        let script = Funscript::new(vec![]);
        let doctor_report = Some(diagnose_funscript(&script, &DoctorConfig::default()));

        Self {
            active_tab: HubTab::Studio,
            script,
            multi_axis: MultiAxisScript::new(),
            active_axis: AxisChannel::Stroke,
            show_ghost_axes: true,

            infill_dialog_open: false,
            infill_pattern: InfillPattern::Sine,
            infill_freq_hz: 1.5,
            infill_min_pos: 10,
            infill_max_pos: 90,
            infill_start_ms: 0,
            infill_end_ms: 5000,

            script_path: None,
            timeline_state: TimelineState::default(),
            doctor_report,
            status_message: None,

            audio_waveform: None,
            waveform_rx: None,
            video_texture: None,
            last_video_frame_ts: -999,

            is_playing: false,
            playback_speed: 1.0,
            last_frame_time: None,

            video_path: None,
            video_info: None,
            model_path: Some(PathBuf::from(
                "/home/brianklam/Desktop/Funscripts/FunGen/fungen-data/cache/models/FunGen-12n-pov-1.1.0.onnx",
            )),
            target_fps: 30.0,
            neural_conf: 0.35,
            pov_mode: false,
            vr_mode: false,
            detrend_window: 2.0,
            norm_window: 3.0,
            use_neural: true,
            generate_multi_axis: true,

            is_generating: false,
            generation_progress: 0.0,
            generation_status: "Idle".to_string(),
            worker_rx: None,

            sync_udp_address: "127.0.0.1:8888".to_string(),
            sync_udp_enabled: false,
        }
    }
}

impl eframe::App for FunGenApp {
    fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        // 1. Poll background messages (generation and audio waveform)
        self.poll_worker();
        self.poll_waveform();

        // 2. Advance playback clock if playing
        self.update_playback(ctx);

        // 3. Handle Keyboard Shortcuts
        self.handle_shortcuts(ctx);

        // 4. Update video texture frame if cursor timestamp shifted
        self.update_video_texture(ctx);

        // 5. Top Menu & Tab Navigation Bar
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
                    if ui.button("Studio Editor (F1)").clicked() {
                        self.active_tab = HubTab::Studio;
                        ui.close_menu();
                    }
                    if ui.button("Cinema Player (F2)").clicked() {
                        self.active_tab = HubTab::Cinema;
                        ui.close_menu();
                    }
                    ui.separator();
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
                    if ui.button("Run Script Doctor (F4)").clicked() {
                        self.run_doctor();
                        self.active_tab = HubTab::ScriptDoctor;
                        ui.close_menu();
                    }
                    if ui.button("Quick Fix Speed Violations").clicked() {
                        let fixed = self.script.fix_speed_violations(450.0);
                        self.run_doctor();
                        self.set_status(format!("Clamped {} speed violations to safe limits", fixed));
                        ui.close_menu();
                    }
                    if ui.button("Remove Micro-Jitters (<20ms)").clicked() {
                        let removed = self.script.remove_micro_jitters(20, 2);
                        self.run_doctor();
                        self.set_status(format!("Removed {} micro-jitter keyframes", removed));
                        ui.close_menu();
                    }
                });

                ui.separator();

                // Navigation Tabs with Function Key Hints
                ui.selectable_value(&mut self.active_tab, HubTab::Studio, "  Studio Editor (F1)  ");
                ui.selectable_value(&mut self.active_tab, HubTab::Cinema, "  Cinema Player (F2)  ");
                ui.selectable_value(&mut self.active_tab, HubTab::Generator, "  Generator (F3)  ");
                ui.selectable_value(&mut self.active_tab, HubTab::ScriptDoctor, "  Doctor & Repair (F4)  ");
                ui.selectable_value(&mut self.active_tab, HubTab::DeviceSync, "  Device & Sync (F5)  ");

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.is_generating {
                        ui.spinner();
                        ui.label(RichText::new(&self.generation_status).color(Color32::from_rgb(0, 200, 255)));
                    } else if let Some((ref msg, instant)) = self.status_message {
                        if instant.elapsed().as_secs() < 6 {
                            ui.label(RichText::new(msg).color(Color32::from_rgb(100, 220, 150)));
                        }
                    } else if let Some(ref path) = self.script_path {
                        ui.label(
                            RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
                                .color(Color32::from_rgb(180, 190, 205)),
                        );
                    }
                });
            });
        });

        // 6. Central Panel Rendered based on Active Tab
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                HubTab::Studio => self.render_studio_view(ui),
                HubTab::Cinema => self.render_cinema_view(ui),
                HubTab::Generator => self.render_generator_view(ui),
                HubTab::ScriptDoctor => self.render_doctor_view(ui),
                HubTab::DeviceSync => self.render_device_sync_view(ui),
            }
        });

        // Request repaint if playing or generating
        if self.is_playing || self.is_generating {
            ctx.request_repaint();
        }
    }
}

impl FunGenApp {
    fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg, Instant::now()));
    }

    pub fn switch_axis(&mut self, new_axis: AxisChannel) {
        if self.active_axis == new_axis {
            return;
        }
        // Commit current active script into multi_axis collection
        self.multi_axis.channels.insert(self.active_axis, self.script.clone());
        self.active_axis = new_axis;
        // Load target channel script (or blank if new)
        self.script = self.multi_axis.get_or_create(new_axis).clone();
        self.timeline_state.selected_index = None;
        self.run_doctor();
        self.set_status(format!("Switched active channel to {}", new_axis.display_name()));
    }

    fn handle_shortcuts(&mut self, ctx: &EguiContext) {
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.active_tab = HubTab::Studio;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            self.active_tab = HubTab::Cinema;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F3)) {
            self.active_tab = HubTab::Generator;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F4)) {
            self.active_tab = HubTab::ScriptDoctor;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.active_tab = HubTab::DeviceSync;
        }

        // Playback, frame stepping, and axis channel hotkeys (when not typing in an input field)
        if !ctx.wants_keyboard_input() {
            // Hotkeys 1-7: Switch active multi-axis channel
            if ctx.input(|i| i.key_pressed(egui::Key::Num1)) { self.switch_axis(AxisChannel::Stroke); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num2)) { self.switch_axis(AxisChannel::Surge); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num3)) { self.switch_axis(AxisChannel::Sway); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num4)) { self.switch_axis(AxisChannel::Pitch); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num5)) { self.switch_axis(AxisChannel::Roll); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num6)) { self.switch_axis(AxisChannel::Twist); }
            if ctx.input(|i| i.key_pressed(egui::Key::Num7)) { self.switch_axis(AxisChannel::Suction); }

            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.is_playing = !self.is_playing;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                let step = if ctx.input(|i| i.modifiers.shift) { 1000 } else { 33 };
                self.timeline_state.cursor_time_ms = (self.timeline_state.cursor_time_ms - step).max(0);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                let step = if ctx.input(|i| i.modifiers.shift) { 1000 } else { 33 };
                let dur = self.script.actions.last().map(|a| a.at).unwrap_or(0);
                self.timeline_state.cursor_time_ms = (self.timeline_state.cursor_time_ms + step).min(dur.max(step));
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                self.timeline_state.cursor_time_ms = 0;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::End)) {
                let dur = self.script.actions.last().map(|a| a.at).unwrap_or(0);
                self.timeline_state.cursor_time_ms = dur;
            }
        }
    }

    fn update_video_texture(&mut self, ctx: &EguiContext) {
        let video_path = match self.video_path.as_ref() {
            Some(p) => p,
            None => return,
        };

        let cur_time = self.timeline_state.cursor_time_ms;
        // Don't re-extract if timestamp hasn't changed significantly (within 20ms)
        if (cur_time - self.last_video_frame_ts).abs() < 20 && self.video_texture.is_some() {
            return;
        }

        let w = 480;
        let h = 270;
        if let Ok(raw_rgb) = crate::video::extract_frame_at(video_path, cur_time, w, h) {
            let color_image = egui::ColorImage::from_rgb([w as usize, h as usize], &raw_rgb);
            self.video_texture = Some(ctx.load_texture(
                "current_video_frame",
                color_image,
                egui::TextureOptions::LINEAR,
            ));
            self.last_video_frame_ts = cur_time;
        }
    }

    fn poll_waveform(&mut self) {
        if let Some(ref rx) = self.waveform_rx {
            if let Ok(wf) = rx.try_recv() {
                self.audio_waveform = Some(wf);
                self.waveform_rx = None;
                self.set_status("Audio waveform extracted and loaded".to_string());
            }
        }
    }

    fn update_playback(&mut self, ctx: &EguiContext) {
        if self.is_playing {
            let now = Instant::now();
            if let Some(last) = self.last_frame_time {
                let dt_secs = now.duration_since(last).as_secs_f64();
                let advance_ms = (dt_secs * 1000.0 * self.playback_speed).round() as i64;
                self.timeline_state.cursor_time_ms += advance_ms;

                let duration_ms = self.script.actions.last().map(|a| a.at).unwrap_or(0);
                if duration_ms > 0 && self.timeline_state.cursor_time_ms > duration_ms {
                    self.timeline_state.cursor_time_ms = 0; // Loop playback
                }

                // Auto-scroll timeline view if cursor passes the visible window edge
                let view_end = self.timeline_state.view_start_ms + self.timeline_state.view_duration_ms;
                if (self.timeline_state.cursor_time_ms as f64) > view_end - self.timeline_state.view_duration_ms * 0.15 {
                    self.timeline_state.view_start_ms = (self.timeline_state.cursor_time_ms as f64)
                        - self.timeline_state.view_duration_ms * 0.15;
                }
            }
            self.last_frame_time = Some(now);
            ctx.request_repaint();
        } else {
            self.last_frame_time = None;
        }
    }

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
                        Ok(bundle) => {
                            if let Some(stroke) = bundle.channels.get(&AxisChannel::Stroke) {
                                self.script = stroke.clone();
                            }
                            self.multi_axis = bundle;
                            self.active_axis = AxisChannel::Stroke;
                            self.run_doctor();
                            self.fit_timeline();
                            let count = self.multi_axis.channels.len();
                            self.generation_status = format!("Generation Complete! ({} axes generated)", count);
                            self.set_status(format!("Generated {} axes! Loaded into Studio Editor.", count));
                            self.active_tab = HubTab::Studio;
                        }
                        Err(e) => {
                            self.generation_status = format!("Error: {e}");
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // View 1: Studio Editor (F1)
    // =========================================================================
    fn render_studio_view(&mut self, ui: &mut Ui) {
        let duration_ms = self.script.actions.last().map(|a| a.at).unwrap_or(0);

        // Top Section: Embedded Synchronized Video + Inspector Sidebar
        let top_height = 240.0;
        ui.allocate_ui(egui::vec2(ui.available_width(), top_height), |ui| {
            ui.horizontal(|ui| {
                // Embedded Video Canvas
                let video_w = 426.0;
                let video_h = 240.0;
                ui.group(|ui| {
                    ui.set_width(video_w);
                    ui.set_height(video_h);

                    if let Some(ref tex) = self.video_texture {
                        ui.image((tex.id(), egui::vec2(video_w - 8.0, video_h - 26.0)));
                    } else if let Some(ref p) = self.video_path {
                        ui.vertical_centered(|ui| {
                            ui.add_space(80.0);
                            ui.label(RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).strong());
                            ui.label(RichText::new("Loading video frames...").size(11.0).color(Color32::GRAY));
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.add_space(80.0);
                            ui.label(RichText::new("No Video Loaded").color(Color32::GRAY));
                            if ui.button("Select Video...").clicked() {
                                self.select_video_dialog();
                            }
                        });
                    }

                    // Video caption overlay
                    ui.horizontal(|ui| {
                        let cur_time_str = format_ms(self.timeline_state.cursor_time_ms);
                        ui.monospace(RichText::new(format!("Frame Time: {cur_time_str}")).size(10.0));
                    });
                });

                ui.separator();

                // Keyframe Inspector & Quick Health
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 8.0);
                    self.render_selected_action_editor(ui);
                    ui.separator();
                    self.render_quick_doctor_summary(ui);
                });
            });
        });

        ui.add_space(4.0);
        ui.separator();

        // Bottom Section: Playback Controller + Timeline Canvas & Audio Waveform
        ui.horizontal(|ui| {
            let play_label = if self.is_playing { "⏸ Pause (Space)" } else { "▶ Play (Space)" };
            if ui.button(play_label).clicked() {
                self.is_playing = !self.is_playing;
            }

            if ui.button("⏮ Start").clicked() {
                self.timeline_state.cursor_time_ms = 0;
            }
            if ui.button("◀ 1F").clicked() {
                self.timeline_state.cursor_time_ms = (self.timeline_state.cursor_time_ms - 33).max(0);
            }
            if ui.button("1F ▶").clicked() {
                self.timeline_state.cursor_time_ms = (self.timeline_state.cursor_time_ms + 33).min(duration_ms.max(33));
            }
            if ui.button("⏭ End").clicked() {
                self.timeline_state.cursor_time_ms = duration_ms;
            }

            ui.separator();

            // Playback speed selector
            ui.label("Speed:");
            ui.selectable_value(&mut self.playback_speed, 0.5, "0.5x");
            ui.selectable_value(&mut self.playback_speed, 1.0, "1.0x");
            ui.selectable_value(&mut self.playback_speed, 1.5, "1.5x");
            ui.selectable_value(&mut self.playback_speed, 2.0, "2.0x");

            ui.separator();

            // Formatted time display
            let cur_time_str = format_ms(self.timeline_state.cursor_time_ms);
            let dur_time_str = format_ms(duration_ms);
            ui.monospace(format!("{cur_time_str} / {dur_time_str}"));

            // Current interpolated position
            let cur_pos = self.script.interpolate_position(self.timeline_state.cursor_time_ms);
            ui.colored_label(Color32::from_rgb(0, 200, 255), format!("Pos: {cur_pos:.0}%"));

            ui.separator();

            // Zoom presets
            if ui.button("Fit All").clicked() {
                self.fit_timeline();
            }
            if ui.button("10s").clicked() {
                self.timeline_state.view_duration_ms = 10_000.0;
            }
            if ui.button("30s").clicked() {
                self.timeline_state.view_duration_ms = 30_000.0;
            }
        });

        // Time scrubber slider
        if duration_ms > 0 {
            ui.add(
                Slider::new(&mut self.timeline_state.cursor_time_ms, 0..=duration_ms)
                    .show_value(false)
                    .trailing_fill(true),
            );
        }

        // Multi-Axis Channel Ribbon & Snapping Controls
        ui.horizontal(|ui| {
            ui.label(RichText::new("Axis (1-7):").size(11.0).strong());
            for axis in AxisChannel::ALL {
                let is_active = axis == self.active_axis;
                let count = if is_active {
                    self.script.actions.len()
                } else {
                    self.multi_axis.get(axis).map(|s| s.actions.len()).unwrap_or(0)
                };
                let (r, g, b) = axis.color_rgb();
                let color = Color32::from_rgb(r, g, b);

                let label_text = format!("{} ({})", axis.tcode_axis(), count);
                let btn = if is_active {
                    egui::Button::new(RichText::new(format!("● {label_text}")).color(color).strong())
                } else {
                    egui::Button::new(RichText::new(format!("○ {label_text}")).color(Color32::from_rgb(140, 150, 165)))
                };

                if ui.add(btn).clicked() {
                    self.switch_axis(axis);
                }
            }

            ui.separator();
            ui.checkbox(&mut self.show_ghost_axes, "Ghost Curves");
            ui.checkbox(&mut self.timeline_state.magnetic_snapping, "🧲 Audio Snap (±35ms)");

            ui.separator();
            if ui.button("🌊 Infill Generator...").clicked() {
                self.infill_start_ms = self.timeline_state.view_start_ms as i64;
                self.infill_end_ms = (self.timeline_state.view_start_ms + self.timeline_state.view_duration_ms) as i64;
                self.infill_dialog_open = true;
            }
        });

        // Procedural Waveform Infill Generator Modal
        if self.infill_dialog_open {
            let mut open = self.infill_dialog_open;
            let mut do_generate = false;

            egui::Window::new("Procedural Waveform Infill Generator")
                .open(&mut open)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.heading(format!("Target Axis: {}", self.active_axis.display_name()));
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("Pattern Shape:");
                        ui.selectable_value(&mut self.infill_pattern, InfillPattern::Sine, "Sine");
                        ui.selectable_value(&mut self.infill_pattern, InfillPattern::Triangle, "Triangle");
                        ui.selectable_value(&mut self.infill_pattern, InfillPattern::Sawtooth, "Sawtooth");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Frequency (Hz):");
                        ui.add(Slider::new(&mut self.infill_freq_hz, 0.2..=10.0).suffix(" Hz"));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Min Pos:");
                        ui.add(Slider::new(&mut self.infill_min_pos, 0..=100));
                        ui.label("Max Pos:");
                        ui.add(Slider::new(&mut self.infill_max_pos, 0..=100));
                    });

                    ui.horizontal(|ui| {
                        ui.label("Start (ms):");
                        ui.add(egui::DragValue::new(&mut self.infill_start_ms).speed(100.0));
                        ui.label("End (ms):");
                        ui.add(egui::DragValue::new(&mut self.infill_end_ms).speed(100.0));
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Viewport Window").clicked() {
                            self.infill_start_ms = self.timeline_state.view_start_ms as i64;
                            self.infill_end_ms = (self.timeline_state.view_start_ms + self.timeline_state.view_duration_ms) as i64;
                        }
                        if ui.button("Cursor + 5s").clicked() {
                            self.infill_start_ms = self.timeline_state.cursor_time_ms;
                            self.infill_end_ms = self.timeline_state.cursor_time_ms + 5000;
                        }
                    });

                    ui.separator();
                    if ui.button(RichText::new("⚡ Generate Infill Waveform").strong()).clicked() {
                        do_generate = true;
                    }
                });

            self.infill_dialog_open = open;

            if do_generate {
                self.script.infill_pattern(
                    self.infill_start_ms,
                    self.infill_end_ms,
                    self.infill_min_pos,
                    self.infill_max_pos,
                    self.infill_freq_hz,
                    self.infill_pattern,
                );
                self.run_doctor();
                self.set_status(format!(
                    "Generated {:?} infill on {} ({}ms - {}ms)",
                    self.infill_pattern,
                    self.active_axis.display_name(),
                    self.infill_start_ms,
                    self.infill_end_ms
                ));
                self.infill_dialog_open = false;
            }
        }

        ui.add_space(2.0);

        // Timeline Canvas with Audio Waveform and Ghost Axis Overlays
        ui.label(
            RichText::new("Timeline Curve & Audio Waveform | Left Drag: Keyframe | Double-Click: Add | Scroll: Zoom | Middle/Ctrl+Drag: Pan | Arrows: Step Frame")
                .size(11.0)
                .color(Color32::from_rgb(130, 140, 155)),
        );

        // Synchronize active script to multi_axis map before rendering ghost curves
        self.multi_axis.channels.insert(self.active_axis, self.script.clone());

        let ghost_data: Vec<(&str, &Funscript, Color32)> = if self.show_ghost_axes {
            AxisChannel::ALL
                .iter()
                .filter(|&&a| a != self.active_axis)
                .filter_map(|&a| {
                    self.multi_axis.get(a).and_then(|s| {
                        if !s.actions.is_empty() {
                            let (r, g, b) = a.color_rgb();
                            Some((a.display_name(), s, Color32::from_rgb(r, g, b)))
                        } else {
                            None
                        }
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let (ar, ag, ab) = self.active_axis.color_rgb();
        let active_color = Color32::from_rgb(ar, ag, ab);

        show_timeline(
            ui,
            &mut self.script,
            &mut self.timeline_state,
            self.doctor_report.as_ref(),
            self.audio_waveform.as_ref(),
            if ghost_data.is_empty() { None } else { Some(&ghost_data) },
            active_color,
        );
    }

    // =========================================================================
    // View 2: Cinema Player (F2)
    // =========================================================================
    fn render_cinema_view(&mut self, ui: &mut Ui) {
        let avail_rect = ui.available_rect_before_wrap();
        let cur_pos = self.script.interpolate_position(self.timeline_state.cursor_time_ms);

        // 1. Full-Window Video Canvas
        if let Some(ref tex) = self.video_texture {
            let img_h = (avail_rect.height() - 70.0).max(200.0);
            let img_w = (img_h * (16.0 / 9.0)).min(avail_rect.width());
            let center_x = avail_rect.center().x;
            let video_rect = egui::Rect::from_center_size(
                egui::pos2(center_x, avail_rect.top() + img_h / 2.0),
                egui::vec2(img_w, img_h),
            );
            ui.painter().image(
                tex.id(),
                video_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(avail_rect.height() * 0.35);
                ui.heading("Cinema Player Mode");
                ui.label("No video loaded for playback.");
                if ui.button("Select Video...").clicked() {
                    self.select_video_dialog();
                }
            });
        }

        // 2. Floating Bottom HUD (Auto-Hiding Controls & Live Stroke Position)
        let hud_rect = egui::Rect::from_min_max(
            egui::pos2(avail_rect.left(), avail_rect.bottom() - 65.0),
            avail_rect.max,
        );
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
            ui.painter().rect_filled(ui.available_rect_before_wrap(), 4.0, Color32::from_rgba_unmultiplied(18, 20, 26, 230));
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    let play_label = if self.is_playing { "⏸ Pause" } else { "▶ Play" };
                    if ui.button(play_label).clicked() {
                        self.is_playing = !self.is_playing;
                    }
                    if ui.button("⏮").clicked() {
                        self.timeline_state.cursor_time_ms = 0;
                    }

                    let cur_time_str = format_ms(self.timeline_state.cursor_time_ms);
                    let dur_ms = self.script.actions.last().map(|a| a.at).unwrap_or(0);
                    let dur_str = format_ms(dur_ms);
                    ui.monospace(format!("{cur_time_str} / {dur_str}"));

                    // Live Haptic Gauge Bar
                    ui.separator();
                    ui.label(RichText::new("Haptic Stroke:").size(11.0));
                    let (gauge_rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(gauge_rect, 2.0, Color32::from_rgb(30, 35, 45));
                    let fill_w = (cur_pos / 100.0) * gauge_rect.width();
                    let fill_rect = egui::Rect::from_min_max(gauge_rect.min, egui::pos2(gauge_rect.left() + fill_w, gauge_rect.bottom()));
                    ui.painter().rect_filled(fill_rect, 2.0, Color32::from_rgb(0, 200, 255));
                    ui.strong(format!("{:.0}%", cur_pos));

                    // Telemetry & Studio button
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Exit to Studio (F1)").clicked() {
                            self.active_tab = HubTab::Studio;
                        }
                        ui.monospace(format!("T-Code: L0{:03}", (cur_pos * 10.0).round() as i32));
                    });
                });

                // Scrubber slider
                let duration_ms = self.script.actions.last().map(|a| a.at).unwrap_or(0);
                if duration_ms > 0 {
                    ui.add(
                        Slider::new(&mut self.timeline_state.cursor_time_ms, 0..=duration_ms)
                            .show_value(false)
                            .trailing_fill(true),
                    );
                }
            },
        );
    }

    fn render_selected_action_editor(&mut self, ui: &mut Ui) {
        ui.heading("Keyframe & Channel Inspector");
        if let Some(idx) = self.timeline_state.selected_index {
            if idx < self.script.actions.len() {
                let action_at = self.script.actions[idx].at;
                let mut jump_clicked = false;
                let mut delete_clicked = false;

                ui.horizontal(|ui| {
                    ui.label("Time (ms):");
                    ui.add(egui::DragValue::new(&mut self.script.actions[idx].at).speed(10.0));
                });

                ui.horizontal(|ui| {
                    ui.label("Position:");
                    ui.add(egui::Slider::new(&mut self.script.actions[idx].pos, 0..=100));
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Jump Playhead").clicked() {
                        jump_clicked = true;
                    }
                    if ui.button("🗑 Delete Point").clicked() {
                        delete_clicked = true;
                    }
                });

                if jump_clicked {
                    self.timeline_state.cursor_time_ms = action_at;
                }
                if delete_clicked {
                    self.script.actions.remove(idx);
                    self.timeline_state.selected_index = None;
                    self.run_doctor();
                }
            }
        } else {
            ui.label(RichText::new("Click a keyframe node on the timeline to edit.").color(Color32::GRAY));
        }

        ui.add_space(4.0);
        ui.separator();
        ui.label(RichText::new("Channel Transformations:").size(11.0).strong());
        ui.horizontal(|ui| {
            if ui.button("Invert (100-P)").clicked() {
                self.script.invert_positions();
                self.run_doctor();
                self.set_status("Inverted channel positions".to_string());
            }
            if ui.button("Scale 1.1x").clicked() {
                self.script.scale_amplitude(1.1, 50);
                self.run_doctor();
                self.set_status("Scaled amplitude up 10%".to_string());
            }
            if ui.button("Scale 0.9x").clicked() {
                self.script.scale_amplitude(0.9, 50);
                self.run_doctor();
                self.set_status("Scaled amplitude down 10%".to_string());
            }
        });
    }

    fn render_quick_doctor_summary(&mut self, ui: &mut Ui) {
        ui.heading("Doctor Health");
        if let Some(ref rep) = self.doctor_report {
            let (status_color, status_text) = if rep.speed_violations_count == 0 && rep.timing_inversions_count == 0 {
                (Color32::from_rgb(80, 220, 120), "PERFECT")
            } else if rep.speed_violations_count > 0 {
                (Color32::from_rgb(255, 80, 80), "SPEED VIOLATIONS")
            } else {
                (Color32::from_rgb(255, 200, 60), "WARNINGS DETECTED")
            };

            ui.colored_label(status_color, format!("Status: {status_text}"));
            ui.label(format!("Actions: {}", rep.total_actions));
            ui.label(format!("Max Speed: {:.0} u/s", rep.max_speed_units_per_sec));
            ui.label(format!("Violations: {}", rep.speed_violations_count));

            ui.add_space(4.0);
            if ui.button("Open Full Doctor & Repair (F4)").clicked() {
                self.active_tab = HubTab::ScriptDoctor;
            }
        }
    }

    // =========================================================================
    // View 3: Generator Studio (F3)
    // =========================================================================
    fn render_generator_view(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Funscript Generator Studio");
            ui.label(
                RichText::new("Generate hardware-ready funscripts from video using AI Neural Anatomical Tracking or Optical Flow.")
                    .color(Color32::from_rgb(160, 170, 185)),
            );
            ui.add_space(8.0);

            // Mode Selector
            ui.group(|ui| {
                ui.heading("1. Tracking Engine");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.use_neural, true, "AI Neural YOLOv12 Tracking (Recommended)");
                    ui.radio_value(&mut self.use_neural, false, "Dense Optical Flow (Classical)");
                });
                if self.use_neural {
                    ui.label(
                        RichText::new("Uses Ultralytics YOLOv12 via ONNX Runtime to detect 10 anatomical classes and calculate penetration depth with hybrid flow fallback.")
                            .size(11.0)
                            .color(Color32::GRAY),
                    );
                } else {
                    ui.label(
                        RichText::new("Uses Rayon multi-threaded Lucas-Kanade dense optical flow and 2D velocity divergence to track motion centers.")
                            .size(11.0)
                            .color(Color32::GRAY),
                    );
                }
            });

            ui.add_space(8.0);

            // Input Video Section
            ui.group(|ui| {
                ui.heading("2. Input Video");
                ui.horizontal(|ui| {
                    if ui.button("Browse Video...").clicked() {
                        self.select_video_dialog();
                    }
                    if let Some(ref p) = self.video_path {
                        ui.strong(p.file_name().unwrap_or_default().to_string_lossy());
                    } else {
                        ui.label(RichText::new("No video selected").color(Color32::GRAY));
                    }
                });

                if let Some(ref info) = self.video_info {
                    ui.monospace(info);
                }
            });

            ui.add_space(8.0);

            // Model / Neural Settings (if neural enabled)
            if self.use_neural {
                ui.group(|ui| {
                    ui.heading("3. Neural Model Configuration");
                    ui.horizontal(|ui| {
                        if ui.button("Select ONNX Model...").clicked() {
                            self.select_model_dialog();
                        }
                        if let Some(ref m) = self.model_path {
                            ui.strong(m.file_name().unwrap_or_default().to_string_lossy());
                        } else {
                            ui.label(RichText::new("No model selected").color(Color32::RED));
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Detection Confidence Threshold:");
                        ui.add(Slider::new(&mut self.neural_conf, 0.1..=0.9).step_by(0.05));
                    });
                });
                ui.add_space(8.0);
            }

            // Signal & Processing Settings
            ui.group(|ui| {
                ui.heading(if self.use_neural { "4. Processing Settings" } else { "3. Processing Settings" });
                ui.horizontal(|ui| {
                    ui.label("Target Frame Rate (FPS):");
                    ui.add(Slider::new(&mut self.target_fps, 15.0..=60.0).step_by(1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Detrend Window (seconds):");
                    ui.add(Slider::new(&mut self.detrend_window, 1.0..=5.0).step_by(0.5));
                });
                ui.horizontal(|ui| {
                    ui.label("Normalization Window (seconds):");
                    ui.add(Slider::new(&mut self.norm_window, 1.0..=6.0).step_by(0.5));
                });

                ui.add_space(4.0);
                ui.checkbox(&mut self.pov_mode, "POV Mode (Fixed bottom-center interaction anchor)");
                ui.checkbox(&mut self.vr_mode, "VR 180 Stereo Mode (Crop bottom-left quadrant)");
                ui.checkbox(&mut self.generate_multi_axis, "Generate 6-DOF Multi-Axis Bundle (L0, L1, L2, R1, R0, R2, V0)");
            });

            ui.add_space(12.0);

            // Run Button & Progress Section
            if self.is_generating {
                ui.add(ProgressBar::new(self.generation_progress).text(&self.generation_status));
            } else {
                let can_run = self.video_path.is_some() && (!self.use_neural || self.model_path.is_some());
                if ui.add_enabled(can_run, egui::Button::new(RichText::new("Start Generation").size(16.0))).clicked() {
                    self.start_generation_task();
                }
            }
        });
    }

    // =========================================================================
    // View 4: Script Doctor & Repair (F4)
    // =========================================================================
    fn render_doctor_view(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Script Doctor Quality & Repair Center");
            ui.label("Audit script safety against motor limits, detect timing inversions, and apply one-click batch repairs.");
            ui.add_space(8.0);

            if let Some(rep) = self.doctor_report.clone() {
                let is_perfect = rep.speed_violations_count == 0 && rep.timing_inversions_count == 0;
                let (badge_color, badge_text) = if is_perfect {
                    (Color32::from_rgb(60, 200, 100), "PERFECT - Hardware Certified Safe")
                } else if rep.speed_violations_count > 0 {
                    (Color32::from_rgb(255, 70, 70), "WARNING - Motor Speed Violations Detected")
                } else {
                    (Color32::from_rgb(255, 180, 50), "ATTENTION - Timing Inversions or Anomalies")
                };

                ui.group(|ui| {
                    ui.colored_label(badge_color, RichText::new(badge_text).size(15.0).strong());
                    ui.add_space(6.0);

                    ui.columns(4, |cols| {
                        cols[0].label("Total Actions:");
                        cols[0].strong(format!("{}", rep.total_actions));

                        cols[1].label("Duration:");
                        cols[1].strong(format!("{:.1}s", rep.duration_ms as f64 / 1000.0));

                        cols[2].label("Average Speed:");
                        cols[2].strong(format!("{:.1} units/s", rep.avg_speed_units_per_sec));

                        cols[3].label("Maximum Speed:");
                        let speed_color = if rep.max_speed_units_per_sec > 450.0 {
                            Color32::from_rgb(255, 80, 80)
                        } else {
                            Color32::from_rgb(100, 220, 100)
                        };
                        cols[3].colored_label(speed_color, format!("{:.1} units/s", rep.max_speed_units_per_sec));
                    });

                    ui.add_space(4.0);
                    ui.columns(3, |cols| {
                        cols[0].label("Speed Violations (>450 u/s):");
                        cols[0].colored_label(
                            if rep.speed_violations_count > 0 { Color32::from_rgb(255, 80, 80) } else { Color32::GREEN },
                            format!("{}", rep.speed_violations_count),
                        );

                        cols[1].label("Timing Inversions:");
                        cols[1].colored_label(
                            if rep.timing_inversions_count > 0 { Color32::from_rgb(255, 80, 80) } else { Color32::GREEN },
                            format!("{}", rep.timing_inversions_count),
                        );

                        cols[2].label("Micro-Jitters (<20ms):");
                        cols[2].label(format!("{}", rep.micro_jitter_count));
                    });
                });

                ui.add_space(10.0);

                // Quick Repair Tools
                ui.group(|ui| {
                    ui.heading("Automated Repair Tools");
                    ui.horizontal(|ui| {
                        if ui.button("⚡ Clamp Speed Violations (450 u/s)").clicked() {
                            let fixed = self.script.fix_speed_violations(450.0);
                            self.run_doctor();
                            self.set_status(format!("Clamped {} speed violations to safe limits", fixed));
                        }

                        if ui.button("🧹 Remove Micro-Jitters").clicked() {
                            let removed = self.script.remove_micro_jitters(20, 2);
                            self.run_doctor();
                            self.set_status(format!("Removed {} micro-jitter keyframes", removed));
                        }

                        if ui.button("✨ Sort & Deduplicate").clicked() {
                            let initial = self.script.actions.len();
                            self.script.sanitize();
                            let removed = initial - self.script.actions.len();
                            self.run_doctor();
                            self.set_status(format!("Deduplicated {} redundant actions", removed));
                        }
                    });
                });

                ui.add_space(10.0);

                // Detailed Issue Log Table
                ui.heading("Detected Issues List");
                if rep.issues.is_empty() {
                    ui.label(RichText::new("Zero issues detected. This funscript is optimal.").color(Color32::GREEN));
                } else {
                    for (i, issue) in rep.issues.iter().take(25).enumerate() {
                        ui.horizontal(|ui| {
                            let color = match issue.severity {
                                crate::funscript::IssueSeverity::Error => Color32::from_rgb(255, 80, 80),
                                crate::funscript::IssueSeverity::Warning => Color32::from_rgb(255, 180, 50),
                                crate::funscript::IssueSeverity::Info => Color32::from_rgb(120, 180, 255),
                            };
                            ui.colored_label(color, format!("[{:?}]", issue.severity));
                            ui.monospace(format!("{:>7} ms:", issue.timestamp_ms));
                            ui.label(&issue.description);
                            if ui.button("Jump").clicked() {
                                self.timeline_state.cursor_time_ms = issue.timestamp_ms;
                                self.active_tab = HubTab::Studio;
                            }
                        });
                        if i < rep.issues.len() - 1 {
                            ui.separator();
                        }
                    }
                }
            } else {
                ui.label("No script loaded. Open a funscript to audit.");
            }
        });
    }

    // =========================================================================
    // View 5: Device & Sync (F5)
    // =========================================================================
    fn render_device_sync_view(&mut self, ui: &mut Ui) {
        ui.heading("Hardware Device & T-Code Sync");
        ui.label("Real-time T-Code protocol telemetry and device output simulation for OSR, Handy, Keon, and MultiFunPlayer.");
        ui.add_space(10.0);

        let cur_pos = self.script.interpolate_position(self.timeline_state.cursor_time_ms);
        let tcode_cmd = format!("L0{:03}I100\n", (cur_pos * 10.0).round() as i32);

        ui.horizontal(|ui| {
            // Left: Position Gauge
            ui.group(|ui| {
                ui.set_width(200.0);
                ui.heading("Stroke Position");
                ui.add_space(4.0);

                // Vertical gauge
                let gauge_height = 240.0;
                let gauge_width = 36.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(gauge_width, gauge_height), egui::Sense::hover());
                let painter = ui.painter_at(rect);

                // Background
                painter.rect_filled(rect, 4.0, Color32::from_rgb(30, 35, 45));

                // Active fill bar
                let fill_height = (cur_pos / 100.0) * gauge_height;
                let fill_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.bottom() - fill_height),
                    rect.max,
                );
                painter.rect_filled(fill_rect, 4.0, Color32::from_rgb(0, 200, 255));

                ui.add_space(6.0);
                ui.strong(format!("{:.1}%", cur_pos));
            });

            ui.add_space(10.0);

            // Right: Protocol Stream Monitor & Network
            ui.vertical(|ui| {
                ui.group(|ui| {
                    ui.heading("T-Code v0.3 Telemetry Stream");
                    ui.monospace("Target Axis:   L0 (Main Stroke)");
                    ui.monospace(format!("Command Feed:  {tcode_cmd}"));
                    ui.monospace(format!("Playhead Time: {} ms", self.timeline_state.cursor_time_ms));
                    ui.monospace(format!("Playback:      {}", if self.is_playing { "Active (Streaming)" } else { "Paused" }));
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Network Broadcast (UDP)");
                    ui.horizontal(|ui| {
                        ui.label("Target UDP Host:Port:");
                        ui.text_edit_singleline(&mut self.sync_udp_address);
                    });

                    ui.horizontal(|ui| {
                        let btn_label = if self.sync_udp_enabled { "Disconnect UDP" } else { "Connect UDP Broadcast" };
                        if ui.button(btn_label).clicked() {
                            self.sync_udp_enabled = !self.sync_udp_enabled;
                            self.set_status(if self.sync_udp_enabled {
                                format!("Streaming T-Code to UDP {}", self.sync_udp_address)
                            } else {
                                "UDP Broadcast disconnected".to_string()
                            });
                        }
                        if self.sync_udp_enabled {
                            ui.colored_label(Color32::GREEN, "● Connected");
                        } else {
                            ui.colored_label(Color32::GRAY, "○ Standby");
                        }
                    });
                });
            });
        });
    }

    // =========================================================================
    // Generator Worker Launcher
    // =========================================================================
    fn start_generation_task(&mut self) {
        let video = match self.video_path.clone() {
            Some(v) => v,
            None => return,
        };
        let model = if self.use_neural { self.model_path.clone() } else { None };
        let fps = self.target_fps;
        let pov = self.pov_mode;
        let vr = self.vr_mode;
        let conf = self.neural_conf;
        let detrend_win = self.detrend_window;
        let norm_win = self.norm_window;
        let multi_axis = self.generate_multi_axis;

        let (tx, rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = channel();
        self.worker_rx = Some(rx);
        self.is_generating = true;
        self.generation_progress = 0.0;
        self.generation_status = "Initializing generator...".to_string();

        thread::spawn(move || {
            let width = if model.is_some() { 640 } else { 256 };
            let height = if model.is_some() { 640 } else { 256 };
            let est_total = crate::video::probe_video(&video)
                .ok()
                .map(|m| (m.duration_secs * fps).round().max(1.0) as u64);

            let res = (|| -> anyhow::Result<MultiAxisScript> {
                let stream_cfg = crate::video::StreamConfig {
                    target_width: width,
                    target_height: height,
                    target_fps: fps,
                    vr_mode: vr,
                    is_rgb: model.is_some(),
                };

                let mut stream = crate::video::FrameStreamReader::new(&video, &stream_cfg)?;
                let mut all_samples = Vec::new();
                let mut neural_poses: Vec<(crate::neural::Pose3D, i64)> = Vec::new();

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
                        let pct = est_total
                            .map(|tot| (frame_count as f32 / tot as f32).min(0.99))
                            .unwrap_or(0.5);
                        let _ = tx.send(WorkerMessage::Progress {
                            percent: pct,
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
                        let (pose, _) = trk.update_pose(&detections, Some(&flow));
                        neural_poses.push((pose, ts));
                    }

                    last_raw = curr_raw;
                }

                let mut bundle = MultiAxisScript::new();

                if model.is_some() && !neural_poses.is_empty() {
                    let timestamps: Vec<i64> = neural_poses.iter().map(|(_, ts)| *ts).collect();
                    let stroke_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.stroke).collect();
                    let mut stroke_script = Funscript::new(crate::signal::extract_actions(&stroke_values, &timestamps, true));
                    stroke_script.sanitize();
                    bundle.channels.insert(AxisChannel::Stroke, stroke_script);

                    if multi_axis {
                        let surge_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.surge).collect();
                        let mut surge_script = Funscript::new(crate::signal::extract_actions(&surge_values, &timestamps, true));
                        surge_script.sanitize();
                        bundle.channels.insert(AxisChannel::Surge, surge_script);

                        let sway_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.sway).collect();
                        let mut sway_script = Funscript::new(crate::signal::extract_actions(&sway_values, &timestamps, true));
                        sway_script.sanitize();
                        bundle.channels.insert(AxisChannel::Sway, sway_script);

                        let pitch_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.pitch).collect();
                        let mut pitch_script = Funscript::new(crate::signal::extract_actions(&pitch_values, &timestamps, true));
                        pitch_script.sanitize();
                        bundle.channels.insert(AxisChannel::Pitch, pitch_script);

                        let roll_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.roll).collect();
                        let mut roll_script = Funscript::new(crate::signal::extract_actions(&roll_values, &timestamps, true));
                        roll_script.sanitize();
                        bundle.channels.insert(AxisChannel::Roll, roll_script);

                        let twist_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.twist).collect();
                        let mut twist_script = Funscript::new(crate::signal::extract_actions(&twist_values, &timestamps, true));
                        twist_script.sanitize();
                        bundle.channels.insert(AxisChannel::Twist, twist_script);

                        let suction_values: Vec<f32> = neural_poses.iter().map(|(p, _)| p.suction).collect();
                        let mut suction_script = Funscript::new(crate::signal::extract_actions(&suction_values, &timestamps, true));
                        suction_script.sanitize();
                        bundle.channels.insert(AxisChannel::Suction, suction_script);
                    }
                } else {
                    let (cum, ts) = crate::signal::integrate_flow(&all_samples);
                    let norm = crate::signal::detrend_and_normalize(&cum, fps, detrend_win, norm_win, 1000.0);
                    let mut stroke_script = Funscript::new(crate::signal::extract_actions(&norm, &ts, true));
                    stroke_script.sanitize();
                    bundle.channels.insert(AxisChannel::Stroke, stroke_script);
                }

                Ok(bundle)
            })();

            let _ = tx.send(WorkerMessage::Done(res.map_err(|e| e.to_string())));
        });
    }

    fn open_funscript_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Funscript", &["funscript", "json"])
            .pick_file()
        {
            if let Ok(bundle) = MultiAxisScript::load_bundle(&path) {
                self.multi_axis = bundle;
                self.active_axis = AxisChannel::Stroke;
                self.script = self.multi_axis.get_or_create(AxisChannel::Stroke).clone();
                self.script_path = Some(path.clone());
                self.run_doctor();
                self.fit_timeline();
                let active_count = self
                    .multi_axis
                    .channels
                    .values()
                    .filter(|s| !s.actions.is_empty())
                    .count();
                self.set_status(format!(
                    "Loaded multi-axis bundle ({} active channels) from {}",
                    active_count,
                    path.display()
                ));
            }
        }
    }

    fn save_funscript(&mut self) {
        self.multi_axis.channels.insert(self.active_axis, self.script.clone());
        if let Some(ref path) = self.script_path {
            if self.multi_axis.save_bundle(path).is_ok() {
                self.set_status(format!("Saved multi-axis bundle for {}", path.display()));
            }
        } else {
            self.save_funscript_as();
        }
    }

    fn save_funscript_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Funscript", &["funscript"])
            .save_file()
        {
            self.multi_axis.channels.insert(self.active_axis, self.script.clone());
            if self.multi_axis.save_bundle(&path).is_ok() {
                self.script_path = Some(path.clone());
                self.set_status(format!("Saved multi-axis bundle to {}", path.display()));
            }
        }
    }

    fn select_video_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Video Files", &["mp4", "mkv", "mov", "avi", "webm"])
            .pick_file()
        {
            if let Ok(meta) = crate::video::probe_video(&path) {
                self.video_info = Some(format!(
                    "{}x{}, {:.2} fps, {:.1}s ({} frames)",
                    meta.width, meta.height, meta.fps, meta.duration_secs, meta.total_frames
                ));
            }
            self.video_path = Some(path.clone());

            // Extract audio waveform in background thread
            let (tx, rx) = channel();
            self.waveform_rx = Some(rx);
            let path_for_audio = path.clone();
            thread::spawn(move || {
                let wf = crate::audio::extract_audio_waveform(&path_for_audio).unwrap_or_default();
                let _ = tx.send(wf);
            });

            // Reset video frame cache
            self.last_video_frame_ts = -999;
            self.video_texture = None;
            self.set_status(format!("Selected video: {}", path.file_name().unwrap_or_default().to_string_lossy()));
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

fn format_ms(ms: i64) -> String {
    let total_secs = ms.max(0) / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = ms.max(0) % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}
