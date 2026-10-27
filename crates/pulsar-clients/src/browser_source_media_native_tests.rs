//! Actual private-Engine/HTTP campaign with authored inputs. No rendered browser,
//! physical device, decoded-pixel correspondence or useful model qualification.
use super::*;
use crate::motion::ResponseBody as ClientBody;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::{Shutdown, TcpStream},
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
    fields: BTreeMap<String, String>,
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
    let (status, fields) = parse_header(&bytes[..split]);
    HttpResponse {
        status,
        fields,
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
fn assert_retired_media(url: &str, path: &str) {
    let response = exchange(url, "HEAD", path, b"", None, &[], HTTP_BUDGET);
    assert!(
        !matches!(response.status, 200 | 206),
        "retired media bearer still grants reads"
    );
}
fn begin_slow_reader(url: &str, path: &str) -> TcpStream {
    let mut stream = connect(url, Duration::from_secs(5));
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n",
        authority(url)
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("slow media headers did not arrive");
        stream.set_read_timeout(Some(remaining)).unwrap();
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
        assert!(header.len() <= 8192);
    }
    let (status, fields) = parse_header(&header);
    assert_eq!(status, 200);
    assert_eq!(fields["content-length"], (16 * 1024 * 1024).to_string());
    stream
}

#[test]
#[ignore = "requires a private real Engine, authored model/media, and explicit native campaign"]
fn native_browser_source_media_ranges_capture_and_retirement() {
    let endpoint = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_ENDPOINT").unwrap());
    let bootstrap = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_BOOTSTRAP").unwrap());
    let options_path = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_OPTIONS").unwrap());
    let media = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_MEDIA").unwrap());
    let result_path = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_RESULTS").unwrap());
    let project = ProjectId::new(std::env::var("PULSAR_TEST_BROWSER_PROJECT").unwrap()).unwrap();
    let source_bytes = std::fs::read(&media).unwrap();
    assert_eq!(source_bytes.len(), 55_544);
    assert_eq!(format!("{:x}", Sha256::digest(&source_bytes)), MEDIA_SHA);
    let mut large = tempfile::NamedTempFile::new().unwrap();
    let block = vec![0x5a; 64 * 1024];
    large.write_all(&source_bytes).unwrap();
    let mut remaining = 16 * 1024 * 1024 - source_bytes.len();
    while remaining > 0 {
        let count = remaining.min(block.len());
        large.write_all(&block[..count]).unwrap();
        remaining -= count;
    }
    large.flush().unwrap();
    let mut client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    let source = import(&mut client, &project, &media);
    let slow_source = import(&mut client, &project, large.path());
    let (revision, placement) = place(&mut client, &project, RevisionId::new(0), source.clone());
    let (revision, _) = place(&mut client, &project, revision, slow_source.clone());
    let options: LiveCaptureOptions =
        serde_json::from_slice(&std::fs::read(options_path).unwrap()).unwrap();
    let server_client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    let mut server = Server::start(server_client, project.clone(), revision, options);
    let url = server.url.clone();
    let page = page_scope(&url);
    let catalog = post(&url, "/media/catalog", b"", &page);
    assert_eq!(catalog["entries"].as_array().unwrap().len(), 2);
    let prepared = prepare(&url, &page, &catalog, &source);
    let media_path = prepared["url"].as_str().unwrap();
    for path in ["/player/challenge", "/status"] {
        let response = exchange(&url, "POST", path, b"", Some(&page), &[], HTTP_BUDGET);
        assert_ne!(
            response.status, 200,
            "media preload implicitly activated player or capture"
        );
    }
    let head = exchange(&url, "HEAD", media_path, b"", None, &[], HTTP_BUDGET);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(
        head.fields["content-length"],
        source_bytes.len().to_string()
    );
    assert_eq!(head.fields["accept-ranges"], "bytes");
    assert_eq!(head.fields["x-content-type-options"], "nosniff");
    let etag = head.fields["etag"].clone();
    assert!(etag.starts_with('"') && etag.ends_with('"') && !etag.starts_with("W/"));
    let full = exchange(&url, "GET", media_path, b"", None, &[], HTTP_BUDGET);
    assert_eq!(full.status, 200);
    assert_eq!(full.body, source_bytes);
    assert_eq!(full.fields["etag"], etag);
    let mut reconstructed = Vec::new();
    for offset in (0..source_bytes.len()).step_by(4096) {
        let end = (offset + 4096).min(source_bytes.len()) - 1;
        let response = exchange(
            &url,
            "GET",
            media_path,
            b"",
            None,
            &[("Range", format!("bytes={offset}-{end}"))],
            HTTP_BUDGET,
        );
        assert_eq!(response.status, 206);
        assert_eq!(
            response.fields["content-range"],
            format!("bytes {offset}-{end}/{}", source_bytes.len())
        );
        assert_eq!(
            response.fields["content-length"],
            (end - offset + 1).to_string()
        );
        assert_eq!(response.fields["etag"], etag);
        reconstructed.extend(response.body);
    }
    assert_eq!(reconstructed, source_bytes);
    let suffix = exchange(
        &url,
        "GET",
        media_path,
        b"",
        None,
        &[("Range", "bytes=-17".into())],
        HTTP_BUDGET,
    );
    assert_eq!(suffix.status, 206);
    assert_eq!(suffix.body, source_bytes[source_bytes.len() - 17..]);
    let open = exchange(
        &url,
        "GET",
        media_path,
        b"",
        None,
        &[("Range", "bytes=123-".into())],
        HTTP_BUDGET,
    );
    assert_eq!(open.status, 206);
    assert_eq!(open.body, source_bytes[123..]);
    for range in [
        format!("bytes={}-", source_bytes.len()),
        "bytes=5-3".into(),
        "bytes=0-0,2-2".into(),
        "bytes=18446744073709551616-".into(),
    ] {
        let bad = exchange(
            &url,
            "GET",
            media_path,
            b"",
            None,
            &[("Range", range)],
            HTTP_BUDGET,
        );
        assert_eq!(bad.status, 416);
        assert_eq!(
            bad.fields["content-range"],
            format!("bytes */{}", source_bytes.len())
        );
    }
    for origin in ["null".to_owned(), "https://foreign.invalid".to_owned()] {
        let denied = exchange(
            &url,
            "GET",
            media_path,
            b"",
            None,
            &[("Origin", origin)],
            HTTP_BUDGET,
        );
        assert!(!matches!(denied.status, 200 | 206));
    }
    let denied = exchange(
        &url,
        "GET",
        media_path,
        b"",
        None,
        &[("Host", "foreign.invalid".into())],
        HTTP_BUDGET,
    );
    assert!(!matches!(denied.status, 200 | 206));
    let foreign_page = page_scope(&url);
    let denied = exchange(
        &url,
        "POST",
        "/media/status",
        &serde_json::to_vec(&json!({"selection_id":prepared["selection_id"]})).unwrap(),
        Some(&foreign_page),
        &[],
        HTTP_BUDGET,
    );
    assert_ne!(
        denied.status, 200,
        "foreign page inspected another prepared selection"
    );
    let active = activate(&url, &page, &prepared, 1);
    let mut pulses = Heartbeats::start(url.clone(), active.clone());
    let first_time = 5_500_000i64;
    let started = post_json(
        &url,
        "/start",
        json!({"media_us":first_time,"recording_maximum":512,"source_time_basis":"source_media_presentation"}),
        &active,
    );
    assert_eq!(started["active"], true);
    let epoch = AttemptId::new(started["identity"]["epoch"].as_str().unwrap()).unwrap();
    let ClientBody::LiveCapture(native) = execute(
        &mut client,
        &project,
        Some(revision),
        Command::LiveCaptureStatus {
            epoch: epoch.clone(),
        },
    ) else {
        panic!("native capture")
    };
    let binding = native.options.browser_media.as_ref().unwrap();
    assert_eq!(binding.selection.source_version, source);
    assert_eq!(binding.selection.source.sha256, MEDIA_SHA);
    assert_eq!(binding.selection.placement, placement);
    assert_eq!(native.identity.source, source);
    assert_eq!(
        native.options.source_anchor,
        SourceTimestamp::new(first_time, 1_000_000).unwrap()
    );
    let exact = binding
        .clock
        .project_at(native.options.source_anchor, &placement.spec)
        .unwrap();
    assert_eq!(native.options.exact_project_anchor, Some(exact));
    assert_eq!(native.options.project_anchor.as_nanos(), 916_666_666);
    assert_eq!(
        exact.quantize_nanoseconds().unwrap().remainder_numerator,
        "8"
    );
    assert_eq!(
        native.options.lease_ms, 30_000,
        "campaign expects the first prepared handoff after 15s"
    );
    let stream_started = Instant::now();
    let deadline = stream_started + Duration::from_secs(25);
    let progress_path = result_path.with_file_name("continuation-progress.json");
    let mut trace = Vec::new();
    let mut capture_chain = vec![epoch.to_string()];
    let mut observed_captures = BTreeMap::from([(epoch.to_string(), native.as_ref().clone())]);
    let mut first_handoff = None;
    let mut position = None;
    let mut ordinal = 0u64;
    let mut final_handoffs = 0u64;
    let save_progress = |stage: &str, media_us: i64, trace: &[Value]| {
        let progress = json!({"evidence_kind":"native_stream_progress","complete":stage=="sustained_after_handoff","qualified":false,"stage":stage,
            "initial_epoch":epoch,"source_sha256":MEDIA_SHA,"request_media_us":media_us,
            "stream_wall_us":stream_started.elapsed().as_micros(),"trace":trace});
        std::fs::write(
            &progress_path,
            serde_json::to_vec_pretty(&progress).unwrap(),
        )
        .unwrap();
    };
    loop {
        assert!(
            Instant::now() < deadline,
            "native stream did not sustain a prepared handoff within 25s"
        );
        let media_us = if ordinal == 0 {
            first_time
        } else {
            first_time + i64::try_from(stream_started.elapsed().as_micros()).unwrap()
        };
        save_progress("before_frame", media_us, &trace);
        let mut body = serde_json::to_vec(&json!({"epoch":epoch,"media_us":media_us})).unwrap();
        body.push(b'\n');
        body.extend(vec![64; LIVE_FRAME_BYTES]);
        let response = post(&url, "/frame", &body, &active);
        let handoffs = response["browser"]["handoffs"].as_u64().unwrap();
        if ordinal < 3 || stream_started.elapsed() >= Duration::from_secs(14) {
            assert!(trace.len() < 128, "bounded continuation trace exceeded");
            trace.push(json!({
                "wall_us":stream_started.elapsed().as_micros(),"media_us":media_us,
                "epoch":response["identity"]["epoch"],"active":response["active"],
                "retirement":response["retirement"],"frames":response["received_frames"],
                "analyses":response["inference"]["completed"],"inference_state":response["inference"]["state"],
                "handoffs":handoffs,"pending":response["browser"]["pending_epoch"],
                "predecessor":response["continuation"]["predecessor"]["epoch"],
                "source_anchor":response["options"]["source_anchor"],
                "exact_project_anchor":response["options"]["exact_project_anchor"],
            }));
        }
        save_progress("after_frame", media_us, &trace);
        assert!(
            Instant::now() < deadline,
            "native frame operation exceeded 25s stream bound"
        );
        assert_eq!(response["browser"]["playback_epoch"], epoch.as_str());
        assert_eq!(
            response["active"], true,
            "native Preview returned a retired capture during continuous frames"
        );
        assert!(
            handoffs >= final_handoffs,
            "prepared handoff count regressed"
        );
        final_handoffs = handoffs;
        let current_position =
            response["inference"]["latest"]["result"]["payload"]["position"].as_f64();
        if let Some(value) = current_position {
            assert!((value - (0.25 + 0.5 * 64.0 / 255.0)).abs() < 0.0001);
            position = Some(value);
        }
        let mut native_value = response.clone();
        native_value.as_object_mut().unwrap().remove("browser");
        let observed: LiveCaptureReport = serde_json::from_value(native_value).unwrap();
        observed.validate().unwrap();
        assert_eq!(observed.options.browser_media.as_ref(), Some(binding));
        assert_eq!(observed.identity.source, source);
        let time = SourceTimestamp::new(media_us, 1_000_000).unwrap();
        assert_eq!(
            observed.clock.project_at_source(time).unwrap(),
            binding.clock.project_at(time, &placement.spec).unwrap()
        );
        assert_eq!(
            observed.options.exact_project_anchor,
            Some(
                binding
                    .clock
                    .project_at(observed.options.source_anchor, &placement.spec)
                    .unwrap()
            )
        );
        let current_epoch = observed.identity.epoch.to_string();
        if !observed_captures.contains_key(&current_epoch) {
            let predecessor = observed
                .continuation
                .as_ref()
                .expect("new epoch requires prepared lineage");
            let previous = capture_chain.last().unwrap();
            assert_eq!(
                predecessor.predecessor,
                observed_captures[previous].identity
            );
            assert_eq!(
                observed.options.continuation_mode,
                LiveContinuationMode::WhenReady
            );
            capture_chain.push(current_epoch.clone());
            first_handoff.get_or_insert_with(Instant::now);
        }
        let successor_running = current_epoch != epoch.as_str()
            && observed.inference.as_ref().is_some_and(|info| {
                info.state == LiveCaptureInferenceState::Running
                    && info.completed >= 2
                    && info.accepted > 0
            });
        observed_captures.insert(current_epoch, observed);
        ordinal += 1;
        if stream_started.elapsed() >= Duration::from_secs(18)
            && first_handoff.is_some_and(|began| began.elapsed() >= Duration::from_secs(2))
            && successor_running
            && current_position.is_some()
            && response["browser"]["pending_epoch"].is_null()
        {
            save_progress("sustained_after_handoff", media_us, &trace);
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let stream_wall_us = stream_started.elapsed().as_micros();
    let first_handoff_wall_us = first_handoff
        .unwrap()
        .duration_since(stream_started)
        .as_micros();
    let position = position.unwrap();
    assert!(
        capture_chain.len() >= 2
            && final_handoffs == u64::try_from(capture_chain.len() - 1).unwrap()
    );
    let successor_epoch = AttemptId::new(capture_chain.last().unwrap().clone()).unwrap();
    let ClientBody::LiveCapture(successor) = execute(
        &mut client,
        &project,
        Some(revision),
        Command::LiveCaptureStatus {
            epoch: successor_epoch.clone(),
        },
    ) else {
        panic!("current successor")
    };
    assert!(
        successor.active
            && successor
                .inference
                .as_ref()
                .is_some_and(|info| info.completed >= 2)
    );
    assert_eq!(successor.options.browser_media.as_ref(), Some(binding));
    let ClientBody::LiveCapture(predecessor) = execute(
        &mut client,
        &project,
        Some(revision),
        Command::LiveCaptureStatus {
            epoch: epoch.clone(),
        },
    ) else {
        panic!("retired predecessor")
    };
    assert!(!predecessor.active);
    assert_eq!(
        predecessor.retirement,
        Some(LiveCaptureRetirement::Superseded)
    );
    let (first_pulses, first_max_us) = pulses.join();
    let stop = post(&url, "/player/stop", b"", &active);
    assert_eq!(stop["active"], false);
    let remaining_head = exchange(&url, "HEAD", media_path, b"", None, &[], HTTP_BUDGET);
    assert_eq!(
        remaining_head.status, 200,
        "pause retired selected content access"
    );
    let recording_deadline = Instant::now() + Duration::from_secs(10);
    let mut retained_recordings = Vec::new();
    for capture_epoch in &capture_chain {
        let mut after = 0u64;
        let mut samples = 0usize;
        loop {
            let retained = post_json(
                &url,
                "/recording-page",
                json!({"epoch":capture_epoch,"after":after}),
                &active,
            );
            assert!(
                Instant::now() < recording_deadline,
                "capture-chain recording cleanup did not complete"
            );
            if retained["active"] != false {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            assert_eq!(
                retained["browser_media"],
                serde_json::to_value(binding).unwrap()
            );
            assert_eq!(retained["identity"]["source"], source.as_str());
            assert_eq!(
                retained["clock"],
                serde_json::to_value(&observed_captures[capture_epoch].clock).unwrap()
            );
            assert_eq!(retained["qualified"], false);
            samples += retained["samples"].as_array().unwrap().len();
            if let Some(next) = retained["next"].as_u64() {
                assert!(next > after);
                after = next;
            } else {
                assert_eq!(
                    u64::try_from(samples).unwrap(),
                    retained["total_samples"].as_u64().unwrap()
                );
                assert!(samples > 0);
                retained_recordings.push(json!({"epoch":capture_epoch,"samples":samples,
                    "retirement":retained["retirement"],"source_binding_preserved":true,"clock_preserved":true}));
                break;
            }
        }
    }
    let resumed = activate(&url, &page, &prepared, 2);
    heartbeat(&url, &resumed);
    post(&url, "/player/stop", b"", &resumed);
    let entry = catalog["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_version"].as_str() == Some(slow_source.as_str()))
        .unwrap();
    let replacement = exchange(&url, "POST", "/media/prepare", &serde_json::to_vec(&json!({
        "catalog_generation":catalog["catalog_generation"], "entry_id":entry["entry_id"],
        "presentation_anchor_us":"5000000", "elapsed_anchor_us":"0", "allow_declared_clock":true,
    })).unwrap(), Some(&page), &[], HTTP_BUDGET);
    assert_ne!(
        replacement.status, 200,
        "replacement discarded original preparation without release"
    );
    let release = post_json(
        &url,
        "/media/release",
        json!({"selection_id":prepared["selection_id"]}),
        &page,
    );
    assert_eq!(release["current"], false);
    let slow = prepare(&url, &page, &catalog, &slow_source);
    assert_retired_media(&url, media_path);
    let slow_active = activate(&url, &page, &slow, 1);
    let mut slow_pulses = Heartbeats::start(url.clone(), slow_active.clone());
    let slow_path = slow["url"].as_str().unwrap();
    let mut slow_readers = Vec::new();
    for _ in 0..2 {
        slow_readers.push(begin_slow_reader(&url, slow_path));
    }
    thread::sleep(Duration::from_millis(500));
    let slow_stop_began = Instant::now();
    let (slow_pulses_count, slow_max_us) = slow_pulses.join();
    post(&url, "/player/stop", b"", &slow_active);
    let slow_stop_us = slow_stop_began.elapsed().as_micros();
    assert!(
        slow_stop_us < 1_000_000,
        "slow media writes blocked explicit Stop"
    );
    let release = post_json(
        &url,
        "/media/release",
        json!({"selection_id":slow["selection_id"]}),
        &page,
    );
    assert_eq!(release["current"], false);
    for reader in slow_readers {
        let _ = reader.shutdown(Shutdown::Both);
    }
    assert_retired_media(&url, slow_path);
    let denied = exchange(&url, "POST", "/player/start", &serde_json::to_vec(&json!({"source_id":slow["selection_id"],"media_selection_id":slow["selection_id"],"generation":2,"time_basis":"source_media_presentation"})).unwrap(), Some(&page), &[], HTTP_BUDGET);
    assert_ne!(denied.status, 200, "released media rearmed a player");
    let ClientBody::LiveCapture(after) = execute(
        &mut client,
        &project,
        Some(revision),
        Command::LiveCaptureStatus {
            epoch: epoch.clone(),
        },
    ) else {
        panic!("retired capture")
    };
    assert!(!after.active);
    assert_eq!(
        after.options.browser_media.as_ref(),
        native.options.browser_media.as_ref()
    );
    server.join();
    let mut report = json!({
        "passed":true,"qualified":false,"actual_engine":true,"rendered_browser":false,"rendered_desktop":false,"device_output":false,
        "media_sha256":MEDIA_SHA,"media_bytes":source_bytes.len(),"source_version":source,"placement_id":placement.placement_id,
        "revision":revision,"capture_epoch":epoch,"capture_active":false,"server_joined":true,
        "head_full_and_ranges":true,"range_reconstruction_sha256":format!("{:x}",Sha256::digest(&reconstructed)),
        "suffix_open_and_416":true,"foreign_origin_host_page_refused":true,"prepared_does_not_activate":true,
        "native_source_placement_clock":true,"retained_recording_provenance":true,"pause_keeps_media":true,"resume_generation":true,
        "source_replacement_retires_url":true,"unreleased_replacement_refused":true,"explicit_release_refuses_old_url_and_player":true,
        "slow_media_readers":2,"slow_media_bytes":16*1024*1024,"slow_media_preserves_control":true,
        "independent_heartbeats":first_pulses+slow_pulses_count,"maximum_heartbeat_us":first_max_us.max(slow_max_us),"slow_stop_us":slow_stop_us,
        "diagnostic_position":position,"intentional_start_capture_count":1,"media_clock_qualified":false,"submitted_pixels_decoded_from_media":false,
    });
    report["capture_chain"] = json!(capture_chain);
    report["prepared_continuation_count"] = json!(final_handoffs);
    report["prepared_continuation_sustained"] = json!(true);
    report["stream_wall_us"] = json!(stream_wall_us);
    report["first_handoff_wall_us"] = json!(first_handoff_wall_us);
    report["frames_submitted"] = json!(ordinal);
    report["continuation_trace"] = json!(trace);
    report["retained_recordings"] = json!(retained_recordings);
    std::fs::write(result_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
