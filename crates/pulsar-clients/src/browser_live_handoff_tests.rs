//! Deterministic IPC interleavings; synthetic reports do not attest media pixels.
use super::*;
use std::{
    os::unix::net::UnixListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

struct StatusServer {
    directory: tempfile::TempDir,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<Vec<AttemptId>>>,
}
impl StatusServer {
    fn new(replies: Vec<(AttemptId, LiveCaptureReport)>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut calls = Vec::new();
            while !stopped.load(Ordering::Acquire) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("synthetic accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let request: pulsar_protocol::Request = read_message(&mut stream).unwrap();
                let Command::LiveCaptureStatus { epoch } = request.command else {
                    panic!("handoff reconciliation must only inspect existing captures")
                };
                let (expected, report) = &replies[calls.len()];
                assert_eq!(&epoch, expected);
                calls.push(epoch);
                write_message(
                    &mut stream,
                    &Response::success(
                        request.request_id,
                        pulsar_protocol::ResponseBody::LiveCapture(Box::new(report.clone())),
                    ),
                )
                .unwrap();
            }
            calls
        });
        Self {
            directory,
            stop,
            worker: Some(worker),
        }
    }
    fn client(&self) -> SessionClient {
        let mut client = SessionClient::from_credentials(
            &self.directory.path().join("engine.sock"),
            SessionId::new("synthetic-handoff-session").unwrap(),
            "synthetic-handoff-authentication".into(),
        )
        .unwrap();
        client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(2)));
        client
    }
    fn finish(&mut self) -> Vec<AttemptId> {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for StatusServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn reports() -> (LiveCaptureReport, LiveCaptureReport, LiveCaptureReport) {
    let mut prior: LiveCaptureReport = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    let binding = BrowserMediaBinding {
        preparation_request_id: RequestId::new("synthetic-prepare").unwrap(),
        lease_id: ArtifactId::new("synthetic-media-lease").unwrap(),
        engine_epoch: prior.engine_epoch.clone(),
        project_id: prior.identity.project.clone(),
        revision: prior.identity.revision,
        selection: BrowserMediaSelection {
            source_version: prior.identity.source.clone(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("synthetic-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("synthetic-placement").unwrap(),
                created_revision: prior.identity.revision,
                spec: SourcePlacementSpec {
                    source_version: prior.identity.source.clone(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(30, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::from_nanos(750_000_000),
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
    };
    set_browser_capture_anchor(
        &mut prior.options,
        100_000,
        LiveSourceTimeBasis::SourceMediaPresentation,
        Some(&binding),
    )
    .unwrap();
    prior.clock = prior.options.clock(prior.identity.epoch.clone());
    prior.server_elapsed = Some(LiveClockTick(15_000_000_000));
    prior.inference.as_mut().unwrap().latest = None;
    prior.validate().unwrap();
    let mut next = prior.clone();
    next.identity.epoch = AttemptId::new("synthetic-successor").unwrap();
    next.options = prior
        .continuation_options(SourceTimestamp::new(1, 5).unwrap(), 30_000)
        .unwrap();
    next.options.continuation_mode = LiveContinuationMode::WhenReady;
    next.clock = next.options.clock(next.identity.epoch.clone());
    next.continuation = Some(LiveCaptureContinuation {
        predecessor: prior.identity.clone(),
        predecessor_clock: prior.clock.clone(),
        predecessor_elapsed: prior.server_elapsed.unwrap(),
        predecessor_last_frame: prior.last_frame.clone(),
    });
    next.received_frames = 0;
    next.last_frame = None;
    next.server_elapsed = Some(LiveClockTick(1_000_000));
    let inference = next.inference.as_mut().unwrap();
    inference.attempt_id = AttemptId::new("synthetic-successor-inference").unwrap();
    inference.state = LiveCaptureInferenceState::Starting;
    inference.completed = 0;
    inference.accepted = 0;
    inference.stale = 0;
    next.validate().unwrap();
    let mut retired = prior.clone();
    retired.active = false;
    retired.retirement = Some(LiveCaptureRetirement::Superseded);
    retired.admitted_pixel_bytes = 0;
    retired.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
    retired.validate().unwrap();
    (prior, next, retired)
}

fn preview(client: SessionClient, prior: LiveCaptureReport, next: LiveCaptureReport) -> Preview {
    Preview {
        client,
        project: prior.identity.project.clone(),
        revision: prior.identity.revision,
        options: prior.options.clone(),
        playback_epoch: Some(prior.identity.epoch.clone()),
        report: Some(prior),
        analysis_feed: None,
        analysis_successor: None,
        analysis_commit: None,
        analysis_last_transfer: None,
        analysis_cancelled: false,
        bulk: None,
        sequence: 2,
        last_time: Some(100_000),
        blocked: false,
        pending: Some(Pending {
            report: next,
            bulk: None,
            sequence: 1,
            eligible_after: Instant::now(),
        }),
        eligible_after: Instant::now(),
        handoffs: 0,
        transition_drops: 0,
        recording_maximum: None,
        recordings: Vec::new(),
        retired_recordings: Vec::new(),
        candidate: None,
        export: None,
        gap: None,
    }
}

fn request_status(preview: &mut Preview) -> Result<serde_json::Value> {
    preview.request(&Request {
        method: "POST".into(),
        path: "/status".into(),
        body: Vec::new(),
        scope: Scope::default(),
        received: Instant::now(),
        range: None,
        if_range: None,
        browser_media: None,
        player_permit: None,
        browser_timing: None,
        browser_media_expires: None,
    })
}

#[test]
fn request_tail_reconciles_readiness_between_successor_and_predecessor_status() {
    let (prior, next, retired) = reports();
    let playback = prior.identity.epoch.clone();
    let successor = next.identity.epoch.clone();
    let binding = prior.options.browser_media.clone();
    let mut running = next.clone();
    running.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Running;
    let mut server = StatusServer::new(vec![
        (successor.clone(), next.clone()),
        (playback.clone(), retired.clone()),
        (successor.clone(), running),
        (playback.clone(), retired),
    ]);
    let mut preview = preview(server.client(), prior, next);
    let result = request_status(&mut preview);
    // The test owns synthetic reports, not native captures; do not send Drop cleanup RPCs.
    preview.report = None;
    preview.pending = None;
    let calls = server.finish();
    let view = result.unwrap();
    assert_eq!(
        view["active"], true,
        "a confirmed successor must replace the superseded tail report"
    );
    assert_eq!(view["identity"]["epoch"], successor.as_str());
    assert_eq!(view["browser"]["playback_epoch"], playback.as_str());
    assert_eq!(view["browser"]["handoffs"], 1);
    assert_eq!(
        view["options"]["browser_media"],
        serde_json::to_value(binding).unwrap()
    );
    assert_eq!(
        calls,
        vec![successor.clone(), playback.clone(), successor, playback]
    );
}

#[test]
fn tail_handoff_refuses_unproven_or_changed_successor_without_replacement_work() {
    for fault in [
        "still starting",
        "retired successor",
        "foreign successor",
        "changed source binding",
        "changed engine epoch",
        "foreign predecessor linkage",
        "foreign predecessor status",
        "unconfirmed predecessor retirement",
    ] {
        let (prior, next, retired) = reports();
        let playback = prior.identity.epoch.clone();
        let successor = next.identity.epoch.clone();
        let mut observed = next.clone();
        observed.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Running;
        let mut confirmation = retired.clone();
        match fault {
            "still starting" => {
                observed.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Starting;
            }
            "retired successor" => {
                observed.active = false;
                observed.retirement = Some(LiveCaptureRetirement::Stopped);
                observed.admitted_pixel_bytes = 0;
                observed.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
            }
            "foreign successor" => {
                observed.identity.epoch = AttemptId::new("foreign-successor").unwrap();
                observed.clock.epoch = observed.identity.epoch.clone();
            }
            "changed source binding" => {
                observed
                    .options
                    .browser_media
                    .as_mut()
                    .unwrap()
                    .preparation_request_id = RequestId::new("foreign-preparation").unwrap();
            }
            "changed engine epoch" => {
                observed.engine_epoch = AttemptId::new("foreign-engine").unwrap();
                observed
                    .options
                    .browser_media
                    .as_mut()
                    .unwrap()
                    .engine_epoch = observed.engine_epoch.clone();
            }
            "foreign predecessor linkage" => {
                let link = observed.continuation.as_mut().unwrap();
                link.predecessor.epoch = AttemptId::new("foreign-predecessor").unwrap();
                link.predecessor_clock.epoch = link.predecessor.epoch.clone();
            }
            "foreign predecessor status" => {
                confirmation.identity.epoch = AttemptId::new("foreign-predecessor").unwrap();
                confirmation.clock.epoch = confirmation.identity.epoch.clone();
            }
            "unconfirmed predecessor retirement" => {
                confirmation.retirement = Some(LiveCaptureRetirement::Stopped);
            }
            _ => unreachable!(),
        }
        observed.validate().unwrap();
        confirmation.validate().unwrap();
        let mut replies = vec![
            (successor.clone(), next.clone()),
            (playback.clone(), retired),
            (successor.clone(), observed),
        ];
        if matches!(
            fault,
            "foreign predecessor status" | "unconfirmed predecessor retirement"
        ) {
            replies.push((playback.clone(), confirmation));
        }
        let expected_calls = replies.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let mut server = StatusServer::new(replies);
        let mut preview = preview(server.client(), prior, next);
        let result = request_status(&mut preview);
        let retained_prior = preview.report.take().unwrap().identity.epoch;
        let retained_pending = preview.pending.take().unwrap().report.identity.epoch;
        let calls = server.finish();
        assert!(result.is_err(), "{fault}: must not claim a current handoff");
        assert_eq!(preview.handoffs, 0, "{fault}");
        assert_eq!(
            retained_prior, playback,
            "{fault}: preserve original scope for cleanup"
        );
        assert_eq!(
            retained_pending, successor,
            "{fault}: preserve prepared scope for cleanup"
        );
        assert_eq!(calls, expected_calls, "{fault}: bounded inspection only");
    }
}

#[test]
fn tail_keeps_current_predecessor_and_reports_other_retirement_without_rearming() {
    for retirement in [None, Some(LiveCaptureRetirement::Expired)] {
        let (prior, next, mut retired) = reports();
        let playback = prior.identity.epoch.clone();
        let successor = next.identity.epoch.clone();
        let expected_active = retirement.is_none();
        let tail = if expected_active {
            prior.clone()
        } else {
            retired.retirement = retirement.clone();
            retired
        };
        let mut server = StatusServer::new(vec![
            (successor.clone(), next.clone()),
            (playback.clone(), tail),
        ]);
        let mut preview = preview(server.client(), prior, next);
        let result = request_status(&mut preview);
        preview.report = None;
        preview.pending = None;
        let calls = server.finish();
        let view = result.unwrap();
        assert_eq!(view["active"], expected_active);
        assert_eq!(view["identity"]["epoch"], playback.as_str());
        assert_eq!(
            view["retirement"],
            serde_json::to_value(retirement).unwrap()
        );
        assert_eq!(view["browser"]["handoffs"], 0);
        assert_eq!(calls, vec![successor, playback]);
    }
}

#[test]
fn analysis_feed_accessor_preserves_current_capture_and_refuses_silent_successor_epoch() {
    let (mut prior, next, _) = reports();
    let player = HandyPlayerIdentity {
        launch_id: AttemptId::new("launch").unwrap(),
        page_id: AttemptId::new("page").unwrap(),
        player_id: AttemptId::new("player").unwrap(),
        source_id: AttemptId::new("selection").unwrap(),
        generation: 1,
        time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
    };
    let media = prior.options.browser_media.clone().unwrap();
    let permit = crate::browser_player_control::Permit::for_analysis_test(player.clone());
    let timing = BrowserTimingBinding {
        request_id: RequestId::new("open-timing").unwrap(),
        binding_id: ArtifactId::new("timing").unwrap(),
        engine_epoch: prior.engine_epoch.clone(),
        project_id: prior.identity.project.clone(),
        revision: prior.identity.revision,
        player,
        browser_media: media.clone(),
        policy: BrowserTimingPolicy {
            probe_timeout_ms: 1000,
            observation_timeout_ms: 3000,
        },
    };
    let request = Request {
        method: "POST".into(),
        path: "/start".into(),
        body: Vec::new(),
        scope: Scope::default(),
        received: Instant::now(),
        range: None,
        if_range: None,
        browser_media: Some(media.clone()),
        player_permit: Some(permit.clone()),
        browser_timing: Some(timing.clone()),
        browser_media_expires: None,
    };
    let selection = BrowserAnalysisRequest {
        continuity: false,
        role: BrowserAnalysisRole::Lookahead,
        policy: BrowserAnalysisLookaheadPolicy {
            target_lead_us: 1_000_000,
            minimum_lead_us: 500_000,
            maximum_lead_us: 2_000_000,
        },
    };
    let feed = AnalysisFeed::new(&request, selection, prior.identity.epoch.clone()).unwrap();
    prior.options.browser_analysis = Some(feed.declaration());
    let original = prior.identity.clone();
    let mut server = StatusServer::new(Vec::new());
    let mut preview = preview(server.client(), prior, next.clone());
    preview.pending = None;
    preview.analysis_feed = Some(feed);
    let snapshot = preview.active_analysis_feed().unwrap().unwrap();
    assert_eq!(snapshot.capture, original);
    assert!(snapshot.latest_observation.is_none());
    assert!(snapshot.measured_lead_us.is_none());
    assert_eq!(snapshot.browser_media, media);
    assert_eq!(snapshot.browser_timing, timing);
    assert_eq!(
        snapshot.engine_epoch,
        preview.report.as_ref().unwrap().engine_epoch
    );
    preview.report.as_mut().unwrap().identity.epoch = next.identity.epoch;
    assert!(preview.active_analysis_feed().is_err());
    preview.report.as_mut().unwrap().identity = original;
    permit.cancelled.store(true, Ordering::Release);
    assert!(preview.active_analysis_feed().unwrap().is_none());
    preview.report = None;
    preview.analysis_feed = None;
    assert!(server.finish().is_empty());
}
