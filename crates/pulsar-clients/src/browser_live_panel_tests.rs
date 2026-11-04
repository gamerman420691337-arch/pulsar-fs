use super::*;
use std::time::{Duration, Instant};

fn binding() -> Binding {
    (
        ProjectId::new("browser-project").unwrap(),
        RevisionId::new(0),
    )
}

fn wait_joined(panel: &mut BrowserLivePanel, current: Option<Binding>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while panel.busy() {
        panel.poll(current.clone());
        assert!(
            Instant::now() < deadline,
            "owned browser server did not join"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pending_panel() -> (BrowserLivePanel, Arc<AtomicBool>) {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = cancelled.clone();
    let joined = Arc::new(AtomicBool::new(false));
    let completed = joined.clone();
    let (sender, ready) = mpsc::sync_channel(1);
    sender.send("http://127.0.0.1:12345/".into()).unwrap();
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cancellation.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "test cancellation missing");
            std::thread::sleep(Duration::from_millis(1));
        }
        completed.store(true, Ordering::Release);
        Ok(())
    });
    panel.running = Some(Running {
        binding: binding(),
        cancelled,
        ready,
        thread,
    });
    (panel, joined)
}

#[test]
fn changed_project_or_revision_cancels_and_late_ready_does_not_escape() {
    for current in [
        None,
        Some((binding().0, RevisionId::new(1))),
        Some((
            ProjectId::new("another-project").unwrap(),
            RevisionId::new(0),
        )),
    ] {
        let (mut panel, joined) = pending_panel();
        panel.poll(current.clone());
        assert!(panel.url.is_none());
        assert!(panel
            .running
            .as_ref()
            .unwrap()
            .cancelled
            .load(Ordering::Acquire));
        wait_joined(&mut panel, current);
        assert!(joined.load(Ordering::Acquire));
    }
}

#[test]
fn duplicate_launch_refuses_and_drop_cancels_and_joins() {
    let (mut panel, joined) = pending_panel();
    panel.poll(Some(binding()));
    assert_eq!(panel.url.as_deref(), Some("http://127.0.0.1:12345/"));
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(panel.busy());
    drop(panel);
    assert!(joined.load(Ordering::Acquire));
}

#[test]
fn invalid_launch_bounds_and_missing_config_never_publish_a_url() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.duration_seconds = 0;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
    panel.duration_seconds = 300;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    let root = tempfile::tempdir().unwrap();
    panel.options_path = root
        .path()
        .join("missing.json")
        .to_string_lossy()
        .into_owned();
    panel.start(binding(), egui::Context::default()).unwrap();
    wait_joined(&mut panel, Some(binding()));
    assert!(panel.status.contains("error"));
    assert!(panel.url.is_none());
}

#[test]
fn headless_panel_displays_without_launching_or_connecting() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    let context = egui::Context::default();
    let output = context.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| panel.show(ui, Some(binding()), None, None));
    });
    assert!(!output.shapes.is_empty());
    assert!(!panel.busy());
}

#[test]
fn guard_is_absent_by_default_and_requires_consent_and_absolute_path() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    assert!(panel.capture_guard().unwrap().is_none());
    panel.guard_path = "/unused/guard.json".into();
    assert!(panel
        .capture_guard()
        .err()
        .unwrap()
        .to_string()
        .contains("Explicitly allow"));
    panel.allow_guard_monitoring = true;
    panel.guard_path.clear();
    assert!(panel
        .capture_guard()
        .err()
        .unwrap()
        .to_string()
        .contains("Choose a Handy stop guard"));
    panel.guard_path = "relative-guard.json".into();
    assert!(panel
        .capture_guard()
        .err()
        .unwrap()
        .to_string()
        .contains("must be absolute"));
    assert!(!panel.busy());
}

fn guard_json() -> serde_json::Value {
    serde_json::json!({
        "selection": {
            "acquisition_request_id": "guard-acquisition",
            "controller_id": "guard-controller",
            "expected_generation": 1,
            "play_request_id": "guard-play",
            "play_record_id": "a".repeat(64)
        },
        "policy": {
            "heartbeat_timeout_ms": 1000,
            "media_stall_timeout_ms": 1000,
            "challenge_timeout_ms": 500
        }
    })
}

#[test]
fn guard_snapshot_preserves_selection_and_policy_after_file_changes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("guard.json");
    std::fs::write(&path, serde_json::to_vec(&guard_json()).unwrap()).unwrap();
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.guard_path = path.to_string_lossy().into_owned();
    panel.allow_guard_monitoring = true;
    let guard = panel.capture_guard().unwrap().unwrap();
    std::fs::write(&path, b"invalid replacement").unwrap();
    assert_eq!(guard.selection.controller_id.as_str(), "guard-controller");
    assert_eq!(guard.selection.play_request_id.as_str(), "guard-play");
    assert_eq!(guard.policy.heartbeat_timeout_ms, 1000);
    assert_eq!(guard.policy.media_stall_timeout_ms, 1000);
    assert_eq!(guard.policy.challenge_timeout_ms, 500);
    assert!(panel.capture_guard().is_err());
}

#[test]
fn guard_rejects_secrets_invalid_policy_and_oversized_file_before_spawn() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("guard.json");
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.guard_path = path.to_string_lossy().into_owned();
    panel.allow_guard_monitoring = true;
    panel.options_source = OptionsSource::Generator;
    panel.form.inference = Some(temporal_inference());
    let mut secret = guard_json();
    secret["owner_proof"] = serde_json::json!("forbidden-placeholder");
    std::fs::write(&path, serde_json::to_vec(&secret).unwrap()).unwrap();
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
    let mut invalid_policy = guard_json();
    invalid_policy["policy"]["heartbeat_timeout_ms"] = serde_json::json!(0);
    std::fs::write(&path, serde_json::to_vec(&invalid_policy).unwrap()).unwrap();
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
    std::fs::write(&path, vec![b' '; 65 * 1024]).unwrap();
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
}

fn temporal_inference() -> LiveCaptureInference {
    let profile: DeclaredInferenceProfile = serde_json::from_value(serde_json::json!({
        "schema_version":1,
        "model":{"artifact_id":"a".repeat(64),"sha256":"a".repeat(64),"byte_len":302},
        "runtime":{"artifact_id":"b".repeat(64),"sha256":"b".repeat(64),"byte_len":1000},
        "preprocessing":{"input":{"width":64,"height":64,"channels":6,"layout":"nchw","class_count":1,"decoder":"temporal_script_position"},
            "resize":"nearest_stretch","color":"rgb_frame_pair","scale":vec![1.0f32/255.0;6],"bias":vec![0.0;6],"padding_byte":0},
        "provider":"cpu","minimum_memory_bytes":134217728,"minimum_output_bytes":1048576,"cpu_threads":2
    })).unwrap();
    profile.validate().unwrap();
    LiveCaptureInference {
        model_path: "/explicit/temporal.onnx".into(),
        profile,
    }
}

#[test]
fn direct_live_settings_preserve_explicit_units_and_model_identity() {
    let form = LiveForm {
        pack: None,
        pack_profile: None,
        inference: Some(temporal_inference()),
        maximum_age_ms: 1234,
        added_buffer_ms: 23,
        lease_ms: 4567,
        project_start_ms: 890,
    };
    let options = form.build().unwrap();
    assert_eq!(options.policy.maximum_analysis_age_ns, 1_234_000_000);
    assert_eq!(options.policy.added_buffer_ns, 23_000_000);
    assert_eq!(options.project_anchor.as_nanos(), 890_000_000);
    assert_eq!(options.lease_ms, 4567);
    assert_eq!(
        options.inference.unwrap().profile.model.sha256,
        "a".repeat(64)
    );
    assert!(options.encoded_source.is_none());
    assert_eq!(
        options.source_time_basis,
        LiveSourceTimeBasis::ProducerDeclared
    );
}

#[test]
fn invalid_direct_settings_refuse_before_worker_creation() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.options_source = OptionsSource::Generator;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    panel.form.inference = Some(temporal_inference());
    panel.form.project_start_ms = u64::MAX;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    panel.form.project_start_ms = 0;
    panel.form.maximum_age_ms = 0;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    panel.form.maximum_age_ms = 1000;
    panel.form.added_buffer_ms = 30_001;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    panel.form.added_buffer_ms = 0;
    panel.form.lease_ms = 0;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
}

#[test]
fn failed_capture_and_current_generator_changes_preserve_the_reviewed_snapshot() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    let mut generator = temporal_inference();
    panel.capture_generator(generator.clone()).unwrap();
    let captured = serde_json::to_vec(&panel.form.build().unwrap()).unwrap();
    generator.model_path = "/different/temporal.onnx".into();
    assert_eq!(
        serde_json::to_vec(&panel.form.build().unwrap()).unwrap(),
        captured
    );
    generator.profile.preprocessing.input.width = 32;
    assert!(panel.capture_generator(generator).is_err());
    assert_eq!(
        serde_json::to_vec(&panel.form.build().unwrap()).unwrap(),
        captured
    );
    let mut relative = temporal_inference();
    relative.model_path = "relative.onnx".into();
    assert!(panel.capture_generator(relative).is_err());
    assert_eq!(
        serde_json::to_vec(&panel.form.build().unwrap()).unwrap(),
        captured
    );
}

#[test]
fn a_running_server_refuses_generator_recapture() {
    let (mut panel, joined) = pending_panel();
    assert!(panel.capture_generator(temporal_inference()).is_err());
    assert!(panel.form.inference.is_none());
    panel.stop();
    wait_joined(&mut panel, Some(binding()));
    assert!(joined.load(Ordering::Acquire));
}

#[test]
fn pack_capture_keeps_choice_without_extracting_a_raw_file() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    let choice = ModelPackChoice {
        manifest_sha256: "c".repeat(64),
        selection_revision: 7,
    };
    panel
        .capture_pack(choice.clone(), temporal_inference().profile)
        .unwrap();
    let options = panel.form.build().unwrap();
    assert_eq!(options.model_pack, Some(choice));
    assert!(options.inference.is_none());
    assert!(panel.form.inference.is_none());
    panel.options_source = OptionsSource::Generator;
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
    let (mut running, joined) = pending_panel();
    assert!(running
        .capture_pack(options.model_pack.unwrap(), temporal_inference().profile)
        .is_err());
    running.stop();
    wait_joined(&mut running, Some(binding()));
    assert!(joined.load(Ordering::Acquire));
}

#[test]
#[ignore = "requires a private real Engine and an explicit authored temporal fixture"]
fn native_desktop_browser_server_owns_active_capture_shutdown() {
    use crate::session::EngineApi;
    use pulsar_protocol::{AttemptId, Command};
    use std::io::{Read, Write};
    use std::net::TcpStream;

    #[derive(Clone)]
    struct Scope {
        page: serde_json::Value,
        player: Option<serde_json::Value>,
        player_capability: Option<String>,
    }
    impl Scope {
        fn headers(&self) -> String {
            let mut headers = format!(
                "X-Pulsar-Launch: {}\r\nX-Pulsar-Page: {}\r\nX-Pulsar-Page-Capability: {}\r\n",
                self.page["launch_id"].as_str().unwrap(),
                self.page["page_id"].as_str().unwrap(),
                self.page["page_capability"].as_str().unwrap(),
            );
            if let Some(player) = &self.player {
                headers.push_str(&format!(
                    "X-Pulsar-Player: {}\r\nX-Pulsar-Source: {}\r\nX-Pulsar-Generation: {}\r\nX-Pulsar-Player-Capability: {}\r\n",
                    player["player_id"].as_str().unwrap(),
                    player["source_id"].as_str().unwrap(),
                    player["generation"].as_u64().unwrap(),
                    self.player_capability.as_deref().unwrap(),
                ));
            }
            headers
        }
    }
    fn authority(url: &str) -> &str {
        url.strip_prefix("http://")
            .unwrap()
            .strip_suffix('/')
            .unwrap()
    }
    fn http_response(
        url: &str,
        path: &str,
        body: Option<&[u8]>,
        scope: Option<&Scope>,
        timeout: Duration,
    ) -> (u16, Vec<u8>) {
        let authority = authority(url);
        let mut stream =
            TcpStream::connect_timeout(&authority.parse().unwrap(), Duration::from_secs(3))
                .unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        if let Some(body) = body {
            let headers = scope.map(Scope::headers).unwrap_or_default();
            write!(stream, "POST {path} HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nContent-Type: application/octet-stream\r\n{headers}Content-Length: {}\r\n\r\n", body.len()).unwrap();
            stream.write_all(body).unwrap();
        } else {
            write!(stream, "GET / HTTP/1.1\r\nHost: {authority}\r\n\r\n").unwrap();
        }
        let mut response = Vec::new();
        stream
            .take(2 * 1024 * 1024)
            .read_to_end(&mut response)
            .unwrap();
        let offset = response.windows(4).position(|s| s == b"\r\n\r\n").unwrap() + 4;
        let status = std::str::from_utf8(&response[..offset])
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, response[offset..].to_vec())
    }
    fn http(url: &str, path: &str, body: Option<&[u8]>, scope: Option<&Scope>) -> Vec<u8> {
        let (status, response) = http_response(url, path, body, scope, Duration::from_secs(20));
        assert_eq!(status, 200, "HTTP fixture request refused: {path}");
        response
    }
    fn page_scope(url: &str) -> Scope {
        let page = String::from_utf8(http(url, "/", None, None)).unwrap();
        assert!(page.contains("requestVideoFrameCallback("));
        let encoded = page
            .split_once("const PAGE_SCOPE = ")
            .unwrap()
            .1
            .split_once(';')
            .unwrap()
            .0;
        Scope {
            page: serde_json::from_str(encoded).unwrap(),
            player: None,
            player_capability: None,
        }
    }
    fn heartbeat(url: &str, scope: &Scope) {
        let (status, response) = http_response(
            url,
            "/player/challenge",
            Some(b""),
            Some(scope),
            Duration::from_secs(1),
        );
        assert_eq!(status, 200, "independent player heartbeat refused");
        let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(response["pending"], true);
        assert_eq!(response["configured"], false);
    }
    struct Heartbeat {
        cancelled: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<usize>>,
    }
    impl Heartbeat {
        fn start(url: String, scope: Scope) -> Self {
            let cancelled = Arc::new(AtomicBool::new(false));
            let cancellation = cancelled.clone();
            let thread = std::thread::spawn(move || {
                let mut count = 0;
                while !cancellation.load(Ordering::Acquire) {
                    heartbeat(&url, &scope);
                    count += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                count
            });
            Self {
                cancelled,
                thread: Some(thread),
            }
        }
        fn stop(&mut self) -> usize {
            self.cancelled.store(true, Ordering::Release);
            self.thread.take().unwrap().join().unwrap()
        }
    }
    impl Drop for Heartbeat {
        fn drop(&mut self) {
            self.cancelled.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
    fn ready(panel: &mut BrowserLivePanel, current: &Binding) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        while panel.url.is_none() {
            panel.poll(Some(current.clone()));
            assert!(panel.busy(), "server startup failed: {}", panel.status);
            assert!(Instant::now() < deadline, "server startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        panel.url.clone().unwrap()
    }
    let endpoint = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_ENDPOINT").unwrap());
    let bootstrap = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_BOOTSTRAP").unwrap());
    let options = std::env::var("PULSAR_TEST_BROWSER_OPTIONS").unwrap();
    let result_path = PathBuf::from(std::env::var_os("PULSAR_TEST_BROWSER_RESULTS").unwrap());
    let current = (
        ProjectId::new(std::env::var("PULSAR_TEST_BROWSER_PROJECT").unwrap()).unwrap(),
        RevisionId::new(0),
    );
    let mut client = SessionClient::connect(&endpoint, &bootstrap).unwrap();
    let mut epochs = Vec::new();
    let mut inference_position = None;
    for mode in [
        "direct_options",
        "explicit_stop",
        "revision_change",
        "owner_drop",
        "expiry",
    ] {
        let mut panel = BrowserLivePanel::new(endpoint.clone(), bootstrap.clone());
        panel.options_path = options.clone();
        panel.duration_seconds = if mode == "expiry" { 2 } else { 60 };
        if mode == "direct_options" {
            let input: LiveCaptureOptions =
                serde_json::from_slice(&std::fs::read(&options).unwrap()).unwrap();
            panel.capture_generator(input.inference.unwrap()).unwrap();
            panel.form.maximum_age_ms = 1500;
            panel.form.added_buffer_ms = 20;
            panel.form.project_start_ms = 750;
            panel.form.lease_ms = 12_000;
        }
        panel
            .start(current.clone(), egui::Context::default())
            .unwrap();
        if mode == "direct_options" {
            panel.form.inference = None;
            panel.form.lease_ms = 0;
            panel.options_source = OptionsSource::File;
            panel.options_path = "/missing/changed-after-launch.json".into();
        }
        let url = ready(&mut panel, &current);
        let mut scope = page_scope(&url);
        let source_id = uuid::Uuid::new_v4().to_string();
        let registration = serde_json::to_vec(&serde_json::json!({
            "source_id":source_id,
            "generation":1,
            "time_basis":"source_media_presentation",
        }))
        .unwrap();
        let registered: serde_json::Value = serde_json::from_slice(&http(
            &url,
            "/player/start",
            Some(&registration),
            Some(&scope),
        ))
        .unwrap();
        assert_eq!(registered["player"]["launch_id"], scope.page["launch_id"]);
        assert_eq!(registered["player"]["page_id"], scope.page["page_id"]);
        assert_eq!(registered["player"]["source_id"], source_id);
        assert_eq!(registered["player"]["generation"], 1);
        assert_eq!(registered["guard"]["configured"], false);
        scope.player = Some(registered["player"].clone());
        scope.player_capability =
            Some(registered["player_capability"].as_str().unwrap().to_owned());
        heartbeat(&url, &scope);
        if mode == "direct_options" {
            let another_page = page_scope(&url);
            assert_eq!(another_page.page["launch_id"], scope.page["launch_id"]);
            assert_ne!(another_page.page["page_id"], scope.page["page_id"]);
            let (status, _) = http_response(
                &url,
                "/player/start",
                Some(&registration),
                Some(&another_page),
                Duration::from_secs(1),
            );
            assert_eq!(status, 409, "another page replaced the active player");
            heartbeat(&url, &scope);

            let authority = authority(&url);
            let mut incomplete = Vec::new();
            for _ in 0..8 {
                let mut stream =
                    TcpStream::connect_timeout(&authority.parse().unwrap(), Duration::from_secs(3))
                        .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                write!(stream, "POST /frame HTTP/1.1\r\nHost: {authority}\r\nOrigin: http://{authority}\r\nContent-Type: application/octet-stream\r\n{}Content-Length: 1024\r\n\r\npartial", scope.headers()).unwrap();
                incomplete.push(stream);
            }
            let heartbeat_started = Instant::now();
            heartbeat(&url, &scope);
            assert!(
                heartbeat_started.elapsed() < Duration::from_secs(1),
                "incomplete ingress blocked heartbeat"
            );
            drop(incomplete);
        }
        let mut heartbeats = Heartbeat::start(url.clone(), scope.clone());
        let started: serde_json::Value = serde_json::from_slice(&http(&url, "/start", Some(b"{\"media_us\":0,\"recording_maximum\":128,\"source_time_basis\":\"source_media_presentation\"}"), Some(&scope))).unwrap();
        assert_eq!(started["active"], true);
        if mode == "direct_options" {
            assert_eq!(
                started["options"]["policy"]["maximum_analysis_age_ns"],
                1_500_000_000u64
            );
            assert_eq!(
                started["options"]["policy"]["added_buffer_ns"],
                20_000_000u64
            );
            assert_eq!(started["options"]["project_anchor"], 750_000_000u64);
            assert_eq!(started["options"]["lease_ms"], 12_000);
        }
        let epoch = AttemptId::new(started["identity"]["epoch"].as_str().unwrap()).unwrap();
        if mode == "explicit_stop" || mode == "direct_options" {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut ordinal = 0;
            let mut mode_position = None;
            while mode_position.is_none() {
                let mut body = serde_json::to_vec(
                    &serde_json::json!({"epoch":epoch,"media_us":ordinal*100000}),
                )
                .unwrap();
                body.push(b'\n');
                body.extend(vec![64; pulsar_protocol::LIVE_FRAME_BYTES]);
                let report: serde_json::Value =
                    serde_json::from_slice(&http(&url, "/frame", Some(&body), Some(&scope)))
                        .unwrap();
                mode_position =
                    report["inference"]["latest"]["result"]["payload"]["position"].as_f64();
                ordinal += 1;
                assert!(
                    Instant::now() < deadline,
                    "native diagnostic estimate absent"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            assert!((mode_position.unwrap() - (0.25 + 0.5 * 64.0 / 255.0)).abs() < 0.0001);
            inference_position = mode_position;
        }
        let heartbeat_count = heartbeats.stop();
        if mode == "explicit_stop" || mode == "direct_options" {
            if mode == "direct_options" {
                let stopped: serde_json::Value =
                    serde_json::from_slice(&http(&url, "/player/stop", Some(b""), Some(&scope)))
                        .unwrap();
                assert_eq!(stopped["active"], false);
                assert_eq!(stopped["capture_cleanup_pending"], true);
                let (status, _) = http_response(
                    &url,
                    "/player/challenge",
                    Some(b""),
                    Some(&scope),
                    Duration::from_secs(1),
                );
                assert_eq!(status, 409, "retired player accepted another heartbeat");
                let retained: serde_json::Value =
                    serde_json::from_slice(&http(&url, "/recordings", Some(b""), Some(&scope)))
                        .unwrap();
                assert!(
                    retained["recordings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|entry| entry["epoch"] == epoch.as_str()),
                    "retired player lost its original recording selection"
                );
                let read =
                    serde_json::to_vec(&serde_json::json!({"epoch":epoch,"after":0})).unwrap();
                let page: serde_json::Value = serde_json::from_slice(&http(
                    &url,
                    "/recording-page",
                    Some(&read),
                    Some(&scope),
                ))
                .unwrap();
                assert_eq!(page["identity"]["epoch"], epoch.as_str());
                assert_eq!(
                    page["active"], false,
                    "player stop did not finish capture cleanup before reading retained recording"
                );
                assert!(!page["samples"].as_array().unwrap().is_empty());
            }
            panel.stop();
        } else if mode == "revision_change" {
            panel.poll(Some((current.0.clone(), RevisionId::new(1))));
            assert!(panel.url.is_none());
        } else if mode == "owner_drop" {
            drop(panel);
            panel = BrowserLivePanel::new(endpoint.clone(), bootstrap.clone());
        }
        wait_joined(&mut panel, Some(current.clone()));
        assert!(TcpStream::connect_timeout(
            &authority(&url).parse().unwrap(),
            Duration::from_secs(3),
        )
        .is_err());
        client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(10)));
        let report = client
            .execute(
                Command::LiveCaptureStatus {
                    epoch: epoch.clone(),
                },
                Some(current.0.clone()),
                Some(current.1),
            )
            .unwrap();
        let crate::motion::ResponseBody::LiveCapture(report) = report else {
            panic!("capture status absent")
        };
        assert!(!report.active);
        epochs.push(
            serde_json::json!({"mode":mode,"epoch":epoch,"active":false,"server_joined":true,"independent_heartbeats":heartbeat_count}),
        );
    }
    std::fs::write(result_path, serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"qualified":false,"captures":epochs,"direct_form_admission":true,"launch_snapshot_preserved":true,"page_player_capabilities":true,"page_get_does_not_replace_player":true,"incomplete_ingress_preserves_heartbeat":true,"exact_player_stop":true,"retired_scope_recording_read":true,"diagnostic_position":inference_position,"rendered_desktop":false,"device_output":false})).unwrap()).unwrap();
}

#[test]
fn prepared_browser_config_is_explicit_absolute_and_contains_no_owner_capability() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    assert!(panel.capture_playback().unwrap().is_none());
    panel.playback_path = "/unused/prepared.json".into();
    assert!(panel.capture_playback().is_err());
    panel.allow_playback_controls = true;
    panel.playback_path = "relative.json".into();
    assert!(panel
        .capture_playback()
        .unwrap_err()
        .to_string()
        .contains("absolute"));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("prepared.json");
    panel.playback_path = path.to_string_lossy().into_owned();
    let mut value = serde_json::json!({"acquisition_request_id":"acquisition","controller_id":"controller","expected_generation":0,"clock_request_id":"clock", "qualification":{"dossier_id":"a".repeat(64),"revision":2},"renewal_interval_ms":100,"device_refresh_interval_ms":250});
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let config = panel.capture_playback().unwrap().unwrap();
    assert_eq!(config.controller_id.as_str(), "controller");
    value["owner_proof"] = serde_json::json!("synthetic-forbidden-field");
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(panel.capture_playback().is_err());
}

#[test]
fn hsp_is_disabled_by_default_and_requires_a_file_enablement_and_absolute_path() {
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    assert!(panel.capture_hsp().unwrap().is_none());
    panel.hsp_path = "/unused/hsp.json".into();
    assert!(panel
        .capture_hsp()
        .unwrap_err()
        .to_string()
        .contains("explicit enablement"));
    panel.allow_hsp_controls = true;
    panel.hsp_path.clear();
    assert!(panel
        .capture_hsp()
        .unwrap_err()
        .to_string()
        .contains("configuration file"));
    panel.hsp_path = "relative-hsp.json".into();
    assert!(panel
        .capture_hsp()
        .unwrap_err()
        .to_string()
        .contains("must be absolute"));
    assert!(!panel.busy());
    assert!(panel.url.is_none());
}

fn hsp_json() -> serde_json::Value {
    serde_json::json!({
        "engine_epoch": "hsp-engine-epoch",
        "device_session": "hsp-device-session",
        "device_qualification": {"dossier_id": "a".repeat(64), "revision": 2},
        "source_qualification": {"dossier_id": "b".repeat(64), "revision": 2},
        "preparation_timeout_ms": 10000
    })
}

#[test]
fn hsp_snapshot_keeps_the_native_selection_after_the_file_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hsp.json");
    std::fs::write(&path, serde_json::to_vec(&hsp_json()).unwrap()).unwrap();
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.hsp_path = path.to_string_lossy().into_owned();
    panel.allow_hsp_controls = true;
    let config = panel.capture_hsp().unwrap().unwrap();
    std::fs::write(&path, b"invalid replacement").unwrap();
    assert_eq!(config.engine_epoch.as_str(), "hsp-engine-epoch");
    assert_eq!(config.device_session.as_str(), "hsp-device-session");
    assert_eq!(
        config.device_qualification.dossier_id.as_str(),
        "a".repeat(64)
    );
    assert_eq!(config.device_qualification.revision, 2);
    assert_eq!(
        config.source_qualification.dossier_id.as_str(),
        "b".repeat(64)
    );
    assert_eq!(config.source_qualification.revision, 2);
    assert_eq!(config.preparation_timeout_ms, 10000);
    assert!(config.finite_drain_policy_sha256.is_none());
    assert!(config.continuity_duration_ms.is_none());
    assert!(panel.capture_hsp().is_err());
}

#[test]
fn hsp_rejects_unknown_fields_invalid_selection_and_oversized_config_before_spawn() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hsp.json");
    let mut panel =
        BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
    panel.hsp_path = path.to_string_lossy().into_owned();
    panel.allow_hsp_controls = true;
    panel.options_source = OptionsSource::Generator;
    panel.form.inference = Some(temporal_inference());
    for field in ["owner_proof", "api_key", "connection_key"] {
        let mut value = hsp_json();
        value[field] = serde_json::json!("synthetic-forbidden-field");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            panel.start(binding(), egui::Context::default()).is_err(),
            "{field}"
        );
        assert!(!panel.busy());
    }
    for mutation in 0..3 {
        let mut value = hsp_json();
        match mutation {
            0 => value["preparation_timeout_ms"] = serde_json::json!(0),
            1 => value["device_session"] = value["engine_epoch"].clone(),
            _ => value["source_qualification"]["revision"] = serde_json::json!(1),
        }
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            panel.start(binding(), egui::Context::default()).is_err(),
            "{mutation}"
        );
        assert!(!panel.busy());
    }
    std::fs::write(&path, vec![b' '; 65 * 1024]).unwrap();
    assert!(panel.start(binding(), egui::Context::default()).is_err());
    assert!(!panel.busy());
    assert!(panel.url.is_none());
}

#[test]
fn handy_modes_conflict_before_any_configuration_read_or_thread_spawn() {
    for selected in [
        [true, true, false],
        [true, false, true],
        [false, true, true],
    ] {
        let mut panel =
            BrowserLivePanel::new("/unused/engine.sock".into(), "/unused/pairing.token".into());
        if selected[0] {
            panel.guard_path = "/unused/guard.json".into();
            panel.allow_guard_monitoring = true;
        }
        if selected[1] {
            panel.playback_path = "/unused/prepared.json".into();
            panel.allow_playback_controls = true;
        }
        if selected[2] {
            panel.hsp_path = "/unused/hsp.json".into();
            panel.allow_hsp_controls = true;
        }
        assert!(panel
            .start(binding(), egui::Context::default())
            .unwrap_err()
            .to_string()
            .contains("Choose only one Handy mode"));
        assert!(!panel.busy());
        assert!(panel.url.is_none());
    }
}
