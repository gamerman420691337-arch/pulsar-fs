//! Non-authoritative repair controls and captured uncommitted proposals.
use crate::{motion::CandidateSnapshot, session::EngineApi};
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::{path::PathBuf, sync::Arc};

pub struct RepairRequest {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub program: Arc<MotionProgram>,
    pub options: ScriptRepairOptions,
}

/// Runs on the caller's background request thread. Never commits or exports.
pub fn propose(client: &mut impl EngineApi, request: RepairRequest) -> Result<CandidateSnapshot> {
    let repair = repair_standard_script(&request.program, request.options)?;
    let expected = repair
        .program
        .track(Axis::Stroke)
        .ok_or_else(|| anyhow::anyhow!("Core repair returned no stroke"))?;
    let label = format!("neutral-script-repair-v1 {:?}", request.options);
    let candidate = client.upload_edit(
        request.project.clone(),
        request.revision,
        &repair.program,
        &label,
    )?;
    let matches = candidate.project_id == request.project
        && candidate.base_revision == request.revision
        && candidate.job_id.is_none()
        && candidate.program.tracks().len() == 1
        && candidate.program.track(Axis::Stroke).is_some_and(|track| {
            track.gaps() == expected.gaps()
                && track.actions().len() == expected.actions().len()
                && track
                    .actions()
                    .iter()
                    .zip(expected.actions())
                    .all(|(a, b)| a.time() == b.time() && a.position() == b.position())
        });
    ensure!(matches, "Repair upload returned mismatched scope/base/values for candidate {}; inspect the captured project before further action", candidate.candidate_id);
    Ok(candidate)
}

pub enum RepairAction {
    Import(PathBuf),
    Propose(ScriptRepairOptions),
}

pub(crate) struct RepairPanel {
    maximum: String,
    jitter_window: String,
    jitter_threshold: String,
}
impl Default for RepairPanel {
    fn default() -> Self {
        Self {
            maximum: "450".into(),
            jitter_window: "20".into(),
            jitter_threshold: "2".into(),
        }
    }
}
impl RepairPanel {
    fn options(&self) -> Result<ScriptRepairOptions> {
        Ok(ScriptRepairOptions::new(
            self.maximum.parse()?,
            self.jitter_window.parse()?,
            self.jitter_threshold.parse()?,
        )?)
    }
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        path: &mut String,
        project: bool,
        master: bool,
        busy: bool,
    ) -> Option<RepairAction> {
        let mut action = None;
        ui.heading("Neutral script repair");
        ui.label("Import sanitation proposes a candidate. Review and commit that baseline before proposing repair of the committed stroke timeline.");
        ui.add_enabled_ui(!busy, |ui| {
            ui.label("Funscript path");
            ui.add(egui::TextEdit::singleline(path).char_limit(4096));
            if ui
                .add_enabled(
                    project && !path.trim().is_empty(),
                    egui::Button::new("Import sanitized candidate"),
                )
                .clicked()
            {
                action = Some(RepairAction::Import(PathBuf::from(path.trim())));
            }
            for (label, value) in [
                ("Maximum percent/second", &mut self.maximum),
                ("Jitter window milliseconds", &mut self.jitter_window),
                ("Jitter threshold percent", &mut self.jitter_threshold),
            ] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(value).char_limit(64));
                });
            }
            let options = self.options();
            if let Err(error) = &options {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    format!("Invalid repair policy: {error}"),
                );
            }
            if ui
                .add_enabled(
                    project && master && options.is_ok(),
                    egui::Button::new("Propose numerical repair"),
                )
                .clicked()
            {
                if let Ok(options) = options {
                    action = Some(RepairAction::Propose(options));
                }
            }
        });
        ui.small("Single gap-free stroke on the standard millisecond/percent grid. Existing protection and revision checks apply. Repair remains uncommitted; no export follows. Numeric slopes do not qualify motor safety or motion quality.");
        action
    }
}

pub(crate) fn reply_matches(
    project: Option<(&ProjectId, RevisionId)>,
    binding: &(ProjectId, RevisionId),
) -> bool {
    project.is_some_and(|(id, revision)| id == &binding.0 && revision == binding.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::fixture_descriptor;
    fn program() -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            [(0, 0.0), (10, 0.01), (20, 1.0), (40, 1.0), (100, 0.5)]
                .into_iter()
                .map(|(t, p)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(t * 1_000_000),
                        NormalizedPosition::new(p).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()])
        .unwrap()
    }
    struct Api {
        calls: usize,
        mismatch: u8,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<crate::ResponseBody> {
            panic!("repair must not commit or export")
        }
        fn upload_edit(
            &mut self,
            project: ProjectId,
            revision: RevisionId,
            program: &MotionProgram,
            _: &str,
        ) -> Result<CandidateSnapshot> {
            self.calls += 1;
            assert_eq!(revision, RevisionId::new(4));
            let candidate_id = CandidateId::new("repair").unwrap();
            Ok(CandidateSnapshot {
                project_interval: None,
                candidate_id: candidate_id.clone(),
                project_id: project.clone(),
                base_revision: if self.mismatch == 1 {
                    RevisionId::new(5)
                } else {
                    revision
                },
                motion: fixture_descriptor(
                    program,
                    MotionBinding::Candidate {
                        project_id: project,
                        candidate_id,
                        base_revision: revision,
                    },
                ),
                program: Arc::new(if self.mismatch == 2 {
                    MotionProgram::default()
                } else {
                    program.clone()
                }),
                job_id: (self.mismatch == 3).then(|| JobId::new("wrong-job").unwrap()),
                review: vec![],
            })
        }
    }
    fn request() -> RepairRequest {
        RepairRequest {
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(4),
            program: Arc::new(program()),
            options: ScriptRepairOptions::new(450.0, 20, 2).unwrap(),
        }
    }
    #[test]
    fn proposes_checked_values_without_commit_and_rejects_mismatched_reply() {
        let mut api = Api {
            calls: 0,
            mismatch: 0,
        };
        let candidate = propose(&mut api, request()).unwrap();
        assert_eq!(api.calls, 1);
        assert_eq!(
            candidate
                .program
                .track(Axis::Stroke)
                .unwrap()
                .actions()
                .iter()
                .map(|a| (
                    a.time().as_nanos() / 1_000_000,
                    (a.position().value() * 100.0).round() as i64
                ))
                .collect::<Vec<_>>(),
            vec![(0, 0), (20, 9), (40, 18), (100, 45)]
        );
        for mismatch in [1, 2, 3] {
            api.mismatch = mismatch;
            let error = propose(&mut api, request()).unwrap_err();
            assert!(error.to_string().contains("candidate repair"));
        }
        assert_eq!(api.calls, 4);
    }
    #[test]
    fn unsupported_input_refuses_before_upload_and_late_reply_is_bound() {
        let mut api = Api {
            calls: 0,
            mismatch: 0,
        };
        let mut request = request();
        request.program = Arc::new(MotionProgram::default());
        assert!(propose(&mut api, request).is_err());
        assert_eq!(api.calls, 0);
        let id = ProjectId::new("project").unwrap();
        let other = ProjectId::new("other").unwrap();
        let binding = (id.clone(), RevisionId::new(4));
        assert!(reply_matches(Some((&id, RevisionId::new(4))), &binding));
        assert!(!reply_matches(Some((&other, RevisionId::new(4))), &binding));
        assert!(!reply_matches(Some((&id, RevisionId::new(5))), &binding));
        assert!(!reply_matches(None, &binding));
    }

    fn draw(
        context: &egui::Context,
        panel: &mut RepairPanel,
        path: &mut String,
        project: bool,
        master: bool,
        busy: bool,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<RepairAction>) {
        let mut action = None;
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    action = panel.ui(ui, path, project, master, busy);
                });
            },
        );
        (output, action)
    }
    fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                Some(s.pos + s.galley.rect.center().to_vec2())
            }
            egui::epaint::Shape::Vec(s) => s.iter().find_map(|s| text(s, label)),
            _ => None,
        }
    }
    fn click(
        context: &egui::Context,
        panel: &mut RepairPanel,
        path: &mut String,
        label: &str,
        project: bool,
        master: bool,
        busy: bool,
    ) -> Option<RepairAction> {
        draw(context, panel, path, project, master, busy, vec![]);
        let (output, _) = draw(context, panel, path, project, master, busy, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, label))
            .expect("drawn repair control");
        let mut action = None;
        for pressed in [true, false] {
            let (_, result) = draw(
                context,
                panel,
                path,
                project,
                master,
                busy,
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
            action = action.or(result);
        }
        action
    }
    #[test]
    fn drawn_import_and_proposal_capture_inputs_and_keep_disabled_states_inert() {
        let context = egui::Context::default();
        let mut panel = RepairPanel::default();
        let mut path = "/authored.funscript".into();
        assert!(click(
            &context,
            &mut panel,
            &mut path,
            "Import sanitized candidate",
            false,
            false,
            false
        )
        .is_none());
        let Some(RepairAction::Import(captured)) = click(
            &context,
            &mut panel,
            &mut path,
            "Import sanitized candidate",
            true,
            false,
            false,
        ) else {
            panic!("import action required")
        };
        path = "/changed.funscript".into();
        assert_eq!(captured, PathBuf::from("/authored.funscript"));
        assert!(click(
            &context,
            &mut panel,
            &mut path,
            "Propose numerical repair",
            true,
            false,
            false
        )
        .is_none());
        assert!(click(
            &context,
            &mut panel,
            &mut path,
            "Propose numerical repair",
            true,
            true,
            true
        )
        .is_none());
        let Some(RepairAction::Propose(options)) = click(
            &context,
            &mut panel,
            &mut path,
            "Propose numerical repair",
            true,
            true,
            false,
        ) else {
            panic!("proposal required")
        };
        panel.maximum = "NaN".into();
        assert!(click(
            &context,
            &mut panel,
            &mut path,
            "Propose numerical repair",
            true,
            true,
            false
        )
        .is_none());
        let repaired = repair_standard_script(&program(), options).unwrap();
        assert_eq!(repaired.clamped_positions, 3);
    }
    #[test]
    fn drawn_keyboard_policy_edits_are_checked_before_effects() {
        let context = egui::Context::default();
        let mut panel = RepairPanel::default();
        let mut path = String::new();
        draw(&context, &mut panel, &mut path, true, true, false, vec![]);
        let (output, _) = draw(&context, &mut panel, &mut path, true, true, false, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, "450"))
            .expect("policy text");
        for pressed in [true, false] {
            draw(
                &context,
                &mut panel,
                &mut path,
                true,
                true,
                false,
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
        }
        draw(
            &context,
            &mut panel,
            &mut path,
            true,
            true,
            false,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text("100".into()),
            ],
        );
        assert_eq!(panel.maximum, "100");
        let Some(RepairAction::Propose(options)) = click(
            &context,
            &mut panel,
            &mut path,
            "Propose numerical repair",
            true,
            true,
            false,
        ) else {
            panic!("checked edited policy required")
        };
        let repaired = repair_standard_script(&program(), options).unwrap();
        assert_eq!(
            repaired
                .program
                .track(Axis::Stroke)
                .unwrap()
                .actions()
                .last()
                .unwrap()
                .position()
                .value(),
            0.1
        );
    }
}
