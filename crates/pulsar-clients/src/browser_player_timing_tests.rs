//! Real local IPC with synthetic timing receipts; no media/device qualification.
use super::*;

#[test]
fn timing_diagnostic_retains_sanitized_transport_phase() {
    use crate::session::BrowserTransportPhase;
    for (phase, expected) in [
        (BrowserTransportPhase::Connect, "connect"),
        (BrowserTransportPhase::Deadline, "deadline"),
        (BrowserTransportPhase::Call, "call"),
    ] {
        let error = anyhow::Error::from(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "private path and token",
        ))
        .context(phase)
        .context(FailureStage::Exchange);
        let encoded = serde_json::to_value(FailureDiagnostic::from_error("open", &error)).unwrap();
        assert_eq!(encoded["category"], "transport_timeout");
        assert_eq!(encoded["transport_phase"], expected);
        assert!(!encoded.to_string().contains("private path and token"));
    }
}
use std::os::unix::net::UnixListener;

const NATIVE: u64 = (1_u64 << 53) + 17;

#[cfg(target_os = "linux")]
include!("browser_player_timing_native_tests.rs");

#[test]
fn failed_ordinary_probe_retains_diagnostic_but_exact_release_settles_cleanup() {
    for release in ["inactive", "foreign", "current", "drop"] {
        let mut report = None;
        let mut server = Server::new(move |request| match &request.command {
            Command::OpenBrowserTiming { .. } => {
                let value = opened(request);
                report = Some(value.clone());
                body(value)
            }
            Command::IssueBrowserTimingProbe { .. } => Some(Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "browser timing binding is not current",
            ))),
            Command::ReleaseBrowserTiming { binding_id, player } => {
                let mut value = report.take().unwrap();
                assert_eq!(binding_id, &value.binding.binding_id);
                assert_eq!(player, &value.binding.player);
                if release == "drop" {
                    return None;
                }
                value.current = release == "current";
                if release == "foreign" {
                    value.binding.binding_id = ArtifactId::new("foreign-release").unwrap();
                }
                body(value)
            }
            _ => panic!("failed timing must not retry or reopen"),
        });
        let mut timing = Timing::new(&server.client()).unwrap();
        let player = player();
        timing
            .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
            .unwrap();
        ready(&timing);
        timing
            .handle("/player/timing-probe", &player, b"", 1)
            .unwrap();
        assert!(reply(&timing, &player, 1).is_err());
        let cleanup = timing.shutdown();
        let records = state(&timing.shared);
        let record = &records.entries[0];
        assert!(record.retired);
        if release == "inactive" {
            assert!(record.release == ReleaseState::Acknowledged);
            assert!(!record.report.as_ref().unwrap().current);
            assert!(
                cleanup.is_ok(),
                "exact retirement must settle custody: {cleanup:?}"
            );
        } else {
            assert!(record.release == ReleaseState::Unknown);
            assert!(cleanup
                .unwrap_err()
                .to_string()
                .contains("release_unconfirmed"));
        }
        assert!(
            record.first_failure.is_some(),
            "do not erase the failed exchange"
        );
        drop(records);
        assert_eq!(server.finish().len(), 3);
    }
}

#[test]
fn retirement_after_worker_skip_before_open_dispatch_does_not_create_unknown_custody() {
    let mut server = Server::new(|_| panic!("retired Open must never reach the Engine"));
    let mut timing = Timing::new(&server.client()).unwrap();
    let (entered, observed) = mpsc::sync_channel(1);
    let (resume, released) = mpsc::sync_channel(1);
    state(&timing.shared).before_execute = Some(Arc::new(TestBeforeExecute {
        entered,
        resume: Mutex::new(released),
    }));
    let player = player();
    timing
        .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    observed.recv_timeout(Duration::from_secs(2)).unwrap();
    timing.retire(&player);
    resume.send(()).unwrap();
    ready(&timing);
    timing.shutdown().unwrap();
    assert!(server.finish().is_empty());
}

#[test]
fn failure_diagnostics_allowlist_fields_without_formatting_private_errors() {
    let private = "private-authentication-path".repeat(2048);
    let cases = [
        (
            anyhow::Error::from(ProtocolError::invalid(private.clone()))
                .context(FailureStage::Exchange),
            "protocol_rejected",
            Some(ErrorCode::InvalidRequest),
        ),
        (
            anyhow::Error::from(ProtocolError::invalid(
                "browser timing scope or sequence differs",
            ))
            .context(FailureStage::Exchange),
            "scope_or_sequence",
            Some(ErrorCode::InvalidRequest),
        ),
        (
            anyhow::Error::from(ProtocolError::new(
                ErrorCode::Unavailable,
                "browser timing binding is not current",
            ))
            .context(FailureStage::Exchange),
            "binding_not_current",
            Some(ErrorCode::Unavailable),
        ),
        (
            anyhow::Error::from(pulsar_protocol::TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                private.clone(),
            )))
            .context(FailureStage::Exchange),
            "transport_timeout",
            None,
        ),
        (
            anyhow::Error::from(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                private.clone(),
            ))
            .context(FailureStage::Exchange),
            "transport_timeout",
            None,
        ),
        (
            anyhow::Error::from(pulsar_protocol::TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                private.clone(),
            )))
            .context(FailureStage::Exchange),
            "transport_io",
            None,
        ),
        (
            anyhow::Error::from(pulsar_protocol::TransportError::ResponseMismatch)
                .context(FailureStage::Exchange),
            "transport_framing",
            None,
        ),
        (
            anyhow::Error::msg(private.clone()).context(FailureStage::Reply),
            "reply_validation",
            None,
        ),
        (
            anyhow::Error::msg(private.clone()).context(FailureStage::Exchange),
            "exchange_unclassified",
            None,
        ),
        (anyhow::Error::msg(private), "local_state", None),
    ];
    for (error, category, protocol_code) in cases {
        let value = FailureDiagnostic::from_error("observe", &error);
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(encoded.len() < 192);
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("authentication"));
        assert_eq!(value.category, category);
        assert_eq!(value.protocol_code, protocol_code);
    }
}

#[test]
fn original_exchange_failure_survives_later_unknown_release_without_retry() {
    let mut server = Server::new(|request| match &request.command {
        Command::OpenBrowserTiming { .. } => body(opened(request)),
        Command::IssueBrowserTimingProbe { .. } => Some(Err(ProtocolError::invalid(
            "browser timing scope or sequence differs",
        ))),
        Command::ReleaseBrowserTiming { .. } => Some(Err(ProtocolError::new(
            ErrorCode::Unavailable,
            "private-authentication-path",
        ))),
        _ => panic!("unexpected timing command"),
    });
    let mut timing = Timing::new(&server.client()).unwrap();
    let player = player();
    timing
        .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    ready(&timing);
    timing
        .handle("/player/timing-probe", &player, b"", 1)
        .unwrap();
    assert!(reply(&timing, &player, 1).is_err());
    {
        let records = state(&timing.shared);
        let record = &records.entries[0];
        assert!(record.retired && record.release == ReleaseState::Unknown);
        let failure = diagnostic(record);
        assert_eq!(failure["first_failure"]["operation"], "probe");
        assert_eq!(failure["first_failure"]["stage"], "exchange");
        assert_eq!(failure["first_failure"]["category"], "scope_or_sequence");
        assert_eq!(failure["first_failure"]["protocol_code"], "invalid_request");
        assert!(!failure.to_string().contains("private"));
    }
    let error = timing.shutdown().unwrap_err().to_string();
    assert!(error.contains("cleanup unconfirmed"));
    assert!(!error.contains("authentication"));
    let calls = server.finish();
    assert_eq!(calls.len(), 3, "unknown operations must not be replayed");
    assert_eq!(
        calls
            .iter()
            .filter(|c| matches!(c, Command::ReleaseBrowserTiming { .. }))
            .count(),
        1
    );
}

fn media() -> BrowserMediaBinding {
    BrowserMediaBinding {
        preparation_request_id: RequestId::new("synthetic-source-prepare").unwrap(),
        lease_id: ArtifactId::new("synthetic-source-lease").unwrap(),
        engine_epoch: AttemptId::new("synthetic-engine").unwrap(),
        project_id: ProjectId::new("synthetic-project").unwrap(),
        revision: RevisionId::new(1),
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("synthetic-source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("synthetic-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("synthetic-placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("synthetic-source").unwrap(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(1, 3).unwrap(),
                    project_anchor: ProjectTime::from_nanos(-500_000_000),
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
        lease_ms: 60_000,
    }
}
fn player() -> HandyPlayerIdentity {
    HandyPlayerIdentity {
        launch_id: AttemptId::new("launch").unwrap(),
        page_id: AttemptId::new("page").unwrap(),
        player_id: AttemptId::new("player").unwrap(),
        source_id: AttemptId::new("source-selection").unwrap(),
        generation: 1,
        time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
    }
}
fn opened(request: &pulsar_protocol::Request) -> BrowserTimingReport {
    let Command::OpenBrowserTiming {
        player,
        browser_media,
        policy,
    } = &request.command
    else {
        panic!("expected original Open");
    };
    let report = BrowserTimingReport {
        binding: BrowserTimingBinding {
            request_id: request.request_id.clone(),
            binding_id: ArtifactId::new(format!("timing-{}", request.request_id)).unwrap(),
            engine_epoch: browser_media.engine_epoch.clone(),
            project_id: browser_media.project_id.clone(),
            revision: browser_media.revision,
            player: player.clone(),
            browser_media: browser_media.clone(),
            policy: policy.clone(),
        },
        current: true,
        observation: None,
        qualified: false,
    };
    report.validate().unwrap();
    report
}
fn frame() -> BrowserTimingFrame {
    BrowserTimingFrame {
        sample: HandyPlayerSample {
            browser_tick_us: 10000,
            media_time_us: 100000,
            presented_frames: 1,
            playing: true,
            seeking: false,
            ended: false,
            visible: true,
            playback_rate_milli: 1000,
        },
        callback_tick_us: 9999,
        presentation_time_us: Some(9500),
        expected_display_time_us: Some(10500),
    }
}
fn probe(report: &BrowserTimingReport, sequence: u64) -> BrowserTimingProbe {
    BrowserTimingProbe {
        binding_id: report.binding.binding_id.clone(),
        player: report.binding.player.clone(),
        sequence,
        probe_id: ArtifactId::new("synthetic-probe").unwrap(),
        issued_at_boottime_ns: NATIVE,
        expires_at_boottime_ns: NATIVE + 1_000_000_000,
    }
}
fn observe(report: &mut BrowserTimingReport, frame: BrowserTimingFrame, sequence: u64) {
    report.observation = Some(BrowserTimingObservation {
        observation_id: ArtifactId::new("synthetic-observation").unwrap(),
        binding_id: report.binding.binding_id.clone(),
        sequence,
        bracket: BrowserClockBracket {
            native_issue_ns: NATIVE,
            native_receive_ns: NATIVE + 37,
            browser_sample_us: frame.sample.browser_tick_us,
        },
        project_phase: report
            .binding
            .browser_media
            .clock
            .project_at(
                SourceTimestamp::new(frame.sample.media_time_us as i64, 1_000_000).unwrap(),
                &report.binding.browser_media.selection.placement.spec,
            )
            .unwrap(),
        frame,
        expires_at_boottime_ns: NATIVE + 3_000_000_000,
    });
    report.validate().unwrap();
}

struct Server {
    directory: tempfile::TempDir,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Vec<Command>>>,
}
impl Server {
    fn new(
        mut respond: impl FnMut(
                &pulsar_protocol::Request,
            )
                -> Option<std::result::Result<pulsar_protocol::ResponseBody, ProtocolError>>
            + Send
            + 'static,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(5);
            let mut calls = Vec::new();
            while !stop.load(Ordering::Acquire) && Instant::now() < end {
                let (mut socket, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("synthetic socket accept: {error}"),
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let request: pulsar_protocol::Request = read_message(&mut socket).unwrap();
                calls.push(request.command.clone());
                if let Some(reply) = respond(&request) {
                    let response = match reply {
                        Ok(body) => Response::success(request.request_id, body),
                        Err(error) => Response::failure(request.request_id, error),
                    };
                    write_message(&mut socket, &response).unwrap();
                }
            }
            calls
        });
        Self {
            directory,
            stopped,
            worker: Some(worker),
        }
    }
    fn client(&self) -> SessionClient {
        SessionClient::from_credentials(
            &self.directory.path().join("engine.sock"),
            SessionId::new("synthetic-session").unwrap(),
            "synthetic-authentication-never-displayed".into(),
        )
        .unwrap()
    }
    fn finish(&mut self) -> Vec<Command> {
        self.stopped.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn wait_until(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(2);
    while !ready() {
        assert!(Instant::now() < end, "synthetic timing deadline");
        thread::sleep(Duration::from_millis(1));
    }
}
fn ready(timing: &Timing) {
    wait_until(|| {
        state(&timing.shared)
            .entries
            .first()
            .is_some_and(|r| !r.opening && !r.busy)
    });
}
fn reply(timing: &Timing, player: &HandyPlayerIdentity, ticket: u64) -> Result<Value> {
    let mut value = None;
    wait_until(|| {
        for (actual, result) in timing.poll(Some(player), true) {
            assert_eq!(actual, ticket);
            value = Some(result);
        }
        value.is_some()
    });
    value.unwrap()
}
fn body(
    report: BrowserTimingReport,
) -> Option<std::result::Result<pulsar_protocol::ResponseBody, ProtocolError>> {
    Some(Ok(pulsar_protocol::ResponseBody::BrowserTiming(Box::new(
        report,
    ))))
}

#[test]
fn independent_timing_lane_keeps_exact_large_clock_fractional_phase_and_releases_once() {
    let mut report = None;
    let mut server = Server::new(move |request| match &request.command {
        Command::OpenBrowserTiming { .. } => {
            let value = opened(request);
            report = Some(value.clone());
            body(value)
        }
        Command::IssueBrowserTimingProbe { sequence, .. } => {
            Some(Ok(pulsar_protocol::ResponseBody::BrowserTimingProbe(
                Box::new(probe(report.as_ref().unwrap(), *sequence)),
            )))
        }
        Command::ObserveBrowserTiming {
            frame, sequence, ..
        } => {
            let value = report.as_mut().unwrap();
            observe(value, frame.clone(), *sequence);
            body(value.clone())
        }
        Command::BrowserTimingStatus { .. } => body(report.clone().unwrap()),
        Command::ReleaseBrowserTiming { .. } => {
            let value = report.as_mut().unwrap();
            value.current = false;
            body(value.clone())
        }
        _ => panic!("unrelated operation"),
    });
    assert!(!server.directory.path().join("handy-owner.token").exists());
    let mut timing = Timing::new(&server.client()).unwrap();
    let player = player();
    timing
        .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    ready(&timing);
    assert!(matches!(
        timing
            .handle("/player/timing-probe", &player, b"", 1)
            .unwrap(),
        ControlReply::Pending
    ));
    let challenge = reply(&timing, &player, 1).unwrap();
    assert_eq!(challenge["sequence"], "1");
    assert!(challenge.get("issued_at_boottime_ns").is_none());
    let request = serde_json::to_vec(
        &json!({"sequence":"1","probe_id":challenge["probe_id"],"frame":frame()}),
    )
    .unwrap();
    timing
        .handle("/player/timing-observe", &player, &request, 2)
        .unwrap();
    let observed = reply(&timing, &player, 2).unwrap();
    assert_eq!(observed["observation"]["bracket_width_ns"], "37");
    assert_eq!(
        observed["observation"]["project_phase"],
        json!({"numerator":"-11","denominator":"15"})
    );
    assert_eq!(observed["qualified"], false);
    assert!(!observed.to_string().contains(&NATIVE.to_string()));
    timing
        .handle("/player/timing-status", &player, b"", 3)
        .unwrap();
    assert_eq!(reply(&timing, &player, 3).unwrap(), observed);
    timing.retire(&player);
    timing.retire(&player);
    timing.shutdown().unwrap();
    let calls = server.finish();
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls
            .iter()
            .filter(|c| matches!(c, Command::ReleaseBrowserTiming { .. }))
            .count(),
        1
    );
}

#[test]
fn cancelled_late_open_is_released_before_join_and_never_published_into_replacement() {
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (allow, allowed) = mpsc::sync_channel(1);
    let mut report = None;
    let mut server = Server::new(move |request| match &request.command {
        Command::OpenBrowserTiming { .. } => {
            entered_tx.send(()).unwrap();
            allowed.recv_timeout(Duration::from_secs(2)).unwrap();
            let value = opened(request);
            report = Some(value.clone());
            body(value)
        }
        Command::ReleaseBrowserTiming { binding_id, player } => {
            let mut value = report.take().unwrap();
            assert_eq!(binding_id, &value.binding.binding_id);
            assert_eq!(player, &value.binding.player);
            value.current = false;
            body(value)
        }
        _ => panic!("late Open must only release its exact result"),
    });
    let mut timing = Timing::new(&server.client()).unwrap();
    let player = player();
    let cancelled = Arc::new(AtomicBool::new(false));
    timing
        .open(player.clone(), cancelled.clone(), media())
        .unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        matches!(timing.handle("/player/timing-probe", &player, b"", 1).unwrap(), ControlReply::Ready(v) if v["pending"] == true)
    );
    cancelled.store(true, Ordering::Release);
    timing.retire(&player);
    allow.send(()).unwrap();
    timing.shutdown().unwrap();
    assert!(timing.poll(None, false).is_empty());
    let calls = server.finish();
    assert_eq!(calls.len(), 2);
    assert!(matches!(calls[1], Command::ReleaseBrowserTiming { .. }));
}

#[test]
fn dropped_open_keeps_original_identity_without_reopening_and_explicit_status_only_recovers_history(
) {
    for inspect in [false, true] {
        let mut original = None;
        let mut server = Server::new(move |request| match &request.command {
            Command::OpenBrowserTiming { .. } => {
                assert!(original.is_none());
                original = Some(opened(request));
                None
            }
            Command::BrowserTimingOpenStatus {
                original_request_id,
            } => {
                let mut report = original.clone().unwrap();
                assert_eq!(original_request_id, &report.binding.request_id);
                report.current = false;
                body(report)
            }
            _ => panic!("unknown Open may not mint or release an invented binding"),
        });
        let mut timing = Timing::new(&server.client()).unwrap();
        let player = player();
        timing
            .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
            .unwrap();
        ready(&timing);
        let original = state(&timing.shared).entries[0].request_id.clone();
        let error = timing
            .handle("/player/timing-probe", &player, b"", 1)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(original.as_str()));
        assert!(!error.contains("authentication"));
        if inspect {
            timing
                .handle("/player/timing-status", &player, b"", 2)
                .unwrap();
            assert_eq!(reply(&timing, &player, 2).unwrap()["current"], false);
            assert!(timing
                .handle("/player/timing-probe", &player, b"", 3)
                .is_err());
            timing.shutdown().unwrap();
        } else {
            assert!(timing
                .shutdown()
                .unwrap_err()
                .to_string()
                .contains(original.as_str()));
        }
        let calls = server.finish();
        assert_eq!(calls.len(), if inspect { 2 } else { 1 });
    }
}

#[test]
fn retiring_inflight_observation_requires_exact_release_but_not_its_cancelled_reply() {
    let (entered_tx, entered) = mpsc::sync_channel(1);
    let (allow, allowed) = mpsc::sync_channel(1);
    let mut report = None;
    let mut server = Server::new(move |request| match &request.command {
        Command::OpenBrowserTiming { .. } => {
            let value = opened(request);
            report = Some(value.clone());
            body(value)
        }
        Command::IssueBrowserTimingProbe { sequence, .. } => {
            Some(Ok(pulsar_protocol::ResponseBody::BrowserTimingProbe(
                Box::new(probe(report.as_ref().unwrap(), *sequence)),
            )))
        }
        Command::ObserveBrowserTiming {
            frame, sequence, ..
        } => {
            entered_tx.send(()).unwrap();
            allowed.recv_timeout(Duration::from_secs(2)).unwrap();
            let value = report.as_mut().unwrap();
            observe(value, frame.clone(), *sequence);
            body(value.clone())
        }
        Command::ReleaseBrowserTiming { binding_id, player } => {
            let value = report.as_mut().unwrap();
            assert_eq!(binding_id, &value.binding.binding_id);
            assert_eq!(player, &value.binding.player);
            value.current = false;
            body(value.clone())
        }
        _ => panic!("retirement must only release the exact original binding"),
    });
    let mut timing = Timing::new(&server.client()).unwrap();
    let player = player();
    timing
        .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    ready(&timing);
    timing
        .handle("/player/timing-probe", &player, b"", 1)
        .unwrap();
    let challenge = reply(&timing, &player, 1).unwrap();
    timing
        .handle(
            "/player/timing-observe",
            &player,
            &serde_json::to_vec(
                &json!({"sequence":"1","probe_id":challenge["probe_id"],"frame":frame()}),
            )
            .unwrap(),
            2,
        )
        .unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    timing.retire(&player);
    allow.send(()).unwrap();
    let cleanup = timing.shutdown();
    let records = state(&timing.shared);
    assert!(records.entries[0].retired);
    assert!(records.entries[0].release == ReleaseState::Acknowledged);
    assert!(!records.entries[0].report.as_ref().unwrap().current);
    assert!(records.entries[0].probe.is_none());
    drop(records);
    assert!(
        cleanup.is_ok(),
        "the exact permanent release must settle a cancelled exchange: {cleanup:?}"
    );
    let replies = timing.poll(None, false);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].0, 2);
    assert!(replies[0].1.is_err());
    let calls = server.finish();
    assert_eq!(calls.len(), 4);
    assert!(matches!(calls[3], Command::ReleaseBrowserTiming { .. }));
}

#[test]
fn cancelled_inflight_probe_settles_only_after_exact_permanent_release() {
    for via_token in [false, true] {
        for exchange in ["late", "drop", "error"] {
            for release in ["inactive", "drop", "foreign", "current"] {
                let (entered_tx, entered) = mpsc::sync_channel(1);
                let (allow, allowed) = mpsc::sync_channel(1);
                let mut report = None;
                let mut server = Server::new(move |request| match &request.command {
                    Command::OpenBrowserTiming { .. } => {
                        let value = opened(request);
                        report = Some(value.clone());
                        body(value)
                    }
                    Command::IssueBrowserTimingProbe { sequence, .. } => {
                        entered_tx.send(()).unwrap();
                        allowed.recv_timeout(Duration::from_secs(2)).unwrap();
                        match exchange {
                            "drop" => None,
                            "error" => Some(Err(ProtocolError::invalid("retired timing scope"))),
                            _ => Some(Ok(pulsar_protocol::ResponseBody::BrowserTimingProbe(
                                Box::new(probe(report.as_ref().unwrap(), *sequence)),
                            ))),
                        }
                    }
                    Command::ReleaseBrowserTiming { binding_id, player } => {
                        let mut value = report.take().unwrap();
                        assert_eq!(binding_id, &value.binding.binding_id);
                        assert_eq!(player, &value.binding.player);
                        if release == "drop" {
                            return None;
                        }
                        value.current = release == "current";
                        if release == "foreign" {
                            value.binding.binding_id = ArtifactId::new("foreign-binding").unwrap();
                        }
                        body(value)
                    }
                    _ => panic!("cancelled exchange must not reopen or retry"),
                });
                let mut timing = Timing::new(&server.client()).unwrap();
                let player = player();
                let cancelled = Arc::new(AtomicBool::new(false));
                timing
                    .open(player.clone(), cancelled.clone(), media())
                    .unwrap();
                ready(&timing);
                timing
                    .handle("/player/timing-probe", &player, b"", 1)
                    .unwrap();
                entered.recv_timeout(Duration::from_secs(2)).unwrap();
                if via_token {
                    cancelled.store(true, Ordering::Release);
                } else {
                    timing.retire(&player);
                }
                allow.send(()).unwrap();
                let cleanup = timing.shutdown();
                let records = state(&timing.shared);
                let record = &records.entries[0];
                assert!(record.retired);
                assert!(record.probe.is_none());
                if release == "inactive" {
                    assert!(record.release == ReleaseState::Acknowledged);
                    assert!(!record.report.as_ref().unwrap().current);
                    assert!(
                        cleanup.is_ok(),
                        "{via_token}/{exchange}/{release}: {cleanup:?}"
                    );
                } else {
                    assert!(record.release == ReleaseState::Unknown);
                    let error = cleanup.unwrap_err().to_string();
                    assert!(error.contains("release_unconfirmed"));
                    assert!(error.contains(record.request_id.as_str()));
                }
                drop(records);
                assert!(timing.current_binding(&player).is_err());
                let replies = timing.poll(None, false);
                assert_eq!(replies.len(), 1);
                assert!(replies[0].1.is_err());
                let calls = server.finish();
                assert_eq!(calls.len(), 3);
                assert!(matches!(calls[2], Command::ReleaseBrowserTiming { .. }));
            }
        }
    }
}

#[test]
fn release_missing_foreign_and_current_replies_remain_uncertain_after_join() {
    for fault in ["drop", "foreign", "current"] {
        let mut report = None;
        let mut server = Server::new(move |request| match &request.command {
            Command::OpenBrowserTiming { .. } => {
                let value = opened(request);
                report = Some(value.clone());
                body(value)
            }
            Command::ReleaseBrowserTiming { .. } => {
                if fault == "drop" {
                    return None;
                }
                let mut value = report.take().unwrap();
                if fault == "foreign" {
                    value.binding.binding_id = ArtifactId::new("foreign-binding").unwrap();
                    value.current = false;
                }
                body(value)
            }
            _ => panic!("no automatic release retry"),
        });
        let mut timing = Timing::new(&server.client()).unwrap();
        let player = player();
        timing
            .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
            .unwrap();
        ready(&timing);
        let original = state(&timing.shared).entries[0].request_id.clone();
        timing.retire(&player);
        let error = timing.shutdown().unwrap_err().to_string();
        assert!(error.contains(original.as_str()));
        assert!(error.contains("release_unconfirmed"));
        assert!(!error.contains("authentication"));
        assert!(timing.worker.is_none());
        assert_eq!(server.finish().len(), 2);
    }
}

#[test]
fn timing_projection_never_rounds_exact_phase_or_native_sequence_into_javascript_numbers() {
    let request = pulsar_protocol::Request {
        version: PROTOCOL_VERSION,
        request_id: RequestId::new("original-open").unwrap(),
        session: Some(SessionId::new("synthetic-session").unwrap()),
        auth_token: None,
        project: Some(media().project_id.clone()),
        expected_revision: Some(media().revision),
        command: Command::OpenBrowserTiming {
            player: player(),
            browser_media: media(),
            policy: policy(),
        },
    };
    let mut report = opened(&request);
    observe(&mut report, frame(), u64::MAX);
    let value = projection(&report);
    assert_eq!(value["observation"]["sequence"], u64::MAX.to_string());
    assert!(value["observation"]["project_phase"]["numerator"].is_string());
    assert!(value["observation"]["project_phase"]["denominator"].is_string());
    assert!(value["observation"].get("bracket").is_none());
}

#[test]
fn acknowledged_retirement_rotates_beyond_record_limit_but_unknown_original_stays_retained() {
    for keep_unknown in [false, true] {
        let mut original = true;
        let mut active = None;
        let mut server = Server::new(move |request| match &request.command {
            Command::OpenBrowserTiming { .. } => {
                let report = opened(request);
                if original && keep_unknown {
                    original = false;
                    return None;
                }
                original = false;
                active = Some(report.clone());
                body(report)
            }
            Command::ReleaseBrowserTiming { binding_id, player } => {
                let mut report = active.take().unwrap();
                assert_eq!(binding_id, &report.binding.binding_id);
                assert_eq!(player, &report.binding.player);
                report.current = false;
                body(report)
            }
            _ => panic!("rotation performs only explicit opens and exact release"),
        });
        let mut timing = Timing::new(&server.client()).unwrap();
        let unknown = if keep_unknown {
            timing
                .open(player(), Arc::new(AtomicBool::new(false)), media())
                .unwrap();
            ready(&timing);
            Some(state(&timing.shared).entries[0].request_id.clone())
        } else {
            None
        };
        for generation in 2..=34 {
            let mut current = player();
            current.generation = generation;
            current.player_id = AttemptId::new(format!("player-{generation}")).unwrap();
            timing
                .open(current.clone(), Arc::new(AtomicBool::new(false)), media())
                .unwrap();
            wait_until(|| {
                state(&timing.shared)
                    .entries
                    .iter()
                    .find(|r| r.player == current)
                    .is_some_and(|r| !r.opening && !r.busy)
            });
            timing.retire(&current);
            wait_until(|| {
                state(&timing.shared)
                    .entries
                    .iter()
                    .find(|r| r.player == current)
                    .is_some_and(|r| r.release == ReleaseState::Acknowledged && !r.busy)
            });
            assert!(state(&timing.shared).entries.len() <= if keep_unknown { 2 } else { 1 });
        }
        match unknown {
            Some(id) => assert!(timing
                .shutdown()
                .unwrap_err()
                .to_string()
                .contains(id.as_str())),
            None => timing.shutdown().unwrap(),
        }
        let calls = server.finish();
        assert_eq!(
            calls
                .iter()
                .filter(|c| matches!(c, Command::ReleaseBrowserTiming { .. }))
                .count(),
            33
        );
    }
}

#[test]
fn a_pending_timing_reply_keeps_its_record_until_consumed_even_after_release() {
    let mut active = None;
    let mut server = Server::new(move |request| match &request.command {
        Command::OpenBrowserTiming { .. } => {
            let report = opened(request);
            active = Some(report.clone());
            body(report)
        }
        Command::BrowserTimingStatus { .. } => body(active.clone().unwrap()),
        Command::ReleaseBrowserTiming { .. } => {
            let mut report = active.take().unwrap();
            report.current = false;
            body(report)
        }
        _ => panic!("unexpected request"),
    });
    let mut timing = Timing::new(&server.client()).unwrap();
    let first = player();
    timing
        .open(first.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    ready(&timing);
    timing
        .handle("/player/timing-status", &first, b"", 1)
        .unwrap();
    wait_until(|| state(&timing.shared).entries[0].pending_reply);
    timing.retire(&first);
    wait_until(|| state(&timing.shared).entries[0].release == ReleaseState::Acknowledged);
    let mut second = first.clone();
    second.generation = 2;
    second.player_id = AttemptId::new("second-player").unwrap();
    timing
        .open(second.clone(), Arc::new(AtomicBool::new(false)), media())
        .unwrap();
    assert_eq!(state(&timing.shared).entries.len(), 2);
    let replies = timing.poll(Some(&second), true);
    assert_eq!(replies.len(), 1);
    assert!(replies[0].1.is_err());
    timing.shutdown().unwrap();
    server.finish();
}

#[test]
fn foreign_probe_or_changed_observation_refuses_and_releases_only_original_binding() {
    for fault in [
        "probe binding",
        "probe player",
        "probe sequence",
        "observation frame",
        "observation bracket",
        "remote error",
    ] {
        let mut active = None;
        let mut server = Server::new(move |request| match &request.command {
            Command::OpenBrowserTiming { .. } => {
                let report = opened(request);
                active = Some(report.clone());
                body(report)
            }
            Command::IssueBrowserTimingProbe { sequence, .. } => {
                let mut value = probe(active.as_ref().unwrap(), *sequence);
                match fault {
                    "probe binding" => value.binding_id = ArtifactId::new("foreign").unwrap(),
                    "probe player" => value.player.generation += 1,
                    "probe sequence" => value.sequence += 1,
                    _ => {}
                }
                Some(Ok(pulsar_protocol::ResponseBody::BrowserTimingProbe(
                    Box::new(value),
                )))
            }
            Command::ObserveBrowserTiming {
                frame, sequence, ..
            } => {
                if fault == "remote error" {
                    return Some(Err(ProtocolError::invalid(
                        "synthetic-authentication-never-displayed raw private path",
                    )));
                }
                let mut report = active.clone().unwrap();
                let mut frame = frame.clone();
                if fault == "observation frame" {
                    frame.sample.media_time_us += 1;
                }
                observe(&mut report, frame, *sequence);
                if fault == "observation bracket" {
                    let observation = report.observation.as_mut().unwrap();
                    observation.bracket.native_issue_ns += 1;
                    observation.bracket.native_receive_ns += 1;
                    observation.expires_at_boottime_ns += 1;
                }
                report.validate().unwrap();
                body(report)
            }
            Command::ReleaseBrowserTiming { binding_id, player } => {
                let mut report = active.take().unwrap();
                assert_eq!(binding_id, &report.binding.binding_id);
                assert_eq!(player, &report.binding.player);
                report.current = false;
                body(report)
            }
            _ => panic!("failure must not create replacement timing"),
        });
        let mut timing = Timing::new(&server.client()).unwrap();
        let player = player();
        timing
            .open(player.clone(), Arc::new(AtomicBool::new(false)), media())
            .unwrap();
        ready(&timing);
        timing
            .handle("/player/timing-probe", &player, b"", 1)
            .unwrap();
        let challenge = reply(&timing, &player, 1);
        let result = if fault.starts_with("probe") {
            challenge
        } else {
            let challenge = challenge.unwrap();
            timing
                .handle(
                    "/player/timing-observe",
                    &player,
                    &serde_json::to_vec(
                        &json!({"sequence":"1","probe_id":challenge["probe_id"],"frame":frame()}),
                    )
                    .unwrap(),
                    2,
                )
                .unwrap();
            reply(&timing, &player, 2)
        };
        let message = result.unwrap_err().to_string();
        assert!(!message.contains("authentication"));
        assert!(!message.contains("private path"));
        {
            let records = state(&timing.shared);
            let value = diagnostic(&records.entries[0]);
            let operation = if fault.starts_with("probe") {
                "probe"
            } else {
                "observe"
            };
            assert_eq!(value["first_failure"]["operation"], operation, "{fault}");
            assert_eq!(
                value["first_failure"]["stage"],
                if fault == "remote error" {
                    "exchange"
                } else {
                    "reply"
                },
                "{fault}"
            );
            let diagnostic = value.to_string();
            assert!(!diagnostic.contains("authentication"));
            assert!(!diagnostic.contains("private path"));
        }
        assert!(timing
            .handle("/player/timing-probe", &player, b"", 3)
            .is_err());
        timing.shutdown().unwrap();
        {
            let records = state(&timing.shared);
            let record = &records.entries[0];
            assert!(record.release == ReleaseState::Acknowledged);
            assert!(!record.report.as_ref().unwrap().current);
            assert!(record.first_failure.is_some());
            assert!(record.probe.is_none());
        }
        let calls = server.finish();
        assert_eq!(calls.len(), if fault.starts_with("probe") { 3 } else { 4 });
    }
}

#[test]
fn paused_exact_player_status_is_historical_and_page_only_or_probe_access_is_refused() {
    let mut original = None;
    let mut server = Server::new(move |request| match &request.command {
        Command::OpenBrowserTiming { .. } => {
            let report = opened(request);
            original = Some(report.clone());
            body(report)
        }
        Command::ReleaseBrowserTiming { .. } => {
            let report = original.as_mut().unwrap();
            report.current = false;
            body(report.clone())
        }
        Command::BrowserTimingOpenStatus {
            original_request_id,
        } => {
            let report = original.as_ref().unwrap();
            assert_eq!(original_request_id, &report.binding.request_id);
            assert!(!report.current);
            body(report.clone())
        }
        _ => panic!("paused history may not rearm or issue probes"),
    });
    let binding = media();
    let mut control = PlayerControl::new(
        &server.client(),
        binding.project_id.clone(),
        binding.revision,
        None,
    )
    .unwrap();
    let page = control.page().unwrap();
    let page_scope = Scope {
        launch: page["launch_id"].as_str().unwrap().into(),
        page: page["page_id"].as_str().unwrap().into(),
        page_capability: page["page_capability"].as_str().unwrap().into(),
        ..Scope::default()
    };
    let ControlReply::Ready(start) = control.handle_with_media("/player/start", &page_scope,
        br#"{"source_id":"selection","media_selection_id":"selection","generation":1,"time_basis":"source_media_presentation"}"#, 1, Some(binding)).unwrap() else { panic!("player must start independently of timing Open"); };
    let scope = Scope {
        player: Some(start["player"]["player_id"].as_str().unwrap().into()),
        source: Some("selection".into()),
        generation: Some(1),
        player_capability: Some(start["player_capability"].as_str().unwrap().into()),
        ..page_scope.clone()
    };
    ready(&control.timing);
    control.stop();
    wait_until(|| state(&control.timing.shared).entries[0].release == ReleaseState::Acknowledged);
    assert!(!control.active());
    assert!(control
        .handle("/player/timing-status", &page_scope, b"", 2)
        .is_err());
    assert!(control
        .handle("/player/timing-probe", &scope, b"", 3)
        .is_err());
    assert!(matches!(
        control
            .handle("/player/timing-status", &scope, b"", 4)
            .unwrap(),
        ControlReply::Pending
    ));
    let mut reply = None;
    wait_until(|| {
        for (ticket, value) in control.poll() {
            assert_eq!(ticket, 4);
            reply = Some(value);
        }
        reply.is_some()
    });
    assert_eq!(reply.unwrap().unwrap()["current"], false);
    assert!(!control.active());
    control.shutdown().unwrap();
    let calls = server.finish();
    assert_eq!(calls.len(), 3);
    assert!(matches!(calls[2], Command::BrowserTimingOpenStatus { .. }));
}
