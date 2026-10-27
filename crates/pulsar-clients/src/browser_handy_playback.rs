//! Explicit browser Start with three independent native lanes. No native credentials reach JS.
use crate::{
    browser_player_control::ControlReply,
    session::{new_request_id, SessionClient},
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const RPC: Duration = Duration::from_secs(3);
const FAST: Duration = Duration::from_millis(200);
const REFRESH: Duration = Duration::from_secs(10);
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub acquisition_request_id: RequestId,
    pub controller_id: ClientId,
    pub expected_generation: u64,
    pub clock_request_id: RequestId,
    pub qualification: HandyBrowserStartQualificationBinding,
    pub renewal_interval_ms: u16,
    pub device_refresh_interval_ms: u16,
}
impl Config {
    pub(crate) fn validate(&self) -> Result<()> {
        self.qualification.validate()?;
        ensure!(
            self.acquisition_request_id != self.clock_request_id
                && self.expected_generation <= i64::MAX as u64,
            "invalid prepared controller selection"
        );
        ensure!(
            (50..=1000).contains(&self.renewal_interval_ms)
                && (100..=2000).contains(&self.device_refresh_interval_ms),
            "browser ownership intervals exceed software bounds"
        );
        Ok(())
    }
}
#[derive(Clone)]
struct Inspected {
    player: HandyPlayerIdentity,
    timing: BrowserTimingBinding,
    media: BrowserMediaBinding,
    view: Value,
}
struct Intent {
    request: RequestId,
    player: HandyPlayerIdentity,
    timing: BrowserTimingBinding,
    cancelled: Arc<AtomicBool>,
    reserved: Instant,
    dispatched: bool,
    start_finished: bool,
    stop_requested: bool,
    released: bool,
    history: Option<HandyPlaybackStatus>,
    owner: Option<HandyBrowserPlaybackReport>,
    refresh_request: Option<RequestId>,
    issue: Option<&'static str>,
}
#[derive(Default)]
struct State {
    inspected: Option<Inspected>,
    intent: Option<Intent>,
    failed_worker: bool,
}
type Shared = Arc<Mutex<State>>;
fn state(shared: &Shared) -> std::sync::MutexGuard<'_, State> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}
#[derive(Clone)]
struct Native {
    client: Arc<SessionClient>,
    project: ProjectId,
    revision: RevisionId,
    config: Config,
    proof: String,
}
impl Native {
    fn call(
        &self,
        id: RequestId,
        command: Command,
        revision: Option<RevisionId>,
        deadline: Instant,
    ) -> Result<pulsar_protocol::ResponseBody> {
        self.client
            .browser_call_until(id, command, self.project.clone(), revision, deadline)
    }
    fn history(
        &self,
        request: &RequestId,
        timing: &BrowserTimingBinding,
        deadline: Instant,
    ) -> Result<HandyPlaybackStatus> {
        let pulsar_protocol::ResponseBody::HandyPlayback(report) = self.call(
            new_request_id(),
            Command::HandyPlaybackStatus {
                play_request_id: request.clone(),
                owner_proof: self.proof.clone(),
            },
            None,
            deadline,
        )?
        else {
            anyhow::bail!("unexpected original Start status");
        };
        self.check_history(&report, request, timing)?;
        Ok(*report)
    }
    fn check_history(
        &self,
        report: &HandyPlaybackStatus,
        request: &RequestId,
        timing: &BrowserTimingBinding,
    ) -> Result<()> {
        report.validate()?;
        ensure!(
            report.project_id == self.project && &report.play_request_id == request,
            "original Start identity changed"
        );
        if let Some(history) = &report.history {
            let lease = &history.original_controller;
            let decision = history
                .browser_start
                .as_ref()
                .context("original Start browser decision absent")?;
            ensure!(
                lease.acquisition_request_id == self.config.acquisition_request_id
                    && lease.controller_id == self.config.controller_id
                    && lease.generation == self.config.expected_generation
                    && lease.project_id == self.project
                    && lease.revision == self.revision
                    && history.clock_request_id == self.config.clock_request_id
                    && decision.qualification == self.config.qualification
                    && decision.timing_binding == *timing,
                "original Start captured subject changed"
            );
        }
        Ok(())
    }
    fn check_owner(
        &self,
        reply: pulsar_protocol::ResponseBody,
        request: &RequestId,
        timing: &BrowserTimingBinding,
        record: Option<&ArtifactId>,
    ) -> Result<HandyBrowserPlaybackReport> {
        let pulsar_protocol::ResponseBody::HandyBrowserPlayback(report) = reply else {
            anyhow::bail!("unexpected browser owner response");
        };
        report.validate()?;
        ensure!(
            report.project_id == self.project
                && &report.play_request_id == request
                && report.browser_timing_binding_id == timing.binding_id
                && record.is_none_or(|id| report.play_record_id.as_ref() == Some(id))
                && report
                    .controller
                    .as_ref()
                    .is_none_or(|lease| lease.acquisition_request_id
                        == self.config.acquisition_request_id
                        && lease.controller_id == self.config.controller_id
                        && lease.project_id == self.project
                        && lease.revision == self.revision
                        && lease.generation >= self.config.expected_generation),
            "browser owner subject changed"
        );
        Ok(*report)
    }
    fn inspect(
        &self,
        player: HandyPlayerIdentity,
        media: BrowserMediaBinding,
        timing: BrowserTimingBinding,
        deadline: Instant,
    ) -> Result<Inspected> {
        ensure!(
            timing.player == player
                && timing.browser_media == media
                && timing.project_id == self.project
                && timing.revision == self.revision,
            "native timing/source selection differs"
        );
        let pulsar_protocol::ResponseBody::HandyController(controller) = self.call(
            new_request_id(),
            Command::HandyControllerStatus {
                acquisition_request_id: self.config.acquisition_request_id.clone(),
                owner_proof: self.proof.clone(),
            },
            None,
            deadline,
        )?
        else {
            anyhow::bail!("controller response differs");
        };
        controller.validate()?;
        let lease = &controller.lease;
        ensure!(lease.state == HandyControllerState::Active && lease.controller_id == self.config.controller_id
            && lease.acquisition_request_id == self.config.acquisition_request_id && lease.generation == self.config.expected_generation
            && lease.project_id == self.project && lease.revision == self.revision
            && u64::from(self.config.renewal_interval_ms) * 2 < lease.duration_ms,
            "prepared controller is stale, changed, or too short for selected renewal interval; explicitly prepare again");
        let pulsar_protocol::ResponseBody::HandyPlaybackPreparation(preparation) = self.call(
            new_request_id(),
            Command::HandyPlaybackPreparationStatus {
                preparation_request_id: lease.preparation_request_id.clone(),
                owner_proof: self.proof.clone(),
            },
            None,
            deadline,
        )?
        else {
            anyhow::bail!("preparation response differs");
        };
        preparation.validate()?;
        ensure!(
            preparation.preparation_id == lease.preparation_id
                && preparation.request_id == lease.preparation_request_id
                && preparation.state == HandyPlaybackPreparationState::Prepared
                && !preparation.physical_stop_pending,
            "preparation is retired or its identity changed"
        );
        let pulsar_protocol::ResponseBody::HandyBrowserStartQualification(dossier) = self.call(
            new_request_id(),
            Command::HandyBrowserStartQualificationStatus {
                dossier_id: self.config.qualification.dossier_id.clone(),
                owner_proof: self.proof.clone(),
            },
            None,
            deadline,
        )?
        else {
            anyhow::bail!("browser dossier response differs");
        };
        dossier.validate()?;
        let subject = &dossier.manifest.subject;
        let inspection = &preparation.correlation.inspection;
        ensure!(
            dossier.dossier_id == self.config.qualification.dossier_id
                && dossier.revision == self.config.qualification.revision
                && dossier.state == HandyQualificationState::Accepted
                && subject.origin == HandyEvidenceOrigin::Physical
                && subject.project_id == self.project
                && subject.revision == self.revision
                && subject.selection == media.selection
                && subject.clock == media.clock
                && subject.master_sha256 == inspection.master_sha256
                && subject.script == inspection.script
                && Some(&subject.device_qualification)
                    == inspection.accepted_qualification.as_ref(),
            "accepted browser/source/script dossier does not match prepared selection"
        );
        let view = json!({"selection_checked":true,"controller_id":lease.controller_id,"acquisition_request_id":lease.acquisition_request_id,
            "generation":lease.generation.to_string(),"preparation_id":lease.preparation_id,"device_id":lease.device_id,
            "dossier_id":dossier.dossier_id,"dossier_revision":dossier.revision.to_string(),"environment_sha256":subject.browser_environment.sha256,
            "source_sha256":media.selection.source.sha256,"placement_id":media.selection.placement.placement_id,
            "master_sha256":subject.master_sha256,"script_sha256":subject.script.sha256,"hosted_sha256":inspection.hosted.sha256,
            "timing_binding_id":timing.binding_id,"clock_request_id":self.config.clock_request_id,
            "note":"Selection inspected. Engine rechecks current clock, evidence and authority at Start; stale pre-start input requires explicit preparation."});
        Ok(Inspected {
            player,
            media,
            timing,
            view,
        })
    }
}
fn record_id(intent: &Intent) -> Option<ArtifactId> {
    intent
        .history
        .as_ref()
        .and_then(|h| h.history.as_ref())
        .map(|h| h.record_id.clone())
        .or_else(|| intent.owner.as_ref().and_then(|o| o.play_record_id.clone()))
}
fn retain_history(intent: &mut Intent, report: HandyPlaybackStatus) -> Result<()> {
    let known = record_id(intent);
    ensure!(
        known
            .as_ref()
            .is_none_or(|id| report.history.as_ref().is_some_and(|h| &h.record_id == id)),
        "original play record disappeared or changed"
    );
    intent.history = Some(report);
    Ok(())
}
fn view(intent: &Intent) -> Value {
    let history = intent.history.as_ref().and_then(|h| h.history.as_ref());
    let owner = intent.owner.as_ref();
    let alignment = owner.and_then(|o| o.alignment.as_ref()).map(|a| json!({
        "phase_earliest_ns":a.phase_earliest_ns.to_string(),"phase_latest_ns":a.phase_latest_ns.to_string(),
        "start_time_ms":a.start_time_ms.to_string(),"total_error_ns":a.total_error_ns.to_string()}));
    json!({"configured":true,"original_request_id":intent.request,"play_record_id":record_id(intent),
        "state":if intent.released { "released" } else if intent.stop_requested || intent.cancelled.load(Ordering::Acquire) { "stop_pending" }
            else if intent.issue.is_some() { "unknown" } else if intent.start_finished && history.is_some() { "submitted" } else if intent.start_finished { "unknown" } else { "pending" },
        "history_state":history.map(|h| h.state),"owner_current_at_last_reply":owner.is_some_and(|o| o.current) && !intent.released && !intent.stop_requested,
        "physical_stop_pending":owner.is_none_or(|o| o.physical_stop_pending),
        "controller_generation":owner.and_then(|o| o.controller.as_ref()).map(|c| c.generation.to_string()),
        "refresh_request_id":intent.refresh_request,"issue":intent.issue,"alignment":alignment,
        "note":"Acknowledgment is transport history; physical response and stopping remain unconfirmed."})
}
struct Inspection {
    player: HandyPlayerIdentity,
    media: BrowserMediaBinding,
    timing: BrowserTimingBinding,
    ticket: u64,
    deadline: Instant,
}
enum Job {
    Inspect(Box<Inspection>),
    Start,
    Status {
        player: HandyPlayerIdentity,
        ticket: u64,
        deadline: Instant,
    },
}
struct Event {
    player: HandyPlayerIdentity,
    ticket: u64,
    result: Result<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    acknowledge_current_environment: bool,
    allow_play: bool,
    allow_online_inspection: bool,
}
pub(crate) struct Playback {
    shared: Shared,
    jobs: Option<mpsc::SyncSender<Job>>,
    events: mpsc::Receiver<Event>,
    stopping: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
    cleanup: Native,
}

impl Playback {
    pub(crate) fn new(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        config: Config,
    ) -> Result<Self> {
        config.validate()?;
        let proof = crate::project_package::read_local_owner_proof(
            &client.output_state_directory()?,
            "handy-owner.token",
        )?;
        let native = Native {
            client: Arc::new(client.fork_native()),
            project,
            revision,
            config,
            proof,
        };
        let shared = Arc::new(Mutex::new(State::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let (jobs, receiver) = mpsc::sync_channel::<Job>(4);
        let (events, completed) = mpsc::sync_channel::<Event>(8);
        let n = native.clone();
        let s = shared.clone();
        let stop = stopping.clone();
        let starter = thread::Builder::new().name("pulsar-browser-start".into()).spawn(move || {
            while let Ok(job) = receiver.recv() {
                match job {
                    Job::Inspect(inspection) => {
                        let Inspection { player, media, timing, ticket, deadline } = *inspection;
                        let result = if stop.load(Ordering::Acquire) { Err(anyhow::anyhow!("browser owner shutting down")) } else {
                            n.inspect(player.clone(), media, timing, deadline).map(|selected| { let view = selected.view.clone(); state(&s).inspected = Some(selected); view })
                                .map_err(|_| anyhow::anyhow!("Prepared controller, source, script or browser dossier is unavailable or changed; explicitly inspect current preparation."))
                        };
                        if events.try_send(Event { player, ticket, result }).is_err() { state(&s).failed_worker = true; }
                    }
                    Job::Start => {
                        let selected = {
                            let mut shared = state(&s); let Some(intent) = &mut shared.intent else { continue; };
                            if stop.load(Ordering::Acquire) || intent.stop_requested || intent.cancelled.load(Ordering::Acquire) {
                                intent.start_finished = true; continue;
                            }
                            intent.dispatched = true; (intent.request.clone(), intent.timing.clone(), intent.reserved + RPC)
                        };
                        let (request, timing, deadline) = selected;
                        let reply = n.call(request.clone(), Command::StartHandyPlaybackFromBrowser {
                            acquisition_request_id:n.config.acquisition_request_id.clone(),controller_id:n.config.controller_id.clone(),
                            expected_generation:n.config.expected_generation,clock_request_id:n.config.clock_request_id.clone(),
                            browser_timing_binding_id:timing.binding_id.clone(),qualification:n.config.qualification.clone(),
                            acknowledge_current_environment:true,allow_play:true,owner_proof:n.proof.clone(),
                        }, Some(n.revision), deadline).and_then(|body| {
                            let pulsar_protocol::ResponseBody::HandyPlayback(report) = body else { anyhow::bail!("Start reply differs"); };
                            n.check_history(&report, &request, &timing)?; Ok(*report)
                        });
                        let mut shared = state(&s); let intent = shared.intent.as_mut().expect("original intent retained"); intent.start_finished = true;
                        match reply.and_then(|report|retain_history(intent,report)) { Ok(()) => {}, Err(_) => { intent.issue = Some("original_start_unconfirmed"); intent.stop_requested = true; } }
                    }
                    Job::Status { player, ticket, deadline } => {
                        let original = state(&s).intent.as_ref().map(|i| (i.request.clone(), i.timing.clone()));
                        let result = match original {
                            Some((request, timing)) => n.history(&request, &timing, deadline).and_then(|report| {
                                let mut shared=state(&s); let intent=shared.intent.as_mut().expect("original intent retained"); retain_history(intent,report)?; Ok(view(intent))
                            }).map_err(|_| anyhow::anyhow!("Original Start status unconfirmed; no replacement request was sent")),
                            None => Ok(json!({"configured":true,"state":"not_started"})),
                        };
                        if events.try_send(Event { player, ticket, result }).is_err() { state(&s).failed_worker = true; }
                    }
                }
            }
        })?;
        let n = native.clone();
        let s = shared.clone();
        let stop = stopping.clone();
        let renewer = thread::Builder::new()
            .name("pulsar-browser-owner".into())
            .spawn(move || {
                let mut next = Instant::now();
                let mut shutdown_deadline = None;
                loop {
                    if stop.load(Ordering::Acquire) && shutdown_deadline.is_none() {
                        shutdown_deadline = Some(Instant::now() + RPC);
                    }
                    let selected = {
                        let shared = state(&s);
                        shared.intent.as_ref().map(|i| {
                            (
                                i.request.clone(),
                                i.timing.clone(),
                                record_id(i),
                                i.stop_requested
                                    || i.cancelled.load(Ordering::Acquire)
                                    || stop.load(Ordering::Acquire),
                                i.released,
                                i.start_finished,
                                i.reserved,
                            )
                        })
                    };
                    let Some((request, timing, record, release, released, finished, reserved)) =
                        selected
                    else {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    if released {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    if !release && Instant::now() < next {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    let command = if release {
                        Command::ReleaseHandyBrowserPlayback {
                            play_request_id: request.clone(),
                            play_record_id: record.clone(),
                            browser_timing_binding_id: timing.binding_id.clone(),
                            owner_proof: n.proof.clone(),
                        }
                    } else {
                        Command::RenewHandyBrowserPlayback {
                            play_request_id: request.clone(),
                            play_record_id: record.clone(),
                            browser_timing_binding_id: timing.binding_id.clone(),
                            owner_proof: n.proof.clone(),
                        }
                    };
                    let reply = n
                        .call(new_request_id(), command, None, Instant::now() + FAST)
                        .and_then(|body| n.check_owner(body, &request, &timing, record.as_ref()));
                    let mut shared = state(&s);
                    let intent = shared.intent.as_mut().expect("original intent retained");
                    match reply {
                        Ok(report) if release && !report.current => {
                            intent.owner = Some(report);
                            intent.released = true;
                        }
                        Ok(report) if !release && report.current => intent.owner = Some(report),
                        _ if !release && !finished && Instant::now() < reserved + RPC => {}
                        _ => {
                            intent.issue.get_or_insert(if release {
                                "owner_release_unconfirmed"
                            } else {
                                "owner_renewal_unconfirmed"
                            });
                            intent.stop_requested = true;
                        }
                    }
                    if shutdown_deadline.is_some_and(|d| Instant::now() >= d) {
                        break;
                    }
                    drop(shared);
                    next = Instant::now()
                        + Duration::from_millis(u64::from(n.config.renewal_interval_ms));
                    if release {
                        thread::sleep(Duration::from_millis(20));
                    }
                }
            });
        let renewer = match renewer {
            Ok(worker) => worker,
            Err(error) => {
                stopping.store(true, Ordering::Release);
                drop(jobs);
                let _ = starter.join();
                return Err(error.into());
            }
        };
        let cleanup = native.clone();
        let n = native;
        let s = shared.clone();
        let stop = stopping.clone();
        let refresher = thread::Builder::new()
            .name("pulsar-browser-device-timing".into())
            .spawn(move || {
                let mut next = Instant::now();
                while !stop.load(Ordering::Acquire) {
                    let selected = {
                        let shared = state(&s);
                        shared
                            .intent
                            .as_ref()
                            .filter(|i| {
                                !i.stop_requested
                                    && !i.released
                                    && !i.cancelled.load(Ordering::Acquire)
                                    && i.owner.as_ref().is_some_and(|o| o.current)
                            })
                            .map(|i| (i.request.clone(), i.timing.clone()))
                    };
                    let Some((request, timing)) = selected else {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    if Instant::now() < next {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    let history = n.history(&request, &timing, Instant::now() + RPC);
                    let acknowledged = history.as_ref().is_ok_and(|h| {
                        h.history
                            .as_ref()
                            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
                    });
                    let terminal_bad = history.as_ref().is_err()
                        || history.as_ref().is_ok_and(|h| {
                            h.history.as_ref().is_some_and(|h| {
                                matches!(
                                    h.state,
                                    HandyPlaybackOperationState::Unknown
                                        | HandyPlaybackOperationState::Cancelled
                                )
                            })
                        });
                    {
                        let mut shared = state(&s);
                        let intent = shared.intent.as_mut().expect("original intent retained");
                        let changed = history
                            .ok()
                            .is_some_and(|report| retain_history(intent, report).is_err());
                        if (terminal_bad || changed)
                            && !intent.stop_requested
                            && !intent.cancelled.load(Ordering::Acquire)
                        {
                            intent
                                .issue
                                .get_or_insert("original_start_history_unconfirmed");
                            intent.stop_requested = true;
                        }
                    }
                    if acknowledged && !stop.load(Ordering::Acquire) {
                        let original = {
                            let mut shared = state(&s);
                            let intent = shared.intent.as_mut().expect("original intent retained");
                            if intent.stop_requested || intent.cancelled.load(Ordering::Acquire) {
                                None
                            } else {
                                let id = new_request_id();
                                intent.refresh_request = Some(id.clone());
                                Some((id, record_id(intent)))
                            }
                        };
                        if let Some((refresh, record)) = original {
                            let result = n
                                .call(
                                    refresh.clone(),
                                    Command::RefreshHandyBrowserPlaybackTiming {
                                        play_request_id: request.clone(),
                                        play_record_id: record.clone(),
                                        browser_timing_binding_id: timing.binding_id.clone(),
                                        allow_online_inspection: true,
                                        owner_proof: n.proof.clone(),
                                    },
                                    None,
                                    Instant::now() + REFRESH,
                                )
                                .and_then(|body| {
                                    let pulsar_protocol::ResponseBody::HandyContinuousTiming(
                                        report,
                                    ) = body
                                    else {
                                        anyhow::bail!("device timing reply differs");
                                    };
                                    report.validate()?;
                                    ensure!(
                                        report.request_id == refresh
                                            && report.play_request_id == request
                                            && Some(&report.play_record_id) == record.as_ref()
                                            && report.controller.project_id == n.project
                                            && report.controller.controller_id
                                                == n.config.controller_id
                                            && report.controller.acquisition_request_id
                                                == n.config.acquisition_request_id
                                            && report.current,
                                        "device timing subject is not current"
                                    );
                                    Ok(())
                                });
                            if result.is_err() {
                                let mut shared = state(&s);
                                let intent =
                                    shared.intent.as_mut().expect("original intent retained");
                                if !intent.released
                                    && !intent.stop_requested
                                    && !intent.cancelled.load(Ordering::Acquire)
                                {
                                    intent
                                        .issue
                                        .get_or_insert("qualified_device_timing_unconfirmed");
                                    intent.stop_requested = true;
                                }
                            }
                        }
                    }
                    next = Instant::now()
                        + Duration::from_millis(u64::from(n.config.device_refresh_interval_ms));
                }
            });
        let refresher = match refresher {
            Ok(worker) => worker,
            Err(error) => {
                stopping.store(true, Ordering::Release);
                drop(jobs);
                let _ = starter.join();
                let _ = renewer.join();
                return Err(error.into());
            }
        };
        Ok(Self {
            shared,
            jobs: Some(jobs),
            events: completed,
            stopping,
            threads: vec![starter, refresher, renewer],
            cleanup,
        })
    }
    fn enqueue(&self, job: Job) -> Result<()> {
        ensure!(
            self.jobs
                .as_ref()
                .is_some_and(|tx| tx.try_send(job).is_ok()),
            "browser playback lane busy or unavailable"
        );
        Ok(())
    }
    #[allow(clippy::too_many_arguments)] // Each value is a separately captured HTTP/native scope pin.
    pub(crate) fn handle(
        &self,
        path: &str,
        player: &HandyPlayerIdentity,
        cancelled: Arc<AtomicBool>,
        media: Option<BrowserMediaBinding>,
        timing: Option<BrowserTimingBinding>,
        body: &[u8],
        ticket: u64,
    ) -> Result<ControlReply> {
        match path {
            "/player/handy-inspect" => {
                ensure!(body.is_empty(), "inspection body refused");
                self.enqueue(Job::Inspect(Box::new(Inspection {
                    player: player.clone(),
                    media: media.context("prepare an Engine source first")?,
                    timing: timing.context("wait for a fresh browser timing observation")?,
                    ticket,
                    deadline: Instant::now() + RPC,
                })))?;
                Ok(ControlReply::Pending)
            }
            "/player/handy-start" => {
                let consent: Consent = serde_json::from_slice(body)?;
                ensure!(
                    consent.allow_play
                        && consent.acknowledge_current_environment
                        && consent.allow_online_inspection,
                    "explicit environment, play and ongoing online inspection consent required"
                );
                let timing = timing.context("current browser timing required")?;
                let media = media.context("prepared source required")?;
                let value = {
                    let mut shared = state(&self.shared);
                    ensure!(
                        shared.intent.is_none(),
                        "original Start retained; inspect it, do not create a replacement"
                    );
                    let inspected = shared
                        .inspected
                        .as_ref()
                        .context("inspect this exact prepared selection before Start")?;
                    ensure!(
                        inspected.player == *player
                            && inspected.media == media
                            && inspected.timing == timing,
                        "inspected player/source changed"
                    );
                    shared.intent = Some(Intent {
                        request: new_request_id(),
                        player: player.clone(),
                        timing,
                        cancelled,
                        reserved: Instant::now(),
                        dispatched: false,
                        start_finished: false,
                        stop_requested: false,
                        released: false,
                        history: None,
                        owner: None,
                        refresh_request: None,
                        issue: None,
                    });
                    view(shared.intent.as_ref().unwrap())
                };
                if let Err(error) = self.enqueue(Job::Start) {
                    let mut shared = state(&self.shared);
                    let intent = shared.intent.as_mut().unwrap();
                    intent.start_finished = true;
                    intent.stop_requested = true;
                    intent.issue = Some("original_start_not_dispatched");
                    return Err(error);
                }
                Ok(ControlReply::Ready(value))
            }
            "/player/handy-inspect-original" => {
                ensure!(body.is_empty(), "status body refused");
                self.require_original_player(player)?;
                self.enqueue(Job::Status {
                    player: player.clone(),
                    ticket,
                    deadline: Instant::now() + RPC,
                })?;
                Ok(ControlReply::Pending)
            }
            "/player/handy-stop" => {
                ensure!(body.is_empty(), "stop body refused");
                self.require_original_player(player)?;
                self.retire(player);
                Ok(ControlReply::Ready(self.snapshot(player)?))
            }
            "/player/handy-status" => {
                ensure!(body.is_empty(), "status body refused");
                Ok(ControlReply::Ready(self.snapshot(player)?))
            }
            _ => anyhow::bail!("unknown browser playback operation"),
        }
    }
    fn require_original_player(&self, player: &HandyPlayerIdentity) -> Result<()> {
        let shared = state(&self.shared);
        if let Some(intent) = &shared.intent {
            ensure!(intent.player==*player,"original Start {} belongs to a previous player; preserve and inspect that original identity",intent.request);
        }
        Ok(())
    }
    fn snapshot(&self, player: &HandyPlayerIdentity) -> Result<Value> {
        self.require_original_player(player)?;
        Ok(state(&self.shared)
            .intent
            .as_ref()
            .map_or(json!({"configured":true,"state":"not_started"}), view))
    }
    pub(crate) fn retire(&self, player: &HandyPlayerIdentity) {
        let mut shared = state(&self.shared);
        if let Some(intent) = &mut shared.intent {
            if intent.player == *player {
                intent.stop_requested = true;
            }
        }
        if shared
            .inspected
            .as_ref()
            .is_some_and(|s| s.player == *player)
        {
            shared.inspected = None;
        }
    }
    pub(crate) fn poll(&self, retained: Option<&HandyPlayerIdentity>) -> Vec<(u64, Result<Value>)> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            let result = if retained == Some(&event.player) {
                event.result
            } else {
                Err(anyhow::anyhow!(
                    "browser playback scope changed during inspection"
                ))
            };
            out.push((event.ticket, result));
        }
        out
    }
    pub(crate) fn shutdown(&mut self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        if let Some(intent) = &mut state(&self.shared).intent {
            intent.stop_requested = true;
        }
        self.jobs.take();
        let joining = !self.threads.is_empty();
        for worker in self.threads.drain(..) {
            if worker.join().is_err() {
                state(&self.shared).failed_worker = true;
            }
        }
        if joining {
            let outstanding = {
                let shared = state(&self.shared);
                shared
                    .intent
                    .as_ref()
                    .filter(|i| !i.released)
                    .map(|i| (i.request.clone(), i.timing.clone(), record_id(i)))
            };
            if let Some((request, timing, record)) = outstanding {
                let reply = self
                    .cleanup
                    .call(
                        new_request_id(),
                        Command::ReleaseHandyBrowserPlayback {
                            play_request_id: request.clone(),
                            play_record_id: record.clone(),
                            browser_timing_binding_id: timing.binding_id.clone(),
                            owner_proof: self.cleanup.proof.clone(),
                        },
                        None,
                        Instant::now() + FAST,
                    )
                    .and_then(|body| {
                        self.cleanup
                            .check_owner(body, &request, &timing, record.as_ref())
                    });
                if let Ok(report) = reply {
                    if !report.current {
                        let mut shared = state(&self.shared);
                        let intent = shared
                            .intent
                            .as_mut()
                            .expect("original retained during cleanup");
                        intent.owner = Some(report);
                        intent.released = true;
                    }
                }
            }
        }
        let shared = state(&self.shared);
        ensure!(!shared.failed_worker&&shared.intent.as_ref().is_none_or(|i|i.released),"browser playback release unconfirmed; original operation {}; native expiry and stop custody remain",shared.intent.as_ref().map_or(Value::Null,view));
        Ok(())
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(all(test, unix))]
#[path = "browser_handy_playback_tests.rs"]
mod tests;
