//! Actual private-Engine HTTP timing campaign using explicitly authored callback facts.
//! This does not establish browser display/source-clock calibration or device behavior.
use super::*;
use crate::motion::ResponseBody as ClientBody;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

const MEDIA_SHA: &str = "6643ea76ed68f8c0074bf1b45906990b5bd03ca0ae6839aae56901498f915acc";
const RESPONSE_LIMIT: u64 = 2 * 1024 * 1024;
const HTTP_BUDGET: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct PageScope {
    page: Value,
    player: Option<Value>,
    player_capability: Option<String>,
}
impl PageScope {
    fn fields(&self) -> BTreeMap<String, String> {
        let mut fields = BTreeMap::from([
            (
                "X-Pulsar-Launch".into(),
                self.page["launch_id"].as_str().unwrap().into(),
            ),
            (
                "X-Pulsar-Page".into(),
                self.page["page_id"].as_str().unwrap().into(),
            ),
            (
                "X-Pulsar-Page-Capability".into(),
                self.page["page_capability"].as_str().unwrap().into(),
            ),
        ]);
        if let Some(player) = &self.player {
            fields.extend([
                (
                    "X-Pulsar-Player".into(),
                    player["player_id"].as_str().unwrap().into(),
                ),
                (
                    "X-Pulsar-Source".into(),
                    player["source_id"].as_str().unwrap().into(),
                ),
                (
                    "X-Pulsar-Generation".into(),
                    player["generation"].as_u64().unwrap().to_string(),
                ),
                (
                    "X-Pulsar-Player-Capability".into(),
                    self.player_capability.as_ref().unwrap().clone(),
                ),
            ]);
        }
        fields
    }
}
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}
fn authority(url: &str) -> &str {
    url.strip_prefix("http://")
        .unwrap()
        .strip_suffix('/')
        .unwrap()
}
fn connect(url: &str, timeout: Duration) -> TcpStream {
    let stream =
        TcpStream::connect_timeout(&authority(url).parse().unwrap(), Duration::from_secs(3))
            .unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
}
fn parse_header(header: &[u8]) -> (u16, BTreeMap<String, String>) {
    let text = std::str::from_utf8(header).unwrap();
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let mut fields = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        assert!(fields
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_none());
    }
    (status, fields)
}
fn exchange(
    url: &str,
    method: &str,
    path: &str,
    body: &[u8],
    scope: Option<&PageScope>,
    extra: &[(&str, String)],
    timeout: Duration,
) -> HttpResponse {
    let mut stream = connect(url, timeout);
    let mut fields = scope.map(PageScope::fields).unwrap_or_default();
    fields.insert("Host".into(), authority(url).to_owned());
    fields.insert("Content-Length".into(), body.len().to_string());
    if method == "POST" {
        fields.insert("Origin".into(), format!("http://{}", authority(url)));
        fields.insert("Content-Type".into(), "application/octet-stream".into());
    }
    for (name, value) in extra {
        fields.insert((*name).into(), value.clone());
    }
    write!(stream, "{method} {path} HTTP/1.1\r\n").unwrap();
    for (name, value) in fields {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body).unwrap();
    let mut bytes = Vec::new();
    stream
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        u64::try_from(bytes.len()).is_ok_and(|length| length <= RESPONSE_LIMIT),
        "HTTP fixture response exceeded limit"
    );
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert!(split <= 8192);
    let (status, _fields) = parse_header(&bytes[..split]);
    HttpResponse {
        status,
        body: bytes[split..].to_vec(),
    }
}
fn post(url: &str, path: &str, body: &[u8], scope: &PageScope) -> Value {
    let response = exchange(url, "POST", path, body, Some(scope), &[], HTTP_BUDGET);
    if response.status != 200 {
        // Only fixed control routes are diagnostic. Media bearer paths and
        // request headers must never enter assertion output.
        let route = match path {
            "/media/catalog" => "/media/catalog",
            "/media/prepare" => "/media/prepare",
            "/media/status" => "/media/status",
            "/media/release" => "/media/release",
            "/player/start" => "/player/start",
            "/player/challenge" => "/player/challenge",
            "/player/timing-probe" => "/player/timing-probe",
            "/player/timing-observe" => "/player/timing-observe",
            "/player/timing-status" => "/player/timing-status",
            "/player/stop" => "/player/stop",
            "/start" => "/start",
            "/frame" => "/frame",
            "/status" => "/status",
            "/recording-page" => "/recording-page",
            _ => "<unlisted control route>",
        };
        let error = serde_json::from_slice::<Value>(&response.body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| {
                String::from_utf8_lossy(&response.body[..response.body.len().min(2048)])
                    .into_owned()
            })
            .chars()
            .take(500)
            .collect::<String>();
        assert_eq!(
            response.status, 200,
            "native fixture POST {route} refused; bounded backend error: {error}"
        );
    }
    serde_json::from_slice(&response.body).unwrap()
}
fn post_json(url: &str, path: &str, body: Value, scope: &PageScope) -> Value {
    post(url, path, &serde_json::to_vec(&body).unwrap(), scope)
}
fn page_scope(url: &str) -> PageScope {
    let response = exchange(url, "GET", "/", b"", None, &[], HTTP_BUDGET);
    assert_eq!(response.status, 200);
    let page = String::from_utf8(response.body).unwrap();
    let encoded = page
        .split_once("const PAGE_SCOPE = ")
        .unwrap()
        .1
        .split_once(';')
        .unwrap()
        .0;
    PageScope {
        page: serde_json::from_str(encoded).unwrap(),
        player: None,
        player_capability: None,
    }
}
fn heartbeat(url: &str, scope: &PageScope) -> Duration {
    let began = Instant::now();
    let response = exchange(
        url,
        "POST",
        "/player/challenge",
        b"",
        Some(scope),
        &[],
        Duration::from_secs(1),
    );
    assert_eq!(response.status, 200, "independent heartbeat refused");
    let body: Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["configured"], false);
    assert_eq!(body["pending"], true);
    let elapsed = began.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "media operation starved player control"
    );
    elapsed
}
struct Heartbeats {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<(u64, u128)>>,
}
impl Heartbeats {
    fn start(url: String, scope: PageScope) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let thread = thread::spawn(move || {
            let mut count = 0;
            let mut max_us = 0;
            while !cancelled.load(Ordering::Acquire) {
                max_us = max_us.max(heartbeat(&url, &scope).as_micros());
                count += 1;
                thread::sleep(Duration::from_millis(100));
            }
            (count, max_us)
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
    fn join(&mut self) -> (u64, u128) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Heartbeats {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
    url: String,
}
impl Server {
    fn start(
        client: SessionClient,
        project: ProjectId,
        revision: RevisionId,
        options: LiveCaptureOptions,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            run_controlled(
                client,
                project,
                revision,
                options,
                ServerControl {
                    port: 0,
                    duration_seconds: 180,
                    media_lease_ms: None,
                    cancelled,
                    guard: None,
                    playback: None,
                    hsp: None,
                    ready: move |url| {
                        tx.send(url)
                            .map_err(|_| anyhow::anyhow!("fixture ready receiver closed"))
                    },
                },
            )
        });
        let mut server = Self {
            stop,
            thread: Some(thread),
            url: String::new(),
        };
        server.url = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("browser fixture startup did not finish");
        server
    }
    fn join(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("browser server panicked")
            .expect("browser server cleanup failed");
        assert!(TcpStream::connect_timeout(
            &authority(&self.url).parse().unwrap(),
            Duration::from_secs(1)
        )
        .is_err());
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn execute(
    client: &mut SessionClient,
    project: &ProjectId,
    revision: Option<RevisionId>,
    command: Command,
) -> ClientBody {
    client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(20)));
    client
        .execute(command, Some(project.clone()), revision)
        .unwrap()
}
fn import(client: &mut SessionClient, project: &ProjectId, path: &Path) -> SourceVersionId {
    let ClientBody::Source { source_version } = execute(
        client,
        project,
        None,
        Command::ImportSource {
            path: path.to_owned(),
        },
    ) else {
        panic!("source response")
    };
    source_version
}
fn place(
    client: &mut SessionClient,
    project: &ProjectId,
    revision: RevisionId,
    source_version: SourceVersionId,
) -> (RevisionId, SourcePlacement) {
    let ClientBody::Project(snapshot) = execute(
        client,
        project,
        Some(revision),
        Command::SetSourcePlacement {
            spec: SourcePlacementSpec {
                source_version,
                source_start: SourceTimestamp::new(1, 3).unwrap(),
                source_end: SourceTimestamp::new(30, 1).unwrap(),
                source_anchor: SourceTimestamp::new(1, 3).unwrap(),
                project_anchor: ProjectTime::from_nanos(750_000_000),
                rate_numerator: 1,
                rate_denominator: 1,
            },
            replace: None,
        },
    ) else {
        panic!("placement response")
    };
    (
        snapshot.revision,
        snapshot.placements.last().unwrap().clone(),
    )
}
fn prepare(url: &str, scope: &PageScope, catalog: &Value, source: &SourceVersionId) -> Value {
    let entry = catalog["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_version"].as_str() == Some(source.as_str()))
        .unwrap();
    let mut response = post_json(
        url,
        "/media/prepare",
        json!({
            "catalog_generation":catalog["catalog_generation"], "entry_id":entry["entry_id"],
            "presentation_anchor_us":"5000000", "elapsed_anchor_us":"0", "allow_declared_clock":true,
        }),
        scope,
    );
    let selection = response["selection_id"].as_str().unwrap().to_owned();
    let end = Instant::now() + Duration::from_secs(30);
    while response["state"] == "pending" {
        assert!(
            Instant::now() < end,
            "native media verification did not finish"
        );
        thread::sleep(Duration::from_millis(10));
        response = post_json(
            url,
            "/media/status",
            json!({"selection_id":selection}),
            scope,
        );
    }
    assert_eq!(response["state"], "ready");
    assert_eq!(response["current"], true);
    assert_eq!(response["trim_start_us"], 5_333_334u64);
    assert_eq!(response["trim_end_us"], 35_000_000u64);
    assert!(response["url"].as_str().unwrap().starts_with("/media/"));
    response
}
fn activate(url: &str, page: &PageScope, prepared: &Value, generation: u64) -> PageScope {
    let reply = post_json(
        url,
        "/player/start",
        json!({
            "source_id":prepared["selection_id"], "media_selection_id":prepared["selection_id"],
            "generation":generation, "time_basis":"source_media_presentation",
        }),
        page,
    );
    assert_eq!(reply["player"]["source_id"], prepared["selection_id"]);
    assert_eq!(reply["guard"]["configured"], false);
    PageScope {
        page: page.page.clone(),
        player: Some(reply["player"].clone()),
        player_capability: Some(reply["player_capability"].as_str().unwrap().to_owned()),
    }
}

struct AuthoredBrowserClock {
    origin: Instant,
    presented_frames: u64,
}
impl AuthoredBrowserClock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            presented_frames: 0,
        }
    }
    fn media_us(&self) -> i64 {
        5_500_000 + i64::try_from(self.origin.elapsed().as_micros()).unwrap()
    }
    fn fresh_frame(&mut self) -> BrowserTimingFrame {
        self.presented_frames += 1;
        let elapsed = u64::try_from(self.origin.elapsed().as_micros()).unwrap();
        let sample_tick = 1_000_000 + elapsed;
        BrowserTimingFrame {
            sample: HandyPlayerSample {
                browser_tick_us: sample_tick,
                media_time_us: 5_500_000 + elapsed,
                presented_frames: self.presented_frames,
                playing: true,
                seeking: false,
                ended: false,
                visible: true,
                playback_rate_milli: 1000,
            },
            callback_tick_us: sample_tick - 10,
            presentation_time_us: Some(sample_tick - 50),
            expected_display_time_us: Some(sample_tick + 16_000),
        }
    }
}
fn timing_probe(url: &str, scope: &PageScope) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let probe = post(url, "/player/timing-probe", b"", scope);
        if probe["pending"] != true {
            assert!(
                probe["sequence"].is_string(),
                "native sequence must stay exact in JS"
            );
            assert!(probe["probe_id"].is_string());
            assert_eq!(probe["probe_timeout_ms"], 1000);
            assert!(probe.get("issued_at_boottime_ns").is_none());
            assert!(probe.get("expires_at_boottime_ns").is_none());
            return probe;
        }
        assert!(
            Instant::now() < deadline,
            "original timing open did not finish"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn timing_payload(probe: &Value, frame: &BrowserTimingFrame) -> Value {
    json!({"sequence":probe["sequence"],"probe_id":probe["probe_id"],"frame":frame})
}
fn timing_observe(
    url: &str,
    scope: &PageScope,
    clock: &mut AuthoredBrowserClock,
) -> (Value, BrowserTimingFrame) {
    let probe = timing_probe(url, scope);
    // The authored callback is constructed only after the nonce response.
    // Its frame counter and timestamp advance, unlike a timer reusing old data.
    let frame = clock.fresh_frame();
    frame.validate().unwrap();
    let observed = post_json(
        url,
        "/player/timing-observe",
        timing_payload(&probe, &frame),
        scope,
    );
    assert_eq!(observed["current"], true);
    assert_eq!(observed["qualified"], false);
    assert_eq!(observed["observation"]["sequence"], probe["sequence"]);
    assert_eq!(
        observed["observation"]["frame"],
        serde_json::to_value(&frame).unwrap()
    );
    assert!(observed["observation"]["bracket_width_ns"].is_string());
    assert!(observed["observation"]["project_phase"]["numerator"].is_string());
    assert!(observed["observation"]["project_phase"]["denominator"].is_string());
    (observed, frame)
}
fn timing_native(
    client: &mut SessionClient,
    project: &ProjectId,
    binding_id: &ArtifactId,
    player: &HandyPlayerIdentity,
) -> BrowserTimingReport {
    let ClientBody::BrowserTiming(report) = execute(
        client,
        project,
        None,
        Command::BrowserTimingStatus {
            binding_id: binding_id.clone(),
            player: player.clone(),
        },
    ) else {
        panic!("native timing report")
    };
    report.validate().unwrap();
    *report
}
fn check_observation(report: &BrowserTimingReport, projection: &Value, frame: &BrowserTimingFrame) {
    assert!(report.current && !report.qualified);
    let observation = report
        .observation
        .as_ref()
        .expect("accepted native observation");
    observation.bracket.validate().unwrap();
    assert_eq!(observation.frame, *frame);
    assert_eq!(
        observation.bracket.browser_sample_us,
        frame.sample.browser_tick_us
    );
    assert!(observation.bracket.native_receive_ns >= observation.bracket.native_issue_ns);
    let width = observation.bracket.native_receive_ns - observation.bracket.native_issue_ns;
    assert!(width <= u64::from(report.binding.policy.probe_timeout_ms) * 1_000_000);
    assert_eq!(
        projection["observation"]["bracket_width_ns"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        width
    );
    assert_eq!(
        projection["observation"]["observation_id"],
        observation.observation_id.as_str()
    );
    assert_eq!(
        projection["observation"]["sequence"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        observation.sequence
    );
    assert_eq!(projection["binding_id"], report.binding.binding_id.as_str());
    assert_eq!(
        projection["open_request_id"],
        report.binding.request_id.as_str()
    );
    let source_time = SourceTimestamp::new(
        i64::try_from(frame.sample.media_time_us).unwrap(),
        1_000_000,
    )
    .unwrap();
    let phase = report
        .binding
        .browser_media
        .clock
        .project_at(
            source_time,
            &report.binding.browser_media.selection.placement.spec,
        )
        .unwrap();
    assert_eq!(observation.project_phase, phase);
    assert_eq!(
        projection["observation"]["project_phase"],
        serde_json::to_value(phase).unwrap()
    );
    assert_eq!(
        observation.expires_at_boottime_ns,
        observation.bracket.native_issue_ns
            + u64::from(report.binding.policy.observation_timeout_ms) * 1_000_000
    );
    assert!(observation.expires_at_boottime_ns > observation.bracket.native_receive_ns);
}
fn retired_timing(
    client: &mut SessionClient,
    project: &ProjectId,
    binding: &BrowserTimingBinding,
) -> BrowserTimingReport {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let report = timing_native(client, project, &binding.binding_id, &binding.player);
        if !report.current {
            return report;
        }
        assert!(
            Instant::now() < deadline,
            "exact player timing was not retired promptly"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn refused_timing(url: &str, scope: &PageScope, path: &str, body: &[u8]) {
    let response = exchange(url, "POST", path, body, Some(scope), &[], HTTP_BUDGET);
    assert_ne!(
        response.status, 200,
        "retired player timing scope was accepted"
    );
}
fn player_identity(scope: &PageScope) -> HandyPlayerIdentity {
    serde_json::from_value(scope.player.as_ref().unwrap().clone()).unwrap()
}

#[test]
#[ignore = "requires a private actual Engine and authored media/model timing campaign"]
fn native_browser_timing_nonce_phase_continuation_and_retirement() {
    let endpoint = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_ENDPOINT").unwrap());
    let bootstrap = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_BOOTSTRAP").unwrap());
    let options_path = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_OPTIONS").unwrap());
    let media = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_MEDIA").unwrap());
    let result_path = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_RESULTS").unwrap());
    let project = ProjectId::new(std::env::var("PULSAR_TEST_BROWSER_PROJECT").unwrap()).unwrap();
    let media_bytes = std::fs::read(&media).unwrap();
    assert_eq!(media_bytes.len(), 55_544);
    assert_eq!(format!("{:x}", Sha256::digest(&media_bytes)), MEDIA_SHA);
    let mut client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    let source = import(&mut client, &project, &media);
    let (revision, placement) = place(&mut client, &project, RevisionId::new(0), source.clone());
    let options: LiveCaptureOptions =
        serde_json::from_slice(&std::fs::read(options_path).unwrap()).unwrap();
    assert_eq!(
        options.lease_ms, 10_000,
        "native timing fixture requires its declared 10s lease"
    );
    let mut server = Server::start(
        SessionClient::connect(&endpoint, &bootstrap).unwrap(),
        project.clone(),
        revision,
        options,
    );
    let url = server.url.clone();
    let page = page_scope(&url);
    let catalog = post(&url, "/media/catalog", b"", &page);
    let prepared = prepare(&url, &page, &catalog, &source);
    let active = activate(&url, &page, &prepared, 1);
    let mut pulses = Heartbeats::start(url.clone(), active.clone());
    let mut clock = AuthoredBrowserClock::new();
    let (initial, first_frame) = timing_observe(&url, &active, &mut clock);
    let initial_binding_id = ArtifactId::new(initial["binding_id"].as_str().unwrap()).unwrap();
    let initial_native = timing_native(
        &mut client,
        &project,
        &initial_binding_id,
        &player_identity(&active),
    );
    check_observation(&initial_native, &initial, &first_frame);
    assert_eq!(
        initial_native
            .binding
            .browser_media
            .selection
            .source_version,
        source
    );
    assert_eq!(
        initial_native.binding.browser_media.selection.placement,
        placement
    );
    let original_binding = initial_native.binding.clone();
    let original_observation = initial_native
        .observation
        .as_ref()
        .unwrap()
        .observation_id
        .clone();
    let no_capture = exchange(
        &url,
        "POST",
        "/status",
        b"",
        Some(&active),
        &[],
        HTTP_BUDGET,
    );
    assert_ne!(
        no_capture.status, 200,
        "timing open implicitly started inference capture"
    );
    let started = post_json(
        &url,
        "/start",
        json!({"media_us":clock.media_us(),
        "recording_maximum":256,"source_time_basis":"source_media_presentation"}),
        &active,
    );
    assert_eq!(started["active"], true);
    let capture_epoch = AttemptId::new(started["identity"]["epoch"].as_str().unwrap()).unwrap();
    let mut capture_chain = vec![capture_epoch.to_string()];
    let began = Instant::now();
    let deadline = began + Duration::from_secs(11);
    let progress_path = result_path.with_file_name("timing-progress.json");
    let mut trace = Vec::new();
    let mut observations = 1u64;
    let mut last_sequence = initial_native.observation.as_ref().unwrap().sequence;
    let mut successor_since = None;
    let mut diagnostic_position = None;
    let (last_projection, last_frame) = loop {
        assert!(
            Instant::now() < deadline,
            "timing continuation campaign exceeded its bound"
        );
        let progress = json!({"qualified":false,"stage":"before_probe","capture_chain":capture_chain,
            "timing_binding_id":initial_binding_id,"observations":observations,"trace":trace});
        std::fs::write(
            &progress_path,
            serde_json::to_vec_pretty(&progress).unwrap(),
        )
        .unwrap();
        let (observed, frame) = timing_observe(&url, &active, &mut clock);
        assert_eq!(observed["binding_id"], initial_binding_id.as_str());
        assert_eq!(
            observed["open_request_id"],
            original_binding.request_id.as_str()
        );
        let sequence = observed["observation"]["sequence"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(
            sequence,
            last_sequence + 1,
            "capture continuation reset timing sequence"
        );
        last_sequence = sequence;
        observations += 1;
        let source_time = SourceTimestamp::new(
            i64::try_from(frame.sample.media_time_us).unwrap(),
            1_000_000,
        )
        .unwrap();
        let phase = original_binding
            .browser_media
            .clock
            .project_at(source_time, &placement.spec)
            .unwrap();
        assert_eq!(
            observed["observation"]["project_phase"],
            serde_json::to_value(phase).unwrap()
        );
        let mut body = serde_json::to_vec(
            &json!({"epoch":capture_epoch,"media_us":frame.sample.media_time_us}),
        )
        .unwrap();
        body.push(b'\n');
        body.extend(vec![64; LIVE_FRAME_BYTES]);
        let capture = post(&url, "/frame", &body, &active);
        assert_eq!(capture["active"], true);
        assert_eq!(capture["browser"]["playback_epoch"], capture_epoch.as_str());
        assert_eq!(
            capture["options"]["browser_media"],
            serde_json::to_value(&original_binding.browser_media).unwrap()
        );
        let current_epoch = capture["identity"]["epoch"].as_str().unwrap();
        if !capture_chain.iter().any(|epoch| epoch == current_epoch) {
            assert_eq!(
                capture["continuation"]["predecessor"]["epoch"],
                capture_chain.last().unwrap().as_str()
            );
            capture_chain.push(current_epoch.to_owned());
            successor_since.get_or_insert_with(Instant::now);
        }
        let current_position =
            capture["inference"]["latest"]["result"]["payload"]["position"].as_f64();
        if let Some(position) = current_position {
            assert!((position - (0.25 + 0.5 * 64.0 / 255.0)).abs() < 0.0001);
            diagnostic_position = Some(position);
        }
        assert!(trace.len() < 96);
        trace.push(json!({"wall_us":began.elapsed().as_micros(),"capture_epoch":current_epoch,
            "timing_binding_id":observed["binding_id"],"sequence":sequence,
            "bracket_width_ns":observed["observation"]["bracket_width_ns"],
            "project_phase":observed["observation"]["project_phase"],"handoffs":capture["browser"]["handoffs"]}));
        let progress = json!({"qualified":false,"stage":"after_frame","capture_chain":capture_chain,
            "timing_binding_id":initial_binding_id,"observations":observations,"trace":trace});
        std::fs::write(
            &progress_path,
            serde_json::to_vec_pretty(&progress).unwrap(),
        )
        .unwrap();
        if began.elapsed() >= Duration::from_secs(7)
            && successor_since.is_some_and(|since| since.elapsed() >= Duration::from_secs(1))
            && capture["inference"]["completed"]
                .as_u64()
                .is_some_and(|count| count >= 2)
            && current_position.is_some()
            && capture["inference"]["latest"]["fresh"] == true
        {
            break (observed, frame);
        }
        thread::sleep(Duration::from_millis(150));
    };
    let stream_wall_us = began.elapsed().as_micros();
    assert!(capture_chain.len() >= 2);
    let after_handoff = timing_native(
        &mut client,
        &project,
        &initial_binding_id,
        &player_identity(&active),
    );
    check_observation(&after_handoff, &last_projection, &last_frame);
    assert_eq!(after_handoff.binding, original_binding);
    assert_ne!(
        after_handoff.observation.as_ref().unwrap().observation_id,
        original_observation
    );
    let pending = timing_probe(&url, &active);
    let stale_body = serde_json::to_vec(&timing_payload(&pending, &clock.fresh_frame())).unwrap();
    let (first_pulses, first_max_us) = pulses.join();
    post(&url, "/player/stop", b"", &active);
    refused_timing(&url, &active, "/player/timing-observe", &stale_body);
    refused_timing(&url, &active, "/player/timing-probe", b"");
    let paused_history = post(&url, "/player/timing-status", b"", &active);
    assert_eq!(paused_history["current"], false);
    assert_eq!(paused_history["qualified"], false);
    assert_eq!(paused_history["binding_id"], initial_binding_id.as_str());
    assert_eq!(
        paused_history["observation"]["observation_id"],
        after_handoff
            .observation
            .as_ref()
            .unwrap()
            .observation_id
            .as_str()
    );
    let paused = retired_timing(&mut client, &project, &original_binding);
    assert_eq!(
        paused.observation.as_ref().unwrap().observation_id,
        after_handoff.observation.as_ref().unwrap().observation_id
    );
    let preparation_request_id =
        RequestId::new(prepared["preparation_request_id"].as_str().unwrap()).unwrap();
    let ClientBody::BrowserMediaPreparation(still_prepared) = execute(
        &mut client,
        &project,
        None,
        Command::BrowserMediaPreparationStatus {
            original_request_id: preparation_request_id,
        },
    ) else {
        panic!("prepared source status")
    };
    assert!(
        still_prepared.current,
        "pause should release timing, not selected content reads"
    );
    let resumed = activate(&url, &page, &prepared, 2);
    let mut resumed_pulses = Heartbeats::start(url.clone(), resumed.clone());
    let (resumed_observed, resumed_frame) = timing_observe(&url, &resumed, &mut clock);
    let resumed_id = ArtifactId::new(resumed_observed["binding_id"].as_str().unwrap()).unwrap();
    assert_ne!(resumed_id, initial_binding_id);
    let resumed_native = timing_native(
        &mut client,
        &project,
        &resumed_id,
        &player_identity(&resumed),
    );
    check_observation(&resumed_native, &resumed_observed, &resumed_frame);
    assert_eq!(
        resumed_native.binding.browser_media,
        original_binding.browser_media
    );
    assert_ne!(
        resumed_native.binding.request_id,
        original_binding.request_id
    );
    refused_timing(&url, &active, "/player/timing-observe", &stale_body);
    let resumed_status = post(&url, "/player/timing-status", b"", &resumed);
    assert_eq!(resumed_status["current"], true);
    assert_eq!(
        resumed_status["observation"]["observation_id"],
        resumed_observed["observation"]["observation_id"]
    );
    let pending_resumed = timing_probe(&url, &resumed);
    let released_body =
        serde_json::to_vec(&timing_payload(&pending_resumed, &clock.fresh_frame())).unwrap();
    let (resumed_count, resumed_max_us) = resumed_pulses.join();
    let release = post_json(
        &url,
        "/media/release",
        json!({"selection_id":prepared["selection_id"]}),
        &page,
    );
    assert_eq!(release["current"], false);
    refused_timing(&url, &resumed, "/player/timing-observe", &released_body);
    let released = retired_timing(&mut client, &project, &resumed_native.binding);
    let replacement = prepare(&url, &page, &catalog, &source);
    assert_ne!(replacement["selection_id"], prepared["selection_id"]);
    let replacement_player = activate(&url, &page, &replacement, 1);
    let mut replacement_pulses = Heartbeats::start(url.clone(), replacement_player.clone());
    let (replacement_observed, replacement_frame) =
        timing_observe(&url, &replacement_player, &mut clock);
    let replacement_id =
        ArtifactId::new(replacement_observed["binding_id"].as_str().unwrap()).unwrap();
    assert_ne!(replacement_id, resumed_id);
    let replacement_native = timing_native(
        &mut client,
        &project,
        &replacement_id,
        &player_identity(&replacement_player),
    );
    check_observation(
        &replacement_native,
        &replacement_observed,
        &replacement_frame,
    );
    assert_ne!(
        replacement_native.binding.browser_media.lease_id,
        original_binding.browser_media.lease_id
    );
    refused_timing(&url, &resumed, "/player/timing-observe", &released_body);
    refused_timing(&url, &active, "/player/timing-status", b"");
    let replacement_status = post(&url, "/player/timing-status", b"", &replacement_player);
    assert_eq!(replacement_status["current"], true);
    assert_eq!(
        replacement_status["observation"]["observation_id"],
        replacement_observed["observation"]["observation_id"]
    );
    let (replacement_count, replacement_max_us) = replacement_pulses.join();
    post(&url, "/player/stop", b"", &replacement_player);
    post_json(
        &url,
        "/media/release",
        json!({"selection_id":replacement["selection_id"]}),
        &page,
    );
    let final_timing = retired_timing(&mut client, &project, &replacement_native.binding);
    for epoch in &capture_chain {
        let ClientBody::LiveCapture(report) = execute(
            &mut client,
            &project,
            Some(revision),
            Command::LiveCaptureStatus {
                epoch: AttemptId::new(epoch.clone()).unwrap(),
            },
        ) else {
            panic!("capture retirement")
        };
        assert!(!report.active);
        assert_eq!(
            report.options.browser_media.as_ref(),
            Some(&original_binding.browser_media)
        );
    }
    server.join();
    let mut result = json!({"passed":true,"qualified":false,"actual_engine":true,
        "rendered_browser":false,"device_output":false,"authored_callback_facts":true,
        "callback_display_calibrated":false,"source_clock_qualified":false,
        "without_handy_configuration":true,"nonce_fresh_frame_exchange":true,
        "whole_native_bracket_preserved":true,"exact_declared_phase":true,
        "timing_without_capture":true,"continuation_keeps_timing":true,
        "pause_preserves_media_retires_timing":true,"stale_replacement_scope_refused":true,
        "source_release_retires_pending_timing":true,"capture_chain":capture_chain,
        "timing_binding_ids":[initial_binding_id,resumed_id,replacement_id],
        "observations":observations+2,"server_joined":true,"capture_active":false});
    result["diagnostic_position"] = json!(diagnostic_position.unwrap());
    result["media_sha256"] = json!(MEDIA_SHA);
    result["source_version"] = json!(source);
    result["stream_wall_us"] = json!(stream_wall_us);
    result["capture_lease_ms"] = json!(10_000);
    result["intentional_start_capture_count"] = json!(1);
    result["independent_heartbeats"] = json!(first_pulses + resumed_count + replacement_count);
    result["maximum_heartbeat_us"] =
        json!(first_max_us.max(resumed_max_us).max(replacement_max_us));
    result["trace"] = json!(trace);
    result["observed_native"] = json!([
        initial_native,
        after_handoff,
        resumed_native,
        replacement_native
    ]);
    result["retired_native"] = json!([paused, released, final_timing]);
    std::fs::write(result_path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
