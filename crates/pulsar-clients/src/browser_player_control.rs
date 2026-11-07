//! Process-local browser capabilities and an optional native stop guard.
//! No Engine credential or native timing value is serialized into a page.
use crate::session::{new_request_id, SessionClient};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
#[path = "browser_player_timing.rs"]
mod timing;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GuardConfig {
    pub selection: HandyContinuousTimingSelection,
    pub policy: HandyPlayerGuardPolicy,
}
impl GuardConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        self.selection.validate()?;
        self.policy.validate()?;
        Ok(())
    }
}
#[derive(Clone, Default)]
pub(super) struct Scope {
    pub launch: String,
    pub page: String,
    pub page_capability: String,
    pub player: Option<String>,
    pub source: Option<String>,
    pub generation: Option<u64>,
    pub player_capability: Option<String>,
}
#[derive(Clone)]
pub(super) struct Permit {
    pub identity: Option<HandyPlayerIdentity>,
    pub cancelled: Arc<AtomicBool>,
    page: String,
}
impl Permit {
    #[cfg(test)]
    pub(crate) fn for_analysis_test(identity: HandyPlayerIdentity) -> Self {
        Self {
            page: identity.page_id.to_string(),
            identity: Some(identity),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn current(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
    }
}
struct Page {
    capability: String,
    cancelled: Arc<AtomicBool>,
}
struct Player {
    identity: HandyPlayerIdentity,
    capability: String,
    cancelled: Arc<AtomicBool>,
    active: bool,
    heartbeat: Instant,
    binding: Option<HandyPlayerGuardReport>,
    binding_pending: bool,
    browser_media: Option<BrowserMediaBinding>,
    sequence: u64,
    fast_pending: bool,
    failed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    #[serde(default)]
    media_selection_id: Option<AttemptId>,
    source_id: AttemptId,
    generation: u64,
    time_basis: LiveSourceTimeBasis,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observe {
    sequence: u64,
    challenge_id: ArtifactId,
    sample: HandyPlayerSample,
}
fn id() -> AttemptId {
    AttemptId::new(uuid::Uuid::new_v4().to_string()).expect("UUID")
}
fn capability() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
const FAST_TIMEOUT: Duration = Duration::from_millis(200);
const BIND_TIMEOUT: Duration = Duration::from_secs(3);
const LOCAL_HEARTBEAT: Duration = Duration::from_secs(3);

const DIAGNOSTIC_LIMIT: usize = 16;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum GuardPhase {
    BindingPending,
    Bound,
    ReleasePending,
    Released,
    NotDispatched,
    Uncertain,
}
#[derive(Clone, Serialize)]
struct GuardDiagnostic {
    binding_request_id: RequestId,
    binding_id: Option<ArtifactId>,
    player: HandyPlayerIdentity,
    phase: GuardPhase,
    /// Static classifications only. Never retain remote errors or request credentials.
    issue: Option<&'static str>,
}
struct GuardRecord {
    diagnostic: GuardDiagnostic,
    report: Option<HandyPlayerGuardReport>,
}
#[derive(Default)]
struct Diagnostics {
    records: Vec<GuardRecord>,
    overflow: bool,
    worker_failed: bool,
}
type SharedDiagnostics = Arc<Mutex<Diagnostics>>;
fn records(value: &SharedDiagnostics) -> std::sync::MutexGuard<'_, Diagnostics> {
    value
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
impl Diagnostics {
    fn summary(&self) -> String {
        // IDs are public original-operation subjects. No bearer/proof, raw errors or
        // native report timestamps are retained in these process-local diagnostics.
        let mut values: Vec<_> = self.records.iter().map(|r| &r.diagnostic).collect();
        values.sort_by_key(|record| {
            (
                record.issue.is_none(),
                matches!(
                    record.phase,
                    GuardPhase::Released | GuardPhase::NotDispatched
                ),
            )
        });
        format!("Handy stop guard cleanup is unconfirmed; process-local original operations {}; overflow={}; worker_failed={}; native expiry remains fallback, physical stopping is unconfirmed",
            serde_json::to_string(&values).expect("diagnostic fields serialize"), self.overflow,self.worker_failed)
    }
    fn failure(&self) -> Option<String> {
        (self.overflow
            || self.worker_failed
            || self.records.iter().any(|r| {
                r.diagnostic.issue.is_some() || r.diagnostic.phase == GuardPhase::Uncertain
            }))
        .then(|| self.summary())
    }
    fn unfinished(&self) -> bool {
        self.records.iter().any(|r| {
            !matches!(
                r.diagnostic.phase,
                GuardPhase::Released | GuardPhase::NotDispatched
            )
        })
    }
}
fn record_issue(diagnostics: &SharedDiagnostics, request: &RequestId, issue: &'static str) {
    let mut state = records(diagnostics);
    if let Some(record) = state
        .records
        .iter_mut()
        .find(|r| &r.diagnostic.binding_request_id == request)
    {
        record.diagnostic.issue.get_or_insert(issue);
    } else {
        state.overflow = true;
    }
}
fn mark_uncertain(diagnostics: &SharedDiagnostics, request: &RequestId, issue: &'static str) {
    let mut state = records(diagnostics);
    if let Some(record) = state
        .records
        .iter_mut()
        .find(|r| &r.diagnostic.binding_request_id == request)
    {
        record.diagnostic.phase = GuardPhase::Uncertain;
        record.diagnostic.issue.get_or_insert(issue);
    } else {
        state.overflow = true;
    }
}
fn sanitized_error(diagnostics: &SharedDiagnostics) -> anyhow::Error {
    anyhow::anyhow!(records(diagnostics).summary())
}
struct BindJob {
    browser_media: Option<BrowserMediaBinding>,
    request_id: RequestId,
    identity: HandyPlayerIdentity,
    cancelled: Arc<AtomicBool>,
}
struct FastJob {
    ticket: u64,
    identity: HandyPlayerIdentity,
    command: Command,
    cancelled: Arc<AtomicBool>,
}
struct Event {
    ticket: Option<u64>,
    identity: HandyPlayerIdentity,
    binding_id: Option<ArtifactId>,
    observation: Option<(u64, HandyPlayerSample)>,
    result: Result<pulsar_protocol::ResponseBody>,
}
struct ReleaseJob {
    original: HandyPlayerGuardReport,
}
struct Native {
    binds: Option<mpsc::SyncSender<BindJob>>,
    fast: Option<mpsc::SyncSender<FastJob>>,
    releases: Option<mpsc::SyncSender<ReleaseJob>>,
    events: mpsc::Receiver<Event>,
    threads: Vec<thread::JoinHandle<()>>,
    proof: String,
    diagnostics: SharedDiagnostics,
    cleanup_client: SessionClient,
    project: ProjectId,
}
fn release_command(report: &HandyPlayerGuardReport, proof: &str) -> Command {
    Command::ReleaseHandyPlayerGuard {
        binding_id: report.binding.binding_id.clone(),
        player: report.binding.player.clone(),
        owner_proof: proof.into(),
    }
}
/// Reserve exactly one release attempt. Repeated local stop/drop never pretends
/// an uncertain release was successful and never silently retries it.
fn reserve_release(
    diagnostics: &SharedDiagnostics,
    report: &HandyPlayerGuardReport,
) -> Option<ReleaseJob> {
    let mut state = records(diagnostics);
    let Some(record) = state.records.iter_mut().find(|r| {
        r.diagnostic.binding_request_id == report.binding.request_id
            && r.diagnostic.player == report.binding.player
    }) else {
        state.overflow = true;
        return None;
    };
    if record.diagnostic.binding_id.as_ref() != Some(&report.binding.binding_id) {
        record
            .diagnostic
            .issue
            .get_or_insert("release_subject_mismatch");
        return None;
    }
    if record.diagnostic.phase != GuardPhase::Bound {
        return None;
    }
    record.diagnostic.phase = GuardPhase::ReleasePending;
    Some(ReleaseJob {
        original: report.clone(),
    })
}
fn release_once(
    client: &SessionClient,
    project: &ProjectId,
    proof: &str,
    diagnostics: &SharedDiagnostics,
    job: ReleaseJob,
) {
    let original = &job.original;
    let result = client
        .browser_call_until(
            new_request_id(),
            release_command(original, proof),
            project.clone(),
            None,
            Instant::now() + FAST_TIMEOUT,
        )
        .and_then(|reply| {
            let pulsar_protocol::ResponseBody::HandyPlayerGuard(report) = reply else {
                anyhow::bail!("release reply kind differs")
            };
            report.validate()?;
            ensure!(
                !report.current
                    && report.binding.controller.project_id == *project
                    && serde_json::to_value(&report.binding)?
                        == serde_json::to_value(&original.binding)?
                    && report.bound_at_boottime_ns == original.bound_at_boottime_ns,
                "release reply does not confirm the exact retired binding"
            );
            Ok(())
        });
    if result.is_ok() {
        if let Some(record) = records(diagnostics)
            .records
            .iter_mut()
            .find(|r| r.diagnostic.binding_request_id == original.binding.request_id)
        {
            record.diagnostic.phase = GuardPhase::Released;
        }
    } else {
        mark_uncertain(
            diagnostics,
            &original.binding.request_id,
            "release_reply_unconfirmed",
        );
    }
}
fn validate_binding_reply(
    reply: pulsar_protocol::ResponseBody,
    request_id: &RequestId,
    identity: &HandyPlayerIdentity,
    config: &GuardConfig,
    browser_media: &Option<BrowserMediaBinding>,
    project: &ProjectId,
    revision: RevisionId,
) -> Result<pulsar_protocol::ResponseBody> {
    let pulsar_protocol::ResponseBody::HandyPlayerGuard(report) = &reply else {
        anyhow::bail!("unexpected guard binding reply")
    };
    report.validate()?;
    ensure!(
        report.binding.request_id == *request_id
            && report.binding.player == *identity
            && report.binding.selection == config.selection
            && report.binding.policy == config.policy
            && report.binding.browser_media == *browser_media
            && report.binding.controller.project_id == *project
            && report.binding.controller.revision == revision,
        "guard binding reply changed captured subject"
    );
    Ok(reply)
}
impl Native {
    fn new(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        config: GuardConfig,
    ) -> Result<Self> {
        config.validate()?;
        let proof = crate::project_package::read_local_owner_proof(
            &client.output_state_directory()?,
            "handy-owner.token",
        )?;
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default()));
        let (binds, bind_rx) = mpsc::sync_channel::<BindJob>(1);
        let (fast, fast_rx) = mpsc::sync_channel::<FastJob>(8);
        let (releases, release_rx) = mpsc::sync_channel::<ReleaseJob>(16);
        let (events, event_rx) = mpsc::channel();
        let bind_client = client.fork_native();
        let bind_project = project.clone();
        let bind_proof = proof.clone();
        let bind_events = events.clone();
        let bind_diagnostics = diagnostics.clone();
        let binder = thread::Builder::new()
            .name("pulsar-player-bind".into())
            .spawn(move || {
                while let Ok(job) = bind_rx.recv() {
                    if job.cancelled.load(Ordering::Acquire) {
                        if let Some(record) = records(&bind_diagnostics)
                            .records
                            .iter_mut()
                            .find(|r| r.diagnostic.binding_request_id == job.request_id)
                        {
                            record.diagnostic.phase = GuardPhase::NotDispatched;
                        }
                        continue;
                    }
                    let request_id = job.request_id;
                    let result = bind_client
                        .browser_call_until(
                            request_id.clone(),
                            Command::BindHandyPlayerGuard {
                                selection: config.selection.clone(),
                                player: job.identity.clone(),
                                policy: config.policy.clone(),
                                browser_media: job.browser_media.clone(),
                                allow_monitoring: true,
                                owner_proof: bind_proof.clone(),
                            },
                            bind_project.clone(),
                            Some(revision),
                            Instant::now() + BIND_TIMEOUT,
                        )
                        .and_then(|reply| {
                            validate_binding_reply(
                                reply,
                                &request_id,
                                &job.identity,
                                &config,
                                &job.browser_media,
                                &bind_project,
                                revision,
                            )
                        });
                    // Read only the original ID after uncertainty. Never issue replacement Bind.
                    let result = match result {
                        Ok(reply) => Ok(reply),
                        Err(_) => bind_client
                            .browser_call_until(
                                new_request_id(),
                                Command::HandyPlayerGuardStatus {
                                    binding_request_id: request_id.clone(),
                                    owner_proof: bind_proof.clone(),
                                },
                                bind_project.clone(),
                                None,
                                Instant::now() + BIND_TIMEOUT,
                            )
                            .and_then(|reply| {
                                validate_binding_reply(
                                    reply,
                                    &request_id,
                                    &job.identity,
                                    &config,
                                    &job.browser_media,
                                    &bind_project,
                                    revision,
                                )
                            }),
                    };
                    let result = match result {
                        Ok(reply) => {
                            let pulsar_protocol::ResponseBody::HandyPlayerGuard(report) = &reply
                            else {
                                unreachable!("validated guard reply")
                            };
                            if let Some(record) = records(&bind_diagnostics)
                                .records
                                .iter_mut()
                                .find(|r| r.diagnostic.binding_request_id == request_id)
                            {
                                record.diagnostic.binding_id =
                                    Some(report.binding.binding_id.clone());
                                record.diagnostic.phase = GuardPhase::Bound;
                                record.report = Some((**report).clone());
                            }
                            if job.cancelled.load(Ordering::Acquire) {
                                if let Some(release) = reserve_release(&bind_diagnostics, report) {
                                    release_once(
                                        &bind_client,
                                        &bind_project,
                                        &bind_proof,
                                        &bind_diagnostics,
                                        release,
                                    );
                                }
                            }
                            Ok(reply)
                        }
                        Err(_) => {
                            mark_uncertain(
                                &bind_diagnostics,
                                &request_id,
                                "bind_and_original_status_unconfirmed",
                            );
                            Err(sanitized_error(&bind_diagnostics))
                        }
                    };
                    if bind_events
                        .send(Event {
                            ticket: None,
                            identity: job.identity,
                            binding_id: None,
                            observation: None,
                            result,
                        })
                        .is_err()
                    {
                        record_issue(
                            &bind_diagnostics,
                            &request_id,
                            "binding_result_delivery_failed",
                        );
                    }
                }
            })?;
        let fast_client = client.fork_native();
        let fast_project = project.clone();
        let fast_proof = proof.clone();
        let fast_diagnostics = diagnostics.clone();
        let runner = thread::Builder::new()
            .name("pulsar-player-control".into())
            .spawn(move || loop {
                while let Ok(job) = release_rx.try_recv() {
                    release_once(
                        &fast_client,
                        &fast_project,
                        &fast_proof,
                        &fast_diagnostics,
                        job,
                    );
                }
                match fast_rx.recv_timeout(Duration::from_millis(5)) {
                    Ok(job) => {
                        let binding_id = match &job.command {
                            Command::ObserveHandyPlayer { binding_id, .. }
                            | Command::IssueHandyPlayerChallenge { binding_id, .. } => {
                                Some(binding_id.clone())
                            }
                            _ => None,
                        };
                        let observation = match &job.command {
                            Command::ObserveHandyPlayer {
                                sequence, sample, ..
                            } => Some((*sequence, sample.clone())),
                            _ => None,
                        };
                        let result = if job.cancelled.load(Ordering::Acquire) {
                            Err(anyhow::anyhow!("player retired before control dispatch"))
                        } else {
                            fast_client
                                .browser_call_until(
                                    new_request_id(),
                                    job.command,
                                    fast_project.clone(),
                                    None,
                                    Instant::now() + FAST_TIMEOUT,
                                )
                                .map_err(|_| anyhow::anyhow!("native control exchange unconfirmed"))
                        };
                        let original = records(&fast_diagnostics)
                            .records
                            .iter()
                            .find(|r| {
                                r.diagnostic.player == job.identity
                                    && r.diagnostic.binding_id == binding_id
                            })
                            .map(|r| r.diagnostic.binding_request_id.clone());
                        if result.is_err() && !job.cancelled.load(Ordering::Acquire) {
                            if let Some(id) = &original {
                                record_issue(
                                    &fast_diagnostics,
                                    id,
                                    "native_control_exchange_unconfirmed",
                                );
                            }
                        }
                        if events
                            .send(Event {
                                ticket: Some(job.ticket),
                                identity: job.identity,
                                binding_id,
                                observation,
                                result,
                            })
                            .is_err()
                        {
                            if let Some(id) = &original {
                                record_issue(
                                    &fast_diagnostics,
                                    id,
                                    "control_result_delivery_failed",
                                );
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        while let Ok(job) = release_rx.try_recv() {
                            release_once(
                                &fast_client,
                                &fast_project,
                                &fast_proof,
                                &fast_diagnostics,
                                job,
                            );
                        }
                        break;
                    }
                }
            });
        let runner = match runner {
            Ok(t) => t,
            Err(e) => {
                drop(binds);
                let _ = binder.join();
                return Err(e.into());
            }
        };
        Ok(Self {
            binds: Some(binds),
            fast: Some(fast),
            releases: Some(releases),
            events: event_rx,
            threads: vec![binder, runner],
            proof,
            diagnostics,
            cleanup_client: client.fork_native(),
            project,
        })
    }
    fn bind(
        &self,
        identity: HandyPlayerIdentity,
        cancelled: Arc<AtomicBool>,
        browser_media: Option<BrowserMediaBinding>,
    ) -> Result<()> {
        let request_id = new_request_id();
        {
            let mut state = records(&self.diagnostics);
            ensure!(
                state.failure().is_none() && !state.unfinished(),
                "{}",
                state.summary()
            );
            if state.records.len() >= DIAGNOSTIC_LIMIT {
                state.overflow = true;
                anyhow::bail!("{}", state.summary());
            }
            state.records.push(GuardRecord {
                diagnostic: GuardDiagnostic {
                    binding_request_id: request_id.clone(),
                    binding_id: None,
                    player: identity.clone(),
                    phase: GuardPhase::BindingPending,
                    issue: None,
                },
                report: None,
            });
        }
        let job = BindJob {
            browser_media,
            request_id: request_id.clone(),
            identity,
            cancelled,
        };
        let accepted = self
            .binds
            .as_ref()
            .is_some_and(|tx| tx.try_send(job).is_ok());
        if !accepted {
            mark_uncertain(&self.diagnostics, &request_id, "binding_queue_unavailable");
            return Err(sanitized_error(&self.diagnostics));
        }
        Ok(())
    }
    fn failure(&self) -> Option<String> {
        records(&self.diagnostics).failure()
    }
    fn release(&self, report: &HandyPlayerGuardReport) {
        let Some(job) = reserve_release(&self.diagnostics, report) else {
            return;
        };
        if !self
            .releases
            .as_ref()
            .is_some_and(|tx| tx.try_send(job).is_ok())
        {
            mark_uncertain(
                &self.diagnostics,
                &report.binding.request_id,
                "release_queue_unavailable",
            );
        }
    }
    fn join(&mut self) -> Result<()> {
        self.binds.take();
        self.fast.take();
        self.releases.take();
        for thread in self.threads.drain(..) {
            if thread.join().is_err() {
                records(&self.diagnostics).worker_failed = true;
            }
        }
        // A successful Bind may be queued in events when shutdown begins. Its
        // cached exact subject still receives one release, without needing poll().
        let remaining: Vec<_> = records(&self.diagnostics)
            .records
            .iter()
            .filter(|r| r.diagnostic.phase == GuardPhase::Bound)
            .filter_map(|r| r.report.clone())
            .collect();
        for report in remaining {
            if let Some(job) = reserve_release(&self.diagnostics, &report) {
                release_once(
                    &self.cleanup_client,
                    &self.project,
                    &self.proof,
                    &self.diagnostics,
                    job,
                );
            }
        }
        let state = records(&self.diagnostics);
        ensure!(
            state.failure().is_none() && !state.unfinished(),
            "{}",
            state.summary()
        );
        Ok(())
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

pub(super) enum ControlReply {
    Ready(Value),
    Pending,
}
pub(super) struct PlayerControl {
    launch: AttemptId,
    pages: BTreeMap<String, Page>,
    player: Option<Player>,
    native: Option<Native>,
    timing: timing::Timing,
    playback: Option<crate::browser_handy_playback::Playback>,
    hsp: Option<crate::browser_handy_hsp::Hsp>,
    guard_configured: bool,
    work_generation: Arc<AtomicBool>,
}
impl PlayerControl {
    #[cfg(test)]
    pub fn new(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        guard: Option<GuardConfig>,
    ) -> Result<Self> {
        Self::new_with_playback(client, project, revision, guard, None)
    }
    #[cfg(test)]
    pub fn new_with_playback(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        guard: Option<GuardConfig>,
        playback: Option<crate::browser_handy_playback::Config>,
    ) -> Result<Self> {
        Self::new_with_hsp(client, project, revision, guard, playback, None)
    }
    pub fn new_with_hsp(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        guard: Option<GuardConfig>,
        playback: Option<crate::browser_handy_playback::Config>,
        hsp: Option<crate::browser_handy_hsp::Config>,
    ) -> Result<Self> {
        ensure!(
            usize::from(guard.is_some())
                + usize::from(playback.is_some())
                + usize::from(hsp.is_some())
                <= 1,
            "choose one Handy browser control mode"
        );
        let configured = guard.is_some();
        Ok(Self {
            launch: id(),
            pages: BTreeMap::new(),
            player: None,
            native: guard
                .map(|c| Native::new(client, project.clone(), revision, c))
                .transpose()?,
            timing: timing::Timing::new(client)?,
            hsp: hsp
                .map(|config| {
                    crate::browser_handy_hsp::Hsp::new(client, project.clone(), revision, config)
                })
                .transpose()?,
            playback: playback
                .map(|config| {
                    crate::browser_handy_playback::Playback::new(client, project, revision, config)
                })
                .transpose()?,
            guard_configured: configured,
            work_generation: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn page(&mut self) -> Result<Value> {
        ensure!(
            self.pages.len() < 16,
            "page admission full; restart the native player server"
        );
        let page = id();
        let cap = capability();
        self.pages.insert(
            page.to_string(),
            Page {
                capability: cap.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
        Ok(json!({"launch_id":self.launch,"page_id":page,"page_capability":cap}))
    }
    fn page_for(&self, scope: &Scope) -> Result<&Page> {
        ensure!(
            scope.launch == self.launch.as_str(),
            "browser launch differs"
        );
        let page = self.pages.get(&scope.page).context("browser page absent")?;
        ensure!(
            page.capability == scope.page_capability && !page.cancelled.load(Ordering::Acquire),
            "browser page capability differs"
        );
        Ok(page)
    }
    fn matches(player: &Player, scope: &Scope) -> bool {
        scope.player.as_deref() == Some(player.identity.player_id.as_str())
            && scope.source.as_deref() == Some(player.identity.source_id.as_str())
            && scope.generation == Some(player.identity.generation)
            && scope.player_capability.as_ref() == Some(&player.capability)
            && scope.page == player.identity.page_id.as_str()
    }
    pub fn authorize(&self, scope: &Scope, live: bool) -> Result<Permit> {
        self.page_for(scope)?;
        if let Some(player) = &self.player {
            if live || player.active || scope.player.is_some() {
                ensure!(Self::matches(player, scope), "browser player scope differs");
                if live {
                    ensure!(
                        player.active && !player.cancelled.load(Ordering::Acquire),
                        "browser player retired"
                    );
                }
                return Ok(Permit {
                    identity: Some(player.identity.clone()),
                    cancelled: if live {
                        player.cancelled.clone()
                    } else {
                        self.work_generation.clone()
                    },
                    page: scope.page.clone(),
                });
            }
        }
        ensure!(
            !live && scope.player.is_none(),
            "register an explicit player first"
        );
        Ok(Permit {
            identity: None,
            cancelled: self.work_generation.clone(),
            page: scope.page.clone(),
        })
    }
    pub fn still_matches(&self, permit: &Permit) -> bool {
        permit.current()
            && self
                .pages
                .get(&permit.page)
                .is_some_and(|p| !p.cancelled.load(Ordering::Acquire))
            && match (&permit.identity, &self.player) {
                (Some(identity), Some(player)) => identity == &player.identity,
                (None, Some(player)) => !player.active,
                (None, None) => true,
                _ => false,
            }
    }
    pub fn stop(&mut self) {
        if let Some(player) = &mut self.player {
            player.active = false;
            player.cancelled.store(true, Ordering::Release);
            self.timing.retire(&player.identity);
            if let Some(playback) = &self.playback {
                playback.retire(&player.identity);
            }
            if let Some(hsp) = &self.hsp {
                hsp.retire(&player.identity);
            }
            if let (Some(native), Some(report)) = (&self.native, &player.binding) {
                native.release(report);
            }
        }
    }
    pub fn active(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.active)
    }
    pub fn tick(&mut self) -> bool {
        if self
            .player
            .as_ref()
            .is_some_and(|p| p.active && p.heartbeat.elapsed() > LOCAL_HEARTBEAT)
        {
            self.stop();
            true
        } else {
            false
        }
    }
    pub fn media_page(&self, scope: &Scope) -> Result<Arc<AtomicBool>> {
        Ok(self.page_for(scope)?.cancelled.clone())
    }
    pub fn active_browser_media(&self) -> Option<&BrowserMediaBinding> {
        self.player
            .as_ref()
            .filter(|p| p.active)
            .and_then(|p| p.browser_media.as_ref())
    }
    pub fn browser_media_for(&self, permit: &Permit) -> Result<Option<BrowserMediaBinding>> {
        ensure!(
            self.still_matches(permit),
            "browser player changed before source binding"
        );
        Ok(self
            .player
            .as_ref()
            .filter(|p| permit.identity.as_ref() == Some(&p.identity))
            .and_then(|p| p.browser_media.clone()))
    }
    pub fn browser_timing_for(&self, permit: &Permit) -> Result<BrowserTimingBinding> {
        ensure!(
            self.still_matches(permit),
            "visible player changed before timing selection"
        );
        let player = permit.identity.as_ref().context("visible player absent")?;
        self.timing.current_binding(player)
    }
    pub fn successor_timing_for(&self, permit: &Permit) -> Result<BrowserTimingBinding> {
        ensure!(self.still_matches(permit), "successor player retired");
        self.timing
            .successor_binding(permit.identity.as_ref().context("player absent")?)
    }
    pub fn require_analysis_continuity_supported(&self) -> Result<()> {
        ensure!(
            !self.guard_configured && self.playback.is_none(),
            "analysis continuity cannot transfer an existing hosted HSSP playback or stop guard"
        );
        Ok(())
    }
    pub fn cancel_analysis_successor(&self, permit: &Permit) -> Result<()> {
        ensure!(
            self.still_matches(permit),
            "analysis cancellation player changed"
        );
        self.timing
            .cancel_successor(permit.identity.as_ref().context("player absent")?);
        Ok(())
    }
    pub fn check_analysis_commit(
        &self,
        permit: &Permit,
        commit: &crate::browser_live::AnalysisCommit,
    ) -> Result<()> {
        ensure!(
            self.still_matches(permit) && self.active_browser_media() == Some(&commit.old_media),
            "analysis commit player/media differs"
        );
        commit.transfer.validate()?;
        ensure!(
            commit.transfer.phase == BrowserAnalysisTransferPhase::Transferred,
            "native transfer is not committed"
        );
        self.timing
            .check_successor(&commit.old_timing, &commit.new_timing)
    }
    pub fn commit_analysis(
        &mut self,
        permit: &Permit,
        commit: &crate::browser_live::AnalysisCommit,
    ) -> Result<()> {
        self.check_analysis_commit(permit, commit)?;
        self.timing
            .commit_successor(&commit.old_timing, &commit.new_timing)?;
        self.player.as_mut().context("player absent")?.browser_media =
            Some(commit.new_media.clone());
        Ok(())
    }
    pub fn hsp_handle(
        &mut self,
        path: &str,
        permit: &Permit,
        source: Option<crate::browser_live::BrowserAnalysisFeedSnapshot>,
        body: &[u8],
    ) -> Result<Value> {
        ensure!(
            self.still_matches(permit),
            "HSP browser operation belongs to a retired page/player"
        );
        if let Some(source) = &source {
            ensure!(
                permit.identity.as_ref() == Some(&source.binding.player),
                "HSP analysis snapshot belongs to another player"
            );
        }
        match self.hsp.as_mut() {
            Some(hsp) => hsp.handle(path, permit, source, body),
            None if path == "/player/hsp-inspect" || path == "/player/hsp-status" => {
                ensure!(body.is_empty(), "HSP status/inspection body must be empty");
                Ok(json!({"configured":false,"phase":"not_configured","error":null}))
            }
            None => anyhow::bail!("live HSP browser controls are not configured"),
        }
    }
    #[cfg(test)]
    pub fn handle(
        &mut self,
        path: &str,
        scope: &Scope,
        body: &[u8],
        ticket: u64,
    ) -> Result<ControlReply> {
        self.handle_with_media(path, scope, body, ticket, None)
    }
    pub fn handle_with_media(
        &mut self,
        path: &str,
        scope: &Scope,
        body: &[u8],
        ticket: u64,
        browser_media: Option<BrowserMediaBinding>,
    ) -> Result<ControlReply> {
        self.page_for(scope)?;
        if path == "/player/timing-next-open" {
            let permit = self.authorize(scope, true)?;
            ensure!(body.is_empty(), "successor timing body refused");
            self.timing.open_successor(
                permit.identity.as_ref().context("player absent")?,
                browser_media.context("successor media absent")?,
            )?;
            return Ok(ControlReply::Ready(json!({"pending":true})));
        }
        if matches!(
            path,
            "/player/handy-inspect"
                | "/player/handy-start"
                | "/player/handy-inspect-original"
                | "/player/handy-status"
                | "/player/handy-stop"
        ) {
            let live = matches!(path, "/player/handy-inspect" | "/player/handy-start");
            let permit = self.authorize(scope, live)?;
            let identity = permit
                .identity
                .as_ref()
                .context("exact player capability required for browser playback")?;
            let playback = self
                .playback
                .as_ref()
                .context("prepared Handy browser controls are not configured")?;
            let media = self
                .player
                .as_ref()
                .filter(|p| p.identity == *identity)
                .and_then(|p| p.browser_media.clone());
            let timing = if live {
                Some(self.timing.current_binding(identity)?)
            } else {
                None
            };
            return playback.handle(
                path,
                identity,
                permit.cancelled,
                media,
                timing,
                body,
                ticket,
            );
        }
        if matches!(
            path,
            "/player/timing-probe"
                | "/player/timing-observe"
                | "/player/timing-status"
                | "/player/timing-next-probe"
                | "/player/timing-next-observe"
                | "/player/timing-next-status"
        ) {
            let permit = self.authorize(scope, path != "/player/timing-status")?;
            return self.timing.handle(
                path,
                permit
                    .identity
                    .as_ref()
                    .context("exact player capability required for timing")?,
                body,
                ticket,
            );
        }
        if path != "/player/stop" {
            if let Some(error) = self.native.as_ref().and_then(Native::failure) {
                anyhow::bail!("{error}");
            }
        }
        if path == "/player/start" {
            ensure!(
                self.player.as_ref().is_none_or(|p| !p.active),
                "stop the prior player before replacement"
            );
            let start: Start = serde_json::from_slice(body)?;
            ensure!(
                start.media_selection_id.is_some() == browser_media.is_some(),
                "prepared source binding is native-only"
            );
            if let Some(selection) = &start.media_selection_id {
                ensure!(
                    selection == &start.source_id,
                    "prepared source selection differs"
                );
            }
            if let Some(binding) = &browser_media {
                binding.validate()?;
                ensure!(
                    start.time_basis == LiveSourceTimeBasis::SourceMediaPresentation,
                    "bound player time basis differs"
                );
            }
            let identity = HandyPlayerIdentity {
                launch_id: self.launch.clone(),
                page_id: AttemptId::new(scope.page.clone())?,
                player_id: id(),
                source_id: start.source_id,
                generation: start.generation,
                time_basis: start.time_basis,
            };
            identity.validate()?;
            if let Some(previous) = &self.player {
                ensure!(
                    identity.source_id != previous.identity.source_id
                        || identity.generation > previous.identity.generation,
                    "player source generation was already retired"
                );
            }
            let cap = capability();
            let cancelled = Arc::new(AtomicBool::new(false));
            if let Some(native) = &self.native {
                native.bind(identity.clone(), cancelled.clone(), browser_media.clone())?;
            }
            let timing = match &browser_media {
                Some(binding) => {
                    match self
                        .timing
                        .open(identity.clone(), cancelled.clone(), binding.clone())
                    {
                        Ok(()) => json!({"configured":true,"pending":true}),
                        Err(error) => {
                            json!({"configured":true,"pending":false,"error":error.to_string()})
                        }
                    }
                }
                None => json!({"configured":false,"pending":false}),
            };
            self.work_generation.store(true, Ordering::Release);
            self.work_generation = Arc::new(AtomicBool::new(false));
            self.player = Some(Player {
                identity: identity.clone(),
                capability: cap.clone(),
                cancelled,
                active: true,
                heartbeat: Instant::now(),
                binding: None,
                binding_pending: self.guard_configured,
                browser_media,
                sequence: 0,
                fast_pending: false,
                failed: false,
            });
            return Ok(ControlReply::Ready(
                json!({"player":identity,"player_capability":cap,"guard":{"configured":self.guard_configured,"pending":self.guard_configured,"current":false},"timing":timing,"handy":{"configured":self.playback.is_some()},"hsp":{"configured":self.hsp.is_some()},"heartbeat_interval_ms":100}),
            ));
        }
        self.authorize(scope, true)?;
        if path == "/player/stop" {
            ensure!(body.is_empty(), "stop body refused");
            self.stop();
            if let Some(error) = self.native.as_ref().and_then(Native::failure) {
                anyhow::bail!("{error}");
            }
            return Ok(ControlReply::Ready(
                json!({"active":false,"capture_cleanup_pending":true,"synchronized":false}),
            ));
        }
        let player = self.player.as_mut().context("player absent")?;
        ensure!(
            !player.failed,
            "Handy stop guard unavailable; stop this player"
        );
        if path == "/player/challenge" {
            ensure!(body.is_empty(), "challenge body refused");
            if !self.guard_configured || player.binding_pending {
                player.heartbeat = Instant::now();
                return Ok(ControlReply::Ready(
                    json!({"pending":true,"configured":self.guard_configured}),
                ));
            }
        }
        let report = player
            .binding
            .as_ref()
            .context("native guard binding absent")?;
        ensure!(
            report.current && !player.fast_pending,
            "native guard unavailable or control pending"
        );
        let native = self.native.as_ref().context("guard absent")?;
        let command = match path {
            "/player/challenge" => Command::IssueHandyPlayerChallenge {
                binding_id: report.binding.binding_id.clone(),
                player: player.identity.clone(),
                sequence: player
                    .sequence
                    .checked_add(1)
                    .context("player sequence overflow")?,
                owner_proof: native.proof.clone(),
            },
            "/player/observe" => {
                let observed: Observe = serde_json::from_slice(body)?;
                observed.sample.validate()?;
                ensure!(
                    Some(observed.sequence) == player.sequence.checked_add(1),
                    "player sequence differs"
                );
                Command::ObserveHandyPlayer {
                    binding_id: report.binding.binding_id.clone(),
                    player: player.identity.clone(),
                    sequence: observed.sequence,
                    challenge_id: observed.challenge_id,
                    sample: observed.sample,
                    owner_proof: native.proof.clone(),
                }
            }
            _ => anyhow::bail!("unknown player operation"),
        };
        let accepted = native.fast.as_ref().is_some_and(|tx| {
            tx.try_send(FastJob {
                ticket,
                identity: player.identity.clone(),
                command,
                cancelled: player.cancelled.clone(),
            })
            .is_ok()
        });
        if !accepted {
            record_issue(
                &native.diagnostics,
                &report.binding.request_id,
                "control_queue_unavailable",
            );
            return Err(sanitized_error(&native.diagnostics));
        }
        player.heartbeat = Instant::now();
        player.fast_pending = true;
        Ok(ControlReply::Pending)
    }
    pub fn poll(&mut self) -> Vec<(u64, Result<Value>)> {
        let mut replies = Vec::new();
        while let Some(event) = self.native.as_ref().and_then(|n| n.events.try_recv().ok()) {
            let current = self
                .player
                .as_ref()
                .is_some_and(|p| p.active && p.identity == event.identity);
            if !current {
                if let Ok(pulsar_protocol::ResponseBody::HandyPlayerGuard(report)) = &event.result {
                    if let Some(native) = &self.native {
                        if report.validate().is_ok()
                            && report.binding.player == event.identity
                            && event
                                .binding_id
                                .as_ref()
                                .is_none_or(|id| id == &report.binding.binding_id)
                        {
                            native.release(report);
                        }
                    }
                }
                if let Some(ticket) = event.ticket {
                    replies.push((
                        ticket,
                        Err(anyhow::anyhow!("player retired during native operation")),
                    ));
                }
                continue;
            }
            let player = self.player.as_mut().expect("checked player");
            let result = event.result.and_then(|reply| match reply {
                pulsar_protocol::ResponseBody::HandyPlayerGuard(report) => {
                    report.validate()?;
                    ensure!(
                        event.ticket.is_none() || event.observation.is_some(),
                        "unexpected guard report for challenge"
                    );
                    ensure!(
                        report.binding.player == player.identity,
                        "native guard player differs"
                    );
                    if let Some(binding) = &player.binding {
                        ensure!(
                            serde_json::to_value(&binding.binding)?
                                == serde_json::to_value(&report.binding)?
                                && binding.bound_at_boottime_ns == report.bound_at_boottime_ns,
                            "native guard binding differs"
                        );
                    }
                    if let Some((sequence, sample)) = &event.observation {
                        ensure!(
                            report.last_sequence == *sequence
                                && report.last_sample.as_ref() == Some(sample),
                            "native guard observation differs"
                        );
                    }
                    player.sequence = report.last_sequence;
                    let active = report.current;
                    player.binding = Some(*report);
                    ensure!(active, "native guard retired");
                    Ok(json!({"current":true,"synchronized":false}))
                }
                pulsar_protocol::ResponseBody::HandyPlayerChallenge(challenge) => {
                    challenge.validate()?;
                    ensure!(
                        event.ticket.is_some()
                            && event.observation.is_none()
                            && challenge.current
                            && challenge.player == player.identity
                            && player
                                .binding
                                .as_ref()
                                .is_some_and(|b| b.binding.binding_id == challenge.binding_id)
                            && Some(challenge.sequence) == player.sequence.checked_add(1),
                        "native challenge differs"
                    );
                    Ok(json!({"sequence":challenge.sequence,"challenge_id":challenge.challenge_id}))
                }
                _ => anyhow::bail!("unexpected native player response"),
            });
            if event.ticket.is_none() {
                player.binding_pending = false;
            } else {
                player.fast_pending = false;
            }
            let result = if result.is_err() {
                player.failed = true;
                if let Some(native) = &self.native {
                    if let Some(report) = &player.binding {
                        record_issue(
                            &native.diagnostics,
                            &report.binding.request_id,
                            "native_control_reply_unconfirmed",
                        );
                    }
                }
                self.stop();
                Err(self
                    .native
                    .as_ref()
                    .map(|native| sanitized_error(&native.diagnostics))
                    .unwrap_or_else(|| anyhow::anyhow!("native guard outcome unconfirmed")))
            } else {
                result
            };
            if let Some(ticket) = event.ticket {
                replies.push((ticket, result));
            }
        }
        replies.extend(
            self.timing
                .poll(self.player.as_ref().map(|p| &p.identity), self.active()),
        );
        if let Some(playback) = &self.playback {
            replies.extend(playback.poll(self.player.as_ref().map(|p| &p.identity)));
        }
        replies
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.stop();
        self.work_generation.store(true, Ordering::Release);
        for page in self.pages.values() {
            page.cancelled.store(true, Ordering::Release);
        }
        let play = self
            .playback
            .as_mut()
            .map_or(Ok(()), crate::browser_handy_playback::Playback::shutdown);
        let hsp = self
            .hsp
            .as_mut()
            .map_or(Ok(()), crate::browser_handy_hsp::Hsp::shutdown);
        let play = match (play, hsp) {
            (Ok(()), value) | (value, Ok(())) => value,
            (Err(play), Err(hsp)) => Err(anyhow::anyhow!("{play}; {hsp}")),
        };
        let guard = self.native.as_mut().map_or(Ok(()), Native::join);
        let timing = self.timing.shutdown();
        let timing = match (timing, play) {
            (Ok(()), result) | (result, Ok(())) => result,
            (Err(timing), Err(play)) => Err(anyhow::anyhow!("{timing}; {play}")),
        };
        match (guard, timing) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(guard), Err(timing)) => Err(anyhow::anyhow!("{guard}; {timing}")),
        }
    }
}
impl Drop for PlayerControl {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
#[path = "browser_player_control_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "browser_player_guard_io_tests.rs"]
mod guard_io_tests;
