use super::*;

impl PulsarDesktop {
    pub(super) fn stash_tag_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Exact tag name");
        ui.text_edit_singleline(&mut self.stash_tag_name);
        ui.checkbox(&mut self.stash_create_tag, "Create the tag if absent");
        ui.label("Tag request ID (retain for reconciliation)");
        ui.text_edit_singleline(&mut self.stash_tag_request);
        let can_add = !self.busy
            && self.stash_scan.as_ref().is_some_and(|scan| {
                scan.state == StashScanState::VerifiedLinked
                    && self
                        .project
                        .as_ref()
                        .is_some_and(|p| p.project_id == scan.project_id)
            });
        if ui
            .add_enabled(
                can_add,
                egui::Button::new("Add tag to verified attached scene"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let scan = self
                    .stash_scan
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Reconcile the scene attachment first"))?;
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                let id = if self.stash_tag_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_tag_request.trim())?
                };
                let command = Command::StashAddTag {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: scan.request_id.clone(),
                    tag_name: self.stash_tag_name.clone(),
                    create: self.stash_create_tag,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, project.project_id.clone(), project.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_tag_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_tag_request.trim().is_empty(),
                egui::Button::new("Reconcile recorded tag creation or ADD"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashTagStatus {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    tag_request_id: RequestId::new(self.stash_tag_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.label("Resume request ID (separate from the original tag request)");
        ui.text_edit_singleline(&mut self.stash_tag_resume_request);
        let can_resume = !self.busy
            && self.project.is_some()
            && self
                .stash_tag
                .as_ref()
                .is_some_and(|tag| tag.state == StashTagState::Resolved && !tag.add_dispatched);
        if ui
            .add_enabled(
                can_resume,
                egui::Button::new("Attach resolved tag whose ADD was never dispatched"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                let id = if self.stash_tag_resume_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_tag_resume_request.trim())?
                };
                let command = Command::StashResumeTag {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    tag_request_id: RequestId::new(self.stash_tag_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, project.project_id.clone(), project.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_tag_resume_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(tag) = &self.stash_tag {
            ui.label(format!(
                "Tag: {:?}; ID: {}; ADD dispatched: {}",
                tag.state,
                tag.tag_id.as_deref().unwrap_or("unknown"),
                tag.add_dispatched
            ));
        }
        ui.small("Tag updates use ADD and preserve existing tags. Creation and attachment are separate outcomes; unknown effects require reconciliation rather than another dispatch.");
    }
    pub(super) fn stash_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stash library");
        ui.label("GraphQL endpoint");
        ui.text_edit_singleline(&mut self.stash_endpoint);
        ui.label("Credential environment variable name (optional)");
        ui.text_edit_singleline(&mut self.stash_credential_env);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Find scenes missing scripts"))
            .clicked()
        {
            match crate::project_package::read_local_owner_proof(
                &self.owner_directory,
                "stash-owner.token",
            ) {
                Ok(proof) => {
                    let connection = StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    };
                    self.send(
                        Command::StashListMissing {
                            connection,
                            limit: 100,
                            owner_proof: proof,
                        },
                        None,
                        None,
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.label("Folder mappings JSON file");
        ui.text_edit_singleline(&mut self.stash_mappings_path);
        ui.label("Per-source placement/interval plans JSON file (optional)");
        ui.text_edit_singleline(&mut self.stash_source_plans_path);
        ui.label("Per-source annotated views JSON file (optional)");
        ui.text_edit_singleline(&mut self.stash_annotated_views_path);
        ui.checkbox(
            &mut self.stash_require_selected_source,
            "Require capture to match selected project source",
        );
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Check mapped files before generation"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                let mappings: Vec<StashRootMapping> = serde_json::from_slice(&bytes)?;
                let proof = crate::project_package::read_local_owner_proof(
                    &self.owner_directory,
                    "stash-owner.token",
                )?;
                Ok(Command::StashPlanMissing {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    limit: 100,
                    mappings,
                    owner_proof: proof,
                })
            })();
            match command {
                Ok(command) => {
                    self.send(command, None, None);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.separator();
        ui.colored_label(Color32::YELLOW,"Generation uses the Generator model and preset settings. Scripts remain unqualified; unresolved motion gaps block publication.");
        ui.checkbox(&mut self.stash_batch_scan, "Scan successful publications");
        ui.checkbox(
            &mut self.stash_batch_tag_requested,
            "Apply the exact tag below after verified attachment",
        );
        ui.label("Engine batch ID (retain for recovery)");
        ui.text_edit_singleline(&mut self.stash_batch_id);
        if ui
            .add_enabled(
                !self.busy && self.stash_batch.is_none(),
                egui::Button::new("Generate and publish missing scripts"),
            )
            .clicked()
        {
            let config = (|| -> anyhow::Result<crate::stash_flow::BatchConfig> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                Ok(crate::stash_flow::BatchConfig {
                    source_plans: if self.stash_source_plans_path.trim().is_empty() {
                        vec![]
                    } else {
                        crate::inference::read_stash_source_plans(std::path::Path::new(
                            &self.stash_source_plans_path,
                        ))?
                    },
                    annotated_views: if self.stash_annotated_views_path.trim().is_empty() {
                        vec![]
                    } else {
                        crate::inference::read_stash_annotated_views(std::path::Path::new(
                            &self.stash_annotated_views_path,
                        ))?
                    },
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    mappings: serde_json::from_slice(&bytes)?,
                    limit: 100,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                    preset: self.preset.clone(),
                    model_path: self.selected_model()?,
                    model_pack: if self.use_model && self.use_selected_pack {
                        Some(self.pack_panel.generation_choice()?)
                    } else {
                        None
                    },
                    scan: self.stash_batch_scan || self.stash_batch_tag_requested,
                    tag: self.stash_batch_tag_requested.then(|| StashBatchTag {
                        name: self.stash_tag_name.clone(),
                        create: self.stash_create_tag,
                    }),
                    request_id: Some(if self.stash_batch_id.trim().is_empty() {
                        crate::session::new_request_id()
                    } else {
                        RequestId::new(self.stash_batch_id.trim())?
                    }),
                    detach: false,
                    settings: GenerationSettings {
                        detector_score_threshold: self.generation_detector_score_threshold(),
                        analysis_width: self.generation_analysis_dimensions().0,
                        analysis_height: self.generation_analysis_dimensions().1,
                        video_view: self.video_view.clone(),
                        inference_profile: self
                            .use_model
                            .then(|| self.generation_profile())
                            .flatten(),
                        model_input: self.model_contract(),
                        ..Default::default()
                    },
                })
            })();
            match config {
                Err(error) => self.status = error.to_string(),
                Ok(config) => {
                    self.stash_batch_id = config
                        .request_id
                        .as_ref()
                        .expect("batch ID set before submission")
                        .to_string();
                    let endpoint = self.batch_endpoint.clone();
                    let bootstrap = self.batch_bootstrap.clone();
                    let repaint = ui.ctx().clone();
                    let (sender, receiver) = mpsc::sync_channel(1);
                    self.stash_batch = Some(receiver);
                    self.stash_batch_report = None;
                    self.status="Stash generation running in separate projects. Closing this window does not cancel engine-owned jobs.".into();
                    std::thread::spawn(move || {
                        let result = (|| -> anyhow::Result<_> {
                            let mut client = SessionClient::connect(&endpoint, &bootstrap)?;
                            crate::stash_flow::generate_missing(&mut client, &config)
                        })()
                        .map_err(|error| error.to_string());
                        let _ = sender.send(result);
                        repaint.request_repaint();
                    });
                }
            }
        }
        if self.stash_batch.is_some() {
            ui.spinner();
            ui.label("Engine batch running. Closing this window does not cancel scheduling or engine-owned jobs.");
        }
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("Read batch status", 0),
                ("Resume recorded batch", 1),
                ("Cancel future batch effects", 2),
            ] {
                if ui
                    .add_enabled(
                        !self.busy && !self.stash_batch_id.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    let command = (|| -> anyhow::Result<Command> {
                        let id = RequestId::new(self.stash_batch_id.trim())?;
                        let proof = crate::project_package::read_local_owner_proof(
                            &self.owner_directory,
                            "stash-owner.token",
                        )?;
                        Ok(match kind {
                            0 => Command::StashBatchStatus {
                                batch_request_id: id,
                                offset: self.stash_batch_offset,
                                owner_proof: proof,
                            },
                            1 => Command::StashResumeBatch {
                                batch_request_id: id,
                                owner_proof: proof,
                            },
                            _ => Command::StashCancelBatch {
                                batch_request_id: id,
                                owner_proof: proof,
                            },
                        })
                    })();
                    match command {
                        Ok(command) => {
                            self.send(command, None, None);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            for (label, offset, enabled) in [
                (
                    "Previous five items",
                    self.stash_batch_offset.saturating_sub(5),
                    self.stash_batch_offset > 0,
                ),
                (
                    "Next five items",
                    self.stash_batch_offset.saturating_add(5),
                    self.stash_batch_has_more,
                ),
            ] {
                if ui
                    .add_enabled(!self.busy && enabled, egui::Button::new(label))
                    .clicked()
                {
                    let command = (|| -> anyhow::Result<Command> {
                        Ok(Command::StashBatchStatus {
                            batch_request_id: RequestId::new(self.stash_batch_id.trim())?,
                            offset,
                            owner_proof: crate::project_package::read_local_owner_proof(
                                &self.owner_directory,
                                "stash-owner.token",
                            )?,
                        })
                    })();
                    match command {
                        Ok(command) => {
                            self.send(command, None, None);
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            ui.small(format!(
                "{} selected items; page starts at {}",
                self.stash_batch_item_count, self.stash_batch_offset
            ));
        });
        if let Some(report) = &self.stash_batch_report {
            ui.collapsing("Batch outcomes and reconciliation IDs", |ui| {
                ui.monospace(report);
            });
        }
        ui.label("Publication request ID (retain for uncertain outcomes)");
        ui.text_edit_singleline(&mut self.stash_publication_request);
        ui.checkbox(
            &mut self.stash_source_time,
            "Map placed motion to original source time for publication",
        );
        ui.checkbox(
            &mut self.stash_use_archived,
            "Use verified archived generation (no live job)",
        );
        if self.stash_use_archived {
            if let Some(report) = self.archived_generation.as_ref().filter(|r| {
                self.project
                    .as_ref()
                    .is_some_and(|p| p.project_id == r.project_id)
                    && self
                        .candidate
                        .as_ref()
                        .is_some_and(|c| c.candidate_id == r.candidate_id)
            }) {
                egui::ComboBox::from_id_salt("stash-archived-generation")
                    .selected_text(if self.stash_archived_namespace.is_empty() {
                        "Select original namespace"
                    } else {
                        &self.stash_archived_namespace
                    })
                    .show_ui(ui, |ui| {
                        for origin in &report.origins {
                            ui.selectable_value(
                                &mut self.stash_archived_namespace,
                                origin.origin_manifest.sha256.clone(),
                                format!(
                                    "{} / historical job {}",
                                    origin.origin_manifest.sha256, origin.job_id
                                ),
                            );
                        }
                    });
            }
            ui.label("Verify current master ancestry in the candidate panel, select its original namespace, and freshly bind the current Stash source before publication.");
        }
        if ui
            .add_enabled(
                !self.busy && self.stash_capture.is_some() && self.candidate.is_some(),
                egui::Button::new("Publish current committed motion"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                let capture = self
                    .stash_capture
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Capture a Stash source first"))?;
                let candidate = self
                    .candidate
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Select a generated candidate"))?;
                let (job_id, archived_origin) = if self.stash_use_archived {
                    let report = self.archived_generation.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Verify archived generation and current master ancestry first"
                        )
                    })?;
                    let project = self
                        .project
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Select project"))?;
                    anyhow::ensure!(
                        report.project_id == project.project_id
                            && report.candidate_id == candidate.candidate_id
                            && report
                                .master_derivation
                                .as_ref()
                                .is_some_and(|m| m.revision == project.revision
                                    && m.master.sha256 == project.motion.program.sha256),
                        "Archived ancestry report is absent or stale"
                    );
                    let origin = report
                        .origins
                        .iter()
                        .find(|o| o.origin_manifest.sha256 == self.stash_archived_namespace)
                        .ok_or_else(|| anyhow::anyhow!("Select an inspected original namespace"))?;
                    (origin.job_id.clone(), Some(origin.origin_manifest.clone()))
                } else {
                    (candidate.job_id.clone().ok_or_else(||anyhow::anyhow!("Stash publication requires a generation job or explicitly selected archival evidence"))?,None)
                };
                Ok(Command::StashExportScript {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    source_version: capture.source_version.clone(),
                    scene_id: capture.scene_id.clone(),
                    file_id: capture.file_id.clone(),
                    candidate_id: candidate.candidate_id.clone(),
                    job_id,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                    source_time: self.stash_source_time,
                    archived_origin,
                })
            })();
            match command {
                Ok(command) => {
                    let id = if self.stash_publication_request.trim().is_empty() {
                        Ok(crate::session::new_request_id())
                    } else {
                        RequestId::new(self.stash_publication_request.trim())
                    };
                    match (id, self.project.as_ref()) {
                        (Ok(id), Some(p)) => {
                            let project = p.project_id.clone();
                            let revision = p.revision;
                            self.stash_publication_request = id.to_string();
                            self.enqueue(
                                RpcCommand::Identified(id, command),
                                Some(project),
                                Some(revision),
                            );
                        }
                        (Err(error), _) => self.status = error.to_string(),
                        _ => self.status = "Open a project first".into(),
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && !self.stash_publication_request.trim().is_empty(),
                egui::Button::new("Observe publication status"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<_> {
                Ok(Command::StashPublicationStatus {
                    publication_request_id: RequestId::new(self.stash_publication_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(report) = &self.stash_publication_report {
            ui.collapsing("Publication observation", |ui| {
                ui.monospace(report);
            });
        }
        if let Some(export) = &self.stash_export {
            ui.monospace(export.path.display().to_string());
        }
        ui.separator();
        ui.label("Scan request ID (retain for reconciliation)");
        ui.text_edit_singleline(&mut self.stash_scan_request);
        if ui
            .add_enabled(
                !self.busy
                    && self
                        .stash_export
                        .as_ref()
                        .is_some_and(|e| e.publication_request_id.is_some()),
                egui::Button::new("Submit targeted scan for current publication"),
            )
            .clicked()
        {
            let work = (|| -> anyhow::Result<_> {
                let publication = self
                    .stash_export
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Publish a script first"))?;
                let snapshot = self
                    .project
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Open the publication project"))?;
                if snapshot.project_id != publication.project_id {
                    anyhow::bail!("Open the publication project before scanning");
                }
                let id = if self.stash_scan_request.trim().is_empty() {
                    crate::session::new_request_id()
                } else {
                    RequestId::new(self.stash_scan_request.trim())?
                };
                let command = Command::StashSubmitScan {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    publication_request_id: publication.publication_request_id.clone().ok_or_else(
                        || anyhow::anyhow!("Legacy publication lacks its request ID"),
                    )?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                };
                Ok((id, command, snapshot.project_id.clone(), snapshot.revision))
            })();
            match work {
                Ok((id, command, project, revision)) => {
                    self.stash_scan_request = id.to_string();
                    self.enqueue(
                        RpcCommand::Identified(id, command),
                        Some(project),
                        Some(revision),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_scan_request.trim().is_empty(),
                egui::Button::new("Reconcile recorded scan and attachment"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashScanStatus {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: RequestId::new(self.stash_scan_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if ui
            .add_enabled(
                !self.busy && self.project.is_some() && !self.stash_scan_request.trim().is_empty(),
                egui::Button::new("Observe attachment without a scan job ID"),
            )
            .clicked()
        {
            let command = (|| -> anyhow::Result<Command> {
                Ok(Command::StashReconcileAttachment {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scan_request_id: RequestId::new(self.stash_scan_request.trim())?,
                    owner_proof: crate::project_package::read_local_owner_proof(
                        &self.owner_directory,
                        "stash-owner.token",
                    )?,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, false);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        if let Some(scan) = &self.stash_scan {
            ui.label(format!(
                "Scan: {:?}; remote job: {}",
                scan.state,
                scan.remote_job_id.as_deref().unwrap_or("unknown")
            ));
            if let Some(observation) = &scan.attachment {
                ui.label(format!(
                    "Attachment observation: {:?} at {} ms UTC; remote script hash unverified",
                    observation.state, observation.observed_at_unix_ms
                ));
            }
        }
        ui.small("Submit records dispatch before contacting Stash. Unknown acknowledgements are never automatically resubmitted. Reconciliation needs the same authenticated session and publication project.");

        self.stash_tag_controls(ui);
        ui.small("Mappings are a JSON array of server_root and local_root pairs. Existing scripts and symlink paths block automatic generation.");
        let mut selected_capture = None;
        if let Some(plan) = &self.stash_plan {
            for file in &plan.files {
                ui.label(format!("Scene {}: {:?}", file.scene_id, file.status));
                if let Some(source) = &file.local_source {
                    ui.monospace(source.display().to_string());
                }
                if file.status == StashFileStatus::ReadyForSnapshot
                    && ui
                        .add_enabled(
                            !self.busy && self.project.is_some(),
                            egui::Button::new("Capture source in current project"),
                        )
                        .clicked()
                {
                    selected_capture = file
                        .file_id
                        .as_ref()
                        .map(|id| (file.scene_id.clone(), id.clone()));
                }
            }
        }
        if let Some((scene_id, file_id)) = selected_capture {
            let command = (|| -> anyhow::Result<Command> {
                let bytes = crate::inference::read_bounded_profile(std::path::Path::new(
                    &self.stash_mappings_path,
                ))?;
                let mappings = serde_json::from_slice(&bytes)?;
                let owner_proof = crate::project_package::read_local_owner_proof(
                    &self.owner_directory,
                    "stash-owner.token",
                )?;
                Ok(Command::StashImportSource {
                    connection: StashConnection {
                        endpoint: self.stash_endpoint.clone(),
                        credential_env: (!self.stash_credential_env.trim().is_empty())
                            .then(|| self.stash_credential_env.trim().to_owned()),
                    },
                    scene_id,
                    file_id,
                    expected_server_path: None,
                    expected_source_version: if self.stash_require_selected_source {
                        Some(self.source.clone().ok_or_else(|| {
                            anyhow::anyhow!("Select the existing project source to rebind")
                        })?)
                    } else {
                        None
                    },
                    allow_multifile_selection: None,
                    mappings,
                    owner_proof,
                })
            })();
            match command {
                Ok(command) => {
                    self.project_command(command, true);
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        ui.small("Discovery sends a metadata query to the selected server. Credentials must already be in the engine environment. Scripts are not generated, overwritten or scanned by discovery.");
        if let Some(discovery) = &self.stash_discovery {
            ui.label(format!(
                "{} scenes returned; server count {} (not a transaction snapshot).",
                discovery.scenes.len(),
                discovery.reported_count
            ));
            for scene in &discovery.scenes {
                ui.separator();
                ui.label(format!("{}: {}", scene.id, scene.title));
                if scene.files.len() != 1 {
                    ui.colored_label(
                        Color32::YELLOW,
                        "Ambiguous or missing video files: explicit file selection is required.",
                    );
                }
                for file in &scene.files {
                    ui.monospace(&file.path);
                }
            }
        }
    }
}
