//! Loopback scheduling tests with a bounded fake Preview owner, not Engine/device qualification.
use super::*;

#[test]
fn timing_routes_keep_player_scope_and_existing_bounded_ingress() {
    let scope = scope();
    for path in [
        "/player/timing-probe",
        "/player/timing-observe",
        "/player/timing-status",
    ] {
        let received = Instant::now();
        let request = parse_request(
            &post_bytes("127.0.0.1:1", path, &scope, b""),
            "127.0.0.1:1",
            received,
        )
        .unwrap();
        assert_eq!(request.path, path);
        assert_eq!(request.scope.player, scope.player);
        assert_eq!(request.scope.source, scope.source);
        assert_eq!(request.scope.generation, scope.generation);
        assert_eq!(request.scope.player_capability, scope.player_capability);
        assert_eq!(request.received, received);
        assert!(parse_request(
            &post_bytes("127.0.0.1:1", path, &scope, &[0; 4097]),
            "127.0.0.1:1",
            received
        )
        .is_err());
    }
}

#[test]
fn concurrent_shutdown_failures_preserve_original_guard_and_capture_identities() {
    let error = joined_results(
        Err(anyhow::anyhow!("incomplete request")),
        Err(anyhow::anyhow!(
            "binding request original-bind remains unconfirmed"
        )),
        Err(anyhow::anyhow!(
            "capture original-capture cleanup remains pending"
        )),
    )
    .unwrap_err()
    .to_string();
    for expected in ["incomplete request", "original-bind", "original-capture"] {
        assert!(error.contains(expected), "lost diagnostic: {expected}");
    }
    joined_results(Ok(()), Ok(()), Ok(())).unwrap();
}
use std::{
    net::{Ipv4Addr, Shutdown, SocketAddr},
    path::Path,
    sync::Mutex,
};

const TEST_IO: Duration = Duration::from_secs(2);
const START_BODY: &[u8] = br#"{"media_us":0,"source_time_basis":"source_media_presentation"}"#;

fn scope() -> Scope {
    Scope {
        launch: "launch-test".into(),
        page: "page-test".into(),
        page_capability: "page-capability-test".into(),
        player: Some("player-test".into()),
        source: Some("source-test".into()),
        generation: Some(7),
        player_capability: Some("player-capability-test".into()),
    }
}

fn post_bytes(authority: &str, path: &str, scope: &Scope, body: &[u8]) -> Vec<u8> {
    let mut header = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nX-Pulsar-Launch: {}\r\nX-Pulsar-Page: {}\r\nX-Pulsar-Page-Capability: {}\r\n",
        body.len(), scope.launch, scope.page, scope.page_capability
    );
    for (name, value) in [
        ("X-Pulsar-Player", scope.player.clone()),
        ("X-Pulsar-Source", scope.source.clone()),
        (
            "X-Pulsar-Generation",
            scope.generation.map(|n| n.to_string()),
        ),
        (
            "X-Pulsar-Player-Capability",
            scope.player_capability.clone(),
        ),
    ] {
        if let Some(value) = value {
            header.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    header.push_str("\r\n");
    let mut bytes = header.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn socket_pair() -> (TcpStream, Connection, String) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let peer = TcpStream::connect_timeout(&address, TEST_IO).unwrap();
    peer.set_read_timeout(Some(TEST_IO)).unwrap();
    peer.set_write_timeout(Some(TEST_IO)).unwrap();
    let deadline = Instant::now() + TEST_IO;
    let server = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "local accept deadline expired");
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("local accept failed: {error}"),
        }
    };
    (peer, Connection::new(server).unwrap(), address.to_string())
}

fn collect_partial(connection: &mut Connection, authority: &str, expected_bytes: usize) {
    let deadline = Instant::now() + TEST_IO;
    loop {
        assert!(connection.read(authority).unwrap().is_none());
        if connection.bytes.len() == expected_bytes {
            return;
        }
        assert!(Instant::now() < deadline, "partial input deadline expired");
        thread::sleep(Duration::from_millis(1));
    }
}

fn complete_request(connection: &mut Connection, authority: &str) -> Request {
    let deadline = Instant::now() + TEST_IO;
    loop {
        if let Some(request) = connection.read(authority).unwrap() {
            return request;
        }
        assert!(Instant::now() < deadline, "complete input deadline expired");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn parser_preserves_every_exact_scope_field_and_received_timestamp() {
    let authority = "127.0.0.1:34567";
    let scope = scope();
    let bytes = post_bytes(authority, "/player/observe", &scope, b"{}");
    let received = Instant::now();
    let request = parse_request(&bytes, authority, received).unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/player/observe");
    assert_eq!(request.body, b"{}");
    assert_eq!(request.received, received);
    assert_eq!(request.scope.launch, scope.launch);
    assert_eq!(request.scope.page, scope.page);
    assert_eq!(request.scope.page_capability, scope.page_capability);
    assert_eq!(request.scope.player, scope.player);
    assert_eq!(request.scope.source, scope.source);
    assert_eq!(request.scope.generation, scope.generation);
    assert_eq!(request.scope.player_capability, scope.player_capability);
}

#[test]
fn parser_refuses_duplicate_scope_headers_invalid_generations_and_origin_changes() {
    let authority = "127.0.0.1:34567";
    let valid =
        String::from_utf8(post_bytes(authority, "/player/challenge", &scope(), b"")).unwrap();
    for changed in [
        valid.replace("\r\n\r\n", "\r\nx-pulsar-page: another-page\r\n\r\n"),
        valid.replace("X-Pulsar-Generation: 7", "X-Pulsar-Generation: -1"),
        valid.replace(
            "X-Pulsar-Generation: 7",
            "X-Pulsar-Generation: 18446744073709551616",
        ),
        valid.replace(
            "Origin: http://127.0.0.1:34567",
            "Origin: http://127.0.0.1:34568",
        ),
        valid.replace("Host: 127.0.0.1:34567", "Host: example.invalid"),
        valid.replace(
            "Content-Length: 0",
            "Content-Length: 0\r\nTransfer-Encoding: chunked",
        ),
        valid.replace(
            "Content-Length: 0",
            "Content-Length: 0\r\ncontent-length: 0",
        ),
    ] {
        assert!(parse_request(changed.as_bytes(), authority, Instant::now()).is_err());
    }
}

#[test]
fn parser_enforces_exact_body_length_player_limit_and_bulk_limit() {
    let authority = "127.0.0.1:34567";
    for (route, limit) in [("/player/observe", 4096), ("/frame", BODY_LIMIT)] {
        let full = post_bytes(authority, route, &scope(), &vec![b'x'; limit]);
        assert!(parse_request(&full, authority, Instant::now()).is_ok());
        assert!(parse_request(&full[..full.len() - 1], authority, Instant::now()).is_err());
        let mut extra = full;
        extra.push(b'x');
        assert!(parse_request(&extra, authority, Instant::now()).is_err());
        let too_large = post_bytes(authority, route, &scope(), &vec![b'x'; limit + 1]);
        assert!(parse_request(&too_large, authority, Instant::now()).is_err());
    }
}

#[test]
fn incomplete_header_does_not_block_a_different_connection_step() {
    let (mut partial_peer, mut partial, partial_authority) = socket_pair();
    let prefix = format!("POST /player/challenge HTTP/1.1\r\nHost: {partial_authority}\r\n");
    partial_peer.write_all(prefix.as_bytes()).unwrap();
    collect_partial(&mut partial, &partial_authority, prefix.len());
    let (mut peer, mut connection, authority) = socket_pair();
    peer.write_all(&post_bytes(&authority, "/player/challenge", &scope(), b""))
        .unwrap();
    assert_eq!(
        complete_request(&mut connection, &authority).path,
        "/player/challenge"
    );
    assert!(partial.read(&partial_authority).unwrap().is_none());
    assert!(!partial.waiting);
}

#[test]
fn incomplete_body_does_not_block_a_different_connection_and_resumes_exactly() {
    let (mut partial_peer, mut partial, partial_authority) = socket_pair();
    let bytes = post_bytes(
        &partial_authority,
        "/player/observe",
        &scope(),
        b"0123456789",
    );
    partial_peer.write_all(&bytes[..bytes.len() - 5]).unwrap();
    collect_partial(&mut partial, &partial_authority, bytes.len() - 5);
    let (mut peer, mut connection, authority) = socket_pair();
    peer.write_all(&post_bytes(&authority, "/player/stop", &scope(), b""))
        .unwrap();
    assert_eq!(
        complete_request(&mut connection, &authority).path,
        "/player/stop"
    );
    partial_peer.write_all(b"56789").unwrap();
    assert_eq!(
        complete_request(&mut partial, &partial_authority).body,
        b"0123456789"
    );
}

#[test]
fn slow_reader_gets_only_a_bounded_write_step_while_another_socket_advances() {
    let (_slow_peer, mut slow, _) = socket_pair();
    slow.reply(200, "application/octet-stream", vec![b'x'; 1024 * 1024])
        .unwrap();
    assert!(!slow.write().unwrap());
    assert!(slow.written <= 16 * 1024);
    let (mut peer, mut connection, authority) = socket_pair();
    peer.write_all(&post_bytes(&authority, "/player/stop", &scope(), b""))
        .unwrap();
    assert_eq!(
        complete_request(&mut connection, &authority).path,
        "/player/stop"
    );
    slow.deadline = Instant::now() - Duration::from_millis(1);
    let before = slow.written;
    assert!(slow.write().is_err());
    assert_eq!(slow.written, before);
}

#[test]
fn incomplete_peer_eof_and_expired_input_are_bounded_failures() {
    let (mut peer, mut connection, authority) = socket_pair();
    peer.write_all(b"POST /player/challenge HTTP/1.1\r\n")
        .unwrap();
    peer.shutdown(Shutdown::Write).unwrap();
    let deadline = Instant::now() + TEST_IO;
    loop {
        if connection.read(&authority).is_err() {
            break;
        }
        assert!(Instant::now() < deadline, "EOF was not observed");
        thread::sleep(Duration::from_millis(1));
    }
    let (_peer, mut expired, authority) = socket_pair();
    expired.deadline = Instant::now() - Duration::from_millis(1);
    assert!(expired.read(&authority).is_err());
}

#[test]
fn ingress_rejects_pipelining_and_oversized_announced_bodies_before_dispatch() {
    for payload in [
        "POST /player/challenge HTTP/1.1\r\nContent-Length: 0\r\n\r\nextra".to_owned(),
        format!(
            "POST /frame HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            BODY_LIMIT + 1
        ),
        format!("POST /frame HTTP/1.1\r\nX-Padding: {}", "x".repeat(8192)),
    ] {
        let (mut peer, mut connection, authority) = socket_pair();
        peer.write_all(payload.as_bytes()).unwrap();
        let deadline = Instant::now() + TEST_IO;
        loop {
            match connection.read(&authority) {
                Err(_) => break,
                Ok(None) => {
                    assert!(Instant::now() < deadline, "invalid request was not refused");
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(Some(_)) => panic!("invalid ingress was dispatched"),
            }
        }
    }
}

struct GatedPreview {
    blocked_route: &'static str,
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<Mutex<Vec<String>>>,
    blocked_once: bool,
    active: bool,
}

impl PreviewBackend for GatedPreview {
    fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    fn operation_deadline(&mut self, _deadline: Instant) -> Result<()> {
        Ok(())
    }
    fn active(&self) -> bool {
        self.active
    }
    fn request(&mut self, request: &Request) -> Result<serde_json::Value> {
        self.calls.lock().unwrap().push(request.path.clone());
        if request.path == self.blocked_route && !self.blocked_once {
            self.blocked_once = true;
            self.entered.try_send(())?;
            self.release.recv_timeout(Duration::from_secs(4))?;
        }
        if matches!(request.path.as_str(), "/start" | "/frame") {
            self.active = true;
        }
        Ok(serde_json::json!({"completed_path":request.path,"source":request.scope.source}))
    }
    fn stop(&mut self) -> Result<()> {
        self.active = false;
        self.calls.lock().unwrap().push("stop".into());
        Ok(())
    }
}

struct TestServer {
    address: SocketAddr,
    authority: String,
    entered: mpsc::Receiver<()>,
    release: mpsc::SyncSender<()>,
    cancelled: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<Result<()>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    fn new(blocked_route: &'static str) -> Self {
        let client = SessionClient::from_credentials(
            Path::new("/mnt/4tb/tmp/pulsar-player-http-tests-unused.sock"),
            SessionId::new("synthetic-http-test-session").unwrap(),
            "synthetic-http-test-credential".into(),
        )
        .unwrap();
        let players = PlayerControl::new(
            &client,
            ProjectId::new("synthetic-http-test-project").unwrap(),
            RevisionId::new(0),
            None,
        )
        .unwrap();
        let (entered_tx, entered) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let preview = GatedPreview {
            blocked_route,
            entered: entered_tx,
            release: release_rx,
            calls: calls.clone(),
            blocked_once: false,
            active: false,
        };
        let thread = thread::spawn(move || {
            run(preview, players, 0, 10, stop, |url| {
                ready_tx.try_send(url)?;
                Ok(())
            })
        });
        // Install the join owner before waiting for startup, so assertion failures also join.
        let mut server = Self {
            address: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            authority: String::new(),
            entered,
            release,
            cancelled,
            thread: Some(thread),
            calls,
        };
        let url = ready.recv_timeout(TEST_IO).unwrap();
        server.authority = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .into();
        server.address = server.authority.parse().unwrap();
        server
    }
    fn send(&self, bytes: &[u8]) -> TcpStream {
        let mut socket = TcpStream::connect_timeout(&self.address, TEST_IO).unwrap();
        socket.set_read_timeout(Some(TEST_IO)).unwrap();
        socket.set_write_timeout(Some(TEST_IO)).unwrap();
        socket.write_all(bytes).unwrap();
        socket
    }
    fn post(&self, route: &str, scope: &Scope, body: &[u8]) -> (u16, serde_json::Value) {
        let socket = self.send(&post_bytes(&self.authority, route, scope, body));
        let (status, bytes) = Self::response(socket);
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    fn response(mut socket: TcpStream) -> (u16, Vec<u8>) {
        let mut bytes = Vec::new();
        Read::by_ref(&mut socket)
            .take(1024 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        let split = bytes.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
        let header = std::str::from_utf8(&bytes[..split]).unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, bytes[split..].to_vec())
    }
    fn page(&self) -> Scope {
        let socket =
            self.send(format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", self.authority).as_bytes());
        let (status, body) = Self::response(socket);
        assert_eq!(status, 200);
        let html = String::from_utf8(body).unwrap();
        let value = html
            .split_once("const PAGE_SCOPE = ")
            .unwrap()
            .1
            .split_once(';')
            .unwrap()
            .0;
        let value: serde_json::Value = serde_json::from_str(value).unwrap();
        Scope {
            launch: value["launch_id"].as_str().unwrap().into(),
            page: value["page_id"].as_str().unwrap().into(),
            page_capability: value["page_capability"].as_str().unwrap().into(),
            ..Scope::default()
        }
    }
    fn start_player(&self, page: &Scope, source: &str, generation: u64) -> Scope {
        let body = serde_json::to_vec(&serde_json::json!({
            "source_id":source,"generation":generation,"time_basis":"source_media_presentation"
        }))
        .unwrap();
        let (status, value) = self.post("/player/start", page, &body);
        assert_eq!(status, 200, "{value}");
        Scope {
            player: Some(value["player"]["player_id"].as_str().unwrap().into()),
            source: Some(value["player"]["source_id"].as_str().unwrap().into()),
            generation: value["player"]["generation"].as_u64(),
            player_capability: Some(value["player_capability"].as_str().unwrap().into()),
            ..page.clone()
        }
    }
    fn finish(mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.release.try_send(());
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.release.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn stalled_start_frame_and_editor_do_not_block_control_or_publish_into_replacements() {
    for route in ["/start", "/frame", "/prepare-candidate"] {
        let server = TestServer::new(route);
        let page = server.page();
        let first = server.start_player(&page, "source-one", 1);
        if route != "/start" {
            assert_eq!(server.post("/start", &first, START_BODY).0, 200);
        }
        let body = if route == "/start" { START_BODY } else { b"{}" };
        let pending = server.send(&post_bytes(&server.authority, route, &first, body));
        server.entered.recv_timeout(TEST_IO).unwrap();
        let (status, heartbeat) = server.post("/player/challenge", &first, b"");
        assert_eq!(status, 200, "{route}: {heartbeat}");
        assert_eq!(
            heartbeat,
            serde_json::json!({"pending":true,"configured":false})
        );
        let (status, stopped) = server.post("/player/stop", &first, b"");
        assert_eq!(status, 200, "{route}: {stopped}");
        assert_eq!(stopped["capture_cleanup_pending"], true);
        let second = server.start_player(&page, "source-two", 2);
        for old_route in ["/player/stop", "/player/challenge", "/stop"] {
            assert_eq!(server.post(old_route, &first, b"").0, 409, "{old_route}");
        }
        server.release.try_send(()).unwrap();
        let (status, late) = TestServer::response(pending);
        assert_eq!(
            status,
            409,
            "late {route}: {}",
            String::from_utf8_lossy(&late)
        );
        let late: serde_json::Value = serde_json::from_slice(&late).unwrap();
        assert!(late["error"].as_str().unwrap().contains("scope retired"));
        assert_eq!(server.post("/player/challenge", &second, b"").0, 200);
        let (status, current) = server.post("/start", &second, START_BODY);
        assert_eq!(status, 200, "{current}");
        assert_eq!(current["source"], "source-two");
        assert!(server
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call == "stop"));
        server.finish();
    }
}

#[test]
fn actual_http_loop_keeps_control_available_with_incomplete_clients() {
    let server = TestServer::new("/unused");
    let page = server.page();
    let player = server.start_player(&page, "source-one", 1);
    let _header = server.send(b"POST /frame HTTP/1.1\r\n");
    let body = post_bytes(&server.authority, "/frame", &player, b"1234567890");
    let _body = server.send(&body[..body.len() - 5]);
    assert_eq!(server.post("/player/challenge", &player, b"").0, 200);
    assert_eq!(server.post("/player/stop", &player, b"").0, 200);
    assert!(server
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|call| call == "stop"));
    server.finish();
}

#[test]
fn actual_http_page_get_and_old_pagehide_cannot_stop_replacement() {
    let server = TestServer::new("/unused");
    let old_page = server.page();
    let old = server.start_player(&old_page, "source-one", 1);
    let new_page = server.page();
    assert_eq!(server.post("/player/challenge", &old, b"").0, 200);
    assert_eq!(server.post("/player/stop", &old, b"").0, 200);
    let new = server.start_player(&new_page, "source-two", 2);
    // These are the two scoped keepalive operations sent by an old pagehide handler.
    assert_eq!(server.post("/player/stop", &old, b"").0, 409);
    assert_eq!(server.post("/stop", &old, b"").0, 409);
    assert_eq!(server.post("/player/challenge", &new, b"").0, 200);
    server.finish();
}

#[test]
fn hsp_routes_keep_exact_scope_origin_and_small_bodies() {
    let scope = scope();
    for path in [
        "/player/hsp-inspect",
        "/player/hsp-prepare",
        "/player/hsp-preparation-status",
        "/player/hsp-cancel",
        "/player/hsp-bind",
        "/player/hsp-play",
        "/player/hsp-stop",
        "/player/hsp-status",
        "/player/hsp-continuity-enable",
        "/player/hsp-continuity-status",
    ] {
        let now = Instant::now();
        let wire = post_bytes("127.0.0.1:1", path, &scope, b"");
        let request = parse_request(&wire, "127.0.0.1:1", now).unwrap();
        assert_eq!(request.path, path);
        assert_eq!(request.scope.player_capability, scope.player_capability);
        assert_eq!(request.scope.source, scope.source);
        assert_eq!(request.scope.generation, scope.generation);
        assert!(parse_request(
            &post_bytes("127.0.0.1:1", path, &scope, &[0; 4097]),
            "127.0.0.1:1",
            now
        )
        .is_err());
        let foreign = String::from_utf8(wire).unwrap().replace(
            "Origin: http://127.0.0.1:1",
            "Origin: http://foreign.invalid",
        );
        assert!(parse_request(foreign.as_bytes(), "127.0.0.1:1", now).is_err());
    }
}

#[test]
fn hsp_status_and_cleanup_routes_bypass_a_held_preview_operation() {
    let server = TestServer::new("/start");
    let page = server.page();
    let player = server.start_player(&page, "source-hsp", 1);
    let pending = server.send(&post_bytes(
        &server.authority,
        "/start",
        &player,
        START_BODY,
    ));
    server.entered.recv_timeout(TEST_IO).unwrap();
    let (code, view) = server.post("/player/hsp-status", &player, b"");
    assert_eq!(code, 200);
    assert_eq!(view["configured"], false);
    assert_eq!(view["phase"], "not_configured");
    // No configured owner in this HTTP scheduling fixture. These direct
    // refusals still must arrive while the preview owner remains held.
    for path in ["/player/hsp-stop", "/player/hsp-cancel"] {
        assert_eq!(server.post(path, &player, b"").0, 409);
    }
    assert_eq!(server.post("/player/stop", &player, b"").0, 200);
    server.release.try_send(()).unwrap();
    assert_eq!(TestServer::response(pending).0, 409);
    server.finish();
}

#[test]
fn queued_hsp_source_inspection_cannot_publish_into_a_replacement_player() {
    let server = TestServer::new("/start");
    let page = server.page();
    let old = server.start_player(&page, "old-hsp-source", 1);
    let start = server.send(&post_bytes(&server.authority, "/start", &old, START_BODY));
    server.entered.recv_timeout(TEST_IO).unwrap();
    let inspection = server.send(&post_bytes(
        &server.authority,
        "/player/hsp-inspect",
        &old,
        b"",
    ));
    assert_eq!(server.post("/player/stop", &old, b"").0, 200);
    let next = server.start_player(&page, "next-hsp-source", 2);
    assert_eq!(server.post("/player/hsp-status", &old, b"").0, 409);
    server.release.try_send(()).unwrap();
    assert_eq!(TestServer::response(start).0, 409);
    assert_eq!(TestServer::response(inspection).0, 409);
    let (code, view) = server.post("/player/hsp-inspect", &next, b"");
    assert_eq!(code, 200);
    assert_eq!(view["configured"], false);
    assert!(
        !server
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with("/player/hsp-")),
        "HSP source inspection must use the typed native snapshot seam, never Preview request JSON"
    );
    server.finish();
}
