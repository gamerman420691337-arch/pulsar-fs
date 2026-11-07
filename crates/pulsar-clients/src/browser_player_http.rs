//! Bounded nonblocking HTTP ingress. Preview and native guard I/O have separate owners.
use super::*;
use crate::browser_player_control::{ControlReply, Permit, PlayerControl};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
const CONNECTIONS: usize = 16;
const JOBS: usize = 4;
const OPERATION: Duration = Duration::from_secs(12);
const CLEANUP: Duration = Duration::from_secs(3);
const MAX_RESPONSE: usize = 64 * 1024 * 1024;
fn lookahead_default_ms(maximum_analysis_age_ns: u64) -> u32 {
    // Prefer half the age budget, subject to the input's 1ms floor and a
    // responsive one-second cap. Admission and expiry remain authoritative.
    (maximum_analysis_age_ns / 2_000_000).clamp(1, 1000) as u32
}
pub(super) fn frame_timeout_required(
    active: bool,
    requires_frames: bool,
    last_frame: Instant,
    now: Instant,
) -> bool {
    active
        && requires_frames
        && now
            .checked_duration_since(last_frame)
            .is_none_or(|age| age > Duration::from_secs(3))
}
pub(super) trait PreviewBackend: Send + 'static {
    fn initial_lookahead_ms(&self) -> Option<u32> {
        None
    }
    fn initialize(&mut self) -> Result<()>;
    fn operation_deadline(&mut self, deadline: Instant) -> Result<()>;
    fn active(&self) -> bool;
    fn requires_frame_heartbeat(&self) -> bool {
        true
    }
    fn hsp_source_snapshot(&self) -> Result<Option<BrowserAnalysisFeedSnapshot>> {
        Ok(None)
    }
    fn take_analysis_commit(&mut self) -> Option<AnalysisCommit> {
        None
    }
    fn take_analysis_cancellation(&mut self) -> bool {
        false
    }
    fn request(&mut self, request: &Request) -> Result<serde_json::Value>;
    fn stop(&mut self) -> Result<()>;
}
impl PreviewBackend for Preview {
    fn initial_lookahead_ms(&self) -> Option<u32> {
        Some(lookahead_default_ms(
            self.options.policy.maximum_analysis_age_ns,
        ))
    }
    fn hsp_source_snapshot(&self) -> Result<Option<BrowserAnalysisFeedSnapshot>> {
        self.active_analysis_feed()
    }
    fn take_analysis_commit(&mut self) -> Option<AnalysisCommit> {
        self.analysis_commit.take()
    }
    fn take_analysis_cancellation(&mut self) -> bool {
        std::mem::take(&mut self.analysis_cancelled)
    }
    fn initialize(&mut self) -> Result<()> {
        self.recover_recordings()
    }
    fn operation_deadline(&mut self, deadline: Instant) -> Result<()> {
        Preview::operation_deadline(self, deadline)
    }
    fn active(&self) -> bool {
        self.report.is_some()
    }
    fn requires_frame_heartbeat(&self) -> bool {
        Preview::requires_frame_heartbeat(self)
    }
    fn request(&mut self, request: &Request) -> Result<serde_json::Value> {
        Preview::request(self, request)
    }
    fn stop(&mut self) -> Result<()> {
        Preview::stop(self)
    }
}

struct Job {
    ticket: u64,
    request: Request,
    permit: Permit,
    deadline: Instant,
}
struct Completion {
    ticket: u64,
    permit: Permit,
    result: Result<Vec<u8>>,
    analysis_commit: Option<AnalysisCommit>,
    analysis_cancelled: bool,
    hsp_request: Option<HspSourceRequest>,
}
struct HspSourceRequest {
    path: String,
    body: Vec<u8>,
    source: Option<BrowserAnalysisFeedSnapshot>,
    deadline: Instant,
}
struct Worker {
    jobs: Option<mpsc::SyncSender<Job>>,
    results: mpsc::Receiver<Completion>,
    stop: Arc<AtomicBool>,
    cleanup: Arc<AtomicBool>,
    frame_pending: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}
impl Worker {
    fn new(mut preview: impl PreviewBackend) -> Result<Self> {
        let (jobs, rx) = mpsc::sync_channel::<Job>(JOBS);
        let (tx, results) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let cleanup = Arc::new(AtomicBool::new(false));
        let clear = cleanup.clone();
        let frame_pending = Arc::new(AtomicBool::new(false));
        let frame = frame_pending.clone();
        let thread = thread::Builder::new()
            .name("pulsar-browser-preview".into())
            .spawn(move || {
                preview.operation_deadline(Instant::now() + OPERATION)?;
                preview.initialize()?;
                let mut last_frame = Instant::now();
                let mut failure = None;
                loop {
                    if clear.swap(false, Ordering::AcqRel)
                        || frame_timeout_required(
                            preview.active(),
                            preview.requires_frame_heartbeat(),
                            last_frame,
                            Instant::now(),
                        )
                    {
                        preview.operation_deadline(Instant::now() + CLEANUP)?;
                        if let Err(error) = preview.stop() {
                            failure = Some(error);
                        }
                    }
                    if cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let job = match rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(j) => j,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let is_frame = matches!(
                        job.request.path.as_str(),
                        "/frame" | "/analysis/successor-frame"
                    );
                    let hsp_source_request = matches!(
                        job.request.path.as_str(),
                        "/player/hsp-inspect" | "/player/hsp-bind"
                    );
                    let mut hsp_snapshot = None;
                    let result = if !job.permit.current() || Instant::now() >= job.deadline {
                        Err(anyhow::anyhow!(
                            "browser operation retired or expired before dispatch"
                        ))
                    } else if hsp_source_request {
                        preview.hsp_source_snapshot().map(|snapshot| {
                            hsp_snapshot = snapshot;
                            serde_json::Value::Null
                        })
                    } else {
                        preview
                            .operation_deadline(job.deadline)
                            .and_then(|()| preview.request(&job.request))
                    };
                    if (job.request.path == "/start" || is_frame) && result.is_ok() {
                        last_frame = job.request.received;
                    }
                    let needs_stop = !job.permit.current()
                        || (result.is_err()
                            && matches!(
                                job.request.path.as_str(),
                                "/start"
                                    | "/frame"
                                    | "/analysis/successor-frame"
                                    | "/status"
                                    | "/stop"
                            ));
                    if needs_stop {
                        preview.operation_deadline(Instant::now() + CLEANUP)?;
                        if let Err(error) = preview.stop() {
                            failure = Some(error);
                        }
                    }
                    if is_frame {
                        frame.store(false, Ordering::Release);
                    }
                    let hsp_request =
                        (hsp_source_request && result.is_ok()).then(|| HspSourceRequest {
                            path: job.request.path.clone(),
                            body: job.request.body.clone(),
                            source: hsp_snapshot,
                            deadline: job.deadline,
                        });
                    let analysis_commit = if result.is_ok() && !hsp_source_request {
                        preview.take_analysis_commit()
                    } else {
                        None
                    };
                    let analysis_cancelled = result.is_ok()
                        && !hsp_source_request
                        && preview.take_analysis_cancellation();
                    // Large editor/archive serialization stays on the Preview owner.
                    let result = result.and_then(|value| {
                        let bytes = serde_json::to_vec(&value)?;
                        ensure!(bytes.len() <= MAX_RESPONSE, "HTTP response limit");
                        Ok(bytes)
                    });
                    let _ = tx.send(Completion {
                        ticket: job.ticket,
                        permit: job.permit,
                        result,
                        analysis_commit,
                        analysis_cancelled,
                        hsp_request,
                    });
                }
                preview.operation_deadline(Instant::now() + CLEANUP)?;
                if let Err(error) = preview.stop() {
                    failure = Some(error);
                }
                if let Some(error) = failure {
                    return Err(error.context("capture cleanup outcome requires inspection"));
                }
                Ok(())
            })?;
        Ok(Self {
            jobs: Some(jobs),
            results,
            stop,
            cleanup,
            frame_pending,
            thread: Some(thread),
        })
    }
    fn submit(&self, job: Job) -> Result<()> {
        let frame = matches!(
            job.request.path.as_str(),
            "/frame" | "/analysis/successor-frame"
        );
        if frame {
            ensure!(
                !self.frame_pending.swap(true, Ordering::AcqRel),
                "one frame operation is already pending"
            );
        }
        let result = self
            .jobs
            .as_ref()
            .context("preview worker stopped")?
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("preview operation queue full or stopped"));
        if result.is_err() && frame {
            self.frame_pending.store(false, Ordering::Release);
        }
        result
    }
    fn join(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        self.cleanup.store(true, Ordering::Release);
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("preview worker panicked"))??;
        }
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

struct Connection {
    stream: TcpStream,
    bytes: Vec<u8>,
    expected: Option<usize>,
    deadline: Instant,
    waiting: bool,
    response: Option<Vec<u8>>,
    written: usize,
}
impl Connection {
    fn new(stream: TcpStream) -> Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            bytes: Vec::new(),
            expected: None,
            deadline: Instant::now() + Duration::from_secs(3),
            waiting: false,
            response: None,
            written: 0,
        })
    }
    fn read(&mut self, authority: &str) -> Result<Option<Request>> {
        ensure!(
            Instant::now() < self.deadline,
            "HTTP input deadline expired"
        );
        let mut read_budget = 16 * 1024;
        while read_budget > 0 {
            let mut block = [0u8; 4096];
            let n = match self.stream.read(&mut block) {
                Ok(0) => anyhow::bail!("incomplete HTTP request"),
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            read_budget -= n.min(read_budget);
            self.bytes.extend_from_slice(&block[..n]);
            ensure!(self.bytes.len() <= BODY_LIMIT + 8192, "HTTP body limit");
            if self.expected.is_none() {
                if let Some(offset) = self.bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let split = offset + 4;
                    ensure!(split <= 8192, "HTTP header limit");
                    let header = std::str::from_utf8(&self.bytes[..split])?;
                    let mut length = None;
                    for line in header.split("\r\n").skip(1).filter(|s| !s.is_empty()) {
                        let (key, value) = line.split_once(':').context("malformed HTTP header")?;
                        if key.eq_ignore_ascii_case("content-length") {
                            ensure!(length.is_none(), "duplicate Content-Length");
                            length = Some(value.trim().parse::<usize>()?);
                        }
                    }
                    let length = length.unwrap_or(0);
                    ensure!(length <= BODY_LIMIT, "HTTP body limit");
                    self.expected = Some(split + length);
                } else {
                    ensure!(self.bytes.len() < 8192, "HTTP header limit");
                }
            }
            if let Some(length) = self.expected {
                if self.bytes.len() >= length {
                    ensure!(self.bytes.len() == length, "pipelined HTTP request refused");
                    self.waiting = true;
                    self.deadline = Instant::now() + OPERATION;
                    return Ok(Some(parse_request(&self.bytes, authority, Instant::now())?));
                }
            }
        }
        Ok(None)
    }
    fn reply(&mut self, status: u16, kind: &str, body: Vec<u8>) -> Result<()> {
        ensure!(body.len() <= MAX_RESPONSE, "HTTP response limit");
        let mut bytes=format!("HTTP/1.1 {status} Response\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'\r\n\r\n",body.len()).into_bytes();
        bytes.extend(body);
        self.response = Some(bytes);
        self.bytes.clear();
        self.deadline = Instant::now() + Duration::from_secs(2);
        self.waiting = true;
        Ok(())
    }
    fn json(&mut self, result: Result<serde_json::Value>) {
        let (status, value) = match result {
            Ok(value) => (200, value),
            Err(error) => (
                409,
                serde_json::json!({"error":error.to_string().chars().take(500).collect::<String>()}),
            ),
        };
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let _ = self.reply(status, "application/json", bytes);
        }
    }
    fn write(&mut self) -> Result<bool> {
        ensure!(
            Instant::now() < self.deadline,
            "HTTP output deadline expired"
        );
        let bytes = self.response.as_ref().context("HTTP output absent")?;
        let end = (self.written + 16 * 1024).min(bytes.len());
        match self.stream.write(&bytes[self.written..end]) {
            Ok(0) => anyhow::bail!("HTTP peer closed"),
            Ok(n) => self.written += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        Ok(self.written == bytes.len())
    }
}

#[cfg(test)]
pub(super) fn run(
    preview: impl PreviewBackend,
    players: PlayerControl,
    port: u16,
    duration: u16,
    cancelled: Arc<AtomicBool>,
    ready: impl FnOnce(String) -> Result<()>,
) -> Result<()> {
    run_inner(preview, players, None, port, duration, cancelled, ready)
}
pub(super) fn run_with_media(
    preview: impl PreviewBackend,
    players: PlayerControl,
    media: super::source_media::MediaController,
    port: u16,
    duration: u16,
    cancelled: Arc<AtomicBool>,
    ready: impl FnOnce(String) -> Result<()>,
) -> Result<()> {
    run_inner(
        preview,
        players,
        Some(media),
        port,
        duration,
        cancelled,
        ready,
    )
}
fn run_inner(
    preview: impl PreviewBackend,
    mut players: PlayerControl,
    mut media: Option<super::source_media::MediaController>,
    port: u16,
    duration: u16,
    cancelled: Arc<AtomicBool>,
    ready: impl FnOnce(String) -> Result<()>,
) -> Result<()> {
    // Listener and worker are owned before publishing the URL; every exit joins both workers.
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    let authority = listener.local_addr()?.to_string();
    let initial_lookahead_ms = preview.initial_lookahead_ms();
    let mut worker = Worker::new(preview)?;
    let mut sockets = BTreeMap::<u64, Connection>::new();
    let mut next = 0u64;
    let result = (|| -> Result<()> {
        ready(format!("http://{authority}/"))?;
        let end = Instant::now() + Duration::from_secs(u64::from(duration));
        while Instant::now() < end && !cancelled.load(Ordering::Acquire) {
            let active = players.active();
            if let Some(source) = &mut media {
                source.tick();
                for (ticket, result) in source.poll() {
                    if let Some(connection) = sockets.get_mut(&ticket) {
                        connection.json(result);
                    }
                }
                if players
                    .active_browser_media()
                    .is_some_and(|binding| !source.binding_current(binding))
                {
                    players.stop();
                }
            }
            players.tick();
            for (ticket, result) in players.poll() {
                if let Some(connection) = sockets.get_mut(&ticket) {
                    connection.json(result);
                }
            }
            if active && !players.active() {
                worker.cleanup.store(true, Ordering::Release);
                if let Some(source) = &mut media {
                    source.cancel_successor();
                }
            }
            while let Ok(completion) = worker.results.try_recv() {
                if let Some(operation) = completion.hsp_request {
                    let result = if Instant::now() >= operation.deadline {
                        Err(anyhow::anyhow!(
                            "original HSP source inspection expired before dispatch"
                        ))
                    } else {
                        players.hsp_handle(
                            &operation.path,
                            &completion.permit,
                            operation.source,
                            &operation.body,
                        )
                    };
                    if let Some(connection) = sockets.get_mut(&completion.ticket) {
                        connection.json(result);
                    }
                    continue;
                }
                if completion.analysis_cancelled {
                    if players
                        .cancel_analysis_successor(&completion.permit)
                        .is_err()
                    {
                        players.stop();
                        worker.cleanup.store(true, Ordering::Release);
                    }
                    if let Some(source) = &mut media {
                        source.cancel_successor();
                    }
                }
                if let Some(commit) = &completion.analysis_commit {
                    let committed = (|| -> Result<()> {
                        let source = media.as_mut().context("source media unavailable")?;
                        source.check_successor(&commit.old_media, &commit.new_media)?;
                        players.check_analysis_commit(&completion.permit, commit)?;
                        source.commit_successor(&commit.old_media, &commit.new_media)?;
                        players.commit_analysis(&completion.permit, commit)
                    })();
                    if let Err(error) = committed {
                        players.stop();
                        worker.cleanup.store(true, Ordering::Release);
                        if let Some(connection) = sockets.get_mut(&completion.ticket) {
                            connection.json(Err(error));
                        }
                        continue;
                    }
                }
                if let Some(connection) = sockets.get_mut(&completion.ticket) {
                    if players.still_matches(&completion.permit) {
                        match completion.result {
                            Ok(bytes) => connection.reply(200, "application/json", bytes)?,
                            Err(error) => connection.json(Err(error)),
                        }
                    } else {
                        connection.json(Err(anyhow::anyhow!(
                            "browser scope retired during operation; original outcome retained"
                        )));
                    }
                }
            }
            if worker.thread.as_ref().is_some_and(|t| t.is_finished()) {
                anyhow::bail!("preview worker stopped; inspect original capture outcomes");
            }
            // At most CONNECTIONS sockets and bounded bytes per socket per turn.
            for _ in 0..CONNECTIONS {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if sockets.len() >= CONNECTIONS {
                            drop(stream);
                            continue;
                        }
                        next = next.checked_add(1).context("HTTP ticket overflow")?;
                        sockets.insert(next, Connection::new(stream)?);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
            }
            let tickets: Vec<_> = sockets.keys().copied().collect();
            for ticket in tickets {
                let connection = sockets.get_mut(&ticket).expect("connection retained");
                if connection.deadline <= Instant::now() {
                    sockets.remove(&ticket);
                    continue;
                }
                if connection.response.is_some() {
                    if connection.write().unwrap_or(true) {
                        sockets.remove(&ticket);
                    }
                    continue;
                }
                if connection.waiting {
                    continue;
                }
                let mut request = match connection.read(&authority) {
                    Ok(Some(r)) => r,
                    Ok(None) => continue,
                    Err(_) => {
                        let _ = connection.reply(400, "text/plain", b"Request refused".to_vec());
                        continue;
                    }
                };
                if matches!(request.method.as_str(), "GET" | "HEAD")
                    && request.path.starts_with("/media/")
                {
                    let Some(source) = &media else {
                        connection.reply(
                            503,
                            "text/plain",
                            b"Media service unavailable".to_vec(),
                        )?;
                        continue;
                    };
                    let owned = sockets.remove(&ticket).expect("accepted media socket");
                    let mut stream = Some(owned.stream);
                    if source
                        .submit_range(
                            &mut stream,
                            &request.path,
                            request.method == "HEAD",
                            request.range.take(),
                            request.if_range.take(),
                        )
                        .is_err()
                    {
                        if let Some(stream) = stream {
                            let mut refused = Connection::new(stream)?;
                            refused.reply(403, "text/plain", b"Media request refused".to_vec())?;
                            sockets.insert(ticket, refused);
                        }
                    }
                    continue;
                }
                if request.method == "GET" {
                    let result = players.page().and_then(|mut page| {
                        if let Some(ms) = initial_lookahead_ms {
                            page["default_lookahead_ms"] = serde_json::json!(ms);
                        }
                        Ok(PAGE
                            .replace(
                                "/*__PULSAR_PAGE_SCOPE__*/ null",
                                &serde_json::to_string(&page)?,
                            )
                            .into_bytes())
                    });
                    match result {
                        Ok(body) => {
                            connection.reply(200, "text/html; charset=utf-8", body)?;
                        }
                        Err(error) => connection.json(Err(error)),
                    }
                    continue;
                }
                if request.path.starts_with("/media/") {
                    if matches!(
                        request.path.as_str(),
                        "/media/continue" | "/media/continue-status"
                    ) {
                        if let Err(error) = players.require_analysis_continuity_supported() {
                            connection.json(Err(error));
                            continue;
                        }
                    }
                    if matches!(
                        request.path.as_str(),
                        "/media/continue" | "/media/continue-status"
                    ) && players.authorize(&request.scope, true).is_err()
                    {
                        connection.json(Err(anyhow::anyhow!(
                            "media continuation requires the original active player"
                        )));
                        continue;
                    }
                    let result = players.media_page(&request.scope).and_then(|page| {
                        media.as_mut().context("media service unavailable")?.handle(
                            &request.path,
                            &request.scope,
                            page,
                            &request.body,
                            ticket,
                        )
                    });
                    match result {
                        Ok(super::source_media::MediaReply::Ready(value)) => {
                            connection.json(Ok(value))
                        }
                        Ok(super::source_media::MediaReply::Pending) => {}
                        Err(error) => connection.json(Err(error)),
                    }
                    continue;
                }
                if request.path.starts_with("/player/hsp-") {
                    let live = matches!(
                        request.path.as_str(),
                        "/player/hsp-inspect"
                            | "/player/hsp-prepare"
                            | "/player/hsp-bind"
                            | "/player/hsp-play"
                            | "/player/hsp-continuity-enable"
                    );
                    let permit = match players.authorize(&request.scope, live) {
                        Ok(permit) => permit,
                        Err(error) => {
                            connection.json(Err(error));
                            continue;
                        }
                    };
                    if matches!(
                        request.path.as_str(),
                        "/player/hsp-inspect" | "/player/hsp-bind"
                    ) {
                        let deadline = request.received + OPERATION;
                        if let Err(error) = worker.submit(Job {
                            ticket,
                            request,
                            permit,
                            deadline,
                        }) {
                            connection.json(Err(error));
                        }
                    } else {
                        connection.json(players.hsp_handle(
                            &request.path,
                            &permit,
                            None,
                            &request.body,
                        ));
                    }
                    continue;
                }
                if request.path.starts_with("/player/") {
                    let was_active = players.active();
                    let binding = if request.path == "/player/start" {
                        media
                            .as_ref()
                            .map(|source| source.resolve_player(&request.scope, &request.body))
                            .transpose()
                            .map(Option::flatten)
                    } else if request.path == "/player/timing-next-open" {
                        players
                            .active_browser_media()
                            .context("active media absent")
                            .and_then(|old| {
                                media
                                    .as_ref()
                                    .context("media unavailable")?
                                    .successor_binding(old)
                            })
                            .map(Some)
                    } else {
                        Ok(None)
                    };
                    let result = binding.and_then(|binding| {
                        players.handle_with_media(
                            &request.path,
                            &request.scope,
                            &request.body,
                            ticket,
                            binding,
                        )
                    });
                    match result {
                        Ok(ControlReply::Ready(value)) => connection.json(Ok(value)),
                        Ok(ControlReply::Pending) => {}
                        Err(error) => connection.json(Err(error)),
                    }
                    if was_active && !players.active() {
                        worker.cleanup.store(true, Ordering::Release);
                        if let Some(source) = &mut media {
                            source.cancel_successor();
                        }
                    }
                    continue;
                }
                let live = matches!(
                    request.path.as_str(),
                    "/start"
                        | "/frame"
                        | "/status"
                        | "/analysis/finish"
                        | "/analysis/finish-status"
                        | "/analysis/prepare"
                        | "/analysis/transfer"
                        | "/analysis/transfer-status"
                        | "/analysis/successor-frame"
                        | "/analysis/cancel-successor"
                );
                let permit = match players.authorize(&request.scope, live) {
                    Ok(p) => p,
                    Err(error) => {
                        connection.json(Err(error));
                        continue;
                    }
                };
                if live {
                    request.player_permit = Some(permit.clone());
                    let successor = matches!(
                        request.path.as_str(),
                        "/analysis/prepare" | "/analysis/successor-frame"
                    );
                    request.browser_timing = if successor {
                        players.successor_timing_for(&permit).ok()
                    } else {
                        players.browser_timing_for(&permit).ok()
                    };
                    match players.browser_media_for(&permit) {
                        Ok(binding) => {
                            if successor {
                                let selected = binding
                                    .as_ref()
                                    .context("active media absent")
                                    .and_then(|old| {
                                        media
                                            .as_ref()
                                            .context("media unavailable")?
                                            .successor_binding(old)
                                    });
                                match selected {
                                    Ok(next) => request.browser_media = Some(next),
                                    Err(error) => {
                                        connection.json(Err(error));
                                        continue;
                                    }
                                }
                            } else {
                                request.browser_media = binding;
                            }
                        }
                        Err(error) => {
                            connection.json(Err(error));
                            continue;
                        }
                    }
                    request.browser_media_expires = request
                        .browser_media
                        .as_ref()
                        .and_then(|binding| media.as_ref()?.binding_expires(binding));
                }
                if request.path == "/start" {
                    let checked = serde_json::from_slice::<Start>(&request.body)
                        .map_err(anyhow::Error::from)
                        .and_then(|start| {
                            if start.analysis.as_ref().is_some_and(|a| a.continuity) {
                                players.require_analysis_continuity_supported()?;
                            }
                            ensure!(
                                permit.identity.as_ref().is_some_and(|identity| identity
                                    .time_basis
                                    == start.source_time_basis),
                                "capture source basis differs from the registered player"
                            );
                            Ok(())
                        });
                    if let Err(error) = checked {
                        connection.json(Err(error));
                        continue;
                    }
                }
                if request.path == "/stop" {
                    // Compatibility stop is scoped too, and cannot overtake a replacement.
                    players.stop();
                    if let Some(source) = &mut media {
                        source.cancel_successor();
                    }
                    worker.cleanup.store(true, Ordering::Release);
                    connection.json(Ok(
                        serde_json::json!({"active":false,"capture_cleanup_pending":true}),
                    ));
                    continue;
                }
                let deadline = Instant::now() + OPERATION;
                if let Err(error) = worker.submit(Job {
                    ticket,
                    request,
                    permit,
                    deadline,
                }) {
                    connection.json(Err(error));
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    })();
    sockets.clear();
    drop(listener);
    // Native release/expiry is independent of pending capture cleanup and is attempted first.
    worker.stop.store(true, Ordering::Release);
    worker.cleanup.store(true, Ordering::Release);
    let guard_result = players.shutdown();
    let source_result = media.as_mut().map_or(Ok(()), |source| source.shutdown());
    let worker_result = worker.join();
    let result = joined_results(result, guard_result, worker_result);
    match (result, source_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(operation), Err(source)) => {
            Err(anyhow::anyhow!("{operation}; source cleanup: {source}"))
        }
    }
}

fn joined_results(operation: Result<()>, guard: Result<()>, preview: Result<()>) -> Result<()> {
    let failures = [
        ("browser operation", operation),
        ("native player cleanup", guard),
        ("capture cleanup", preview),
    ]
    .into_iter()
    .filter_map(|(name, result)| result.err().map(|error| format!("{name}: {error:#}")))
    .collect::<Vec<_>>();
    ensure!(failures.is_empty(), "{}", failures.join("; "));
    Ok(())
}

#[cfg(test)]
#[path = "browser_player_http_tests.rs"]
mod tests;
