#![forbid(unsafe_code)]
//! First-party presentation clients. All authoritative work crosses the same
//! versioned protocol; this crate cannot import the engine implementation.

#[cfg(test)]
fn test_host_path(coordinate: &str) -> String {
    std::env::temp_dir()
        .join("pulsar-clients-authored-fixtures")
        .join(coordinate.trim_start_matches('/'))
        .to_str()
        .expect("test temporary directory is UTF-8")
        .to_owned()
}

mod assistant_settings;
#[doc(hidden)]
pub mod audio_panel;
pub use assistant_settings::{
    AssistantHostReply, AssistantHostRequest, AssistantHostService, AssistantHostSetup,
};
mod bundled_models;
pub mod cli;
pub mod library_axis_inventory;
mod library_generation;
#[doc(hidden)]
pub mod library_panel;
mod library_publication;
pub mod library_visual_panel;
mod video_view_panel;
pub use bundled_models::BundledModelDiscovery;
mod browser_handy_hsp;
mod browser_handy_playback;
mod browser_live;
mod browser_live_panel;
mod browser_player_control;
mod desktop;
mod editor;
mod equivalence_panel;
mod evidence_panel;
mod evidence_recovery;
mod gap_resolution;
pub mod handy_browser_start;
pub mod handy_continuous_timing;
pub mod handy_hosting;
pub mod handy_hsp;
pub mod handy_hsp_live;
pub mod handy_hsp_preparation;
pub mod handy_hsp_source_qualification;
#[doc(hidden)]
pub mod handy_panel;
pub mod handy_playback;
pub mod handy_qualification;
pub mod handy_reconciliation;
pub mod handy_registration;
mod handy_registration_panel;
pub mod handy_registration_recovery;
pub mod handy_script;
pub mod handy_session;
pub mod handy_setup;
pub mod handy_stop;
mod image_origin_pixels;
mod inference;
pub mod inference_equivalence;
pub mod inference_equivalence_recovery;
mod live_stream;
mod modality;
pub mod motion;
pub mod motion_evidence;
pub mod motion_evidence_export;
mod native_preparation_panel;
mod native_preparation_recovery;
pub mod neutral_bundle;
mod neutral_bundle_panel;
mod pack_panel;
mod pattern_review;
mod patterns;
#[doc(hidden)]
pub mod repair_panel;
mod script_repair;
pub use patterns::build_pattern_input;
#[doc(hidden)]
pub mod pack_recovery;
#[doc(hidden)]
pub mod portable;
mod preview;
mod private_recovery;
mod reference_panel;
mod reference_set;
mod runtime_bootstrap;
pub use runtime_bootstrap::run_runtime_bootstrap;
mod stash_flow;
mod target_picker;
mod target_view_authoring;
mod wasm_archive_recovery;
mod wasm_cleanup;
mod wasm_panel;
mod wasm_recovery;
pub use motion::{CandidateSnapshot, ProjectSnapshot, ResponseBody};
#[cfg(unix)]
mod package_staging_recovery;
pub mod project_package;
pub mod project_package_import;
pub mod session;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser};
use cli::{Cli, Commands, ModelCommands, ProjectCommands};
use pulsar_protocol::*;
pub use session::{EngineApi, SessionClient};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub use desktop::PulsarDesktop;
mod assistant_client;
mod assistant_edit_intent;
mod assistant_generation_intent;
mod assistant_panel;
mod live_evidence;
mod wasm_edit_client;
pub use preview::PreviewState;

/// Used by the executable composition root before daemon discovery. Help,
/// version, and completion generation never need a running engine.
pub fn command_needs_engine(args: &[OsString]) -> bool {
    match Cli::try_parse_from(args) {
        Ok(cli) => !matches!(
            cli.command,
            Some(
                Commands::Completions { .. }
                    | Commands::ReferenceSetCreate { .. }
                    | Commands::ReferenceSetInspect { .. }
                    | Commands::LibraryAxes { .. }
                    | Commands::MotionEvidenceCheck { .. }
                    | Commands::AssistantSetup { .. }
                    | Commands::AssistantSelect { .. }
                    | Commands::AssistantDisable { .. }
                    | Commands::RuntimeInstall { .. }
                    | Commands::RuntimeUpdate { .. }
                    | Commands::RuntimeUpdatePack { .. }
                    | Commands::RuntimePack { .. }
                    | Commands::RuntimeLaunch { .. }
                    | Commands::RuntimeCacheLaunch { .. }
                    | Commands::RuntimeCacheInspect { .. }
                    | Commands::RuntimeModels { .. }
                    | Commands::BundledModels
                    | Commands::RuntimeStageInspect { .. }
                    | Commands::RuntimeArchiveStageInspect { .. }
                    | Commands::RuntimeArchiveStageDiscard { .. }
                    | Commands::RuntimeStageDiscard { .. }
            )
        ),
        Err(_) => false,
    }
}

/// Local host grammar extraction for the executable composition root only.
/// The clients do not write host policy or acquire Engine authority.
pub fn assistant_setup_command(args: &[OsString]) -> Option<Commands> {
    match Cli::try_parse_from(args) {
        Ok(Cli {
            command:
                Some(
                    command @ (Commands::AssistantSetup { .. }
                    | Commands::AssistantSelect { .. }
                    | Commands::AssistantDisable { .. }),
                ),
        }) => Some(command),
        _ => None,
    }
}

/// Offline installation grammar for trusted executable host execution only.
pub fn runtime_install_command(args: &[OsString]) -> Option<Commands> {
    match Cli::try_parse_from(args) {
        Ok(Cli {
            command:
                Some(
                    command @ (Commands::RuntimeLaunch { .. }
                    | Commands::RuntimeCacheLaunch { .. }
                    | Commands::RuntimeCacheInspect { .. }
                    | Commands::RuntimeModels { .. }
                    | Commands::BundledModels
                    | Commands::RuntimePack { .. }
                    | Commands::RuntimeInstall { .. }
                    | Commands::RuntimeUpdate { .. }
                    | Commands::RuntimeUpdatePack { .. }
                    | Commands::RuntimeStageInspect { .. }
                    | Commands::RuntimeArchiveStageInspect { .. }
                    | Commands::RuntimeArchiveStageDiscard { .. }
                    | Commands::RuntimeStageDiscard { .. }),
                ),
        }) => Some(command),
        _ => None,
    }
}

/// Only valid graphical invocations may recover from launcher startup failure.
pub fn command_opens_gui(args: &[OsString]) -> bool {
    matches!(
        Cli::try_parse_from(args),
        Ok(Cli {
            command: None | Some(Commands::Gui),
            ..
        })
    )
}

/// Classify the arguments forwarded by the packaged installation launcher.
pub fn runtime_launch_opens_gui(arguments: &[OsString]) -> bool {
    let args = std::iter::once(OsString::from("pulsar"))
        .chain(arguments.iter().cloned())
        .collect::<Vec<_>>();
    command_opens_gui(&args)
}

pub fn run_gui(endpoint: PathBuf, bootstrap: PathBuf) -> Result<()> {
    run_gui_with_startup_warning(endpoint, bootstrap, None)
}

/// Opens the normal authenticated desktop with a retained launcher diagnostic.
/// This does not retry engine spawning or grant any engine/device authority.
pub fn run_gui_with_startup_warning(
    endpoint: PathBuf,
    bootstrap: PathBuf,
    warning: Option<String>,
) -> Result<()> {
    run_gui_with_bundled_discovery(endpoint, bootstrap, warning, None)
}

pub fn run_gui_with_bundled_discovery(
    endpoint: PathBuf,
    bootstrap: PathBuf,
    warning: Option<String>,
    discovery: Option<BundledModelDiscovery>,
) -> Result<()> {
    run_gui_with_host_services(endpoint, bootstrap, warning, discovery, None)
}
pub fn run_gui_with_host_services(
    endpoint: PathBuf,
    bootstrap: PathBuf,
    warning: Option<String>,
    discovery: Option<BundledModelDiscovery>,
    assistant_host: Option<AssistantHostService>,
) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([760.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Pulsar Desktop",
        options,
        Box::new(move |context| {
            let mut desktop = PulsarDesktop::new(context, endpoint, bootstrap);
            desktop.set_startup_warning(warning);
            if let Some(service) = assistant_host {
                desktop.configure_assistant_host(service);
            }
            if let Some(discovery) = discovery {
                desktop.configure_bundled_discovery(discovery);
            }
            Ok(Box::new(desktop))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Desktop startup failed: {error}"))
}

/// Parse informational output before per-user state or worker discovery.
/// Operational commands return false for the trusted host's ordinary dispatch.
pub fn run_informational_command(args: &[OsString]) -> Result<bool> {
    match Cli::try_parse_from(args) {
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            Ok(true)
        }
        Err(error) => Err(error.into()),
        Ok(Cli {
            command: Some(Commands::Completions { shell }),
        }) => {
            clap_complete::generate(shell, &mut Cli::command(), "pulsar", &mut std::io::stdout());
            Ok(true)
        }
        Ok(_) => Ok(false),
    }
}

pub fn run_cli(args: Vec<OsString>, endpoint: &Path, bootstrap: &Path) -> Result<()> {
    let cli = Cli::parse_from(args);
    let Some(command) = cli.command else {
        return run_gui(endpoint.to_owned(), bootstrap.to_owned());
    };
    match command {
        Commands::JobAttempts {project,job,after_attempt,limit} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::JobAttempts {job_id:JobId::new(job)?,after_attempt:after_attempt.map(AttemptId::new).transpose()?,limit},Some(ProjectId::new(project)?),None)?)
        }
        Commands::ReferenceSetCreate { root,video,reference,view,id,group,authorization,reference_offset_ms,destination } => {
            let entry=reference_set::load_entry(&root,reference_set::ReferenceEntryInput {
                id,group,video,reference,view:reference_set::ReferenceViewInput::File(view),offset_ms:reference_offset_ms,
            })?;
            let destination=reference_set::export_new(&destination,&authorization,&[entry])?;
            print_json(&serde_json::json!({"directory":destination,"qualified":false,"alignment":"declared_unreviewed"}))
        }
        Commands::ReferenceSetInspect { directory } => {
            let (_,entries)=reference_set::load_set(&directory)?;
            print_json(&serde_json::json!({"qualified":false,"alignment":"declared_unreviewed","entries":entries.iter().map(|e|serde_json::json!({"id":e.id,"group":e.group,"source_sha256":e.source_sha256,"reference_sha256":e.reference_sha256,"reference_offset_ms":e.offset_ms,"points":e.curve.points().len()})).collect::<Vec<_>>()}))
        }
        Commands::ExportDeviceMotion { project, path, profile } => {
            let bytes = inference::read_bounded_profile(&profile)?;
            let profile: DynamicsProfile = serde_json::from_slice(&bytes).context("invalid dynamics profile")?;
            profile.validate()?;
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let project = get_snapshot(&mut client, &ProjectId::new(project)?)?;
            print_response(client.execute(Command::ExportDeviceMotion { path:absolute_path(&path)?,profile },
                Some(project.project_id),Some(project.revision))?)
        }
        Commands::Gui => run_gui(endpoint.to_owned(), bootstrap.to_owned()),
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "pulsar", &mut std::io::stdout());
            Ok(())
        }
        Commands::PatternCandidate {project,file,revision} => {
            let input=patterns::read_pattern_input(&file)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            patterns::stage(&mut client,&project,RevisionId::new(revision),input)
        }
        Commands::Synthesize { prompt, image, output } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let input = match image {
                Some(path) => modality::Input::Image { path, prompt },
                None => modality::Input::Text { prompt },
            };
            modality::synthesize(&mut client, input, &output.unwrap_or_else(|| PathBuf::from("pulsar-synthetic.funscript")))
        }
        Commands::AudioMap { video, output, mode, axis, amplitude, offset } => {
            let mode = match mode.as_str() {
                "envelope" => AudioMode::Envelope, "beats" => AudioMode::Beats,
                "vibe" | "osc" | "oscillation" => AudioMode::Vibe,
                _ => return unsupported("audio mapping mode; choose envelope, beats or vibe"),
            };
            let mapping = AudioMapping::new(modality::parse_axis(&axis)?, amplitude, NormalizedPosition::new(offset)?)?;
            let output = output.unwrap_or_else(|| video.with_extension("funscript"));
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::synthesize(&mut client, modality::Input::Audio { path: video, mapping, mode, analysis: None }, &output)
        }
        Commands::EditValues {project,file,revision,label} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            modality::edit_values(&mut client,&project,&file,RevisionId::new(revision),&label)
        },
        Commands::GapCandidate {project,gaps,revision,axis,linear_edge_hold} => {
            anyhow::ensure!(linear_edge_hold,"explicit linear/edge-hold policy required");
            let selected:Vec<TimeRange>=serde_json::from_slice(&inference::read_bounded_profile(&gaps)?)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let candidate=gap_resolution::propose(&mut client,ProjectId::new(project)?,RevisionId::new(revision),modality::parse_axis(&axis)?,&selected)?;
            eprintln!("Authored gap-fill proposal; original knot evidence and shared span receipts retained. Review and commit separately.");
            print_response(ResponseBody::Candidate(Box::new(candidate)))
        },
        Commands::CommitCandidate {project,candidate,revision,request_id} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute_with_id(RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?,Command::CommitCandidate {candidate_id:CandidateId::new(candidate)?},Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))?)
        },
        Commands::Protection {command:cli::ProtectionCommands::Status {project}} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;modality::review(&mut client,&project,None,None)
        },
        Commands::Protection {command:cli::ProtectionCommands::Set {project,regions,revision,request_id}} => {
            let regions:Vec<ProtectedRegion>=serde_json::from_slice(&inference::read_bounded_profile(&regions)?)?;
            anyhow::ensure!(regions.len()<=1024,"Protection list exceeds 1024 regions");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute_with_id(RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?,Command::SetProtectedRegions {regions},Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))?)
        },
        Commands::Regenerate {project,source_version,revision,settings,model,preset} => {
            let settings=settings.as_deref().map(|path|->anyhow::Result<GenerationSettings>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?.unwrap_or_default();settings.validate()?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            modality::regenerate(&mut client,&project,&source_version,RevisionId::new(revision),settings,model.as_deref(),&preset)
        },
        Commands::ImportScript { script, project } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::import_script(&mut client, &script, project.as_deref())
        }
        Commands::Review { project, candidate, max_speed } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::review(&mut client, &project, candidate.as_deref(), max_speed)
        }
        Commands::ReviewStatus {project} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::ReviewStatus,Some(ProjectId::new(project)?),None)?)
        },
        Commands::ReviewDisposition {project,review_id,disposition,note,revision,request_id,start_ns,end_ns} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let disposition=if disposition=="resolved" {pulsar_protocol::ReviewDisposition::Resolved}else{pulsar_protocol::ReviewDisposition::Reopened};
            let intent=if let (Some(start),Some(end))=(start_ns,end_ns){Command::SetReviewIntervalDisposition {review_id,range:pulsar_protocol::TimeRange::new(pulsar_protocol::ProjectTime::from_nanos(start),pulsar_protocol::ProjectTime::from_nanos(end))?,disposition,note}}else{Command::SetReviewDisposition {review_id,disposition,note}};
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            eprintln!("Review decision request ID: {id}");
            print_response(client.execute_identified(id,intent,Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))?)
        },
        Commands::MergeCandidate { project, candidate, axes, start, end, output } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::merge(&mut client, &project, &candidate, &axes, start, end, output.as_deref())
        }
        Commands::ExportAxis { project, path, axis } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let project = get_snapshot(&mut client, &ProjectId::new(project)?)?;
            print_response(client.execute(Command::ExportAxis { path: absolute_path(&path)?, axis: modality::parse_axis(&axis)? },
                Some(project.project_id), Some(project.revision))?)
        }
        Commands::ExportBundle { project, destination, revision, axes } => {
            let selected=if axes.is_empty() {Axis::ALL.to_vec()} else {axes.iter().map(|axis|modality::parse_axis(axis)).collect::<Result<Vec<_>>>()?};
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            print_json(&neutral_bundle::export_neutral_axis_bundle(&mut client, ProjectId::new(project)?, RevisionId::new(revision),
                &absolute_path(&destination)?, &selected, &project_package::PackageDownloadControl::default())?)
        }
        Commands::LibraryAxes { folder, recursive } => {
            print_json(&library_axis_inventory::inspect(&folder, recursive)?)
        }
        Commands::LibraryBatchMigrate { batch, observed_report, recipe, request_id } => {
            let bytes=inference::read_bounded_profile(&observed_report)?;
            let observed:LibraryBatchReport=match serde_json::from_slice(&bytes) {
                Ok(report)=>report,
                Err(_)=>match serde_json::from_slice::<pulsar_protocol::ResponseBody>(&bytes)? {
                    pulsar_protocol::ResponseBody::LibraryBatch(report)=>report,
                    _=>anyhow::bail!("Expected an inspected local batch report"),
                },
            };
            let batch=RequestId::new(batch)?;
            anyhow::ensure!(observed.batch_request_id==batch,"Observed report belongs to another batch");
            observed.validate_migration()?;
            let recipe:LibraryBatchRecipe=serde_json::from_slice(&inference::read_bounded_profile(&recipe)?)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute_identified(RequestId::new(request_id)?,Command::LibraryBatchMigrate {batch_request_id:batch,expected_report_sha256:observed.report_sha256()?,recipe},None,None)?)
        }
        Commands::LibraryBatchPrepare { folder, recursive, audio_bundle_recipe, visual_recipe } => {
            let visual = visual_recipe.is_some();
            let path = visual_recipe.or(audio_bundle_recipe).context("Batch recipe required")?;
            let options=library_generation::retained_batch_options(&folder,recursive,&path)?;
            anyhow::ensure!(visual == matches!(options.recipe, LibraryBatchRecipe::Visual(_)), "Batch recipe kind differs from selected CLI option");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::LibraryBatchPrepare { options },None,None)?)
        }
        Commands::LibraryBatchRun { batch, publication_state, steps, poll_ms } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=library_publication::run(&mut client,RequestId::new(batch)?,&absolute_path(&publication_state)?,steps,poll_ms)?;
            print_json(&report)?;
            anyhow::ensure!(!report.needs_attention,"Retained workflow needs attention; no replacement work created");
            Ok(())
        }
        Commands::LibraryBatchWorkflowStatus { batch, publication_state } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_json(&library_publication::workflow_status(&mut client,RequestId::new(batch)?,&absolute_path(&publication_state)?)?)
        }
        Commands::LibraryBatchRecoverPreparation { batch, publication_state, index, staging, preparation_receipt } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_json(&library_publication::recover_preparation(&mut client,RequestId::new(batch)?,&absolute_path(&publication_state)?,index,&absolute_path(&staging)?,&absolute_path(&preparation_receipt)?)?)
        }
        Commands::LibraryBatchPublicationPrepare { batch, publication_state, steps } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=library_publication::prepare(&mut client,RequestId::new(batch)?,&absolute_path(&publication_state)?,steps)?;
            print_json(&report)?;
            anyhow::ensure!(!report.items.iter().any(|item|item.error.is_some()),"Retained publication preparation needs attention; no replacement intent created");
            Ok(())
        }
        Commands::LibraryBatchPublish { batch, publication_state, steps } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=library_publication::publish(&mut client,RequestId::new(batch)?,&absolute_path(&publication_state)?,steps)?;
            print_json(&report)?;
            anyhow::ensure!(!report.items.iter().any(|item|item.error.is_some()),"Retained publication needs attention; no replacement intent created");
            Ok(())
        }
        Commands::LibraryBatchRetirePending { publication_state, pending, sha256 } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_json(&library_publication::retire_pending(&mut client,&absolute_path(&publication_state)?,&absolute_path(&pending)?,&sha256)?)
        }
        Commands::LibraryBatchPublicationStatus { publication_state } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_json(&library_publication::status(&mut client,&absolute_path(&publication_state)?)?)
        }
        Commands::LibraryBatchCancel { batch, request_id } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute_identified(RequestId::new(request_id)?,Command::LibraryBatchCancel {batch_request_id:RequestId::new(batch)?},None,None)?)
        }
        Commands::LibraryBatchRelease { batch, request_id } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute_identified(RequestId::new(request_id)?,Command::LibraryBatchRelease { batch_request_id:RequestId::new(batch)? },None,None)?)
        }
        Commands::LibraryBatchList => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::LibraryBatchList,None,None)?)
        }
        Commands::LibraryBatchStatus { batch } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::LibraryBatchStatus { batch_request_id:RequestId::new(batch)? },None,None)?)
        }
        Commands::LibraryBatchAdvance { batch, steps } => {
            anyhow::ensure!((1..=1000).contains(&steps),"Choose 1..=1000 explicit phase steps");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let batch=RequestId::new(batch)?;
            let motion::ResponseBody::LibraryBatch(mut report)=client.execute(Command::LibraryBatchStatus {batch_request_id:batch.clone()},None,None)? else {bail!("Unexpected local batch status");};
            for _ in 0..steps {
                if report.generation_ready {break;}
                let request=report.pending_advance_request_id.clone().unwrap_or_else(session::new_request_id);
                let motion::ResponseBody::LibraryBatch(next)=client.execute_identified(request,Command::LibraryBatchAdvance {batch_request_id:batch.clone()},None,None)? else {bail!("Unexpected local batch advance");};
                report=next;
                if report.items.iter().any(|item|item.error.is_some()) {print_json(&report)?;bail!("Retained local batch needs attention; no replacement work created");}
                if report.items.iter().any(|item|item.phase==LibraryBatchPhase::Generating) {std::thread::sleep(std::time::Duration::from_millis(10));}
            }
            print_json(&report)
        }
        Commands::ExportBundleInspect { staging } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            print_json(&neutral_bundle::inspect_neutral_bundle(&mut client, &absolute_path(&staging)?)?)
        }
        Commands::ExportBundleResume { staging } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            print_json(&neutral_bundle::resume_neutral_bundle(&mut client, &absolute_path(&staging)?, &project_package::PackageDownloadControl::default())?)
        }
        Commands::AllowPackaging { project, acknowledge_unencrypted_private_data } => {
            anyhow::ensure!(acknowledge_unencrypted_private_data, project_package::PACKAGE_PRIVACY_WARNING);
            eprintln!("{}", project_package::PACKAGE_PRIVACY_WARNING);
            let consent = project_package::PackagePrivacyConsent::acknowledge_unencrypted_private_data();
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            project_package::allow_packaging_local(&mut client, ProjectId::new(project)?, bootstrap, &consent)?;
            println!("Packaging allowed for this authenticated owner session only.");
            Ok(())
        }
        Commands::MotionEvidenceExport { project, destination, revision, axis, acknowledge_unencrypted_private_data } => {
            anyhow::ensure!(acknowledge_unencrypted_private_data, project_package::PACKAGE_PRIVACY_WARNING);
            eprintln!("{}", project_package::PACKAGE_PRIVACY_WARNING);
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let receipt = motion_evidence_export::export_motion_evidence(&mut client,
                ProjectId::new(project)?, RevisionId::new(revision), modality::parse_axis(&axis)?,
                &absolute_path(&destination)?, &project_package::PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                &project_package::PackageDownloadControl::default())?;
            println!("{}",serde_json::to_string_pretty(&receipt)?); Ok(())
        }
        Commands::MotionEvidenceResume { staging, acknowledge_unencrypted_private_data } => {
            anyhow::ensure!(acknowledge_unencrypted_private_data, project_package::PACKAGE_PRIVACY_WARNING);
            eprintln!("{}", project_package::PACKAGE_PRIVACY_WARNING);
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let receipt = motion_evidence_export::resume_motion_evidence(&mut client,
                &absolute_path(&staging)?, &project_package::PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
                &project_package::PackageDownloadControl::default())?;
            println!("{}",serde_json::to_string_pretty(&receipt)?); Ok(())
        }
        Commands::MotionEvidenceCheck { script, package, project, revision, axis } => {
            let report = motion_evidence::verify_motion_evidence_files(
                &absolute_path(&script)?, &absolute_path(&package)?,
                &ProjectId::new(project)?, RevisionId::new(revision), modality::parse_axis(&axis)?,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Commands::PackageExport { project, destination, acknowledge_unencrypted_private_data, revision, request_id } => {
            anyhow::ensure!(acknowledge_unencrypted_private_data, project_package::PACKAGE_PRIVACY_WARNING);
            eprintln!("{}", project_package::PACKAGE_PRIVACY_WARNING);
            let consent = project_package::PackagePrivacyConsent::acknowledge_unencrypted_private_data();
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let project = ProjectId::new(project)?;
            let revision = match revision { Some(value) => RevisionId::new(value), None => client.project_package_revision(project.clone())? };
            let request_id = match request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            eprintln!("Package Start request: {}", request_id);
            std::io::Write::flush(&mut std::io::stderr())?;
            let mut status = client.start_project_export_with_request_id(project.clone(), revision, &consent, request_id)?;
            eprintln!("Package export operation: {}", status.operation_id);
            eprintln!("Disconnect does not cancel. Use package-status, package-cancel, or package-download with this operation ID.");
            std::io::Write::flush(&mut std::io::stderr())?;
            let operation = status.operation_id.clone();
            let start_request = status.request_id.clone();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30 * 60);
            while !status.state.is_terminal() {
                anyhow::ensure!(std::time::Instant::now() < deadline,
                    "Package wait expired; operation {} is retained and was not cancelled", operation);
                let remaining = deadline.checked_duration_since(std::time::Instant::now()).context("Package wait deadline expired")?;
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(100)));
                status = client.project_package_status_until(project.clone(), operation.clone(), deadline)?;
                anyhow::ensure!(status.request_id == start_request && status.requested_revision == revision,
                    "Package status changed its original request/revision identity");
            }
            anyhow::ensure!(status.state == PackageExportOperationState::Ready,
                "Package operation {} ended {:?}: {:?}", operation, status.state, status.error);
            let begin_id = session::new_request_id();
            eprintln!("Package download request: {}", begin_id);
            std::io::Write::flush(&mut std::io::stderr())?;
            let receipt = client.download_project_package_with_request_id(project, operation, &destination, &consent,
                &project_package::PackageDownloadControl::default(), begin_id)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            Ok(())
        }
        Commands::PackageStatus { project, operation } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.project_package_status(ProjectId::new(project)?, PackageOperationId::new(operation)?)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::PackageCancel { project, operation } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.cancel_project_export(ProjectId::new(project)?, PackageOperationId::new(operation)?)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::PackageDownload { project, operation, destination, acknowledge_unencrypted_private_data, request_id } => {
            anyhow::ensure!(acknowledge_unencrypted_private_data, project_package::PACKAGE_PRIVACY_WARNING);
            eprintln!("{}", project_package::PACKAGE_PRIVACY_WARNING);
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let consent = project_package::PackagePrivacyConsent::acknowledge_unencrypted_private_data();
            let request_id = match request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            eprintln!("Package download request: {}", request_id);
            std::io::Write::flush(&mut std::io::stderr())?;
            let receipt = client.download_project_package_with_request_id(ProjectId::new(project)?, PackageOperationId::new(operation)?,
                &destination, &consent, &project_package::PackageDownloadControl::default(), request_id)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            Ok(())
        }
        Commands::PackageRelease { project, operation, acknowledge_discarding_engine_copy } => {
            anyhow::ensure!(acknowledge_discarding_engine_copy, "Explicit engine-copy disposal acknowledgement is required");
            eprintln!("Deleting the retained engine package copy. This does not verify importability or modify a downloaded destination.");
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.release_project_export(ProjectId::new(project)?, PackageOperationId::new(operation)?)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::PackageImport { source, request_id, begin_request_id, seal_request_id } => {
            eprintln!("{}", project_package_import::PACKAGE_IMPORT_NOTICE);
            let start_id = match request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            let begin_id = match begin_request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            let seal_id = match seal_request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            eprintln!("Import Start request: {start_id}\nImport Begin request: {begin_id}\nImport Seal request: {seal_id}");
            std::io::Write::flush(&mut std::io::stderr())?;
            let control = project_package_import::PackageImportControl::default();
            let mut prepared = project_package_import::PreparedPackageImport::open(&source, &control)?;
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.start_project_import(&prepared, start_id)?;
            eprintln!("Project import operation: {}", status.operation_id);
            eprintln!("Disconnect does not cancel. Use package-import-status or package-import-cancel with this operation ID.");
            std::io::Write::flush(&mut std::io::stderr())?;
            let status = client.continue_project_import(&mut prepared, status.operation_id, begin_id, seal_id, &control)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            anyhow::ensure!(status.state == pulsar_protocol::PackageImportOperationState::Completed,
                "Import ended {:?}; no completed clone is claimed", status.state);
            Ok(())
        }
        Commands::PackageImportStatus { operation } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.project_import_status(pulsar_protocol::PackageImportOperationId::new(operation)?)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::PackageImportCancel { operation } => {
            eprintln!("Requesting engine cancellation. Already completed publication is not undone.");
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.cancel_project_import(pulsar_protocol::PackageImportOperationId::new(operation)?)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Commands::PackageImportResume { operation, source, begin_request_id, seal_request_id } => {
            eprintln!("{}", project_package_import::PACKAGE_IMPORT_NOTICE);
            let begin_id = match begin_request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            let seal_id = match seal_request_id { Some(id) => RequestId::new(id)?, None => session::new_request_id() };
            eprintln!("Import Begin request: {begin_id}\nImport Seal request: {seal_id}");
            std::io::Write::flush(&mut std::io::stderr())?;
            let control = project_package_import::PackageImportControl::default();
            let mut prepared = project_package_import::PreparedPackageImport::open(&source, &control)?;
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let status = client.continue_project_import(&mut prepared, pulsar_protocol::PackageImportOperationId::new(operation)?,
                begin_id, seal_id, &control)?;
            println!("{}", serde_json::to_string_pretty(&status)?);
            anyhow::ensure!(status.state == pulsar_protocol::PackageImportOperationState::Completed,
                "Import ended {:?}; no completed clone is claimed", status.state);
            Ok(())
        }
        Commands::RuntimeInstall { .. } | Commands::RuntimeUpdate { .. } | Commands::RuntimeUpdatePack { .. } | Commands::RuntimePack { .. }
                    | Commands::RuntimeLaunch { .. } | Commands::RuntimeCacheLaunch { .. } | Commands::RuntimeCacheInspect { .. } | Commands::RuntimeModels { .. } | Commands::BundledModels | Commands::RuntimeStageInspect { .. } | Commands::RuntimeArchiveStageInspect { .. } | Commands::RuntimeArchiveStageDiscard { .. } | Commands::RuntimeStageDiscard { .. } => bail!("runtime installation must run through the local executable host"),
        Commands::AssistantSetup { .. } | Commands::AssistantSelect { .. } | Commands::AssistantDisable { .. } => bail!("assistant setup must run through the local executable host"),
        Commands::Assistant {command} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            assistant_client::run(&mut client,endpoint,command)
        }
        Commands::WasmEdit { command } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            wasm_edit_client::run(&mut client,command)
        }
        Commands::NativePreparation {command} => {
            use cli::NativePreparationCommands as C;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let directory=bootstrap.parent().context("Missing private engine directory")?;
            match &command {
                C::Saved {project} => {
                    let project=ProjectId::new(project.clone())?;
                    let entries=native_preparation_recovery::load(directory,endpoint,client.session_id(),&client.recovery_credential_binding())?;
                    return print_json(&entries.into_iter().filter(|e|e.project==project).collect::<Vec<_>>());
                }
                C::Dismiss {project,original_request_id} => {
                    native_preparation_recovery::dismiss(directory,endpoint,client.session_id(),&client.recovery_credential_binding(),&ProjectId::new(project.clone())?,&RequestId::new(original_request_id.clone())?)?;
                    return print_json(&serde_json::json!({"dismissed":original_request_id,"engine_operation_changed":false}));
                }
                _ => {}
            }
            let (project,revision,operation,identity)=match command {
                C::Saved {..}|C::Dismiss {..}=>unreachable!("handled local recovery"),
                C::Limits {project,revision}=>(project,revision,Command::NativeImagePreparationLimits,None),
                C::Start {project,revision,engine_epoch,preparation_ms,retention_ms,request_id} => (project,revision,
                    Command::StartNativeImagePreparation {engine_epoch:AttemptId::new(engine_epoch)?,options:NativeImagePreparationOptions {preparation_ms,retention_ms}},Some(request_id)),
                C::Resolve {project,revision,original_request_id} => (project,revision,Command::ResolveNativeImagePreparation {original_request_id:RequestId::new(original_request_id)?},None),
                C::Status {project,revision,operation} => (project,revision,Command::NativeImagePreparationStatus {operation_id:AttemptId::new(operation)?},None),
                C::Cancel {project,revision,operation,request_id} => (project,revision,Command::CancelNativeImagePreparation {operation_id:AttemptId::new(operation)?},Some(request_id)),
                C::Release {project,revision,operation,request_id} => (project,revision,Command::ReleaseNativeImagePreparation {operation_id:AttemptId::new(operation)?},Some(request_id)),
            };
            let response=native_preparation_recovery::execute(&mut client,directory,endpoint,identity.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id),operation,ProjectId::new(project)?,RevisionId::new(revision))?;
            match &response {
                ResponseBody::NativeImagePreparation(value)=>value.validate()?,
                ResponseBody::NativeImagePreparationLimits(value)=>value.validate()?,
                _=>bail!("Unexpected native preparation response"),
            }
            print_response(response)
        }
        Commands::LiveCapture{command}=>{
            use cli::LiveCaptureCommands;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let (project,revision,operation,request_id)=match command {
                LiveCaptureCommands::Stream{project,revision,epoch}=>{
                    let report=live_stream::run(&mut client,ProjectId::new(project)?,RevisionId::new(revision),AttemptId::new(epoch)?)?;
                    return print_response(ResponseBody::LiveCapture((report).into()));
                },
                LiveCaptureCommands::Encoded{project,revision,epoch,source}=>{
                    use std::io::Read;
                    let ResponseBody::LiveCapture(report)=client.execute(Command::LiveCaptureStatus{epoch:AttemptId::new(epoch)?},Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))? else {bail!("Unexpected capture status response");};
                    let maximum=report.options.encoded_source.as_ref().ok_or_else(||anyhow::anyhow!("Capture did not admit encoded input"))?.maximum_input_bytes;
                    let mut options=std::fs::OpenOptions::new();options.read(true);
                    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.custom_flags((rustix::fs::OFlags::NOFOLLOW|rustix::fs::OFlags::NONBLOCK).bits() as i32);}
                    let mut file=options.open(source)?;let metadata=file.metadata()?;anyhow::ensure!(metadata.is_file()&&metadata.len()>0&&metadata.len()<=maximum,"Encoded source must be a nonempty bounded regular file");
                    let mut bulk=client.open_live_encoded(&report)?;let mut chunk=[0u8;65536];let mut total=0u64;
                    loop {let n=file.read(&mut chunk)?;if n==0{break;}total=total.checked_add(n as u64).ok_or_else(||anyhow::anyhow!("Encoded file size overflow"))?;anyhow::ensure!(total<=metadata.len(),"Encoded file changed during intake");bulk.push(&chunk[..n])?;}
                    anyhow::ensure!(total==metadata.len(),"Encoded file changed during intake");
                    return print_response(ResponseBody::LiveCapture((bulk.finish()?).into()));
                },
                LiveCaptureCommands::Clock{project,revision,epoch}=>{
                    let ResponseBody::LiveCapture(report)=client.execute(Command::LiveCaptureStatus{epoch:AttemptId::new(epoch)?},Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))? else {bail!("Unexpected capture status response");};
                    let bulk=client.open_live_bulk(&report)?;
                    return print_json(bulk.clock_observation()?.facts());
                },
                LiveCaptureCommands::Push{project,revision,epoch,header,pixels}=>{
                    let header:LiveFrameHeader=serde_json::from_slice(&inference::read_bounded_profile(&header)?)?;header.validate()?;
                    let pixels=inference::read_bounded_profile(&pixels)?;
                    anyhow::ensure!(pixels.len()==LIVE_FRAME_BYTES,"Expected exactly one packed 64x64 RGB frame");
                    use sha2::{Digest,Sha256};
                    anyhow::ensure!(format!("{:x}",Sha256::digest(&pixels))==header.sha256,"Frame pixels differ from declared hash");
                    let ResponseBody::LiveCapture(report)=client.execute(Command::LiveCaptureStatus{epoch:AttemptId::new(epoch)?},Some(ProjectId::new(project)?),Some(RevisionId::new(revision)))? else {bail!("Unexpected capture status response");};
                    let mut bulk=client.open_live_bulk(&report)?;
                    return print_response(ResponseBody::LiveCapture((bulk.upload(&header,&pixels)?).into()));
                },
                LiveCaptureCommands::RecordingStart{project,revision,epoch,maximum_samples,request_id}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::StartLiveRecording{epoch:AttemptId::new(epoch)?,maximum_samples},request_id),
                LiveCaptureCommands::RecordingStage{project,revision,options,request_id}=>{
                    let options:LiveRecordingStageOptions=serde_json::from_slice(&inference::read_bounded_profile(&options)?)?;options.validate()?;
                    (ProjectId::new(project)?,RevisionId::new(revision),Command::StageLiveRecording{options},request_id)
                },
                LiveCaptureCommands::RecordingPage{project,revision,epoch,after,limit}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::LiveRecordingPage{epoch:AttemptId::new(epoch)?,after,limit},None),
                LiveCaptureCommands::RecordingDiscard{project,revision,epoch,recording_request_id}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::DiscardLiveRecording{epoch:AttemptId::new(epoch)?,recording_request_id:RequestId::new(recording_request_id)?},None),
                LiveCaptureCommands::Limits{project,revision}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::LiveCaptureLimits,None),
                LiveCaptureCommands::ContinueAt{project,revision,predecessor,source_anchor,lease_ms,request_id,engine_epoch}=>{
                    let project=ProjectId::new(project)?;let revision=RevisionId::new(revision);let predecessor=AttemptId::new(predecessor)?;
                    let source_anchor:SourceTimestamp=serde_json::from_slice(&inference::read_bounded_profile(&source_anchor)?)?;
                    let engine_epoch=capture_engine_epoch(&mut client,&project,revision,engine_epoch)?;
                    (project,revision,Command::ContinueLiveCaptureAt{engine_epoch,predecessor,source_anchor,lease_ms},request_id)
                }
                LiveCaptureCommands::Continue{project,revision,predecessor,options,request_id,engine_epoch}=>{
                    let project=ProjectId::new(project)?;let revision=RevisionId::new(revision);
                    let options:LiveCaptureOptions=serde_json::from_slice(&inference::read_bounded_profile(&options)?)?;options.validate()?;
                    let engine_epoch=capture_engine_epoch(&mut client,&project,revision,engine_epoch)?;
                    (project,revision,Command::ContinueLiveCapture{engine_epoch,predecessor:AttemptId::new(predecessor)?,options},request_id)
                }
                LiveCaptureCommands::Start{project,revision,options,request_id,engine_epoch}=>{
                    let project=ProjectId::new(project)?;let revision=RevisionId::new(revision);
                    let options:LiveCaptureOptions=serde_json::from_slice(&inference::read_bounded_profile(&options)?)?;options.validate()?;
                    let engine_epoch=capture_engine_epoch(&mut client,&project,revision,engine_epoch)?;
                    (project,revision,Command::StartLiveCapture{engine_epoch,options},request_id)
                },
                LiveCaptureCommands::DownloadEvidence{project,revision,epoch,part,output}=>{
                    let part=serde_json::from_value(serde_json::Value::String(part))?;
                    let receipt=live_evidence::download(&client,ProjectId::new(project)?,RevisionId::new(revision),AttemptId::new(epoch)?,part,&output)?;
                    println!("{}",serde_json::to_string_pretty(&receipt)?);
                    return Ok(());
                },
                LiveCaptureCommands::RemoveEvidence{project,revision,epoch,manifest_sha256,request_id,confirm_removal}=>{
                    anyhow::ensure!(confirm_removal,"Explicit retained artifact removal confirmation required");validate_sha256(&manifest_sha256)?;
                    (ProjectId::new(project)?,RevisionId::new(revision),Command::RemoveRetainedLiveCudaEvidence{epoch:AttemptId::new(epoch)?,expected_manifest_sha256:manifest_sha256},Some(request_id))
                },
                LiveCaptureCommands::ListEvidence{project,revision,after,limit}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::ListLiveCudaEvidence{after:after.map(AttemptId::new).transpose()?,limit},None),
                LiveCaptureCommands::Evidence{project,revision,epoch}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::InspectLiveCudaEvidence{epoch:AttemptId::new(epoch)?},None),
                LiveCaptureCommands::Status{project,revision,epoch}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::LiveCaptureStatus{epoch:AttemptId::new(epoch)?},None),
                LiveCaptureCommands::CloseInput { project, revision, epoch, request_id, final_frame, no_frames } => {
                    anyhow::ensure!(no_frames != final_frame.is_some(), "Choose the exact final header or explicitly declare no frames");
                    let final_frame = final_frame.map(|path| -> Result<LiveFrameHeader> { let value: LiveFrameHeader = serde_json::from_slice(&inference::read_bounded_profile(&path)?)?; value.validate()?; Ok(value) }).transpose()?;
                    (ProjectId::new(project)?, RevisionId::new(revision), Command::CloseBrowserAnalysisInput { epoch: AttemptId::new(epoch)?, final_frame }, Some(request_id))
                },
                LiveCaptureCommands::Finish{project,revision,epoch,request_id}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::FinishLiveCapture{epoch:AttemptId::new(epoch)?},request_id),
                LiveCaptureCommands::Stop{project,revision,epoch,request_id}=>(ProjectId::new(project)?,RevisionId::new(revision),Command::StopLiveCapture{epoch:AttemptId::new(epoch)?},request_id),
            };
            let response=client.execute_identified(request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id),operation,Some(project),Some(revision))?;print_response(response)
        }
        Commands::BrowserLive { project, revision, options, port, duration_seconds, media_lease_ms, handy_player_guard, allow_handy_player_monitoring, handy_browser_play, allow_handy_browser_control, handy_hsp, allow_handy_hsp_control } => {
            anyhow::ensure!(handy_player_guard.is_some() == allow_handy_player_monitoring,
                "Handy player monitoring requires both a selection file and explicit consent");
            anyhow::ensure!(handy_browser_play.is_some() == allow_handy_browser_control,"Prepared browser playback controls require a selection file and explicit enablement");
            anyhow::ensure!(handy_hsp.is_some() == allow_handy_hsp_control,"Handy HSP controls require a configuration file and explicit enablement");
            validate_browser_handy_modes(handy_player_guard.is_some(), handy_browser_play.is_some(), handy_hsp.is_some())?;
            let hsp = handy_hsp.as_deref().map(read_browser_hsp_config).transpose()?;
            let guard: Option<browser_player_control::GuardConfig> = handy_player_guard.as_deref()
                .map(|path| -> Result<_> { Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?) })
                .transpose()?;
            let options: LiveCaptureOptions = serde_json::from_slice(&inference::read_bounded_profile(&options)?)?;
            let playback:Option<browser_handy_playback::Config>=handy_browser_play.as_deref().map(|path|->Result<_>{
                anyhow::ensure!(path.is_absolute(),"Prepared browser playback configuration path must be absolute");
                let config:browser_handy_playback::Config=serde_json::from_slice(&inference::read_bounded_profile(path)?)?;config.validate()?;Ok(config)
            }).transpose()?;
            browser_live::run(SessionClient::connect(endpoint, bootstrap)?, ProjectId::new(project)?, RevisionId::new(revision), options, browser_live::LaunchOptions { port, duration_seconds, media_lease_ms, guard, playback, hsp })
        }
        Commands::Capabilities => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            print_response(client.execute(Command::Capabilities, None, None)?)
        }
        Commands::Project { command } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            match command {
                ProjectCommands::Create{name}=>print_response(client.execute(Command::CreateProject{name},None,None)?),
                ProjectCommands::Open{project_id}=>print_response(client.execute(Command::OpenProject{project_id:ProjectId::new(project_id)?},None,None)?),
                ProjectCommands::Place{project_id,spec,replace}=>{
                    let spec:SourcePlacementSpec=serde_json::from_slice(&inference::read_bounded_profile(&spec)?)?;spec.validate()?;
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;
                    print_response(client.execute(Command::SetSourcePlacement{spec,replace:replace.map(SourcePlacementId::new).transpose()?},Some(p.project_id),Some(p.revision))?)
                },
                ProjectCommands::CandidateImageOriginPixels {project_id,candidate_id,record_sha256}=>{validate_sha256(&record_sha256)?;print_response(client.execute(Command::CandidateImageOriginPixels {candidate_id:CandidateId::new(candidate_id)?,record_sha256},Some(ProjectId::new(project_id)?),None)?)},
                ProjectCommands::CandidateImageOriginStatus {project_id,candidate_id}=>print_response(client.execute(Command::CandidateImageOriginStatus {candidate_id:CandidateId::new(candidate_id)?},Some(ProjectId::new(project_id)?),None)?),
                ProjectCommands::ArchivedGenerationStatus {project_id,candidate_id,master_revision}=>print_response(client.execute(Command::ArchivedGenerationStatus {candidate_id:CandidateId::new(candidate_id)?,master_revision:master_revision.map(RevisionId::new)},Some(ProjectId::new(project_id)?),None)?),
                ProjectCommands::ArchivedRetimeStatus {project_id,revision}=>print_response(client.execute(Command::ArchivedMotionRetimeStatus {revision:RevisionId::new(revision)},Some(ProjectId::new(project_id)?),None)?),
                ProjectCommands::RetimeStatus {project_id,revision}=>print_response(client.execute(Command::MotionRetimeStatus {revision:RevisionId::new(revision)},Some(ProjectId::new(project_id)?),None)?),
                ProjectCommands::Retime {project_id,from_placement,to_placement,axes,revision,request_id}=>{
                    let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
                    eprintln!("Motion retiming request {id}");
                    print_response(client.execute_identified(id,Command::RetimeMotion {from_placement:SourcePlacementId::new(from_placement)?,to_placement:SourcePlacementId::new(to_placement)?,axes:axes.iter().map(|a|modality::parse_axis(a)).collect::<Result<Vec<_>>>()?},Some(ProjectId::new(project_id)?),Some(RevisionId::new(revision)))?)
                },
                ProjectCommands::RemovePlacement{project_id,placement_id}=>{
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;
                    print_response(client.execute(Command::RemoveSourcePlacement{placement_id:SourcePlacementId::new(placement_id)?},Some(p.project_id),Some(p.revision))?)
                },
                ProjectCommands::PreviewPlacement{project_id,placement_id,time_ns,video_view}=>{
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;let placement_id=SourcePlacementId::new(placement_id)?;
                    let placement=p.placements.iter().find(|p|p.placement_id==placement_id).context("source placement is not active")?;
                    let view=video_view.as_deref().map(|path|->Result<VideoView>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
                    let context=FrameContext{source_version:placement.spec.source_version.clone(),source_placement:placement_id.clone(),frame:FrameId::new(0),transform:video_view_transform(view.as_ref())?,seek_generation:0,request_generation:0};
                    let expected=PlacedPreviewRequest{placement:placement.clone(),project_time:ProjectTime::from_nanos(time_ns)};
                    let result=client.execute(Command::PreviewPlacedFrame{placement_id,project_time:expected.project_time,context:context.clone(),video_view:view,model_path:None,model_input:None,inference_profile:None},Some(p.project_id),None)?;
                    if let ResponseBody::Preview(preview)=&result{anyhow::ensure!(preview.matches_placed_seek_request(&context,&expected),"placed preview receipt mismatch");}
                    print_response(result)
                },
                ProjectCommands::PreviewAnnotated {project_id,time_ns,annotated_view,source_sample_stride,source_version,placement_id,model,inference_profile,model_pack}=>{
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;
                    let spec=AnnotatedPreviewSpec {view:inference::read_annotated_view(&annotated_view)?,source_sample_stride};
                    let (model,profile)=if let Some(digest)=model_pack {
                        let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
                        let ResponseBody::ModelPackSelection(selected)=client.execute(Command::ModelPackSelectionStatus{owner_proof:proof},None,None)? else {bail!("unexpected pack selection response")};
                        selected.validate()?;let choice=selected.choice.context("No offline pack selected")?;
                        anyhow::ensure!(choice.manifest_sha256==digest,"Requested pack differs from current owner selection");
                        (AnnotatedPreviewModel::Pack {choice},selected.profile.context("Selected profile is absent")?)
                    }else{
                        let profile=inference::read_profile(inference_profile.as_deref().context("annotated profile required")?)?;
                        (AnnotatedPreviewModel::File {model_path:absolute_path(&model.context("annotated model required")?)?,profile:(profile.clone()).into()},profile)
                    };
                    spec.validate_profile(&profile)?;
                    let (source,placement,time)=if let Some(id)=placement_id {
                        let id=SourcePlacementId::new(id)?;let placement=p.placements.iter().find(|p|p.placement_id==id).context("source placement is not active")?;
                        (placement.spec.source_version.clone(),id.clone(),ModelPackPreviewTime::Placement{placement_id:id,project_time:ProjectTime::from_nanos(time_ns)})
                    }else{
                        let source=SourceVersionId::new(source_version.context("Source version or placement required")?)?;
                        let placement=SourcePlacementId::new(format!("primary:{source}"))?;
                        (source,placement,ModelPackPreviewTime::Source{source_time:SourceTimestamp::new(time_ns,1_000_000_000)?,time_basis:PreviewTimeBasis::ElapsedFromFirstFrame})
                    };
                    let context=FrameContext{source_version:source,source_placement:placement,frame:FrameId::new(0),transform:annotated_preview_transform(&spec.view)?,seek_generation:0,request_generation:0};
                    let result=client.execute(Command::PreviewAnnotatedFrame{context:context.clone(),time:time.clone(),spec:spec.clone(),model:model.clone()},Some(p.project_id),None)?;
                    let preview=match (&model,&result) {
                        (AnnotatedPreviewModel::File {..},ResponseBody::Preview(preview))=>preview,
                        (AnnotatedPreviewModel::Pack {choice},ResponseBody::ModelPackPreview(report))=>{report.validate()?;anyhow::ensure!(&report.choice==choice,"Annotated preview pack choice differs");&report.preview},
                        _=>bail!("Unexpected annotated preview response"),
                    };
                    let evidence=preview.annotated.as_ref().context("Annotated preview evidence absent")?;evidence.validate(preview)?;
                    anyhow::ensure!(serde_json::to_value(&evidence.spec)?==serde_json::to_value(&spec)? && evidence.model==profile.model && evidence.runtime==profile.runtime,"Annotated preview model/view differs");
                    use sha2::{Digest,Sha256};
                    anyhow::ensure!(evidence.profile.sha256==format!("{:x}",Sha256::digest(serde_json::to_vec(&profile)?)),"Annotated preview profile differs");
                    let matches=match time {
                        ModelPackPreviewTime::Source{source_time,time_basis}=>preview.matches_timed_seek_request(&context,&source_time,time_basis),
                        ModelPackPreviewTime::Placement{placement_id,project_time}=>{let placement=p.placements.iter().find(|p|p.placement_id==placement_id).context("Placement absent")?;preview.matches_placed_seek_request(&context,&PlacedPreviewRequest{placement:placement.clone(),project_time})}
                    };
                    anyhow::ensure!(matches,"Annotated preview differs from requested time/context");
                    print_response(result)
                },
                ProjectCommands::PreviewPack{project_id,time_ns,model_pack,source_version,placement_id,video_view}=>{
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;
                    let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
                    let ResponseBody::ModelPackSelection(selected)=client.execute(Command::ModelPackSelectionStatus{owner_proof:proof},None,None)? else {bail!("unexpected pack selection response")};
                    selected.validate()?;let choice=selected.choice.context("No offline pack selected")?;
                    anyhow::ensure!(choice.manifest_sha256==model_pack,"Requested pack differs from current owner selection");
                    let view=video_view.as_deref().map(|path|->Result<VideoView>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
                    let (source,placement,time)=if let Some(id)=placement_id {
                        let id=SourcePlacementId::new(id)?;let placement=p.placements.iter().find(|p|p.placement_id==id).context("source placement is not active")?;
                        (placement.spec.source_version.clone(),id.clone(),ModelPackPreviewTime::Placement{placement_id:id,project_time:ProjectTime::from_nanos(time_ns)})
                    }else{
                        let source=SourceVersionId::new(source_version.context("Source version or placement required")?)?;
                        let placement=SourcePlacementId::new(format!("primary:{source}"))?;
                        (source,placement,ModelPackPreviewTime::Source{source_time:SourceTimestamp::new(time_ns,1_000_000_000)?,time_basis:PreviewTimeBasis::ElapsedFromFirstFrame})
                    };
                    let context=FrameContext{source_version:source,source_placement:placement,frame:FrameId::new(0),transform:video_view_transform(view.as_ref())?,seek_generation:0,request_generation:0};
                    let result=client.execute(Command::PreviewModelPackFrame{context:context.clone(),time:time.clone(),video_view:view,choice:choice.clone()},Some(p.project_id),None)?;
                    let ResponseBody::ModelPackPreview(report)=&result else {bail!("Unexpected pack preview response")};
                    report.validate()?;
                    let profile=selected.profile.context("Selected profile is absent")?;
                    use sha2::{Digest,Sha256};
                    anyhow::ensure!(report.choice==choice&&report.model==profile.model&&report.profile_sha256==format!("{:x}",Sha256::digest(serde_json::to_vec(&profile)?)),"Pack preview differs from selected model/profile");
                    let matches=match time {
                        ModelPackPreviewTime::Source{source_time,time_basis}=>report.preview.matches_timed_seek_request(&context,&source_time,time_basis),
                        ModelPackPreviewTime::Placement{placement_id,project_time}=>{
                            let placement=p.placements.iter().find(|p|p.placement_id==placement_id).context("Placement is absent")?;
                            report.preview.matches_placed_seek_request(&context,&PlacedPreviewRequest{placement:placement.clone(),project_time})
                        }
                    };
                    anyhow::ensure!(matches,"Pack preview differs from requested frame/view/time");
                    print_response(result)
                },
                ProjectCommands::GeneratePlacement{project_id,placement_id,preset,video_view}=>{
                    let p=get_snapshot(&mut client,&ProjectId::new(project_id)?)?;
                    let id=SourcePlacementId::new(placement_id)?;
                    let placement=p.placements.iter().find(|p|p.placement_id==id).context("source placement is not active")?.clone();
                    let view=video_view.as_deref().map(|path|->Result<VideoView>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
                    let settings=GenerationSettings{source_placement:Some(placement.clone()),video_view:view,..Default::default()};
                    print_response(client.execute(Command::Generate{source_version:placement.spec.source_version,preset,settings:Some(settings),model_path:None},Some(p.project_id),Some(p.revision))?)
                },
            }

        }
        Commands::Generate { video, mixed_audio, audio_source, audio_alignment, target_seed, annotated_view, raw_frame_pair, output, model, model_pack, model_class_count, inference_profile, fallback_selection, conf, fps, source_sample_stride, width, height, detrend_window,
            norm_window, batch_size, vr, video_view, pov, no_balance_global, no_keyframe_reduction,
            cut_diff, cut_flow, multi_axis, profile, cuts } => {
            // No legacy option is silently ignored. Additional modes remain
            // in the grammar and migration ledger until an engine adapter owns them.
            if fps != 30.0
                || detrend_window != 2.0 || norm_window != 3.0 || batch_size != 2000 || vr || pov
                || no_balance_global || no_keyframe_reduction || cut_diff != 30.0 || cut_flow != 8.0
                || multi_axis || cuts.is_some() {
                return unsupported("non-default legacy generation settings; use the supported default/fast stroke path until typed worker adapters are available");
            }
            if !matches!(profile.as_str(), "default" | "fast" | "maximum" | "quality") {
                return unsupported("requested generation profile; choose default, fast, maximum or quality");
            }
            if conf.is_some() {
                anyhow::ensure!(model.is_some() || model_pack.is_some(), "--conf requires an explicit detector model or selected pack");
            }
            let fallback_selection = fallback_selection.as_deref().map(read_fallback_selection).transpose()?;
            if fallback_selection.is_some() {
                anyhow::ensure!(raw_frame_pair.is_none(), "Fallback selection requires decoded media, not raw frame pairs");
                anyhow::ensure!(model_pack.is_some() || (model.is_some() && inference_profile.is_some()), "Fallback selection requires --model with --inference-profile, or --model-pack");
            }
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let pack_selection = if let Some(hash)=model_pack {
                let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
                let ResponseBody::ModelPackSelection(report)=client.execute(Command::ModelPackSelectionStatus {owner_proof:proof},None,None)? else {bail!("unexpected pack selection response")};
                report.validate()?;
                anyhow::ensure!(report.choice.as_ref().is_some_and(|c|c.manifest_sha256==hash),"Requested model pack is not the current owner selection");
                Some(report)
            } else {None};
            let inference_profile = if let Some(report)=&pack_selection {report.profile.clone()} else {inference_profile.as_deref().map(inference::read_profile).transpose()?};
            if target_seed.is_some() && model.is_none() && pack_selection.is_none() {bail!("Target seed requires an explicit model or selected pack");}
            if raw_frame_pair.is_some() && inference_profile.is_none() {bail!("Raw frame pair requires a declared profile or selected pack");}
            let model_input = if let Some(profile) = &inference_profile {
                Some(profile.preprocessing.input.clone())
            } else if model.is_some() {
                let class_count = model_class_count.context("Custom models require --model-class-count; tensor layouts are never guessed")?;
                Some(ModelInputContract { width: width.unwrap_or(640), height: height.unwrap_or(640),
                    channels: 3, layout: TensorLayout::Nchw, class_count, decoder: DetectorDecoder::YoloChannelMajor })
            } else {
                if model_class_count.is_some() { bail!("--model-class-count requires --model"); }
                None
            };
            let video_view=video_view.as_deref().map(|path|->anyhow::Result<VideoView>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            let mixed_audio=mixed_audio.as_deref().map(|path|->anyhow::Result<MixedAudioSettings>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            if mixed_audio.as_ref().is_some_and(|audio| audio.independent.is_some()) { bail!("CLI mixed settings cannot name an existing project source; use --audio-source and --audio-alignment"); }
            let audio_alignment=audio_alignment.as_deref().map(|path|->anyhow::Result<AudioAlignment>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            let independent_audio=match (audio_source, audio_alignment) {
                (Some(path),Some(alignment)) => Some((path,alignment)),
                (None,None) => None,
                _ => bail!("Independent audio requires an explicit source and alignment"),
            };
            let raw_frame_pair=raw_frame_pair.as_deref().map(|path|->anyhow::Result<RawFramePair>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            let target_seed=target_seed.as_deref().map(|path|->anyhow::Result<AnalysisTargetSeed>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            let annotated_view=annotated_view.as_deref().map(inference::read_annotated_view).transpose()?;
            let (analysis_width,analysis_height)=inference::generation_analysis_dimensions(model_input.as_ref(),width,height);
            let settings = GenerationSettings { source_sample_stride, detector_score_threshold: conf, annotated_view, target_seed, raw_frame_pair, mixed_audio, video_view, inference_profile, analysis_width, analysis_height,
                model_input, ..Default::default() };
            settings.validate()?;
            let model_path = model.map(|path| absolute_path(&path)).transpose()?;
            let output = output.unwrap_or_else(|| video.with_extension("funscript"));
            generate_to_file_with_pack(&mut client, FileGenerationInput { video: &video, output: &output, preset: &profile, settings, model_path, independent_audio, choice: pack_selection.and_then(|s|s.choice), fallback_selection })
        }
        Commands::Fix { script, output, max_speed, jitter_window, jitter_threshold } => {
            let options=ScriptRepairOptions::new(max_speed,jitter_window,jitter_threshold)?;
            let output=script_repair::output_path(&script,output.as_deref())?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            script_repair::fix(&mut client,&script,&output,options)
        },
        Commands::Doctor { script, max_speed } => {
            validate_segment_speed_limit(max_speed)?;
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::doctor(&mut client, &script, max_speed)
        },
        Commands::Info { media } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            modality::info(&mut client,&media)
        },
        Commands::Scan { folder, recursive, multi_axis, missing_only, json, csv, generate, generation, overwrite } => {
            if overwrite { return unsupported("library overwrite requires an explicit preservation contract"); }
            if !generate && !library_generation::is_default(&generation) {bail!("Library generation settings require --generate");}
            let mode=library_generation::folder_mode(&generation,multi_axis,endpoint,bootstrap)?;
            if json && csv { bail!("Choose --json or --csv, not both"); }
            let entries = library_entries(&folder, recursive, mode.is_audio_bundle())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries.iter().filter(|entry| !missing_only || !entry.has_script).collect::<Vec<_>>())?);
            } else if csv {
                println!("video,script,has_script");
                for entry in entries.iter().filter(|entry| !missing_only || !entry.has_script) {
                    println!("\"{}\",\"{}\",{}", entry.video.display().to_string().replace('"', "\"\""),
                        entry.script.display().to_string().replace('"', "\"\""), entry.has_script);
                }
            } else {
                for entry in entries.iter().filter(|entry| !missing_only || !entry.has_script) {
                    println!("{} {}", if entry.has_script { "PRESENT" } else { "MISSING" }, entry.video.display());
                }
                println!("{} media files; {} missing {}", entries.len(), entries.iter().filter(|entry| !entry.has_script).count(), if mode.is_audio_bundle() {"audio bundle targets (existing contents unverified)"} else {"stroke companions"});
            }
            if generate {
                mode.ensure_execution_platform()?;
                let mut client = SessionClient::connect(endpoint, bootstrap)?;
                for entry in entries.into_iter().filter(|entry| !entry.has_script) {
                    mode.generate_file(&mut client,&entry.video,&entry.script,&generation.preset)?;
                }
            }
            Ok(())
        }
        Commands::Batch { folder, recursive, generation, multi_axis, overwrite, dry_run } => {
            if overwrite { return unsupported("batch overwrite requires an explicit preservation contract"); }
            let mode=library_generation::folder_mode(&generation,multi_axis,endpoint,bootstrap)?;
            let entries = library_entries(&folder, recursive, mode.is_audio_bundle())?;
            let pending: Vec<_> = entries.into_iter().filter(|entry| !entry.has_script).collect();
            if dry_run { return print_json(&pending); }
            mode.ensure_execution_platform()?;
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            for entry in pending { mode.generate_file(&mut client,&entry.video,&entry.script,&generation.preset)?; }
            Ok(())
        }
        Commands::AudioSynth { video, output, mode, band, axis, threshold, bundle, bundle_recipe, atomic_bundle } => {
            if bundle {
                anyhow::ensure!(mode == "beats" && band == "bass" && axis == "stroke" && threshold == 0.35,
                    "Audio bundle uses per-axis recipes; single-axis controls must retain defaults");
                let recipe_path = bundle_recipe.context("Audio bundle requires --bundle-recipe with explicit six-axis settings")?;
                let recipe: AudioBundleRecipe = serde_json::from_slice(&inference::read_bounded_profile(&recipe_path)?)?;
                let output = output.unwrap_or_else(|| video.with_extension("audio-bundle"));
                let mut client = SessionClient::connect(endpoint, bootstrap)?;
                return if atomic_bundle { modality::synthesize_atomic_audio_bundle(&mut client, &video, recipe, &output) }
                    else { modality::synthesize_audio_bundle(&mut client, &video, recipe, &output) };
            }
            let band = match band.as_str() {
                "full" => AudioBand::Full, "bass" => AudioBand::Bass,
                "mid" => AudioBand::Mid, "high" => AudioBand::High,
                _ => anyhow::bail!("audio band must be full, bass, mid or high"),
            };
            let options = AudioAnalysisOptions::new(band, threshold)?;
            let analysis = (band != AudioBand::Full || threshold != AUDIO_ONSET_THRESHOLD).then_some(options);
            let mode = match mode.as_str() {
                "envelope" => AudioMode::Envelope, "beats" => AudioMode::Beats,
                "vibe" | "osc" | "oscillation" => AudioMode::Vibe,
                _ => return unsupported("audio-synth mode; envelope, beats and vibe are supported"),
            };
            let mapping = AudioMapping::new(modality::parse_axis(&axis)?, 0.5, NormalizedPosition::new(0.25)?)?;
            let output = output.unwrap_or_else(|| video.with_extension("funscript"));
            eprintln!("{band:?} RMS mapping: offset 0.25, amplitude 0.5; onset threshold {threshold}, envelope gated={}. Onsets are not musical beats.", analysis.is_some());
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            modality::synthesize(&mut client, modality::Input::Audio { path: video, mapping, mode, analysis }, &output)
        }
        Commands::Model { command: ModelCommands::Equivalence { command } } => inference_equivalence::run(command, endpoint, bootstrap),
        Commands::Model {command:ModelCommands::PackWithdraw {manifest_sha256,selection_revision,expected_selection_revision,include_unclassified_generation,request_id}} => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?;eprintln!("request_id={}",id.as_str());
            let directory=bootstrap.parent().context("missing private engine directory")?;
            print_response(pack_recovery::execute(directory,&mut client,id,Command::WithdrawModelPackApproval {choice:pulsar_protocol::ModelPackChoice {manifest_sha256,selection_revision},expected_selection_revision,include_unclassified_generation,owner_proof:proof})?)
        }
        Commands::Model {command:ModelCommands::PackSelectionStatus} => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::ModelPackSelectionStatus {owner_proof:proof},None,None)?)
        }
        Commands::Model {command:ModelCommands::PackSelect {manifest_sha256,expected_selection_revision,approve_unqualified_native_execution,request_id}} => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?;eprintln!("request_id={}",id.as_str());
            print_response(client.execute_identified(id,Command::SelectModelPack {manifest_sha256:Some(manifest_sha256),expected_selection_revision,approve_unqualified_native_execution,owner_proof:proof},None,None)?)
        }
        Commands::Model {command:ModelCommands::PackDeactivate {expected_selection_revision,request_id}} => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?;eprintln!("request_id={}",id.as_str());
            print_response(client.execute_identified(id,Command::SelectModelPack {manifest_sha256:None,expected_selection_revision,approve_unqualified_native_execution:false,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackImport {source,request_id} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?;
            eprintln!("request_id={}",id.as_str());
            print_response(client.execute_identified(id,Command::ImportModelPack {source,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackDiscardStage {basename,manifest_sha256,request_id} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=RequestId::new(request_id.unwrap_or_else(||uuid::Uuid::new_v4().to_string()))?;
            eprintln!("request_id={}",id.as_str());
            print_response(client.execute_identified(id,Command::DiscardModelPackStage {basename,manifest_sha256,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::CatalogRefresh {source,bootstrap:root,bootstrap_sha256,expected_revision,approve_bootstrap,request_id} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            print_response(client.execute_identified(id,Command::RefreshModelCatalog {source:absolute_path(&source)?,bootstrap:absolute_path(&root)?,bootstrap_sha256,expected_catalog_revision:expected_revision,approve_bootstrap,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackCatalogSelect {bootstrap_sha256,manifest_sha256,expected_catalog_revision,expected_selection_revision,approve_unqualified_native_execution,request_id} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            print_response(client.execute_identified(id,Command::SelectCatalogModelPack {manifest_sha256,binding:pulsar_protocol::ModelCatalogBinding {bootstrap_sha256,catalog_revision:expected_catalog_revision},expected_selection_revision,approve_unqualified_native_execution,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackCatalogCheck {bootstrap_sha256,manifest_sha256,expected_revision} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::CheckModelPackCatalog {bootstrap_sha256,manifest_sha256,expected_catalog_revision:expected_revision,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::CatalogStatus {bootstrap_sha256} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::ModelCatalogStatus {bootstrap_sha256,owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackStagingStatus } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::ModelPackStagingStatus {owner_proof:proof},None,None)?)
        }
        Commands::Model { command:ModelCommands::PackStatus {manifest_sha256} } => {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::ModelPackStatus {manifest_sha256,owner_proof:proof},None,None)?)
        }
        Commands::Model { command } => {
            if matches!(&command, ModelCommands::Download { .. }) {
                return unsupported("model downloads; no redistribution-qualified bundle is installed");
            }
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let ResponseBody::InferenceCatalog(catalog) = client.execute(Command::InferenceCatalog, None, None)? else {
                bail!("engine did not return its inference catalog");
            };
            match command {
                ModelCommands::Profile { model, temporal_script_position, annotated_temporal_position, letterbox, padding_byte, width, height, class_count } => {
                    let mut profile=inference::custom_profile(&catalog, &model, width, height, class_count)?;
                    if letterbox {
                        profile.preprocessing.resize=InferenceResize::Letterbox;
                        profile.preprocessing.padding_byte=padding_byte.unwrap_or(114);
                        profile.validate()?;
                    }
                    if temporal_script_position || annotated_temporal_position {
                        profile.preprocessing.input.channels=6;
                        profile.preprocessing.input.decoder=DetectorDecoder::TemporalScriptPosition;
                        profile.preprocessing.color=InferenceColor::RgbFramePair;
                        profile.preprocessing.resize=if annotated_temporal_position {InferenceResize::AnnotatedTemporalView}else{InferenceResize::NearestStretch};
                        profile.preprocessing.scale=vec![1.0/255.0;6];profile.preprocessing.bias=vec![0.0;6];profile.validate()?;
                    }
                    print_json(&profile)
                }
                _ => print_json(&catalog),
            }
        }
        Commands::HandyRegister {device,label,api_key_env,connection_key_env,expected_registration_revision,disabled,request_id} => {
            let change=handy_registration::RegistrationChange::Save {device_id:DeviceId::new(device)?,expected_revision:expected_registration_revision,settings:HandyRegistrationSettings {label,api_key_env,connection_key_env},disabled};
            change.validate()?;
            let id=RequestId::new(request_id)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            eprintln!("Handy registration request {id}; retain this ID and configuration for exact recovery.");
            let receipt=handy_registration::change(&mut client,bootstrap.parent().context("missing private engine directory")?,id,&change)?;
            print_response(ResponseBody::HandyRegistrationSaved(Box::new(receipt)))
        }
        Commands::HandyDisable {device,expected_registration_revision,request_id} => {
            let change=handy_registration::RegistrationChange::Disable {device_id:DeviceId::new(device)?,expected_revision:expected_registration_revision};
            change.validate()?;
            let id=RequestId::new(request_id)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            eprintln!("Handy registration request {id}; retain this ID and revision for exact recovery.");
            let receipt=handy_registration::change(&mut client,bootstrap.parent().context("missing private engine directory")?,id,&change)?;
            print_response(ResponseBody::HandyRegistrationSaved(Box::new(receipt)))
        }
        Commands::HandyResolveRegistration {original_request_id,intent_json,request_id} => {
            anyhow::ensure!(intent_json.len()<=8192,"Captured registration intent exceeds 8 KiB");
            let intent:HandyRegistrationIntent=serde_json::from_str(&intent_json)?; intent.validate()?;
            let original=RequestId::new(original_request_id)?; let resolution=RequestId::new(request_id)?;
            anyhow::ensure!(original!=resolution,"Resolution requires a distinct request ID");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            eprintln!("Handy resolution {resolution} for original {original}; retain both identities for recovery.");
            let result=handy_registration::resolve(&mut client,bootstrap.parent().context("missing private engine directory")?,resolution,original,&intent)?;
            print_response(ResponseBody::HandyRegistrationResolution(Box::new(result)))
        }
        Commands::HandyPrepareStroke {project,path,revision,request_id} => {
            let project=ProjectId::new(project)?;let revision=RevisionId::new(revision);let id=RequestId::new(request_id)?;let path=absolute_path(&path)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_script::prepare(&mut client,project,revision,path,id)?;
            print_response(ResponseBody::HandyStrokeScript(Box::new(receipt)))
        }
        Commands::HandyHostStroke {project,revision,preparation_request_id,script_sha256,engine_epoch,request_id,allow_temporary_public_hosting} => {
            let intent=handy_hosting::HostingIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),preparation:RequestId::new(preparation_request_id)?,script_sha256,engine_epoch:DeviceSessionId::new(engine_epoch)?};
            let id=RequestId::new(request_id)?;
            anyhow::ensure!(allow_temporary_public_hosting,"Explicit temporary PUBLIC hosting consent is required");
            validate_sha256(&intent.script_sha256)?;
            eprintln!("Hosting request {id}: only the prepared CSV will be disclosed to Handy's temporary public hosting service. An unknown outcome must not trigger an automatic new upload.");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_hosting::host(&mut client,bootstrap.parent().context("missing private Engine directory")?,id,&intent,allow_temporary_public_hosting)?;
            print_response(ResponseBody::HandyHosting(Box::new(report)))
        }
        Commands::HandyHostingStatus {project,upload_request_id} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_hosting::status(&mut client,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(upload_request_id)?)?;
            print_response(ResponseBody::HandyHosting(Box::new(report)))
        }
        Commands::HandyForgetHosting {project,upload_request_id,request_id} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_hosting::forget(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RequestId::new(upload_request_id)?)?;
            print_response(ResponseBody::HandyHosting(Box::new(report)))
        }
        Commands::HandySetup {project,revision,engine_epoch,device_session,hosting_request_id,qualification_id,qualification_revision,request_id,allow_online_setup} => {
            let intent=handy_setup::SetupInspectionIntent {qualification:Some(HandyQualificationBinding {dossier_id:ArtifactId::new(qualification_id)?,revision:qualification_revision}),project:ProjectId::new(project)?,revision:RevisionId::new(revision),engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,hosting_request_id:RequestId::new(hosting_request_id)?};
            anyhow::ensure!(allow_online_setup,"Handy setup requires explicit online setup consent");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::setup(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_online_setup)?;
            print_response(ResponseBody::HandySetupStatus(Box::new(result)))
        }
        Commands::HandyCancelSetup {project,setup_request_id,record_id,request_id} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::cancel(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RequestId::new(setup_request_id)?,ArtifactId::new(record_id)?)?;
            print_response(ResponseBody::HandySetupCancellation(Box::new(result)))
        }
        Commands::HandySetupStatus {project,setup_request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(setup_request_id)?)?;
            print_response(ResponseBody::HandySetupStatus(Box::new(result)))
        }
        Commands::HandyPlay {project,revision,acquisition_request_id,controller_id,generation,clock_request_id,start_time_ms,request_id,allow_play} => {
            anyhow::ensure!(allow_play,"Handy playback requires explicit effect consent");
            let id=RequestId::new(request_id)?;
            let intent=handy_playback::PlaybackIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,generation,clock_request_id:RequestId::new(clock_request_id)?,start_time_ms};
            intent.validate(&id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_playback::start(&mut api,bootstrap.parent().context("missing private Engine directory")?,id,&intent,allow_play)?;
            print_response(ResponseBody::HandyPlayback(Box::new(value)))
        }
        Commands::HandyRefreshTiming {project,revision,acquisition_request_id,controller_id,generation,play_request_id,play_record_id,request_id,allow_online_inspection} => {
            anyhow::ensure!(allow_online_inspection,"continuous timing refresh requires explicit online inspection consent");
            let id=RequestId::new(request_id)?;
            let intent=handy_continuous_timing::RefreshIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),selection:HandyContinuousTimingSelection {acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,expected_generation:generation,play_request_id:RequestId::new(play_request_id)?,play_record_id:ArtifactId::new(play_record_id)?}};
            intent.validate(&id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_continuous_timing::refresh(&mut api,bootstrap.parent().context("missing private Engine directory")?,id,&intent,allow_online_inspection)?;
            print_response(ResponseBody::HandyContinuousTiming(Box::new(value)))
        }
        Commands::HandyRefreshTimingStatus {project,refresh_request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(refresh_request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_continuous_timing::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,original)?;
            print_response(ResponseBody::HandyContinuousTiming(Box::new(value)))
        }
        Commands::HandyPlayerGuardStatus {project,binding_request_id} => {
            let project=ProjectId::new(project)?;
            let original=RequestId::new(binding_request_id)?;
            let owner_proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private Engine directory")?,"handy-owner.token")?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let ResponseBody::HandyPlayerGuard(report)=api.execute(Command::HandyPlayerGuardStatus {binding_request_id:original.clone(),owner_proof},Some(project.clone()),None)? else {bail!("player guard status response differs");};
            report.validate()?;
            anyhow::ensure!(report.binding.request_id==original && report.binding.controller.project_id==project,"player guard status changed the original binding or project");
            print_response(ResponseBody::HandyPlayerGuard(report))
        }
        Commands::HandyReconcilePlan {project,device_id,registration_revision,engine_epoch,device_session,qualification_id,qualification_revision} => {
            let project=ProjectId::new(project)?;
            let selection=HandyRecoverySelection {device_id:DeviceId::new(device_id)?,registration_revision,engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,qualification:HandyQualificationBinding {dossier_id:ArtifactId::new(qualification_id)?,revision:qualification_revision}};
            selection.validate()?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_reconciliation::plan(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,&selection)?;
            print_response(ResponseBody::HandyRecoveryPlan(Box::new(value)))
        }
        Commands::HandyReconcile {project,plan_id,request_id,allow_online_inspection,allow_resolution} => {
            anyhow::ensure!(allow_online_inspection && allow_resolution,"Handy reconciliation requires explicit online inspection and resolution consent");
            let project=ProjectId::new(project)?;let plan_id=ArtifactId::new(plan_id)?;validate_sha256(plan_id.as_str())?;let id=RequestId::new(request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_reconciliation::start(&mut api,bootstrap.parent().context("missing private Engine directory")?,id,project,plan_id,allow_online_inspection,allow_resolution)?;
            print_response(ResponseBody::HandyRecovery(Box::new(value)))
        }
        Commands::HandyReconcileStatus {project,reconciliation_request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(reconciliation_request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_reconciliation::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,original)?;
            print_response(ResponseBody::HandyRecovery(Box::new(value)))
        }
        Commands::HandyReconcileCancel {project,reconciliation_request_id,request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(reconciliation_request_id)?;let id=RequestId::new(request_id)?;
            anyhow::ensure!(id!=original,"reconciliation cancellation requires its own request identity");
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_reconciliation::cancel(&mut api,bootstrap.parent().context("missing private Engine directory")?,id,project,original)?;
            print_response(ResponseBody::HandyRecovery(Box::new(value)))
        }
        Commands::HandyHspLiveBind { project, revision, preparation_request_id, capture_epoch, inference_attempt_id, browser_timing_binding_id, source_qualification_id, source_qualification_revision, finite_drain_policy_sha256, request_id, acknowledge_current_environment, allow_online_control } => {
            let intent = handy_hsp_live::LiveIntent { project: ProjectId::new(project)?, revision: RevisionId::new(revision), selection: HandyHspLiveSelection {
                finite_drain_policy_sha256,
                preparation_request_id: RequestId::new(preparation_request_id)?, capture_epoch: AttemptId::new(capture_epoch)?, inference_attempt_id: AttemptId::new(inference_attempt_id)?, browser_timing: ArtifactId::new(browser_timing_binding_id)?, source_qualification: HandyHspSourceQualificationBinding { dossier_id: ArtifactId::new(source_qualification_id)?, revision: source_qualification_revision },
            }};
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let value = handy_hsp_live::bind(&mut client, bootstrap.parent().context("missing private Engine directory")?, RequestId::new(request_id)?, &intent, acknowledge_current_environment, allow_online_control)?;
            print_response(ResponseBody::HandyHspLive(Box::new(value)))
        }
        Commands::HandyHspLivePlay { project, revision, stream_request_id, request_id } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let value = handy_hsp_live::play(&mut client, bootstrap.parent().context("missing private Engine directory")?, RequestId::new(request_id)?, ProjectId::new(project)?, RevisionId::new(revision), RequestId::new(stream_request_id)?)?;
            print_response(ResponseBody::HandyHspLive(Box::new(value)))
        }
        Commands::HandyHspLiveStop { project, stream_request_id, request_id } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let value = handy_hsp_live::stop(&mut client, bootstrap.parent().context("missing private Engine directory")?, RequestId::new(request_id)?, ProjectId::new(project)?, RequestId::new(stream_request_id)?)?;
            print_response(ResponseBody::HandyHspLive(Box::new(value)))
        }
        Commands::HandyHspLiveStatus { project, stream_request_id } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let value = handy_hsp_live::status(&mut client, bootstrap.parent().context("missing private Engine directory")?, ProjectId::new(project)?, RequestId::new(stream_request_id)?)?;
            print_response(ResponseBody::HandyHspLive(Box::new(value)))
        }
        Commands::HandyHspContinuityEnable { project, revision, stream_request_id, duration_ms, request_id, allow_online_observations } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let intent = handy_hsp_live::ContinuityIntent { project: ProjectId::new(project)?, revision: RevisionId::new(revision), stream_request_id: RequestId::new(stream_request_id)?, duration_ms };
            let value = handy_hsp_live::enable_continuity(&mut client, bootstrap.parent().context("missing private Engine directory")?, RequestId::new(request_id)?, &intent, allow_online_observations)?;
            print_response(ResponseBody::HandyHspContinuity(Box::new(value)))
        }
        Commands::HandyHspContinuityStatus { project, original_request_id } => {
            let mut client = SessionClient::connect(endpoint, bootstrap)?;
            let value = handy_hsp_live::continuity_status(&mut client, bootstrap.parent().context("missing private Engine directory")?, ProjectId::new(project)?, RequestId::new(original_request_id)?)?;
            print_response(ResponseBody::HandyHspContinuity(Box::new(value)))
        }
        Commands::HandyHspPrepare {project,revision,engine_epoch,device_session,qualification_id,qualification_revision,timeout_ms,request_id,allow_online_synchronization} => {
            let intent=handy_hsp_preparation::PreparationIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,qualification:HandyQualificationBinding {dossier_id:ArtifactId::new(qualification_id)?,revision:qualification_revision},timeout_ms};
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_hsp_preparation::prepare(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_online_synchronization)?;
            print_response(ResponseBody::HandyHspPreparation(Box::new(value)))
        }
        Commands::HandyHspPreparationStatus {project,preparation_request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_hsp_preparation::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(preparation_request_id)?)?;
            print_response(ResponseBody::HandyHspPreparation(Box::new(value)))
        }
        Commands::HandyHspPreparationCancel {project,preparation_request_id,request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_hsp_preparation::cancel(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RequestId::new(preparation_request_id)?)?;
            print_response(ResponseBody::HandyHspPreparation(Box::new(value)))
        }
        Commands::HandyHspStatus {project,stream_request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(stream_request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_hsp::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,original)?;
            print_response(ResponseBody::HandyHsp(Box::new(value)))
        }
        Commands::HandyStopStatus {project,acquisition_request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(acquisition_request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_stop::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,original)?;
            print_response(ResponseBody::HandyStop(Box::new(value)))
        }
        Commands::HandyPlayStatus {project,play_request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(play_request_id)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_playback::status(&mut api,bootstrap.parent().context("missing private Engine directory")?,project,original)?;
            print_response(ResponseBody::HandyPlayback(Box::new(value)))
        }
        Commands::HandyPlayCancel {project,play_request_id,record_id,request_id} => {
            let project=ProjectId::new(project)?;let original=RequestId::new(play_request_id)?;let id=RequestId::new(request_id)?;let record=ArtifactId::new(record_id)?;
            handy_playback::validate_cancellation(&id,&original,&record)?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_playback::cancel(&mut api,bootstrap.parent().context("missing private Engine directory")?,id,project,original,record)?;
            print_response(ResponseBody::HandyPlayback(Box::new(value)))
        }
        Commands::HandyPlaybackClock {project,revision,acquisition_request_id,controller_id,generation,sync_request_id,sync_record_id,request_id,allow_online_timing} => {
            anyhow::ensure!(allow_online_timing,"playback clock acquisition requires explicit online timing consent");
            let intent=handy_setup::DeviceTimingIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,generation};
            let sync=HandyClockSyncBinding {sync_request_id:RequestId::new(sync_request_id)?,record_id:ArtifactId::new(sync_record_id)?};sync.validate()?;
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::acquire_playback_clock(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,&sync,allow_online_timing)?;
            print_response(ResponseBody::HandyPlaybackClock(Box::new(value)))
        }
        Commands::HandyClockSync {project,revision,acquisition_request_id,controller_id,generation,request_id,allow_online_synchronization} => {
            anyhow::ensure!(allow_online_synchronization,"clock synchronization requires explicit effect consent");
            let intent=handy_setup::DeviceTimingIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,generation};
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::synchronize_clock(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_online_synchronization)?;
            print_response(ResponseBody::HandyClockSync(Box::new(value)))
        }
        Commands::HandyClockSyncStatus {project,sync_request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::clock_sync_status(&mut api,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(sync_request_id)?)?;
            print_response(ResponseBody::HandyClockSync(Box::new(value)))
        }
        Commands::HandyClockSyncCancel {project,sync_request_id,record_id,request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::cancel_clock_sync(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RequestId::new(sync_request_id)?,ArtifactId::new(record_id)?)?;
            print_response(ResponseBody::HandyClockSync(Box::new(value)))
        }
        Commands::HandyInspectDeviceTiming {project,revision,acquisition_request_id,controller_id,generation,request_id,allow_online_inspection} => {
            anyhow::ensure!(allow_online_inspection,"device timing inspection requires explicit online consent");
            let intent=handy_setup::DeviceTimingIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,generation};
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::inspect_device_timing(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_online_inspection)?;
            print_response(ResponseBody::HandyDeviceTiming(Box::new(value)))
        }
        Commands::HandySampleServerClock {project,revision,acquisition_request_id,controller_id,generation,request_id,allow_online_sampling} => {
            anyhow::ensure!(allow_online_sampling,"server-clock acquisition needs explicit online sampling consent");
            let intent=handy_setup::ServerClockIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,generation};
            let mut api=SessionClient::connect(endpoint,bootstrap)?;
            let value=handy_setup::sample_server_clock(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_online_sampling)?;
            print_response(ResponseBody::HandyServerClock(Box::new(value)))
        }
        Commands::HandyControllerAcquire {project,revision,preparation_request_id,preparation_id,lease_ms,request_id,allow_arm,allow_online_inspection} => {
            let intent=handy_setup::ControllerAcquireIntent {project:ProjectId::new(project)?,revision:RevisionId::new(revision),preparation_request_id:RequestId::new(preparation_request_id)?,preparation_id:ArtifactId::new(preparation_id)?,duration_ms:lease_ms};
            anyhow::ensure!(allow_arm&&allow_online_inspection,"controller acquisition needs explicit local arming and online inspection consent");
            let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::controller_acquire(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,allow_arm,allow_online_inspection)?;print_response(ResponseBody::HandyController(Box::new(value)))
        }
        Commands::HandyControllerRenew {project,revision,acquisition_request_id,controller_id,generation,request_id} => {
            let intent=handy_setup::ControllerChangeIntent {project:ProjectId::new(project)?,acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,renewal:Some((RevisionId::new(revision),generation))};let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::controller_change(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent)?;print_response(ResponseBody::HandyController(Box::new(value)))
        }
        Commands::HandyControllerRelease {project,acquisition_request_id,controller_id,request_id} => {
            let intent=handy_setup::ControllerChangeIntent {project:ProjectId::new(project)?,acquisition_request_id:RequestId::new(acquisition_request_id)?,controller_id:ClientId::new(controller_id)?,renewal:None};let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::controller_change(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent)?;print_response(ResponseBody::HandyController(Box::new(value)))
        }
        Commands::HandyControllerStatus {project,acquisition_request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::controller_status(&mut api,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(acquisition_request_id)?)?;print_response(ResponseBody::HandyController(Box::new(value)))
        }
        Commands::HandyPreparePlayback {project,setup_request_id,record_id,qualification_id,qualification_revision,revision,engine_epoch,device_session,hosting_request_id,request_id,allow_online_inspection} => {
            let qualification=HandyQualificationBinding {dossier_id:ArtifactId::new(qualification_id)?, revision:qualification_revision};qualification.validate()?;
            let intent=handy_setup::SetupInspectionIntent {qualification:Some(qualification),project:ProjectId::new(project)?,revision:RevisionId::new(revision),engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,hosting_request_id:RequestId::new(hosting_request_id)?};
            let selection=handy_setup::ProgramInspectionIntent {setup:intent, setup_request_id:RequestId::new(setup_request_id)?, record_id:ArtifactId::new(record_id)?};
            let id=RequestId::new(request_id)?;
            anyhow::ensure!(allow_online_inspection,"Handy program inspection requires explicit online consent");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::prepare_playback(&mut client,bootstrap.parent().context("missing private Engine directory")?,id,&selection,allow_online_inspection)?;
            print_response(ResponseBody::HandyPlaybackPreparation(Box::new(result)))
        }
        Commands::HandyPlaybackStatus {project,preparation_request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::playback_status(&mut api,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,RequestId::new(preparation_request_id)?)?;print_response(ResponseBody::HandyPlaybackPreparation(Box::new(value)))
        }
        Commands::HandyRetirePlayback {project,preparation_request_id,preparation_id,request_id} => {
            let mut api=SessionClient::connect(endpoint,bootstrap)?;let value=handy_setup::retire_playback(&mut api,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RequestId::new(preparation_request_id)?,ArtifactId::new(preparation_id)?)?;print_response(ResponseBody::HandyPlaybackRetirement(Box::new(value)))
        }
        Commands::HandyInspectProgram {project,setup_request_id,record_id,qualification_id,qualification_revision,revision,engine_epoch,device_session,hosting_request_id,request_id,allow_online_inspection} => {
            let qualification=HandyQualificationBinding {dossier_id:ArtifactId::new(qualification_id)?, revision:qualification_revision};qualification.validate()?;
            let intent=handy_setup::SetupInspectionIntent {qualification:Some(qualification),project:ProjectId::new(project)?,revision:RevisionId::new(revision),engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,hosting_request_id:RequestId::new(hosting_request_id)?};
            let selection=handy_setup::ProgramInspectionIntent {setup:intent, setup_request_id:RequestId::new(setup_request_id)?, record_id:ArtifactId::new(record_id)?};
            let id=RequestId::new(request_id)?;
            anyhow::ensure!(allow_online_inspection,"Handy program inspection requires explicit online consent");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::inspect_program(&mut client,bootstrap.parent().context("missing private Engine directory")?,id,&selection,allow_online_inspection)?;
            print_response(ResponseBody::HandyProgramCorrelation(Box::new(result)))
        }
        Commands::HandyInspectSetup {project,qualification_id,qualification_revision,revision,engine_epoch,device_session,hosting_request_id,request_id,allow_online_inspection} => {
            let qualification=match (qualification_id,qualification_revision) {(Some(id),Some(revision))=>Some(HandyQualificationBinding {dossier_id:ArtifactId::new(id)?,revision}),(None,None)=>None,_=>anyhow::bail!("qualification ID and revision must be supplied together")};
            if let Some(binding)=&qualification {binding.validate()?;}
            let intent=handy_setup::SetupInspectionIntent {qualification,project:ProjectId::new(project)?,revision:RevisionId::new(revision),engine_epoch:DeviceSessionId::new(engine_epoch)?,device_session:DeviceSessionId::new(device_session)?,hosting_request_id:RequestId::new(hosting_request_id)?};
            let id=RequestId::new(request_id)?;
            anyhow::ensure!(allow_online_inspection,"Handy setup inspection requires explicit online consent");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let result=handy_setup::inspect(&mut client,bootstrap.parent().context("missing private Engine directory")?,id,&intent,allow_online_inspection)?;
            print_response(ResponseBody::HandySetupInspection(Box::new(result)))
        }
        Commands::HandyQualificationContext { transport } => {
            let transport=match transport.as_str() {"hssp"=>pulsar_protocol::HandyQualificationTransport::Hssp,"hsp"=>pulsar_protocol::HandyQualificationTransport::Hsp,_=>anyhow::bail!("unsupported Handy qualification transport")};
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let value=handy_qualification::context_for_transport(&mut client,bootstrap.parent().context("missing private Engine directory")?,transport)?;print_response(ResponseBody::HandyQualificationContext(Box::new(value)))
        }
        Commands::HandyHspSourceQualificationImport { project, manifest, revision, manifest_sha256, request_id } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_hsp_source_qualification::import(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RevisionId::new(revision),absolute_path(&manifest)?,manifest_sha256)?;
            print_response(ResponseBody::HandyHspSourceQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyHspSourceQualificationStatus { project, dossier } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_hsp_source_qualification::status(&mut client,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,ArtifactId::new(dossier)?)?;
            print_response(ResponseBody::HandyHspSourceQualification(Box::new(report)))
        }
        Commands::HandyHspSourceQualificationAccept { project, dossier, revision, record_revision, operator, request_id, acknowledge_measured_evidence } => {
            let intent=handy_hsp_source_qualification::Review {project:ProjectId::new(project)?,project_revision:Some(RevisionId::new(revision)),dossier:ArtifactId::new(dossier)?,record_revision,operator:Some(operator),reason:String::new()};
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_hsp_source_qualification::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,acknowledge_measured_evidence)?;
            print_response(ResponseBody::HandyHspSourceQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyHspSourceQualificationWithdraw { project, dossier, record_revision, reason, request_id } => {
            let intent=handy_hsp_source_qualification::Review {project:ProjectId::new(project)?,project_revision:None,dossier:ArtifactId::new(dossier)?,record_revision,operator:None,reason};
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_hsp_source_qualification::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,false)?;
            print_response(ResponseBody::HandyHspSourceQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyBrowserStartQualificationImport { project, manifest, revision, manifest_sha256, request_id } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_browser_start::import(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,ProjectId::new(project)?,RevisionId::new(revision),absolute_path(&manifest)?,manifest_sha256)?;
            print_response(ResponseBody::HandyBrowserStartQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyBrowserStartQualificationStatus { project, dossier } => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_browser_start::status(&mut client,bootstrap.parent().context("missing private Engine directory")?,ProjectId::new(project)?,ArtifactId::new(dossier)?)?;
            print_response(ResponseBody::HandyBrowserStartQualification(Box::new(report)))
        }
        Commands::HandyBrowserStartQualificationAccept { project, dossier, revision, record_revision, operator, request_id, acknowledge_measured_evidence } => {
            let intent=handy_browser_start::Review {project:ProjectId::new(project)?,project_revision:Some(RevisionId::new(revision)),dossier:ArtifactId::new(dossier)?,record_revision,operator:Some(operator),reason:String::new()};
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_browser_start::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,acknowledge_measured_evidence)?;
            print_response(ResponseBody::HandyBrowserStartQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyBrowserStartQualificationWithdraw { project, dossier, record_revision, reason, request_id } => {
            let intent=handy_browser_start::Review {project:ProjectId::new(project)?,project_revision:None,dossier:ArtifactId::new(dossier)?,record_revision,operator:None,reason};
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let receipt=handy_browser_start::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,false)?;
            print_response(ResponseBody::HandyBrowserStartQualificationSaved(Box::new(receipt)))
        }
        Commands::HandyQualificationImport {manifest,manifest_sha256,request_id} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let value=handy_qualification::import(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,absolute_path(&manifest)?,manifest_sha256)?;print_response(ResponseBody::HandyQualificationSaved(Box::new(value)))
        }
        Commands::HandyQualificationStatus {dossier} => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let value=handy_qualification::status(&mut client,bootstrap.parent().context("missing private Engine directory")?,ArtifactId::new(dossier)?)?;print_response(ResponseBody::HandyQualification(Box::new(value)))
        }
        Commands::HandyQualificationAccept {dossier,revision,engine_epoch,device_session,note,request_id,acknowledge_physical_evidence,allow_online_inspection} => {
            anyhow::ensure!(acknowledge_physical_evidence&&allow_online_inspection,"Explicit physical evidence acceptance and online inspection consent required");
            let intent=handy_qualification::Review {dossier:ArtifactId::new(dossier)?,revision,note,acceptance:Some((DeviceSessionId::new(engine_epoch)?,DeviceSessionId::new(device_session)?))};let mut client=SessionClient::connect(endpoint,bootstrap)?;let value=handy_qualification::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,acknowledge_physical_evidence,allow_online_inspection)?;print_response(ResponseBody::HandyQualificationSaved(Box::new(value)))
        }
        Commands::HandyQualificationWithdraw {dossier,revision,note,request_id} => {
            let intent=handy_qualification::Review {dossier:ArtifactId::new(dossier)?,revision,note,acceptance:None};let mut client=SessionClient::connect(endpoint,bootstrap)?;let value=handy_qualification::review(&mut client,bootstrap.parent().context("missing private Engine directory")?,RequestId::new(request_id)?,&intent,false,false)?;print_response(ResponseBody::HandyQualificationSaved(Box::new(value)))
        }
        Commands::HandySessions => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let catalog=handy_session::list(&mut client,bootstrap.parent().context("missing private engine directory")?)?;
            print_response(ResponseBody::HandySessions(Box::new(catalog)))
        }
        Commands::HandyOpenSession {device,engine_epoch,expected_registration_revision,binding_sha256,request_id,allow_online_inspection} => {
            let device=DeviceId::new(device)?;let epoch=DeviceSessionId::new(engine_epoch)?;let id=RequestId::new(request_id)?;
            validate_sha256(&binding_sha256)?;
            anyhow::ensure!(allow_online_inspection,"Opening requires explicit online inspection consent");
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let directory=bootstrap.parent().context("missing private engine directory")?;
            let binding=handy_session::OpenConnection {epoch,device,revision:expected_registration_revision,binding_sha256};
            let report=handy_session::open(&mut client,directory,id,&binding,allow_online_inspection)?;
            print_response(ResponseBody::HandySession(Box::new(report)))
        }
        Commands::HandyCloseSession {engine_epoch,device_session,request_id} => {
            let epoch=DeviceSessionId::new(engine_epoch)?;let device_session=DeviceSessionId::new(device_session)?;let id=RequestId::new(request_id)?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_session::close(&mut client,bootstrap.parent().context("missing private engine directory")?,id,epoch,device_session)?;
            print_response(ResponseBody::HandySession(Box::new(report)))
        }
        Commands::HandyRegistrations => {
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let catalog=handy_registration::list(&mut client,bootstrap.parent().context("missing private engine directory")?)?;
            print_response(ResponseBody::HandyRegistrations(Box::new(catalog)))
        }
        Commands::HandyInspectRegistered {device,expected_registration_revision,binding_sha256,allow_online_inspection} => {
            let device=DeviceId::new(device)?;
            anyhow::ensure!(allow_online_inspection,"Handy online inspection requires explicit consent");
            anyhow::ensure!(expected_registration_revision.is_some()==binding_sha256.is_some(),"Registration revision and binding pins must be supplied together");
            if let Some(hash)=&binding_sha256 { validate_sha256(hash)?; }
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let directory=bootstrap.parent().context("missing private engine directory")?;
            let catalog=handy_registration::list(&mut client,directory)?;
            let registration=catalog.registrations.into_iter().find(|r|r.device_id==device).context("Handy registration not found")?;
            if let (Some(revision),Some(hash))=(expected_registration_revision,binding_sha256) {
                anyhow::ensure!(registration.revision==revision && registration.binding_sha256==hash,"Handy registration changed; inspect its current catalog entry");
            }
            let report=handy_registration::inspect(&mut client,directory,&registration,allow_online_inspection)?;
            print_response(ResponseBody::HandyRegisteredInspection(Box::new(report)))
        }
        Commands::HandyInspect {api_key_env,connection_key_env,allow_online_inspection} => {
            let inspection=HandyInspectionRequest {api_key_env,connection_key_env,allow_online_inspection};
            inspection.validate()?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let report=handy_panel::inspect(&mut client, bootstrap.parent().context("missing private engine directory")?, inspection)?;
            print_response(ResponseBody::HandyInspection(report))
        }
        Commands::Bench => unsupported("hardware benchmark campaign (B/C qualification adapter)"),
        Commands::Play { .. } => unsupported("physical playback; no qualified driver, stopping bound, or admission is established by this build"),
        Commands::Stash { command:cli::StashCommands::ListMissing {limit,url,api_key,api_key_env} } => {
            anyhow::ensure!(api_key.is_none(),"Plaintext --api-key is no longer accepted. Use --api-key-env NAME and launch the engine via secrets run.");
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashListMissing {
                connection:StashConnection {endpoint:url,credential_env:api_key_env},limit,owner_proof:proof
            },None,None)?)
        }
        Commands::Stash {command:cli::StashCommands::GenerateMissing {source_plans,annotated_views,mappings,limit,url,api_key_env,model,model_pack,model_class_count,inference_profile,video_view,detector_score_threshold,width,height,preset,scan,tag,create_tag,request_id,detach}} => {
            if detector_score_threshold.is_some() {
                anyhow::ensure!(model.is_some() || model_pack.is_some(), "--detector-score-threshold requires an explicit detector model or selected pack");
            }
            let bytes=inference::read_bounded_profile(&mappings).context("read bounded Stash mappings")?;
            let mappings:Vec<StashRootMapping>=serde_json::from_slice(&bytes)?;
            let inference_profile=inference_profile.as_deref().map(inference::read_profile).transpose()?;
            let model_input=if let Some(profile)=&inference_profile {anyhow::ensure!(model_class_count.is_none_or(|count|count==profile.preprocessing.input.class_count),"--model-class-count conflicts with declared Stash inference profile");Some(profile.preprocessing.input.clone())}
                else if model.is_some() {Some(ModelInputContract {width:width.unwrap_or(640),height:height.unwrap_or(640),channels:3,layout:TensorLayout::Nchw,class_count:model_class_count.context("Custom Stash models require --model-class-count")?,decoder:DetectorDecoder::YoloChannelMajor})}
                else {anyhow::ensure!(model_class_count.is_none(),"--model-class-count requires --model");None};
            let video_view=video_view.as_deref().map(|path|->anyhow::Result<VideoView>{Ok(serde_json::from_slice(&inference::read_bounded_profile(path)?)?)}).transpose()?;
            let (analysis_width,analysis_height)=inference::generation_analysis_dimensions(model_input.as_ref(),width,height);
            let mut settings=GenerationSettings {detector_score_threshold,video_view,inference_profile,model_input,analysis_width,analysis_height,..Default::default()};
            let source_plans=source_plans.as_deref().map(inference::read_stash_source_plans).transpose()?.unwrap_or_default();
            let annotated_views=annotated_views.as_deref().map(inference::read_stash_annotated_views).transpose()?.unwrap_or_default();
            if model_pack.is_none() {validate_stash_batch_source_plans(&settings,&annotated_views,&source_plans)?;}
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let model_pack=if let Some(digest)=model_pack {
                let owner=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"model-pack-owner.token")?;
                let ResponseBody::ModelPackSelection(selected)=client.execute(Command::ModelPackSelectionStatus{owner_proof:owner},None,None)? else {bail!("Unexpected pack selection response")};
                selected.validate()?;let choice=selected.choice.context("No offline pack selected")?;
                anyhow::ensure!(choice.manifest_sha256==digest,"Requested pack differs from current owner selection");
                let profile=selected.profile.context("Selected profile is absent")?;settings.model_input=Some(profile.preprocessing.input.clone());settings.inference_profile=Some(profile);
                (settings.analysis_width,settings.analysis_height)=inference::generation_analysis_dimensions(settings.model_input.as_ref(),width,height);
                validate_stash_batch_source_plans(&settings,&annotated_views,&source_plans)?;Some(choice)
            }else{None};
            anyhow::ensure!(!create_tag || tag.is_some(),"--create-tag requires --tag");
            let scan=scan || tag.is_some();let tag=tag.map(|name|StashBatchTag {name,create:create_tag});
            let config=stash_flow::BatchConfig {scan,tag,request_id:request_id.map(RequestId::new).transpose()?,detach,connection:StashConnection {endpoint:url,credential_env:api_key_env},mappings,limit,owner_proof:proof,preset,settings,annotated_views,source_plans,model_path:model.map(|path|absolute_path(&path)).transpose()?,model_pack};
            let report=stash_flow::generate_missing(&mut client,&config)?;
            print_json(&report)?;
            anyhow::ensure!(!report.has_failures(),"Stash generation has failed or unknown outcomes; retained project/job/request IDs must be reconciled before retrying");
            Ok(())
        }
        Commands::Stash {command:cli::StashCommands::BatchStatus {batch_request_id,offset}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashBatchStatus {batch_request_id:RequestId::new(batch_request_id)?,offset,owner_proof:proof},None,None)?)
        }
        Commands::Stash {command:cli::StashCommands::ResumeBatch {batch_request_id,request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);eprintln!("Stash batch resume request {id}");
            print_response(client.execute_with_id(id,Command::StashResumeBatch {batch_request_id:RequestId::new(batch_request_id)?,owner_proof:proof},None,None)?)
        }
        Commands::Stash {command:cli::StashCommands::MigrateBatch {batch_request_id,expected_ledger_sha256,generation,request_id}}=> {
            let generation:StashBatchGeneration=serde_json::from_slice(&inference::read_bounded_profile(&generation)?)?;generation.validate()?;
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);eprintln!("Stash batch migration request {id}; resume separately after inspection");
            print_response(client.execute_with_id(id,Command::StashMigrateBatch {batch_request_id:RequestId::new(batch_request_id)?,expected_ledger_sha256,generation,owner_proof:proof},None,None)?)
        }
        Commands::Stash {command:cli::StashCommands::CancelBatch {batch_request_id,request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);eprintln!("Stash batch cancellation request {id}");
            print_response(client.execute_with_id(id,Command::StashCancelBatch {batch_request_id:RequestId::new(batch_request_id)?,owner_proof:proof},None,None)?)
        }
        Commands::Stash {command:cli::StashCommands::AddTag {project,scan_request_id,tag,create_tag,url,api_key_env,revision,request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let project=ProjectId::new(project)?;
            let revision=match revision {Some(value)=>RevisionId::new(value),None=>get_snapshot(&mut client,&project)?.revision};
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            eprintln!("Stash tag request {id}; retain this ID for reconciliation.");
            let response=client.execute_with_id(id,Command::StashAddTag {connection:StashConnection {endpoint:url,credential_env:api_key_env},scan_request_id:RequestId::new(scan_request_id)?,tag_name:tag,create:create_tag,owner_proof:proof},Some(project),Some(revision))?;
            let incomplete=matches!(&response,ResponseBody::StashTag(receipt) if receipt.state!=StashTagState::VerifiedAdded);print_response(response)?;
            if incomplete {bail!("Stash tag effect remains incomplete or unknown; reconcile before retrying");}Ok(())
        }
        Commands::Stash {command:cli::StashCommands::TagStatus {project,tag_request_id,url,api_key_env}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashTagStatus {connection:StashConnection {endpoint:url,credential_env:api_key_env},tag_request_id:RequestId::new(tag_request_id)?,owner_proof:proof},Some(ProjectId::new(project)?),None)?)
        }
        Commands::Stash {command:cli::StashCommands::ResumeTag {project,tag_request_id,url,api_key_env,revision,request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let project=ProjectId::new(project)?;
            let revision=match revision {Some(value)=>RevisionId::new(value),None=>get_snapshot(&mut client,&project)?.revision};
            let id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);eprintln!("Stash tag resume request {id}");
            let response=client.execute_with_id(id,Command::StashResumeTag {connection:StashConnection {endpoint:url,credential_env:api_key_env},tag_request_id:RequestId::new(tag_request_id)?,owner_proof:proof},Some(project),Some(revision))?;
            let incomplete=matches!(&response,ResponseBody::StashTag(receipt) if receipt.state!=StashTagState::VerifiedAdded);print_response(response)?;
            if incomplete {bail!("Stash tag ADD remains incomplete or unknown; reconcile before retrying");}Ok(())
        }
        Commands::Stash {command:cli::StashCommands::SubmitScan {project,publication_request_id,url,api_key_env,revision,request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;let project=ProjectId::new(project)?;
            let revision=match revision {Some(value)=>RevisionId::new(value),None=>get_snapshot(&mut client,&project)?.revision};
            let request_id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            eprintln!("Stash scan request {request_id}; reconcile this ID before resubmitting.");
            let response=client.execute_with_id(request_id,Command::StashSubmitScan {connection:StashConnection {endpoint:url,credential_env:api_key_env},publication_request_id:RequestId::new(publication_request_id)?,owner_proof:proof},Some(project),Some(revision))?;
            let unknown=matches!(&response,ResponseBody::StashScan(receipt) if receipt.state==StashScanState::DispatchUnknown);
            print_response(response)?;
            if unknown {bail!("Stash scan acknowledgement is unknown; reconcile the retained request ID, do not resubmit");}Ok(())
        }
        Commands::Stash {command:cli::StashCommands::ScanStatus {project,scan_request_id,url,api_key_env}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashScanStatus {connection:StashConnection {endpoint:url,credential_env:api_key_env},scan_request_id:RequestId::new(scan_request_id)?,owner_proof:proof},Some(ProjectId::new(project)?),None)?)
        }
        Commands::Stash {command:cli::StashCommands::ReconcileAttachment {project,scan_request_id,url,api_key_env}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashReconcileAttachment {connection:StashConnection {endpoint:url,credential_env:api_key_env},scan_request_id:RequestId::new(scan_request_id)?,owner_proof:proof},Some(ProjectId::new(project)?),None)?)
        }
        Commands::Stash {command:cli::StashCommands::PublicationStatus {project,publication_request_id}}=> {
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashPublicationStatus {publication_request_id:RequestId::new(publication_request_id)?,owner_proof:proof},Some(ProjectId::new(project)?),None)?)
        }
        Commands::Stash {command:cli::StashCommands::PublishScript {project,scene,file,source_version,candidate,job,url,api_key_env,revision,request_id,source_time,archived_origin}}=> {
            let archived_origin=archived_origin.map(|p|->Result<PackageObjectRef>{let bytes=inference::read_bounded_profile(&p)?;let reference:PackageObjectRef=serde_json::from_slice(&bytes)?;reference.validate()?;Ok(reference)}).transpose()?;
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let project=ProjectId::new(project)?;let revision=match revision {Some(value)=>RevisionId::new(value),None=>get_snapshot(&mut client,&project)?.revision};
            let request_id=request_id.map(RequestId::new).transpose()?.unwrap_or_else(session::new_request_id);
            eprintln!("Stash publication request {request_id}; retain this ID and exact arguments if the acknowledgement is lost.");
            print_response(client.execute_with_id(request_id,Command::StashExportScript {connection:StashConnection {endpoint:url,credential_env:api_key_env},source_version:SourceVersionId::new(source_version)?,scene_id:scene,file_id:file,candidate_id:CandidateId::new(candidate)?,job_id:JobId::new(job)?,owner_proof:proof,source_time,archived_origin},Some(project),Some(revision))?)
        }
        Commands::Stash { command:cli::StashCommands::CaptureSource {project,scene,file,mappings,url,api_key_env,revision,expected_source_version} } => {
            let bytes=inference::read_bounded_profile(&mappings).context("read bounded Stash mappings")?;
            let mappings:Vec<StashRootMapping>=serde_json::from_slice(&bytes)?;
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            let project=ProjectId::new(project)?;
            let revision=match revision {Some(value)=>RevisionId::new(value),None=>get_snapshot(&mut client,&project)?.revision};
            print_response(client.execute(Command::StashImportSource {connection:StashConnection {endpoint:url,credential_env:api_key_env},scene_id:scene,file_id:file,expected_server_path:None,expected_source_version:expected_source_version.map(SourceVersionId::new).transpose()?,mappings,owner_proof:proof},Some(project),Some(revision))?)
        }
        Commands::Stash { command:cli::StashCommands::PlanMissing {limit,url,api_key_env,mappings} } => {
            let bytes=inference::read_bounded_profile(&mappings).context("read bounded Stash mappings")?;
            let mappings:Vec<StashRootMapping>=serde_json::from_slice(&bytes)?;
            let proof=project_package::read_local_owner_proof(bootstrap.parent().context("missing private engine directory")?,"stash-owner.token")?;
            let mut client=SessionClient::connect(endpoint,bootstrap)?;
            print_response(client.execute(Command::StashPlanMissing {connection:StashConnection {endpoint:url,credential_env:api_key_env},limit,mappings,owner_proof:proof},None,None)?)
        }
        Commands::Stash { .. } => unsupported("Stash auto-generation/linking; discovery is available through stash list-missing"),
    }
}

fn validate_browser_handy_modes(guard: bool, playback: bool, hsp: bool) -> Result<()> {
    anyhow::ensure!(
        [guard, playback, hsp].into_iter().filter(|selected| *selected).count() <= 1,
        "Choose only one Handy mode: existing-play monitoring, prepared Start controls, or HSP controls"
    );
    Ok(())
}

fn read_browser_hsp_config(path: &Path) -> Result<browser_handy_hsp::Config> {
    anyhow::ensure!(
        path.is_absolute(),
        "Handy HSP configuration path must be absolute"
    );
    let config: browser_handy_hsp::Config =
        serde_json::from_slice(&inference::read_bounded_profile(path)?)?;
    config.validate()?;
    Ok(config)
}

fn unsupported<T>(operation: &str) -> Result<T> {
    Err(ProtocolError::unsupported(operation).into())
}

/// An explicit boot preserves a saved request fingerprint; Engine authority
/// decides whether it is historical replay or a refused stale admission.
fn capture_engine_epoch(
    client: &mut impl EngineApi,
    project: &ProjectId,
    revision: RevisionId,
    declared: Option<String>,
) -> Result<AttemptId> {
    if let Some(epoch) = declared {
        return Ok(AttemptId::new(epoch)?);
    }
    let ResponseBody::LiveCaptureLimits(limits) = client.execute(
        Command::LiveCaptureLimits,
        Some(project.clone()),
        Some(revision),
    )?
    else {
        bail!("Unexpected capture limits response");
    };
    Ok(limits.engine_epoch)
}

fn print_response(response: ResponseBody) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub fn create_project(api: &mut impl EngineApi, name: String) -> Result<ProjectSnapshot> {
    match api.execute(Command::CreateProject { name }, None, None)? {
        ResponseBody::Project(project) => Ok(*project),
        _ => bail!("Engine returned an unexpected create-project response"),
    }
}

pub fn get_snapshot(api: &mut impl EngineApi, project: &ProjectId) -> Result<ProjectSnapshot> {
    match api.execute(Command::GetSnapshot, Some(project.clone()), None)? {
        ResponseBody::Project(snapshot) => Ok(*snapshot),
        _ => bail!("Engine returned an unexpected project response"),
    }
}

/// The policy is nonsecret intent. Reading it grants no evidence acceptance or
/// execution permission; the Engine checks all selected records and current scope.
fn read_fallback_selection(path: &Path) -> Result<InferenceFallbackSelection> {
    use std::{fs, io::Read};
    cli::parse_fallback_selection_path(
        path.to_str()
            .context("Fallback selection path must be UTF-8")?,
    )
    .map_err(anyhow::Error::msg)?;
    let limit = INFERENCE_EQUIVALENCE_MANIFEST_LIMIT;
    let named = fs::symlink_metadata(path).context("Inspect fallback selection")?;
    anyhow::ensure!(
        named.is_file() && !named.file_type().is_symlink(),
        "Fallback selection must be a regular file"
    );
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    let mut file = options.open(path).context("Open fallback selection")?;
    let before = file.metadata()?;
    anyhow::ensure!(
        before.is_file() && (1..=limit).contains(&before.len()),
        "Fallback selection must contain 1..128 KiB of regular-file JSON"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    let named_after = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        bytes.len() as u64 == before.len()
            && after.len() == before.len()
            && named_after.is_file()
            && !named_after.file_type().is_symlink()
            && named_after.len() == before.len()
            && before.modified().ok() == after.modified().ok(),
        "Fallback selection changed during intake"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let identity = |m: &fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            )
        };
        anyhow::ensure!(
            identity(&named) == identity(&before)
                && identity(&before) == identity(&after)
                && identity(&before) == identity(&named_after),
            "Fallback selection identity changed during intake"
        );
    }
    let selection: InferenceFallbackSelection =
        serde_json::from_slice(&bytes).context("Parse fallback selection")?;
    selection.validate()?;
    Ok(selection)
}

fn prepared_fallback_model(
    settings: &GenerationSettings,
    path: Option<&Path>,
    choice: Option<&ModelPackChoice>,
    selection: Option<&InferenceFallbackSelection>,
) -> Result<Option<InferenceFallbackModel>> {
    let Some(selection) = selection else {
        return Ok(None);
    };
    selection.validate()?;
    anyhow::ensure!(
        settings.raw_frame_pair.is_none(),
        "Fallback selection requires decoded media"
    );
    let profile = settings
        .inference_profile
        .as_ref()
        .context("Fallback selection requires an explicit inference profile")?;
    profile.validate()?;
    anyhow::ensure!(
        settings
            .model_input
            .as_ref()
            .is_some_and(|input| serde_json::to_vec(input).ok()
                == serde_json::to_vec(&profile.preprocessing.input).ok()),
        "Fallback model input differs from the explicit profile"
    );
    let model = match (path, choice) {
        (Some(path), None) => InferenceFallbackModel::Path {
            path: path.to_owned(),
        },
        (None, Some(choice)) => InferenceFallbackModel::ModelPack {
            choice: choice.clone(),
        },
        _ => bail!("Fallback selection requires exactly one explicit model path or selected pack"),
    };
    model.validate()?;
    Ok(Some(model))
}

#[cfg(test)]
fn generate_to_file(
    api: &mut impl EngineApi,
    video: &Path,
    output: &Path,
    preset: &str,
    settings: GenerationSettings,
    model_path: Option<PathBuf>,
    independent_audio: Option<(PathBuf, AudioAlignment)>,
) -> Result<()> {
    generate_to_file_with_pack(
        api,
        FileGenerationInput {
            video,
            output,
            preset,
            settings,
            model_path,
            independent_audio,
            choice: None,
            fallback_selection: None,
        },
    )
}
struct FileGenerationInput<'a> {
    video: &'a Path,
    output: &'a Path,
    preset: &'a str,
    settings: GenerationSettings,
    model_path: Option<PathBuf>,
    independent_audio: Option<(PathBuf, AudioAlignment)>,
    choice: Option<ModelPackChoice>,
    fallback_selection: Option<InferenceFallbackSelection>,
}
fn generate_to_file_with_pack(
    api: &mut impl EngineApi,
    input: FileGenerationInput<'_>,
) -> Result<()> {
    let FileGenerationInput {
        video,
        output,
        preset,
        mut settings,
        model_path,
        independent_audio,
        choice,
        fallback_selection,
    } = input;
    settings.validate_execution_preset(preset, model_path.is_some() || choice.is_some())?;
    let fallback_model = prepared_fallback_model(
        &settings,
        model_path.as_deref(),
        choice.as_ref(),
        fallback_selection.as_ref(),
    )?;
    let name = video
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let project = create_project(
        api,
        if name.is_empty() {
            "Generated motion".into()
        } else {
            name
        },
    )?;
    eprintln!(
        "Project: {}. Generation output remains unqualified until benchmark evidence exists.",
        project.project_id
    );
    let source_version = match api.execute(
        Command::ImportSource {
            path: absolute_path(video)?,
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Source { source_version } => source_version,
        _ => bail!("Engine returned an unexpected source-import response"),
    };
    if let Some((path, alignment)) = independent_audio {
        let current = get_snapshot(api, &project.project_id)?;
        let audio_version = match api.execute(
            Command::ImportSource {
                path: absolute_path(&path)?,
            },
            Some(project.project_id.clone()),
            Some(current.revision),
        )? {
            ResponseBody::Source { source_version } => source_version,
            _ => bail!("Engine returned an unexpected audio import response"),
        };
        settings
            .mixed_audio
            .as_mut()
            .context("Independent audio requires mixed-audio settings")?
            .independent = Some(IndependentAudio {
            source_version: audio_version,
            alignment,
        });
    }
    let project = get_snapshot(api, &project.project_id)?;
    let job = match api.execute(
        if let Some(selection) = fallback_selection {
            Command::GenerateWithInferenceFallback {
                source_version,
                preset: preset.to_owned(),
                settings: Box::new(settings),
                model: fallback_model.context("Validated fallback model is missing")?,
                selection: Box::new(selection),
            }
        } else if let Some(choice) = choice {
            Command::GenerateFromModelPack {
                source_version,
                preset: preset.to_owned(),
                settings: Some(settings),
                choice,
            }
        } else {
            Command::Generate {
                source_version,
                preset: preset.to_owned(),
                settings: Some(settings),
                model_path,
            }
        },
        Some(project.project_id.clone()),
        Some(project.revision),
    )? {
        ResponseBody::Job(job) => job,
        _ => bail!("Engine returned an unexpected generation response"),
    };
    let candidate = modality::await_candidate(api, &project, job)?;
    modality::commit_and_export(api, &project, candidate, output, Axis::Stroke)
}

pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Ok(std::env::current_dir()
        .context("Resolve client path")?
        .join(path))
}

#[derive(serde::Serialize)]
struct LibraryEntry {
    video: PathBuf,
    script: PathBuf,
    has_script: bool,
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Discovery is client input selection, not generation or project persistence.
/// Do not traverse symlinks or an unbounded directory tree.
fn library_entries(
    folder: &Path,
    recursive: bool,
    audio_bundle: bool,
) -> Result<Vec<LibraryEntry>> {
    let root = absolute_path(folder)?;
    let mut directories = vec![(root, 0usize)];
    let mut entries = Vec::new();
    let mut visited = 0usize;
    while let Some((directory, depth)) = directories.pop() {
        if depth > 64 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "library depth exceeds 64",
            )
            .into());
        }
        for entry in std::fs::read_dir(&directory)
            .with_context(|| format!("Read library directory {}", directory.display()))?
        {
            let entry = entry?;
            visited += 1;
            if visited > 100_000 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "library scan exceeds 100000 entries",
                )
                .into());
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && recursive {
                directories.push((entry.path(), depth + 1));
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let video = matches!(
                extension.as_str(),
                "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg"
            );
            let audio = audio_bundle
                && matches!(
                    extension.as_str(),
                    "wav" | "flac" | "mp3" | "m4a" | "ogg" | "opus" | "aac" | "aiff" | "aif"
                );
            if kind.is_file() && (video || audio) {
                let script = if audio_bundle {
                    let mut name = path
                        .file_name()
                        .context("Media filename missing")?
                        .to_os_string();
                    name.push(".pulsar-audio-bundle");
                    path.with_file_name(name)
                } else {
                    path.with_extension("funscript")
                };
                let has_script = if audio_bundle {
                    match std::fs::symlink_metadata(&script) {
                        Ok(_) => true,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                        Err(error) => return Err(error.into()),
                    }
                } else {
                    script.is_file()
                };
                entries.push(LibraryEntry {
                    has_script,
                    video: path,
                    script,
                });
            }
        }
    }
    entries.sort_by(|a, b| a.video.cmp(&b.video));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reference_set_commands_parse_without_engine_startup() {
        use clap::Parser;
        let create = [
            "pulsar",
            "reference-set-create",
            "--root",
            "/data/Live-Reference",
            "--video",
            "clip.mp4",
            "--reference",
            "clip.funscript",
            "--view",
            "/data/view.json",
            "--id",
            "clip",
            "--group",
            "source-family",
            "--authorization",
            "owner record",
            "--reference-offset-ms",
            "-250",
            "--destination",
            "/data/new-set",
        ]
        .map(std::ffi::OsString::from);
        assert!(matches!(
            super::Cli::try_parse_from(&create).unwrap().command,
            Some(super::Commands::ReferenceSetCreate {
                reference_offset_ms: -250,
                ..
            })
        ));
        assert!(!super::command_needs_engine(&create));
        let inspect =
            ["pulsar", "reference-set-inspect", "/data/set"].map(std::ffi::OsString::from);
        assert!(matches!(
            super::Cli::try_parse_from(&inspect).unwrap().command,
            Some(super::Commands::ReferenceSetInspect { .. })
        ));
        assert!(!super::command_needs_engine(&inspect));
    }
    use super::*;
    #[test]
    fn companion_axis_inspection_is_local_without_engine_startup() {
        assert!(!command_needs_engine(&args(&[
            "pulsar",
            "library-axes",
            "/fixture"
        ])));
        assert!(!command_needs_engine(&args(&[
            "pulsar",
            "library-axes",
            "/fixture",
            "--recursive"
        ])));
    }
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn audio_folder_targets_keep_full_names_and_preserve_existing_stroke_and_bundle_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.wav"), b"discovery only").unwrap();
        std::fs::write(dir.path().join("clip.flac"), b"discovery only").unwrap();
        std::fs::write(dir.path().join("clip.funscript"), b"preserve stroke").unwrap();
        let existing = dir.path().join("clip.flac.pulsar-audio-bundle");
        std::fs::write(&existing, b"preserve unrelated target").unwrap();
        let entries = library_entries(dir.path(), false, true).unwrap();
        assert_eq!(entries.len(), 2);
        assert_ne!(entries[0].script, entries[1].script);
        assert!(entries
            .iter()
            .any(|entry| entry.script == existing && entry.has_script));
        assert!(entries.iter().any(|entry| entry.script
            == dir.path().join("clip.wav.pulsar-audio-bundle")
            && !entry.has_script));
        assert!(library_entries(dir.path(), false, false)
            .unwrap()
            .is_empty());
        assert_eq!(
            std::fs::read(existing).unwrap(),
            b"preserve unrelated target"
        );
        assert_eq!(
            std::fs::read(dir.path().join("clip.funscript")).unwrap(),
            b"preserve stroke"
        );
    }
    #[test]
    fn runtime_launch_is_host_only_and_forwards_arguments() {
        let valid = args(&[
            "pulsar",
            "runtime-launch",
            "/supplier",
            "/cache",
            "--",
            "--help",
        ]);
        assert!(!command_needs_engine(&valid));
        assert!(!command_opens_gui(&valid));
        assert!(
            matches!(runtime_install_command(&valid), Some(Commands::RuntimeLaunch {arguments, ..}) if arguments == [OsString::from("--help")])
        );
    }
    #[test]
    fn atomic_bundle_cli_requires_explicit_bundle_and_exact_export_revision() {
        assert!(Cli::try_parse_from(args(&[
            "pulsar",
            "audio-synth",
            "/source.wav",
            "--atomic-bundle"
        ]))
        .is_err());
        assert!(Cli::try_parse_from(args(&[
            "pulsar",
            "export-bundle",
            "project",
            "/destination"
        ]))
        .is_err());
        assert!(matches!(
            Cli::try_parse_from(args(&[
                "pulsar",
                "export-bundle",
                "project",
                "/destination",
                "--revision",
                "3"
            ]))
            .unwrap()
            .command,
            Some(Commands::ExportBundle { revision: 3, .. })
        ));
    }
    #[test]
    fn audio_bundle_recipe_is_explicit_and_cannot_be_used_without_bundle() {
        assert!(Cli::try_parse_from(args(&[
            "pulsar",
            "audio-synth",
            "/source.wav",
            "--bundle-recipe",
            "/recipe.json"
        ]))
        .is_err());
        assert!(matches!(
            Cli::try_parse_from(args(&[
                "pulsar",
                "audio-synth",
                "/source.wav",
                "--bundle",
                "--bundle-recipe",
                "/recipe.json"
            ]))
            .unwrap()
            .command,
            Some(Commands::AudioSynth {
                bundle: true,
                bundle_recipe: Some(_),
                ..
            })
        ));
    }
    #[test]
    fn retained_runtime_selection_is_host_only_and_preserves_forwarded_arguments() {
        let launch = args(&[
            "pulsar",
            "runtime-cache-launch",
            "/cache",
            "digest",
            "--",
            "--version",
        ]);
        assert!(!command_needs_engine(&launch));
        assert!(!command_opens_gui(&launch));
        assert!(
            matches!(runtime_install_command(&launch), Some(Commands::RuntimeCacheLaunch { arguments, .. }) if arguments == [OsString::from("--version")])
        );
        let inspect = args(&["pulsar", "runtime-cache-inspect", "/cache", "digest"]);
        assert!(!command_needs_engine(&inspect));
        assert!(!command_opens_gui(&inspect));
        assert!(matches!(
            runtime_install_command(&inspect),
            Some(Commands::RuntimeCacheInspect { .. })
        ));
        assert!(Cli::try_parse_from(args(&["pulsar", "runtime-cache-launch", "/cache"])).is_err());
    }
    #[test]
    fn bundled_discovery_is_local_host_only_and_never_requires_engine_or_gui() {
        for command in [
            args(&["pulsar", "bundled-models"]),
            args(&["pulsar", "runtime-models", "/cache", "digest"]),
        ] {
            assert!(!command_needs_engine(&command));
            assert!(!command_opens_gui(&command));
            assert!(runtime_install_command(&command).is_some());
        }
        assert!(Cli::try_parse_from(args(&["pulsar", "runtime-models", "/cache"])).is_err());
        assert!(
            Cli::try_parse_from(args(&["pulsar", "bundled-models", "--approve-native"])).is_err()
        );
    }
    #[test]
    fn runtime_pack_is_local_without_engine_or_gui() {
        let valid = args(&["pulsar", "runtime-pack", "/supplier", "/archive"]);
        assert!(!command_needs_engine(&valid));
        assert!(!command_opens_gui(&valid));
        assert!(runtime_install_command(&valid).is_some());
    }
    #[test]
    fn runtime_recovery_commands_require_local_host_and_explicit_discard_approval() {
        let inspect = args(&["pulsar", "runtime-stage-inspect", "/stage"]);
        assert!(!command_needs_engine(&inspect));
        assert!(!command_opens_gui(&inspect));
        assert!(runtime_install_command(&inspect).is_some());
        let discard = args(&[
            "pulsar",
            "runtime-stage-discard",
            "/stage",
            "--expected-manifest-sha256",
            "digest",
            "--approve-discard",
        ]);
        assert!(!command_needs_engine(&discard));
        assert!(runtime_install_command(&discard).is_some());
        let mut unapproved = discard;
        unapproved.pop();
        assert!(Cli::try_parse_from(unapproved).is_err());
    }
    #[test]
    fn runtime_update_requires_pinned_base_and_runs_without_engine() {
        let hash = "a".repeat(64);
        let command = args(&[
            "pulsar",
            "runtime-update",
            "/delta",
            "/base",
            "/new",
            "--expected-base-manifest-sha256",
            &hash,
        ]);
        assert!(!command_needs_engine(&command));
        assert!(
            matches!(runtime_install_command(&command),Some(Commands::RuntimeUpdate {expected_base_manifest_sha256, ..}) if expected_base_manifest_sha256==hash)
        );
        assert!(runtime_install_command(&args(&[
            "pulsar",
            "runtime-update",
            "/delta",
            "/base",
            "/new"
        ]))
        .is_none());
    }
    #[test]
    fn runtime_update_pack_requires_pin_and_stays_local() {
        let command = args(&[
            "pulsar",
            "runtime-update-pack",
            "/new",
            "/base",
            "/archive",
            "--expected-base-manifest-sha256",
            &"a".repeat(64),
        ]);
        assert!(!command_needs_engine(&command));
        assert!(!command_opens_gui(&command));
        assert!(matches!(
            runtime_install_command(&command),
            Some(Commands::RuntimeUpdatePack { .. })
        ));
        assert!(runtime_install_command(&args(&[
            "pulsar",
            "runtime-update-pack",
            "/new",
            "/base",
            "/archive"
        ]))
        .is_none());
    }
    #[test]
    fn archive_stage_commands_are_local_and_discard_requires_approval() {
        let inspect = args(&["pulsar", "runtime-archive-stage-inspect", "/stage"]);
        assert!(!command_needs_engine(&inspect));
        assert!(!command_opens_gui(&inspect));
        assert!(runtime_install_command(&inspect).is_some());
        let discard = args(&[
            "pulsar",
            "runtime-archive-stage-discard",
            "/stage",
            "--expected-stage-sha256",
            &"a".repeat(64),
            "--approve-discard",
        ]);
        assert!(!command_needs_engine(&discard));
        assert!(!command_opens_gui(&discard));
        assert!(runtime_install_command(&discard).is_some());
        let mut incomplete = discard;
        incomplete.pop();
        assert!(runtime_install_command(&incomplete).is_none());
    }
    #[test]
    fn runtime_install_is_a_local_host_command_without_engine_or_gui() {
        let valid = args(&[
            "pulsar",
            "runtime-install",
            "/supplier",
            "/new-installation",
        ]);
        assert!(!command_needs_engine(&valid));
        assert!(!command_opens_gui(&valid));
        assert!(runtime_install_command(&valid).is_some());
        assert!(
            runtime_install_command(&args(&["pulsar", "runtime-install", "/supplier"])).is_none()
        );
        assert!(!command_needs_engine(&args(&[
            "pulsar",
            "runtime-install",
            "--help"
        ])));
    }
    #[test]
    fn startup_selection_and_disable_are_approved_local_host_commands() {
        for words in [
            vec![
                "pulsar",
                "assistant-select",
                "--policy",
                "/policy",
                "--approve-host-inference",
            ],
            vec![
                "pulsar",
                "assistant-disable",
                "--expected-policy-sha256",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "--approve-disable",
            ],
        ] {
            let approved = words.iter().map(OsString::from).collect::<Vec<_>>();
            assert!(assistant_setup_command(&approved).is_some());
            assert!(!command_needs_engine(&approved));
            let unapproved = approved[..approved.len() - 1].to_vec();
            assert!(assistant_setup_command(&unapproved).is_none());
            assert!(!command_needs_engine(&unapproved));
        }
    }
    #[test]
    fn host_setup_requires_explicit_approval_and_never_starts_engine() {
        let valid = args(&[
            "pulsar",
            "assistant-setup",
            "--output",
            "/install/policy.json",
            "--runtime",
            "/install/runtime",
            "--model",
            "/install/model",
            "--approve-host-inference",
        ]);
        assert!(!command_needs_engine(&valid));
        assert!(!command_opens_gui(&valid));
        assert!(assistant_setup_command(&valid).is_some());
        let mut unapproved = valid.clone();
        unapproved.pop();
        assert!(assistant_setup_command(&unapproved).is_none());
        assert!(Cli::try_parse_from(unapproved).is_err());
        assert!(!command_needs_engine(&args(&[
            "pulsar",
            "assistant-setup",
            "--help"
        ])));
    }
    #[test]
    fn informational_commands_do_not_start_engine() {
        assert!(!command_needs_engine(&args(&["pulsar", "--help"])));
        assert!(!command_needs_engine(&args(&["pulsar", "--version"])));
        assert!(!command_needs_engine(&args(&[
            "pulsar",
            "completions",
            "bash"
        ])));
        assert!(command_needs_engine(&args(&["pulsar"])));
        assert!(command_needs_engine(&args(&[
            "pulsar",
            "generate",
            "video.mp4"
        ])));
    }

    #[test]
    fn only_valid_graphical_commands_open_recovery_window() {
        for values in [&["pulsar"][..], &["pulsar", "gui"][..]] {
            assert!(command_opens_gui(&args(values)));
        }
        for values in [
            &["pulsar", "--help"][..],
            &["pulsar", "--version"][..],
            &["pulsar", "gui", "--help"][..],
            &["pulsar", "gui", "unexpected"][..],
            &["pulsar", "completions", "bash"][..],
            &["pulsar", "generate", "video.mp4"][..],
        ] {
            assert!(!command_opens_gui(&args(values)));
        }
    }

    #[test]
    fn forwarded_runtime_arguments_preserve_graphical_classification() {
        for values in [&[][..], &["gui"][..]] {
            assert!(runtime_launch_opens_gui(&args(values)));
        }
        for values in [
            &["--help"][..],
            &["--version"][..],
            &["gui", "--help"][..],
            &["gui", "unexpected"][..],
            &["completions", "bash"][..],
            &["engine"][..],
            &["generate", "video.mp4"][..],
        ] {
            assert!(!runtime_launch_opens_gui(&args(values)));
        }
    }

    #[test]
    fn unsupported_legacy_capability_keeps_typed_failure() {
        let error = unsupported::<()>("legacy test mode").unwrap_err();
        assert_eq!(
            error.downcast_ref::<ProtocolError>().unwrap().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn unsupported_visual_recipes_refuse_before_any_file_generation_side_effect() {
        struct NoCalls;
        impl EngineApi for NoCalls {
            fn execute(
                &mut self,
                _: Command,
                _: Option<ProjectId>,
                _: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                panic!("invalid generation must refuse before project/source creation")
            }
        }
        for (preset, settings, model) in [
            ("maximum", GenerationSettings::default(), None),
            ("quality", GenerationSettings::default(), None),
            ("dense", GenerationSettings::default(), None),
            (
                "default",
                GenerationSettings {
                    source_sample_stride: 0,
                    ..Default::default()
                },
                None,
            ),
            (
                "maximum",
                GenerationSettings {
                    model_input: Some(ModelInputContract {
                        width: 64,
                        height: 64,
                        channels: 6,
                        layout: TensorLayout::Nchw,
                        class_count: 1,
                        decoder: DetectorDecoder::TemporalScriptPosition,
                    }),
                    ..Default::default()
                },
                Some(PathBuf::from("/fixture.onnx")),
            ),
        ] {
            assert!(generate_to_file(
                &mut NoCalls,
                Path::new("/fixture.mkv"),
                Path::new("/output.funscript"),
                preset,
                settings,
                model,
                None
            )
            .is_err());
        }
    }

    #[test]
    fn cli_and_batch_generation_use_engine_commits_and_pin_candidate_revision() {
        struct FakeEngine {
            responses: std::collections::VecDeque<ResponseBody>,
            requests: Vec<(Command, Option<ProjectId>, Option<RevisionId>)>,
        }
        impl EngineApi for FakeEngine {
            fn execute(
                &mut self,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                self.requests.push((command, project, revision));
                self.responses
                    .pop_front()
                    .context("unexpected extra request")
            }
        }
        let project_id = ProjectId::new("project").unwrap();
        let snapshot = |revision| ProjectSnapshot {
            project_id: project_id.clone(),
            revision: RevisionId::new(revision),
            name: "fixture".into(),
            motion: crate::motion::fixture_descriptor(
                &MotionProgram::default(),
                MotionBinding::ProjectRevision {
                    project_id: project_id.clone(),
                    revision: RevisionId::new(revision),
                },
            ),
            program: MotionProgram::default().into(),
            sources: vec![],
            placements: vec![],
        };
        let candidate_id = CandidateId::new("candidate").unwrap();
        let output = std::env::temp_dir().join("pulsar-test-output.funscript");
        let mut fake = FakeEngine {
            requests: vec![],
            responses: [
                ResponseBody::Project((snapshot(0)).into()),
                ResponseBody::Source {
                    source_version: SourceVersionId::new("source").unwrap(),
                },
                ResponseBody::Project((snapshot(1)).into()),
                ResponseBody::Job(JobSnapshot {
                    job_id: JobId::new("job").unwrap(),
                    attempt_id: AttemptId::new("attempt").unwrap(),
                    project_id: project_id.clone(),
                    base_revision: RevisionId::new(1),
                    state: "completed".into(),
                    candidate_id: Some(candidate_id),
                    error: None,
                }),
                ResponseBody::Candidate(
                    (CandidateSnapshot {
                        project_interval: None,
                        candidate_id: CandidateId::new("candidate").unwrap(),
                        project_id: project_id.clone(),
                        base_revision: RevisionId::new(1),
                        motion: crate::motion::fixture_descriptor(
                            &MotionProgram::default(),
                            MotionBinding::Candidate {
                                project_id: project_id.clone(),
                                candidate_id: CandidateId::new("candidate").unwrap(),
                                base_revision: RevisionId::new(1),
                            },
                        ),
                        program: MotionProgram::default().into(),
                        job_id: Some(JobId::new("job").unwrap()),
                        review: vec![],
                    })
                    .into(),
                ),
                ResponseBody::Project((snapshot(2)).into()),
                ResponseBody::AxisExported {
                    path: output.clone(),
                    revision: RevisionId::new(2),
                    axis: Axis::Stroke,
                },
            ]
            .into_iter()
            .collect(),
        };
        generate_to_file(
            &mut fake,
            Path::new("fixture.mp4"),
            &output,
            "default",
            GenerationSettings::default(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(fake.requests.len(), 7);
        assert!(matches!(fake.requests[1].0, Command::ImportSource { .. }));
        assert!(matches!(fake.requests[3].0, Command::Generate { .. }));
        assert!(matches!(
            fake.requests[5].0,
            Command::CommitCandidate { .. }
        ));
        assert_eq!(fake.requests[5].2.unwrap().value(), 1);
        assert!(matches!(
            fake.requests[6].0,
            Command::ExportAxis {
                axis: Axis::Stroke,
                ..
            }
        ));
        assert_eq!(fake.requests[6].2.unwrap().value(), 2);
    }
}

#[cfg(test)]
mod inference_fallback_generation_tests {
    use super::*;

    fn selection() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![InferenceFallbackReference {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                expected_revision: 2,
                expected_report_sha256: "b".repeat(64),
            }],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Forbidden,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }
    fn settings() -> GenerationSettings {
        let profile = crate::inference_equivalence::environment_tests::profile();
        GenerationSettings {
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile),
            source_sample_stride: 3,
            ..Default::default()
        }
    }

    #[test]
    fn fallback_policy_intake_is_bounded_strict_and_validates_selection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("policy.json");
        let expected = selection();
        let mut bytes = serde_json::to_vec(&expected).unwrap();
        bytes.resize(INFERENCE_EQUIVALENCE_MANIFEST_LIMIT as usize, b' ');
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            read_fallback_selection(&path).unwrap().identity().unwrap(),
            expected.identity().unwrap()
        );
        bytes.push(b' ');
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_fallback_selection(&path).is_err());
        assert!(read_fallback_selection(directory.path()).is_err());
        assert!(read_fallback_selection(Path::new("policy.json")).is_err());
        for raw in [
            serde_json::to_string(&expected)
                .unwrap()
                .replacen('{', "{\"schema_version\":1,", 1),
            serde_json::to_string(&expected)
                .unwrap()
                .replacen('{', "{\"accepted\":true,", 1),
        ] {
            std::fs::write(&path, raw).unwrap();
            assert!(read_fallback_selection(&path).is_err());
        }
        let mut invalid = expected;
        invalid.maximum_fallbacks = 0;
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(read_fallback_selection(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn fallback_policy_intake_refuses_symlinks_without_requiring_secret_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("policy.json");
        std::fs::write(&path, serde_json::to_vec(&selection()).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        read_fallback_selection(&path).unwrap();
        let link = directory.path().join("alias.json");
        symlink(&path, &link).unwrap();
        assert!(read_fallback_selection(&link).is_err());
    }

    #[test]
    fn invalid_fallback_or_implicit_model_refuses_before_project_or_source_creation() {
        struct NoCalls;
        impl EngineApi for NoCalls {
            fn execute(
                &mut self,
                _: Command,
                _: Option<ProjectId>,
                _: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                panic!("invalid fallback must refuse before any Engine command")
            }
        }
        for mode in 0..3 {
            let mut policy = selection();
            let prepared = if mode == 0 {
                GenerationSettings::default()
            } else {
                settings()
            };
            if mode == 1 {
                policy.maximum_fallbacks = 0;
            }
            let model = if mode == 2 {
                None
            } else {
                Some(PathBuf::from(crate::test_host_path("model.onnx")))
            };
            assert!(generate_to_file_with_pack(
                &mut NoCalls,
                FileGenerationInput {
                    video: Path::new("source.mp4"),
                    output: Path::new("output.funscript"),
                    preset: "default",
                    settings: prepared,
                    model_path: model,
                    independent_audio: None,
                    choice: None,
                    fallback_selection: Some(policy)
                }
            )
            .is_err());
        }
        assert!(
            prepared_fallback_model(&GenerationSettings::default(), None, None, None)
                .unwrap()
                .is_none()
        );
    }

    struct FakeEngine {
        responses: std::collections::VecDeque<ResponseBody>,
        requests: Vec<(Command, Option<ProjectId>, Option<RevisionId>)>,
    }
    impl EngineApi for FakeEngine {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.requests.push((command, project, revision));
            self.responses
                .pop_front()
                .context("unexpected extra command")
        }
    }

    #[test]
    fn explicit_fallback_pending_preserves_profile_pack_choice_review_commit_and_export() {
        for (opt_in, pack) in [(false, false), (false, true), (true, false), (true, true)] {
            let project_id = ProjectId::new("project").unwrap();
            let snapshot = |revision| ProjectSnapshot {
                project_id: project_id.clone(),
                revision: RevisionId::new(revision),
                name: "fixture".into(),
                motion: crate::motion::fixture_descriptor(
                    &MotionProgram::default(),
                    MotionBinding::ProjectRevision {
                        project_id: project_id.clone(),
                        revision: RevisionId::new(revision),
                    },
                ),
                program: MotionProgram::default().into(),
                sources: vec![],
                placements: vec![],
            };
            let candidate_id = CandidateId::new("candidate").unwrap();
            let output = std::env::temp_dir().join("pulsar-test-output.funscript");
            let mut fake = FakeEngine {
                requests: vec![],
                responses: [
                    ResponseBody::Project((snapshot(0)).into()),
                    ResponseBody::Source {
                        source_version: SourceVersionId::new("source").unwrap(),
                    },
                    ResponseBody::Project((snapshot(1)).into()),
                    ResponseBody::Job(JobSnapshot {
                        job_id: JobId::new("job").unwrap(),
                        attempt_id: AttemptId::new("attempt").unwrap(),
                        project_id: project_id.clone(),
                        base_revision: RevisionId::new(1),
                        state: if opt_in {
                            "fallback_pending"
                        } else {
                            "running"
                        }
                        .into(),
                        candidate_id: None,
                        error: None,
                    }),
                    ResponseBody::Job(JobSnapshot {
                        job_id: JobId::new("job").unwrap(),
                        attempt_id: AttemptId::new("successor-attempt").unwrap(),
                        project_id: project_id.clone(),
                        base_revision: RevisionId::new(1),
                        state: "completed".into(),
                        candidate_id: Some(candidate_id),
                        error: None,
                    }),
                    ResponseBody::Candidate(
                        (CandidateSnapshot {
                            project_interval: None,
                            candidate_id: CandidateId::new("candidate").unwrap(),
                            project_id: project_id.clone(),
                            base_revision: RevisionId::new(1),
                            motion: crate::motion::fixture_descriptor(
                                &MotionProgram::default(),
                                MotionBinding::Candidate {
                                    project_id: project_id.clone(),
                                    candidate_id: CandidateId::new("candidate").unwrap(),
                                    base_revision: RevisionId::new(1),
                                },
                            ),
                            program: MotionProgram::default().into(),
                            job_id: Some(JobId::new("job").unwrap()),
                            review: vec![],
                        })
                        .into(),
                    ),
                    ResponseBody::Project((snapshot(2)).into()),
                    ResponseBody::AxisExported {
                        path: output.clone(),
                        revision: RevisionId::new(2),
                        axis: Axis::Stroke,
                    },
                ]
                .into_iter()
                .collect(),
            };

            let settings = settings();
            let expected_settings = serde_json::to_vec(&settings).unwrap();
            let policy = selection();
            let expected_policy = serde_json::to_vec(&policy).unwrap();
            let choice = pack.then(|| ModelPackChoice {
                manifest_sha256: "c".repeat(64),
                selection_revision: 7,
            });
            let path = (!pack).then(|| PathBuf::from(crate::test_host_path("model.onnx")));
            generate_to_file_with_pack(
                &mut fake,
                FileGenerationInput {
                    video: Path::new("source.mp4"),
                    output: &output,
                    preset: "default",
                    settings,
                    model_path: path.clone(),
                    independent_audio: None,
                    choice: choice.clone(),
                    fallback_selection: opt_in.then_some(policy),
                },
            )
            .unwrap();
            assert_eq!(fake.requests.len(), 8);
            match (&fake.requests[3].0, opt_in, pack) {
                (Command::GenerateWithInferenceFallback {source_version,preset,settings,model,selection},true,_) => {
                    assert_eq!(source_version.as_str(),"source");
                    assert_eq!(preset,"default");
                    assert_eq!(serde_json::to_vec(settings.as_ref()).unwrap(),expected_settings);
                    assert_eq!(serde_json::to_vec(selection.as_ref()).unwrap(),expected_policy);
                    match model {
                        InferenceFallbackModel::Path {path:actual}=>assert_eq!(Some(actual),path.as_ref()),
                        InferenceFallbackModel::ModelPack {choice:actual}=>assert_eq!(Some(actual),choice.as_ref()),
                    }
                }
                (Command::Generate {settings:Some(actual),model_path,..},false,false) => {
                    assert_eq!(serde_json::to_vec(actual).unwrap(),expected_settings);
                    assert_eq!(model_path,&path);
                }
                (Command::GenerateFromModelPack {settings:Some(actual),choice:actual_choice,..},false,true) => {
                    assert_eq!(serde_json::to_vec(actual).unwrap(),expected_settings);
                    assert_eq!(Some(actual_choice),choice.as_ref());
                }
                _=>panic!("fallback must be explicit; strict model/profile and pack commands remain unchanged"),
            }
            assert!(matches!(fake.requests[4].0, Command::JobStatus { .. }));
            assert!(matches!(
                fake.requests[6].0,
                Command::CommitCandidate { .. }
            ));
            assert_eq!(fake.requests[6].2.unwrap().value(), 1);
            assert!(matches!(
                fake.requests[7].0,
                Command::ExportAxis {
                    axis: Axis::Stroke,
                    ..
                }
            ));
            assert_eq!(fake.requests[7].2.unwrap().value(), 2);
            assert!(fake.requests.iter().all(|(command, _, _)| !matches!(
                command,
                Command::AcceptInferenceEquivalence { .. }
            )));
        }
    }
}
