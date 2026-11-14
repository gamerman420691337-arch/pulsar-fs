use super::*;

impl PulsarDesktop {
    pub(super) fn fallback_binding(&self) -> Option<inference_fallback_panel::Binding> {
        if self.generation_mode != GeneratorInputMode::Video
            || !self.use_model
            || !self.has_media_source()
        {
            return None;
        }
        let profile = self.generation_profile()?;
        profile.validate().ok()?;
        let choice = if self.use_selected_pack {
            Some(self.pack_panel.generation_choice().ok()?)
        } else {
            if self.model_path.trim().is_empty() {
                return None;
            }
            None
        };
        let project = self.project.as_ref()?;
        Some(inference_fallback_panel::Binding {
            session: self.fallback_session.clone()?,
            project: project.project_id.clone(),
            revision: project.revision,
            source: self.source.clone()?,
            model_key: serde_json::to_vec(&(
                self.use_selected_pack,
                &self.model_path,
                choice,
                profile,
            ))
            .ok()?,
        })
    }

    pub(super) fn generation_profile(&self) -> Option<DeclaredInferenceProfile> {
        if !self.use_model {
            return None;
        }
        if self.use_selected_pack {
            self.pack_panel.profile()
        } else {
            self.inference_profile.clone()
        }
    }
    pub(super) fn model_contract(&self) -> Option<ModelInputContract> {
        if let Some(profile) = self.generation_profile() {
            return Some(profile.preprocessing.input);
        }
        if self.use_model && self.use_selected_pack {
            return None;
        }
        self.use_model.then_some(ModelInputContract {
            width: self.model_width,
            height: self.model_height,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: self.model_classes,
            decoder: DetectorDecoder::YoloChannelMajor,
        })
    }

    pub(super) fn generation_analysis_dimensions(&self) -> (u32, u32) {
        let input = self.model_contract();
        let custom = self.use_analysis_dimensions
            && !input
                .as_ref()
                .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition);
        crate::inference::generation_analysis_dimensions(
            input.as_ref(),
            custom.then_some(self.analysis_dimensions[0]),
            custom.then_some(self.analysis_dimensions[1]),
        )
    }

    pub(super) fn generation_detector_score_threshold(&self) -> Option<f32> {
        selected_detector_score_threshold(
            self.use_detector_score_threshold,
            self.detector_score_threshold,
            self.model_contract().as_ref(),
        )
    }

    pub(super) fn selected_model(&self) -> anyhow::Result<Option<PathBuf>> {
        if !self.use_model {
            return Ok(None);
        }
        if self.use_selected_pack {
            self.pack_panel.generation_choice()?;
            return Ok(None);
        }
        if self.model_path.trim().is_empty() {
            anyhow::bail!("Select a model path or disable neural analysis");
        }
        Ok(Some(absolute_path(std::path::Path::new(&self.model_path))?))
    }

    pub(super) fn request_preview(&mut self) {
        if !self.has_media_source() {
            self.preview_pending = false;
            self.preview_playing = false;
            self.preview.clear();
            self.texture = None;
            self.previous_texture = None;
            return;
        }
        if self.busy {
            return;
        }
        let (Some(source), Some(project)) = (self.source.clone(), self.project.as_ref()) else {
            return;
        };
        let placement = project
            .placements
            .iter()
            .find(|p| Some(&p.placement_id) == self.selected_placement.as_ref())
            .cloned();
        let source = placement
            .as_ref()
            .map(|p| p.spec.source_version.clone())
            .unwrap_or(source);
        let mut context = FrameContext {
            source_version: source.clone(),
            source_placement: placement
                .as_ref()
                .map(|p| p.placement_id.clone())
                .unwrap_or_else(|| {
                    SourcePlacementId::new(format!("primary:{source}"))
                        .expect("bounded source identifier")
                }),
            // For time-based seeks, the engine replaces this placeholder with
            // the decoder's actual ordinal. It is never shown as a source frame.
            frame: FrameId::new(0),
            transform: match video_view_transform(self.video_view.as_ref()) {
                Ok(value) => value,
                Err(error) => {
                    self.status = error.to_string();
                    self.preview_pending = false;
                    return;
                }
            },
            seek_generation: self.seek_generation,
            request_generation: self.request_generation,
        };
        let source_time = match SourceTimestamp::new(
            (self.playhead_seconds * 1_000_000_000.0).round() as i64,
            1_000_000_000,
        ) {
            Ok(value) => value,
            Err(error) => {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
        };
        let plain_reference_preview = self.tab == Tab::References;
        let preview_model = self.use_model && !self.author_target_mode && !plain_reference_preview;
        let mut model_path = match if self.author_target_mode || plain_reference_preview {
            Ok(None)
        } else {
            self.selected_model()
        } {
            Ok(value) => value,
            Err(error) => {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
        };
        let project_id = project.project_id.clone();
        let profile = self.generation_profile();
        let annotated_profile = preview_model
            && profile.as_ref().is_some_and(|p| {
                matches!(
                    p.preprocessing.resize,
                    InferenceResize::AnnotatedTemporalView
                )
            });
        if annotated_profile || (preview_model && self.annotated_view.is_some()) {
            let prepared = (|| -> anyhow::Result<_> {
                let profile=profile.ok_or_else(||anyhow::anyhow!("Annotated target preview requires an explicit annotated CPU or CUDA profile"))?;
                let spec = AnnotatedPreviewSpec {
                    view: self.annotated_view.clone().ok_or_else(|| {
                        anyhow::anyhow!("Load a source-bound target view for annotated preview")
                    })?,
                    source_sample_stride: 1,
                };
                spec.validate_profile(&profile)?;
                context.transform = annotated_preview_transform(&spec.view)?;
                let model = if self.use_selected_pack {
                    AnnotatedPreviewModel::Pack {
                        choice: self.pack_panel.generation_choice()?,
                    }
                } else {
                    AnnotatedPreviewModel::File {
                        model_path: model_path
                            .clone()
                            .ok_or_else(|| anyhow::anyhow!("Annotated preview model is absent"))?,
                        profile: (profile.clone()).into(),
                    }
                };
                let placed = placement.clone().map(|placement| PlacedPreviewRequest {
                    placement,
                    project_time: ProjectTime::from_nanos(source_time.numerator()),
                });
                let time = if let Some(request) = &placed {
                    ModelPackPreviewTime::Placement {
                        placement_id: request.placement.placement_id.clone(),
                        project_time: request.project_time,
                    }
                } else {
                    ModelPackPreviewTime::Source {
                        source_time,
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    }
                };
                self.preview
                    .begin_annotated(context, time, placed, spec, model, profile)
            })();
            self.preview_pending = false;
            match prepared {
                Ok(command) => {
                    self.last_preview = Instant::now();
                    self.send(command, Some(project_id), None);
                }
                Err(error) => {
                    self.preview.clear();
                    self.texture = None;
                    self.previous_texture = None;
                    self.preview_playing = false;
                    self.status = error.to_string();
                }
            }
            return;
        }
        let mut model_input = if self.author_target_mode || plain_reference_preview {
            None
        } else {
            self.model_contract()
        };
        let mut inference_profile = preview_model
            .then(|| self.inference_profile.clone())
            .flatten();
        let pack_detector = preview_model
            && self.use_selected_pack
            && model_input
                .as_ref()
                .is_some_and(|c| c.decoder == DetectorDecoder::YoloChannelMajor);
        let pack_choice = if pack_detector {
            match self.pack_panel.generation_choice() {
                Ok(choice) => Some(choice),
                Err(error) => {
                    self.status = error.to_string();
                    self.preview_pending = false;
                    return;
                }
            }
        } else {
            None
        };
        if self.use_selected_pack
            || model_input
                .as_ref()
                .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition)
        {
            model_path = None;
            model_input = None;
            inference_profile = None;
        }
        if let Some(placement) = placement {
            let request = PlacedPreviewRequest {
                placement,
                project_time: ProjectTime::from_nanos(source_time.numerator()),
            };
            if let Err(error) = request.validate(&context) {
                self.status = error.to_string();
                self.preview_pending = false;
                return;
            }
            self.preview.begin_placed(context.clone(), request.clone());
            self.preview_pending = false;
            self.last_preview = Instant::now();
            let command = if let Some(choice) = pack_choice {
                Command::PreviewModelPackFrame {
                    include_source_end: false,
                    context,
                    time: ModelPackPreviewTime::Placement {
                        placement_id: request.placement.placement_id,
                        project_time: request.project_time,
                    },
                    video_view: self.video_view.clone(),
                    choice,
                }
            } else {
                Command::PreviewPlacedFrame {
                    placement_id: request.placement.placement_id,
                    project_time: request.project_time,
                    context,
                    video_view: self.video_view.clone(),
                    model_path,
                    model_input,
                    inference_profile,
                }
            };
            self.send(command, Some(project_id), None);
            return;
        }
        self.preview.begin_timed(
            context.clone(),
            source_time,
            PreviewTimeBasis::ElapsedFromFirstFrame,
        );
        self.preview_pending = false;
        self.last_preview = Instant::now();
        let include_source_end = self.capabilities.as_ref().is_some_and(|capabilities| {
            capabilities
                .operations
                .iter()
                .any(|op| op == "preview_source_end")
        });
        self.send(
            if let Some(choice) = pack_choice {
                Command::PreviewModelPackFrame {
                    include_source_end,
                    context,
                    time: ModelPackPreviewTime::Source {
                        source_time,
                        time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    },
                    video_view: self.video_view.clone(),
                    choice,
                }
            } else {
                Command::PreviewFrame {
                    include_source_end,
                    time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                    video_view: self.video_view.clone(),
                    context,
                    source_time,
                    model_path,
                    model_input,
                    inference_profile,
                }
            },
            Some(project_id),
            None,
        );
    }

    pub(super) fn generator(&mut self, ui: &mut egui::Ui) {
        ui.heading("Generator");
        ui.horizontal(|ui| {
            ui.label("Existing candidate ID");
            ui.text_edit_singleline(&mut self.open_candidate_id);
            if ui
                .add_enabled(
                    !self.busy
                        && self.project.is_some()
                        && !self.open_candidate_id.trim().is_empty(),
                    egui::Button::new("Open candidate for review"),
                )
                .clicked()
            {
                match CandidateId::new(self.open_candidate_id.trim()) {
                    Ok(candidate_id) => {
                        self.project_command(Command::GetCandidate { candidate_id }, false);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        });
        egui::ComboBox::from_label("Input modality")
            .selected_text(self.generation_mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    GeneratorInputMode::Video,
                    GeneratorInputMode::Text,
                    GeneratorInputMode::Patterns,
                    GeneratorInputMode::Image,
                    GeneratorInputMode::Audio,
                    GeneratorInputMode::Funscript,
                ] {
                    ui.selectable_value(&mut self.generation_mode, mode, mode.label());
                }
            });
        ui.label("Immutable input -> isolated worker -> review candidate -> atomic commit.");
        match self.generation_mode {
            GeneratorInputMode::Video => self.video_generator(ui),
            GeneratorInputMode::Patterns => {
                ui.label("One strict pattern line per axis (one through six axes)");
                ui.text_edit_multiline(&mut self.pattern_prompt);
                ui.small("Use the same duration for every axis. Axes: stroke, sway, surge, roll, pitch, yaw.");
                ui.colored_label(Color32::YELLOW,"Explicit synthetic patterns; no observed motion or free-form AI interpretation.");
                if ui
                    .add_enabled(
                        !self.busy && self.project.is_some(),
                        egui::Button::new("Generate axis-pattern candidate"),
                    )
                    .clicked()
                {
                    match crate::patterns::build_pattern_input(&self.pattern_prompt)
                        .and_then(crate::patterns::candidate_command)
                    {
                        Ok(command) => {
                            self.project_command(command, true);
                        }
                        Err(error) => self.status = format!("{error:#}"),
                    }
                }
            }
            GeneratorInputMode::Text | GeneratorInputMode::Image => {
                ui.label("Strict pattern syntax");
                ui.text_edit_multiline(&mut self.pattern_prompt);
                ui.small("Required: pattern, axis, duration, frequency, amplitude, offset. Patterns: sine, triangle, hold, pulse.");
                ui.colored_label(Color32::YELLOW, "Deterministic preset syntax, not free-form language understanding or a local AI assistant.");
                if self.generation_mode == GeneratorInputMode::Image {
                    ui.label("Import/select a still image in Project. Prompt supplies motion; image content is not claimed to infer motion.");
                }
                let enabled = !self.busy
                    && self.project.is_some()
                    && (self.generation_mode == GeneratorInputMode::Text
                        || self.has_media_source());
                if ui
                    .add_enabled(enabled, egui::Button::new("Synthesize pattern candidate"))
                    .clicked()
                {
                    if self.pattern_prompt.trim().is_empty() {
                        self.status = "A nonempty constrained pattern prompt is required.".into();
                    } else {
                        let input = if self.generation_mode == GeneratorInputMode::Text {
                            GenerationInput::Text {
                                prompt: self.pattern_prompt.clone(),
                            }
                        } else {
                            GenerationInput::Image {
                                source_version: self.source.clone().unwrap(),
                                prompt: self.pattern_prompt.clone(),
                            }
                        };
                        self.project_command(Command::GenerateInput { input }, true);
                    }
                }
            }
            GeneratorInputMode::Audio => {
                let source = self
                    .has_media_source()
                    .then(|| self.source.clone())
                    .flatten();
                if let Some(input) =
                    self.audio_panel
                        .ui(ui, source, !self.busy && self.project.is_some())
                {
                    match input {
                        Ok(input) => {
                            self.project_command(Command::GenerateInput { input }, true);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            GeneratorInputMode::Funscript => {
                ui.label("Funscript path");
                ui.text_edit_singleline(&mut self.script_import_path);
                ui.label("Import proposes an engine-owned candidate. Original script and committed timeline remain unchanged.");
                if ui
                    .add_enabled(
                        !self.busy && self.project.is_some(),
                        egui::Button::new("Import script candidate"),
                    )
                    .clicked()
                {
                    if self.script_import_path.trim().is_empty() {
                        self.status = "Enter a funscript path.".into();
                    } else {
                        match absolute_path(std::path::Path::new(&self.script_import_path)) {
                            Ok(path) => {
                                self.project_command(Command::ImportFunscript { path }, true);
                            }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                }
            }
        }
        if let Some(job) = self.job.clone() {
            ui.separator();
            ui.label(format!("Job {} / attempt {}", job.job_id, job.attempt_id));
            ui.label(format!("State: {}", job.state));
            if job.state == "fallback_pending" && job.error.is_none() {
                ui.label("Checking the selected equivalent backends. This job remains active and can be cancelled.");
            }
            if ui
                .add_enabled(
                    !self.busy
                        && self
                            .capabilities
                            .as_ref()
                            .is_some_and(|c| c.operations.iter().any(|o| o == "job_attempts")),
                    egui::Button::new("Inspect attempt history"),
                )
                .clicked()
            {
                self.project_command(
                    Command::JobAttempts {
                        job_id: job.job_id.clone(),
                        after_attempt: None,
                        limit: 8,
                    },
                    false,
                );
            }
            if let Some(page) = self
                .job_attempts
                .clone()
                .filter(|p| p.job_id == job.job_id && p.current_attempt == job.attempt_id)
            {
                ui.collapsing("Attempt declarations and completion observations",|ui| {
                    ui.small("Attempt-ID order; declarations are not proof of executed provider placement. Missing completion or retirement is unknown. This history does not authorize fallback.");
                    for attempt in &page.attempts {
                        ui.label(format!("{} · {}",attempt.attempt_id,attempt.state));
                        if let Some(profile)=&attempt.declared_profile {
                            ui.small(format!("Declared {:?} · profile {}",profile.provider,attempt.declared_profile_sha256.as_deref().unwrap_or("unavailable")));
                        } else {ui.small("No explicit profile retained; historical defaults are not inferred.");}
                        if let Some(outcome)=&attempt.outcome {
                            if let Some(error)=&outcome.validation_error {ui.label(format!("{:?}: {}{}",error.code,error.message,if error.message_truncated {" [truncated; full message digest retained]"}else{""}));}
                            ui.small(if outcome.worker_retirement.is_some(){"Worker-tree retirement observed."}else{"Worker-tree retirement not established by this record."});
                            if let Some(absence)=outcome.worker_retirement.as_ref().and_then(|r|r.provider_unavailable.as_ref()) {ui.label(format!("{:?} was absent from the selected runtime's provider list.",absence.provider));}
                        } else {ui.small("No completion observation retained.");}
                        ui.separator();
                    }
                    if let Some(after)=page.next_after {
                        if ui.add_enabled(!self.busy,egui::Button::new("Next attempt page")).clicked() {
                            self.project_command(Command::JobAttempts {job_id:job.job_id.clone(),after_attempt:Some(after),limit:8},false);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh job"))
                    .clicked()
                {
                    self.project_command(
                        Command::JobStatus {
                            job_id: job.job_id.clone(),
                        },
                        false,
                    );
                }
                if ui
                    .add_enabled(
                        !self.busy && job.candidate_id.is_none(),
                        egui::Button::new("Cancel job"),
                    )
                    .clicked()
                {
                    self.project_command(
                        Command::CancelJob {
                            job_id: job.job_id.clone(),
                        },
                        false,
                    );
                }
                if let Some(candidate_id) = job.candidate_id {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Review candidate"))
                        .clicked()
                    {
                        self.project_command(Command::GetCandidate { candidate_id }, false);
                    }
                }
            });
        }
        if let Some(candidate) = self.candidate.clone() {
            ui.separator();
            let stale = self
                .project
                .as_ref()
                .is_some_and(|project| project.revision != candidate.base_revision);
            self.candidate_controls(ui, &candidate, stale);
        }
        ui.separator();
        ui.label("Free-form local AI, visual-semantic image interpretation, trained multimodal inference and physical qualification remain explicit implementation/research gaps.");
    }

    pub(super) fn video_generator(&mut self, ui: &mut egui::Ui) {
        if self.selected_placement.is_some() {
            ui.colored_label(Color32::YELLOW,"Generation uses the selected placement's project timing. Existing motion changes only through an explicit candidate commit.");
        }
        self.video_view_controls(ui);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Inspect inference support"))
            .clicked()
        {
            self.send(Command::InferenceCatalog, None, None);
        }
        if let Some(catalog) = &self.inference_catalog {
            ui.label(&catalog.limitation);
            ui.label(format!(
                "Worker available: {}; runtime installed: {}; qualified profiles: {}",
                catalog.worker_available,
                catalog.runtime.is_some(),
                catalog.qualified_profiles.len()
            ));
        }
        egui::ComboBox::from_label("Stroke preset")
            .selected_text(&self.preset)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.preset, "default".into(), "Default");
                ui.selectable_value(&mut self.preset, "fast".into(), "Fast");
                let available = self
                    .model_contract()
                    .is_some_and(|input| input.decoder != DetectorDecoder::TemporalScriptPosition);
                ui.add_enabled_ui(available, |ui| {
                    ui.selectable_value(&mut self.preset, "maximum".into(), "Maximum");
                });
            });
        if self.preset == "maximum" {
            ui.label("Maximum uses denser visual tracking. Select a detector model; a quality improvement is not yet qualified.");
        }
        if ui
            .checkbox(
                &mut self.use_model,
                "Use declared model analysis (unqualified)",
            )
            .changed()
        {
            self.seek(self.playhead_seconds);
        }
        if let Some(pack) = self.bundled_models.ui(ui, self.busy) {
            if let Err(error) = self.pack_panel.prepare_bundled_import(&pack) {
                self.status = error.to_string();
            }
        }
        if let Some(action) = self.pack_panel.ui(ui, self.busy) {
            self.submit_pack_action(action);
        }
        if self.use_model {
            let previous = self.use_selected_pack;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.use_selected_pack, false, "Model file");
                ui.selectable_value(
                    &mut self.use_selected_pack,
                    true,
                    "Owner-selected offline pack",
                );
            });
            if previous != self.use_selected_pack {
                self.seek(self.playhead_seconds);
            }
            if self.use_selected_pack {
                if let Some(profile) = self.pack_panel.profile() {
                    ui.label(format!(
                        "Selected {:?} profile, {} threads; {} × {}, {} classes.",
                        profile.provider,
                        profile.cpu_threads,
                        profile.preprocessing.input.width,
                        profile.preprocessing.input.height,
                        profile.preprocessing.input.class_count
                    ));
                } else {
                    ui.label("Refresh and explicitly approve a pack before generation.");
                }
                ui.label("Annotated CPU or CUDA packs use the loaded source-bound target view for frame-pair preview. Detector packs use the selected video view.");
            } else {
                ui.label("Model path");
                if ui.text_edit_singleline(&mut self.model_path).changed() {
                    self.seek(self.playhead_seconds);
                }
                ui.label("Optional inference profile JSON");
                ui.text_edit_singleline(&mut self.inference_profile_path);
                ui.horizontal(|ui| {
                if ui.button("Load profile").clicked() {
                    match crate::inference::read_profile(std::path::Path::new(&self.inference_profile_path)) {
                        Ok(profile) => {
                            self.inference_profile = Some(profile);
                            self.status = "Profile loaded. The engine checks exact model and runtime bytes before execution.".into();
                            self.seek(self.playhead_seconds);
                        }
                        Err(error) => self.status = format!("Profile was not changed: {error:#}"),
                    }
                }
                if ui.button("Use default CPU profile").clicked() {
                    self.inference_profile = None;
                    self.seek(self.playhead_seconds);
                }
            });
                if let Some(profile) = &self.inference_profile {
                    ui.label(format!(
                        "Initial profile: {:?}, {} threads; {} × {}, {} classes.",
                        profile.provider,
                        profile.cpu_threads,
                        profile.preprocessing.input.width,
                        profile.preprocessing.input.height,
                        profile.preprocessing.input.class_count
                    ));
                }
                ui.add_enabled_ui(self.inference_profile.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Input");
                        ui.add(egui::DragValue::new(&mut self.model_width).range(1..=4096));
                        ui.add(egui::DragValue::new(&mut self.model_height).range(1..=4096));
                        ui.label("Classes");
                        ui.add(egui::DragValue::new(&mut self.model_classes).range(1..=65536));
                    });
                });
                ui.label(format!(
                    "Declared model decoder: {:?}. No format guessing.",
                    self.model_contract().map(|c| c.decoder)
                ));
                if self
                    .model_contract()
                    .is_some_and(|c| c.decoder == DetectorDecoder::TemporalScriptPosition)
                {
                    ui.label("Annotated CPU or CUDA profiles preview the declared target frame pair. Other script-position profiles show pixels only. Estimates remain unqualified.");
                }
                ui.colored_label(Color32::YELLOW, "Custom model output is unqualified. Runtime must be installed and pinned by the engine.");
            }
        }
        let temporal_analysis = self
            .model_contract()
            .is_some_and(|input| input.decoder == DetectorDecoder::TemporalScriptPosition);
        ui.add_enabled_ui(!temporal_analysis, |ui| {
            ui.checkbox(
                &mut self.use_analysis_dimensions,
                "Custom decoded analysis size",
            );
            ui.add_enabled_ui(self.use_analysis_dimensions, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut self.analysis_dimensions[0]).range(1..=16384));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut self.analysis_dimensions[1]).range(1..=16384));
                });
            });
        });
        let (analysis_width, analysis_height) = self.generation_analysis_dimensions();
        ui.label(format!("Generation analysis limit: {analysis_width} × {analysis_height}. Flat video keeps its aspect ratio. Preview size is independent."));
        if temporal_analysis {
            ui.label("This temporal model requires fixed 64 × 64 analysis.");
        }
        let detector_available = self
            .model_contract()
            .is_some_and(|c| c.decoder != DetectorDecoder::TemporalScriptPosition);
        ui.add_enabled_ui(detector_available, |ui| {
            ui.checkbox(
                &mut self.use_detector_score_threshold,
                "Custom minimum generation detector score",
            );
            ui.add_enabled(
                self.use_detector_score_threshold,
                egui::Slider::new(&mut self.detector_score_threshold, 0.0..=1.0)
                    .text("Minimum score"),
            );
        });
        ui.label(
            "Generation detector scores are uncalibrated. The default and preview cutoff is 0.25.",
        );
        ui.label(
            "Annotated target view (optional, requires an annotated temporal CPU or CUDA profile)",
        );
        ui.text_edit_singleline(&mut self.annotated_view_path);
        ui.horizontal(|ui| {
            if ui.button("Load target view").clicked() {
                match crate::inference::read_annotated_view(std::path::Path::new(&self.annotated_view_path)) {
                    Ok(view)=>{self.annotated_view=Some(view);self.seek(self.playhead_seconds);self.status="Target view loaded. Preview and generation check its source content binding.".into();},
                    Err(error)=>self.status=format!("Target view was not changed: {error:#}"),
                }
            }
            if ui.button("Clear target view").clicked() {self.annotated_view=None;self.seek(self.playhead_seconds);}
        });
        if let Some(view) = &self.annotated_view {
            ui.label(format!(
                "Target {}; region {:?}; source {}",
                view.declaration.target_id, view.declaration.target_region, view.source_sha256
            ));
        }
        let can_author = self.has_media_source() && self.video_view.is_some();
        if ui
            .add_enabled(
                can_author,
                egui::Checkbox::new(&mut self.author_target_mode, "Draw target view in preview"),
            )
            .changed()
        {
            self.preview_playing = false;
            if self.author_target_mode {
                self.pick_target_mode = false;
            }
            self.seek(self.playhead_seconds);
        }
        if self.author_target_mode {
            ui.small("Declare the eye/projection, then drag inside the paused preview. This fixed rectangle declares geometry only; it does not check tracking or script timing.");
            ui.horizontal(|ui| {
                ui.label("Target ID");
                ui.text_edit_singleline(&mut self.authored_target_id);
            });
            let draft = self.target_author.current_draft(
                self.preview.displayed(),
                self.video_view.as_ref(),
                &self.authored_target_id,
            );
            if let Ok(view) = &draft {
                ui.label(format!(
                    "Draft region {:?}; analysis limits {:?}",
                    view.declaration.target_region, view.declaration.analysis_dimensions
                ));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(draft.is_ok(), egui::Button::new("Use drawn target view"))
                    .clicked()
                {
                    if let Ok(view) = &draft {
                        self.annotated_view = Some(view.clone());
                        self.author_target_mode = false;
                        self.seek(self.playhead_seconds);
                        self.status =
                            "Target view selected. Model and source admission still apply.".into();
                    }
                }
                if ui
                    .add_enabled(
                        draft.is_ok() && !self.annotated_view_path.trim().is_empty(),
                        egui::Button::new("Save new drawn target view"),
                    )
                    .clicked()
                {
                    if let Ok(view) = &draft {
                        self.status = match crate::target_view_authoring::save_new(
                            std::path::Path::new(&self.annotated_view_path),
                            view,
                        ) {
                            Ok(path) => format!("Target view saved: {}", path.display()),
                            Err(error) => format!("Target view save failed: {error:#}"),
                        };
                    }
                }
            });
        }
        ui.checkbox(
            &mut self.use_generation_interval,
            "Reanalyse a project-time interval",
        );
        if self.use_generation_interval {
            ui.horizontal(|ui| {
                for (index, label) in ["Start ns (inclusive)", "End ns (exclusive)"]
                    .into_iter()
                    .enumerate()
                {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut self.generation_interval_ns[index])
                            .range(0..=i64::MAX),
                    );
                }
            });
            ui.label("Only interval samples reach perception. Source decoding still verifies the container; clocks and source stride are unchanged. Apply preserves outside motion and protected stencils.");
        }
        let can_pick = self.has_media_source()
            && self.use_model
            && self
                .model_contract()
                .is_some_and(|c| c.decoder != DetectorDecoder::TemporalScriptPosition);
        if ui
            .add_enabled(
                can_pick || self.pick_target_mode,
                egui::Checkbox::new(
                    &mut self.pick_target_mode,
                    "Pick observed target in preview",
                ),
            )
            .changed()
        {
            self.preview_playing = false;
            if self.pick_target_mode {
                self.author_target_mode = false;
            }
            self.seek(self.playhead_seconds);
        }
        if self.pick_target_mode {
            ui.small("Click one observed box in the paused preview, then explicitly use the draft. Overlapping boxes and missing detections refuse. Generation re-detects at the original frame; identity and quality remain unqualified.");
            let picked = self
                .target_picker
                .current_draft(self.preview.displayed(), self.video_view.as_ref());
            if let Ok(picked) = &picked {
                ui.label(format!(
                    "Picked source frame {}; normalized point {:.4}, {:.4}",
                    picked.seed.source_frame_index, picked.seed.x, picked.seed.y
                ));
            }
            if ui
                .add_enabled(
                    picked.is_ok(),
                    egui::Button::new("Use picked tracking target"),
                )
                .clicked()
            {
                if let Ok(picked) = picked {
                    self.target_seed_frame = picked.seed.source_frame_index;
                    self.target_seed_point = [picked.seed.x, picked.seed.y];
                    // Preview observations carry no detector class; never invent one.
                    self.target_seed_filter_class = false;
                    self.target_seed_binding = Some((
                        picked.context().source_version.clone(),
                        picked.context().transform.clone(),
                    ));
                    self.use_target_seed = true;
                    self.pick_target_mode = false;
                    self.target_picker.clear();
                    self.status = "Target seed selected. Preview has no class metadata; add a class filter explicitly if needed. Generation admission and re-detection still apply.".into();
                }
            }
        }
        ui.checkbox(
            &mut self.use_target_seed,
            "Select a tracking target explicitly",
        );
        if self.use_target_seed {
            ui.label("At this original source-frame ordinal, select the unique observed box containing the analysis-view point. Missing identity remains a gap; no automatic target switching.");
            ui.horizontal(|ui| {
                ui.label("Source frame");
                ui.add(egui::DragValue::new(&mut self.target_seed_frame));
            });
            ui.horizontal(|ui| {
                for (index, label) in ["Normalized x", "Normalized y"].into_iter().enumerate() {
                    ui.label(label);
                    ui.add(
                        egui::DragValue::new(&mut self.target_seed_point[index])
                            .range(0.0..=0.999999),
                    );
                }
            });
            ui.checkbox(&mut self.target_seed_filter_class, "Require detector class");
            if self.target_seed_filter_class {
                ui.add(egui::DragValue::new(&mut self.target_seed_class));
            }
            if ui
                .add_enabled(
                    self.source.is_some(),
                    egui::Button::new("Bind target declaration to current source and view"),
                )
                .clicked()
            {
                match video_view_transform(self.video_view.as_ref()) {
                    Ok(transform) => {
                        self.target_seed_binding =
                            self.source.clone().map(|source| (source, transform));
                        self.status="Target declaration bound. Generation still requires a detector and one observed matching box.".into();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            if !self.use_model
                || self
                    .model_contract()
                    .is_none_or(|model| model.decoder == DetectorDecoder::TemporalScriptPosition)
            {
                ui.colored_label(Color32::YELLOW,"An indexed-video detector is required. Script-position and global-motion recipes cannot select an entity.");
            }
        }
        ui.checkbox(
            &mut self.use_mixed_audio,
            "Use audio alongside video motion",
        );
        if self.use_mixed_audio {
            ui.checkbox(
                &mut self.use_independent_audio,
                "Use a separately imported audio source",
            );
            if self.use_independent_audio {
                if let Some(project) = &self.project {
                    let selected = project
                        .sources
                        .iter()
                        .find(|source| {
                            Some(&source.source_version) == self.independent_audio_source.as_ref()
                        })
                        .map(|source| source.label.as_str())
                        .unwrap_or("Select imported audio media");
                    egui::ComboBox::from_id_salt("independent-audio-source")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for source in project.sources.iter().filter(|source| {
                                source.kind == SourceKind::Media
                                    && Some(&source.source_version) != self.source.as_ref()
                            }) {
                                ui.selectable_value(
                                    &mut self.independent_audio_source,
                                    Some(source.source_version.clone()),
                                    &source.label,
                                );
                            }
                        });
                }
                for (i, label) in [
                    "Video clock anchor (seconds)",
                    "Audio clock anchor (seconds)",
                ]
                .into_iter()
                .enumerate()
                {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        ui.add(egui::DragValue::new(&mut self.audio_alignment_anchors[i].0));
                        ui.label("/");
                        ui.add(
                            egui::DragValue::new(&mut self.audio_alignment_anchors[i].1)
                                .range(1..=u32::MAX),
                        );
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Audio seconds per video second");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.audio_clock_rate.0)
                            .char_limit(20)
                            .desired_width(100.0),
                    );
                    ui.label("/");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.audio_clock_rate.1)
                            .char_limit(10)
                            .desired_width(80.0),
                    );
                });
                ui.label("Declare corresponding anchors and a positive clock rate. Alignment is not measured; unsupported audio times remain gaps.");
            } else {
                ui.label(
                    "Audio uses the current video container and its original stream timestamps.",
                );
            }
            ui.checkbox(
                &mut self.creative_audio,
                "Creative: replace visual motion with audio RMS",
            );
            ui.label("Faithful preserves available visual samples. Audio assistance is synthesized; RMS maps to 0.5–0.75 without normalization.");
        }
        let fallback_binding = self.fallback_binding();
        self.fallback_panel.sync(fallback_binding);
        let fallback_supported = self.capabilities.as_ref().is_some_and(|capabilities| {
            capabilities
                .operations
                .iter()
                .any(|operation| operation == "generate_with_inference_fallback")
        });
        self.fallback_panel.ui(ui, self.busy, fallback_supported);
        if ui
            .add_enabled(
                !self.busy && self.has_media_source(),
                egui::Button::new("Generate stroke candidate"),
            )
            .clicked()
        {
            let selection = self.selected_model().and_then(|model| {
                let audio = if self.use_mixed_audio && self.use_independent_audio {
                    Some(independent_audio_selection(&self.project.as_ref().expect("media project").sources,self.source.as_ref(),self.independent_audio_source.as_ref(),self.audio_alignment_anchors,independent_audio_rate(&self.audio_clock_rate.0,&self.audio_clock_rate.1)?)?)
                } else {None};
                let target = if self.use_target_seed {
                    let transform=video_view_transform(self.video_view.as_ref())?;
                    if !self.target_seed_binding.as_ref().is_some_and(|(source,view)|Some(source)==self.source.as_ref() && view==&transform) {
                        return Err(ProtocolError::invalid("Bind the target declaration to the current source and view before generation").into());
                    }
                    Some(AnalysisTargetSeed {source_frame_index:self.target_seed_frame,x:self.target_seed_point[0],y:self.target_seed_point[1],class_id:self.target_seed_filter_class.then_some(self.target_seed_class)})
                } else {None};Ok((model,audio,target))
            });
            match selection {
                Ok((model_path, independent, target_seed)) => {
                    let project_interval = if self.use_generation_interval {
                        match TimeRange::new(
                            ProjectTime::from_nanos(self.generation_interval_ns[0]),
                            ProjectTime::from_nanos(self.generation_interval_ns[1]),
                        ) {
                            Ok(interval) => Some(interval),
                            Err(error) => {
                                self.status = error.to_string();
                                return;
                            }
                        }
                    } else {
                        None
                    };
                    let settings = GenerationSettings {
                        detector_score_threshold: self.generation_detector_score_threshold(),
                        annotated_view: self.annotated_view.clone(),
                        project_interval,
                        target_seed,
                        analysis_width: self.generation_analysis_dimensions().0,
                        analysis_height: self.generation_analysis_dimensions().1,
                        mixed_audio: self.use_mixed_audio.then(|| MixedAudioSettings {
                            independent,
                            intent: if self.creative_audio {
                                MultimodalIntent::CreativeAudio
                            } else {
                                MultimodalIntent::Faithful
                            },
                            mapping: AudioMapping::new(
                                Axis::Stroke,
                                0.25,
                                NormalizedPosition::new(0.5).expect("constant"),
                            )
                            .expect("constant"),
                        }),
                        source_placement: self
                            .project
                            .as_ref()
                            .and_then(|p| {
                                p.placements.iter().find(|p| {
                                    Some(&p.placement_id) == self.selected_placement.as_ref()
                                })
                            })
                            .cloned(),
                        video_view: self.video_view.clone(),
                        inference_profile: self.generation_profile(),
                        model_input: self.model_contract(),
                        ..Default::default()
                    };
                    match settings.validate_execution_preset(&self.preset, self.use_model) {
                        Ok(()) => {
                            let command = if self.use_model && self.use_selected_pack {
                                match self.pack_panel.generation_command(
                                    self.source.clone().unwrap(),
                                    self.preset.clone(),
                                    settings,
                                ) {
                                    Ok(command) => command,
                                    Err(error) => {
                                        self.status = error.to_string();
                                        return;
                                    }
                                }
                            } else {
                                Command::Generate {
                                    source_version: self.source.clone().unwrap(),
                                    preset: self.preset.clone(),
                                    settings: Some(settings),
                                    model_path,
                                }
                            };
                            if self.fallback_panel.enabled() {
                                match self.fallback_panel.prepare(command) {
                                    Ok(intent) => {
                                        let binding = intent.binding().clone();
                                        if self.enqueue(
                                            RpcCommand::FallbackGeneration(Box::new(intent)),
                                            Some(binding.project),
                                            Some(binding.revision),
                                        ) {
                                            self.fallback_panel.submitted();
                                            self.status = "Reading the selected policy and submitting the captured generation request.".into();
                                        }
                                    }
                                    Err(error) => self.status = error.to_string(),
                                }
                            } else {
                                self.project_command(command, true);
                            }
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
    }

    pub(super) fn video_view_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.video_view.clone();
        crate::video_view_panel::show(ui, &mut self.video_view);
        if before != self.video_view {
            self.seek(self.playhead_seconds);
        }
    }

    pub(super) fn candidate_controls(
        &mut self,
        ui: &mut egui::Ui,
        candidate: &CandidateSnapshot,
        stale: bool,
    ) {
        ui.label(format!(
            "Candidate {} / base revision {}",
            candidate.candidate_id,
            candidate.base_revision.value()
        ));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Verify image generation origin"),
            )
            .clicked()
        {
            self.candidate_image_origin = None;
            self.candidate_image_pixels = None;
            self.pending_image_record = None;
            self.project_command(
                Command::CandidateImageOriginStatus {
                    candidate_id: candidate.candidate_id.clone(),
                },
                false,
            );
        }
        let mut show_image = None;
        if let Some(report) = self
            .candidate_image_origin
            .as_ref()
            .filter(|r| image_origin_scope_matches(r, candidate))
        {
            ui.label("Verified image-conditioned synthesis. The retained approval grants no new execution authority and does not establish observed temporal motion or model qualification.");
            for origin in &report.origins {
                let image = origin
                    .approval
                    .report
                    .image
                    .as_ref()
                    .expect("validated image origin");
                ui.label(format!(
                    "Original project {} / revision {} / candidate {}",
                    origin.approval.report.project_id,
                    origin.approval.report.base_revision.value(),
                    origin.candidate_id
                ));
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Show conditioning image"))
                    .clicked()
                {
                    show_image = Some(origin.record.sha256.clone());
                }
                ui.monospace(format!(
                    "Source SHA-256: {}\nPixels SHA-256: {}\nApproval record SHA-256: {}",
                    image.source.sha256, image.rgb.sha256, origin.record.sha256
                ));
                if let Some(AssistantProposal::SynthesizePatterns { patterns }) =
                    &origin.approval.report.proposal
                {
                    ui.push_id(&origin.record.sha256, |ui| {
                        ui.collapsing("Review retained generation patterns", |ui| {
                            ui.small("These are the historical approved patterns. Later candidate or master edits remain separate.");
                            self.pattern_reviews.show(ui, patterns);
                        });
                    });
                }
            }
            ui.collapsing("Retained image approval and generation details", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(report).unwrap_or_else(|e| e.to_string()),
                );
            });
        }
        if let Some(record_sha256) = show_image {
            self.pending_image_record = Some(record_sha256.clone());
            self.candidate_image_pixels = None;
            self.project_command(
                Command::CandidateImageOriginPixels {
                    candidate_id: candidate.candidate_id.clone(),
                    record_sha256,
                },
                false,
            );
        }
        if let Some((pixels, texture)) =
            self.candidate_image_pixels.as_ref().filter(|(pixels, _)| {
                image_origin_scope_matches(&pixels.report, candidate)
                    && self
                        .candidate_image_origin
                        .as_ref()
                        .is_some_and(|r| r.origins.iter().any(|o| o.record == pixels.record))
            })
        {
            ui.label(format!(
                "Retained normalized conditioning image: {} × {}",
                pixels.frame.width, pixels.frame.height
            ));
            let size = texture.size_vec2();
            let scale = (ui.available_width().min(320.0) / size.x)
                .min(320.0 / size.y)
                .min(1.0);
            ui.image((texture.id(), size * scale));
            ui.small("These are the stored pixels supplied to the image-conditioned proposal. A still image does not establish temporal motion.");
        }
        if candidate.job_id.is_none()
            && ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Verify archived generation origin"),
                )
                .clicked()
        {
            self.archived_generation = None;
            self.project_command(
                Command::ArchivedGenerationStatus {
                    candidate_id: candidate.candidate_id.clone(),
                    master_revision: None,
                },
                false,
            );
        }
        if candidate.job_id.is_none()
            && ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Verify current master ancestry"),
                )
                .clicked()
        {
            self.archived_generation = None;
            let master_revision = self.project.as_ref().map(|p| p.revision);
            self.project_command(
                Command::ArchivedGenerationStatus {
                    candidate_id: candidate.candidate_id.clone(),
                    master_revision,
                },
                false,
            );
        }
        if let Some(report) = self.archived_generation.as_ref().filter(|r| {
            r.project_id == candidate.project_id
                && r.candidate_id == candidate.candidate_id
                && r.master_derivation.as_ref().is_none_or(|m| {
                    self.project.as_ref().is_some_and(|p| {
                        p.revision == m.revision && p.motion.program.sha256 == m.master.sha256
                    })
                })
        }) {
            ui.label(format!("{} verified historical generation anchors. No live job or publication authority transfers.",report.origins.len()));
            if let Some(master) = &report.master_derivation {
                ui.label(format!("Current master revision {} has checked ancestry and {} recorded retiming steps. Publication still requires fresh source/destination authority and clock proof migration.",master.revision.value(),master.retiming_revisions.len()));
            }
            ui.collapsing("Historical generation details", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(report).unwrap_or_else(|e| e.to_string()),
                );
            });
        }
        ui.label("Amber curve is proposed motion; review bands identify localized uncertainty. It is not committed.");
        let rows = self.candidate_evidence.rows(&candidate.program);
        if rows.iter().any(|row| row.synthesized > 0) {
            ui.label("This candidate contains synthesized actions. Those actions do not establish observed temporal motion.");
        }
        if rows.iter().any(|row| row.gaps > 0) {
            ui.label(
                "Unresolved evidence gaps remain. Standard export requires explicit resolution.",
            );
        }
        ui.collapsing("Motion evidence by axis", |ui| {
            ui.small("Counts describe action labels and gap intervals, not durations or model qualification. These labels alone do not identify an image origin.");
            if rows.is_empty() {
                ui.label("No motion axes supplied.");
            } else {
                egui::Grid::new("candidate-motion-evidence").striped(true).show(ui, |ui| {
                    for heading in ["Axis", "Observed", "Predicted", "Inferred", "Synthesized", "Gaps"] {
                        ui.label(heading);
                    }
                    ui.end_row();
                    for row in rows {
                        ui.label(row.axis.as_str());
                        for count in [row.observed, row.predicted, row.inferred, row.synthesized, row.gaps] {
                            ui.label(count.to_string());
                        }
                        ui.end_row();
                    }
                });
            }
        });
        egui::ScrollArea::vertical()
            .max_height(140.0)
            .id_salt("candidate-review-flags")
            .show(ui, |ui| {
                for flag in candidate.review.iter().take(256) {
                    ui.label(format!(
                        "{} / {:?} / {:.3}..{:.3}s / {:?}",
                        flag.reason.as_str(),
                        flag.axis,
                        flag.range.start().as_nanos() as f64 / 1e9,
                        flag.range.end().as_nanos() as f64 / 1e9,
                        flag.evidence
                    ));
                    if let Some(confidence) = &flag.confidence {
                        ui.small(format!(
                            "Confidence {:.3} ({:?})",
                            confidence.value(),
                            confidence.calibration()
                        ));
                    }
                }
                if candidate.review.len() > 256 {
                    ui.label(format!(
                        "{} additional flags retained by the engine.",
                        candidate.review.len() - 256
                    ));
                }
                if candidate.review.is_empty() {
                    ui.label("No flags supplied. This is not a guarantee of correct motion.");
                }
            });
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Inspect candidate and protected regions"),
            )
            .clicked()
        {
            self.project_command(
                Command::Diagnostics {
                    candidate_id: Some(candidate.candidate_id.clone()),
                    maximum_speed_percent_per_second: Some(self.segment_speed_limit),
                },
                false,
            );
        }
        if stale {
            ui.colored_label(
                Color32::YELLOW,
                "Project changed. Explicit rebase and fresh review are required.",
            );
            if ui
                .add_enabled(!self.busy, egui::Button::new("Explicitly rebase candidate"))
                .clicked()
            {
                self.project_command(
                    Command::RebaseCandidate {
                        candidate_id: candidate.candidate_id.clone(),
                    },
                    true,
                );
            }
        }
        let candidate_axes: Vec<_> = candidate
            .program
            .tracks()
            .iter()
            .map(|track| track.axis())
            .collect();
        ui.horizontal_wrapped(|ui| {
            for &axis in &candidate_axes {
                let mut selected = self.merge_axes.contains(&axis);
                if ui.checkbox(&mut selected, format!("{axis:?}")).changed() {
                    if selected {
                        self.merge_axes.push(axis);
                    } else {
                        self.merge_axes.retain(|value| *value != axis);
                    }
                }
            }
        });
        let selected_axes: Vec<_> = candidate_axes
            .iter()
            .copied()
            .filter(|axis| self.merge_axes.contains(axis))
            .collect();
        ui.checkbox(
            &mut self.merge_range_enabled,
            "Merge only a project-time range",
        );
        if self.merge_range_enabled {
            ui.horizontal(|ui| {
                ui.label("Start");
                ui.add(
                    egui::DragValue::new(&mut self.merge_start_seconds)
                        .range(0.0..=86_400.0)
                        .suffix(" s"),
                );
                ui.label("End");
                ui.add(
                    egui::DragValue::new(&mut self.merge_end_seconds)
                        .range(0.0..=86_400.0)
                        .suffix(" s"),
                );
            });
        }
        if ui
            .add_enabled(
                !self.busy && !stale && !selected_axes.is_empty(),
                egui::Button::new("Merge selected axes / preserve protection"),
            )
            .clicked()
        {
            let range = if self.merge_range_enabled {
                crate::modality::range(Some(self.merge_start_seconds), Some(self.merge_end_seconds))
            } else {
                Ok(candidate.project_interval)
            };
            match range {
                Ok(range) => {
                    self.project_command(
                        Command::MergeCandidate {
                            candidate_id: candidate.candidate_id.clone(),
                            axes: selected_axes,
                            range,
                        },
                        true,
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && !stale && !candidate_axes.is_empty(),
                egui::Button::new("Apply all candidate axes / preserve protected edits"),
            )
            .clicked()
        {
            self.project_command(
                Command::MergeCandidate {
                    candidate_id: candidate.candidate_id.clone(),
                    axes: candidate_axes,
                    range: candidate.project_interval,
                },
                true,
            );
        }
        if let Some(interval) = candidate.project_interval {
            ui.label(format!("Native interval candidate: {}..{} ns. Whole replacement of an existing master is refused; applying uses this retained range.",interval.start().as_nanos(),interval.end().as_nanos()));
        }
        if ui
            .add_enabled(
                !self.busy
                    && !stale
                    && (candidate.project_interval.is_none()
                        || self
                            .project
                            .as_ref()
                            .is_some_and(|p| p.program.tracks().is_empty())),
                egui::Button::new("Commit whole reviewed candidate"),
            )
            .clicked()
        {
            self.project_command(
                Command::CommitCandidate {
                    candidate_id: candidate.candidate_id.clone(),
                },
                true,
            );
        }
        if ui.button("Dismiss candidate preview").clicked() {
            self.candidate = None;
            self.diagnostics = None;
        }
        self.diagnostics_view(ui);
    }
}
