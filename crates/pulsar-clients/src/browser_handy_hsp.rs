//! Browser HSP intent custody. The page receives a small observation projection;
//! only native workers read owner proof and submit the original Engine intents.
use crate::{
    browser_live::BrowserAnalysisFeedSnapshot,
    browser_player_control::Permit,
    handy_hsp_live::{self, ContinuityIntent, LiveIntent},
    handy_hsp_preparation::{self, PreparationIntent},
    session::{new_request_id, SessionClient},
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const RPC: Duration = Duration::from_secs(3);
const TICK: Duration = Duration::from_millis(50);
const CLEANUP_ATTEMPTS: u8 = 4;
const LIFECYCLE_HISTORY_LIMIT: usize = 8;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub device_qualification: HandyQualificationBinding,
    pub source_qualification: HandyHspSourceQualificationBinding,
    pub preparation_timeout_ms: u64,
    #[serde(default)]
    pub finite_drain_policy_sha256: Option<String>,
    #[serde(default)]
    pub continuity_duration_ms: Option<u32>,
}
impl Config {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_handy_hsp_preparation_selection(
            &self.engine_epoch,
            &self.device_session,
            &self.device_qualification,
            self.preparation_timeout_ms,
        )?;
        self.source_qualification.validate()?;
        if let Some(hash) = &self.finite_drain_policy_sha256 {
            validate_sha256(hash)?;
        }
        ensure!(
            self.continuity_duration_ms
                .is_none_or(|ms| (1..=600_000).contains(&ms)),
            "HSP continuity duration exceeds the browser lifecycle bound"
        );
        Ok(())
    }
}

#[derive(Clone)]
struct Inspected {
    source: BrowserAnalysisFeedSnapshot,
    device: Value,
    dossier: Value,
}
/// The first browser-owner cleanup request, independent of any earlier native
/// runtime Stop. Repeated cleanup cannot relabel player loss as a public action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CleanupTrigger {
    PublicStop,
    PublicCancel,
    PlayerRetired,
    OwnerShutdown,
    OperationUnconfirmed,
    OperationNotDispatched,
}
/// Only conclusively joined lifecycles enter this bounded public archive.
#[derive(Clone, Serialize)]
struct ArchivedLifecycle {
    player_id: Option<AttemptId>,
    prepare_request_id: Option<RequestId>,
    stream_request_id: Option<RequestId>,
    play_request_id: Option<RequestId>,
    enable_request_id: Option<RequestId>,
    stop_request_id: Option<RequestId>,
    cancel_request_id: Option<RequestId>,
    cleanup_trigger: Option<CleanupTrigger>,
    phase: &'static str,
}
#[derive(Default)]
struct State {
    player: Option<HandyPlayerIdentity>,
    cancelled: Option<Arc<AtomicBool>>,
    inspected: Option<Inspected>,
    sequence: u64,
    lifecycle_sequence: u64,
    previous_lifecycles: Vec<ArchivedLifecycle>,
    pending: Option<&'static str>,
    mutation_active: bool,
    read_pending: bool,
    prepare: Option<RequestId>,
    stream: Option<RequestId>,
    selection: Option<HandyHspLiveSelection>,
    play: Option<RequestId>,
    enable: Option<RequestId>,
    stop: Option<RequestId>,
    cancel: Option<RequestId>,
    prepare_dispatched: bool,
    bind_dispatched: bool,
    enable_dispatched: bool,
    prepare_unsent: bool,
    bind_unsent: bool,
    enable_unsent: bool,
    preparation: Option<HandyHspPreparationStatus>,
    live: Option<HandyHspLiveStatus>,
    continuity: Option<HandyHspContinuityStatus>,
    stop_latched: bool,
    cleanup_trigger: Option<CleanupTrigger>,
    cleanup_attempts: u8,
    cleanup_confirmed: bool,
    error: Option<&'static str>,
    worker_failed: bool,
}
type Shared = Arc<Mutex<State>>;
fn state(shared: &Shared) -> std::sync::MutexGuard<'_, State> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn equal<T: Serialize>(a: &T, b: &T) -> bool {
    serde_json::to_value(a)
        .ok()
        .zip(serde_json::to_value(b).ok())
        .is_some_and(|(a, b)| a == b)
}
/// Progress observations may change between Inspect and Bind; the selected
/// capture, inference, policy, media and timing identities may not.
fn same_scope(a: &BrowserAnalysisFeedSnapshot, b: &BrowserAnalysisFeedSnapshot) -> bool {
    a.binding == b.binding
        && a.browser_media == b.browser_media
        && a.browser_timing == b.browser_timing
        && a.engine_epoch == b.engine_epoch
        && a.capture == b.capture
        && a.inference_attempt_id == b.inference_attempt_id
        && equal(&a.inference_profile, &b.inference_profile)
        && a.estimated == b.estimated
        && a.maximum_analysis_age_ns == b.maximum_analysis_age_ns
}
fn finite_selection_matches(selected: &Option<String>, documented: &Option<String>) -> bool {
    selected
        .as_ref()
        .is_none_or(|selected| documented.as_ref() == Some(selected))
}
fn source_scope(
    source: &BrowserAnalysisFeedSnapshot,
    player: &HandyPlayerIdentity,
    project: &ProjectId,
    revision: RevisionId,
) -> Result<()> {
    source.browser_media.validate()?;
    source.browser_timing.validate()?;
    source.binding.policy.validate()?;
    let profile = source
        .inference_profile
        .as_ref()
        .context("actual CPU profile unavailable")?;
    profile.validate()?;
    ensure!(
        source.estimated
            && profile.provider == InferenceProvider::Cpu
            && source
                .inference_attempt_id
                .as_ref()
                .is_some_and(|id| id != &source.capture.epoch)
            && source.maximum_analysis_age_ns > 0
            && source.binding.player == *player
            && source.browser_timing.player == *player
            && source.binding.capture_epoch == source.capture.epoch
            && source.browser_timing.browser_media == source.browser_media
            && source.engine_epoch == source.browser_media.engine_epoch
            && source.browser_timing.engine_epoch == source.engine_epoch
            && source.browser_media.project_id == *project
            && source.browser_media.revision == revision
            && source.capture.project == *project
            && source.capture.revision == revision
            && source.capture.source == source.browser_media.selection.source_version
            && player.time_basis == LiveSourceTimeBasis::SourceMediaPresentation,
        "native HSP source selection is unavailable or changed"
    );
    ensure!(
        source
            .latest_observation
            .as_ref()
            .is_some_and(|observation| observation.feed_id == source.binding.feed_id
                && observation.generation == source.binding.generation
                && observation.role == source.binding.role
                && observation.presented_frames > 0)
            && source
                .measured_lead_us
                .is_some_and(|lead| lead >= source.binding.policy.minimum_lead_us
                    && lead <= source.binding.policy.maximum_lead_us),
        "current analysis lead unavailable"
    );
    Ok(())
}
fn bind_selection(
    config: &Config,
    prepare: RequestId,
    source: &BrowserAnalysisFeedSnapshot,
) -> Result<HandyHspLiveSelection> {
    let selection = HandyHspLiveSelection {
        preparation_request_id: prepare,
        capture_epoch: source.capture.epoch.clone(),
        inference_attempt_id: source
            .inference_attempt_id
            .clone()
            .context("CPU attempt unavailable")?,
        browser_timing: source.browser_timing.binding_id.clone(),
        source_qualification: config.source_qualification.clone(),
        finite_drain_policy_sha256: config.finite_drain_policy_sha256.clone(),
    };
    selection.validate()?;
    Ok(selection)
}

#[derive(Clone)]
struct Native {
    directory: PathBuf,
    project: ProjectId,
    revision: RevisionId,
    config: Config,
}
impl Native {
    fn inspect(
        &self,
        client: &mut SessionClient,
        source: BrowserAnalysisFeedSnapshot,
    ) -> Result<Inspected> {
        source_scope(
            &source,
            &source.binding.player,
            &self.project,
            self.revision,
        )?;
        ensure!(
            source.engine_epoch.as_str() == self.config.engine_epoch.as_str(),
            "selected source Engine epoch changed"
        );
        let sessions = crate::handy_session::list(client, &self.directory)?;
        ensure!(
            sessions.engine_epoch == self.config.engine_epoch,
            "selected Engine epoch changed"
        );
        let session = sessions
            .sessions
            .iter()
            .find(|s| s.device_session == self.config.device_session)
            .context("selected device session unavailable")?;
        ensure!(
            session.state == HandyConnectionState::Observed,
            "device session is not observed"
        );
        let device = crate::handy_qualification::status(
            client,
            &self.directory,
            self.config.device_qualification.dossier_id.clone(),
        )?;
        let dossier = crate::handy_hsp_source_qualification::status(
            client,
            &self.directory,
            self.project.clone(),
            self.config.source_qualification.dossier_id.clone(),
        )?;
        let subject = &dossier.manifest.subject;
        let unit = &device.manifest.subject;
        ensure!(
            unit.context.transport.as_str() == HANDY_HSP_QUALIFICATION_TRANSPORT
                && unit.device_id == session.registration.device_id
                && unit.registration_revision == session.registration.revision
                && unit.registration_binding_sha256 == session.registration.binding_sha256
                && session
                    .inspection
                    .as_ref()
                    .and_then(|i| i.identity.as_ref())
                    == Some(&unit.identity)
                && subject.device_qualification == self.config.device_qualification
                && subject.context == unit.context
                && subject.project_id == self.project
                && subject.revision == self.revision
                && subject.selection == source.browser_media.selection
                && subject.clock == source.browser_media.clock
                && source
                    .inference_profile
                    .as_ref()
                    .is_some_and(|profile| equal(profile, &subject.inference_profile))
                && subject.source_time_basis == source.binding.player.time_basis
                && subject.lookahead == source.binding.policy
                && subject.maximum_analysis_age_ns == source.maximum_analysis_age_ns
                && finite_selection_matches(
                    &self.config.finite_drain_policy_sha256,
                    &subject.finite_drain_policy_sha256
                ),
            "selected device or source dossier differs from current native scope"
        );
        // Dossier states remain observations. Production Engine admission owns
        // acceptance; this UI never creates an accepted/physical fixture verdict.
        Ok(Inspected {
            device: json!({"session_id":self.config.device_session,
                "qualification_id":device.dossier_id,"qualification_revision":bounded(device.revision),
                "state":device.state}),
            dossier: json!({"qualification_id":dossier.dossier_id,
                "qualification_revision":bounded(dossier.revision),"state":dossier.state,
                "origin":subject.origin,"environment_sha256":subject.browser_environment.sha256,
                "model_sha256":subject.inference_profile.model.sha256}),
            source,
        })
    }
    fn preparation_intent(&self) -> PreparationIntent {
        PreparationIntent {
            project: self.project.clone(),
            revision: self.revision,
            engine_epoch: self.config.engine_epoch.clone(),
            device_session: self.config.device_session.clone(),
            qualification: self.config.device_qualification.clone(),
            timeout_ms: self.config.preparation_timeout_ms,
        }
    }
    fn check_preparation(&self, report: &HandyHspPreparationStatus) -> Result<()> {
        report.validate()?;
        ensure!(
            report.project_id == self.project
                && report.history.as_ref().is_none_or(|h| h.engine_epoch
                    == self.config.engine_epoch
                    && h.device_session == self.config.device_session
                    && h.revision == self.revision
                    && h.qualification == self.config.device_qualification),
            "original HSP preparation subject changed"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Inspect,
    Prepare,
    Bind,
    Play,
    Enable,
    Status,
    Stop,
    Cancel,
}
impl Action {
    fn parse(path: &str, body: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Prepare {
            allow_synchronization: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Bind {
            acknowledge_current_environment: bool,
            allow_online_control: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Play {
            allow_play: bool,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Enable {
            allow_continuity: bool,
        }
        ensure!(body.len() <= 256, "HSP intent body exceeds bound");
        let action = match path {
            "/player/hsp-prepare" => {
                ensure!(
                    serde_json::from_slice::<Prepare>(body)?.allow_synchronization,
                    "explicit synchronization consent required"
                );
                Self::Prepare
            }
            "/player/hsp-bind" => {
                let v: Bind = serde_json::from_slice(body)?;
                ensure!(
                    v.acknowledge_current_environment && v.allow_online_control,
                    "explicit environment and online control consent required"
                );
                Self::Bind
            }
            "/player/hsp-play" => {
                ensure!(
                    serde_json::from_slice::<Play>(body)?.allow_play,
                    "explicit Play consent required"
                );
                Self::Play
            }
            "/player/hsp-continuity-enable" => {
                ensure!(
                    serde_json::from_slice::<Enable>(body)?.allow_continuity,
                    "explicit continuity consent required"
                );
                Self::Enable
            }
            "/player/hsp-inspect" => Self::Inspect,
            "/player/hsp-status"
            | "/player/hsp-preparation-status"
            | "/player/hsp-continuity-status" => Self::Status,
            "/player/hsp-stop" => Self::Stop,
            "/player/hsp-cancel" => Self::Cancel,
            _ => anyhow::bail!("unknown HSP route"),
        };
        if matches!(
            action,
            Self::Inspect | Self::Status | Self::Stop | Self::Cancel
        ) {
            ensure!(
                body.is_empty(),
                "HSP observation and cleanup require an empty body"
            );
        }
        Ok(action)
    }
    fn label(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Prepare => "prepare",
            Self::Bind => "bind",
            Self::Play => "play",
            Self::Enable => "continuity_enable",
            Self::Status => "status",
            Self::Stop => "stop",
            Self::Cancel => "cancel",
        }
    }
}
struct Job {
    action: Action,
    sequence: u64,
    source: Option<BrowserAnalysisFeedSnapshot>,
    deadline: Instant,
}
#[derive(Clone)]
struct Originals {
    prepare: Option<RequestId>,
    stream: Option<RequestId>,
    selection: Option<HandyHspLiveSelection>,
    play: Option<RequestId>,
    enable: Option<RequestId>,
    stop: Option<RequestId>,
    cancel: Option<RequestId>,
}
impl Originals {
    fn from_state(s: &State) -> Self {
        Self {
            prepare: s.prepare.clone(),
            stream: s.stream.clone(),
            selection: s.selection.clone(),
            play: s.play.clone(),
            enable: s.enable.clone(),
            stop: s.stop.clone(),
            cancel: s.cancel.clone(),
        }
    }
    fn for_observation(s: &State) -> Self {
        let mut ids = Self::from_state(s);
        // Until the mutation worker decides whether to enter execute, neither
        // status nor cleanup should query an original that cannot yet exist.
        if !s.prepare_dispatched || s.prepare_unsent {
            ids.prepare = None;
        }
        if !s.bind_dispatched || s.bind_unsent {
            ids.stream = None;
        }
        if !s.enable_dispatched || s.enable_unsent {
            ids.enable = None;
        }
        ids
    }
}
#[derive(Default)]
struct Reports {
    inspected: Option<Inspected>,
    preparation: Option<HandyHspPreparationStatus>,
    live: Option<HandyHspLiveStatus>,
    continuity: Option<HandyHspContinuityStatus>,
}
fn execute(
    native: &Native,
    client: &mut SessionClient,
    job: &Job,
    ids: &Originals,
) -> Result<Reports> {
    client.set_operation_deadline(Some(job.deadline));
    let mut reports = Reports::default();
    let directory = &native.directory;
    let project = native.project.clone();
    let revision = native.revision;
    match job.action {
        Action::Inspect => {
            reports.inspected =
                Some(native.inspect(client, job.source.clone().context("native source absent")?)?)
        }
        Action::Prepare => {
            reports.preparation = Some(handy_hsp_preparation::prepare(
                client,
                directory,
                ids.prepare.clone().context("original prepare absent")?,
                &native.preparation_intent(),
                true,
            )?)
        }
        Action::Bind => {
            reports.live = Some(handy_hsp_live::bind(
                client,
                directory,
                ids.stream.clone().context("original stream absent")?,
                &LiveIntent {
                    project,
                    revision,
                    selection: ids.selection.clone().context("original source absent")?,
                },
                true,
                true,
            )?)
        }
        Action::Play => {
            reports.live = Some(handy_hsp_live::play(
                client,
                directory,
                ids.play.clone().context("original Play absent")?,
                project,
                revision,
                ids.stream.clone().context("original stream absent")?,
            )?)
        }
        Action::Enable => {
            reports.continuity = Some(handy_hsp_live::enable_continuity(
                client,
                directory,
                ids.enable.clone().context("original continuity absent")?,
                &ContinuityIntent {
                    project,
                    revision,
                    stream_request_id: ids.stream.clone().context("original stream absent")?,
                    duration_ms: native
                        .config
                        .continuity_duration_ms
                        .context("continuity duration absent")?,
                },
                true,
            )?)
        }
        Action::Status => {
            if let Some(id) = &ids.stream {
                reports.live = Some(handy_hsp_live::status(
                    client,
                    directory,
                    project.clone(),
                    id.clone(),
                )?);
            }
            if let Some(id) = &ids.prepare {
                reports.preparation = Some(handy_hsp_preparation::status(
                    client,
                    directory,
                    project.clone(),
                    id.clone(),
                )?);
            }
            if let Some(id) = &ids.enable {
                reports.continuity = Some(handy_hsp_live::continuity_status(
                    client,
                    directory,
                    project,
                    id.clone(),
                )?);
            }
        }
        Action::Stop | Action::Cancel => anyhow::bail!("cleanup has a dedicated lane"),
    }
    Ok(reports)
}
fn retain(native: &Native, s: &mut State, reports: Reports) -> Result<()> {
    if let Some(report) = reports.preparation {
        native.check_preparation(&report)?;
        ensure!(
            s.prepare.as_ref() == Some(&report.preparation_request_id),
            "original preparation changed"
        );
        s.preparation = Some(report);
    }
    if let Some(report) = reports.live {
        report.validate()?;
        ensure!(
            report.project_id == native.project
                && s.stream.as_ref() == Some(&report.stream_request_id)
                && s.selection.as_ref() == Some(&report.selection)
                && report
                    .source_subject
                    .as_ref()
                    .is_none_or(|subject| subject.revision == native.revision)
                && report.native.as_ref().is_none_or(|n| n
                    .original_play_request_id
                    .as_ref()
                    .is_none_or(|id| s.play.as_ref() == Some(id)))
                && s.live
                    .as_ref()
                    .and_then(|r| r.origin.as_ref())
                    .is_none_or(|origin| report.origin.as_ref() == Some(origin)),
            "original HSP stream subject changed"
        );
        s.live = Some(report);
    }
    if let Some(report) = reports.continuity {
        report.validate()?;
        ensure!(
            report.project_id == native.project
                && s.stream.as_ref() == Some(&report.stream_request_id)
                && s.enable.as_ref() == Some(&report.enable_request_id)
                && native.config.continuity_duration_ms == Some(report.duration_ms)
                && report
                    .history
                    .as_ref()
                    .is_none_or(|h| s.prepare.as_ref() == Some(&h.preparation_request_id)),
            "original continuity subject changed"
        );
        s.continuity = Some(report);
    }
    if let Some(inspected) = reports.inspected {
        s.inspected = Some(inspected);
    }
    Ok(())
}
fn latch_stop(s: &mut State, trigger: CleanupTrigger) {
    if !s.stop_latched {
        s.cleanup_trigger = Some(trigger);
        match s.sequence.checked_add(1) {
            Some(next) => s.sequence = next,
            None => s.error = Some("owner_sequence_exhausted"),
        }
    }
    s.stop_latched = true;
    if s.stream.is_some() {
        s.stop.get_or_insert_with(new_request_id);
    }
    if s.prepare.is_some() {
        s.cancel.get_or_insert_with(new_request_id);
    }
    // Retirement remains latched after cleanup. Repeated observations must not
    // announce work that has already settled or exhausted its original budget.
    if !s.cleanup_confirmed && s.cleanup_attempts < CLEANUP_ATTEMPTS {
        s.pending = Some(if s.stream.is_some() { "stop" } else { "cancel" });
    }
}
fn preparation_ready(s: &State) -> bool {
    s.preparation.as_ref().is_some_and(|r| {
        r.native.as_ref().is_some_and(|n| {
            n.phase == HandyHspPreparationPhase::Ready
                && !n.worker_active
                && !n.metadata_pending
                && !n.cancel_requested
                && n.error.is_none()
        }) && r
            .history
            .as_ref()
            .is_some_and(|h| !h.resolved && !h.custody_transferred && h.linked_stream_id.is_none())
    })
}
fn stream_playable(s: &State) -> bool {
    s.live.as_ref().is_some_and(|r| {
        r.error.is_none()
            && r.native
                .as_ref()
                .is_some_and(|n| !n.retired && n.error.is_none())
            && r.stream
                .as_ref()
                .and_then(|r| r.native.as_ref())
                .is_some_and(|n| {
                    n.runtime_state == HandyHspRuntimeState::Prefilling
                        && n.post_setup_bound
                        && !n.retired
                        && !n.stop_pending
                        // Original intents may queue behind an Append or its
                        // metadata. The Engine serializes effects and checks
                        // the original expiry again before issuing Play.
                        && n.total_added >= 2
                })
    })
}
fn cleanup_complete(s: &State) -> bool {
    if s.mutation_active || s.worker_failed {
        return false;
    }
    let stream = s.stream.is_none()
        || s.bind_unsent
        || s.live.as_ref().is_some_and(|r| {
            r.native.as_ref().is_some_and(|n| n.retired)
                && r.stream
                    .as_ref()
                    .and_then(|r| r.native.as_ref())
                    .is_some_and(|n| {
                        !n.native_active
                            && n.current_effect.is_none()
                            && !n.metadata_pending
                            && !n.stop_pending
                            && matches!(
                                n.runtime_state,
                                HandyHspRuntimeState::StopAcknowledged
                                    | HandyHspRuntimeState::SetupCancelled
                                    | HandyHspRuntimeState::SetupUnsent
                            )
                    })
        });
    let preparation = s.prepare.is_none()
        || s.prepare_unsent
        || s.preparation.as_ref().is_some_and(|r| {
            r.native.as_ref().is_some_and(|n| {
                n.phase == HandyHspPreparationPhase::Retired
                    && !n.worker_active
                    && !n.metadata_pending
            }) && r
                .history
                .as_ref()
                .is_none_or(|h| h.resolved || h.custody_transferred)
        });
    let continuity = s.enable.is_none()
        || s.enable_unsent
        || s.continuity.as_ref().is_some_and(|r| {
            r.native
                .as_ref()
                .is_some_and(|n| n.retired && n.worker_joined && !n.worker_active)
        });
    stream && preparation && continuity
}

/// Called only by the mutation worker before entering execute. A transport or
/// execution error must never pass through this path or become known-unsent.
fn skipped_before_execute(s: &mut State, job: &Job) {
    if job.sequence < s.lifecycle_sequence {
        return;
    }
    match job.action {
        Action::Prepare if !s.prepare_dispatched => s.prepare_unsent = true,
        Action::Bind if !s.bind_dispatched => s.bind_unsent = true,
        Action::Enable if !s.enable_dispatched => s.enable_unsent = true,
        _ => {}
    }
    s.mutation_active = false;
    latch_stop(s, CleanupTrigger::OperationNotDispatched);
}

/// Explicit Inspect alone may open a fresh lifecycle. Old outcomes remain
/// inspectable as fixed public identities, and uncertain cleanup is never reset.
fn archive_and_reset(s: &mut State) -> Result<()> {
    ensure!(
        s.cleanup_confirmed
            && cleanup_complete(s)
            && !s.mutation_active
            && !s.read_pending
            && !s.worker_failed,
        "original HSP cleanup must be confirmed before inspecting a fresh lifecycle"
    );
    let has_original = s.prepare.is_some() || s.stream.is_some();
    ensure!(
        !has_original || s.previous_lifecycles.len() < LIFECYCLE_HISTORY_LIMIT,
        "HSP lifecycle history is full; create a new owner after cleanup"
    );
    let sequence = s
        .sequence
        .checked_add(1)
        .context("HSP owner sequence exhausted")?;
    let archived = has_original.then(|| ArchivedLifecycle {
        player_id: s.player.as_ref().map(|p| p.player_id.clone()),
        prepare_request_id: s.prepare.clone(),
        stream_request_id: s.stream.clone(),
        play_request_id: s.play.clone(),
        enable_request_id: s.enable.clone(),
        stop_request_id: s.stop.clone(),
        cancel_request_id: s.cancel.clone(),
        cleanup_trigger: s.cleanup_trigger,
        phase: "cleanup_confirmed",
    });
    let mut previous_lifecycles = std::mem::take(&mut s.previous_lifecycles);
    if let Some(archived) = archived {
        previous_lifecycles.push(archived);
    }
    *s = State {
        sequence,
        lifecycle_sequence: sequence,
        previous_lifecycles,
        ..State::default()
    };
    Ok(())
}

fn work(
    native: Native,
    mut client: SessionClient,
    shared: Shared,
    shutdown: Arc<AtomicBool>,
    jobs: mpsc::Receiver<Job>,
    mutation: bool,
) {
    while let Ok(job) = jobs.recv() {
        let ids = {
            let mut s = state(&shared);
            if shutdown.load(Ordering::Acquire)
                || (mutation
                    && (s.stop_latched
                        || s.cancelled
                            .as_ref()
                            .is_some_and(|v| v.load(Ordering::Acquire))))
                || (job.action == Action::Inspect
                    && s.cancelled
                        .as_ref()
                        .is_some_and(|v| v.load(Ordering::Acquire)))
            {
                if mutation {
                    skipped_before_execute(&mut s, &job);
                } else {
                    s.read_pending = false;
                }
                continue;
            }
            if mutation {
                match job.action {
                    Action::Prepare => s.prepare_dispatched = true,
                    Action::Bind => s.bind_dispatched = true,
                    Action::Enable => s.enable_dispatched = true,
                    _ => {}
                }
                Originals::from_state(&s)
            } else {
                Originals::for_observation(&s)
            }
        };
        let result = execute(&native, &mut client, &job, &ids);
        finish(&native, &mut state(&shared), &job, result, mutation);
    }
}
fn finish(native: &Native, s: &mut State, job: &Job, result: Result<Reports>, mutation: bool) {
    // A delayed prior-lifecycle completion must not even clear the new lane's
    // pending flag, before the ordinary operation sequence fence below.
    if job.sequence < s.lifecycle_sequence {
        return;
    }
    if mutation {
        s.mutation_active = false;
    } else {
        s.read_pending = false;
    }
    if s.sequence != job.sequence || (s.stop_latched && job.action != Action::Status) {
        return;
    }
    if job.action == Action::Inspect
        && s.cancelled
            .as_ref()
            .is_some_and(|v| v.load(Ordering::Acquire))
    {
        return;
    }
    if s.pending == Some(job.action.label()) {
        s.pending = None;
    }
    match result.and_then(|r| retain(native, s, r)) {
        Ok(()) => {
            if job.action == Action::Inspect || s.error == Some("original_status_unconfirmed") {
                s.error = None;
            }
        }
        Err(_) => {
            s.error = Some(match job.action {
                Action::Inspect => "selection_inspection_unavailable",
                Action::Status => "original_status_unconfirmed",
                _ => "original_operation_unconfirmed",
            });
            if mutation {
                latch_stop(s, CleanupTrigger::OperationUnconfirmed);
            }
        }
    }
}

/// Cleanup uses original IDs after permit retirement and independently of both
/// queues. Each pass has one total RPC budget and never holds the state lock.
fn cleanup(native: Native, mut client: SessionClient, shared: Shared, shutdown: Arc<AtomicBool>) {
    let mut shutdown_deadline = None;
    loop {
        if shutdown.load(Ordering::Acquire) && shutdown_deadline.is_none() {
            shutdown_deadline = Some(Instant::now() + RPC + RPC + TICK);
        }
        let work = {
            let mut s = state(&shared);
            if shutdown.load(Ordering::Acquire) {
                latch_stop(&mut s, CleanupTrigger::OwnerShutdown);
            } else if s
                .cancelled
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Acquire))
            {
                latch_stop(&mut s, CleanupTrigger::PlayerRetired);
            }
            if s.stop_latched && !s.cleanup_confirmed && s.cleanup_attempts < CLEANUP_ATTEMPTS {
                Some(Originals::for_observation(&s))
            } else {
                None
            }
        };
        if let Some(ids) = work {
            client.set_operation_deadline(Some(
                shutdown_deadline.map_or(Instant::now() + RPC, |end| end.min(Instant::now() + RPC)),
            ));
            let mut reports = Reports::default();
            let mut failed = false;
            if let (Some(id), Some(stream)) = (&ids.stop, &ids.stream) {
                match handy_hsp_live::stop(
                    &mut client,
                    &native.directory,
                    id.clone(),
                    native.project.clone(),
                    stream.clone(),
                ) {
                    Ok(r) => reports.live = Some(r),
                    Err(_) => failed = true,
                }
            }
            if let (Some(id), Some(prepare)) = (&ids.cancel, &ids.prepare) {
                match handy_hsp_preparation::cancel(
                    &mut client,
                    &native.directory,
                    id.clone(),
                    native.project.clone(),
                    prepare.clone(),
                ) {
                    Ok(r) => reports.preparation = Some(r),
                    Err(_) => failed = true,
                }
            }
            if let Some(id) = &ids.enable {
                match handy_hsp_live::continuity_status(
                    &mut client,
                    &native.directory,
                    native.project.clone(),
                    id.clone(),
                ) {
                    Ok(r) => reports.continuity = Some(r),
                    Err(_) => failed = true,
                }
            }
            let mut s = state(&shared);
            if retain(&native, &mut s, reports).is_err() {
                failed = true;
            }
            if !s.mutation_active {
                s.cleanup_attempts = s.cleanup_attempts.saturating_add(1);
            }
            s.cleanup_confirmed = !failed && cleanup_complete(&s);
            if s.cleanup_confirmed {
                s.pending = None;
            } else if s.cleanup_attempts >= CLEANUP_ATTEMPTS {
                s.pending = None;
                s.error = Some("original_cleanup_unconfirmed");
            }
        }
        if shutdown_deadline.is_some_and(|end| Instant::now() >= end)
            || (shutdown.load(Ordering::Acquire) && {
                let s = state(&shared);
                s.cleanup_confirmed || s.cleanup_attempts >= CLEANUP_ATTEMPTS
            })
        {
            break;
        }
        thread::sleep(TICK);
    }
}

pub(crate) struct Hsp {
    native: Native,
    shared: Shared,
    mutation: Option<mpsc::SyncSender<Job>>,
    reads: Option<mpsc::SyncSender<Job>>,
    shutdown: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Hsp {
    pub(crate) fn new(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        config: Config,
    ) -> Result<Self> {
        config.validate()?;
        let native = Native {
            directory: client.output_state_directory()?,
            project,
            revision,
            config,
        };
        let shared = Arc::new(Mutex::new(State::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (mutation, mutations) = mpsc::sync_channel(1);
        let (reads, observations) = mpsc::sync_channel(1);
        let mut owner = Self {
            native,
            shared,
            mutation: Some(mutation),
            reads: Some(reads),
            shutdown,
            threads: Vec::new(),
        };
        for (name, jobs, mutating) in [
            ("pulsar-hsp-intents", mutations, true),
            ("pulsar-hsp-status", observations, false),
        ] {
            let native = owner.native.clone();
            let shared = owner.shared.clone();
            let shutdown = owner.shutdown.clone();
            let client = client.fork_native();
            match thread::Builder::new()
                .name(name.into())
                .spawn(move || work(native, client, shared, shutdown, jobs, mutating))
            {
                Ok(thread) => owner.threads.push(thread),
                Err(error) => {
                    let _ = owner.shutdown();
                    return Err(error.into());
                }
            }
        }
        let native = owner.native.clone();
        let shared = owner.shared.clone();
        let shutdown = owner.shutdown.clone();
        let client = client.fork_native();
        match thread::Builder::new()
            .name("pulsar-hsp-cleanup".into())
            .spawn(move || cleanup(native, client, shared, shutdown))
        {
            Ok(thread) => owner.threads.push(thread),
            Err(error) => {
                let _ = owner.shutdown();
                return Err(error.into());
            }
        }
        Ok(owner)
    }
    pub(crate) fn handle(
        &mut self,
        path: &str,
        permit: &Permit,
        source: Option<BrowserAnalysisFeedSnapshot>,
        body: &[u8],
    ) -> Result<Value> {
        let action = Action::parse(path, body)
            .map_err(|_| anyhow::anyhow!("invalid HSP intent or missing explicit consent"))?;
        ensure!(
            !self.shutdown.load(Ordering::Acquire),
            "HSP owner is shutting down"
        );
        {
            let mut s = state(&self.shared);
            let player = permit
                .identity
                .as_ref()
                .context("HSP requires an original player identity")?;
            if action == Action::Inspect
                && (s.stop_latched || s.player.as_ref().is_some_and(|selected| selected != player))
            {
                ensure!(
                    permit.current(),
                    "fresh HSP inspection requires a current player"
                );
                if self.threads.iter().any(thread::JoinHandle::is_finished) {
                    s.worker_failed = true;
                }
                source_scope(
                    source
                        .as_ref()
                        .context("current native analysis snapshot unavailable")?,
                    player,
                    &self.native.project,
                    self.native.revision,
                )
                .map_err(|_| anyhow::anyhow!("native HSP analysis is unavailable or changed"))?;
                archive_and_reset(&mut s)?;
            }
            ensure!(
                s.player.as_ref().is_none_or(|selected| selected == player),
                "HSP original player differs"
            );
            if matches!(action, Action::Stop | Action::Cancel) {
                s.cleanup_attempts = 0;
                s.cleanup_confirmed = false;
                latch_stop(
                    &mut s,
                    if action == Action::Stop {
                        CleanupTrigger::PublicStop
                    } else {
                        CleanupTrigger::PublicCancel
                    },
                );
                drop(s);
                return Ok(self.view());
            }
            if action != Action::Status {
                ensure!(
                    permit.current() && !s.stop_latched,
                    "HSP player is retired; only original status and cleanup remain available"
                );
            }
            if action == Action::Status && s.read_pending {
                drop(s);
                return Ok(self.view());
            }
            if matches!(action, Action::Inspect | Action::Bind) {
                let source = source
                    .as_ref()
                    .context("current native analysis snapshot unavailable")?;
                source_scope(source, player, &self.native.project, self.native.revision).map_err(
                    |_| anyhow::anyhow!("native HSP analysis is unavailable or changed"),
                )?;
                if action == Action::Bind {
                    ensure!(
                        s.inspected
                            .as_ref()
                            .is_some_and(|i| same_scope(&i.source, source)),
                        "HSP source changed since inspection; inspect again"
                    );
                }
            }
            if s.player.is_none() {
                s.player = Some(player.clone());
                s.cancelled = Some(permit.cancelled.clone());
            }
            // Status can precede Inspect and carries a retained-page permit.
            // Before authorizing any fresh work, install the exact live player
            // cancellation flag captured by this explicit inspection.
            if action == Action::Inspect {
                s.cancelled = Some(permit.cancelled.clone());
            }
            let mutation = !matches!(action, Action::Inspect | Action::Status);
            if mutation {
                ensure!(!s.mutation_active, "original HSP operation is pending");
                let next_sequence = s
                    .sequence
                    .checked_add(1)
                    .context("HSP owner sequence exhausted")?;
                match action {
                    Action::Prepare => {
                        ensure!(
                            s.inspected.is_some() && s.prepare.is_none(),
                            "inspect current source before creating one original preparation"
                        );
                        s.prepare = Some(new_request_id());
                    }
                    Action::Bind => {
                        ensure!(
                            preparation_ready(&s) && s.stream.is_none(),
                            "original HSP preparation is not ready or is consumed"
                        );
                        let selection = bind_selection(
                            &self.native.config,
                            s.prepare.clone().context("original preparation absent")?,
                            source.as_ref().context("native source absent")?,
                        )?;
                        s.selection = Some(selection);
                        s.stream = Some(new_request_id());
                    }
                    Action::Play => {
                        ensure!(
                            stream_playable(&s) && s.play.is_none(),
                            "original stream cannot accept a new Play intent"
                        );
                        ensure!(
                            s.enable.is_none()
                                || s.continuity.as_ref().is_some_and(|r| r.error.is_none()
                                    && r.native.as_ref().is_some_and(|n| n.current && !n.retired)),
                            "requested continuity is unconfirmed"
                        );
                        s.play = Some(new_request_id());
                    }
                    Action::Enable => {
                        ensure!(
                            self.native.config.continuity_duration_ms.is_some()
                                && s.enable.is_none()
                                && (stream_playable(&s)
                                    || s.live
                                        .as_ref()
                                        .and_then(|r| r.stream.as_ref())
                                        .and_then(|r| r.native.as_ref())
                                        .is_some_and(|n| n.runtime_state
                                            == HandyHspRuntimeState::Playing
                                            && !n.retired)),
                            "original stream cannot accept continuity"
                        );
                        s.enable = Some(new_request_id());
                    }
                    _ => unreachable!(),
                }
                s.sequence = next_sequence;
                s.mutation_active = true;
                s.pending = Some(action.label());
            } else {
                ensure!(!s.read_pending, "original HSP inspection is pending");
                if action == Action::Inspect {
                    ensure!(
                        s.stream.is_none() && !s.mutation_active,
                        "original stream source selection is immutable"
                    );
                    s.sequence = s
                        .sequence
                        .checked_add(1)
                        .context("HSP owner sequence exhausted")?;
                    s.inspected = None;
                    s.pending = Some("inspect");
                }
                s.read_pending = true;
            }
            let job = Job {
                action,
                sequence: s.sequence,
                source,
                deadline: Instant::now() + RPC,
            };
            let sender = if mutation {
                &self.mutation
            } else {
                &self.reads
            };
            if sender
                .as_ref()
                .context("HSP lane unavailable")?
                .try_send(job)
                .is_err()
            {
                if mutation {
                    s.mutation_active = false;
                    s.error = Some("original_operation_not_dispatched");
                    latch_stop(&mut s, CleanupTrigger::OperationNotDispatched);
                } else {
                    s.read_pending = false;
                    if s.pending == Some(action.label()) {
                        s.pending = None;
                    }
                    s.error = Some("observation_lane_unavailable");
                }
            }
        }
        Ok(self.view())
    }
    pub(crate) fn retire(&self, identity: &HandyPlayerIdentity) {
        let mut s = state(&self.shared);
        if s.player.as_ref() == Some(identity) {
            latch_stop(&mut s, CleanupTrigger::PlayerRetired);
        }
    }
    pub(crate) fn view(&self) -> Value {
        view(&self.native.config, &state(&self.shared))
    }
    pub(crate) fn shutdown(&mut self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        {
            let mut s = state(&self.shared);
            latch_stop(&mut s, CleanupTrigger::OwnerShutdown);
            s.cleanup_attempts = 0;
        }
        self.mutation.take();
        self.reads.take();
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                state(&self.shared).worker_failed = true;
            }
        }
        let s = state(&self.shared);
        ensure!(!s.worker_failed && cleanup_complete(&s),"HSP original cleanup is unconfirmed; native expiry remains fallback and physical stopping is unconfirmed");
        Ok(())
    }
}
impl Drop for Hsp {
    fn drop(&mut self) {
        if !self.threads.is_empty() {
            let _ = self.shutdown();
        }
    }
}
fn bounded(n: u64) -> u64 {
    n.min((1u64 << 53) - 1)
}
fn view(config: &Config, s: &State) -> Value {
    let native = s
        .live
        .as_ref()
        .and_then(|r| r.stream.as_ref())
        .and_then(|r| r.native.as_ref());
    let preparation=s.prepare.as_ref().map(|id| {
        let report=s.preparation.as_ref(); let n=report.and_then(|r|r.native.as_ref());
        json!({"request_id":id,"phase":if s.prepare_unsent {json!("not_dispatched")} else {n.map(|n|serde_json::to_value(n.phase).expect("enum")).unwrap_or(json!(if report.is_some() {"historical_only"} else {"pending"}))},
            "worker_active":n.is_some_and(|n|n.worker_active),"metadata_pending":!s.prepare_unsent && n.is_none_or(|n|n.metadata_pending),
            "history_state":report.and_then(|r|r.history.as_ref()).map(|h|h.state)})
    });
    let stream=s.stream.as_ref().map(|id|json!({"request_id":id,
        "phase":if s.bind_unsent {json!("not_dispatched")} else {native.map(|n|serde_json::to_value(n.runtime_state).expect("enum")).unwrap_or(json!(if s.live.is_some() {"historical_only"} else {"binding"}))},
        "delivered_samples":bounded(s.live.as_ref().and_then(|r|r.native.as_ref()).map_or(0,|n|n.delivered_samples)),
        "active_source_generation":s.live.as_ref().and_then(|r|r.native.as_ref()).and_then(|n|n.active_source.as_ref()).map(|a|bounded(a.generation)),
        "metadata_pending":!s.bind_unsent && native.is_none_or(|n|n.metadata_pending),"stop_pending":(s.stop_latched && !s.cleanup_confirmed) || native.is_some_and(|n|n.stop_pending)}));
    let continuity=s.enable.as_ref().map(|id| {let n=s.continuity.as_ref().and_then(|r|r.native.as_ref()); json!({"request_id":id,
        "phase":if s.enable_unsent {"not_dispatched"} else if s.continuity.as_ref().is_some_and(|r|r.error.is_some()) {"refused"} else if n.is_some_and(|n|n.retired) {"retired"} else if n.is_some_and(|n|n.current) {"observing"} else {"unconfirmed"},
        "current":n.is_some_and(|n|n.current),"generation":n.map(|n|bounded(n.generation)),
        "worker_active":n.is_some_and(|n|n.worker_active),"worker_joined":n.is_some_and(|n|n.worker_joined)})});
    let cleanup_observed = s.cleanup_confirmed && cleanup_complete(s);
    let error = s.error.or_else(|| {
        if s.live.as_ref().is_some_and(|r| {
            r.error.is_some() || r.native.as_ref().is_some_and(|n| n.error.is_some())
        }) || s
            .preparation
            .as_ref()
            .and_then(|r| r.native.as_ref())
            .is_some_and(|n| {
                n.error
                    .as_ref()
                    .is_some_and(|error| !(error.code == ErrorCode::Cancelled && cleanup_observed))
            })
            || s.continuity.as_ref().is_some_and(|r| {
                r.error.is_some()
                    || r.native.as_ref().is_some_and(|n| {
                        n.error.as_ref().is_some_and(|error| {
                            // Retiring the preparation cancels its clock owner.
                            // Only complete, joined cleanup establishes that this
                            // cancellation is an observed terminal outcome.
                            !(error.code == ErrorCode::Cancelled && cleanup_observed)
                        })
                    })
            })
        {
            Some("native_refusal_or_uncertainty")
        } else {
            None
        }
    });
    let usable = !s.stop_latched && !s.mutation_active && error.is_none();
    let phase = if s.stop_latched {
        if s.cleanup_confirmed {
            "cleanup_observed"
        } else {
            "stopping"
        }
    } else if s.pending.is_some() {
        s.pending.unwrap()
    } else if error.is_some() {
        "unconfirmed"
    } else if let Some(n) = native {
        match n.runtime_state {
            HandyHspRuntimeState::Prefilling => "prefilling",
            HandyHspRuntimeState::Playing => {
                if n.finite_drain.is_some() {
                    "finishing"
                } else {
                    "playing"
                }
            }
            HandyHspRuntimeState::StopAcknowledged => "stop_acknowledged",
            HandyHspRuntimeState::StopUnknown => "stop_unknown",
            HandyHspRuntimeState::StopRequired
            | HandyHspRuntimeState::StopInFlight
            | HandyHspRuntimeState::StopUnclaimed => "stopping",
            _ => "binding",
        }
    } else if preparation_ready(s) {
        "ready_to_bind"
    } else if s.prepare.is_some() {
        "preparing"
    } else if s.inspected.is_some() {
        "inspected"
    } else {
        "not_inspected"
    };
    json!({"configured":true,"phase":phase,"selection_checked":s.inspected.is_some() && !s.stop_latched,
        "can_prepare":usable && s.inspected.is_some() && s.prepare.is_none(),
        "can_bind":usable && s.inspected.is_some() && s.stream.is_none() && preparation_ready(s),
        "can_play":usable && s.play.is_none() && stream_playable(s) && (s.enable.is_none() || s.continuity.as_ref().and_then(|r|r.native.as_ref()).is_some_and(|n|n.current && !n.retired)),
        "can_enable_continuity":usable && config.continuity_duration_ms.is_some() && s.enable.is_none() && (stream_playable(s) || native.is_some_and(|n|n.runtime_state == HandyHspRuntimeState::Playing && !n.retired)),
        "can_stop":s.stream.is_some(),"can_cancel":s.prepare.is_some(),"continuity_duration_ms":config.continuity_duration_ms,
        "pending_action":s.pending,"device":s.inspected.as_ref().map(|i|i.device.clone()).unwrap_or(json!({"session_id":config.device_session,"qualification_id":config.device_qualification.dossier_id,"qualification_revision":bounded(config.device_qualification.revision)})),
        "source":s.inspected.as_ref().map(|i|i.dossier.clone()).unwrap_or(json!({"qualification_id":config.source_qualification.dossier_id,"qualification_revision":bounded(config.source_qualification.revision)})),
        "preparation":preparation,"stream":stream,"continuity":continuity,"error":error,
        "cleanup":{"trigger":s.cleanup_trigger,"stop_request_id":s.stop,"cancel_request_id":s.cancel},
        "previous_lifecycles":s.previous_lifecycles})
}

#[cfg(test)]
#[path = "browser_handy_hsp_tests.rs"]
mod tests;
