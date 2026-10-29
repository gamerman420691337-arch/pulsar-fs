//! Prepared imported bytes and declared time mapping. Read authority is separate from Play.
use super::*;
use crate::session::new_request_id;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};
const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);
const RANGE_TIMEOUT: Duration = Duration::from_secs(30);
const RANGE_SLOTS: usize = 4;
fn identifier() -> AttemptId {
    AttemptId::new(uuid::Uuid::new_v4().to_string()).expect("UUID")
}
fn bearer() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    catalog_generation: AttemptId,
    entry_id: AttemptId,
    presentation_anchor_us: String,
    elapsed_anchor_us: String,
    allow_declared_clock: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionRead {
    selection_id: Option<AttemptId>,
}
fn anchor(value: &str) -> Result<SourceTimestamp> {
    ensure!(
        !value.is_empty() && value.len() <= 19 && value.bytes().all(|c| c.is_ascii_digit()),
        "declared anchor must be nonnegative integer microseconds"
    );
    Ok(SourceTimestamp::new(value.parse::<i64>()?, 1_000_000)?)
}
#[derive(Clone)]
struct Intent {
    selection_id: AttemptId,
    request_id: RequestId,
    engine_epoch: AttemptId,
    project: ProjectId,
    revision: RevisionId,
    selection: BrowserMediaSelection,
    clock: DeclaredSourceClock,
    lease_ms: u32,
    page: String,
    page_cancelled: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    dispatched: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    expires: Instant,
    predecessor: Option<BrowserMediaBinding>,
}
struct Prepared {
    intent: Intent,
    report: Option<BrowserMediaPreparationReport>,
    ready: Option<Arc<ReadySource>>,
    pending: usize,
    release_pending: bool,
    cleanup_attempted: bool,
    error: Option<&'static str>,
}
#[derive(Clone)]
struct ReadySource {
    intent: Intent,
    binding: BrowserMediaBinding,
    token: String,
    mime: &'static str,
}
impl ReadySource {
    fn current(&self) -> bool {
        Instant::now() < self.intent.expires
            && !self.intent.cancelled.load(Ordering::Acquire)
            && !self.intent.page_cancelled.load(Ordering::Acquire)
            && !self.intent.released.load(Ordering::Acquire)
    }
}
struct Catalog {
    page: String,
    generation: AttemptId,
    value: BrowserMediaCatalog,
    entries: BTreeMap<String, BrowserMediaSelection>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpKind {
    Prepare,
    Status,
    Release,
}
enum Operation {
    Catalog {
        ticket: u64,
        page: String,
        page_cancelled: Arc<AtomicBool>,
    },
    Preparation {
        ticket: Option<u64>,
        intent: Box<Intent>,
        kind: OpKind,
    },
}
enum Payload {
    Catalog(BrowserMediaCatalog),
    LocalRelease,
    Preparation {
        report: Box<BrowserMediaPreparationReport>,
        mime: Option<&'static str>,
        kind: OpKind,
    },
}
struct Event {
    ticket: Option<u64>,
    page: String,
    selection_id: Option<AttemptId>,
    request_id: Option<RequestId>,
    result: Result<Payload>,
}
struct RangeJob {
    stream: TcpStream,
    source: Arc<ReadySource>,
    route: Arc<Mutex<Option<Arc<ReadySource>>>>,
    head: bool,
    range: Option<String>,
    if_range: Option<String>,
}
pub(super) enum MediaReply {
    Ready(serde_json::Value),
    Pending,
}
pub(super) struct MediaController {
    project: ProjectId,
    revision: RevisionId,
    end: Instant,
    lease_cap_ms: u32,
    catalog: Option<Catalog>,
    prepared: Option<Prepared>,
    successor: Option<Prepared>,
    retired: Vec<Prepared>,
    route: Arc<Mutex<Option<Arc<ReadySource>>>>,
    operations: Option<mpsc::SyncSender<Operation>>,
    events: mpsc::Receiver<Event>,
    ranges: Vec<mpsc::SyncSender<RangeJob>>,
    threads: Vec<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    range_slots: Arc<AtomicUsize>,
    next_range: AtomicUsize,
    cleanup: SessionClient,
    shutdown_done: bool,
    shutdown_error: Option<String>,
}
fn checked_report(
    reply: pulsar_protocol::ResponseBody,
    intent: &Intent,
) -> Result<BrowserMediaPreparationReport> {
    let pulsar_protocol::ResponseBody::BrowserMediaPreparation(report) = reply else {
        anyhow::bail!("unexpected media preparation response")
    };
    report.validate()?;
    ensure!(
        report.request_id == intent.request_id
            && report.project_id == intent.project
            && report.revision == intent.revision
            && report.engine_epoch == intent.engine_epoch
            && report.selection == intent.selection
            && report.clock == intent.clock
            && report.lease_ms == intent.lease_ms
            && intent.predecessor.as_ref().is_none_or(|old| report
                .binding
                .as_ref()
                .is_none_or(|new| new.lease_id != old.lease_id
                    && new.preparation_request_id != old.preparation_request_id)),
        "media preparation response changed original subject"
    );
    Ok(*report)
}
fn call_original(
    client: &SessionClient,
    intent: &Intent,
    kind: OpKind,
) -> Result<BrowserMediaPreparationReport> {
    let (id, command, revision) = match kind {
        OpKind::Prepare => (
            intent.request_id.clone(),
            match &intent.predecessor {
                Some(predecessor) => Command::PrepareBrowserMediaSuccessor {
                    predecessor: predecessor.clone(),
                    lease_ms: intent.lease_ms,
                },
                None => Command::PrepareBrowserMedia {
                    engine_epoch: intent.engine_epoch.clone(),
                    selection: intent.selection.clone(),
                    clock: intent.clock,
                    lease_ms: intent.lease_ms,
                    allow_declared_clock: true,
                },
            },
            Some(intent.revision),
        ),
        OpKind::Status => (
            new_request_id(),
            Command::BrowserMediaPreparationStatus {
                original_request_id: intent.request_id.clone(),
            },
            None,
        ),
        OpKind::Release => (
            new_request_id(),
            Command::ReleaseBrowserMedia {
                original_request_id: intent.request_id.clone(),
            },
            None,
        ),
    };
    if kind == OpKind::Prepare {
        intent.dispatched.store(true, Ordering::Release);
    }
    let report = checked_report(
        client.browser_media_call_until(
            id,
            command,
            intent.project.clone(),
            revision,
            Instant::now() + CONTROL_TIMEOUT,
        )?,
        intent,
    )?;
    if kind == OpKind::Release {
        ensure!(
            !report.current && report.state != BrowserMediaPreparationState::Pending,
            "source release not confirmed"
        );
        intent.released.store(true, Ordering::Release);
    }
    Ok(report)
}
fn media_mime(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        && bytes.windows(4).take(4096).any(|p| p == b"webm")
    {
        return Ok("video/webm");
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1" | b"M4V "
        )
    {
        return Ok("video/mp4");
    }
    anyhow::bail!("source container is not admitted for browser media")
}
fn selected_media_lease(end: Instant, now: Instant, cap_ms: u32) -> Result<u32> {
    ensure!(
        (1..=BROWSER_MEDIA_MAX_LEASE_MS).contains(&cap_ms),
        "media lease cap must be 1..=600000 ms"
    );
    let remaining = end
        .checked_duration_since(now)
        .context("source launch expired")?
        .as_millis();
    let selected = u32::try_from(remaining.min(u128::from(cap_ms)))?;
    ensure!(selected > 0, "source launch expired");
    Ok(selected)
}
impl MediaController {
    pub fn new(
        client: &SessionClient,
        project: ProjectId,
        revision: RevisionId,
        duration: u16,
        media_lease_ms: Option<u32>,
    ) -> Result<Self> {
        let lease_cap_ms = media_lease_ms.unwrap_or(BROWSER_MEDIA_MAX_LEASE_MS);
        ensure!(
            (1..=BROWSER_MEDIA_MAX_LEASE_MS).contains(&lease_cap_ms),
            "media lease cap must be 1..=600000 ms"
        );
        let end = Instant::now() + Duration::from_secs(u64::from(duration));
        let stop = Arc::new(AtomicBool::new(false));
        let (operations, rx) = mpsc::sync_channel::<Operation>(4);
        let (tx, events) = mpsc::channel();
        let native = client.fork_native();
        let stop_worker = stop.clone();
        let project_worker = project.clone();
        let worker=thread::Builder::new().name("pulsar-browser-source".into()).spawn(move || {
            let mut mime_cache=BTreeMap::<String,(BrowserMediaBinding,&'static str)>::new();
            while let Ok(operation)=rx.recv() {
                let (ticket,page,selection_id,request_id,result)=match operation {
                    Operation::Catalog {ticket,page,page_cancelled}=>{
                        let result=(||->Result<Payload>{ensure!(!stop_worker.load(Ordering::Acquire)&&!page_cancelled.load(Ordering::Acquire),"source catalog cancelled");
                            let pulsar_protocol::ResponseBody::BrowserMediaCatalog(catalog)=native.browser_media_call_until(new_request_id(),Command::BrowserMediaCatalog,project_worker.clone(),Some(revision),Instant::now()+CONTROL_TIMEOUT)? else {anyhow::bail!("unexpected media catalog reply")};
                            catalog.validate()?;ensure!(catalog.project_id==project_worker&&catalog.revision==revision,"media catalog scope differs");Ok(Payload::Catalog(*catalog))})();
                        (Some(ticket),page,None,None,result)
                    },
                    Operation::Preparation {ticket,intent,kind}=>{
                        let result=(||->Result<Payload>{
                            if kind==OpKind::Prepare {ensure!(!stop_worker.load(Ordering::Acquire)&&!intent.cancelled.load(Ordering::Acquire)&&!intent.page_cancelled.load(Ordering::Acquire),"source preparation cancelled");}
                            if kind==OpKind::Release && !intent.dispatched.load(Ordering::Acquire) {intent.released.store(true,Ordering::Release);return Ok(Payload::LocalRelease);}
                            let report=call_original(&native,&intent,kind)?;
                            let mime=if kind!=OpKind::Release && report.state==BrowserMediaPreparationState::Ready && report.current && !intent.cancelled.load(Ordering::Acquire) {
                                let binding=report.binding.as_ref().context("ready binding absent")?;
                                if let Some((original,mime))=mime_cache.get(intent.request_id.as_str()) {ensure!(original==binding,"source binding changed after preparation");Some(*mime)}
                                else {ensure!(mime_cache.len()<8,"source media header cache bound");let mut bulk=native.open_source_media_until(binding,Instant::now()+CONTROL_TIMEOUT)?;
                                    let prefix=bulk.download(0,u32::try_from(binding.selection.source.byte_len.min(65536))?)?;let mime=media_mime(&prefix)?;mime_cache.insert(intent.request_id.to_string(),(binding.clone(),mime));Some(mime)}
                            }else{None};
                            Ok(Payload::Preparation {report:Box::new(report),mime,kind})
                        })();
                        // A late Ready never publishes a URL after a local release or shutdown.
                        if kind!=OpKind::Release && intent.dispatched.load(Ordering::Acquire) && (intent.cancelled.load(Ordering::Acquire)||intent.page_cancelled.load(Ordering::Acquire)||stop_worker.load(Ordering::Acquire)) && !intent.released.load(Ordering::Acquire) {let _=call_original(&native,&intent,OpKind::Release);}
                        if intent.released.load(Ordering::Acquire){mime_cache.remove(intent.request_id.as_str());}
                        (ticket,intent.page.clone(),Some(intent.selection_id.clone()),Some(intent.request_id.clone()),result)
                    }
                };
                let result=result.map_err(|_|anyhow::anyhow!("source operation unconfirmed; inspect or release the original preparation"));
                let _=tx.send(Event {ticket,page,selection_id,request_id,result});
            }
        })?;
        let slots = Arc::new(AtomicUsize::new(0));
        let mut threads = vec![worker];
        let mut ranges = Vec::new();
        for ordinal in 0..2 {
            let (sender, receiver) = mpsc::sync_channel::<RangeJob>(1);
            let native = client.fork_native();
            let occupied = slots.clone();
            let stopping = stop.clone();
            match thread::Builder::new()
                .name(format!("pulsar-browser-range-{ordinal}"))
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        let _ = serve_range(&native, job, &stopping);
                        occupied.fetch_sub(1, Ordering::AcqRel);
                    }
                }) {
                Ok(thread) => {
                    threads.push(thread);
                    ranges.push(sender);
                }
                Err(error) => {
                    stop.store(true, Ordering::Release);
                    drop(operations);
                    drop(ranges);
                    for thread in threads {
                        let _ = thread.join();
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(Self {
            project,
            revision,
            end,
            lease_cap_ms,
            catalog: None,
            prepared: None,
            successor: None,
            retired: Vec::new(),
            route: Arc::new(Mutex::new(None)),
            operations: Some(operations),
            events,
            ranges,
            threads,
            stop,
            range_slots: slots,
            next_range: AtomicUsize::new(0),
            cleanup: client.fork_native(),
            shutdown_done: false,
            shutdown_error: None,
        })
    }
    fn queue(&self, operation: Operation) -> Result<()> {
        self.operations
            .as_ref()
            .context("source worker stopping")?
            .try_send(operation)
            .map_err(|_| anyhow::anyhow!("source operation queue full or stopped"))
    }
    fn view(prepared: &Prepared) -> Result<serde_json::Value> {
        let released = prepared.intent.released.load(Ordering::Acquire);
        let current =
            prepared.ready.as_ref().is_some_and(|r| r.current()) && prepared.error.is_none();
        let state = if released {
            "cancelled"
        } else if prepared.error.is_some() {
            "failed"
        } else if current {
            "ready"
        } else {
            match prepared.report.as_ref().map(|r| r.state) {
                Some(BrowserMediaPreparationState::Ready) => "ready",
                Some(BrowserMediaPreparationState::Failed) => "failed",
                Some(BrowserMediaPreparationState::Cancelled) => "cancelled",
                _ => "pending",
            }
        };
        let mut value = serde_json::json!({"selection_id":prepared.intent.selection_id,"preparation_request_id":prepared.intent.request_id,"state":state,"current":current,"error":prepared.error});
        if current {
            let ready = prepared.ready.as_ref().expect("current ready source");
            let (start, end) = ready
                .binding
                .clock
                .presentation_trim_micros(&ready.binding.selection.placement.spec)?;
            value["url"] = serde_json::json!(format!("/media/{}", ready.token));
            value["trim_start_us"] = serde_json::json!(start);
            value["trim_end_us"] = serde_json::json!(end);
        }
        Ok(value)
    }
    pub fn handle(
        &mut self,
        path: &str,
        scope: &Scope,
        page_cancelled: Arc<AtomicBool>,
        body: &[u8],
        ticket: u64,
    ) -> Result<MediaReply> {
        if matches!(path, "/media/continue" | "/media/continue-status") {
            let input: SelectionRead = serde_json::from_slice(body)?;
            let old = self
                .prepared
                .as_ref()
                .context("active media preparation absent")?;
            ensure!(
                old.intent.page == scope.page
                    && input.selection_id.as_ref() == Some(&old.intent.selection_id),
                "media continuation scope differs"
            );
            ensure!(
                old.ready.as_ref().is_some_and(|r| r.current()) && old.error.is_none(),
                "media predecessor expired"
            );
            if path == "/media/continue" && self.successor.is_none() {
                ensure!(
                    self.retired.len() < 4,
                    "media predecessor cleanup must settle before continuation"
                );
                let admitted_before = Instant::now();
                let lease_ms = selected_media_lease(self.end, admitted_before, self.lease_cap_ms)?;
                let mut intent = old.intent.clone();
                intent.predecessor =
                    Some(old.ready.as_ref().expect("checked ready").binding.clone());
                intent.request_id = new_request_id();
                intent.lease_ms = lease_ms;
                intent.cancelled = Arc::new(AtomicBool::new(false));
                intent.dispatched = Arc::new(AtomicBool::new(false));
                intent.released = Arc::new(AtomicBool::new(false));
                intent.expires = admitted_before
                    .checked_add(Duration::from_millis(u64::from(lease_ms)))
                    .context("media successor deadline overflow")?;
                self.successor = Some(Prepared {
                    intent: intent.clone(),
                    report: None,
                    ready: None,
                    pending: 1,
                    release_pending: false,
                    cleanup_attempted: false,
                    error: None,
                });
                if let Err(error) = self.queue(Operation::Preparation {
                    ticket: Some(ticket),
                    intent: Box::new(intent),
                    kind: OpKind::Prepare,
                }) {
                    let next = self.successor.as_mut().expect("retained successor");
                    next.pending = 0;
                    next.error = Some("successor dispatch unconfirmed");
                    return Err(error);
                }
                return Ok(MediaReply::Pending);
            }
            let next = self
                .successor
                .as_mut()
                .context("no original media successor")?;
            if next.pending > 0 || next.ready.as_ref().is_some_and(|r| r.current()) {
                return Ok(MediaReply::Ready(Self::view(next)?));
            }
            next.pending += 1;
            let intent = next.intent.clone();
            if let Err(error) = self.queue(Operation::Preparation {
                ticket: Some(ticket),
                intent: Box::new(intent),
                kind: OpKind::Status,
            }) {
                self.successor.as_mut().expect("retained successor").pending -= 1;
                return Err(error);
            }
            return Ok(MediaReply::Pending);
        }
        if path == "/media/catalog" {
            ensure!(body.is_empty(), "catalog body refused");
            self.queue(Operation::Catalog {
                ticket,
                page: scope.page.clone(),
                page_cancelled,
            })?;
            return Ok(MediaReply::Pending);
        }
        if path == "/media/prepare" {
            ensure!(
                self.prepared
                    .as_ref()
                    .is_none_or(|p| p.intent.released.load(Ordering::Acquire)),
                "release or inspect the retained original source preparation before replacement"
            );
            let input: Prepare = serde_json::from_slice(body)?;
            ensure!(
                input.allow_declared_clock,
                "explicit declared-clock acknowledgement is required"
            );
            let catalog = self
                .catalog
                .as_ref()
                .context("inspect the Engine media catalog first")?;
            ensure!(
                catalog.page == scope.page && catalog.generation == input.catalog_generation,
                "source catalog generation differs"
            );
            let selection = catalog
                .entries
                .get(input.entry_id.as_str())
                .context("source catalog entry absent")?
                .clone();
            let clock = DeclaredSourceClock {
                presentation_anchor: anchor(&input.presentation_anchor_us)?,
                elapsed_anchor: anchor(&input.elapsed_anchor_us)?,
            };
            clock.validate()?;
            let admitted_before = Instant::now();
            let lease_ms = selected_media_lease(self.end, admitted_before, self.lease_cap_ms)?;
            let intent = Intent {
                selection_id: identifier(),
                request_id: new_request_id(),
                engine_epoch: catalog.value.engine_epoch.clone(),
                project: self.project.clone(),
                revision: self.revision,
                selection,
                clock,
                lease_ms,
                page: scope.page.clone(),
                page_cancelled,
                cancelled: Arc::new(AtomicBool::new(false)),
                dispatched: Arc::new(AtomicBool::new(false)),
                released: Arc::new(AtomicBool::new(false)),
                expires: admitted_before
                    .checked_add(Duration::from_millis(u64::from(lease_ms)))
                    .context("source deadline overflow")?,
                predecessor: None,
            };
            pulsar_protocol::Request::new(
                intent.request_id.clone(),
                Command::PrepareBrowserMedia {
                    engine_epoch: intent.engine_epoch.clone(),
                    selection: intent.selection.clone(),
                    clock: intent.clock,
                    lease_ms,
                    allow_declared_clock: true,
                },
            )
            .in_project(intent.project.clone(), Some(intent.revision))
            .validate()?;
            ensure!(
                self.successor.is_none() || self.retired.len() < 4,
                "media predecessor cleanup must settle before replacement"
            );
            // A new original preparation cannot inherit an earlier parent's
            // successor. Keep its pending replies and exact native cleanup
            // obligation addressable until retirement actually settles.
            if let Some(previous) = self.successor.take() {
                previous.intent.cancelled.store(true, Ordering::Release);
                self.retired.push(previous);
            }
            self.prepared = Some(Prepared {
                intent: intent.clone(),
                report: None,
                ready: None,
                pending: 1,
                release_pending: false,
                cleanup_attempted: false,
                error: None,
            });
            if let Err(error) = self.queue(Operation::Preparation {
                ticket: Some(ticket),
                intent: Box::new(intent),
                kind: OpKind::Prepare,
            }) {
                let p = self.prepared.as_mut().expect("reserved");
                p.pending = 0;
                p.error = Some("source preparation was not dispatched");
                return Err(error);
            }
            return Ok(MediaReply::Pending);
        }
        let input: SelectionRead = serde_json::from_slice(body)?;
        if path == "/media/release"
            && input.selection_id.is_none()
            && self
                .prepared
                .as_ref()
                .is_none_or(|prepared| prepared.intent.page != scope.page)
        {
            // Explicit recovery after a rejected/lost Prepare must not strand
            // the page, and must never release another page's preparation.
            return Ok(MediaReply::Ready(serde_json::json!({
                "selection_id":null,"state":"cancelled","current":false,"not_dispatched":true
            })));
        }
        let prepared = self
            .prepared
            .as_mut()
            .context("this page has no original source preparation")?;
        ensure!(
            prepared.intent.page == scope.page
                && input
                    .selection_id
                    .as_ref()
                    .is_none_or(|id| id == &prepared.intent.selection_id),
            "source selection belongs to another preparation"
        );
        match path {
            "/media/status" => {
                if prepared.pending > 0 || prepared.intent.released.load(Ordering::Acquire) {
                    return Ok(MediaReply::Ready(Self::view(prepared)?));
                }
                if !prepared.intent.dispatched.load(Ordering::Acquire) {
                    return Ok(MediaReply::Ready(Self::view(prepared)?));
                }
            }
            "/media/release" => {
                if let Some(next) = &self.successor {
                    next.intent.cancelled.store(true, Ordering::Release);
                }
                for old in &self.retired {
                    old.intent.cancelled.store(true, Ordering::Release);
                }
                prepared.intent.cancelled.store(true, Ordering::Release);
                prepared.ready = None;
                if prepared.release_pending {
                    return Ok(MediaReply::Ready(Self::view(prepared)?));
                }
                if !prepared.intent.dispatched.load(Ordering::Acquire) && prepared.pending == 0 {
                    prepared.intent.released.store(true, Ordering::Release);
                    return Ok(MediaReply::Ready(Self::view(prepared)?));
                }
                if prepared.intent.released.load(Ordering::Acquire) {
                    return Ok(MediaReply::Ready(Self::view(prepared)?));
                }
                prepared.release_pending = true;
                prepared.cleanup_attempted = true;
            }
            _ => anyhow::bail!("unknown source operation"),
        }
        let kind = if path == "/media/release" {
            OpKind::Release
        } else {
            OpKind::Status
        };
        let intent = prepared.intent.clone();
        prepared.pending += 1;
        if let Err(error) = self.queue(Operation::Preparation {
            ticket: Some(ticket),
            intent: Box::new(intent),
            kind,
        }) {
            let p = self.prepared.as_mut().unwrap();
            p.pending -= 1;
            p.release_pending = false;
            p.error = Some("source operation queue unavailable; original preparation retained");
            return Err(error);
        }
        Ok(MediaReply::Pending)
    }
    pub fn poll(&mut self) -> Vec<(u64, Result<serde_json::Value>)> {
        let mut output = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            let stable_token = self
                .prepared
                .as_ref()
                .and_then(|p| p.ready.as_ref())
                .map(|r| r.token.clone());
            let result = match event.result {
                Err(error) if event.selection_id.is_none() => Err(error),
                Ok(Payload::Catalog(catalog)) => {
                    let generation = identifier();
                    let mut entries = BTreeMap::new();
                    let mut view = Vec::new();
                    for entry in &catalog.entries {
                        let id = identifier();
                        entries.insert(id.to_string(), entry.selection.clone());
                        view.push(serde_json::json!({"entry_id":id,"label":entry.label,"source_version":entry.selection.source_version,"placement_id":entry.selection.placement.placement_id,"sha256":entry.selection.source.sha256,"byte_len":entry.selection.source.byte_len.to_string()}));
                    }
                    self.catalog = Some(Catalog {
                        page: event.page,
                        generation: generation.clone(),
                        value: catalog,
                        entries,
                    });
                    Ok(serde_json::json!({"catalog_generation":generation,"entries":view}))
                }
                result => {
                    let Some(p) = self
                        .prepared
                        .iter_mut()
                        .chain(self.successor.iter_mut())
                        .chain(self.retired.iter_mut())
                        .find(|p| {
                            Some(&p.intent.request_id) == event.request_id.as_ref()
                                && Some(&p.intent.selection_id) == event.selection_id.as_ref()
                        })
                    else {
                        if let Some(ticket) = event.ticket {
                            output.push((
                                ticket,
                                Err(anyhow::anyhow!("source preparation was replaced")),
                            ));
                        }
                        continue;
                    };
                    p.pending = p.pending.saturating_sub(1);
                    match result {
                        Ok(Payload::LocalRelease) => {
                            p.release_pending = false;
                            p.error = None;
                            p.ready = None;
                            Self::view(p)
                        }
                        Ok(Payload::Preparation { report, mime, kind }) => {
                            if kind == OpKind::Release {
                                p.release_pending = false;
                                p.ready = None;
                                p.error = None;
                            } else if report.state == BrowserMediaPreparationState::Ready
                                && report.current
                                && !p.intent.cancelled.load(Ordering::Acquire)
                                && !p.intent.page_cancelled.load(Ordering::Acquire)
                                && Instant::now() < p.intent.expires
                            {
                                if let (Some(binding), Some(mime)) = (&report.binding, mime) {
                                    if p.ready.as_ref().is_some_and(|r| r.binding != *binding) {
                                        p.intent.cancelled.store(true, Ordering::Release);
                                        p.error = Some("source binding changed during inspection");
                                        p.ready = None;
                                    } else if p.ready.is_none() {
                                        p.ready = Some(Arc::new(ReadySource {
                                            intent: p.intent.clone(),
                                            binding: binding.clone(),
                                            token: stable_token.clone().unwrap_or_else(bearer),
                                            mime,
                                        }));
                                        p.error = None;
                                    }
                                }
                            } else if !report.current
                                || matches!(
                                    report.state,
                                    BrowserMediaPreparationState::Failed
                                        | BrowserMediaPreparationState::Cancelled
                                )
                            {
                                if p.ready.is_some() {
                                    p.intent.cancelled.store(true, Ordering::Release);
                                }
                                p.ready = None;
                            }
                            p.report = Some(*report);
                            Self::view(p)
                        }
                        Err(_) => {
                            p.release_pending = false;
                            p.error=Some("source operation unconfirmed; inspect or release original preparation");
                            Err(anyhow::anyhow!("source operation unconfirmed; original preparation {} selection {}",p.intent.request_id,p.intent.selection_id))
                        }
                        Ok(Payload::Catalog(_)) => unreachable!(),
                    }
                }
            };
            if let Some(ticket) = event.ticket {
                output.push((ticket, result));
            }
        }
        if let Ok(mut route) = self.route.lock() {
            *route = self.prepared.as_ref().and_then(|p| p.ready.clone());
        }
        output
    }
    pub fn cancel_successor(&mut self) {
        if let Some(next) = &self.successor {
            next.intent.cancelled.store(true, Ordering::Release);
        }
        self.tick();
    }
    pub fn successor_binding(
        &self,
        predecessor: &BrowserMediaBinding,
    ) -> Result<BrowserMediaBinding> {
        ensure!(
            self.binding_current(predecessor),
            "media predecessor not current"
        );
        let next = self.successor.as_ref().context("media successor absent")?;
        ensure!(
            next.intent.predecessor.as_ref() == Some(predecessor) && next.error.is_none(),
            "media successor link differs"
        );
        let ready = next
            .ready
            .as_ref()
            .filter(|r| r.current())
            .context("media successor not ready")?;
        ensure!(
            ready.binding != *predecessor
                && ready.binding.selection == predecessor.selection
                && ready.binding.clock == predecessor.clock,
            "media successor bytes or mapping differ"
        );
        Ok(ready.binding.clone())
    }
    pub fn check_successor(
        &self,
        old: &BrowserMediaBinding,
        new: &BrowserMediaBinding,
    ) -> Result<()> {
        ensure!(
            self.successor_binding(old)? == *new
                && self.successor.as_ref().is_some_and(|p| p.pending == 0),
            "media transfer not settled"
        );
        Ok(())
    }
    /// Only a confirmed native capture transfer may change this local route.
    pub fn commit_successor(
        &mut self,
        old: &BrowserMediaBinding,
        new: &BrowserMediaBinding,
    ) -> Result<()> {
        self.check_successor(old, new)?;
        let next = self.successor.take().expect("checked successor");
        let prior = self.prepared.replace(next).expect("checked predecessor");
        self.retired.push(prior);
        *self
            .route
            .lock()
            .map_err(|_| anyhow::anyhow!("media route poisoned"))? =
            self.prepared.as_ref().and_then(|p| p.ready.clone());
        Ok(())
    }
    pub fn tick(&mut self) {
        let mut releases = Vec::new();
        for p in &mut self.retired {
            if p.ready
                .as_ref()
                .is_none_or(|ready| Arc::strong_count(ready) == 1)
            {
                p.intent.cancelled.store(true, Ordering::Release);
            }
        }
        for p in self
            .prepared
            .iter_mut()
            .chain(self.successor.iter_mut())
            .chain(self.retired.iter_mut())
        {
            if Instant::now() >= p.intent.expires || p.intent.page_cancelled.load(Ordering::Acquire)
            {
                p.intent.cancelled.store(true, Ordering::Release);
            }
            if p.intent.cancelled.load(Ordering::Acquire)
                && !p.intent.released.load(Ordering::Acquire)
                && !p.release_pending
                && !p.cleanup_attempted
                && p.pending == 0
            {
                // The worker resolves an unissued intent locally; it still
                // needs a confirmed release to clear its retained cleanup slot.
                p.ready = None;
                p.release_pending = true;
                p.cleanup_attempted = true;
                p.pending += 1;
                releases.push(p.intent.clone());
            }
        }
        for intent in releases {
            let id = intent.request_id.clone();
            if self
                .queue(Operation::Preparation {
                    ticket: None,
                    intent: Box::new(intent),
                    kind: OpKind::Release,
                })
                .is_err()
            {
                if let Some(p) = self
                    .prepared
                    .iter_mut()
                    .chain(self.successor.iter_mut())
                    .chain(self.retired.iter_mut())
                    .find(|p| p.intent.request_id == id)
                {
                    p.pending -= 1;
                    p.release_pending = false;
                    p.error = Some("source cleanup queue unavailable");
                }
            }
        }
        self.retired
            .retain(|p| !p.intent.released.load(Ordering::Acquire) || p.pending > 0);
    }
    pub fn resolve_player(
        &self,
        scope: &Scope,
        body: &[u8],
    ) -> Result<Option<BrowserMediaBinding>> {
        #[derive(Deserialize)]
        struct Selected {
            media_selection_id: Option<AttemptId>,
            source_id: AttemptId,
        }
        let selected: Selected = serde_json::from_slice(body)?;
        let Some(id) = selected.media_selection_id else {
            return Ok(None);
        };
        let p = self.prepared.as_ref().context("prepared source absent")?;
        ensure!(
            p.intent.page == scope.page && p.intent.selection_id == id && selected.source_id == id,
            "player names another prepared source"
        );
        let source = p.ready.as_ref().context("source is not ready")?;
        ensure!(
            source.current() && p.error.is_none(),
            "prepared source expired or retired"
        );
        Ok(Some(source.binding.clone()))
    }
    pub fn binding_current(&self, binding: &BrowserMediaBinding) -> bool {
        self.prepared
            .as_ref()
            .and_then(|p| p.ready.as_ref())
            .is_some_and(|r| r.binding == *binding && r.current())
    }
    pub fn binding_expires(&self, binding: &BrowserMediaBinding) -> Option<Instant> {
        self.prepared
            .iter()
            .chain(self.successor.iter())
            .filter_map(|p| p.ready.as_ref())
            .find(|r| r.binding == *binding && r.current())
            .map(|r| r.intent.expires)
    }
    pub fn submit_range(
        &self,
        stream: &mut Option<TcpStream>,
        path: &str,
        head: bool,
        range: Option<String>,
        if_range: Option<String>,
    ) -> Result<()> {
        let token = path.strip_prefix("/media/").context("media path absent")?;
        ensure!(
            token.len() == 64
                && token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "media token malformed"
        );
        let source = self
            .prepared
            .as_ref()
            .and_then(|p| p.ready.as_ref())
            .filter(|r| r.token == token && r.current())
            .context("media read authority unavailable")?
            .clone();
        ensure!(stream.is_some(), "media socket absent");
        let mut occupied = self.range_slots.load(Ordering::Acquire);
        loop {
            ensure!(occupied < RANGE_SLOTS, "media connection admission full");
            let next = occupied
                .checked_add(1)
                .context("media connection count overflow")?;
            match self.range_slots.compare_exchange_weak(
                occupied,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => occupied = actual,
            }
        }
        let mut job = RangeJob {
            stream: stream.take().context("media socket absent")?,
            source,
            route: self.route.clone(),
            head,
            range,
            if_range,
        };
        let first = self.next_range.fetch_add(1, Ordering::AcqRel);
        for index in 0..self.ranges.len() {
            let sender = &self.ranges[first.wrapping_add(index) % self.ranges.len()];
            match sender.try_send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Full(value)) => job = value,
                Err(mpsc::TrySendError::Disconnected(value)) => job = value,
            }
        }
        self.range_slots.fetch_sub(1, Ordering::AcqRel);
        *stream = Some(job.stream);
        anyhow::bail!("media range workers busy")
    }
    pub fn shutdown(&mut self) -> Result<()> {
        if self.shutdown_done {
            if let Some(error) = &self.shutdown_error {
                anyhow::bail!("{error}");
            }
            return Ok(());
        }
        self.shutdown_done = true;
        self.stop.store(true, Ordering::Release);
        for p in self
            .prepared
            .iter()
            .chain(self.successor.iter())
            .chain(self.retired.iter())
        {
            p.intent.cancelled.store(true, Ordering::Release);
        }
        self.operations.take();
        self.ranges.clear();
        let mut panicked = false;
        for t in self.threads.drain(..) {
            panicked |= t.join().is_err();
        }
        let mut failure = None;
        for p in self
            .prepared
            .iter()
            .chain(self.successor.iter())
            .chain(self.retired.iter())
        {
            if p.intent.dispatched.load(Ordering::Acquire)
                && !p.intent.released.load(Ordering::Acquire)
                && call_original(&self.cleanup, &p.intent, OpKind::Release).is_err()
            {
                failure = Some(format!(
                    "source cleanup unconfirmed; original preparation {} selection {}",
                    p.intent.request_id, p.intent.selection_id
                ));
            }
        }
        if panicked {
            failure = Some(match failure {
                Some(error) => format!("{error}; source media worker panicked"),
                None => "source media worker panicked".into(),
            });
        }
        self.shutdown_error = failure;
        if let Some(error) = &self.shutdown_error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}
impl Drop for MediaController {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn byte_range(value: Option<&str>, length: u64) -> Result<Option<(u64, u64)>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let range = value
        .strip_prefix("bytes=")
        .context("unsupported range unit")?;
    ensure!(
        !range.contains(',') && !range.bytes().any(|b| b.is_ascii_whitespace()),
        "single byte range required"
    );
    let (first, last) = range.split_once('-').context("malformed byte range")?;
    ensure!(
        first.bytes().all(|b| b.is_ascii_digit()) && last.bytes().all(|b| b.is_ascii_digit()),
        "malformed byte range digits"
    );
    if first.is_empty() {
        let suffix = last.parse::<u64>()?;
        ensure!(suffix > 0 && length > 0, "unsatisfiable byte range");
        return Ok(Some((length.saturating_sub(suffix), length - 1)));
    }
    let start = first.parse::<u64>()?;
    let end = if last.is_empty() {
        length.checked_sub(1).context("empty source")?
    } else {
        last.parse::<u64>()?.min(length.saturating_sub(1))
    };
    ensure!(start < length && start <= end, "unsatisfiable byte range");
    Ok(Some((start, end)))
}
fn write_until(
    stream: &mut TcpStream,
    bytes: &[u8],
    source: &ReadySource,
    stop: &AtomicBool,
    deadline: Instant,
) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        ensure!(
            source.current() && !stop.load(Ordering::Acquire) && Instant::now() < deadline,
            "media delivery retired"
        );
        match stream.write(&bytes[offset..]) {
            Ok(0) => anyhow::bail!("media peer closed"),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn selected_range_source(
    route: &Mutex<Option<Arc<ReadySource>>>,
    previous: &ReadySource,
) -> Result<Arc<ReadySource>> {
    let selected = route
        .lock()
        .map_err(|_| anyhow::anyhow!("media route poisoned"))?
        .as_ref()
        .filter(|r| r.current())
        .cloned()
        .context("media delivery route retired")?;
    ensure!(
        selected.binding.selection == previous.binding.selection
            && selected.binding.clock == previous.binding.clock
            && selected.binding.project_id == previous.binding.project_id
            && selected.binding.revision == previous.binding.revision
            && selected.binding.engine_epoch == previous.binding.engine_epoch
            && selected.token == previous.token
            && selected.mime == previous.mime,
        "media range successor differs from the original response"
    );
    Ok(selected)
}
fn serve_range(client: &SessionClient, mut job: RangeJob, stop: &AtomicBool) -> Result<()> {
    let mut source = job.source;
    // The HTTP response keeps its original operation budget. Only a future
    // chunk may select the newly committed native media capability.
    let response_deadline = Instant::now() + RANGE_TIMEOUT;
    let mut deadline = response_deadline.min(source.intent.expires);
    ensure!(
        source.current() && !stop.load(Ordering::Acquire),
        "source read retired"
    );
    let length = source.binding.selection.source.byte_len;
    let etag = format!("\"{}\"", source.binding.selection.source.sha256);
    let selected = if job.head || job.if_range.as_ref().is_some_and(|v| v != &etag) {
        Ok(None)
    } else {
        byte_range(job.range.as_deref(), length)
    };
    let (status, start, end, content_range) = match selected {
        Ok(Some((start, end))) => (
            206,
            start,
            end,
            format!("Content-Range: bytes {start}-{end}/{length}\r\n"),
        ),
        Ok(None) => (200, 0, length - 1, String::new()),
        Err(_) => {
            let header=format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\nContent-Length: 0\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n");
            return write_until(&mut job.stream, header.as_bytes(), &source, stop, deadline);
        }
    };
    let mut bulk = match client.open_source_media_until(
        &source.binding,
        (Instant::now() + CONTROL_TIMEOUT).min(deadline),
    ) {
        Ok(bulk) => bulk,
        Err(error) => {
            source.intent.cancelled.store(true, Ordering::Release);
            return Err(error);
        }
    };
    let count = end
        .checked_sub(start)
        .and_then(|n| n.checked_add(1))
        .context("range length overflow")?;
    let header=format!("HTTP/1.1 {status} Response\r\nContent-Type: {}\r\nContent-Length: {count}\r\n{content_range}Accept-Ranges: bytes\r\nETag: {etag}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCross-Origin-Resource-Policy: same-origin\r\n\r\n",source.mime);
    write_until(&mut job.stream, header.as_bytes(), &source, stop, deadline)?;
    if job.head {
        return Ok(());
    }
    let mut offset = start;
    while offset <= end {
        let selected = selected_range_source(&job.route, &source)?;
        if selected.binding != source.binding {
            ensure!(
                selected.binding.selection == source.binding.selection
                    && selected.binding.clock == source.binding.clock
                    && selected.token == source.token
                    && selected.mime == source.mime,
                "media range successor differs or predecessor expired"
            );
            let next_deadline = response_deadline.min(selected.intent.expires);
            let next_bulk = client.open_source_media_until(
                &selected.binding,
                (Instant::now() + CONTROL_TIMEOUT).min(next_deadline),
            )?;
            source = selected;
            bulk = next_bulk;
            deadline = next_deadline;
        }
        ensure!(
            source.current() && !stop.load(Ordering::Acquire) && Instant::now() < deadline,
            "source delivery retired"
        );
        let size = u32::try_from((end - offset + 1).min(65536))?;
        bulk.set_operation_deadline(Some((Instant::now() + CONTROL_TIMEOUT).min(deadline)));
        let bytes = match bulk.download(offset, size) {
            Ok(bytes) => bytes,
            Err(error) => {
                source.intent.cancelled.store(true, Ordering::Release);
                return Err(error.into());
            }
        };
        write_until(&mut job.stream, &bytes, &source, stop, deadline)?;
        offset = offset
            .checked_add(u64::from(size))
            .context("range offset overflow")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "browser_source_media_tests.rs"]
mod tests;
