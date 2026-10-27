//! View state for host-owner pack operations; execution stays in the engine.
use eframe::egui;
use pulsar_protocol::*;

type PackIntent = crate::pack_recovery::PackRecoveryIntent;

pub(crate) enum PackAction {
    Import,
    Integrity,
    Refresh,
    Select,
    Deactivate,
    Withdraw,
    Replay,
    Dismiss,
    CatalogRefresh,
    CatalogStatus,
    CatalogCheck,
    CatalogSelect,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackReplyKind {
    Integrity,
    Current,
    Mutation,
    Withdrawal,
    CatalogCurrent,
    CatalogMutation,
    Association,
}
pub(crate) struct PackSubmission {
    pub command: Command,
    pub id: Option<RequestId>,
    pub reply: PackReplyKind,
    intent: Option<PackIntent>,
}
#[derive(Default)]
pub(crate) struct PackPanel {
    catalog_source: String,
    catalog_bootstrap: String,
    catalog_digest: String,
    catalog_revision: u64,
    approve_bootstrap: bool,
    catalog: Option<ModelCatalogReport>,
    catalog_history: Option<ModelCatalogReport>,
    association: Option<ModelPackCatalogReport>,
    expected_catalog: Option<(String, Option<String>, Option<u64>)>,
    source: String,
    digest: String,
    approve: bool,
    include_unclassified_generation: bool,
    pub selection: Option<ModelPackSelectionReport>,
    integrity: Option<ModelPackReport>,
    historical: Option<ModelPackSelectionReport>,
    pub message: String,
    pending: Option<PackReplyKind>,
    recovery: Option<(RequestId, PackIntent)>,
    recoveries: Vec<crate::pack_recovery::PackRecoveryEntry>,
}
impl PackPanel {
    pub(crate) fn prepare_bundled_import(
        &mut self,
        pack: &crate::bundled_models::BundledPack,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.pending.is_none() && self.recovery.is_none(),
            "Resolve the pending pack operation first"
        );
        validate_sha256(&pack.manifest_sha256)?;
        pack.manifest.validate()?;
        self.source = pack
            .source
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Pack source path is not UTF-8"))?
            .to_owned();
        self.digest = pack.manifest_sha256.clone();
        self.approve = false;
        self.integrity = None;
        self.association = None;
        self.message =
            "Bundled path prepared. Import and execution approval are separate actions.".into();
        Ok(())
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<PackAction> {
        let mut action = None;
        egui::CollapsingHeader::new("Offline model packs").default_open(true).show(ui,|ui| {
            ui.label("Pack directory");ui.text_edit_singleline(&mut self.source);
            ui.label("Installed manifest SHA-256");if ui.text_edit_singleline(&mut self.digest).changed(){self.approve=false;self.integrity=None;self.association=None;}
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy&&!self.source.trim().is_empty(),egui::Button::new("Import inert pack")).clicked(){action=Some(PackAction::Import);}
                if ui.add_enabled(!busy&&!self.digest.trim().is_empty(),egui::Button::new("Check installed bytes")).clicked(){action=Some(PackAction::Integrity);}
                if ui.add_enabled(!busy,egui::Button::new("Refresh owner selection")).clicked(){action=Some(PackAction::Refresh);}
            });
            if let Some(report)=&self.integrity {
                ui.label(format!("Integrity receipt for {}: {} assets, {} bytes. Execution approval is separate.",report.manifest_sha256,report.asset_count,report.total_bytes));
            }
            if let Some(report)=&self.selection {
                ui.label(format!("Current recorded selection revision: {}",report.selection_revision));
                if let Some(choice)=&report.choice {
                    ui.label(format!("Selected: {}",choice.manifest_sha256));
                    if let Some(binding)=&report.catalog_binding {ui.label(format!("Catalog-bound approval: {} at revision {}. Admission checks current metadata; changes require reapproval.",binding.bootstrap_sha256,binding.catalog_revision));}
                    ui.label("Owner approved future offline analysis and generation. Model quality, rights and publisher trust remain unverified.");
                } else {ui.label("No pack selected for future analysis or generation.");}
            } else {ui.label("Refresh the owner selection before selecting or deactivating a pack.");}
            ui.add_enabled(!busy,egui::Checkbox::new(&mut self.approve,"I approve this unqualified model for future offline analysis and generation through the pinned declared runtime"));
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy&&self.selection.is_some()&&self.approve&&!self.digest.trim().is_empty(),egui::Button::new("Approve selection")).clicked(){action=Some(PackAction::Select);}
                if ui.add_enabled(!busy&&self.selection.as_ref().is_some_and(|s|s.choice.is_some()),egui::Button::new("Deactivate future analysis and generation")).clicked(){action=Some(PackAction::Deactivate);}
            });
            ui.add_enabled(!busy,egui::Checkbox::new(&mut self.include_unclassified_generation,"Also cancel older generation with no retained admission classification"));
            if ui.add_enabled(!busy&&self.selection.as_ref().is_some_and(|s|s.choice.is_some()),egui::Button::new("Withdraw approval and cancel its unfinished generation")).clicked(){action=Some(PackAction::Withdraw);}
            egui::CollapsingHeader::new("Offline publisher catalogs").default_open(true).show(ui,|ui| {
                ui.label("Captured catalog directory");ui.add_enabled(!busy,egui::TextEdit::singleline(&mut self.catalog_source));
                ui.label("Approved public bootstrap file");if ui.add_enabled(!busy,egui::TextEdit::singleline(&mut self.catalog_bootstrap)).changed(){self.approve_bootstrap=false;}
                ui.label("Bootstrap SHA-256");if ui.add_enabled(!busy,egui::TextEdit::singleline(&mut self.catalog_digest)).changed(){self.catalog=None;self.association=None;self.approve_bootstrap=false;self.approve=false;}
                ui.horizontal(|ui|{ui.label("Expected catalog revision (0 for first intake)");ui.add_enabled(!busy,egui::DragValue::new(&mut self.catalog_revision));});
                ui.add_enabled(!busy,egui::Checkbox::new(&mut self.approve_bootstrap,"I independently approve this exact bootstrap for catalog intake"));
                ui.label("Signed metadata authenticates declarations under the approved root. It does not establish official publisher identity, rights, model quality or online revocation freshness.");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy&&self.approve_bootstrap,egui::Button::new("Accept captured catalog")).clicked(){action=Some(PackAction::CatalogRefresh);}
                    if ui.add_enabled(!busy&&!self.catalog_digest.trim().is_empty(),egui::Button::new("Read recorded catalog status")).clicked(){action=Some(PackAction::CatalogStatus);}
                });
                if let Some(report)=&self.catalog {ui.label(format!("Recorded historical catalog: revision {}, root {}. Status does not renew expiry.",report.catalog_revision,report.root_version));}
                if let Some(report)=&self.catalog_history {ui.label(format!("Last catalog operation receipt: revision {}. Current status is queried separately.",report.catalog_revision));}
                if let Some(report)=&self.association {ui.label(format!("Installed manifest {} listed in catalog revision {} at {} ms; metadata deadline {} ms. This point-in-time check grants no execution approval.",report.pack.manifest_sha256,report.catalog.catalog_revision,report.checked_at_unix_ms,report.metadata_expires_at_unix_ms));}
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy&&self.catalog.is_some()&&!self.digest.trim().is_empty(),egui::Button::new("Check pack against catalog")).clicked(){action=Some(PackAction::CatalogCheck);}
                    if ui.add_enabled(!busy&&self.catalog.is_some()&&self.selection.is_some()&&self.approve&&!self.digest.trim().is_empty(),egui::Button::new("Approve catalog-bound selection")).clicked(){action=Some(PackAction::CatalogSelect);}
                });
            });
            for entry in &self.recoveries {
                if ui.selectable_label(self.recovery.as_ref().is_some_and(|(id,_)|*id==entry.id),format!("{} — {:?} — {}",entry.id,entry.intent,if entry.acknowledged{"receipt acknowledged"}else{"unacknowledged"})).clicked(){self.recovery=Some((entry.id.clone(),entry.intent.clone()));}
            }
            if let Some((id,_))=&self.recovery {
                ui.label(format!("Recovery ID: {id}"));
                ui.label("Replay uses the exact original operation. A historical receipt does not restore an old selection.");
                if ui.add_enabled(!busy,egui::Button::new("Replay retained pack operation")).clicked(){action=Some(PackAction::Replay);}
                if ui.add_enabled(!busy&&self.recoveries.iter().any(|entry|entry.id==*id&&entry.acknowledged),egui::Button::new("Dismiss acknowledged recovery entry")).clicked(){action=Some(PackAction::Dismiss);}
            }
            if let Some(report)=&self.historical {ui.label(format!("Last operation receipt: selection revision {}. Current status is queried separately.",report.selection_revision));}
            if !self.message.is_empty(){ui.label(&self.message);}
        });
        action
    }
    pub fn restore_recovery(&mut self, entries: Vec<crate::pack_recovery::PackRecoveryEntry>) {
        if !self
            .recovery
            .as_ref()
            .is_some_and(|(id, _)| entries.iter().any(|entry| entry.id == *id))
        {
            self.recovery = entries
                .last()
                .map(|entry| (entry.id.clone(), entry.intent.clone()));
        }
        self.recoveries = entries;
    }
    pub fn dismissable_id(&self) -> anyhow::Result<RequestId> {
        let (id, _) = self
            .recovery
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No retained recovery entry"))?;
        anyhow::ensure!(
            self.recoveries
                .iter()
                .any(|entry| entry.id == *id && entry.acknowledged),
            "Only acknowledged recovery entries can be dismissed"
        );
        Ok(id.clone())
    }
    pub fn prepare(&self, action: PackAction, proof: String) -> anyhow::Result<PackSubmission> {
        let (reply, intent, id, command) = match action {
            PackAction::Dismiss => {
                anyhow::bail!("Dismissal uses the session-bound local recovery store")
            }
            PackAction::Refresh => (
                PackReplyKind::Current,
                None,
                None,
                Command::ModelPackSelectionStatus { owner_proof: proof },
            ),
            PackAction::CatalogStatus => {
                let digest = self.catalog_digest.trim().to_owned();
                validate_sha256(&digest)?;
                (
                    PackReplyKind::CatalogCurrent,
                    None,
                    None,
                    Command::ModelCatalogStatus {
                        bootstrap_sha256: digest,
                        owner_proof: proof,
                    },
                )
            }
            PackAction::CatalogCheck => {
                let catalog = self
                    .catalog
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Read recorded catalog status first"))?;
                anyhow::ensure!(
                    catalog.bootstrap_sha256 == self.catalog_digest.trim(),
                    "Catalog namespace changed"
                );
                let digest = self.digest.trim().to_owned();
                validate_sha256(&digest)?;
                (
                    PackReplyKind::Association,
                    None,
                    None,
                    Command::CheckModelPackCatalog {
                        bootstrap_sha256: catalog.bootstrap_sha256.clone(),
                        manifest_sha256: digest,
                        expected_catalog_revision: catalog.catalog_revision,
                        owner_proof: proof,
                    },
                )
            }
            PackAction::Integrity => {
                let digest = self.digest.trim().to_owned();
                validate_sha256(&digest)?;
                (
                    PackReplyKind::Integrity,
                    None,
                    None,
                    Command::ModelPackStatus {
                        manifest_sha256: digest,
                        owner_proof: proof,
                    },
                )
            }
            other => {
                let (id, intent) = if matches!(other, PackAction::Replay) {
                    self.recovery
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("No retained pack operation to replay"))?
                } else {
                    let intent = match other {
                        PackAction::CatalogRefresh => {
                            anyhow::ensure!(
                                self.approve_bootstrap,
                                "Explicit independent bootstrap approval is required"
                            );
                            let digest = self.catalog_digest.trim().to_owned();
                            validate_sha256(&digest)?;
                            anyhow::ensure!(
                                !self.catalog_source.trim().is_empty()
                                    && !self.catalog_bootstrap.trim().is_empty(),
                                "Catalog directory and bootstrap file are required"
                            );
                            PackIntent::CatalogRefresh {
                                source: crate::absolute_path(std::path::Path::new(
                                    self.catalog_source.trim(),
                                ))?,
                                bootstrap: crate::absolute_path(std::path::Path::new(
                                    self.catalog_bootstrap.trim(),
                                ))?,
                                bootstrap_sha256: digest,
                                expected_revision: self.catalog_revision,
                            }
                        }
                        PackAction::CatalogSelect => {
                            anyhow::ensure!(
                                self.approve,
                                "Explicit unqualified execution approval is required"
                            );
                            let catalog = self.catalog.as_ref().ok_or_else(|| {
                                anyhow::anyhow!("Read recorded catalog status first")
                            })?;
                            anyhow::ensure!(
                                catalog.bootstrap_sha256 == self.catalog_digest.trim(),
                                "Catalog namespace changed"
                            );
                            let digest = self.digest.trim().to_owned();
                            validate_sha256(&digest)?;
                            PackIntent::CatalogSelect {
                                manifest_sha256: digest,
                                binding: ModelCatalogBinding {
                                    bootstrap_sha256: catalog.bootstrap_sha256.clone(),
                                    catalog_revision: catalog.catalog_revision,
                                },
                                expected_revision: self
                                    .selection
                                    .as_ref()
                                    .ok_or_else(|| {
                                        anyhow::anyhow!("Refresh owner selection first")
                                    })?
                                    .selection_revision,
                            }
                        }
                        PackAction::Import => PackIntent::Import(crate::absolute_path(
                            std::path::Path::new(self.source.trim()),
                        )?),
                        PackAction::Select => {
                            anyhow::ensure!(
                                self.approve,
                                "Explicit unqualified execution approval is required"
                            );
                            let digest = self.digest.trim().to_owned();
                            validate_sha256(&digest)?;
                            PackIntent::Select {
                                manifest_sha256: digest,
                                expected_revision: self
                                    .selection
                                    .as_ref()
                                    .ok_or_else(|| {
                                        anyhow::anyhow!("Refresh the owner selection first")
                                    })?
                                    .selection_revision,
                            }
                        }
                        PackAction::Withdraw => {
                            let selection = self.selection.as_ref().ok_or_else(|| {
                                anyhow::anyhow!("Refresh current selection first")
                            })?;
                            PackIntent::Withdraw {
                                choice: selection
                                    .choice
                                    .clone()
                                    .ok_or_else(|| anyhow::anyhow!("No active approval"))?,
                                expected_revision: selection.selection_revision,
                                include_unclassified_generation: self
                                    .include_unclassified_generation,
                            }
                        }
                        PackAction::Deactivate => PackIntent::Deactivate {
                            expected_revision: self
                                .selection
                                .as_ref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Refresh the owner selection first")
                                })?
                                .selection_revision,
                        },
                        _ => unreachable!(),
                    };
                    (crate::session::new_request_id(), intent)
                };
                let command = intent.command(proof);
                (
                    if matches!(intent, PackIntent::Withdraw { .. }) {
                        PackReplyKind::Withdrawal
                    } else if matches!(intent, PackIntent::CatalogRefresh { .. }) {
                        PackReplyKind::CatalogMutation
                    } else {
                        PackReplyKind::Mutation
                    },
                    Some(intent),
                    Some(id),
                    command,
                )
            }
        };
        Ok(PackSubmission {
            command,
            id,
            reply,
            intent,
        })
    }
    pub fn submitted(&mut self, submission: &PackSubmission) {
        self.pending = Some(submission.reply);
        self.expected_catalog = match &submission.command {
            Command::RefreshModelCatalog {
                bootstrap_sha256,
                expected_catalog_revision,
                ..
            } => Some((
                bootstrap_sha256.clone(),
                None,
                expected_catalog_revision.checked_add(1),
            )),
            Command::ModelCatalogStatus {
                bootstrap_sha256, ..
            } => Some((bootstrap_sha256.clone(), None, None)),
            Command::CheckModelPackCatalog {
                bootstrap_sha256,
                manifest_sha256,
                expected_catalog_revision,
                ..
            } => Some((
                bootstrap_sha256.clone(),
                Some(manifest_sha256.clone()),
                Some(*expected_catalog_revision),
            )),
            _ => None,
        };
        if let (Some(id), Some(intent)) = (&submission.id, &submission.intent) {
            self.recovery = Some((id.clone(), intent.clone()));
        }
        self.approve = false;
        self.approve_bootstrap = false;
        self.association = None;
        if matches!(
            submission.reply,
            PackReplyKind::CatalogMutation | PackReplyKind::CatalogCurrent
        ) {
            self.catalog = None;
        }
        if matches!(
            submission.reply,
            PackReplyKind::Current
                | PackReplyKind::Mutation
                | PackReplyKind::CatalogMutation
                | PackReplyKind::Withdrawal
        ) {
            self.selection = None;
        }
        self.message =
            "Pack request pending. Retain its recovery ID until the outcome is known.".into();
    }
    /// Only an explicit current-status query can populate the generation choice.
    pub fn accept_withdrawal(&mut self, report: ModelPackWithdrawalReport) -> anyhow::Result<bool> {
        report.validate()?;
        anyhow::ensure!(
            self.pending == Some(PackReplyKind::Withdrawal),
            "Unexpected withdrawal response"
        );
        let (_, intent) = self
            .recovery
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Withdrawal intent absent"))?;
        let PackIntent::Withdraw {
            choice,
            expected_revision,
            include_unclassified_generation,
        } = intent
        else {
            anyhow::bail!("Withdrawal intent changed");
        };
        anyhow::ensure!(
            &report.choice == choice
                && report.selection.selection_revision
                    == if choice.selection_revision == *expected_revision {
                        expected_revision.saturating_add(1)
                    } else {
                        *expected_revision
                    }
                && (*include_unclassified_generation || report.cancelled_unclassified_jobs == 0),
            "Withdrawal receipt differs from retained intent"
        );
        self.pending = None;
        self.selection = None;
        self.include_unclassified_generation = false;
        self.message=format!("Withdrawal receipt: {} approved jobs and {} unclassified jobs cancelled. Worker retirement remains independent. Refreshing current selection.",report.cancelled_approved_jobs,report.cancelled_unclassified_jobs);
        self.historical = Some(report.selection);
        Ok(true)
    }
    pub fn accept_selection(&mut self, report: ModelPackSelectionReport) -> anyhow::Result<bool> {
        report.validate()?;
        let pending = self.pending.take();
        match pending {
            Some(PackReplyKind::Current) => {
                self.selection = Some(report);
                self.message =
                    "Recorded selection refreshed; generation rechecks installed bytes.".into();
                Ok(false)
            }
            Some(PackReplyKind::Mutation) => {
                self.historical = Some(report);
                self.selection = None;
                self.message = "Operation receipt retained. Refreshing current selection.".into();
                Ok(true)
            }
            _ => anyhow::bail!("Unexpected pack selection response; refresh current status"),
        }
    }
    pub fn accept_integrity(&mut self, report: ModelPackReport) -> anyhow::Result<bool> {
        report.validate()?;
        let pending = self.pending.take();
        anyhow::ensure!(
            matches!(
                pending,
                Some(PackReplyKind::Mutation | PackReplyKind::Integrity)
            ),
            "Unexpected pack integrity response"
        );
        self.digest = report.manifest_sha256.clone();
        self.integrity = Some(report);
        self.message =
            "Inert pack integrity receipt retained. Selection requires separate explicit approval."
                .into();
        Ok(pending == Some(PackReplyKind::Mutation))
    }
    pub fn accept_catalog(&mut self, report: ModelCatalogReport) -> anyhow::Result<bool> {
        report.validate()?;
        let (namespace, _, revision) = self
            .expected_catalog
            .take()
            .ok_or_else(|| anyhow::anyhow!("Unexpected catalog receipt"))?;
        anyhow::ensure!(
            namespace == report.bootstrap_sha256
                && revision.is_none_or(|v| v == report.catalog_revision),
            "Catalog receipt differs from submitted intent"
        );
        match self.pending.take() {
            Some(PackReplyKind::CatalogCurrent) => {
                anyhow::ensure!(
                    self.catalog_digest.trim() == namespace,
                    "Catalog inputs changed; reread status"
                );
                self.catalog_revision = report.catalog_revision;
                self.catalog = Some(report);
                self.message =
                    "Historical catalog status refreshed; admission checks fresh metadata.".into();
                Ok(false)
            }
            Some(PackReplyKind::CatalogMutation) => {
                self.catalog_digest = namespace;
                self.catalog_history = Some(report);
                self.catalog = None;
                self.message =
                    "Historical catalog operation receipt retained. Reading current status.".into();
                Ok(true)
            }
            _ => anyhow::bail!("Unexpected catalog response"),
        }
    }
    pub fn accept_association(&mut self, report: ModelPackCatalogReport) -> anyhow::Result<()> {
        report.validate()?;
        let (namespace, manifest, revision) = self
            .expected_catalog
            .take()
            .ok_or_else(|| anyhow::anyhow!("Unexpected catalog association"))?;
        anyhow::ensure!(
            self.pending.take() == Some(PackReplyKind::Association)
                && namespace == report.catalog.bootstrap_sha256
                && manifest.as_ref() == Some(&report.pack.manifest_sha256)
                && revision == Some(report.catalog.catalog_revision),
            "Association differs from submitted intent"
        );
        anyhow::ensure!(
            self.catalog_digest.trim() == namespace
                && manifest.as_deref() == Some(self.digest.trim()),
            "Association inputs changed; recheck the requested pack"
        );
        self.association = Some(report);
        self.message="Point-in-time signed association and installed integrity checked; execution approval remains separate.".into();
        Ok(())
    }
    pub fn failed(&mut self) {
        if self.pending.take().is_some() {
            self.selection = None;
            self.message="Pack request failed or its outcome is unknown. Replay the retained operation or refresh current status.".into();
        }
    }
    pub fn generation_choice(&self) -> anyhow::Result<ModelPackChoice> {
        let report = self.selection.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Refresh the current owner selection before generation")
        })?;
        report.validate()?;
        report.choice.clone().ok_or_else(|| {
            anyhow::anyhow!("No offline pack is selected; explicitly approve one first")
        })
    }
    pub fn profile(&self) -> Option<DeclaredInferenceProfile> {
        self.selection.as_ref().and_then(|r| r.profile.clone())
    }
    pub fn generation_command(
        &self,
        source_version: SourceVersionId,
        preset: String,
        mut settings: GenerationSettings,
    ) -> anyhow::Result<Command> {
        let choice = self.generation_choice()?;
        let profile = self
            .profile()
            .ok_or_else(|| anyhow::anyhow!("Selected pack lacks a declared profile"))?;
        if let Some(input) = &settings.model_input {
            anyhow::ensure!(
                serde_json::to_value(input)? == serde_json::to_value(&profile.preprocessing.input)?,
                "Generation input conflicts with selected pack"
            );
        }
        if let Some(selected) = &settings.inference_profile {
            anyhow::ensure!(
                serde_json::to_value(selected)? == serde_json::to_value(&profile)?,
                "Generation profile conflicts with selected pack"
            );
        }
        settings.model_input = Some(profile.preprocessing.input.clone());
        settings.inference_profile = Some(profile);
        settings.validate_execution_preset(&preset, true)?;
        Ok(Command::GenerateFromModelPack {
            source_version,
            preset,
            settings: Some(settings),
            choice,
        })
    }

    pub fn matches_preview(&self, report: &ModelPackPreviewResult) -> bool {
        use sha2::{Digest, Sha256};
        if report.validate().is_err()
            || self.generation_choice().ok().as_ref() != Some(&report.choice)
        {
            return false;
        }
        let Some(profile) = self.profile() else {
            return false;
        };
        let Ok(bytes) = serde_json::to_vec(&profile) else {
            return false;
        };
        report.model == profile.model
            && report.profile_sha256 == format!("{:x}", Sha256::digest(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_preparation_resets_approval_without_importing_or_changing_selection() {
        let mut panel = PackPanel {
            selection: Some(report(7, true)),
            approve: true,
            ..Default::default()
        };
        let assets=[("model.onnx","model"),("runtime.bin","runtime"),("profile.json","profile"),("LICENSE","license")].iter().map(|(path,role)|serde_json::json!({"path":path,"role":role,"byte_len":1,"sha256":"c".repeat(64)})).collect::<Vec<_>>();
        let pack=crate::bundled_models::BundledPack {
            manifest_sha256:"b".repeat(64),source:std::path::PathBuf::from(crate::test_host_path("/fixture/bundled-pack")),
            manifest:serde_json::from_value(serde_json::json!({"schema_version":1,"pack_id":"authored-fixture","target_os":"linux","target_arch":"x86_64","ort_api_version":24,"labels":["fixture"],"assets":assets,"license_path":"LICENSE","source_description":"Authored test"})).unwrap(),
        };
        panel.prepare_bundled_import(&pack).unwrap();
        assert_eq!(panel.digest, pack.manifest_sha256);
        assert!(!panel.approve);
        assert!(panel.pending.is_none());
        assert!(panel.recovery.is_none());
        assert_eq!(panel.selection.as_ref().unwrap().selection_revision, 7);
        let submission = panel
            .prepare(PackAction::Import, "fixture-owner-proof".into())
            .unwrap();
        assert!(
            matches!(submission.command,Command::ImportModelPack{source,..} if source==pack.source)
        );
        assert!(panel
            .prepare(PackAction::Select, "fixture-owner-proof".into())
            .is_err());
        panel.pending = Some(PackReplyKind::Mutation);
        let source = panel.source.clone();
        assert!(panel.prepare_bundled_import(&pack).is_err());
        assert_eq!(panel.source, source);
    }
    fn report(revision: u64, active: bool) -> ModelPackSelectionReport {
        let identity = |sha: String| ArtifactIdentity {
            artifact_id: ArtifactId::new(sha.clone()).unwrap(),
            sha256: sha,
            byte_len: 100,
        };
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: identity("a".repeat(64)),
            runtime: identity("b".repeat(64)),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 4096,
            minimum_output_bytes: 1024,
            cpu_threads: 2,
            cuda_resources: None,
        };
        ModelPackSelectionReport {
            schema_version: 1,
            catalog_binding: None,
            selection_revision: revision,
            choice: active.then(|| ModelPackChoice {
                manifest_sha256: "c".repeat(64),
                selection_revision: revision,
            }),
            profile: active.then_some(profile),
            native_execution_approved: active,
            selected_by: (revision > 0).then(|| SessionId::new("fixture-owner").unwrap()),
            selection_request: (revision > 0).then(|| RequestId::new("fixture-selection").unwrap()),
            rights_verified: false,
            compatible: false,
            qualified: false,
        }
    }
    fn refresh(panel: &mut PackPanel, report: ModelPackSelectionReport) {
        let work = panel
            .prepare(PackAction::Refresh, "fixture-owner-proof".into())
            .unwrap();
        panel.submitted(&work);
        assert!(!panel.accept_selection(report).unwrap());
    }
    #[test]
    fn historical_selection_receipt_and_failed_ack_never_authorize_generation() {
        let mut panel = PackPanel {
            digest: "c".repeat(64),
            ..Default::default()
        };
        assert!(panel
            .prepare(PackAction::Select, "fixture-owner-proof".into())
            .is_err());
        refresh(&mut panel, report(0, false));
        panel.approve = true;
        let selection = panel
            .prepare(PackAction::Select, "fixture-owner-proof".into())
            .unwrap();
        panel.submitted(&selection);
        assert!(!panel.approve);
        assert!(panel.generation_choice().is_err());
        panel.failed();
        panel.digest = "d".repeat(64);
        let retry = panel
            .prepare(PackAction::Replay, "fixture-owner-proof".into())
            .unwrap();
        assert_eq!(retry.id, selection.id);
        assert_eq!(
            serde_json::to_value(&retry.command).unwrap(),
            serde_json::to_value(&selection.command).unwrap()
        );
        panel.submitted(&retry);
        assert!(panel.accept_selection(report(1, true)).unwrap());
        assert!(panel.generation_choice().is_err());
        refresh(&mut panel, report(2, false));
        assert!(panel.generation_choice().is_err());
        let replay = panel
            .prepare(PackAction::Replay, "fixture-owner-proof".into())
            .unwrap();
        panel.submitted(&replay);
        assert!(panel.accept_selection(report(1, true)).unwrap());
        assert!(panel.generation_choice().is_err());
        refresh(&mut panel, report(3, true));
        assert_eq!(panel.generation_choice().unwrap().selection_revision, 3);
        panel.failed();
        assert_eq!(panel.generation_choice().unwrap().selection_revision, 3);
        println!("HEADLESS_PACK_STATE: lost acknowledgement preserves original intent; historical replay cannot rearm selection");
    }
    #[test]
    fn maximum_preserves_selected_pack_and_refuses_temporal_without_fallback() {
        let mut panel = PackPanel::default();
        refresh(&mut panel, report(4, true));
        let source = SourceVersionId::new("tier-source").unwrap();
        let command = panel
            .generation_command(
                source.clone(),
                "maximum".into(),
                GenerationSettings::default(),
            )
            .unwrap();
        let Command::GenerateFromModelPack { preset, choice, .. } = command else {
            panic!("pack command required")
        };
        assert_eq!(preset, "maximum");
        assert_eq!(choice.selection_revision, 4);
        let mut temporal = report(5, true);
        let profile = temporal.profile.as_mut().unwrap();
        profile.preprocessing.input.decoder = DetectorDecoder::TemporalScriptPosition;
        profile.preprocessing.input.channels = 6;
        profile.preprocessing.color = InferenceColor::RgbFramePair;
        profile.preprocessing.scale = vec![1.0 / 255.0; 6];
        profile.preprocessing.bias = vec![0.0; 6];
        refresh(&mut panel, temporal);
        assert!(panel
            .generation_command(
                source.clone(),
                "maximum".into(),
                GenerationSettings::default()
            )
            .is_err());
        assert_eq!(panel.generation_choice().unwrap().selection_revision, 5);
        assert!(panel
            .generation_command(
                source,
                "default".into(),
                GenerationSettings {
                    analysis_width: 64,
                    analysis_height: 64,
                    ..Default::default()
                }
            )
            .is_ok());
    }

    #[test]
    fn pack_generation_preserves_authored_settings_and_refuses_conflicting_profile() {
        let mut panel = PackPanel::default();
        refresh(&mut panel, report(4, true));
        let source = SourceVersionId::new("fixture-media").unwrap();
        let settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            source_placement: Some(SourcePlacement {
                placement_id: SourcePlacementId::new("fixture-placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: source.clone(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(1, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(1, 2).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            }),
            project_interval: Some(
                TimeRange::new(ProjectTime::from_nanos(100), ProjectTime::from_nanos(200)).unwrap(),
            ),
            target_seed: Some(AnalysisTargetSeed {
                source_frame_index: 2,
                x: 0.25,
                y: 0.75,
                class_id: Some(0),
            }),
            mixed_audio: Some(MixedAudioSettings {
                independent: Some(IndependentAudio {
                    source_version: SourceVersionId::new("fixture-audio").unwrap(),
                    alignment: AudioAlignment {
                        rate: None,
                        video_anchor: SourceTimestamp::new(1, 2).unwrap(),
                        audio_anchor: SourceTimestamp::new(-1, 3).unwrap(),
                    },
                }),
                intent: MultimodalIntent::Faithful,
                mapping: AudioMapping::new(
                    Axis::Stroke,
                    0.25,
                    NormalizedPosition::new(0.5).unwrap(),
                )
                .unwrap(),
            }),
            ..Default::default()
        };
        let Command::GenerateFromModelPack {
            source_version,
            preset,
            settings: Some(actual),
            choice,
        } = panel
            .generation_command(source.clone(), "fast".into(), settings.clone())
            .unwrap()
        else {
            panic!("wrong command")
        };
        assert_eq!(source_version, source);
        assert_eq!(preset, "fast");
        assert_eq!(choice.selection_revision, 4);
        let mut expected = settings.clone();
        expected.model_input = Some(panel.profile().unwrap().preprocessing.input.clone());
        expected.inference_profile = panel.profile();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let mut reverse = settings.clone();
        reverse
            .source_placement
            .as_mut()
            .unwrap()
            .spec
            .rate_numerator = -1;
        assert!(panel
            .generation_command(source.clone(), "fast".into(), reverse)
            .is_err());
        let mut conflicting = settings;
        conflicting.model_input = Some(ModelInputContract {
            width: 32,
            ..panel.profile().unwrap().preprocessing.input
        });
        assert!(panel
            .generation_command(source, "fast".into(), conflicting)
            .is_err());
        println!("HEADLESS_PACK_GENERATION: exact selected fence/profile; authored interval, target and independent audio preserved");
    }
    #[test]
    fn preview_envelope_refuses_changed_selection_model_profile_and_frame() {
        use sha2::{Digest, Sha256};
        let mut panel = PackPanel::default();
        refresh(&mut panel, report(4, true));
        let profile = panel.profile().unwrap();
        let context = FrameContext {
            source_version: SourceVersionId::new("fixture-source").unwrap(),
            source_placement: SourcePlacementId::new("fixture-placement").unwrap(),
            frame: FrameId::new(4),
            transform: TransformId::new("source-plane").unwrap(),
            seek_generation: 3,
            request_generation: 8,
        };
        let preview = PreviewResult {
            annotated: None,
            time_basis: PreviewTimeBasis::SourcePts,
            elapsed_source_time: None,
            source_origin: None,
            placed: None,
            coordinate_map: None,
            context,
            source_time: SourceTimestamp::new(1, 2).unwrap(),
            requested_source_time: SourceTimestamp::new(1, 2).unwrap(),
            source_frame_index: 4,
            observations: vec![],
            frame: None,
            analysis: AnalysisStatus::Available,
        };
        let envelope = ModelPackPreviewResult {
            choice: panel.generation_choice().unwrap(),
            model: profile.model.clone(),
            profile_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&profile).unwrap())
            ),
            preview,
            qualified: false,
        };
        assert!(panel.matches_preview(&envelope));
        let mut changed = envelope.clone();
        changed.choice.selection_revision = 3;
        assert!(!panel.matches_preview(&changed));
        let mut changed = envelope.clone();
        changed.model.sha256 = "d".repeat(64);
        assert!(!panel.matches_preview(&changed));
        let mut changed = envelope.clone();
        changed.profile_sha256 = "d".repeat(64);
        assert!(!panel.matches_preview(&changed));
        let mut changed = envelope.clone();
        changed.preview.context.frame = FrameId::new(5);
        assert!(!panel.matches_preview(&changed));
        let mut changed = envelope.clone();
        changed.qualified = true;
        assert!(!panel.matches_preview(&changed));
        refresh(&mut panel, report(5, false));
        assert!(!panel.matches_preview(&envelope));
    }
    #[cfg(unix)]
    #[test]
    fn restored_recovery_is_not_selection_authority_and_replay_preserves_original_intent() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new("restored-window").unwrap();
        let binding = "b".repeat(64);
        let id = RequestId::new("original-selection").unwrap();
        let command = Command::SelectModelPack {
            manifest_sha256: Some("c".repeat(64)),
            expected_selection_revision: 4,
            approve_unqualified_native_execution: true,
            owner_proof: "fixture-owner-proof".into(),
        };
        crate::pack_recovery::prepare(directory.path(), &session, &binding, &id, &command).unwrap();
        let mut panel = PackPanel::default();
        panel.restore_recovery(
            crate::pack_recovery::load(directory.path(), &session, &binding).unwrap(),
        );
        assert!(panel.generation_choice().is_err());
        assert!(!panel.approve);
        assert!(panel.dismissable_id().is_err());
        panel.digest = "d".repeat(64);
        let replay = panel
            .prepare(PackAction::Replay, "fixture-owner-proof".into())
            .unwrap();
        assert_eq!(replay.id, Some(id));
        assert_eq!(
            serde_json::to_value(replay.command).unwrap(),
            serde_json::to_value(command).unwrap()
        );
        let context = egui::Context::default();
        assert!(click(
            &context,
            &mut panel,
            false,
            "Dismiss acknowledged recovery entry"
        )
        .is_none());
        assert!(matches!(
            click(
                &context,
                &mut panel,
                false,
                "Replay retained pack operation"
            ),
            Some(PackAction::Replay)
        ));
        assert!(panel
            .prepare(PackAction::Dismiss, "fixture-owner-proof".into())
            .is_err());
        crate::pack_recovery::acknowledge(
            directory.path(),
            &session,
            &binding,
            &RequestId::new("original-selection").unwrap(),
        )
        .unwrap();
        panel.restore_recovery(
            crate::pack_recovery::load(directory.path(), &session, &binding).unwrap(),
        );
        assert!(matches!(
            click(
                &context,
                &mut panel,
                false,
                "Dismiss acknowledged recovery entry"
            ),
            Some(PackAction::Dismiss)
        ));
    }
    fn draw(
        ctx: &egui::Context,
        panel: &mut PackPanel,
        busy: bool,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<PackAction>) {
        let mut action = None;
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
                    action = panel.ui(ui, busy);
                });
            },
        );
        (output, action)
    }
    fn find_text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::epaint::Shape::Vec(shapes) => {
                shapes.iter().find_map(|shape| find_text(shape, label))
            }
            _ => None,
        }
    }
    fn click(
        ctx: &egui::Context,
        panel: &mut PackPanel,
        busy: bool,
        label: &str,
    ) -> Option<PackAction> {
        let (output, _) = draw(ctx, panel, busy, vec![]);
        let at = output
            .shapes
            .iter()
            .find_map(|s| find_text(&s.shape, label))
            .unwrap_or_else(|| panic!("missing drawn control: {label}"));
        draw(
            ctx,
            panel,
            busy,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        draw(
            ctx,
            panel,
            busy,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
        .1
    }
    fn catalog_report(revision: u64) -> ModelCatalogReport {
        ModelCatalogReport {
            schema_version: 1,
            bootstrap_sha256: "b".repeat(64),
            current_root_sha256: "a".repeat(64),
            catalog_revision: revision,
            root_version: 1,
            timestamp_version: 1,
            snapshot_version: 1,
            targets_version: 1,
            manifest_sha256s: vec!["c".repeat(64)],
            accepted_by: SessionId::new("catalog-ui-owner").unwrap(),
            accepted_request: RequestId::new("catalog-ui-request").unwrap(),
            authenticated_at_acceptance: true,
            current_publisher_trust: false,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
        }
    }
    fn association_report() -> ModelPackCatalogReport {
        let pack = ModelPackReport {
            manifest_sha256: "c".repeat(64),
            pack_id: ArtifactId::new("ui-association").unwrap(),
            asset_count: 4,
            total_bytes: 4,
            declared_target_matches_host: true,
            verified_at_unix_ms: 1,
            integrity_verified: true,
            native_execution_approved: false,
            rights_verified: false,
            compatible: false,
            qualified: false,
            active: false,
        };
        let association = ModelPackCatalogReport {
            schema_version: 1,
            catalog: catalog_report(2),
            pack,
            checked_at_unix_ms: 2,
            metadata_expires_at_unix_ms: 3,
            listed_in_catalog: true,
            online_revocation_checked: false,
        };
        association.validate().unwrap();
        association
    }
    #[test]
    fn headless_catalog_controls_require_explicit_approval_and_distinguish_history() {
        let ctx = egui::Context::default();
        let mut panel = PackPanel {
            catalog_digest: "b".repeat(64),
            catalog_source: crate::test_host_path("/authored/catalog"),
            catalog_bootstrap: crate::test_host_path("/authored/root.json"),
            digest: "c".repeat(64),
            ..Default::default()
        };
        assert!(click(&ctx, &mut panel, false, "Accept captured catalog").is_none());
        click(
            &ctx,
            &mut panel,
            false,
            "I independently approve this exact bootstrap for catalog intake",
        );
        assert!(panel.approve_bootstrap);
        assert!(matches!(
            click(&ctx, &mut panel, false, "Accept captured catalog"),
            Some(PackAction::CatalogRefresh)
        ));
        let first = panel
            .prepare(PackAction::CatalogRefresh, "fixture-owner".into())
            .unwrap();
        panel.submitted(&first);
        assert!(panel.accept_catalog(catalog_report(1)).unwrap());
        assert!(panel.catalog.is_none());
        assert!(panel.generation_choice().is_err());
        let status = panel
            .prepare(PackAction::CatalogStatus, "fixture-owner".into())
            .unwrap();
        panel.submitted(&status);
        assert!(!panel.accept_catalog(catalog_report(2)).unwrap());
        assert_eq!(panel.catalog_revision, 2);
        let check = panel
            .prepare(PackAction::CatalogCheck, "fixture-owner".into())
            .unwrap();
        panel.submitted(&check);
        panel.digest = "d".repeat(64);
        assert!(panel.accept_association(association_report()).is_err());
        assert!(panel.association.is_none());
        panel.digest = "c".repeat(64);
        refresh(&mut panel, report(0, false));
        panel.approve = true;
        assert!(matches!(
            click(&ctx, &mut panel, false, "Approve catalog-bound selection"),
            Some(PackAction::CatalogSelect)
        ));
        let selection = panel
            .prepare(PackAction::CatalogSelect, "fixture-owner".into())
            .unwrap();
        assert!(matches!(
            selection.command,
            Command::SelectCatalogModelPack {
                binding: ModelCatalogBinding {
                    catalog_revision: 2,
                    ..
                },
                expected_selection_revision: 0,
                ..
            }
        ));
        panel.submitted(&selection);
        assert!(panel.selection.is_none());
    }
    #[test]
    fn withdrawal_panel_requires_current_status_and_refreshes_after_receipt() {
        let mut panel = PackPanel::default();
        assert!(panel
            .prepare(PackAction::Withdraw, "fixture-proof".into())
            .is_err());
        refresh(&mut panel, report(1, true));
        panel.include_unclassified_generation = true;
        let work = panel
            .prepare(PackAction::Withdraw, "fixture-proof".into())
            .unwrap();
        let Command::WithdrawModelPackApproval {
            choice,
            expected_selection_revision,
            include_unclassified_generation,
            ..
        } = &work.command
        else {
            panic!("wrong command")
        };
        assert_eq!(*expected_selection_revision, 1);
        assert!(*include_unclassified_generation);
        let choice = choice.clone();
        panel.submitted(&work);
        assert!(panel.selection.is_none());
        let mut receipt = ModelPackWithdrawalReport {
            schema_version: 1,
            choice,
            selection: report(1, false),
            cancelled_approved_jobs: 1,
            cancelled_unclassified_jobs: 1,
        };
        assert!(panel.accept_withdrawal(receipt.clone()).is_err());
        receipt.selection = report(2, false);
        assert!(panel.accept_withdrawal(receipt).unwrap());
        assert!(panel.selection.is_none());
        assert!(!panel.include_unclassified_generation);
        assert!(panel.historical.is_some());
        assert!(panel
            .prepare(PackAction::Withdraw, "fixture-proof".into())
            .is_err());
    }
    #[test]
    fn headless_draw_and_pointer_input_require_fresh_status_and_explicit_approval() {
        let ctx = egui::Context::default();
        let mut panel = PackPanel {
            digest: "c".repeat(64),
            ..Default::default()
        };
        assert!(click(&ctx, &mut panel, false, "Approve selection").is_none());
        assert!(click(&ctx,&mut panel,false,"I approve this unqualified model for future offline analysis and generation through the pinned declared runtime").is_none());
        assert!(panel.approve);
        assert!(click(&ctx, &mut panel, false, "Approve selection").is_none());
        refresh(&mut panel, report(0, false));
        assert!(!panel.approve);
        click(&ctx,&mut panel,false,"I approve this unqualified model for future offline analysis and generation through the pinned declared runtime");
        assert!(panel.approve);
        assert!(click(&ctx, &mut panel, true, "Approve selection").is_none());
        assert!(matches!(
            click(&ctx, &mut panel, false, "Approve selection"),
            Some(PackAction::Select)
        ));
        panel.association = Some(association_report());
        click(&ctx, &mut panel, false, &"c".repeat(64));
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        draw(
            &ctx,
            &mut panel,
            false,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: Some(egui::Key::A),
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
                egui::Event::Text("d".repeat(64)),
            ],
        );
        assert_eq!(panel.digest, "d".repeat(64));
        assert!(!panel.approve);
        assert!(
            panel.association.is_none(),
            "changing manifest must clear the old association"
        );
        assert!(click(&ctx, &mut panel, false, "Approve selection").is_none());
        assert!(click(
            &ctx,
            &mut panel,
            false,
            "Deactivate future analysis and generation"
        )
        .is_none());
        refresh(&mut panel, report(1, true));
        assert!(matches!(
            click(
                &ctx,
                &mut panel,
                false,
                "Deactivate future analysis and generation"
            ),
            Some(PackAction::Deactivate)
        ));
        println!("HEADLESS_PACK_UI: actual egui draw and pointer controls refuse missing status/approval/busy selection; explicit activation/deactivation emit typed actions");
    }
}
