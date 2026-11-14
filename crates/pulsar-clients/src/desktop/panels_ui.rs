use super::*;

impl PulsarDesktop {
    pub(super) fn doctor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Evidence and protection review");
        ui.label("Diagnostics are read-only. Explicit unlock changes protection; it does not repair motion or qualify hardware.");
        ui.label("Average normalized segment slope; gaps are skipped. Peak velocity, jitter repair and device safety are not measured.");
        ui.horizontal(|ui| {
            ui.label("Maximum percent/second");
            ui.add(
                egui::DragValue::new(&mut self.segment_speed_limit)
                    .range(0.001..=100_000_000_000.0),
            );
        });
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Inspect committed revision"),
            )
            .clicked()
        {
            self.project_command(
                Command::Diagnostics {
                    candidate_id: None,
                    maximum_speed_percent_per_second: Some(self.segment_speed_limit),
                },
                false,
            );
        }
        self.diagnostics_view(ui);
        ui.separator();
        let master = self
            .project
            .as_ref()
            .is_some_and(|p| !p.program.tracks().is_empty());
        if let Some(action) = self.repair_panel.ui(
            ui,
            &mut self.script_import_path,
            self.project.is_some(),
            master,
            self.busy,
        ) {
            match action {
                crate::repair_panel::RepairAction::Import(path) => match absolute_path(&path) {
                    Ok(path) => {
                        self.project_command(Command::ImportFunscriptForRepair { path }, true);
                    }
                    Err(error) => self.status = error.to_string(),
                },
                crate::repair_panel::RepairAction::Propose(options) => {
                    if let Some(project) = self.project.clone() {
                        let request = crate::repair_panel::RepairRequest {
                            project: project.project_id.clone(),
                            revision: project.revision,
                            program: project.program,
                            options,
                        };
                        self.enqueue(
                            RpcCommand::Repair(request),
                            Some(project.project_id),
                            Some(project.revision),
                        );
                    }
                }
            }
        }
    }

    pub(super) fn diagnostics_current(&self) -> Option<&DiagnosticsReport> {
        self.diagnostics.as_ref().filter(|report| {
            diagnostics_match_view(
                report,
                self.project
                    .as_ref()
                    .map(|project| (&project.project_id, project.revision)),
                self.candidate
                    .as_ref()
                    .map(|candidate| &candidate.candidate_id),
            )
        })
    }

    pub(super) fn diagnostics_view(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.diagnostics_current().cloned() else {
            return;
        };
        if report.candidate_id.is_some()
            && self
                .candidate
                .as_ref()
                .map(|candidate| &candidate.candidate_id)
                != report.candidate_id.as_ref()
        {
            ui.label("Diagnostics refer to a different candidate; request fresh diagnostics.");
            return;
        }
        ui.separator();
        ui.label(format!(
            "Diagnostics revision {} / {} protected regions",
            report.revision.value(),
            report.protected_regions.len()
        ));
        if let Some(scan) = &report.segment_speed {
            ui.label(format!("Average segment slope limit {} percent/second: {} violations / {} visited edges; {} unavailable edges skipped{}.",
                scan.maximum_percent_per_second, scan.violations.len(), scan.scanned_segments, scan.unavailable_segments,
                if scan.truncated { "; truncated prefix" } else { "" }));
            for violation in &scan.violations {
                ui.label(format!(
                    "{:?} {}..{} ns: {:.6} percent/second",
                    violation.axis,
                    violation.range.start().as_nanos(),
                    violation.range.end().as_nanos(),
                    violation.average_percent_per_second
                ));
            }
        }
        ui.label("Manual commits protect changed interpolation spans automatically. Unlocking is an explicit, undoable policy change.");
        let mut unlock = None;
        for (index, region) in report.protected_regions.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Protected {:?}: {:.3}..{:.3}s",
                    region.axis,
                    region.range.start().as_nanos() as f64 / 1e9,
                    region.range.end().as_nanos() as f64 / 1e9
                ));
                if ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Explicitly unlock this region"),
                    )
                    .clicked()
                {
                    unlock = Some(index);
                }
            });
        }
        if let Some(index) = unlock {
            let mut regions = report.protected_regions.clone();
            regions.remove(index);
            self.project_command(Command::SetProtectedRegions { regions }, true);
        }
        for issue in report.issues.iter().take(256) {
            let span = issue
                .range
                .as_ref()
                .map(|range| {
                    format!(
                        "{:.3}..{:.3}s",
                        range.start().as_nanos() as f64 / 1e9,
                        range.end().as_nanos() as f64 / 1e9
                    )
                })
                .unwrap_or_default();
            ui.label(format!(
                "{} {:?} {}: {}",
                issue.code, issue.axis, span, issue.message
            ));
        }
        if report.issues.is_empty() {
            ui.label("No issues reported by these checks. Neural accuracy and safety remain unqualified.");
        }
        if self.candidate.is_none() {
            self.review_decisions_view(ui);
        }
    }
    pub(super) fn review_decisions_view(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.review_status.clone().filter(|r| {
            self.project
                .as_ref()
                .is_some_and(|p| p.project_id == r.project_id && p.revision == r.revision)
        }) else {
            return;
        };
        ui.separator();
        ui.label(
            "Explicit review decisions retain the original warnings. Edits require review again.",
        );
        if report.unknown {
            ui.label(
                "Review ancestry remains unknown; decisions do not establish missing evidence.",
            );
        }
        ui.label("Review rationale (required)");
        ui.text_edit_singleline(&mut self.review_note);
        ui.checkbox(&mut self.review_partial, "Review a selected subinterval");
        if self.review_partial {
            ui.horizontal(|ui| {
                ui.label("Start ns");
                ui.add(egui::DragValue::new(&mut self.review_interval_ns.0));
                ui.label("End ns (exclusive)");
                ui.add(egui::DragValue::new(&mut self.review_interval_ns.1));
            });
        }
        let mut selected = None;
        for entry in report.entries.iter().take(256) {
            let unresolved = entry
                .unresolved_ranges()
                .unwrap_or_else(|_| vec![entry.flag.range]);
            let resolved = unresolved.is_empty();
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {:?}: {:?} {}..{} ns",
                    entry.flag.reason.as_str(),
                    if resolved { "resolved" } else { "open" },
                    entry.flag.axis,
                    entry.flag.range.start().as_nanos(),
                    entry.flag.range.end().as_nanos()
                ));
                let range = TimeRange::new(
                    ProjectTime::from_nanos(self.review_interval_ns.0),
                    ProjectTime::from_nanos(self.review_interval_ns.1),
                )
                .ok()
                .filter(|r| {
                    r.start() >= entry.flag.range.start() && r.end() <= entry.flag.range.end()
                });
                if self.review_partial {
                    for (label, disposition) in [
                        ("Resolve selected interval", ReviewDisposition::Resolved),
                        ("Reopen selected interval", ReviewDisposition::Reopened),
                    ] {
                        if ui
                            .add_enabled(
                                !self.busy
                                    && range.is_some()
                                    && validate_review_intent(&entry.review_id, &self.review_note)
                                        .is_ok(),
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            selected = Some((entry.review_id.clone(), disposition, range));
                        }
                    }
                } else if ui
                    .add_enabled(
                        !self.busy
                            && validate_review_intent(&entry.review_id, &self.review_note).is_ok(),
                        egui::Button::new(if resolved {
                            "Reopen whole warning"
                        } else {
                            "Resolve whole warning"
                        }),
                    )
                    .clicked()
                {
                    selected = Some((
                        entry.review_id.clone(),
                        if resolved {
                            ReviewDisposition::Reopened
                        } else {
                            ReviewDisposition::Resolved
                        },
                        None,
                    ));
                }
            });
            if !entry.decision_history.is_empty() {
                ui.label(format!("Unreviewed ranges: {:?}", unresolved));
                for d in &entry.decision_history {
                    ui.label(format!(
                        "{} {:?} at revision {}: {}",
                        d.actor, d.range, d.recorded_revision, d.note
                    ));
                }
            }
            if let Some(d) = &entry.decision {
                ui.label(format!(
                    "Recorded actor {} at revision {}: {}",
                    d.actor, d.recorded_revision, d.note
                ));
            }
        }
        if report.entries.len() > 256 {
            ui.label("Additional warnings are available through CLI review-status.");
        }
        if let Some((id, disposition, range)) = selected {
            let request_id = crate::session::new_request_id();
            let command = if let Some(range) = range {
                Command::SetReviewIntervalDisposition {
                    review_id: id,
                    range,
                    disposition,
                    note: self.review_note.clone(),
                }
            } else {
                Command::SetReviewDisposition {
                    review_id: id,
                    disposition,
                    note: self.review_note.clone(),
                }
            };
            self.review_retry = Some((
                request_id.clone(),
                command.clone(),
                report.project_id.clone(),
                report.revision,
            ));
            self.review_send(request_id, command, report.project_id, report.revision);
        }
        if let Some((id, command, project, revision)) = self.review_retry.clone() {
            ui.label(format!("Last review request: {id}"));
            if ui
                .add_enabled(!self.busy, egui::Button::new("Retry exact review request"))
                .clicked()
            {
                self.review_send(id, command, project, revision);
            }
        }
    }
    pub(super) fn review_send(
        &mut self,
        id: RequestId,
        command: Command,
        project: ProjectId,
        revision: RevisionId,
    ) {
        if self
            .sender
            .try_send(RpcWork {
                command: RpcCommand::Identified(id, command),
                project: Some(project),
                revision: Some(revision),
            })
            .is_ok()
        {
            self.busy = true;
        } else {
            self.status = "Review queue is full; exact request retained for retry".into();
        }
    }

    pub(super) fn paint_review_spans(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        duration: f64,
    ) {
        let paint = |range: &TimeRange, color: Color32| {
            let start = (range.start().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
            let end = (range.end().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
            if end > start {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left() + start * rect.width(), rect.top()),
                        egui::pos2(rect.left() + end * rect.width(), rect.bottom()),
                    ),
                    0.0,
                    color,
                );
            }
        };
        if let Some(candidate) = &self.candidate {
            for flag in &candidate.review {
                if flag.axis.is_none() || flag.axis == Some(self.timeline_axis) {
                    paint(
                        &flag.range,
                        Color32::from_rgba_unmultiplied(235, 159, 43, 30),
                    );
                }
            }
        }
        if let Some(report) = self.diagnostics_current() {
            for region in &report.protected_regions {
                if region.axis.is_none() || region.axis == Some(self.timeline_axis) {
                    paint(
                        &region.range,
                        Color32::from_rgba_unmultiplied(158, 169, 188, 60),
                    );
                }
            }
        }
    }

    pub(super) fn has_media_source(&self) -> bool {
        self.project.as_ref().is_some_and(|project| {
            project.sources.iter().any(|source| {
                source.kind == pulsar_protocol::SourceKind::Media
                    && self.source.as_ref() == Some(&source.source_version)
            })
        })
    }

    pub(super) fn project_panel(&mut self, ui: &mut egui::Ui) {
        if let Some((count, revision)) = self
            .diagnostics_current()
            .map(|report| (report.issues.len(), report.revision.value()))
        {
            ui.heading("Review status");
            if count > 0 {
                ui.colored_label(
                    Color32::YELLOW,
                    format!("{count} diagnostic issue(s) at revision {revision}"),
                );
            } else {
                ui.label(format!(
                    "No issues from current checks at revision {revision}."
                ));
                ui.small("This is not accuracy or safety qualification.");
            }
            if ui.button("Open read-only diagnostics").clicked() {
                self.tab = Tab::Doctor;
            }
            ui.separator();
        }
        ui.heading("Project");
        ui.text_edit_singleline(&mut self.project_name);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Create project"))
            .clicked()
        {
            self.send(
                Command::CreateProject {
                    name: self.project_name.clone(),
                },
                None,
                None,
            );
        }
        ui.label("Existing project ID");
        ui.text_edit_singleline(&mut self.open_project_id);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Open project"))
            .clicked()
        {
            match ProjectId::new(self.open_project_id.trim()) {
                Ok(project_id) => {
                    self.send(Command::OpenProject { project_id }, None, None);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(project) = &self.project {
            ui.separator();
            ui.label(RichText::new(&project.name).strong());
            ui.label(format!("Revision {}", project.revision.value()));
            ui.label(format!("{} motion axes", project.program.tracks().len()));
            let sources = project.sources.clone();
            for source in sources {
                if source.kind != pulsar_protocol::SourceKind::Media {
                    let kind = match source.kind {
                        pulsar_protocol::SourceKind::GenerationInput => "generation input",
                        pulsar_protocol::SourceKind::Funscript => "funscript",
                        pulsar_protocol::SourceKind::LiveRecording => "accepted live recording",
                        pulsar_protocol::SourceKind::Media => unreachable!(),
                    };
                    ui.add_enabled(
                        false,
                        egui::Label::new(format!("{} [{}; not previewable]", source.label, kind)),
                    );
                    continue;
                }
                if ui
                    .selectable_label(
                        self.source.as_ref() == Some(&source.source_version),
                        &source.label,
                    )
                    .clicked()
                {
                    self.source = Some(source.source_version);
                    self.selected_placement = None;
                    self.seek(0.0);
                }
            }
        }
        self.placement_controls(ui);
        ui.separator();
        ui.label("Media path");
        ui.text_edit_singleline(&mut self.source_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Import immutable source"),
            )
            .clicked()
        {
            if self.source_path.trim().is_empty() {
                self.status = "Enter a source path.".into();
            } else {
                match absolute_path(std::path::Path::new(&self.source_path)) {
                    Ok(path) => {
                        self.project_command(Command::ImportSource { path }, true);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        ui.separator();
        ui.label(format!("Export {:?} .funscript path", self.timeline_axis));
        ui.text_edit_singleline(&mut self.export_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Export committed revision"),
            )
            .clicked()
        {
            if self.export_path.trim().is_empty() {
                self.status = "Enter an export path.".into();
            } else {
                match absolute_path(std::path::Path::new(&self.export_path)) {
                    Ok(path) => {
                        self.project_command(
                            Command::ExportAxis {
                                path,
                                axis: self.timeline_axis,
                            },
                            true,
                        );
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        let edge_count = self
            .project
            .as_ref()
            .map(|project| editor::unavailable_edge_count(&project.program, self.timeline_axis))
            .unwrap_or(0);
        if edge_count > 0 {
            ui.collapsing("Unavailable edge time", |ui| {
                ui.label(format!("{} unavailable intervals lie outside this axis's first and last keyframes.", edge_count));
                ui.small("Trim excludes that unsupported time. Keyframe times, positions and evidence stay unchanged; internal gaps still block export. Undo restores the excluded intervals.");
                if ui.add_enabled(!self.busy, egui::Button::new("Trim unavailable edge time on selected axis")).clicked() {
                    if let Some(project) = &self.project {
                        match editor::trim_unavailable_edges(&project.program, self.timeline_axis) {
                            Ok(proposal) => { self.send_edit(proposal, project.project_id.clone(), project.revision); }
                            Err(error) => self.status = error.to_string(),
                        }
                    }
                }
            });
        }
        ui.label("Device dynamics profile JSON path");
        ui.text_edit_singleline(&mut self.dynamics_profile_path);
        if ui
            .add_enabled(
                !self.busy && self.project.is_some(),
                egui::Button::new("Export device-motion proposal JSON"),
            )
            .clicked()
        {
            let result = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.dynamics_profile_path,
                ))?;
                let profile: DynamicsProfile = serde_json::from_slice(&bytes)?;
                profile.validate()?;
                anyhow::ensure!(!self.export_path.trim().is_empty(), "Enter an export path.");
                Ok(Command::ExportDeviceMotion {
                    path: absolute_path(std::path::Path::new(&self.export_path))?,
                    profile,
                })
            })();
            match result {
                Ok(command) => {
                    self.project_command(command, true);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.small("Proposal only. Requires the specified quintic interpolation and stopped entry; it does not qualify or arm a device.");
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Undo"),
                )
                .clicked()
            {
                self.project_command(Command::Undo, true);
            }
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Redo"),
                )
                .clicked()
            {
                self.project_command(Command::Redo, true);
            }
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Refresh"),
                )
                .clicked()
            {
                self.project_command(Command::GetSnapshot, false);
            }
        });
    }

    pub(super) fn placement_controls(&mut self, ui: &mut egui::Ui) {
        let placements = self
            .project
            .as_ref()
            .map(|p| p.placements.clone())
            .unwrap_or_default();
        ui.collapsing("Source placements",|ui|{
            ui.label("Align media in project time. Existing motion is preserved and flagged for review.");
            if ui.selectable_label(self.selected_placement.is_none(),"Original clip clock").clicked(){self.selected_placement=None;self.seek(0.0);}
            for p in placements {
                if ui.selectable_label(self.selected_placement.as_ref()==Some(&p.placement_id),format!("{} / speed {}/{}",p.placement_id,p.spec.rate_numerator,p.spec.rate_denominator)).clicked(){
                    self.source=Some(p.spec.source_version.clone());if self.retime_from.is_empty(){self.retime_from=p.placement_id.to_string();}self.selected_placement=Some(p.placement_id);
                    self.placement_times=[p.spec.source_start.numerator() as f64/f64::from(p.spec.source_start.denominator()),p.spec.source_end.numerator() as f64/f64::from(p.spec.source_end.denominator()),p.spec.source_anchor.numerator() as f64/f64::from(p.spec.source_anchor.denominator()),p.spec.project_anchor.as_nanos() as f64/1e9];
                    self.placement_rate=(p.spec.rate_numerator,p.spec.rate_denominator);self.seek(self.placement_times[3]);
                }
            }
            for (index,label) in ["Trim start","Trim end","Source anchor","Project anchor"].iter().enumerate(){ui.horizontal(|ui|{ui.label(*label);ui.add(egui::DragValue::new(&mut self.placement_times[index]).range(if index==3{-86400.0..=86400.0}else{0.0..=86400.0}).suffix(" s"));});}
            ui.horizontal(|ui|{ui.label("Speed");ui.add(egui::DragValue::new(&mut self.placement_rate.0).range(-1000..=1000));ui.label("/");ui.add(egui::DragValue::new(&mut self.placement_rate.1).range(1..=1000));});
            if ui.add_enabled(!self.busy && self.has_media_source(),egui::Button::new(if self.selected_placement.is_some(){"Replace selected placement"}else{"Add placement"})).clicked(){
                let build=||->anyhow::Result<SourcePlacementSpec>{
                    let source_version=self.source.clone().ok_or_else(||anyhow::anyhow!("Select media first"))?;
                    let rational=|seconds:f64|->anyhow::Result<SourceTimestamp>{anyhow::ensure!(seconds.is_finite(),"Time must be finite");Ok(SourceTimestamp::new((seconds*1e9).round() as i64,1_000_000_000)?)};
                    let spec=SourcePlacementSpec{source_version,source_start:rational(self.placement_times[0])?,source_end:rational(self.placement_times[1])?,source_anchor:rational(self.placement_times[2])?,project_anchor:ProjectTime::from_nanos((self.placement_times[3]*1e9).round() as i64),rate_numerator:self.placement_rate.0,rate_denominator:self.placement_rate.1};spec.validate()?;Ok(spec)
                };
                match build(){Ok(spec)=>{self.placement_save_pending=self.project_command(Command::SetSourcePlacement{spec,replace:self.selected_placement.clone()},true);},Err(error)=>self.status=error.to_string()}
            }
            if let Some(placement_id)=self.selected_placement.clone(){if ui.add_enabled(!self.busy,egui::Button::new("Remove placement")).clicked(){self.project_command(Command::RemoveSourcePlacement{placement_id},true);}}
            ui.separator();
            ui.label("Retime existing motion");
            ui.label("Treat selected motion as the old clock and map it to the selected placement. Positions stay unchanged; protected spans and trim loss refuse. Undo restores the previous motion.");
            ui.horizontal(|ui|{ui.label("Old placement ID");ui.add(egui::TextEdit::singleline(&mut self.retime_from).char_limit(128));});
            let available=self.project.as_ref().map(|p|p.program.tracks().iter().map(|t|t.axis()).collect::<Vec<_>>()).unwrap_or_default();
            self.retime_axes.retain(|axis|available.contains(axis));
            ui.horizontal_wrapped(|ui|{for axis in available {let mut selected=self.retime_axes.contains(&axis);if ui.checkbox(&mut selected,axis.as_str()).changed(){self.retime_axes.retain(|a|*a!=axis);if selected{self.retime_axes.push(axis);}}}});
            if ui.add_enabled(!self.busy && !self.retime_axes.is_empty() && self.selected_placement.is_some() && !self.retime_from.is_empty(),egui::Button::new("Retime selected motion")).clicked(){
                match SourcePlacementId::new(self.retime_from.clone()) {Ok(from_placement)=>{let to_placement=self.selected_placement.clone().expect("enabled with selected placement");self.project_command(Command::RetimeMotion {from_placement,to_placement,axes:self.retime_axes.clone()},true);},Err(error)=>self.status=error.to_string()}
            }
            ui.collapsing("Retiming receipts",|ui|{
                ui.label("Inspect a recorded edit by revision. Imported attribution remains historical.");
                ui.horizontal(|ui|{
                    ui.label("Revision");
                    if ui.add_enabled(!self.busy,egui::DragValue::new(&mut self.retime_status_revision)).changed(){self.retime_receipt=None;}
                    for (label,archival) in [("Local receipt",false),("Imported receipt",true)] {
                        if ui.add_enabled(!self.busy && self.project.is_some(),egui::Button::new(label)).clicked(){
                            self.retime_receipt=None;let revision=RevisionId::new(self.retime_status_revision);
                            self.project_command(if archival {Command::ArchivedMotionRetimeStatus {revision}} else {Command::MotionRetimeStatus {revision}},false);
                        }
                    }
                });
                if let Some((summary,details))=&self.retime_receipt {
                    ui.label(summary);ui.collapsing("Receipt details",|ui|{egui::ScrollArea::vertical().max_height(220.0).show(ui,|ui|{ui.monospace(details);});});
                }
            });

        });
    }

    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        if !self.has_media_source() {
            ui.label("No previewable media source. Generated patterns and scripts remain available on the timeline.");
        }
        ui.add_enabled_ui(self.has_media_source(), |ui| {

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.author_target_mode && !self.pick_target_mode, egui::Button::new(if self.preview_playing {
                    "Pause preview"
                } else {
                    "Play preview"
                }))
                .clicked()
            {
                self.preview_playing = !self.preview_playing;
                self.last_tick = Instant::now();
            }
            if ui
                .add_enabled(
                    !self.author_target_mode && !self.pick_target_mode,
                    egui::Button::new("Stop preview"),
                )
                .clicked()
            {
                self.preview_playing = false;
                let start = self.preview_placement()
                    .map_or(0.0, |placement| placement.project_anchor.as_nanos() as f64 / 1e9);
                self.seek(start);
            }
            let response = ui.add(
                egui::DragValue::new(&mut self.playhead_seconds)
                    .speed(0.04)
                    .range(if self.selected_placement.is_some(){-86_400.0..=86_400.0}else{0.0..=86_400.0})
                    .suffix(" s"),
            );
            if response.changed() {
                self.seek(self.playhead_seconds);
            }
            if ui
                .add_enabled(
                    !self.busy && self.has_media_source(),
                    egui::Button::new("Analyze displayed time"),
                )
                .clicked()
            {
                self.seek(self.playhead_seconds);
            }
        });
        ui.small("Independent preview. The declared eye and viewport apply to displayed pixels. Audio and device playback remain separate.");
        let available = ui.available_size();
        let annotated = self.preview.displayed().is_some_and(|result| result.annotated.is_some());
        let metadata_height = if annotated { 140.0 } else { 90.0 };
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available.x.max(1.0), (available.y - metadata_height).max(80.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(8, 11, 17));
        if let Some(texture) = &self.texture {
            let draw_image = |texture: &egui::TextureHandle, viewport: egui::Rect| {
                let image_size = texture.size_vec2();
                let scale = (viewport.width() / image_size.x).min(viewport.height() / image_size.y);
                let image_rect = egui::Rect::from_center_size(viewport.center(), image_size * scale);
                painter.image(
                    texture.id(), image_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                image_rect
            };
            let current_viewport = if let Some(previous) = &self.previous_texture {
                let middle = rect.center().x;
                let previous_viewport = egui::Rect::from_min_max(rect.min, egui::pos2(middle - 4.0, rect.bottom()));
                let current_viewport = egui::Rect::from_min_max(egui::pos2(middle + 4.0, rect.top()), rect.max);
                draw_image(previous, previous_viewport);
                painter.text(previous_viewport.left_top() + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP,
                    "Previous target", egui::FontId::proportional(14.0), Color32::WHITE);
                current_viewport
            } else { rect };
            let image_rect = draw_image(texture, current_viewport);
            if self.previous_texture.is_some() {
                painter.text(current_viewport.left_top() + egui::vec2(6.0, 6.0), egui::Align2::LEFT_TOP,
                    "Current target", egui::FontId::proportional(14.0), Color32::WHITE);
            }
            if let Some(result) = self.preview.displayed() {
                if self.pick_target_mode {
                    if let Err(error) = self.target_picker.widget(ui, image_rect, result, self.video_view.as_ref()) {
                        self.status = format!("Target pick unavailable: {error:#}");
                    }
                }
                if self.author_target_mode {
                    if let Some(view) = &self.video_view {
                        if let Err(error) = self.target_author.widget(ui, image_rect, result, view) {
                            self.status = format!("Target drawing unavailable: {error:#}");
                        }
                    } else { self.target_author.clear(); }
                }
                if let (Some(map), Some(cursor)) = (&result.coordinate_map, ui.ctx().pointer_hover_pos()) {
                    if image_rect.contains(cursor) {
                        let point = ViewPoint {
                            x: f64::from((cursor.x - image_rect.left()) / image_rect.width()),
                            y: f64::from((cursor.y - image_rect.top()) / image_rect.height()),
                        };
                        if let Ok(source) = map.to_source(point) {
                            painter.text(image_rect.left_top(), egui::Align2::LEFT_TOP,
                                format!("Source position: {:.4}, {:.4}", source.x, source.y),
                                egui::FontId::monospace(12.0), Color32::WHITE);
                        }
                    }
                }
                for observation in &result.observations {
                    if !observation.matches_frame(&result.context) {
                        continue;
                    }
                    let bounds = observation.bounds();
                    // The worker publishes normalized displayed-view coordinates.
                    // Unmapped model/crop/viewport pixels are never guessed.
                    if bounds.space() != CoordinateSpace::ProjectionNormalized {
                        continue;
                    }
                    let min = egui::pos2(
                        image_rect.left() + bounds.x_min() as f32 * image_rect.width(),
                        image_rect.top() + bounds.y_min() as f32 * image_rect.height(),
                    );
                    let max = egui::pos2(
                        image_rect.left() + bounds.x_max() as f32 * image_rect.width(),
                        image_rect.top() + bounds.y_max() as f32 * image_rect.height(),
                    );
                    let color = if observation.evidence() == EvidenceKind::Observed {
                        Color32::from_rgb(61, 223, 229)
                    } else {
                        Color32::from_rgb(255, 188, 79)
                    };
                    painter.rect_stroke(
                        egui::Rect::from_min_max(min, max),
                        1.0,
                        Stroke::new(2.0_f32, color),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        min,
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:?}", observation.evidence()),
                        egui::FontId::monospace(12.0),
                        color,
                    );
                }
            }
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.busy && self.has_media_source() {
                    "Waiting for a matching source frame"
                } else {
                    "Import media, then request a preview"
                },
                egui::FontId::proportional(18.0),
                Color32::from_rgb(151, 166, 185),
            );
        }
        egui::ScrollArea::vertical().id_salt("preview-metadata")
            .max_height(metadata_height).show(ui, |ui| {
        if let Some(result) = self.preview.displayed() {
            ui.label(format!(
                "Decoded source frame {} / source time {:?}",
                result.source_frame_index, result.source_time
            ));
            if let Some(elapsed) = result.elapsed_source_time { ui.label(format!("Elapsed clip time {:.6} s", elapsed.numerator() as f64/f64::from(elapsed.denominator()))); }
            if let Some(placed)=&result.placed {ui.label(format!("Actual project time {:.6} s",placed.actual_project_time.numerator() as f64/placed.actual_project_time.denominator() as f64));}
            if let Some(evidence)=&result.annotated {
                ui.label(format!("Target {} · original source stride {} · unqualified estimate",
                    evidence.spec.view.declaration.target_id,evidence.spec.source_sample_stride));
                if let Some(position)=evidence.position {ui.label(format!("Target position: {:.6}",position.value()));}
                else {ui.colored_label(Color32::YELLOW,"No supported frame-pair estimate at this sample.");}
                if let Some(previous)=&evidence.previous {
                    ui.label(format!("Previous target: source frame {} / PTS {:?}; current target: source frame {}. Pair follows original source order, including reverse placements.",previous.source_frame_index,previous.source_time,result.source_frame_index));
                }
                ui.small(format!("Model {} · profile {} · runtime {}",evidence.model.sha256,evidence.profile.sha256,evidence.runtime.sha256));
            }
            match &result.analysis {
                AnalysisStatus::Available => {
                    if result.annotated.is_none() {ui.label(format!("{} matching observations. Empty means no detection, not a fixed analysis box.", result.observations.len()));}
                }
                AnalysisStatus::Pending => {
                    ui.label("Analysis pending. No stale evidence shown.");
                }
                AnalysisStatus::Unavailable { reason } => {
                    ui.colored_label(Color32::YELLOW, format!("Analysis unavailable: {reason}"));
                }
            }
        }
        });
            });
    }

    pub(super) fn timeline_duration_seconds(&self) -> f64 {
        self.project
            .iter()
            .map(|project| &project.program)
            .chain(self.candidate.iter().map(|candidate| &candidate.program))
            .flat_map(|program| program.tracks())
            .flat_map(|track| track.actions())
            .map(|action| action.time().as_nanos() as f64 / 1e9)
            .fold(10.0, f64::max)
            .max(self.playhead_seconds + 1.0)
    }

    pub(super) fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Timeline axis");
            for axis in [
                Axis::Stroke,
                Axis::Sway,
                Axis::Surge,
                Axis::Roll,
                Axis::Pitch,
                Axis::Yaw,
            ] {
                if ui
                    .selectable_value(&mut self.timeline_axis, axis, format!("{axis:?}"))
                    .changed()
                {
                    self.gesture = None;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{:?} / NEUTRAL MASTER TIMELINE",
                    self.timeline_axis
                ))
                .strong(),
            );
            ui.add(egui::Slider::new(&mut self.new_position, 0.0..=1.0).text("New position"));
            if ui
                .add_enabled(
                    !self.busy && self.project.is_some(),
                    egui::Button::new("Add at playhead"),
                )
                .clicked()
            {
                if let Some(project) = &self.project {
                    let mut points = editor::axis_points(&project.program, self.timeline_axis);
                    points.push(EditorPoint {
                        nanos: (self.playhead_seconds * 1_000_000_000.0).round() as i64,
                        position: self.new_position,
                    });
                    match editor::replace_axis(&project.program, self.timeline_axis, &points) {
                        Ok(program) => {
                            self.send_edit(
                                EditProposal {
                                    program,
                                    label: format!("Add {:?} keyframe", self.timeline_axis),
                                },
                                project.project_id.clone(),
                                project.revision,
                            );
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        });
        let committed = self
            .project
            .as_ref()
            .map(|project| editor::axis_points(&project.program, self.timeline_axis))
            .unwrap_or_default();
        let points = self
            .gesture
            .as_ref()
            .map(|gesture| gesture.points.clone())
            .unwrap_or_else(|| committed.clone());
        let candidate = self
            .candidate
            .as_ref()
            .map(|candidate| editor::axis_points(&candidate.program, self.timeline_axis))
            .unwrap_or_default();
        let duration = self.timeline_duration_seconds();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 138.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, Color32::from_rgb(10, 15, 23));
        self.paint_review_spans(&painter, rect, duration);
        for index in 0..=4 {
            let y = rect.top() + rect.height() * index as f32 / 4.0;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0_f32, Color32::from_rgb(30, 42, 56)),
            );
        }
        let to_screen = |point: &EditorPoint| {
            egui::pos2(
                rect.left() + (point.nanos as f64 / 1e9 / duration) as f32 * rect.width(),
                rect.bottom() - point.position as f32 * rect.height(),
            )
        };
        let committed_gaps = self
            .project
            .as_ref()
            .and_then(|project| project.program.track(self.timeline_axis))
            .map(|track| track.gaps())
            .unwrap_or_default();
        let candidate_gaps = self
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.program.track(self.timeline_axis))
            .map(|track| track.gaps())
            .unwrap_or_default();
        for (line, gaps, color) in [
            (&candidate, candidate_gaps, Color32::from_rgb(221, 160, 54)),
            (&points, committed_gaps, Color32::from_rgb(57, 214, 227)),
        ] {
            for gap in gaps {
                let left = (gap.start().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
                let right = (gap.end().as_nanos() as f64 / 1e9 / duration).clamp(0.0, 1.0) as f32;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left() + left * rect.width(), rect.top()),
                        egui::pos2(rect.left() + right * rect.width(), rect.bottom()),
                    ),
                    0.0,
                    Color32::from_rgba_unmultiplied(190, 80, 96, 38),
                );
            }
            editor::available_segments(line, gaps, |segment| {
                painter.line_segment(
                    [to_screen(&segment[0]), to_screen(&segment[1])],
                    Stroke::new(1.7_f32, color),
                );
            });
            for point in line {
                if editor::point_is_available(point.nanos, gaps) {
                    painter.circle_filled(to_screen(point), 3.3, color);
                }
            }
        }
        if !committed_gaps.is_empty() || !candidate_gaps.is_empty() {
            painter.text(
                rect.left_top() + egui::vec2(5.0, 4.0),
                egui::Align2::LEFT_TOP,
                "Shaded gaps: motion unavailable; curves are clipped, not filled.",
                egui::FontId::proportional(11.0),
                Color32::from_rgb(240, 173, 185),
            );
        }
        let cursor_x = rect.left() + (self.playhead_seconds / duration) as f32 * rect.width();
        painter.line_segment(
            [
                egui::pos2(cursor_x, rect.top()),
                egui::pos2(cursor_x, rect.bottom()),
            ],
            Stroke::new(1.0_f32, Color32::WHITE),
        );
        if response.drag_started() && !self.busy {
            // The pointer can move beyond the hit radius before egui starts a drag.
            let press_origin = ui.input(|input| input.pointer.press_origin());
            if let (Some(pointer), Some(project)) = (press_origin, self.project.as_ref()) {
                let nearest = committed
                    .iter()
                    .enumerate()
                    .map(|(i, point)| (i, pointer.distance(to_screen(point))))
                    .filter(|(_, distance)| *distance <= 14.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((selected, _)) = nearest {
                    self.gesture = Some(EditGesture {
                        project_id: project.project_id.clone(),
                        base_revision: project.revision,
                        base_program: project.program.clone(),
                        axis: self.timeline_axis,
                        original_point: committed[selected],
                        points: committed,
                        selected,
                    });
                }
            }
        }
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let seconds =
                    ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * duration;
                if let Some(gesture) = &mut self.gesture {
                    if let Some(origin) = ui.input(|input| input.pointer.press_origin()) {
                        let delta = pointer - origin;
                        gesture.move_by(
                            (f64::from(delta.x / rect.width()) * duration * 1e9).round() as i64,
                            -f64::from(delta.y / rect.height()),
                            (duration * 1e9).round() as i64,
                        );
                    }
                } else {
                    self.seek(seconds);
                }
            }
        }
        if response.drag_stopped() {
            if let Some(gesture) = self.gesture.take() {
                match gesture.finish() {
                    Ok((command, project, revision)) => {
                        self.send_edit(command, project, revision);
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
        }
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                self.seek(
                    ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * duration,
                );
            }
        }
        ui.small("Drag a point: one atomic edit on release. Click elsewhere: independent preview seek. White = playhead; cyan = committed/draft gesture; amber = candidate.");
    }

    pub(super) fn consent_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Participant consent");
        ui.label("Consent document path");
        if ui.text_edit_singleline(&mut self.consent_path).changed() {
            self.consent_summary = None;
        }

        let path = PathBuf::from(self.consent_path.trim());
        let mut result = None;
        if ui.button("Inspect consent").clicked() {
            result = Some(crate::consent_status(&path, crate::consent_now_ns()));
        }
        if let Some(summary) = &self.consent_summary {
            ui.label(format!("Content class: {:?}", summary.content_class));
            ui.label(if summary.remote {
                "Provider scope: Remote"
            } else {
                "Provider scope: Local"
            });
            ui.label(if summary.attested {
                "Adult attestation recorded"
            } else {
                "Adult attestation absent or withdrawn"
            });
            if let Some(reason) = summary.effect_refusal {
                ui.label(format!("Effect eligibility refused: {reason}"));
            }
            if ui
                .add_enabled(summary.attested, egui::Button::new("Withdraw attestation"))
                .clicked()
            {
                let now = crate::consent_now_ns();
                result = Some(
                    crate::withdraw_attestation(&path, now)
                        .and_then(|_| crate::consent_status(&path, now)),
                );
            }
            for participant in &summary.participants {
                ui.horizontal(|ui| {
                    ui.label(&participant.reference);
                    if participant.revoked {
                        ui.label("Revoked");
                    } else if ui.button("Revoke consent").clicked() {
                        let now = crate::consent_now_ns();
                        result = Some((|| {
                            let id = ParticipantRef::new(&participant.reference)?;
                            crate::revoke_consent(&path, &id, now)?;
                            crate::consent_status(&path, now)
                        })());
                    }
                });
            }
        }
        if let Some(result) = result {
            match result {
                Ok(summary) => {
                    self.consent_summary = Some(summary);
                    self.status = "Consent status refreshed.".into();
                }
                Err(error) => {
                    self.consent_summary = None;
                    self.status = format!("Consent document: {error:#}");
                }
            }
        }
    }

    pub(super) fn capability_panel(&mut self, ui: &mut egui::Ui) {
        self.native_preparation_panel.show(
            ui,
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        if let Some(action) = self.handy_registration_panel.ui(ui, self.busy) {
            if !self.enqueue(
                RpcCommand::HandyRegistration(Box::new(action.clone())),
                None,
                None,
            ) {
                self.handy_registration_panel.finish(
                    &action,
                    &Err(self.status.clone()),
                    &Err("Action was not queued; refresh after reconnect".into()),
                );
            }
        }
        ui.separator();
        if let Some(inspection) = self.handy_panel.ui(ui, self.busy) {
            self.enqueue(RpcCommand::HandyInspect(inspection), None, None);
        }
        ui.separator();
        ui.heading("Capabilities and qualification");
        if let Some(capabilities) = &self.capabilities {
            ui.label(format!("Protocol {}", capabilities.protocol_version));
            for backend in &capabilities.backends {
                ui.label(format!(
                    "{}: {} / {:?}",
                    backend.name,
                    if backend.available {
                        "available"
                    } else {
                        "unavailable"
                    },
                    backend.qualification
                ));
                if let Some(reason) = &backend.reason {
                    ui.small(reason);
                }
            }
            ui.separator();
            ui.label("Engine operations");
            for operation in &capabilities.operations {
                ui.monospace(operation);
            }
        }
        ui.separator();
        ui.colored_label(Color32::YELLOW, "Physical playback is disabled here. Preview motion is not device admission or physical stopping evidence.");
        ui.add_enabled(
            false,
            egui::Button::new("Arm physical playback: qualification required"),
        );
        ui.label("Doctor offers read-only diagnostics and explicit neutral repair candidates. Handy connectivity/identity inspection is explicit and read-only. Broader hardware validation and native extensions remain migration gaps. Stash discovery, explicit batch generation and linking are available in the Stash tab.");
        self.assistant_panel.show(
            ui,
            self.project
                .as_ref()
                .map(|p| (p.project_id.clone(), p.revision)),
        );
        if let Some(candidate) = self.assistant_panel.take_edit_candidate() {
            if self.project.as_ref().is_some_and(|p| {
                p.project_id == candidate.project_id && p.revision == candidate.base_revision
            }) {
                self.status =
                    "Assistant edit candidate is ready for review and protected commit.".into();
                self.diagnostics = None;
                self.candidate = Some(candidate);
            }
        }
        if let Some(job) = self.assistant_panel.take_generation_job() {
            if self.project.as_ref().is_some_and(|project| {
                project.project_id == job.project_id && project.revision == job.base_revision
            }) {
                self.status = format!(
                    "Assistant-approved generation job {}: {}",
                    job.job_id, job.state
                );
                self.job = Some(job);
                self.last_job_poll = Instant::now();
            }
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Refresh capabilities"))
            .clicked()
        {
            self.send(Command::Capabilities, None, None);
        }
    }
}
