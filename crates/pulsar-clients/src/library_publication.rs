//! Private client handoff history. Engine generation and filesystem publication remain distinct.
use crate::{neutral_bundle::*, SessionClient};
#[cfg(any(target_os = "linux", target_os = "android"))]
use crate::{project_package::PackageDownloadControl, EngineApi, ResponseBody};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

include!("library_pending_retirement.rs");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationPhase {
    AwaitingGeneration,
    Pending,
    Preparing,
    Prepared,
    Published,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationItem {
    pub generation: LibraryBatchItem,
    pub phase: PublicationPhase,
    pub prepared: Option<PreparedNeutralBundle>,
    pub receipt: Option<PublishedNeutralBundle>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialPublication>,
}
include!("library_partial_publication.rs");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationReport {
    pub schema: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<Box<LibraryBatchMigration>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<Box<ModelPackChoice>>,
    pub batch_request_id: RequestId,
    pub items: Vec<PublicationItem>,
    /// Recorded publisher acknowledgements, not a fresh observation of destination contents.
    pub publication_completed: bool,
    pub qualified: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkflowReport {
    pub generation: LibraryBatchReport,
    pub publication: PublicationReport,
    pub last_job_status: Option<JobSnapshot>,
    pub steps_used: u16,
    pub generation_attempts: u16,
    pub publication_attempts: u16,
    pub job_status_polls: u16,
    pub cancellation_requested: bool,
    pub cancellation_acknowledged: bool,
    pub local_stop_requested: bool,
    pub completed: bool,
    pub budget_exhausted: bool,
    pub needs_attention: bool,
    pub qualified: bool,
}

/// Stops only future local scheduler decisions. The current bounded operation may finish.
#[derive(Clone, Default)]
pub struct WorkflowControl(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl WorkflowControl {
    pub fn request_stop(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
    pub fn stopped(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

/// Bounded, explicit per-entry scheduling. Restart dispatches nothing automatically.
pub fn run(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    steps: u16,
    poll_ms: u16,
) -> Result<WorkflowReport> {
    run_controlled(
        client,
        batch,
        directory,
        steps,
        poll_ms,
        &WorkflowControl::default(),
    )
}

pub fn run_controlled(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    steps: u16,
    poll_ms: u16,
    control: &WorkflowControl,
) -> Result<WorkflowReport> {
    ensure!(
        (1..=1000).contains(&steps) && (1..=1000).contains(&poll_ms),
        "Choose 1..=1000 scheduler steps and polling milliseconds"
    );
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let ResponseBody::LibraryBatch(mut generation) = client.execute(
            Command::LibraryBatchStatus {
                batch_request_id: batch.clone(),
            },
            None,
            None,
        )?
        else {
            anyhow::bail!("Unexpected generation status");
        };
        let binding = client.recovery_credential_binding();
        let initial = Journal::from_generation(binding.clone(), generation.clone(), true)?;
        let mut store = local::Store::open(directory, &binding, Some(initial))?;
        if store.journal.reconcile_generation(&generation)? {
            store.save()?;
        }
        let mut result = WorkflowReport {
            generation: generation.clone(),
            publication: store.journal.report.clone(),
            last_job_status: None,
            steps_used: 0,
            generation_attempts: 0,
            publication_attempts: 0,
            job_status_polls: 0,
            cancellation_requested: false,
            cancellation_acknowledged: false,
            local_stop_requested: false,
            completed: false,
            budget_exhausted: false,
            needs_attention: false,
            qualified: false,
        };
        for _ in 0..steps {
            if control.stopped() {
                break;
            }
            let ResponseBody::LibraryBatch(current) = client.execute(
                Command::LibraryBatchStatus {
                    batch_request_id: batch.clone(),
                },
                None,
                None,
            )?
            else {
                anyhow::bail!("Unexpected generation status");
            };
            if store.journal.reconcile_generation(&current)? {
                store.save()?;
            }
            generation = current;
            if generation.cancellation.is_some() {
                break;
            }
            if store.journal.report.publication_completed {
                break;
            }
            if let Some(index) = store.journal.report.items.iter().position(|item| {
                item.phase != PublicationPhase::Published
                    && item.phase != PublicationPhase::AwaitingGeneration
            }) {
                result.steps_used += 1;
                result.publication_attempts += 1;
                publish_item(client, &mut store, index, false)?;
                if store.journal.report.items[index].error.is_some() {
                    break;
                }
                continue;
            }
            if generation.successor_batch_request_id.is_some() {
                break;
            }
            if generation.items.iter().any(|item| item.error.is_some()) {
                break;
            }
            let Some(item) = generation
                .items
                .iter()
                .find(|item| item.phase != LibraryBatchPhase::ReadyForPublication)
            else {
                anyhow::bail!("Workflow readiness differs from generation ledger");
            };
            result.steps_used += 1;
            if item.phase == LibraryBatchPhase::Generating
                && generation.pending_advance_request_id.is_none()
            {
                let observation = read_job(client, item)?;
                result.job_status_polls += 1;
                let state = observation.state.clone();
                result.last_job_status = Some(observation);
                if state != "completed" {
                    if !matches!(state.as_str(), "queued" | "running" | "assistant_paused") {
                        result.needs_attention = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(poll_ms as u64));
                    continue;
                }
            }
            // The Engine records pending mutation IDs before effects. Read its current identity,
            // including after a lost acknowledgement; never substitute another pending advance.
            let request = generation
                .pending_advance_request_id
                .clone()
                .unwrap_or_else(crate::session::new_request_id);
            result.generation_attempts += 1;
            let ResponseBody::LibraryBatch(next) = client.execute_identified(
                request,
                Command::LibraryBatchAdvance {
                    batch_request_id: batch.clone(),
                },
                None,
                None,
            )?
            else {
                anyhow::bail!("Unexpected retained generation advance");
            };
            store.journal.reconcile_generation(&next)?;
            store.save()?;
        }
        let ResponseBody::LibraryBatch(current) = client.execute(
            Command::LibraryBatchStatus {
                batch_request_id: batch.clone(),
            },
            None,
            None,
        )?
        else {
            anyhow::bail!("Unexpected generation status");
        };
        if store.journal.reconcile_generation(&current)? {
            store.save()?;
        }
        generation = current;
        result.cancellation_requested = generation.cancellation.is_some();
        result.cancellation_acknowledged = generation
            .cancellation
            .as_ref()
            .is_some_and(|c| c.completed);
        result.completed = generation.successor_batch_request_id.is_none()
            && !result.cancellation_requested
            && generation.generation_ready
            && store.journal.report.publication_completed;
        result.needs_attention |= generation
            .cancellation
            .as_ref()
            .is_some_and(|c| !c.completed || c.error.is_some());
        result.needs_attention |= generation.successor_batch_request_id.is_some();
        result.needs_attention |= generation.items.iter().any(|item| item.error.is_some())
            || store
                .journal
                .report
                .items
                .iter()
                .any(|item| item.error.is_some() || item.phase == PublicationPhase::Preparing);
        result.local_stop_requested = control.stopped();
        result.budget_exhausted = !result.completed
            && !result.local_stop_requested
            && !result.cancellation_requested
            && !result.needs_attention
            && result.steps_used == steps;
        result.generation = generation;
        result.publication = store.journal.report.clone();
        Ok(result)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, batch, directory, control);
        Err(ProtocolError::unsupported("local batch scheduler on this platform").into())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg(any(target_os = "linux", target_os = "android"))]
struct Journal {
    credential_binding: String,
    report: PublicationReport,
}
#[cfg(any(target_os = "linux", target_os = "android"))]
impl Journal {
    fn new(binding: String, generation: LibraryBatchReport) -> Result<Self> {
        ensure!(
            generation.generation_ready,
            "Retained generation-ready batch required; no replacement generation is dispatched"
        );
        Self::from_generation(binding, generation, false)
    }
    fn from_generation(
        binding: String,
        generation: LibraryBatchReport,
        workflow: bool,
    ) -> Result<Self> {
        generation.validate_migration()?;
        generation.validate_publication_shape()?;
        ensure!(
            !generation.qualified
                && !generation.publication_completed
                && generation.generation_ready
                    == generation
                        .items
                        .iter()
                        .all(|item| item.phase == LibraryBatchPhase::ReadyForPublication),
            "Generation report shape differs"
        );
        let journal = Self {
            credential_binding: binding,
            report: PublicationReport {
                migration: generation.migration.clone(),
                model_pack: generation.model_pack.clone(),
                schema: if generation.model_pack.is_some() {
                    4
                } else if generation.schema == 6 {
                    6
                } else if generation.schema == 5 {
                    5
                } else if generation.is_visual() {
                    3
                } else if workflow {
                    2
                } else {
                    1
                },
                batch_request_id: generation.batch_request_id,
                items: generation
                    .items
                    .into_iter()
                    .map(|generation| PublicationItem {
                        partial: None,
                        phase: if generation.phase == LibraryBatchPhase::ReadyForPublication {
                            PublicationPhase::Pending
                        } else {
                            PublicationPhase::AwaitingGeneration
                        },
                        generation,
                        prepared: None,
                        receipt: None,
                        error: None,
                    })
                    .collect(),
                publication_completed: false,
                qualified: false,
            },
        };
        journal.validate(&journal.credential_binding)?;
        Ok(journal)
    }
    fn reconcile_generation(&mut self, generation: &LibraryBatchReport) -> Result<bool> {
        generation.validate_migration()?;
        generation.validate_publication_shape()?;
        ensure!(
            generation.migration == self.report.migration
                && ((generation.schema == 5) == (self.report.schema == 5))
                && ((generation.schema == 6) == (self.report.schema == 6))
                && (generation.is_visual() == matches!(self.report.schema, 3 | 4))
                && generation.model_pack == self.report.model_pack
                && generation.batch_request_id == self.report.batch_request_id
                && generation.items.len() == self.report.items.len()
                && !generation.qualified
                && !generation.publication_completed,
            "Workflow belongs to another generation batch"
        );
        let mut next = self.report.clone();
        for (item, current) in next.items.iter_mut().zip(&generation.items) {
            if item.phase == PublicationPhase::AwaitingGeneration {
                retain_generation_identity(&item.generation, current)?;
                item.generation = current.clone();
                if current.phase == LibraryBatchPhase::ReadyForPublication {
                    item.phase = PublicationPhase::Pending;
                }
            } else {
                ensure!(
                    serde_json::to_value(&item.generation)? == serde_json::to_value(current)?,
                    "Ready generation identity changed; no publication dispatched"
                );
            }
        }
        let candidate = Self {
            credential_binding: self.credential_binding.clone(),
            report: next,
        };
        candidate.validate(&self.credential_binding)?;
        let changed =
            serde_json::to_value(&candidate.report)? != serde_json::to_value(&self.report)?;
        self.report = candidate.report;
        Ok(changed)
    }
    fn validate(&self, binding: &str) -> Result<()> {
        let report = &self.report;
        if let Some(m) = &report.migration {
            ensure!(
                m.parent_batch_request_id != report.batch_request_id
                    && m.parent_report_sha256.len() == 64
                    && m.parent_report_sha256
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                "Publication migration lineage differs"
            );
        }
        ensure!(
            self.credential_binding == binding
                && binding.len() == 64
                && matches!(report.schema, 1..=6)
                && ((report.schema == 4) == report.model_pack.is_some())
                && !report.qualified
                && (1..=MAX_LIBRARY_BATCH_ITEMS).contains(&report.items.len()),
            "Publication journal identity or shape differs"
        );
        ensure!(
            report.schema != 5
                || report
                    .items
                    .iter()
                    .any(|i| i.generation.publication_axes.is_some()),
            "Selected audio publication lacks a subset"
        );
        ensure!(
            report.schema != 6
                || report
                    .items
                    .iter()
                    .any(|i| i.generation.publication_directory.is_some()),
            "Partial audio publication lacks an existing directory"
        );
        let mut destinations = std::collections::BTreeSet::new();
        for item in &report.items {
            let generation = &item.generation;
            ensure!(
                report.schema == 6 || generation.publication_directory.is_none(),
                "Existing directory requires partial audio publication"
            );
            ensure!(
                if report.schema == 6 {
                    generation.publication_axes.as_deref().is_none_or(|axes| {
                        valid_audio_publication_subset(axes)
                            || (generation.publication_directory.is_some() && axes == Axis::ALL)
                    })
                } else if report.schema == 5 {
                    generation
                        .publication_axes
                        .as_deref()
                        .is_none_or(valid_audio_publication_subset)
                } else {
                    generation.publication_axes
                        == if matches!(report.schema, 3 | 4) {
                            Some(vec![Axis::Stroke])
                        } else {
                            None
                        }
                },
                "Publication axis selection differs"
            );
            ensure!(
                generation.source.is_absolute()
                    && generation.destination.is_absolute()
                    && generation.source_bytes > 0
                    && generation.source_sha256.len() == 64
                    && generation
                        .source_sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && destinations.insert(&generation.destination),
                "Publication requires distinct captured generation entries"
            );
            if item.phase == PublicationPhase::AwaitingGeneration {
                ensure!(
                    matches!(report.schema, 2..=6)
                        && generation.phase != LibraryBatchPhase::ReadyForPublication,
                    "Awaiting publication requires unfinished generation"
                );
                validate_generation_dependencies(generation)?;
            } else {
                ensure!(
                    generation.phase == LibraryBatchPhase::ReadyForPublication
                        && generation.error.is_none()
                        && generation.project_id.is_some()
                        && generation.source_version.is_some()
                        && generation.job_id.is_some()
                        && generation.candidate_id.is_some(),
                    "Publication requires ready generation identities"
                );
            }
            ensure!(
                item.error.as_ref().is_none_or(|error| error.len() <= 4096),
                "Publication error exceeds admission"
            );
            if generation.publication_directory.is_some() {
                ensure!(
                    report.schema == 6
                        && generation
                            .publication_directory
                            .is_some_and(|id| id.inode > 0),
                    "Partial directory identity/schema differs"
                );
                validate_partial_publication(item)?;
                continue;
            }
            ensure!(
                item.partial.is_none(),
                "New-bundle publication cannot carry partial channel intents"
            );
            ensure!(
                matches!(
                    item.phase,
                    PublicationPhase::Prepared | PublicationPhase::Published
                ) == item.prepared.is_some()
                    && (item.phase == PublicationPhase::Published) == item.receipt.is_some(),
                "Publication phase lacks its acknowledgement"
            );
            if let Some(prepared) = &item.prepared {
                ensure!(
                    prepared.schema
                        == if generation.publication_axes().len() == 6 {
                            1
                        } else {
                            2
                        }
                        && Some(&prepared.project) == generation.project_id.as_ref()
                        && prepared.revision == generation.revision
                        && prepared.destination == generation.destination
                        && prepared.staging.parent() == generation.destination.parent()
                        && prepared.staging.file_name().is_some_and(|name| name
                            .to_str()
                            .is_some_and(|name| name.starts_with(".pulsar-neutral-bundle-")))
                        && prepared.requests.len() == generation.publication_axes().len()
                        && prepared
                            .requests
                            .iter()
                            .map(|r| r.axis)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == generation.publication_axes().len()
                        && prepared
                            .requests
                            .iter()
                            .map(|r| &r.request)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len()
                            == generation.publication_axes().len()
                        && prepared
                            .requests
                            .iter()
                            .map(|r| r.axis)
                            .collect::<std::collections::BTreeSet<_>>()
                            == generation.publication_axes().iter().copied().collect(),
                    "Publication retained stage differs from generation selection"
                );
            }
            if let Some(receipt) = &item.receipt {
                ensure!(
                    Some(&receipt.project) == generation.project_id.as_ref()
                        && receipt.revision == generation.revision
                        && receipt.destination == generation.destination
                        && receipt.axes == generation.publication_axes()
                        && receipt.file_and_directory_synced
                        && !receipt.qualified
                        && item.error.is_none(),
                    "Publication receipt differs from selected generation"
                );
            }
        }
        ensure!(
            report.publication_completed
                == report
                    .items
                    .iter()
                    .all(|item| item.phase == PublicationPhase::Published),
            "Publication completion lacks recorded acknowledgements"
        );
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn phase_rank(phase: LibraryBatchPhase) -> u8 {
    match phase {
        LibraryBatchPhase::Selected => 0,
        LibraryBatchPhase::ProjectCreated => 1,
        LibraryBatchPhase::SourceImported => 2,
        LibraryBatchPhase::Generating => 3,
        LibraryBatchPhase::CandidateReady => 4,
        LibraryBatchPhase::ReadyForPublication => 5,
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn validate_generation_dependencies(item: &LibraryBatchItem) -> Result<()> {
    let rank = phase_rank(item.phase);
    ensure!(
        ((rank >= 1) == item.project_id.is_some())
            && ((rank >= 2) == item.source_version.is_some())
            && item
                .source_version
                .as_ref()
                .is_none_or(|source| source.as_str() == item.source_sha256)
            && ((rank >= 3) == item.job_id.is_some())
            && ((rank >= 4) == item.candidate_id.is_some()),
        "Workflow generation phase lacks captured identities"
    );
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn retain_generation_identity(
    previous: &LibraryBatchItem,
    current: &LibraryBatchItem,
) -> Result<()> {
    ensure!(
        previous.source == current.source
            && previous.destination == current.destination
            && previous.source_sha256 == current.source_sha256
            && previous.source_bytes == current.source_bytes
            && previous.publication_axes == current.publication_axes
            && previous.publication_directory == current.publication_directory
            && phase_rank(current.phase) >= phase_rank(previous.phase)
            && current.revision >= previous.revision
            && previous
                .project_id
                .as_ref()
                .is_none_or(|id| Some(id) == current.project_id.as_ref())
            && previous
                .source_version
                .as_ref()
                .is_none_or(|id| Some(id) == current.source_version.as_ref())
            && previous
                .job_id
                .as_ref()
                .is_none_or(|id| Some(id) == current.job_id.as_ref())
            && previous
                .candidate_id
                .as_ref()
                .is_none_or(|id| Some(id) == current.candidate_id.as_ref())
            && current.effects.len() >= previous.effects.len()
            && previous
                .effects
                .iter()
                .zip(&current.effects)
                .all(|(a, b)| a.request_id == b.request_id && a.phase == b.phase),
        "Retained generation identity changed or regressed; no replacement work dispatched"
    );
    validate_generation_dependencies(current)
}

/// Marks an uncertain publication of recovered client metadata; preflight errors carry no marker.
#[derive(Debug)]
pub(crate) struct PreparationWriteUnknown(String);
impl std::fmt::Display for PreparationWriteUnknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Recovery metadata write could not be acknowledged: {}; retain the exact recovery inputs", self.0)
    }
}
impl std::error::Error for PreparationWriteUnknown {}

/// Explicitly attach an inspected original stage after preparation acknowledgement loss.
/// This only restores client metadata; it dispatches no exports or generation.
pub fn recover_preparation(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    index: u16,
    staging: &Path,
    preparation_receipt: &Path,
) -> Result<PublicationReport> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let ResponseBody::LibraryBatch(generation) = client.execute(
            Command::LibraryBatchStatus {
                batch_request_id: batch.clone(),
            },
            None,
            None,
        )?
        else {
            anyhow::bail!("Unexpected generation status");
        };
        let mut store = local::Store::open(directory, &client.recovery_credential_binding(), None)?;
        ensure!(
            store.journal.report.batch_request_id == batch,
            "Publication journal belongs to another batch"
        );
        store.journal.reconcile_generation(&generation)?;
        ensure!(
            (index as usize) < store.journal.report.items.len(),
            "Publication entry index out of range"
        );
        let candidate = store.read_pending_preparation(preparation_receipt)?;
        let prepared = recover_prepared_neutral_bundle(client, staging)?;
        let mut expected = store.journal.clone();
        let changed = attach_preparation(&mut expected.report.items[index as usize], prepared)?;
        ensure!(
            serde_json::to_vec(&expected)? == serde_json::to_vec(&candidate)?,
            "Pending preparation receipt differs from the exact retained journal transition"
        );
        if changed {
            store.journal = expected;
            store.save().map_err(|error| {
                let detail = error.to_string();
                error.context(PreparationWriteUnknown(detail))
            })?;
        }
        Ok(store.journal.report.clone())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (
            client,
            batch,
            directory,
            index,
            staging,
            preparation_receipt,
        );
        Err(ProtocolError::unsupported("local batch preparation recovery on this platform").into())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn attach_preparation(item: &mut PublicationItem, prepared: PreparedNeutralBundle) -> Result<bool> {
    ensure!(
        prepared.project
            == *item
                .generation
                .project_id
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Retained project absent"))?
            && prepared.revision == item.generation.revision
            && prepared.destination == item.generation.destination,
        "Retained stage belongs to another entry, project or revision"
    );
    if item.phase == PublicationPhase::Prepared {
        ensure!(
            item.prepared.as_ref() == Some(&prepared),
            "Preparation identity conflicts with retained acknowledgement"
        );
        return Ok(false);
    }
    ensure!(
        item.phase == PublicationPhase::Preparing
            && item.prepared.is_none()
            && item.receipt.is_none(),
        "Only unresolved preparation accepts an original retained stage"
    );
    item.prepared = Some(prepared);
    item.phase = PublicationPhase::Prepared;
    item.error = None;
    Ok(true)
}

/// Explicit publication steps; an interrupted Preparing phase never creates another intent.
pub fn prepare(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    steps: u16,
) -> Result<PublicationReport> {
    advance(client, batch, directory, steps, true)
}
pub fn publish(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    steps: u16,
) -> Result<PublicationReport> {
    advance(client, batch, directory, steps, false)
}
fn advance(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
    steps: u16,
    prepare_only: bool,
) -> Result<PublicationReport> {
    ensure!(
        (1..=100).contains(&steps),
        "Choose 1..=100 explicit publication steps"
    );
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let ResponseBody::LibraryBatch(generation) = client.execute(
            Command::LibraryBatchStatus {
                batch_request_id: batch.clone(),
            },
            None,
            None,
        )?
        else {
            anyhow::bail!("Unexpected batch generation status");
        };
        ensure!(
            generation.cancellation.is_none(),
            "Local batch cancellation requested; no publication dispatched"
        );
        let binding = client.recovery_credential_binding();
        let initial = Journal::new(binding.clone(), generation.clone())?;
        let mut store = local::Store::open(directory, &binding, Some(initial))?;
        ensure!(
            store.journal.report.batch_request_id == batch
                && store.journal.report.items.len() == generation.items.len(),
            "Publication journal belongs to another batch"
        );
        if store.journal.reconcile_generation(&generation)? {
            store.save()?;
        }
        for _ in 0..steps {
            let ResponseBody::LibraryBatch(current) = client.execute(
                Command::LibraryBatchStatus {
                    batch_request_id: batch.clone(),
                },
                None,
                None,
            )?
            else {
                anyhow::bail!("Unexpected generation status");
            };
            ensure!(
                current.cancellation.is_none(),
                "Local batch cancellation requested; no publication dispatched"
            );
            store.journal.reconcile_generation(&current)?;
            let Some(index) = store.journal.report.items.iter().position(|item| {
                if prepare_only {
                    matches!(
                        item.phase,
                        PublicationPhase::Pending | PublicationPhase::Preparing
                    )
                } else {
                    item.phase != PublicationPhase::Published
                }
            }) else {
                break;
            };
            publish_item(client, &mut store, index, prepare_only)?;
            if store.journal.report.items[index].error.is_some() {
                break;
            }
        }
        Ok(store.journal.report.clone())
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, batch, directory, prepare_only);
        Err(ProtocolError::unsupported("local batch publication on this platform").into())
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn bounded_error(error: anyhow::Error) -> String {
    error.to_string().chars().take(1024).collect()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn publish_item(
    client: &mut SessionClient,
    store: &mut local::Store,
    index: usize,
    prepare_only: bool,
) -> Result<()> {
    if store.journal.report.items[index]
        .generation
        .publication_directory
        .is_some()
    {
        return publish_partial_item(client, store, index, prepare_only);
    }
    let item = &store.journal.report.items[index];
    if item.phase == PublicationPhase::Preparing {
        anyhow::bail!("Publication preparation acknowledgement is unresolved; retain the journal and inspect local staging before any new intent");
    }
    if item.phase == PublicationPhase::Pending {
        store.journal.report.items[index].phase = PublicationPhase::Preparing;
        store.save()?;
        let input = &store.journal.report.items[index].generation;
        match prepare_neutral_axis_bundle(
            client,
            input.project_id.clone().expect("validated"),
            input.revision,
            &input.destination,
            input.publication_axes(),
        ) {
            Ok(prepared) => {
                let item = &mut store.journal.report.items[index];
                item.prepared = Some(prepared);
                item.phase = PublicationPhase::Prepared;
                item.error = None;
                store.save()?;
            }
            Err(error) => {
                store.journal.report.items[index].error = Some(bounded_error(error));
                store.save()?;
                return Ok(());
            }
        }
    }
    if prepare_only {
        return Ok(());
    }
    let prepared = store.journal.report.items[index]
        .prepared
        .clone()
        .expect("validated prepared phase");
    match publish_prepared_neutral_bundle(client, &prepared, &PackageDownloadControl::default()) {
        Ok(receipt) => {
            let item = &mut store.journal.report.items[index];
            item.receipt = Some(receipt);
            item.phase = PublicationPhase::Published;
            item.error = None;
            store.journal.report.publication_completed = store
                .journal
                .report
                .items
                .iter()
                .all(|item| item.phase == PublicationPhase::Published);
            store.save()?;
        }
        Err(error) => {
            store.journal.report.items[index].error = Some(bounded_error(error));
            store.save()?;
            return Ok(());
        }
    }
    Ok(())
}

/// Current Engine status and retained publication history; never advances or writes the journal.
pub fn workflow_status(
    client: &mut SessionClient,
    batch: RequestId,
    directory: &Path,
) -> Result<WorkflowReport> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let ResponseBody::LibraryBatch(generation) = client.execute(
            Command::LibraryBatchStatus {
                batch_request_id: batch,
            },
            None,
            None,
        )?
        else {
            anyhow::bail!("Unexpected generation status");
        };
        let binding = client.recovery_credential_binding();
        let mut store = local::Store::open(directory, &binding, None)?;
        // Validate current bindings without publishing reconciliation into this read-only history.
        let publication = store.journal.report.clone();
        store.journal.reconcile_generation(&generation)?;
        let last_job_status = generation
            .items
            .iter()
            .find(|item| item.phase == LibraryBatchPhase::Generating)
            .map(|item| read_job(client, item))
            .transpose()?;
        let needs_attention = generation.items.iter().any(|item| item.error.is_some())
            || publication
                .items
                .iter()
                .any(|item| item.error.is_some() || item.phase == PublicationPhase::Preparing)
            || last_job_status.as_ref().is_some_and(|job| {
                !matches!(
                    job.state.as_str(),
                    "queued" | "running" | "assistant_paused" | "completed"
                )
            });
        let cancellation_requested = generation.cancellation.is_some();
        let cancellation_acknowledged = generation
            .cancellation
            .as_ref()
            .is_some_and(|c| c.completed);
        let needs_attention = needs_attention
            || generation
                .cancellation
                .as_ref()
                .is_some_and(|c| !c.completed || c.error.is_some());
        let completed = !cancellation_requested
            && generation.generation_ready
            && publication.publication_completed;
        let polls = u16::from(last_job_status.is_some());
        Ok(WorkflowReport {
            generation,
            publication,
            last_job_status,
            steps_used: 0,
            generation_attempts: 0,
            publication_attempts: 0,
            job_status_polls: polls,
            cancellation_requested,
            cancellation_acknowledged,
            local_stop_requested: false,
            completed,
            budget_exhausted: false,
            needs_attention,
            qualified: false,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, batch, directory);
        Err(ProtocolError::unsupported("local batch workflow status on this platform").into())
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn read_job(client: &mut SessionClient, item: &LibraryBatchItem) -> Result<JobSnapshot> {
    let project = item
        .project_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Generating entry lacks project"))?;
    let job = item
        .job_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Generating entry lacks job"))?;
    let ResponseBody::Job(observation) =
        client.execute(Command::JobStatus { job_id: job }, Some(project), None)?
    else {
        anyhow::bail!("Unexpected retained job status");
    };
    validate_job_binding(item, &observation)?;
    Ok(observation)
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn validate_job_binding(item: &LibraryBatchItem, observation: &JobSnapshot) -> Result<()> {
    ensure!(
        Some(&observation.job_id) == item.job_id.as_ref()
            && Some(&observation.project_id) == item.project_id.as_ref()
            && observation.base_revision == item.revision,
        "Retained job status identity differs; no generation dispatched"
    );
    Ok(())
}

/// Authenticated journal history only; never adopts files or dispatches exports.
pub fn status(client: &mut SessionClient, directory: &Path) -> Result<PublicationReport> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Ok(
            local::Store::open(directory, &client.recovery_credential_binding(), None)?
                .journal
                .report
                .clone(),
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        let _ = (client, directory);
        Err(ProtocolError::unsupported("local batch publication history on this platform").into())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod local {
    use super::*;
    use rustix::fs::{open, openat, renameat, statat, AtFlags, FlockOperation, Mode, OFlags};
    use std::{
        fs::File,
        io::{Read, Write},
        os::unix::fs::MetadataExt,
        path::PathBuf,
    };
    const LIMIT: u64 = 1024 * 1024;
    pub(super) struct Store {
        pub(super) journal: Journal,
        root: File,
        path: PathBuf,
        lock: File,
        current: Option<std::fs::Metadata>,
    }
    impl Drop for Store {
        fn drop(&mut self) {
            let _ = rustix::fs::flock(&self.lock, FlockOperation::Unlock);
        }
    }
    fn private(file: &File, directory: bool) -> Result<()> {
        let m = file.metadata()?;
        ensure!(
            m.uid() == rustix::process::geteuid().as_raw()
                && m.mode() & 0o077 == 0
                && if directory {
                    m.is_dir()
                } else {
                    m.is_file() && m.nlink() == 1 && m.mode() & 0o777 == 0o600
                },
            "Publication state requires private owned storage; files must be single-link 0600"
        );
        Ok(())
    }
    fn unchanged(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        a.dev() == b.dev()
            && a.ino() == b.ino()
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    impl Store {
        fn require_current_record(&self) -> Result<()> {
            let file = File::from(openat(
                &self.root,
                "record.json",
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&file, false)?;
            let before = self
                .current
                .as_ref()
                .context("Current publication journal absent")?;
            ensure!(
                unchanged(before, &file.metadata()?),
                "Current acknowledgement record changed; preserve pending metadata"
            );
            let named = statat(&self.root, "record.json", AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                named.st_dev == before.dev() && named.st_ino == before.ino(),
                "Current acknowledgement record name changed"
            );
            Ok(())
        }
        pub(super) fn retire_pending(
            &self,
            path: &Path,
            expected: &str,
        ) -> Result<PendingRetirementReceipt> {
            use sha2::{Digest, Sha256};
            ensure!(
                path.is_absolute() && path.parent() == Some(self.path.as_path()),
                "Pending metadata must be in the current workflow directory"
            );
            ensure!(
                valid_retirement_digest(expected),
                "Expected SHA-256 must be 64 lowercase hexadecimal characters"
            );
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Pending filename absent")?;
            let id = name
                .strip_prefix(".pending-")
                .context("Select an original .pending journal")?;
            ensure!(
                uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id),
                "Pending filename must carry its original canonical UUID"
            );
            let archived = format!(".retired-pending-{id}-{expected}");
            self.check()?;
            let original = statat(&self.root, name, AtFlags::SYMLINK_NOFOLLOW);
            let already = match original {
                Ok(_) => false,
                Err(rustix::io::Errno::NOENT) => true,
                Err(error) => return Err(error.into()),
            };
            if !already {
                match statat(&self.root, archived.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                    Err(rustix::io::Errno::NOENT) => {}
                    Ok(_) => anyhow::bail!("Retirement target already exists; preserve both names"),
                    Err(error) => return Err(error.into()),
                }
            }
            let selected = if already { archived.as_str() } else { name };
            let mut file = File::from(openat(
                &self.root,
                selected,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&file, false)?;
            let before = file.metadata()?;
            ensure!(
                before.len() > 0 && before.len() <= LIMIT,
                "Pending journal exceeds bounded admission"
            );
            let mut bytes = Vec::new();
            (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= LIMIT && unchanged(&before, &file.metadata()?),
                "Pending journal changed during capture"
            );
            ensure!(
                format!("{:x}", Sha256::digest(&bytes)) == expected,
                "Pending journal SHA-256 differs"
            );
            let pending: Journal = serde_json::from_slice(&bytes)?;
            require_covered_pending(&self.journal, &pending)?;
            let named = statat(&self.root, selected, AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                named.st_dev == before.dev() && named.st_ino == before.ino(),
                "Pending journal name changed"
            );
            self.check()?;
            self.require_current_record()?;
            file.sync_all()?;
            if !already {
                rustix::fs::renameat_with(
                    &self.root,
                    name,
                    &self.root,
                    archived.as_str(),
                    rustix::fs::RenameFlags::NOREPLACE,
                )
                .map_err(|error| anyhow::Error::new(error).context(PendingRetirementUnknown))?;
            }
            let finish = (|| -> Result<()> {
                let named = statat(&self.root, archived.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
                ensure!(
                    named.st_dev == before.dev() && named.st_ino == before.ino(),
                    "Retired metadata identity changed; preserve the retained name"
                );
                private(&file, false)?;
                let after = file.metadata()?;
                use std::io::Seek;
                file.seek(std::io::SeekFrom::Start(0))?;
                let mut retained = Vec::new();
                (&mut file).take(LIMIT + 1).read_to_end(&mut retained)?;
                ensure!(
                    retained == bytes && unchanged(&after, &file.metadata()?),
                    "Retired metadata bytes changed; preserve the retained name"
                );
                self.check()?;
                self.require_current_record()?;
                self.root.sync_all()?;
                Ok(())
            })();
            finish.map_err(|error| error.context(PendingRetirementUnknown))?;
            Ok(PendingRetirementReceipt {
                directory: self.path.clone(),
                original_path: path.to_path_buf(),
                retained_path: self.path.join(archived),
                sha256: expected.into(),
                byte_len: before.len(),
                already_retired: already,
                file_and_directory_synced: true,
                qualified: false,
            })
        }
        pub(super) fn read_pending_preparation(&self, path: &Path) -> Result<Journal> {
            ensure!(
                path.is_absolute() && path.parent() == Some(self.path.as_path()),
                "Pending preparation receipt must be inside the retained workflow directory"
            );
            let name = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Pending receipt filename absent"))?;
            ensure!(
                name.to_str()
                    .is_some_and(|name| name.starts_with(".pending-")),
                "Only a retained pending publication receipt can recover preparation"
            );
            let mut file = File::from(openat(
                &self.root,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&file, false)?;
            let before = file.metadata()?;
            ensure!(
                before.len() <= LIMIT,
                "Pending preparation receipt exceeds admission"
            );
            let mut bytes = Vec::new();
            (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= LIMIT && unchanged(&before, &file.metadata()?),
                "Pending preparation receipt changed during capture"
            );
            private(&file, false)?;
            let named = statat(&self.root, name, AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                named.st_dev as u64 == before.dev() && named.st_ino as u64 == before.ino(),
                "Pending preparation receipt name changed"
            );
            let candidate: Journal = serde_json::from_slice(&bytes)?;
            candidate.validate(&self.journal.credential_binding)?;
            self.check()?;
            Ok(candidate)
        }
        pub(super) fn open(path: &Path, binding: &str, initial: Option<Journal>) -> Result<Self> {
            ensure!(
                path.is_absolute(),
                "Absolute private publication-state directory required"
            );
            let root = File::from(open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            private(&root, true)?;
            let lock = File::from(openat(
                &root,
                "lock",
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            private(&lock, false)?;
            rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
            // Construct the owner immediately so any read/refusal retires the lock explicitly.
            let placeholder = Journal {
                credential_binding: binding.to_owned(),
                report: PublicationReport {
                    migration: None,
                    model_pack: None,
                    schema: 1,
                    batch_request_id: RequestId::new("uninitialized")?,
                    items: vec![],
                    publication_completed: false,
                    qualified: false,
                },
            };
            let mut store = Self {
                journal: placeholder,
                root,
                path: path.to_owned(),
                lock,
                current: None,
            };
            store.check()?;
            match openat(
                &store.root,
                "record.json",
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => {
                    let mut file = File::from(fd);
                    private(&file, false)?;
                    let before = file.metadata()?;
                    ensure!(
                        before.len() > 0 && before.len() <= LIMIT,
                        "Publication record exceeds admission"
                    );
                    let mut bytes = Vec::new();
                    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
                    ensure!(
                        bytes.len() as u64 <= LIMIT && unchanged(&before, &file.metadata()?),
                        "Publication record changed or exceeds admission"
                    );
                    store.journal = serde_json::from_slice(&bytes)?;
                    store.current = Some(before);
                    store.journal.validate(binding)?;
                }
                Err(rustix::io::Errno::NOENT) => {
                    store.journal =
                        initial.ok_or_else(|| anyhow::anyhow!("No retained publication record"))?;
                    store.journal.validate(binding)?;
                    store.save()?;
                }
                Err(error) => return Err(error.into()),
            }
            Ok(store)
        }
        fn check(&self) -> Result<()> {
            private(&self.root, true)?;
            private(&self.lock, false)?;
            let held = self.root.metadata()?;
            let named = std::fs::symlink_metadata(&self.path)?;
            ensure!(
                named.is_dir() && named.dev() == held.dev() && named.ino() == held.ino(),
                "Publication state directory replaced"
            );
            let held = self.lock.metadata()?;
            let named = statat(&self.root, "lock", AtFlags::SYMLINK_NOFOLLOW)?;
            ensure!(
                named.st_dev == held.dev() && named.st_ino == held.ino(),
                "Publication ownership lock replaced"
            );
            Ok(())
        }
        pub(super) fn save(&mut self) -> Result<()> {
            self.check()?;
            self.journal.validate(&self.journal.credential_binding)?;
            match (
                &self.current,
                openat(
                    &self.root,
                    "record.json",
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                ),
            ) {
                (None, Err(rustix::io::Errno::NOENT)) => {}
                (Some(before), Ok(fd)) => {
                    let file = File::from(fd);
                    private(&file, false)?;
                    ensure!(
                        unchanged(before, &file.metadata()?),
                        "Publication record replaced or edited"
                    );
                }
                _ => anyhow::bail!("Publication record identity changed"),
            }
            let bytes = serde_json::to_vec(&self.journal)?;
            ensure!(
                bytes.len() as u64 <= LIMIT,
                "Publication record exceeds admission"
            );
            let name = format!(".pending-{}", uuid::Uuid::new_v4());
            let mut file = File::from(openat(
                &self.root,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            let result = (|| -> Result<()> {
                file.write_all(&bytes)?;
                file.sync_all()?;
                self.check()?;
                renameat(&self.root, name.as_str(), &self.root, "record.json")?;
                self.root.sync_all()?;
                self.current = Some(file.metadata()?);
                Ok(())
            })();
            if result.is_err() {
                let _ = rustix::fs::unlinkat(&self.root, name.as_str(), AtFlags::empty());
            }
            result
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    use super::*;
    fn retirement_journals(root: &Path) -> (Journal, Journal) {
        let mut report = generation(root);
        report.schema = 6;
        report.items[0].publication_directory = Some(PublicationDirectoryIdentity {
            device: 1,
            inode: 2,
        });
        report.items[0].publication_axes = Some(vec![Axis::Roll, Axis::Pitch]);
        let mut pending = Journal::from_generation("a".repeat(64), report, true).unwrap();
        pending.report.items[0].phase = PublicationPhase::Prepared;
        pending.report.items[0].partial = Some(PartialPublication {
            directory_identity: PublicationDirectoryIdentity {
                device: 1,
                inode: 2,
            },
            channels: vec![
                PartialPublicationChannel {
                    axis: Axis::Roll,
                    request_id: RequestId::new("original-roll").unwrap(),
                    acknowledged: true,
                    error: None,
                },
                PartialPublicationChannel {
                    axis: Axis::Pitch,
                    request_id: RequestId::new("original-pitch").unwrap(),
                    acknowledged: false,
                    error: None,
                },
            ],
        });
        let mut current = pending.clone();
        current.report.items[0].phase = PublicationPhase::Published;
        current.report.publication_completed = true;
        for channel in &mut current.report.items[0].partial.as_mut().unwrap().channels {
            channel.acknowledged = true;
        }
        (current, pending)
    }
    #[test]
    fn pending_retirement_requires_covered_exact_history_not_matching_file_presence() {
        let dir = tempfile::tempdir().unwrap();
        let (current, pending) = retirement_journals(dir.path());
        require_covered_pending(&current, &pending).unwrap();
        assert!(require_covered_pending(&pending, &pending).is_err());
        let mut foreign = pending.clone();
        foreign.credential_binding = "b".repeat(64);
        assert!(require_covered_pending(&current, &foreign).is_err());
        let mut replaced = pending.clone();
        replaced.report.items[0].partial.as_mut().unwrap().channels[0].request_id =
            RequestId::new("replacement").unwrap();
        assert!(require_covered_pending(&current, &replaced).is_err());
        let mut changed = pending;
        changed.report.batch_request_id = RequestId::new("another-batch").unwrap();
        assert!(require_covered_pending(&current, &changed).is_err());
    }
    #[test]
    fn pending_retirement_moves_preserved_bytes_retries_without_overwriting_current_record() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let (current, pending) = retirement_journals(dir.path());
        let store = local::Store::open(dir.path(), &"a".repeat(64), Some(current)).unwrap();
        let current_bytes = std::fs::read(dir.path().join("record.json")).unwrap();
        let path = dir
            .path()
            .join(format!(".pending-{}", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(&pending).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert!(store.retire_pending(&path, &"0".repeat(64)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let receipt = store.retire_pending(&path, &hash).unwrap();
        assert!(!receipt.already_retired && receipt.file_and_directory_synced);
        assert!(!path.exists());
        assert_eq!(std::fs::read(&receipt.retained_path).unwrap(), bytes);
        let repeated = store.retire_pending(&path, &hash).unwrap();
        assert!(repeated.already_retired);
        assert_eq!(repeated.retained_path, receipt.retained_path);
        assert_eq!(
            std::fs::read(dir.path().join("record.json")).unwrap(),
            current_bytes
        );
    }
    #[test]
    fn pending_retirement_preserves_conflicting_nonprivate_and_linked_metadata() {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let (current, pending) = retirement_journals(dir.path());
        let store = local::Store::open(dir.path(), &"a".repeat(64), Some(current)).unwrap();
        let id = uuid::Uuid::new_v4();
        let path = dir.path().join(format!(".pending-{id}"));
        let bytes = serde_json::to_vec(&pending).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        std::fs::write(&path, &bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.retire_pending(&path, &hash).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("other-link");
        std::fs::hard_link(&path, &link).unwrap();
        assert!(store.retire_pending(&path, &hash).is_err());
        std::fs::remove_file(link).unwrap();
        let retired = dir.path().join(format!(".retired-pending-{id}-{hash}"));
        std::fs::write(&retired, b"protect conflict").unwrap();
        assert!(store.retire_pending(&path, &hash).is_err());
        assert_eq!(std::fs::read(retired).unwrap(), b"protect conflict");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let alias = dir
            .path()
            .join(format!(".pending-{}", uuid::Uuid::new_v4()));
        symlink(&path, &alias).unwrap();
        assert!(store.retire_pending(&alias, &hash).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn partial_channel_intents_require_exact_directory_and_positive_completion() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = generation(dir.path());
        report.schema = 6;
        report.items[0].publication_directory = Some(PublicationDirectoryIdentity {
            device: 1,
            inode: 2,
        });
        report.items[0].publication_axes = Some(vec![Axis::Roll, Axis::Pitch]);
        let mut journal = Journal::from_generation("a".repeat(64), report.clone(), true).unwrap();
        journal.report.items[0].phase = PublicationPhase::Prepared;
        journal.report.items[0].partial = Some(PartialPublication {
            directory_identity: report.items[0].publication_directory.unwrap(),
            channels: vec![
                PartialPublicationChannel {
                    axis: Axis::Roll,
                    request_id: RequestId::new("roll").unwrap(),
                    acknowledged: true,
                    error: None,
                },
                PartialPublicationChannel {
                    axis: Axis::Pitch,
                    request_id: RequestId::new("pitch").unwrap(),
                    acknowledged: false,
                    error: Some("unknown outcome".into()),
                },
            ],
        });
        journal.validate(&"a".repeat(64)).unwrap();
        let mut changed = report.clone();
        changed.items[0]
            .publication_directory
            .as_mut()
            .unwrap()
            .inode = 3;
        assert!(journal.reconcile_generation(&changed).is_err());
        journal.report.items[0].phase = PublicationPhase::Published;
        assert!(journal.validate(&"a".repeat(64)).is_err());
        let partial = journal.report.items[0].partial.as_mut().unwrap();
        partial.channels[1].acknowledged = true;
        partial.channels[1].error = None;
        journal.report.publication_completed = true;
        journal.validate(&"a".repeat(64)).unwrap();
    }
    #[test]
    fn selected_audio_journal_keeps_subset_and_refuses_schema_or_axis_reinterpretation() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = generation(dir.path());
        report.schema = 5;
        report.items[0].publication_axes = Some(Axis::ALL[1..].to_vec());
        let mut journal = Journal::from_generation("a".repeat(64), report.clone(), true).unwrap();
        assert_eq!(journal.report.schema, 5);
        report.schema = 3;
        assert!(journal.reconcile_generation(&report).is_err());
        report.schema = 5;
        report.items[0].publication_axes = Some(vec![Axis::Stroke]);
        assert!(journal.reconcile_generation(&report).is_err());
        journal.report.items[0].generation.publication_axes = Some(Axis::ALL.to_vec());
        assert!(journal.validate(&"a".repeat(64)).is_err());
        let mut stroke = generation(dir.path());
        stroke.schema = 5;
        stroke.items[0].publication_axes = Some(vec![Axis::Stroke]);
        let mut stroke_journal =
            Journal::from_generation("a".repeat(64), stroke.clone(), true).unwrap();
        stroke.schema = 3;
        assert!(stroke.validate_publication_shape().is_ok());
        assert!(stroke_journal.reconcile_generation(&stroke).is_err());
    }
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };
    fn generation(root: &Path) -> LibraryBatchReport {
        let sha = "a".repeat(64);
        LibraryBatchReport {
            migration: None,
            successor_batch_request_id: None,
            model_pack: None,
            schema: 1,
            batch_request_id: RequestId::new("batch").unwrap(),
            items: vec![LibraryBatchItem {
                publication_directory: None,
                source: root.join("input.wav"),
                destination: root.join("output"),
                source_sha256: sha.clone(),
                publication_axes: None,
                source_bytes: 100,
                phase: LibraryBatchPhase::ReadyForPublication,
                project_id: Some(ProjectId::new("project").unwrap()),
                source_version: Some(SourceVersionId::new(sha).unwrap()),
                revision: RevisionId::new(3),
                job_id: Some(JobId::new("job").unwrap()),
                candidate_id: Some(CandidateId::new("candidate").unwrap()),
                effects: vec![],
                error: None,
            }],
            pending_advance_request_id: None,
            cancellation: None,
            generation_ready: true,
            publication_completed: false,
            qualified: false,
        }
    }
    fn initial(root: &Path) -> Journal {
        Journal::new("b".repeat(64), generation(root)).unwrap()
    }
    fn prepared(item: &PublicationItem) -> PreparedNeutralBundle {
        PreparedNeutralBundle {
            schema: 1,
            staging: item
                .generation
                .destination
                .parent()
                .unwrap()
                .join(".pulsar-neutral-bundle-authored"),
            destination: item.generation.destination.clone(),
            project: item.generation.project_id.clone().unwrap(),
            revision: item.generation.revision,
            requests: Axis::ALL
                .into_iter()
                .map(|axis| NeutralBundleAxisIntent {
                    axis,
                    request: RequestId::new(axis.as_str()).unwrap(),
                })
                .collect(),
            parent_identity: NeutralBundleFileIdentity {
                device: 1,
                inode: 2,
            },
            staging_identity: NeutralBundleFileIdentity {
                device: 1,
                inode: 3,
            },
        }
    }
    fn selected(root: &Path) -> LibraryBatchReport {
        let mut report = generation(root);
        report.generation_ready = false;
        let item = &mut report.items[0];
        item.phase = LibraryBatchPhase::Selected;
        item.revision = RevisionId::new(0);
        item.project_id = None;
        item.source_version = None;
        item.job_id = None;
        item.candidate_id = None;
        report
    }
    #[test]
    fn pack_journal_retains_choice_and_refuses_selection_substitution() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = selected(dir.path());
        report.schema = 4;
        report.items[0].publication_axes = Some(vec![Axis::Stroke]);
        report.model_pack = Some(Box::new(ModelPackChoice {
            manifest_sha256: "a".repeat(64),
            selection_revision: 1,
        }));
        let mut journal = Journal::from_generation("b".repeat(64), report.clone(), true).unwrap();
        assert_eq!(journal.report.schema, 4);
        journal.reconcile_generation(&report).unwrap();
        report.model_pack.as_mut().unwrap().selection_revision = 2;
        assert!(journal.reconcile_generation(&report).is_err());
        journal.report.schema = 3;
        assert!(journal.validate(&"b".repeat(64)).is_err());
    }
    #[test]
    fn visual_journal_binds_selected_axes_and_refuses_six_axis_substitution() {
        let root = tempfile::tempdir().unwrap();
        let mut generation = generation(root.path());
        generation.schema = 3;
        generation.items[0].publication_axes = Some(vec![Axis::Stroke]);
        let mut journal = Journal::new("b".repeat(64), generation).unwrap();
        assert_eq!(journal.report.schema, 3);
        let mut stage = prepared(&journal.report.items[0]);
        stage.schema = 2;
        stage.requests.retain(|r| r.axis == Axis::Stroke);
        journal.report.items[0].prepared = Some(stage);
        journal.report.items[0].phase = PublicationPhase::Prepared;
        journal.validate(&"b".repeat(64)).unwrap();
        journal.report.items[0].prepared = Some(prepared(&journal.report.items[0]));
        assert!(journal.validate(&"b".repeat(64)).is_err());
    }
    #[test]
    fn workflow_retains_monotonic_generation_and_refuses_replacement_or_regression() {
        let root = tempfile::tempdir().unwrap();
        let binding = "b".repeat(64);
        let mut current = selected(root.path());
        let mut journal = Journal::from_generation(binding.clone(), current.clone(), true).unwrap();
        assert_eq!(journal.report.schema, 2);
        assert_eq!(
            journal.report.items[0].phase,
            PublicationPhase::AwaitingGeneration
        );
        let item = &mut current.items[0];
        item.phase = LibraryBatchPhase::ProjectCreated;
        item.project_id = Some(ProjectId::new("actual-project").unwrap());
        item.effects.push(LibraryBatchEffect {
            request_id: RequestId::new("create").unwrap(),
            phase: LibraryBatchPhase::Selected,
        });
        assert!(journal.reconcile_generation(&current).unwrap());
        let retained = serde_json::to_vec(&journal.report).unwrap();
        let mut replacement = current.clone();
        replacement.items[0].project_id = Some(ProjectId::new("replacement").unwrap());
        assert!(journal.reconcile_generation(&replacement).is_err());
        let mut regressed = selected(root.path());
        regressed.items[0].source_sha256 = current.items[0].source_sha256.clone();
        assert!(journal.reconcile_generation(&regressed).is_err());
        assert_eq!(serde_json::to_vec(&journal.report).unwrap(), retained);
        current.items[0].phase = LibraryBatchPhase::SourceImported;
        current.items[0].source_version =
            Some(SourceVersionId::new(current.items[0].source_sha256.clone()).unwrap());
        current.items[0].revision = RevisionId::new(1);
        journal.reconcile_generation(&current).unwrap();
        current.items[0].phase = LibraryBatchPhase::Generating;
        current.items[0].job_id = Some(JobId::new("original-job").unwrap());
        journal.reconcile_generation(&current).unwrap();
        let retained = serde_json::to_vec(&journal.report).unwrap();
        replacement = current.clone();
        replacement.items[0].job_id = Some(JobId::new("new-job").unwrap());
        assert!(journal.reconcile_generation(&replacement).is_err());
        assert_eq!(serde_json::to_vec(&journal.report).unwrap(), retained);
    }
    #[test]
    fn ready_entry_can_publish_while_later_generation_fails_without_losing_its_receipt() {
        let root = tempfile::tempdir().unwrap();
        let binding = "b".repeat(64);
        let mut current = generation(root.path());
        let mut later = selected(root.path()).items.remove(0);
        later.source = root.path().join("bad.wav");
        later.destination = root.path().join("bad-output");
        later.error = Some(ProtocolError::new(
            ErrorCode::Unavailable,
            "authored late failure",
        ));
        current.items.push(later);
        current.generation_ready = false;
        let mut journal = Journal::from_generation(binding.clone(), current.clone(), true).unwrap();
        assert_eq!(journal.report.items[0].phase, PublicationPhase::Pending);
        assert_eq!(
            journal.report.items[1].phase,
            PublicationPhase::AwaitingGeneration
        );
        let item = &mut journal.report.items[0];
        item.prepared = Some(prepared(item));
        item.phase = PublicationPhase::Published;
        item.receipt = Some(PublishedNeutralBundle {
            destination: item.generation.destination.clone(),
            project: item.generation.project_id.clone().unwrap(),
            revision: item.generation.revision,
            axes: Axis::ALL.to_vec(),
            file_and_directory_synced: true,
            qualified: false,
        });
        let retained = serde_json::to_vec(&journal.report.items[0]).unwrap();
        journal.validate(&binding).unwrap();
        assert!(!journal.reconcile_generation(&current).unwrap());
        assert_eq!(
            retained,
            serde_json::to_vec(&journal.report.items[0]).unwrap()
        );
        assert!(!journal.report.publication_completed);
        let mut changed = current.clone();
        changed.items[0].revision = RevisionId::new(4);
        assert!(journal.reconcile_generation(&changed).is_err());
        assert_eq!(
            retained,
            serde_json::to_vec(&journal.report.items[0]).unwrap()
        );
    }
    #[test]
    fn observed_job_must_match_recorded_project_job_and_base_revision() {
        let root = tempfile::tempdir().unwrap();
        let item = generation(root.path()).items.remove(0);
        let mut observation = JobSnapshot {
            job_id: item.job_id.clone().unwrap(),
            project_id: item.project_id.clone().unwrap(),
            base_revision: item.revision,
            attempt_id: AttemptId::new("attempt").unwrap(),
            state: "running".into(),
            candidate_id: None,
            error: None,
        };
        validate_job_binding(&item, &observation).unwrap();
        observation.base_revision = RevisionId::new(4);
        assert!(validate_job_binding(&item, &observation).is_err());
        observation.base_revision = item.revision;
        observation.job_id = JobId::new("foreign-job").unwrap();
        assert!(validate_job_binding(&item, &observation).is_err());
    }
    #[test]
    fn journal_completion_requires_bound_publisher_acknowledgements() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let binding = "b".repeat(64);
        let mut journal = initial(root.path());
        journal.report.publication_completed = true;
        assert!(journal.validate(&binding).is_err());
        journal.report.publication_completed = false;
        let item = &mut journal.report.items[0];
        item.phase = PublicationPhase::Prepared;
        item.prepared = Some(prepared(item));
        journal.validate(&binding).unwrap();
        journal.report.items[0].prepared.as_mut().unwrap().requests[0].axis = Axis::Roll;
        assert!(journal.validate(&binding).is_err());
        let original = prepared(&journal.report.items[0]);
        journal.report.items[0].prepared = Some(original);
        let item = &mut journal.report.items[0];
        item.phase = PublicationPhase::Published;
        item.receipt = Some(PublishedNeutralBundle {
            destination: item.generation.destination.clone(),
            project: item.generation.project_id.clone().unwrap(),
            revision: item.generation.revision,
            axes: Axis::ALL.to_vec(),
            file_and_directory_synced: true,
            qualified: false,
        });
        journal.report.publication_completed = true;
        journal.validate(&binding).unwrap();
        journal.report.items[0]
            .receipt
            .as_mut()
            .unwrap()
            .file_and_directory_synced = false;
        assert!(journal.validate(&binding).is_err());
    }
    #[test]
    fn publication_store_reopens_without_effects_and_refuses_foreign_owner_and_live_lock() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let binding = "b".repeat(64);
        let mut store =
            local::Store::open(root.path(), &binding, Some(initial(root.path()))).unwrap();
        assert!(local::Store::open(root.path(), &binding, None).is_err());
        store.journal.report.items[0].phase = PublicationPhase::Preparing;
        store.save().unwrap();
        drop(store);
        let before = fs::read(root.path().join("record.json")).unwrap();
        assert!(local::Store::open(root.path(), &"c".repeat(64), None).is_err());
        let store = local::Store::open(root.path(), &binding, None).unwrap();
        assert_eq!(
            store.journal.report.items[0].phase,
            PublicationPhase::Preparing
        );
        drop(store);
        assert_eq!(before, fs::read(root.path().join("record.json")).unwrap());
        assert!(!root.path().join("output").exists());
    }
    #[test]
    fn publication_store_refuses_replaced_record_and_unsafe_storage_without_repair() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let binding = "b".repeat(64);
        let mut store =
            local::Store::open(root.path(), &binding, Some(initial(root.path()))).unwrap();
        let path = root.path().join("record.json");
        fs::write(&path, b"damaged original").unwrap();
        assert!(store.save().is_err());
        drop(store);
        assert_eq!(fs::read(&path).unwrap(), b"damaged original");
        assert!(local::Store::open(root.path(), &binding, None).is_err());
        fs::remove_file(&path).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, b"sentinel").unwrap();
        symlink(&outside, &path).unwrap();
        assert!(local::Store::open(root.path(), &binding, None).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
        fs::remove_file(path).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(local::Store::open(root.path(), &binding, None).is_err());
    }
    #[test]
    fn recovered_preparation_restores_original_ids_and_exact_repeat_does_not_mutate() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = initial(root.path());
        let item = &mut journal.report.items[0];
        let intent = prepared(item);
        item.phase = PublicationPhase::Preparing;
        item.error = Some("acknowledgement lost".into());
        assert!(attach_preparation(item, intent.clone()).unwrap());
        assert_eq!(item.phase, PublicationPhase::Prepared);
        assert!(item.error.is_none());
        let bytes = serde_json::to_vec(item).unwrap();
        assert!(!attach_preparation(item, intent.clone()).unwrap());
        assert_eq!(bytes, serde_json::to_vec(item).unwrap());
        let mut replacement = intent;
        replacement.requests[0].request = RequestId::new("replacement").unwrap();
        assert!(attach_preparation(item, replacement).is_err());
        assert_eq!(bytes, serde_json::to_vec(item).unwrap());
        journal.validate(&"b".repeat(64)).unwrap();
    }
    #[test]
    fn preparation_recovery_refuses_other_entry_and_phase_without_changing_metadata() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = initial(root.path());
        let item = &mut journal.report.items[0];
        let original = prepared(item);
        let bytes = serde_json::to_vec(item).unwrap();
        assert!(attach_preparation(item, original.clone()).is_err());
        assert_eq!(bytes, serde_json::to_vec(item).unwrap());
        item.phase = PublicationPhase::Preparing;
        for mutation in 0..3 {
            let mut foreign = original.clone();
            match mutation {
                0 => foreign.project = ProjectId::new("other").unwrap(),
                1 => foreign.revision = RevisionId::new(999),
                _ => foreign.destination = root.path().join("other"),
            };
            let before = serde_json::to_vec(item).unwrap();
            assert!(attach_preparation(item, foreign).is_err());
            assert_eq!(before, serde_json::to_vec(item).unwrap());
        }
    }
    #[test]
    fn pending_receipt_capture_refuses_foreign_owner_external_names_links_and_invalid_metadata() {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let binding = "b".repeat(64);
        let store = local::Store::open(root.path(), &binding, Some(initial(root.path()))).unwrap();
        let path = root.path().join(".pending-original");
        let mut candidate = store.journal.clone();
        let intent = prepared(&candidate.report.items[0]);
        candidate.report.items[0].phase = PublicationPhase::Preparing;
        attach_preparation(&mut candidate.report.items[0], intent).unwrap();
        fs::write(&path, serde_json::to_vec(&candidate).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(store.read_pending_preparation(&path).is_ok());
        assert!(store
            .read_pending_preparation(&root.path().join("record.json"))
            .is_err());
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join(".pending-original");
        fs::write(&external, serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert!(store.read_pending_preparation(&external).is_err());
        let alias = root.path().join(".pending-alias");
        fs::hard_link(&path, &alias).unwrap();
        assert!(store.read_pending_preparation(&path).is_err());
        fs::remove_file(alias).unwrap();
        candidate.credential_binding = "c".repeat(64);
        fs::write(&path, serde_json::to_vec(&candidate).unwrap()).unwrap();
        assert!(store.read_pending_preparation(&path).is_err());
        fs::write(&path, b"incomplete JSON").unwrap();
        assert!(store.read_pending_preparation(&path).is_err());
    }
    #[test]
    fn migrated_journal_pins_original_lineage_and_rejects_changed_parent() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = selected(dir.path());
        report.migration = Some(Box::new(LibraryBatchMigration {
            parent_batch_request_id: RequestId::new("original-batch").unwrap(),
            parent_report_sha256: "a".repeat(64),
        }));
        let mut journal = Journal::from_generation("b".repeat(64), report.clone(), true).unwrap();
        assert_eq!(journal.report.migration, report.migration);
        journal.reconcile_generation(&report).unwrap();
        report.migration.as_mut().unwrap().parent_report_sha256 = "c".repeat(64);
        assert!(journal.reconcile_generation(&report).is_err());
        journal
            .report
            .migration
            .as_mut()
            .unwrap()
            .parent_report_sha256 = "INVALID".into();
        assert!(journal.validate(&"b".repeat(64)).is_err());
    }
}
