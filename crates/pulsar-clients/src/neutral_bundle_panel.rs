//! Desktop presentation for the shared atomic exporter; no file or Engine effects.
use crate::{
    neutral_bundle::PublishedNeutralBundle,
    portable::{PortableAction, PortableFailure, PortablePublicationFailure},
    project_package::PackagePublicationOutcome,
};
use eframe::egui;
use pulsar_protocol::*;
use std::path::{Path, PathBuf};

pub(crate) struct NeutralBundleView {
    axes: Vec<Axis>,
    pub(crate) destination: String,
    pub(crate) staging: String,
    target: Option<(ProjectId, RevisionId)>,
    pub(crate) published: Option<PublishedNeutralBundle>,
    failure: Option<PortablePublicationFailure>,
    disposition: bool,
}
impl Default for NeutralBundleView {
    fn default() -> Self {
        Self {
            axes: Axis::ALL.to_vec(),
            destination: String::new(),
            staging: String::new(),
            target: None,
            published: None,
            failure: None,
            disposition: false,
        }
    }
}
impl NeutralBundleView {
    pub(crate) fn failure(&mut self, failure: &PortableFailure) {
        self.published = None;
        self.failure = failure.publication.clone();
        self.disposition = false;
        if let Some(path) = self
            .failure
            .as_ref()
            .and_then(|failure| failure.recovery_staging_path.as_ref())
        {
            self.staging = path.to_string_lossy().into_owned();
        }
    }
    pub(crate) fn published(&mut self, receipt: PublishedNeutralBundle) {
        self.published = Some(receipt);
        self.failure = None;
        self.disposition = false;
        self.staging.clear();
    }
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        current: Option<&(ProjectId, RevisionId)>,
        idle: bool,
    ) -> Option<PortableAction> {
        if self.target.as_ref() != current {
            self.target = current.cloned();
            self.published = None;
        }
        ui.heading("Export neutral axes");
        ui.label("Publish the selected committed tracks into one new private directory. No device adaptation, source media or full project package is included.");
        ui.small("The directory includes hidden recovery metadata. Atomic export is unavailable on unsupported platforms.");
        if let Some((project, revision)) = current {
            ui.label(format!("Project {project}, revision {revision:?}"));
        }
        ui.label("New bundle directory");
        if ui
            .add(egui::TextEdit::singleline(&mut self.destination).char_limit(4096))
            .changed()
        {
            self.published = None;
        }
        ui.label("Retained bundle staging directory");
        ui.add(egui::TextEdit::singleline(&mut self.staging).char_limit(4096));
        let uncertain = self
            .failure
            .as_ref()
            .is_some_and(|failure| failure.outcome != PackagePublicationOutcome::NotPublished);
        if let Some(failure) = &self.failure {
            ui.label(format!(
                "Publication outcome: {:?}; destination {}",
                failure.outcome,
                failure.destination.display()
            ));
            ui.small("Inspect both paths. Do not overwrite or delete possible published files. Unknown Engine dispatches remain refused during exact-intent resume.");
        }
        if uncertain {
            ui.checkbox(
                &mut self.disposition,
                "I have inspected and handled the uncertain publication outcome",
            );
            if ui
                .add_enabled(
                    idle && self.disposition,
                    egui::Button::new("Record bundle recovery disposition"),
                )
                .clicked()
            {
                self.failure = None;
                self.disposition = false;
            }
        }
        let allowed = idle && !uncertain;
        ui.add_enabled_ui(allowed, |ui| {
            ui.horizontal_wrapped(|ui| {
                for axis in Axis::ALL {
                    let mut selected = self.axes.contains(&axis);
                    if ui.checkbox(&mut selected, format!("{axis:?}")).changed() {
                        self.axes.retain(|value| *value != axis);
                        if selected {
                            self.axes.push(axis);
                        }
                        self.axes = Axis::ALL
                            .into_iter()
                            .filter(|value| self.axes.contains(value))
                            .collect();
                        self.published = None;
                    }
                }
            });
        });
        ui.small("Every selected axis must have a committed track. No missing axis is fabricated. Resume uses the original retained selection.");
        let mut action = None;
        if ui
            .add_enabled(
                allowed
                    && !self.axes.is_empty()
                    && current.is_some()
                    && Path::new(&self.destination).is_absolute(),
                egui::Button::new("Export selected-axis bundle"),
            )
            .clicked()
        {
            if let Some((project, revision)) = current {
                self.published = None;
                action = Some(PortableAction::ExportNeutralBundle {
                    project_id: project.clone(),
                    revision: *revision,
                    destination: PathBuf::from(&self.destination),
                    axes: self.axes.clone(),
                });
            }
        }
        if ui
            .add_enabled(
                allowed && Path::new(&self.staging).is_absolute(),
                egui::Button::new("Resume retained axis bundle"),
            )
            .clicked()
        {
            self.published = None;
            action = Some(PortableAction::ResumeNeutralBundle {
                staging: PathBuf::from(&self.staging),
            });
        }
        ui.small("Stop local transfer retains recovery. Verification and final rename have no byte-progress estimate. Master motion remains unchanged.");
        if let Some(receipt) = &self.published {
            ui.label(format!("Published six axes for project {} at revision {:?}: {}. File/directory sync confirmed; quality unqualified.",receipt.project,receipt.revision,receipt.destination.display()));
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn click(
        view: &mut NeutralBundleView,
        target: Option<&(ProjectId, RevisionId)>,
        idle: bool,
        label: &str,
    ) -> Option<PortableAction> {
        let ctx = egui::Context::default();
        let mut action = None;
        let mut draw = |events| {
            ctx.run(
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
                        action = view.show(ui, target, idle);
                    });
                },
            )
        };
        fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                    Some(s.pos + s.galley.rect.center().to_vec2())
                }
                egui::epaint::Shape::Vec(s) => s.iter().find_map(|s| text(s, label)),
                _ => None,
            }
        }
        draw(vec![]);
        let output = draw(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, label))
            .unwrap();
        for pressed in [true, false] {
            draw(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        action
    }
    #[test]
    fn drawn_export_captures_revision_and_busy_missing_relative_inputs_are_inert() {
        let target = (ProjectId::new("project").unwrap(), RevisionId::new(4));
        let mut view = NeutralBundleView {
            destination: "relative".into(),
            ..Default::default()
        };
        assert!(click(
            &mut view,
            Some(&target),
            true,
            "Export selected-axis bundle"
        )
        .is_none());
        view.destination = crate::test_host_path("output");
        assert!(click(&mut view, None, true, "Export selected-axis bundle").is_none());
        assert!(click(
            &mut view,
            Some(&target),
            false,
            "Export selected-axis bundle"
        )
        .is_none());
        let action = click(
            &mut view,
            Some(&target),
            true,
            "Export selected-axis bundle",
        )
        .unwrap();
        view.destination = crate::test_host_path("changed");
        assert!(
            matches!(action,PortableAction::ExportNeutralBundle {project_id,revision,destination,axes} if project_id==target.0 && revision==target.1 && destination==Path::new(&crate::test_host_path("output")) && axes==Axis::ALL)
        );
    }
    #[test]
    fn drawn_selected_export_freezes_axes_and_empty_selection_cannot_dispatch() {
        let target = (ProjectId::new("project").unwrap(), RevisionId::new(4));
        let mut view = NeutralBundleView {
            destination: crate::test_host_path("output"),
            axes: vec![Axis::Stroke],
            ..Default::default()
        };
        let action = click(
            &mut view,
            Some(&target),
            true,
            "Export selected-axis bundle",
        )
        .unwrap();
        view.axes.clear();
        assert!(
            matches!(action, PortableAction::ExportNeutralBundle { axes, .. } if axes == vec![Axis::Stroke])
        );
        assert!(click(
            &mut view,
            Some(&target),
            true,
            "Export selected-axis bundle"
        )
        .is_none());
    }
    #[test]
    fn uncertain_outcome_retains_paths_and_blocks_resume_and_new_export() {
        let mut view = NeutralBundleView {
            destination: crate::test_host_path("output"),
            ..Default::default()
        };
        let target = (ProjectId::new("project").unwrap(), RevisionId::new(4));
        let failure = PortableFailure {
            message: "unknown".into(),
            protocol_error: None,
            publication: Some(PortablePublicationFailure {
                destination: PathBuf::from(crate::test_host_path("output")),
                outcome: PackagePublicationOutcome::OutcomeUnknown,
                recovery_staging_path: Some(PathBuf::from(crate::test_host_path("stage"))),
            }),
        };
        view.failure(&failure);
        assert!(click(
            &mut view,
            Some(&target),
            true,
            "Export selected-axis bundle"
        )
        .is_none());
        assert!(click(&mut view, None, true, "Resume retained axis bundle").is_none());
        assert_eq!(view.staging, crate::test_host_path("stage"));
        view.failure.as_mut().unwrap().outcome = PackagePublicationOutcome::NotPublished;
        assert!(matches!(
            click(&mut view, None, true, "Resume retained axis bundle"),
            Some(PortableAction::ResumeNeutralBundle { .. })
        ));
    }
}
