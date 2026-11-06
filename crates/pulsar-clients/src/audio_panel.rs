//! Audio authoring controls; every request uses checked shared Protocol types.
use eframe::egui;
use pulsar_protocol::*;

#[derive(Clone)]
struct ChannelDraft {
    axis: Axis,
    mode: AudioMode,
    band: AudioBand,
    threshold: f64,
    amplitude: f64,
    offset: f64,
}
impl ChannelDraft {
    fn new(axis: Axis) -> Self {
        Self {
            axis,
            mode: AudioMode::Envelope,
            band: AudioBand::Full,
            threshold: AUDIO_ONSET_THRESHOLD,
            amplitude: 0.5,
            offset: 0.25,
        }
    }
    fn checked(&self) -> Result<AudioChannelRecipe, ProtocolError> {
        Ok(AudioChannelRecipe {
            mapping: AudioMapping::new(
                self.axis,
                self.amplitude,
                NormalizedPosition::new(self.offset)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?,
            )
            .map_err(|e| ProtocolError::invalid(e.to_string()))?,
            mode: self.mode,
            analysis: AudioAnalysisOptions::new(self.band, self.threshold)
                .map_err(|e| ProtocolError::invalid(e.to_string()))?,
        })
    }
    fn ui(&mut self, ui: &mut egui::Ui, fixed_axis: bool) {
        if !fixed_axis {
            egui::ComboBox::from_label("Output axis")
                .selected_text(format!("{:?}", self.axis))
                .show_ui(ui, |ui| {
                    for axis in Axis::ALL {
                        ui.selectable_value(&mut self.axis, axis, format!("{axis:?}"));
                    }
                });
        }
        egui::ComboBox::from_label("Audio mode")
            .selected_text(format!("{:?}", self.mode))
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    (AudioMode::Envelope, "Energy envelope"),
                    (AudioMode::Beats, "Amplitude-onset pulses"),
                    (AudioMode::Vibe, "8 Hz energy-modulated oscillation"),
                ] {
                    ui.selectable_value(&mut self.mode, mode, label);
                }
            });
        egui::ComboBox::from_label("Audio band")
            .selected_text(format!("{:?}", self.band))
            .show_ui(ui, |ui| {
                for band in [
                    AudioBand::Full,
                    AudioBand::Bass,
                    AudioBand::Mid,
                    AudioBand::High,
                ] {
                    ui.selectable_value(&mut self.band, band, format!("{band:?}"));
                }
            });
        ui.add(egui::Slider::new(&mut self.threshold, 0.05..=0.9).text("RMS threshold"));
        ui.add(egui::Slider::new(&mut self.amplitude, 0.0..=1.0).text("Amplitude"));
        ui.add(egui::Slider::new(&mut self.offset, 0.0..=1.0).text("Offset"));
        if let Err(error) = self.checked() {
            ui.colored_label(egui::Color32::YELLOW, error.to_string());
        }
    }
}

pub struct AudioPanel {
    bundle: bool,
    single: ChannelDraft,
    channels: [ChannelDraft; 6],
}
impl Default for AudioPanel {
    fn default() -> Self {
        Self {
            bundle: false,
            single: ChannelDraft::new(Axis::Stroke),
            channels: Axis::ALL.map(ChannelDraft::new),
        }
    }
}
impl AudioPanel {
    /// Checked fixture/import initialization uses the same drafts as the controls.
    /// It does not issue an Engine command or approve a candidate.
    pub fn set_bundle_recipe(&mut self, recipe: &AudioBundleRecipe) {
        for channel in recipe.channels() {
            let draft = self
                .channels
                .iter_mut()
                .find(|draft| draft.axis == channel.mapping.axis())
                .expect("checked recipe covers fixed neutral axes");
            draft.mode = channel.mode;
            draft.band = channel.analysis.band();
            draft.threshold = channel.analysis.threshold();
            draft.amplitude = channel.mapping.amplitude();
            draft.offset = channel.mapping.offset().value();
        }
        self.bundle = true;
    }
    /// Snapshot checked fixed-axis drafts without dispatching or writing a recipe file.
    pub fn bundle_recipe(&self) -> Result<AudioBundleRecipe, ProtocolError> {
        AudioBundleRecipe::new(
            self.channels
                .iter()
                .map(ChannelDraft::checked)
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    pub(crate) fn bundle_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(
            "Each axis has its own synthesized mapping. Review all six settings before generating.",
        );
        for channel in &mut self.channels {
            egui::CollapsingHeader::new(format!("{:?}", channel.axis))
                .id_salt(("audio-bundle", channel.axis))
                .show(ui, |ui| {
                    ui.push_id(channel.axis, |ui| channel.ui(ui, true));
                });
        }
        ui.small("All bundle channels explicitly gate quiet windows, including Full band. These mappings do not measure physical axis motion.");
        ui.small("Offset + amplitude must not exceed 1. Onsets are RMS crossings, not musical beats. Causal filter delay is uncompensated.");
        if let Err(error) = self.bundle_recipe() {
            ui.colored_label(egui::Color32::YELLOW, error.to_string());
        }
    }
    pub fn input(&self, source_version: SourceVersionId) -> Result<GenerationInput, ProtocolError> {
        if self.bundle {
            Ok(GenerationInput::AudioBundle {
                source_version,
                recipe: self.bundle_recipe()?,
            })
        } else {
            let channel = self.single.checked()?;
            let analysis = (self.single.band != AudioBand::Full
                || self.single.threshold != AUDIO_ONSET_THRESHOLD)
                .then_some(channel.analysis);
            Ok(GenerationInput::Audio {
                source_version,
                mapping: channel.mapping,
                mode: channel.mode,
                analysis,
            })
        }
    }
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        source: Option<SourceVersionId>,
        enabled: bool,
    ) -> Option<Result<GenerationInput, ProtocolError>> {
        let mut input = None;
        ui.label("Audio-derived mapping");
        ui.add_enabled_ui(enabled, |ui| {
            ui.checkbox(&mut self.bundle, "Six explicit neutral axes");
            if self.bundle {
                self.bundle_controls(ui);
            } else {
                self.single.ui(ui, false);
                ui.small("Band analysis gates quiet windows. Full/default keeps the original ungated envelope.");
            }
            ui.small("Envelope: offset + amplitude * level. Oscillation centers in that range and holds its center in silence. Offset + amplitude must not exceed 1.");
            ui.small("Onsets are RMS crossings, not musical beats. Causal filter delay is uncompensated.");
            let valid = source.as_ref().is_some_and(|source| self.input(source.clone()).is_ok());
            if ui.add_enabled(valid, egui::Button::new(if self.bundle { "Generate six-axis audio candidate" } else { "Generate audio candidate" })).clicked() {
                if let Some(source) = source { input = Some(self.input(source)); }
            }
        });
        ui.colored_label(egui::Color32::YELLOW, "Audio mappings are synthesized and unqualified. Review and commit the candidate separately.");
        input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> SourceVersionId {
        SourceVersionId::new("authored-source").unwrap()
    }
    #[test]
    fn default_single_axis_retains_legacy_ungated_contract_and_bundle_keeps_independent_drafts() {
        let mut panel = AudioPanel::default();
        let GenerationInput::Audio {
            mapping,
            analysis,
            mode,
            ..
        } = panel.input(source()).unwrap()
        else {
            panic!("single")
        };
        assert!(analysis.is_none());
        assert_eq!(mode, AudioMode::Envelope);
        assert_eq!(mapping.axis(), Axis::Stroke);
        assert_eq!(mapping.amplitude(), 0.5);
        assert_eq!(mapping.offset().value(), 0.25);
        panel.bundle = true;
        panel.channels[1].mode = AudioMode::Vibe;
        panel.channels[1].band = AudioBand::High;
        let GenerationInput::AudioBundle {
            source_version,
            recipe,
        } = panel.input(source()).unwrap()
        else {
            panic!("bundle")
        };
        assert_eq!(source_version, source());
        assert_eq!(recipe.channels().len(), 6);
        assert_eq!(recipe.channels()[1].mode, AudioMode::Vibe);
        assert_eq!(recipe.channels()[1].analysis.band(), AudioBand::High);
        panel.bundle = false;
        assert!(matches!(
            panel.input(source()).unwrap(),
            GenerationInput::Audio {
                analysis: None,
                mode: AudioMode::Envelope,
                ..
            }
        ));
    }
    #[test]
    fn invalid_axis_draft_refuses_entire_bundle_before_request_creation() {
        let mut panel = AudioPanel {
            bundle: true,
            ..Default::default()
        };
        for value in [f64::NAN, -0.1, 1.0] {
            panel.channels[5].offset = value;
            assert!(panel.input(source()).is_err());
        }
        panel.channels[5].offset = 0.25;
        panel.channels[5].threshold = 0.01;
        assert!(panel.input(source()).is_err());
    }
    #[test]
    fn headless_bundle_controls_render_and_disabled_or_missing_source_cannot_emit_requests() {
        let ctx = egui::Context::default();
        let mut panel = AudioPanel {
            bundle: true,
            ..Default::default()
        };
        for (source, enabled) in [
            (None, true),
            (Some(source()), false),
            (Some(source()), true),
        ] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(panel.ui(ui, source.clone(), enabled).is_none());
                });
            });
        }
    }
    fn draw(
        ctx: &egui::Context,
        panel: &mut AudioPanel,
        source: Option<SourceVersionId>,
        enabled: bool,
        events: Vec<egui::Event>,
    ) -> (
        egui::FullOutput,
        Option<Result<GenerationInput, ProtocolError>>,
    ) {
        let mut request = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    request = panel.ui(ui, source.clone(), enabled);
                });
            },
        );
        (output, request)
    }
    fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(shape) if shape.galley.job.text == label => {
                Some(shape.pos + shape.galley.rect.center().to_vec2())
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text(shape, label)),
            _ => None,
        }
    }
    fn click(
        ctx: &egui::Context,
        panel: &mut AudioPanel,
        source: Option<SourceVersionId>,
        enabled: bool,
    ) -> Option<Result<GenerationInput, ProtocolError>> {
        draw(ctx, panel, source.clone(), enabled, vec![]);
        let (output, _) = draw(ctx, panel, source.clone(), enabled, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| text(&shape.shape, "Generate six-axis audio candidate"))
            .expect("drawn audio button");
        let mut result = None;
        for pressed in [true, false] {
            let (_, request) = draw(
                ctx,
                panel,
                source.clone(),
                enabled,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
            result = result.or(request);
        }
        result
    }
    #[test]
    fn drawn_bundle_button_captures_checked_settings_and_inert_states_emit_nothing() {
        let ctx = egui::Context::default();
        let mut panel = AudioPanel {
            bundle: true,
            ..Default::default()
        };
        assert!(click(&ctx, &mut panel, None, true).is_none());
        assert!(click(&ctx, &mut panel, Some(source()), false).is_none());
        panel.channels[4].amplitude = 1.0;
        assert!(click(&ctx, &mut panel, Some(source()), true).is_none());
        panel.channels[4].amplitude = 0.2;
        let input = click(&ctx, &mut panel, Some(source()), true)
            .unwrap()
            .unwrap();
        panel.channels[4].amplitude = 0.6;
        let GenerationInput::AudioBundle { recipe: edited, .. } = panel.input(source()).unwrap()
        else {
            panic!("edited bundle")
        };
        assert_eq!(edited.channels()[4].mapping.amplitude(), 0.6);
        let GenerationInput::AudioBundle {
            source_version,
            recipe,
        } = input
        else {
            panic!("bundle")
        };
        assert_eq!(source_version, source());
        assert_eq!(recipe.channels()[4].mapping.amplitude(), 0.2);
        assert_eq!(recipe.channels().len(), 6);
    }
    #[test]
    fn direct_recipe_snapshot_matches_single_file_bundle_and_is_independent_of_later_edits() {
        let mut panel = AudioPanel {
            bundle: true,
            ..Default::default()
        };
        panel.channels[2].band = AudioBand::Mid;
        panel.channels[4].mode = AudioMode::Beats;
        let snapshot = panel.bundle_recipe().unwrap();
        let GenerationInput::AudioBundle { recipe, .. } = panel.input(source()).unwrap() else {
            panic!("bundle")
        };
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::to_value(recipe).unwrap()
        );
        panel.channels[2].amplitude = f64::NAN;
        assert!(panel.bundle_recipe().is_err());
        assert_eq!(snapshot.channels()[2].analysis.band(), AudioBand::Mid);
        assert!(snapshot
            .channels()
            .iter()
            .all(|c| c.mapping.amplitude().is_finite()));
    }
    #[test]
    fn imported_recipe_order_is_canonicalized_without_changing_any_axis_settings() {
        let mut original = AudioPanel::default();
        for (i, channel) in original.channels.iter_mut().enumerate() {
            channel.amplitude = i as f64 * 0.03;
            channel.offset = 0.1;
        }
        let mut channels = original.bundle_recipe().unwrap().channels().to_vec();
        channels.reverse();
        let reversed = AudioBundleRecipe::new(channels).unwrap();
        let mut imported = AudioPanel::default();
        imported.set_bundle_recipe(&reversed);
        let canonical = imported.bundle_recipe().unwrap();
        for (axis, channel) in Axis::ALL.into_iter().zip(canonical.channels()) {
            assert_eq!(axis, channel.mapping.axis());
            let expected = reversed
                .channels()
                .iter()
                .find(|c| c.mapping.axis() == axis)
                .unwrap();
            assert_eq!(
                serde_json::to_value(channel).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
    }
}
