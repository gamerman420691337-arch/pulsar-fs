//! Desktop batch presentation and bounded background client operations.
pub use crate::library_publication::{PublicationPhase, WorkflowControl, WorkflowReport};
use crate::{EngineApi, ResponseBody, SessionClient};
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::*;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    thread::JoinHandle,
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum LibraryAction {
    InspectAxes {
        folder: PathBuf,
        recursive: bool,
    },
    Intake {
        folder: PathBuf,
        recursive: bool,
        recipe_path: PathBuf,
        request_id: RequestId,
    },
    IntakeRecipe {
        folder: PathBuf,
        recursive: bool,
        recipe: AudioBundleRecipe,
        request_id: RequestId,
    },
    IntakeVisual {
        folder: PathBuf,
        recursive: bool,
        generation: crate::cli::LibraryGenerationOptions,
        #[serde(skip_serializing_if = "Option::is_none")]
        video_view: Option<VideoView>,
        request_id: RequestId,
    },
    Prepare {
        options: LibraryBatchOptions,
        request_id: RequestId,
    },
    MigrateIntake {
        batch: RequestId,
        expected_report_sha256: String,
        recipe_path: PathBuf,
        request_id: RequestId,
    },
    Migrate {
        batch: RequestId,
        expected_report_sha256: String,
        recipe: LibraryBatchRecipe,
        request_id: RequestId,
    },
    List,
    PackSelectionStatus,
    Status {
        batch: RequestId,
    },
    WorkflowStatus {
        batch: RequestId,
        directory: PathBuf,
    },
    RetirePending {
        directory: PathBuf,
        pending: PathBuf,
        sha256: String,
    },
    RecoverPreparation {
        batch: RequestId,
        directory: PathBuf,
        index: u16,
        staging: PathBuf,
        preparation_receipt: PathBuf,
    },
    Run {
        batch: RequestId,
        directory: PathBuf,
        steps: u16,
        poll_ms: u16,
    },
    Cancel {
        batch: RequestId,
        request_id: RequestId,
    },
    Release {
        batch: RequestId,
        request_id: RequestId,
    },
}

impl LibraryAction {
    fn mutating(&self) -> bool {
        !matches!(
            self,
            Self::List
                | Self::InspectAxes { .. }
                | Self::PackSelectionStatus
                | Self::Status { .. }
                | Self::WorkflowStatus { .. }
        )
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "report", rename_all = "snake_case")]
pub enum LibraryReply {
    Axes(crate::library_axis_inventory::Inventory),
    PendingRetired(crate::library_publication::PendingRetirementReceipt),
    Catalog(LibraryBatchCatalog),
    PackSelection(Box<ModelPackSelectionReport>),
    Generation(LibraryBatchReport),
    Workflow(Box<WorkflowReport>),
    Released(LibraryBatchReleaseReceipt),
}
impl LibraryReply {
    fn generation(&self) -> Option<&LibraryBatchReport> {
        match self {
            Self::Generation(r) => Some(r),
            Self::Workflow(r) => Some(&r.generation),
            Self::Released(r) => Some(&r.report),
            Self::Axes(_) | Self::Catalog(_) | Self::PackSelection(_) | Self::PendingRetired(_) => {
                None
            }
        }
    }
}

/// Selection is bounded and uses the same companion-preservation rules as CLI intake.
/// Call on a background thread, never during an egui frame.
pub fn intake(folder: &Path, recursive: bool, recipe_path: &Path) -> Result<LibraryBatchOptions> {
    crate::library_generation::retained_batch_options(folder, recursive, recipe_path)
}

pub struct LibraryOutcome {
    /// Fully captured request content, including the parsed recipe after intake.
    pub retry_action: LibraryAction,
    pub result: Result<LibraryReply, String>,
    pub dispatch_possible: bool,
}
/// Actual client service. Never starts an Engine, imports a model or resumes at startup.
pub fn execute(
    endpoint: &Path,
    bootstrap: &Path,
    action: LibraryAction,
    control: &WorkflowControl,
) -> LibraryOutcome {
    let action = match &action {
        LibraryAction::MigrateIntake {
            batch,
            expected_report_sha256,
            recipe_path,
            request_id,
        } => {
            let recipe = (|| -> Result<LibraryBatchRecipe> {
                Ok(serde_json::from_slice(
                    &crate::inference::read_bounded_profile(recipe_path)?,
                )?)
            })();
            match recipe {
                Ok(recipe) => LibraryAction::Migrate {
                    batch: batch.clone(),
                    expected_report_sha256: expected_report_sha256.clone(),
                    recipe,
                    request_id: request_id.clone(),
                },
                Err(error) => {
                    return LibraryOutcome {
                        retry_action: action,
                        result: Err(bounded_error(error)),
                        dispatch_possible: false,
                    }
                }
            }
        }
        LibraryAction::Intake {
            folder,
            recursive,
            recipe_path,
            request_id,
        } => match intake(folder, *recursive, recipe_path) {
            Ok(options) => LibraryAction::Prepare {
                options,
                request_id: request_id.clone(),
            },
            Err(error) => {
                return LibraryOutcome {
                    retry_action: action,
                    result: Err(bounded_error(error)),
                    dispatch_possible: false,
                }
            }
        },
        LibraryAction::IntakeRecipe {
            folder,
            recursive,
            recipe,
            request_id,
        } => match crate::library_generation::retained_batch_options_with_recipe(
            folder,
            *recursive,
            recipe.clone(),
        ) {
            Ok(options) => LibraryAction::Prepare {
                options,
                request_id: request_id.clone(),
            },
            Err(error) => {
                return LibraryOutcome {
                    retry_action: action,
                    result: Err(bounded_error(error)),
                    dispatch_possible: false,
                }
            }
        },
        LibraryAction::IntakeVisual {
            folder,
            recursive,
            generation,
            video_view,
            request_id,
        } => {
            let options = (|| -> Result<LibraryBatchOptions> {
                ensure!(
                    generation.audio_bundle_recipe.is_none(),
                    "Visual editor cannot contain an audio recipe"
                );
                ensure!(
                    video_view.is_none() || generation.video_view.is_none(),
                    "Choose a form view or a view JSON file"
                );
                if let Some(view) = video_view {
                    view.validate()?;
                }
                let (mut settings, model_path) = crate::library_generation::configure(generation)?;
                if let Some(view) = video_view {
                    settings.video_view = Some(view.clone());
                    settings.validate()?;
                }
                let model_pack = crate::library_generation::selected_pack(
                    generation,
                    &mut settings,
                    endpoint,
                    bootstrap,
                )?;
                let recipe = LibraryVisualRecipe {
                    kind: LibraryVisualKind::Visual,
                    preset: generation.preset.clone(),
                    settings,
                    model_path,
                    model_pack,
                };
                recipe.validate()?;
                crate::library_generation::retained_batch_options_with_generation(
                    folder,
                    *recursive,
                    LibraryBatchRecipe::Visual(Box::new(recipe)),
                )
            })();
            match options {
                Ok(options) => LibraryAction::Prepare {
                    options,
                    request_id: request_id.clone(),
                },
                Err(error) => {
                    return LibraryOutcome {
                        retry_action: action,
                        result: Err(bounded_error(error)),
                        dispatch_possible: false,
                    }
                }
            }
        }
        _ => action,
    };
    let mut dispatch_possible = !matches!(
        action,
        LibraryAction::RecoverPreparation { .. }
            | LibraryAction::InspectAxes { .. }
            | LibraryAction::RetirePending { .. }
    );
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<LibraryReply> {
            if let LibraryAction::InspectAxes { folder, recursive } = &action {
                return Ok(LibraryReply::Axes(crate::library_axis_inventory::inspect(
                    folder, *recursive,
                )?));
            }
            if matches!(&action, LibraryAction::PackSelectionStatus) {
                return Ok(LibraryReply::PackSelection(Box::new(
                    crate::library_generation::current_pack_selection(endpoint, bootstrap)?,
                )));
            }
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let body = match &action {
                LibraryAction::Migrate {
                    batch,
                    expected_report_sha256,
                    recipe,
                    request_id,
                } => client.execute_identified(
                    request_id.clone(),
                    Command::LibraryBatchMigrate {
                        batch_request_id: batch.clone(),
                        expected_report_sha256: expected_report_sha256.clone(),
                        recipe: recipe.clone(),
                    },
                    None,
                    None,
                )?,
                LibraryAction::Prepare {
                    options,
                    request_id,
                } => client.execute_identified(
                    request_id.clone(),
                    Command::LibraryBatchPrepare {
                        options: options.clone(),
                    },
                    None,
                    None,
                )?,
                LibraryAction::List => client.execute(Command::LibraryBatchList, None, None)?,
                LibraryAction::Status { batch } => client.execute(
                    Command::LibraryBatchStatus {
                        batch_request_id: batch.clone(),
                    },
                    None,
                    None,
                )?,
                LibraryAction::Cancel { batch, request_id } => client.execute_identified(
                    request_id.clone(),
                    Command::LibraryBatchCancel {
                        batch_request_id: batch.clone(),
                    },
                    None,
                    None,
                )?,
                LibraryAction::Release { batch, request_id } => client.execute_identified(
                    request_id.clone(),
                    Command::LibraryBatchRelease {
                        batch_request_id: batch.clone(),
                    },
                    None,
                    None,
                )?,
                LibraryAction::RetirePending { directory, pending, sha256 } => {
                    let result=crate::library_publication::retire_pending(&mut client,directory,pending,sha256);
                    match result {
                        Ok(receipt)=>return Ok(LibraryReply::PendingRetired(receipt)),
                        Err(error)=>{
                            dispatch_possible=error.downcast_ref::<crate::library_publication::PendingRetirementUnknown>().is_some();
                            return Err(error);
                        }
                    }
                }
                LibraryAction::RecoverPreparation {
                    batch,
                    directory,
                    index,
                    staging,
                    preparation_receipt,
                } => {
                    dispatch_possible = true;
                    if let Err(error) = crate::library_publication::recover_preparation(
                        &mut client,
                        batch.clone(),
                        directory,
                        *index,
                        staging,
                        preparation_receipt,
                    ) {
                        dispatch_possible = error
                            .downcast_ref::<crate::library_publication::PreparationWriteUnknown>()
                            .is_some();
                        return Err(error);
                    }
                    return Ok(LibraryReply::Workflow(Box::new(
                        crate::library_publication::workflow_status(
                            &mut client,
                            batch.clone(),
                            directory,
                        )?,
                    )));
                }
                LibraryAction::Run {
                    batch,
                    directory,
                    steps,
                    poll_ms,
                } => {
                    return Ok(LibraryReply::Workflow(
                        crate::library_publication::run_controlled(
                            &mut client,
                            batch.clone(),
                            directory,
                            *steps,
                            *poll_ms,
                            control,
                        )
                        .map(Box::new)?,
                    ))
                }
                LibraryAction::WorkflowStatus { batch, directory } => {
                    return Ok(LibraryReply::Workflow(
                        crate::library_publication::workflow_status(
                            &mut client,
                            batch.clone(),
                            directory,
                        )
                        .map(Box::new)?,
                    ))
                }
                LibraryAction::Intake { .. }
                | LibraryAction::IntakeRecipe { .. }
                | LibraryAction::IntakeVisual { .. }
                | LibraryAction::MigrateIntake { .. }
                | LibraryAction::InspectAxes { .. }
                | LibraryAction::PackSelectionStatus => {
                    unreachable!("captured intake")
                }
            };
            match (&action, body) {
                (LibraryAction::List, ResponseBody::LibraryBatchCatalog(report)) => {
                    Ok(LibraryReply::Catalog(report))
                }
                (LibraryAction::Release { .. }, ResponseBody::LibraryBatchReleased(report)) => {
                    Ok(LibraryReply::Released(report))
                }
                (
                    LibraryAction::Migrate { .. }
                    | LibraryAction::Prepare { .. }
                    | LibraryAction::Status { .. }
                    | LibraryAction::Cancel { .. },
                    ResponseBody::LibraryBatch(report),
                ) => Ok(LibraryReply::Generation(report)),
                _ => {
                    anyhow::bail!(
                        "Unexpected local batch acknowledgement; retain original request IDs"
                    )
                }
            }
        }))
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "Local batch service panicked; retain the exact captured request before retry"
            ))
        })
        .and_then(|reply| {
            check_reply(&action, &reply)?;
            Ok(reply)
        })
        .map_err(bounded_error);
    LibraryOutcome {
        retry_action: action,
        result,
        dispatch_possible,
    }
}
fn check_reply(action: &LibraryAction, reply: &LibraryReply) -> Result<()> {
    if let LibraryReply::PendingRetired(report) = reply {
        ensure!(
            matches!(action,LibraryAction::RetirePending {directory,pending,sha256} if *directory==report.directory && *pending==report.original_path && *sha256==report.sha256)
                && report.retained_path.parent() == Some(report.directory.as_path())
                && report.retained_path != report.original_path
                && report.file_and_directory_synced
                && !report.qualified
                && report.byte_len > 0
                && report.byte_len <= 1024 * 1024,
            "Pending retirement acknowledgement differs"
        );
        return Ok(());
    }
    if let LibraryReply::Axes(report) = reply {
        ensure!(
            matches!(action, LibraryAction::InspectAxes { folder, recursive } if crate::absolute_path(folder)? == report.folder && *recursive == report.recursive)
                && report.schema == 1
                && !report.qualified
                && report.entries.len() <= MAX_LIBRARY_BATCH_ITEMS,
            "Axis observation scope differs"
        );
        return Ok(());
    }
    if let LibraryReply::PackSelection(report) = reply {
        ensure!(
            matches!(action, LibraryAction::PackSelectionStatus),
            "Unexpected pack selection response"
        );
        report.validate()?;
        return Ok(());
    }
    if let LibraryReply::Catalog(catalog) = reply {
        ensure!(
            matches!(action, LibraryAction::List)
                && !catalog.qualified
                && catalog.batches.len() <= 128,
            "Local catalog acknowledgement differs"
        );
        return Ok(());
    }
    let expected = match action {
        LibraryAction::Prepare { request_id, .. } | LibraryAction::Migrate { request_id, .. } => {
            request_id
        }
        LibraryAction::Status { batch }
        | LibraryAction::WorkflowStatus { batch, .. }
        | LibraryAction::Run { batch, .. }
        | LibraryAction::RecoverPreparation { batch, .. }
        | LibraryAction::Cancel { batch, .. }
        | LibraryAction::Release { batch, .. } => batch,
        _ => anyhow::bail!("Unexpected local batch response scope"),
    };
    let generation = reply
        .generation()
        .ok_or_else(|| anyhow::anyhow!("Generation acknowledgement absent"))?;
    generation.validate_migration()?;
    generation.validate_publication_shape()?;
    if let LibraryAction::Migrate {
        batch,
        expected_report_sha256,
        recipe,
        ..
    } = action
    {
        ensure!(
            generation
                .migration
                .as_ref()
                .is_some_and(|m| m.parent_batch_request_id == *batch
                    && m.parent_report_sha256 == *expected_report_sha256)
                && generation.model_pack.as_deref() == recipe.model_pack(),
            "Migration acknowledgement lineage or choice differs"
        );
        ensure!(
            matches!(recipe, LibraryBatchRecipe::Visual(_)) == generation.is_visual(),
            "Migration acknowledgement generation family differs"
        );
    }
    ensure!(
        &generation.batch_request_id == expected
            && !generation.qualified
            && !generation.publication_completed
            && (1..=MAX_LIBRARY_BATCH_ITEMS).contains(&generation.items.len()),
        "Local batch acknowledgement scope/shape differs"
    );
    if let LibraryAction::Prepare { options, .. } = action {
        ensure!(
            options.inputs.len() == generation.items.len()
                && options
                    .inputs
                    .iter()
                    .zip(&generation.items)
                    .all(|(input, item)| {
                        item.publication_directory == input.publication_directory
                            && item.publication_axes
                                == if generation.is_visual() {
                                    Some(vec![Axis::Stroke])
                                } else {
                                    input.publication_axes.clone()
                                }
                    }),
            "Batch acknowledgement captured publication axes differ"
        );
        ensure!(
            options.recipe.model_pack() == generation.model_pack.as_deref(),
            "Batch acknowledgement pack choice differs"
        );
        ensure!(
            matches!(options.recipe, LibraryBatchRecipe::Visual(_)) == generation.is_visual(),
            "Batch acknowledgement recipe kind differs"
        );
    }
    if let LibraryAction::Cancel { request_id, .. } = action {
        ensure!(
            generation
                .cancellation
                .as_ref()
                .is_some_and(|cancel| cancel.request_id == *request_id),
            "Cancellation acknowledgement identity differs"
        );
    }
    if let LibraryReply::Workflow(workflow) = reply {
        ensure!(
            workflow.publication.batch_request_id == *expected
                && !workflow.qualified
                && !workflow.publication.qualified,
            "Workflow acknowledgement scope differs"
        );
    }
    Ok(())
}
fn required_path(value: &str) -> Result<PathBuf> {
    ensure!(!value.trim().is_empty(), "Choose an explicit path");
    crate::absolute_path(Path::new(value.trim()))
}
fn bounded_error(error: anyhow::Error) -> String {
    error.to_string().chars().take(4096).collect()
}

struct Job {
    handle: Option<JoinHandle<LibraryOutcome>>,
    action: LibraryAction,
    control: WorkflowControl,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.control.request_stop();
    }
}

pub struct LibraryPanel {
    endpoint: PathBuf,
    bootstrap: PathBuf,
    job: Option<Job>,
    recovery: Option<LibraryAction>,
    pub reply: Option<LibraryReply>,
    pub message: String,
    folder: String,
    recipe: String,
    use_recipe_file: bool,
    audio_recipe: crate::audio_panel::AudioPanel,
    visual: bool,
    visual_recipe: crate::library_visual_panel::LibraryVisualPanel,
    recursive: bool,
    prepare_id: String,
    batch: String,
    directory: String,
    cancel_id: String,
    release_id: String,
    migration_id: String,
    migration_recipe: String,
    steps: u16,
    poll_ms: u16,
    recovery_index: u16,
    recovery_staging: String,
    recovery_receipt: String,
    retirement_pending: String,
    retirement_sha256: String,
}
impl LibraryPanel {
    pub fn new(endpoint: PathBuf, bootstrap: PathBuf) -> Self {
        Self {
            endpoint,
            bootstrap,
            job: None,
            recovery: None,
            reply: None,
            message: String::new(),
            folder: String::new(),
            recipe: String::new(),
            use_recipe_file: false,
            audio_recipe: Default::default(),
            visual: false,
            visual_recipe: Default::default(),
            recursive: false,
            prepare_id: crate::session::new_request_id().to_string(),
            batch: String::new(),
            directory: String::new(),
            cancel_id: crate::session::new_request_id().to_string(),
            release_id: crate::session::new_request_id().to_string(),
            migration_id: crate::session::new_request_id().to_string(),
            migration_recipe: String::new(),
            steps: 1,
            poll_ms: 25,
            recovery_index: 0,
            recovery_staging: String::new(),
            recovery_receipt: String::new(),
            retirement_pending: String::new(),
            retirement_sha256: String::new(),
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn request_local_stop(&self) {
        if let Some(job) = &self.job {
            if matches!(job.action, LibraryAction::Run { .. }) {
                job.control.request_stop();
            }
        }
    }
    pub fn submit(&mut self, action: LibraryAction) -> Result<()> {
        ensure!(!self.busy(), "Another desktop batch operation is pending");
        ensure!(
            self.recovery.is_none() || !action.mutating(),
            "Retry the original retained operation before another mutation"
        );
        self.launch(action)
    }
    pub fn retry(&mut self) -> Result<()> {
        ensure!(!self.busy(), "Another desktop batch operation is pending");
        self.launch(
            self.recovery
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No retained operation to retry"))?,
        )
    }
    fn launch(&mut self, action: LibraryAction) -> Result<()> {
        let endpoint = self.endpoint.clone();
        let bootstrap = self.bootstrap.clone();
        let control = WorkflowControl::default();
        let child_control = control.clone();
        let child_action = action.clone();
        let handle = std::thread::Builder::new()
            .name("pulsar-library-client".into())
            .spawn(move || execute(&endpoint, &bootstrap, child_action, &child_control))?;
        self.message = "Working; progress updates when this bounded operation returns.".into();
        self.job = Some(Job {
            handle: Some(handle),
            action,
            control,
        });
        Ok(())
    }
    pub fn poll(&mut self) {
        if !self.job.as_ref().is_some_and(|job| {
            job.handle
                .as_ref()
                .is_some_and(|handle| handle.is_finished())
        }) {
            return;
        }
        // The thread is terminal before joining, so polling never waits on I/O.
        let mut job = self.job.take().expect("terminal job");
        let action = job.action.clone();
        let dispatch_possible = !matches!(
            action,
            LibraryAction::Intake { .. }
                | LibraryAction::IntakeRecipe { .. }
                | LibraryAction::IntakeVisual { .. }
                | LibraryAction::MigrateIntake { .. }
        );
        // A Drop guard must not prevent moving a terminal JoinHandle out of the job.
        let outcome=match job.handle.take().expect("terminal handle").join() {
            Ok(outcome)=>outcome,
            Err(_)=>LibraryOutcome {retry_action:action,result:Err("Desktop batch operation panicked; inspect current status and retain the original operation before retry".into()),dispatch_possible},
        };
        self.accept(outcome);
    }
    fn accept(&mut self, outcome: LibraryOutcome) {
        match outcome.result {
            Ok(reply) => {
                if let LibraryReply::PackSelection(report) = &reply {
                    if self.recovery.is_none() {
                        self.visual_recipe.pack = report
                            .choice
                            .as_ref()
                            .map(|choice| choice.manifest_sha256.clone())
                            .unwrap_or_default();
                    }
                }
                let unresolved = matches!(&reply,LibraryReply::Generation(r) if r.cancellation.as_ref().is_some_and(|c|!c.completed));
                if unresolved && matches!(outcome.retry_action, LibraryAction::Cancel { .. }) {
                    self.recovery = Some(outcome.retry_action.clone());
                } else if outcome.dispatch_possible && outcome.retry_action.mutating() {
                    self.recovery = None;
                }
                if let Some(generation) = reply.generation() {
                    self.batch = generation.batch_request_id.to_string();
                    if let Some(cancel) = &generation.cancellation {
                        self.cancel_id = cancel.request_id.to_string();
                        if self.recovery.as_ref().is_some_and(|action|matches!(action,LibraryAction::Cancel {batch,..} if *batch==generation.batch_request_id)) {
                            self.recovery=if cancel.completed {None}else{Some(LibraryAction::Cancel {batch:generation.batch_request_id.clone(),request_id:cancel.request_id.clone()})};
                        }
                    }
                    if self.recovery.as_ref().is_some_and(|action|matches!(action,LibraryAction::Prepare {request_id,..} if *request_id==generation.batch_request_id)) {
                        self.recovery=None;
                    }
                }
                self.message=match &reply {
                    LibraryReply::PendingRetired(report)=>format!("Pending metadata retained at {}. Scripts, generation and publication acknowledgements are unchanged.",report.retained_path.display()),
                    LibraryReply::Axes(report)=>format!("{} media; {} present, {} missing, {} blocked channels. Presence is unvalidated; generation and publication require separate action.", report.entries.len(), report.present_channels, report.missing_channels, report.blocked_channels),
                    LibraryReply::PackSelection(report)=>if report.choice.is_some() {"Current pack loaded. Capture will check the current approval again.".into()} else {"No pack is selected. Import and approve a pack in Generator before capture.".into()},
                    LibraryReply::Catalog(report)=>format!("{} retained batches. Selection or inspection dispatches no generation.",report.batches.len()),
                    LibraryReply::Workflow(_) if matches!(outcome.retry_action,LibraryAction::RecoverPreparation {..})=>"Original preparation acknowledgement restored. No generation or export was dispatched; inspect history before continuing.".into(),
                    LibraryReply::Released(_)=>"Batch metadata released; project, job and output files remain preserved.".into(),
                    LibraryReply::Workflow(report) if report.cancellation_requested=>"Batch stop is recorded; generation/publication will not continue.".into(),
                    LibraryReply::Workflow(report) if report.local_stop_requested=>"Local scheduling paused after its current operation. Engine work and recovery state remain retained.".into(),
                    LibraryReply::Workflow(report) if report.needs_attention=>"Workflow needs attention; prior outputs and request IDs remain retained.".into(),
                    LibraryReply::Workflow(report) if report.completed=>"All selected entries have publication receipts; output quality remains unqualified.".into(),
                    LibraryReply::Workflow(_)=>"Step budget reached; continue explicitly to schedule more work.".into(),
                    LibraryReply::Generation(report) if report.migration.is_some() && matches!(outcome.retry_action,LibraryAction::Migrate {..})=>"Successor captured. Choose a fresh publication journal and run explicitly; original outputs remain preserved.".into(),
                    LibraryReply::Generation(report) if report.successor_batch_request_id.is_some()=>format!("Original batch migrated to {}. Select and run the successor explicitly.",report.successor_batch_request_id.as_ref().expect("set")),
                    LibraryReply::Generation(report) if report.cancellation.is_some()=>"Stop policy recorded. Inspect cancellation acknowledgements before release.".into(),
                    LibraryReply::Generation(_)=>"Captured generation state. Intake/status does not publish output.".into(),
                };
                self.reply = Some(reply);
            }
            Err(error) => {
                if outcome.dispatch_possible && outcome.retry_action.mutating() {
                    self.recovery = Some(outcome.retry_action);
                }
                self.message=format!("Operation failed: {error}. Read status or retry the retained operation; no replacement was submitted.");
            }
        }
    }
    fn action(&mut self, action: Result<LibraryAction>) {
        let result = action.and_then(|action| self.submit(action));
        if let Err(error) = result {
            self.message = bounded_error(error);
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<ProjectId> {
        let mut open_project = None;
        ui.heading("Local library batches");
        ui.label("Capture media with an explicit audio or visual recipe, then schedule generation and publication. Existing companion outputs are preserved.");
        ui.small("Audio recipes synthesize six axes; existing adjacent stroke is omitted, and existing bundles receive only missing channels. Visual recipes publish committed stroke motion.");
        let idle = !self.busy();
        let mutate = idle && self.recovery.is_none();
        ui.add_enabled_ui(mutate, |ui| {
            ui.label("Media folder");
            ui.add(egui::TextEdit::singleline(&mut self.folder).char_limit(4096));
            ui.checkbox(&mut self.recursive, "Include subfolders");
            ui.checkbox(&mut self.use_recipe_file, "Use a recipe JSON file");
            if self.use_recipe_file {
                ui.label("Audio or visual generation recipe JSON");
                ui.add(egui::TextEdit::singleline(&mut self.recipe).char_limit(4096));
            } else {
                ui.checkbox(&mut self.visual, "Visual stroke recipe");
                if self.visual {
                    let refresh = ui
                        .push_id("library-visual-recipe", |ui| self.visual_recipe.show(ui))
                        .inner;
                    if refresh {
                        self.action(Ok(LibraryAction::PackSelectionStatus));
                    }
                } else {
                    ui.push_id("library-recipe", |ui| self.audio_recipe.bundle_controls(ui));
                }
            }
            ui.label("Preparation request ID (retain for recovery)");
            ui.add(egui::TextEdit::singleline(&mut self.prepare_id).char_limit(128));
            if ui.button("New preparation ID").clicked() {
                self.prepare_id = crate::session::new_request_id().to_string();
            }
            if ui
                .button(if self.use_recipe_file {
                    "Capture batch from JSON"
                } else if self.visual {
                    "Capture new visual batch"
                } else {
                    "Capture new audio batch"
                })
                .clicked()
            {
                self.action((|| {
                    let folder = required_path(&self.folder)?;
                    let request_id = RequestId::new(self.prepare_id.trim())?;
                    if self.use_recipe_file {
                        Ok(LibraryAction::Intake {
                            folder,
                            recursive: self.recursive,
                            recipe_path: required_path(&self.recipe)?,
                            request_id,
                        })
                    } else if self.visual {
                        Ok(LibraryAction::IntakeVisual {
                            folder,
                            recursive: self.recursive,
                            generation: self.visual_recipe.capture()?,
                            video_view: self.visual_recipe.video_view.clone(),
                            request_id,
                        })
                    } else {
                        Ok(LibraryAction::IntakeRecipe {
                            folder,
                            recursive: self.recursive,
                            recipe: self.audio_recipe.bundle_recipe()?,
                            request_id,
                        })
                    }
                })());
            }
        });
        if ui
            .add_enabled(idle, egui::Button::new("List retained batches"))
            .clicked()
        {
            self.action(Ok(LibraryAction::List));
        }
        if ui
            .add_enabled(idle, egui::Button::new("Inspect companion axes"))
            .clicked()
        {
            self.action(
                required_path(&self.folder).map(|folder| LibraryAction::InspectAxes {
                    folder,
                    recursive: self.recursive,
                }),
            );
        }
        if let Some(LibraryReply::Axes(report)) = &self.reply {
            for entry in &report.entries {
                ui.label(entry.source.display().to_string());
                for channel in &entry.channels {
                    ui.label(format!("{}: {:?}", channel.axis.as_str(), channel.presence));
                }
            }
        }
        if let Some(LibraryReply::Catalog(catalog)) = &self.reply {
            for batch in &catalog.batches {
                let label = format!(
                    "{}: {} entries; {}",
                    batch.batch_request_id,
                    batch.item_count,
                    if batch.successor_batch_request_id.is_some() {
                        "migrated; inspect successor"
                    } else if batch.cancellation_requested {
                        "stop requested"
                    } else if batch.generation_ready {
                        "generation ready"
                    } else {
                        "unfinished"
                    }
                );
                if ui.add_enabled(idle, egui::Button::new(label)).clicked() {
                    self.batch = batch.batch_request_id.to_string();
                }
            }
        }
        ui.add_enabled_ui(idle, |ui| {
            ui.label("Retained batch ID");
            ui.add(egui::TextEdit::singleline(&mut self.batch).char_limit(128));
            ui.label("Existing private workflow directory (0700 on Linux)");
            ui.add(egui::TextEdit::singleline(&mut self.directory).char_limit(4096));
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("Inspect generation state"))
                .clicked()
            {
                self.action(
                    RequestId::new(self.batch.trim())
                        .map(|batch| LibraryAction::Status { batch })
                        .map_err(Into::into),
                );
            }
            if ui
                .add_enabled(idle, egui::Button::new("Inspect workflow history"))
                .clicked()
            {
                self.action((|| {
                    Ok(LibraryAction::WorkflowStatus {
                        batch: RequestId::new(self.batch.trim())?,
                        directory: required_path(&self.directory)?,
                    })
                })());
            }
        });
        ui.add_enabled_ui(mutate, |ui| {
            ui.horizontal(|ui| {
                ui.label("Steps");
                ui.add(egui::DragValue::new(&mut self.steps).range(1..=1000));
                ui.label("Poll ms");
                ui.add(egui::DragValue::new(&mut self.poll_ms).range(1..=1000));
            });
            if ui.button("Run bounded steps").clicked() {
                self.action((|| {
                    Ok(LibraryAction::Run {
                        batch: RequestId::new(self.batch.trim())?,
                        directory: required_path(&self.directory)?,
                        steps: self.steps,
                        poll_ms: self.poll_ms,
                    })
                })());
            }
        });
        if ui
            .add_enabled(
                self.job
                    .as_ref()
                    .is_some_and(|job| matches!(job.action, LibraryAction::Run { .. })),
                egui::Button::new("Pause after current operation"),
            )
            .clicked()
        {
            self.request_local_stop();
        }
        ui.small("Pausing stops future local decisions. A current export may finish and an existing Engine job can continue. To stop retained generation, request cancellation when this operation returns.");
        ui.add_enabled_ui(mutate, |ui| {
            ui.label("Replacement recipe JSON for stopped entries");
            ui.add(egui::TextEdit::singleline(&mut self.migration_recipe).char_limit(32768));
            ui.label("Migration request ID");
            ui.add(egui::TextEdit::singleline(&mut self.migration_id).char_limit(128));
            if ui.button("New migration ID").clicked() {self.migration_id=crate::session::new_request_id().to_string();}
            if ui.button("Create successor from inspected batch").clicked() {
                self.action((|| {
                    let batch=RequestId::new(self.batch.trim())?;
                    let report=self.reply.as_ref().and_then(LibraryReply::generation).ok_or_else(||anyhow::anyhow!("Read original batch status first"))?;
                    ensure!(report.batch_request_id==batch,"Read status for the selected original batch first");
                    Ok(LibraryAction::MigrateIntake {batch,expected_report_sha256:report.report_sha256()?,recipe_path:PathBuf::from(self.migration_recipe.trim()),request_id:RequestId::new(self.migration_id.trim())?})
                })());
            }
            ui.small("Migration captures a new recipe without dispatching generation. Use a fresh publication journal for the successor and continue explicitly.");
            ui.label("Cancellation request ID");
            ui.add(egui::TextEdit::singleline(&mut self.cancel_id).char_limit(128));
            if ui.button("New cancellation ID").clicked() {
                self.cancel_id = crate::session::new_request_id().to_string();
            }
            if ui.button("Cancel retained batch").clicked() {
                self.action((|| {
                    Ok(LibraryAction::Cancel {
                        batch: RequestId::new(self.batch.trim())?,
                        request_id: RequestId::new(self.cancel_id.trim())?,
                    })
                })());
            }
            ui.label("Release request ID");
            ui.add(egui::TextEdit::singleline(&mut self.release_id).char_limit(128));
            if ui.button("New release ID").clicked() {
                self.release_id = crate::session::new_request_id().to_string();
            }
            if ui.button("Release eligible metadata").clicked() {
                self.action((|| {
                    Ok(LibraryAction::Release {
                        batch: RequestId::new(self.batch.trim())?,
                        request_id: RequestId::new(self.release_id.trim())?,
                    })
                })());
            }
        });
        ui.add_enabled_ui(mutate, |ui| {
            ui.collapsing("Retire recovered pending metadata", |ui| {
                ui.small("Move an inspected pending journal into retained history after current publication is complete. Bytes remain preserved. Unknown or uncovered effects refuse.");
                ui.label("Original pending metadata path");
                ui.add(egui::TextEdit::singleline(&mut self.retirement_pending).char_limit(4096));
                ui.label("Inspected pending SHA-256");
                ui.add(egui::TextEdit::singleline(&mut self.retirement_sha256).char_limit(64));
                if ui.button("Retire acknowledged pending journal").clicked() {
                    self.action((|| {
                        ensure!(crate::library_publication::valid_retirement_digest(self.retirement_sha256.trim()),"Inspect and supply the exact lowercase pending SHA-256");
                        Ok(LibraryAction::RetirePending { directory:required_path(&self.directory)?,pending:required_path(&self.retirement_pending)?,sha256:self.retirement_sha256.trim().into() })
                    })());
                }
            });
            ui.collapsing("Recover interrupted publication preparation", |ui| {
                ui.small("Select the original retained stage and its original pending receipt. A matching replacement stage is insufficient. Recovery restores metadata and dispatches no exports.");
                ui.horizontal(|ui| {ui.label("Entry index (zero-based)");ui.add(egui::DragValue::new(&mut self.recovery_index).range(0..=99));});
                ui.label("Original staging directory");
                ui.add(egui::TextEdit::singleline(&mut self.recovery_staging).char_limit(4096));
                ui.label("Original .pending receipt in the workflow directory");
                ui.add(egui::TextEdit::singleline(&mut self.recovery_receipt).char_limit(4096));
                if ui.button("Recover original preparation").clicked() {
                    self.action((|| Ok(LibraryAction::RecoverPreparation {
                        batch: RequestId::new(self.batch.trim())?,
                        directory: required_path(&self.directory)?,
                        index: self.recovery_index,
                        staging: required_path(&self.recovery_staging)?,
                        preparation_receipt: required_path(&self.recovery_receipt)?,
                    }))());
                }
            });
        });
        if self.recovery.is_some() {
            ui.label("A retained operation needs acknowledgement recovery. Read-only inspection remains available.");
            if ui
                .add_enabled(idle, egui::Button::new("Retry exact retained operation"))
                .clicked()
            {
                if let Err(error) = self.retry() {
                    self.message = bounded_error(error);
                }
            }
            ui.collapsing("Retained operation and original IDs", |ui| {
                if let Some(action) = &self.recovery {
                    ui.monospace(
                        serde_json::to_string_pretty(action)
                            .unwrap_or_else(|_| "Cannot format retained operation".into()),
                    );
                }
            });
        }
        if self.busy() {
            ui.spinner();
        }
        ui.label(&self.message);
        if let Some(reply) = &self.reply {
            if let Some(report) = reply.generation() {
                ui.label(format!(
                    "Captured batch {}; generation ready: {}",
                    report.batch_request_id, report.generation_ready
                ));
                for (index, item) in report.items.iter().enumerate() {
                    ui.collapsing(
                        format!(
                            "{}: {} — {:?}",
                            index + 1,
                            item.source.display(),
                            item.phase
                        ),
                        |ui| {
                            ui.label(format!("Destination: {}", item.destination.display()));
                            ui.label(format!(
                                "Selected output axes: {}",
                                item.publication_axes()
                                    .iter()
                                    .map(|axis| axis.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                            let omitted = Axis::ALL
                                .into_iter()
                                .filter(|axis| !item.publication_axes().contains(axis))
                                .map(|axis| axis.as_str())
                                .collect::<Vec<_>>();
                            if !omitted.is_empty() {
                                ui.label(format!("Omitted output axes: {}", omitted.join(", ")));
                            }
                            if let Some(project) = &item.project_id {
                                if ui
                                    .add_enabled(idle, egui::Button::new("Open retained project"))
                                    .clicked()
                                {
                                    open_project = Some(project.clone());
                                }
                            }
                            ui.monospace(format!(
                                "project {:?}; job {:?}; candidate {:?}",
                                item.project_id, item.job_id, item.candidate_id
                            ));
                            if let Some(error) = &item.error {
                                ui.colored_label(egui::Color32::YELLOW, error.to_string());
                            }
                            if let LibraryReply::Workflow(workflow) = reply {
                                if let Some(partial) = workflow
                                    .publication
                                    .items
                                    .get(index)
                                    .and_then(|item| item.partial.as_ref())
                                {
                                    for channel in &partial.channels {
                                        ui.label(format!(
                                            "{}: {}",
                                            channel.axis.as_str(),
                                            if channel.acknowledged {
                                                "publication acknowledged"
                                            } else {
                                                "publication pending"
                                            }
                                        ));
                                        if let Some(error) = &channel.error {
                                            ui.colored_label(egui::Color32::YELLOW, error);
                                        }
                                    }
                                }

                                if let Some(publication) = workflow.publication.items.get(index) {
                                    ui.label(format!("Publication: {:?}", publication.phase));
                                    if let Some(error) = &publication.error {
                                        ui.colored_label(egui::Color32::YELLOW, error);
                                    }
                                }
                            }
                        },
                    );
                }
                if let Some(cancel) = &report.cancellation {
                    ui.label(format!(
                        "Stop request {}; acknowledged: {}; physical retirement verified: {}",
                        cancel.request_id, cancel.completed, cancel.worker_retirement_verified
                    ));
                    if let Some(error) = &cancel.error {
                        ui.colored_label(egui::Color32::YELLOW, error.to_string());
                    }
                }
            }
            ui.collapsing("Captured acknowledgements and recovery IDs", |ui| {
                ui.monospace(
                    serde_json::to_string_pretty(reply)
                        .unwrap_or_else(|_| "Cannot format captured report".into()),
                );
            });
        }
        ui.small("Quality, device stopping and release acceptance remain unqualified. Opening this panel or restarting the application schedules no generation.");
        open_project
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn companion_inspection_needs_no_engine_and_preserves_pending_mutation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"media").unwrap();
        std::fs::write(dir.path().join("clip.funscript"), b"protected").unwrap();
        let action = LibraryAction::InspectAxes {
            folder: dir.path().to_owned(),
            recursive: false,
        };
        let outcome = execute(
            &dir.path().join("absent.sock"),
            &dir.path().join("absent.token"),
            action,
            &WorkflowControl::default(),
        );
        assert!(!outcome.dispatch_possible);
        let report = match outcome.result.as_ref().unwrap() {
            LibraryReply::Axes(r) => r,
            _ => panic!("axis report"),
        };
        assert_eq!((report.present_channels, report.missing_channels), (1, 5));
        let mut view = panel(dir.path());
        view.recovery = Some(LibraryAction::Cancel {
            batch: RequestId::new("original").unwrap(),
            request_id: RequestId::new("stop").unwrap(),
        });
        view.accept(outcome);
        assert!(matches!(view.recovery, Some(LibraryAction::Cancel { .. })));
        assert!(view.batch.is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
    fn panel(root: &Path) -> LibraryPanel {
        LibraryPanel::new(root.join("missing.sock"), root.join("pairing.token"))
    }
    fn recipe() -> AudioBundleRecipe {
        serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap()
    }
    fn generation(root: &Path) -> LibraryBatchReport {
        LibraryBatchReport {
            migration: None,
            successor_batch_request_id: None,
            model_pack: None,
            schema: 1,
            batch_request_id: RequestId::new("batch").unwrap(),
            items: vec![LibraryBatchItem {
                publication_directory: None,
                source: root.join("a.wav"),
                destination: root.join("output"),
                source_sha256: "a".repeat(64),
                publication_axes: None,
                source_bytes: 3,
                phase: LibraryBatchPhase::Selected,
                project_id: None,
                source_version: None,
                revision: RevisionId::new(0),
                job_id: None,
                candidate_id: None,
                effects: vec![],
                error: None,
            }],
            pending_advance_request_id: None,
            cancellation: None,
            generation_ready: false,
            publication_completed: false,
            qualified: false,
        }
    }
    fn click(panel: &mut LibraryPanel, label: &str) -> Option<ProjectId> {
        click_with_context(&egui::Context::default(), panel, label)
    }
    fn click_with_context(
        ctx: &egui::Context,
        panel: &mut LibraryPanel,
        label: &str,
    ) -> Option<ProjectId> {
        let mut selected = None;
        let mut draw = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 3000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| selected = panel.show(ui));
                },
            )
        };
        fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                    Some(s.pos + s.galley.rect.center().to_vec2())
                }
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().find_map(|shape| find(shape, label))
                }
                _ => None,
            }
        }
        draw(vec![]);
        let output = draw(vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| find(&shape.shape, label))
            .expect("drawn control");
        draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        draw(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        selected
    }
    #[test]
    fn visual_status_accepts_recorded_stroke_and_refuses_other_axes() {
        let root = tempfile::tempdir().unwrap();
        let mut report = generation(root.path());
        report.schema = 3;
        report.items[0].publication_axes = Some(vec![Axis::Stroke]);
        let action = LibraryAction::Status {
            batch: report.batch_request_id.clone(),
        };
        check_reply(&action, &LibraryReply::Generation(report.clone())).unwrap();
        report.items[0].publication_axes = Some(vec![Axis::Roll]);
        assert!(check_reply(&action, &LibraryReply::Generation(report)).is_err());
    }
    #[test]
    fn opening_and_invalid_controls_do_not_start_engine_or_capture_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        click(&mut view, "Capture new audio batch");
        assert!(!view.busy());
        assert!(view.message.contains("explicit path"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        click(&mut view, "Run bounded steps");
        assert!(!view.busy());
        assert!(view.recovery.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn bounded_intake_preserves_full_names_and_existing_companions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        std::fs::write(path.join("a.wav"), b"selection only").unwrap();
        std::fs::write(path.join("a.flac"), b"selection only").unwrap();
        std::fs::write(path.join("a.funscript"), b"preserve stroke").unwrap();
        std::fs::write(path.join("a.wav.pulsar-audio-bundle"), b"preserve target").unwrap();
        let recipe_path = path.join("recipe.json");
        std::fs::write(&recipe_path, serde_json::to_vec(&recipe()).unwrap()).unwrap();
        let options = intake(path, false, &recipe_path).unwrap();
        assert_eq!(options.inputs.len(), 1);
        assert_eq!(options.inputs[0].source, path.join("a.flac"));
        assert_eq!(
            options.inputs[0].publication_axes,
            Some(Axis::ALL[1..].to_vec())
        );
        assert_eq!(
            std::fs::read(path.join("a.wav.pulsar-audio-bundle")).unwrap(),
            b"preserve target"
        );
        assert_eq!(
            options.inputs[0].destination,
            path.join("a.flac.pulsar-audio-bundle")
        );
        assert_eq!(
            std::fs::read(path.join("a.funscript")).unwrap(),
            b"preserve stroke"
        );
    }
    #[test]
    fn local_intake_errors_keep_form_editable_without_unknown_dispatch_claim() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        let action = LibraryAction::Intake {
            folder: dir.path().to_owned(),
            recursive: false,
            recipe_path: dir.path().join("missing.json"),
            request_id: RequestId::new("prepare").unwrap(),
        };
        let outcome = execute(
            &view.endpoint,
            &view.bootstrap,
            action,
            &WorkflowControl::default(),
        );
        assert!(!outcome.dispatch_possible);
        view.accept(outcome);
        assert!(view.recovery.is_none());
        assert!(!view.busy());
    }
    #[test]
    fn intake_captures_recipe_and_selection_before_possible_dispatch_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.wav"), b"selection only").unwrap();
        let recipe_path = dir.path().join("recipe.json");
        std::fs::write(&recipe_path, serde_json::to_vec(&recipe()).unwrap()).unwrap();
        let mut view = panel(dir.path());
        let outcome = execute(
            &view.endpoint,
            &view.bootstrap,
            LibraryAction::Intake {
                folder: dir.path().to_owned(),
                recursive: false,
                recipe_path: recipe_path.clone(),
                request_id: RequestId::new("prepare").unwrap(),
            },
            &WorkflowControl::default(),
        );
        assert!(outcome.result.is_err());
        assert!(outcome.dispatch_possible);
        let LibraryAction::Prepare {
            options,
            request_id,
        } = &outcome.retry_action
        else {
            panic!("captured request");
        };
        assert_eq!(request_id.as_str(), "prepare");
        assert_eq!(options.inputs.len(), 1);
        let captured = serde_json::to_value(&outcome.retry_action).unwrap();
        std::fs::write(recipe_path, b"changed invalid recipe").unwrap();
        view.accept(outcome);
        assert_eq!(
            serde_json::to_value(view.recovery.as_ref().unwrap()).unwrap(),
            captured
        );
        assert!(view
            .submit(LibraryAction::Cancel {
                batch: RequestId::new("batch").unwrap(),
                request_id: RequestId::new("different").unwrap()
            })
            .is_err());
    }
    #[test]
    fn authoritative_status_recovers_intake_and_uses_retained_cancellation_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        view.recovery = Some(LibraryAction::Prepare {
            options: LibraryBatchOptions {
                inputs: vec![LibraryBatchInput {
                    publication_directory: None,
                    publication_axes: None,
                    source: dir.path().join("a.wav"),
                    destination: dir.path().join("output"),
                }],
                recipe: recipe().into(),
            },
            request_id: RequestId::new("batch").unwrap(),
        });
        let mut report = generation(dir.path());
        view.accept(LibraryOutcome {
            retry_action: LibraryAction::Status {
                batch: report.batch_request_id.clone(),
            },
            result: Ok(LibraryReply::Generation(report.clone())),
            dispatch_possible: true,
        });
        assert!(view.recovery.is_none());
        view.recovery = Some(LibraryAction::Cancel {
            batch: report.batch_request_id.clone(),
            request_id: RequestId::new("wrong").unwrap(),
        });
        report.schema = 2;
        report.cancellation = Some(LibraryBatchCancellation {
            request_id: RequestId::new("original-cancel").unwrap(),
            jobs: vec![],
            resolved_generation_advance_request_id: None,
            completed: false,
            worker_retirement_verified: false,
            error: Some(ProtocolError::invalid(
                "authored unresolved acknowledgement",
            )),
        });
        view.accept(LibraryOutcome {
            retry_action: LibraryAction::Status {
                batch: report.batch_request_id.clone(),
            },
            result: Ok(LibraryReply::Generation(report.clone())),
            dispatch_possible: true,
        });
        assert_eq!(view.cancel_id, "original-cancel");
        assert!(
            matches!(view.recovery,Some(LibraryAction::Cancel {ref request_id,..}) if request_id.as_str()=="original-cancel")
        );
        report.cancellation.as_mut().unwrap().completed = true;
        report.cancellation.as_mut().unwrap().error = None;
        view.accept(LibraryOutcome {
            retry_action: LibraryAction::Status {
                batch: report.batch_request_id.clone(),
            },
            result: Ok(LibraryReply::Generation(report)),
            dispatch_possible: true,
        });
        assert!(view.recovery.is_none());
    }
    #[test]
    fn response_scope_and_qualification_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = generation(dir.path());
        let action = LibraryAction::Status {
            batch: RequestId::new("different").unwrap(),
        };
        assert!(check_reply(&action, &LibraryReply::Generation(report.clone())).is_err());
        let action = LibraryAction::Status {
            batch: report.batch_request_id.clone(),
        };
        report.qualified = true;
        assert!(check_reply(&action, &LibraryReply::Generation(report)).is_err());
    }
    #[test]
    fn pause_is_local_and_duplicate_mutations_are_disabled_until_terminal_join() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        let (sender, receiver) = std::sync::mpsc::channel();
        let control = WorkflowControl::default();
        let observed = control.clone();
        let action = LibraryAction::Run {
            batch: RequestId::new("batch").unwrap(),
            directory: dir.path().to_owned(),
            steps: 10,
            poll_ms: 1,
        };
        let captured = action.clone();
        view.job = Some(Job {
            handle: Some(std::thread::spawn(move || {
                receiver
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                LibraryOutcome {
                    retry_action: captured,
                    result: Err("authored transport failure".into()),
                    dispatch_possible: true,
                }
            })),
            action,
            control,
        });
        click(&mut view, "Cancel retained batch");
        assert!(!observed.stopped());
        assert!(view.submit(LibraryAction::List).is_err());
        click(&mut view, "Pause after current operation");
        assert!(observed.stopped());
        sender.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while view.busy() {
            view.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(view.recovery.is_some());
    }
    #[test]
    fn inline_editor_button_freezes_recipe_before_background_intake_and_recovery() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.wav"), b"captured input").unwrap();
        let mut view = panel(dir.path());
        view.folder = dir.path().to_string_lossy().into_owned();
        view.prepare_id = "inline-original".into();
        let original = recipe();
        view.audio_recipe.set_bundle_recipe(&original);
        click(&mut view, "Capture new audio batch");
        assert!(view.busy());
        let edited: AudioBundleRecipe = serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.1,"offset":0.2},"mode":"beats","analysis":{"band":"bass","threshold":0.2}})).collect::<Vec<_>>()})).unwrap();
        view.audio_recipe.set_bundle_recipe(&edited);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while view.busy() {
            view.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let Some(LibraryAction::Prepare {
            options,
            request_id,
        }) = &view.recovery
        else {
            panic!("frozen intake recovery")
        };
        assert_eq!(request_id.as_str(), "inline-original");
        assert_eq!(options.inputs.len(), 1);
        assert_eq!(
            serde_json::to_value(&options.recipe).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        assert_ne!(
            serde_json::to_value(&options.recipe).unwrap(),
            serde_json::to_value(edited).unwrap()
        );
        assert!(!dir.path().join("recipe.json").exists());
        assert!(view
            .submit(LibraryAction::IntakeRecipe {
                folder: dir.path().into(),
                recursive: false,
                recipe: recipe(),
                request_id: RequestId::new("replacement").unwrap()
            })
            .is_err());
    }
    #[test]
    fn inline_intake_discovery_failure_keeps_recipe_editable_without_recovery_or_rpc() {
        let dir = tempfile::tempdir().unwrap();
        let action = LibraryAction::IntakeRecipe {
            folder: dir.path().join("absent"),
            recursive: false,
            recipe: recipe(),
            request_id: RequestId::new("inline-refused").unwrap(),
        };
        let outcome = execute(
            &dir.path().join("absent.sock"),
            &dir.path().join("absent.token"),
            action,
            &WorkflowControl::default(),
        );
        assert!(!outcome.dispatch_possible);
        assert!(outcome.result.is_err());
        let mut view = panel(dir.path());
        view.accept(outcome);
        assert!(view.recovery.is_none());
        assert!(!view.use_recipe_file);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn visual_editor_button_freezes_form_and_prepared_recipe_for_recovery() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.mp4"), b"selection only").unwrap();
        let mut view = panel(dir.path());
        view.folder = dir.path().to_string_lossy().into_owned();
        view.visual = true;
        view.visual_recipe.stride = 3;
        click(&mut view, "Declare stereo / spherical source view");
        let declared = VideoView {
            layout: StereoLayout::SideBySideRightLeft,
            eye: StereoEye::Right,
            projection: VideoProjection::Flat,
        };
        assert!(view.visual_recipe.video_view.is_some());
        view.visual_recipe.video_view = Some(declared.clone());
        view.prepare_id = "visual-editor-original".into();
        click(&mut view, "Capture new visual batch");
        assert!(view.busy());
        view.visual_recipe.stride = 7;
        view.visual_recipe.video_view = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while view.busy() {
            view.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let Some(LibraryAction::Prepare {
            options,
            request_id,
        }) = &view.recovery
        else {
            panic!("captured visual recipe")
        };
        assert_eq!(request_id.as_str(), "visual-editor-original");
        let LibraryBatchRecipe::Visual(recipe) = &options.recipe else {
            panic!("visual")
        };
        assert_eq!(recipe.settings.source_sample_stride, 3);
        assert_eq!(recipe.settings.video_view, Some(declared));
        assert_eq!(options.inputs.len(), 1);
        assert!(options.inputs[0]
            .destination
            .ends_with("one.mp4.pulsar-visual-bundle"));
        assert!(!dir.path().join("recipe.json").exists());
    }
    #[test]
    fn visual_profile_is_loaded_off_frame_and_invalid_profile_is_editable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.mp4"), b"selection only").unwrap();
        let mut editor = crate::library_visual_panel::LibraryVisualPanel {
            use_model: true,
            model: dir
                .path()
                .join("missing-model.onnx")
                .to_string_lossy()
                .into(),
            use_profile: true,
            profile: dir
                .path()
                .join("missing-profile.json")
                .to_string_lossy()
                .into(),
            ..Default::default()
        };
        let generation = editor.capture().unwrap();
        editor.profile.clear();
        let action = LibraryAction::IntakeVisual {
            folder: dir.path().into(),
            recursive: false,
            generation,
            video_view: None,
            request_id: RequestId::new("invalid-profile").unwrap(),
        };
        let outcome = execute(
            &dir.path().join("missing.sock"),
            &dir.path().join("missing.token"),
            action,
            &WorkflowControl::default(),
        );
        assert!(!outcome.dispatch_possible);
        assert!(outcome.result.is_err());
        let mut view = panel(dir.path());
        view.accept(outcome);
        assert!(view.recovery.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn drawn_pack_control_excludes_file_model_and_invalid_digest_never_starts() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        view.visual = true;
        view.visual_recipe.use_model = true;
        view.folder = dir.path().to_string_lossy().into();
        click(&mut view, "Use current selected pack");
        assert!(view.visual_recipe.use_pack && !view.visual_recipe.use_model);
        click(&mut view, "Capture new visual batch");
        assert!(!view.busy() && view.recovery.is_none());
        assert!(view.message.contains("Selected pack digest"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn missing_pack_owner_proof_keeps_intake_editable_without_batch_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.mp4"), b"discovery only").unwrap();
        let editor = crate::library_visual_panel::LibraryVisualPanel {
            use_pack: true,
            pack: "a".repeat(64),
            ..Default::default()
        };
        let action = LibraryAction::IntakeVisual {
            folder: dir.path().into(),
            recursive: false,
            generation: editor.capture().unwrap(),
            video_view: None,
            request_id: RequestId::new("pack-owner-refusal").unwrap(),
        };
        let outcome = execute(
            &dir.path().join("missing.sock"),
            &dir.path().join("pairing.token"),
            action,
            &WorkflowControl::default(),
        );
        assert!(!outcome.dispatch_possible && outcome.result.is_err());
        let LibraryAction::IntakeVisual { generation, .. } = &outcome.retry_action else {
            panic!("unchanged intake")
        };
        assert_eq!(generation.model_pack, Some("a".repeat(64)));
        let mut view = panel(dir.path());
        view.accept(outcome);
        assert!(view.recovery.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn drawn_pack_refresh_is_background_and_missing_owner_proof_keeps_form_editable() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        view.visual = true;
        view.visual_recipe.use_pack = true;
        view.visual_recipe.pack = "a".repeat(64);
        click(&mut view, "Load current selected pack");
        assert!(view.busy());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while view.busy() {
            view.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(view.recovery.is_none());
        assert_eq!(view.visual_recipe.pack, "a".repeat(64));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn inactive_pack_refresh_clears_stale_digest_and_never_rewrites_pending_preparation() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = panel(dir.path());
        let report = ModelPackSelectionReport {
            schema_version: 1,
            catalog_binding: None,
            selection_revision: 0,
            choice: None,
            profile: None,
            native_execution_approved: false,
            selected_by: None,
            selection_request: None,
            rights_verified: false,
            compatible: false,
            qualified: false,
        };
        report.validate().unwrap();
        assert!(!LibraryAction::PackSelectionStatus.mutating());
        assert!(check_reply(
            &LibraryAction::List,
            &LibraryReply::PackSelection(Box::new(report.clone()))
        )
        .is_err());
        view.visual_recipe.pack = "a".repeat(64);
        view.accept(LibraryOutcome {
            retry_action: LibraryAction::PackSelectionStatus,
            result: Ok(LibraryReply::PackSelection(Box::new(report.clone()))),
            dispatch_possible: true,
        });
        assert!(view.visual_recipe.pack.is_empty() && view.recovery.is_none());
        view.visual_recipe.pack = "b".repeat(64);
        view.recovery = Some(LibraryAction::IntakeRecipe {
            folder: dir.path().into(),
            recursive: false,
            recipe: recipe(),
            request_id: RequestId::new("retained-intent").unwrap(),
        });
        let original = serde_json::to_value(&view.recovery).unwrap();
        view.accept(LibraryOutcome {
            retry_action: LibraryAction::PackSelectionStatus,
            result: Ok(LibraryReply::PackSelection(Box::new(report))),
            dispatch_possible: true,
        });
        assert_eq!(view.visual_recipe.pack, "b".repeat(64));
        assert_eq!(serde_json::to_value(&view.recovery).unwrap(), original);
    }
    #[test]
    fn drawn_recovery_control_refuses_missing_paths_without_effects() {
        let root = tempfile::tempdir().unwrap();
        let mut view = panel(root.path());
        view.batch = "original-batch".into();
        let ctx = egui::Context::default();
        click_with_context(
            &ctx,
            &mut view,
            "Recover interrupted publication preparation",
        );
        click_with_context(&ctx, &mut view, "Recover original preparation");
        assert!(!view.busy());
        assert!(view.recovery.is_none());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        assert!(view.message.contains("explicit path"));
    }
    #[test]
    fn drawn_recovery_capture_keeps_all_original_inputs_on_unacknowledged_metadata_write() {
        let root = tempfile::tempdir().unwrap();
        let mut view = panel(root.path());
        view.batch = "original-batch".into();
        view.directory = root.path().join("workflow").to_string_lossy().into();
        view.recovery_index = 3;
        view.recovery_staging = root.path().join("original-stage").to_string_lossy().into();
        view.recovery_receipt = root
            .path()
            .join("workflow/.pending-original")
            .to_string_lossy()
            .into();
        let ctx = egui::Context::default();
        click_with_context(
            &ctx,
            &mut view,
            "Recover interrupted publication preparation",
        );
        click_with_context(&ctx, &mut view, "Recover original preparation");
        assert!(view.busy());
        let action = view.job.as_ref().unwrap().action.clone();
        assert!(matches!(
            &action,
            LibraryAction::RecoverPreparation { index: 3, .. }
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while view.busy() {
            view.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            view.recovery.is_none(),
            "connection failure cannot publish recovery metadata"
        );
        view.accept(LibraryOutcome {
            retry_action: action.clone(),
            result: Err("write acknowledgement lost".into()),
            dispatch_possible: true,
        });
        view.recovery_staging = "edited replacement".into();
        view.recovery_receipt = "edited receipt".into();
        assert_eq!(
            serde_json::to_value(view.recovery.as_ref().unwrap()).unwrap(),
            serde_json::to_value(action).unwrap()
        );
        assert!(view
            .submit(LibraryAction::Cancel {
                batch: RequestId::new("original-batch").unwrap(),
                request_id: RequestId::new("replacement").unwrap()
            })
            .is_err());
    }
    #[test]
    fn migration_intake_failure_is_editable_and_success_captures_exact_recipe_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let recipe_path = dir.path().join("replacement.json");
        let report = generation(dir.path());
        let action = LibraryAction::MigrateIntake {
            batch: report.batch_request_id.clone(),
            expected_report_sha256: report.report_sha256().unwrap(),
            recipe_path: recipe_path.clone(),
            request_id: RequestId::new("migration-intent").unwrap(),
        };
        let missing = execute(
            &dir.path().join("absent.sock"),
            dir.path(),
            action.clone(),
            &WorkflowControl::default(),
        );
        assert!(!missing.dispatch_possible);
        assert!(matches!(
            missing.retry_action,
            LibraryAction::MigrateIntake { .. }
        ));
        let audio:AudioBundleRecipe=serde_json::from_value(serde_json::json!({"channels":Axis::ALL.iter().map(|axis|serde_json::json!({"mapping":{"axis":axis,"amplitude":0.2,"offset":0.4},"mode":"envelope","analysis":{"band":"full","threshold":0.05}})).collect::<Vec<_>>()})).unwrap();
        std::fs::write(&recipe_path, serde_json::to_vec(&audio).unwrap()).unwrap();
        let captured = execute(
            &dir.path().join("absent.sock"),
            dir.path(),
            action,
            &WorkflowControl::default(),
        );
        assert!(captured.dispatch_possible);
        assert!(matches!(
            captured.retry_action,
            LibraryAction::Migrate { .. }
        ));
        let saved = serde_json::to_vec(&captured.retry_action).unwrap();
        std::fs::write(recipe_path, b"changed input file").unwrap();
        let retry = execute(
            &dir.path().join("absent.sock"),
            dir.path(),
            captured.retry_action,
            &WorkflowControl::default(),
        );
        assert_eq!(serde_json::to_vec(&retry.retry_action).unwrap(), saved);
    }
    #[test]
    fn inline_view_refuses_invalid_or_conflicting_json_before_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("one.mp4"), b"selection only").unwrap();
        let editor = crate::library_visual_panel::LibraryVisualPanel::default();
        let valid = VideoView {
            layout: StereoLayout::Mono,
            eye: StereoEye::Left,
            projection: VideoProjection::Flat,
        };
        for conflict in [false, true] {
            let mut generation = editor.capture().unwrap();
            if conflict {
                generation.video_view = Some(dir.path().join("never-read.json"));
            }
            let view = if conflict {
                valid.clone()
            } else {
                VideoView {
                    eye: StereoEye::Right,
                    ..valid.clone()
                }
            };
            let action = LibraryAction::IntakeVisual {
                folder: dir.path().into(),
                recursive: false,
                generation,
                video_view: Some(view),
                request_id: RequestId::new("refused-view").unwrap(),
            };
            let outcome = execute(
                &dir.path().join("missing.sock"),
                &dir.path().join("missing.token"),
                action,
                &WorkflowControl::default(),
            );
            assert!(!outcome.dispatch_possible);
            assert!(outcome.result.is_err());
            assert!(matches!(
                outcome.retry_action,
                LibraryAction::IntakeVisual { .. }
            ));
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}
