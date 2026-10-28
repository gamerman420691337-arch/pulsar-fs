//! Local feed admission and role/generation tests; no Engine or physical grants.
use super::*;
use std::sync::atomic::Ordering;
fn request() -> Request {
    let identity = HandyPlayerIdentity {
        launch_id: AttemptId::new("launch").unwrap(),
        page_id: AttemptId::new("page").unwrap(),
        player_id: AttemptId::new("player").unwrap(),
        source_id: AttemptId::new("selection").unwrap(),
        generation: 7,
        time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
    };
    let media = BrowserMediaBinding {
        preparation_request_id: RequestId::new("prepare").unwrap(),
        lease_id: ArtifactId::new("lease").unwrap(),
        engine_epoch: AttemptId::new("boot").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(1),
        lease_ms: 30_000,
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("source").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: RevisionId::new(1),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("source").unwrap(),
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        },
    };
    let timing = BrowserTimingBinding {
        request_id: RequestId::new("timing-open").unwrap(),
        binding_id: ArtifactId::new("timing").unwrap(),
        engine_epoch: media.engine_epoch.clone(),
        project_id: media.project_id.clone(),
        revision: media.revision,
        player: identity.clone(),
        browser_media: media.clone(),
        policy: BrowserTimingPolicy {
            probe_timeout_ms: 1000,
            observation_timeout_ms: 3000,
        },
    };
    Request {
        method: "POST".into(),
        path: "/start".into(),
        body: Vec::new(),
        scope: Scope::default(),
        received: Instant::now(),
        range: None,
        if_range: None,
        browser_media: Some(media),
        player_permit: Some(crate::browser_player_control::Permit::for_analysis_test(
            identity,
        )),
        browser_timing: Some(timing),
        browser_media_expires: None,
    }
}
fn selection() -> BrowserAnalysisRequest {
    BrowserAnalysisRequest {
        continuity: false,
        role: BrowserAnalysisRole::Lookahead,
        policy: BrowserAnalysisLookaheadPolicy {
            target_lead_us: 1_000_000,
            minimum_lead_us: 500_000,
            maximum_lead_us: 2_000_000,
        },
    }
}
fn frame(feed: &AnalysisFeed) -> Frame {
    Frame {
        epoch: feed.binding.capture_epoch.clone(),
        media_us: 1_100_000,
        analysis: Some(AnalysisFrameIdentity {
            feed_id: feed.binding.feed_id.clone(),
            role: BrowserAnalysisRole::Lookahead,
            generation: 7,
            visible_media_us: 100_000,
            visible_presented_frames: 2,
            visible_tick_us: 1000,
            analysis_tick_us: 1100,
            presented_frames: 3,
        }),
    }
}
#[test]
fn browser_analysis_feed_freezes_native_player_media_timing_and_producer_policy() {
    let request = request();
    let feed =
        AnalysisFeed::new(&request, selection(), AttemptId::new("capture").unwrap()).unwrap();
    let declared = feed.declaration();
    declared.validate().unwrap();
    assert_eq!(declared.feed_id, feed.binding.feed_id);
    assert_eq!(
        declared.player,
        request
            .player_permit
            .as_ref()
            .unwrap()
            .identity
            .clone()
            .unwrap()
    );
    assert_eq!(
        declared.timing_binding_id,
        request.browser_timing.as_ref().unwrap().binding_id
    );
    assert_eq!(declared.policy, selection().policy);
    assert_eq!(
        feed.validate_frame(&request, &frame(&feed)).unwrap(),
        1_000_000
    );
    let another =
        AnalysisFeed::new(&request, selection(), AttemptId::new("capture").unwrap()).unwrap();
    assert_ne!(another.binding.feed_id, feed.binding.feed_id);
    assert!(another.validate_frame(&request, &frame(&feed)).is_err());
}
#[test]
fn browser_analysis_feed_refuses_unbound_stale_native_scope_and_mismatched_epoch_or_role() {
    for case in 0..8 {
        let mut r = request();
        let feed = AnalysisFeed::new(&r, selection(), AttemptId::new("capture").unwrap()).unwrap();
        let mut f = frame(&feed);
        match case {
            0 => f.epoch = AttemptId::new("successor").unwrap(),
            1 => f.analysis.as_mut().unwrap().feed_id = ArtifactId::new("stale-feed").unwrap(),
            2 => f.analysis.as_mut().unwrap().generation += 1,
            3 => f.analysis = None,
            4 => r.browser_media.as_mut().unwrap().lease_id = ArtifactId::new("other").unwrap(),
            5 => r.browser_timing.as_mut().unwrap().binding_id = ArtifactId::new("other").unwrap(),
            6 => r.player_permit = None,
            _ => r
                .player_permit
                .as_ref()
                .unwrap()
                .cancelled
                .store(true, Ordering::Release),
        }
        assert!(feed.validate_frame(&r, &f).is_err(), "case {case}");
    }
    assert!(serde_json::from_str::<AnalysisFrameIdentity>(r#"{"feed_id":"feed","role":"visible","generation":7,"visible_media_us":0,"visible_presented_frames":1,"visible_tick_us":1,"analysis_tick_us":1,"presented_frames":1}"#).is_err());
    for case in 0..4 {
        let mut r = request();
        match case {
            0 => r.browser_media = None,
            1 => r.browser_timing = None,
            2 => r.player_permit = None,
            _ => {
                r.player_permit
                    .as_mut()
                    .unwrap()
                    .identity
                    .as_mut()
                    .unwrap()
                    .time_basis = LiveSourceTimeBasis::CameraCapturePresentation
            }
        }
        assert!(AnalysisFeed::new(&r, selection(), AttemptId::new("capture").unwrap()).is_err());
    }
}
#[test]
fn browser_analysis_feed_uses_observed_lead_and_advancing_callback_counters() {
    let r = request();
    let mut feed = AnalysisFeed::new(&r, selection(), AttemptId::new("capture").unwrap()).unwrap();
    let f = frame(&feed);
    feed.latest = f.analysis.clone();
    for case in 0..7 {
        let mut f = frame(&feed);
        let a = f.analysis.as_mut().unwrap();
        a.presented_frames += 1;
        match case {
            0 => a.presented_frames -= 1,
            1 => a.visible_presented_frames = 1,
            2 => a.visible_tick_us = 999,
            3 => a.analysis_tick_us = 999,
            4 => a.analysis_tick_us = 1_001_001,
            5 => f.media_us = 200_000,
            _ => f.media_us = 2_100_001,
        }
        assert!(feed.validate_frame(&r, &f).is_err(), "case {case}");
    }
    for lead in [500_000, 2_000_000] {
        let mut f = frame(&feed);
        f.media_us = f.analysis.as_ref().unwrap().visible_media_us + lead;
        f.analysis.as_mut().unwrap().presented_frames += 1;
        assert_eq!(feed.validate_frame(&r, &f).unwrap(), lead as u64);
    }
}

fn input_fixture() -> (Request, AnalysisFeed, LiveCaptureReport) {
    let request = request();
    let mut report: LiveCaptureReport = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    report.identity.epoch = AttemptId::new("capture").unwrap();
    let media = request.browser_media.as_ref().unwrap();
    report.identity.project = media.project_id.clone();
    report.identity.revision = media.revision;
    report.identity.source = media.selection.source_version.clone();
    report.engine_epoch = media.engine_epoch.clone();
    set_browser_capture_anchor(
        &mut report.options,
        0,
        LiveSourceTimeBasis::SourceMediaPresentation,
        Some(media),
    )
    .unwrap();
    report.clock = report.options.clock(report.identity.epoch.clone());
    report.inference.as_mut().unwrap().latest = None;
    let mut feed = AnalysisFeed::new(&request, selection(), report.identity.epoch.clone()).unwrap();
    report.options.browser_analysis = Some(feed.declaration());
    let frame = frame(&feed);
    let header = LiveFrameHeader {
        frame: FrameId::new(u64::MAX - 1),
        source_time: SourceTimestamp::new(frame.media_us, 1_000_000).unwrap(),
        capture_tick: LiveClockTick(0),
        sha256: "a".repeat(64),
    };
    report.last_frame = Some(header.clone());
    feed.last_upload = Some(AnalysisUploadReceipt {
        header,
        submitted: AnalysisSubmittedFrame {
            media_us: frame.media_us,
            observation: frame.analysis.unwrap(),
        },
    });
    feed.capture_deadline = Some(Instant::now() + Duration::from_secs(20));
    report.validate().unwrap();
    (request, feed, report)
}
fn finish_request(feed: &AnalysisFeed) -> AnalysisFinishRequest {
    AnalysisFinishRequest {
        feed_id: feed.binding.feed_id.clone(),
        generation: feed.binding.generation,
        capture_epoch: feed.binding.capture_epoch.clone(),
        final_observation: feed.last_upload.as_ref().map(|u| u.submitted.clone()),
    }
}
#[test]
fn analysis_input_close_matches_original_browser_upload_without_roundtripping_native_frame_ids() {
    let (r, feed, report) = input_fixture();
    let expected = finish_request(&feed);
    feed.validate_finish(&r, &expected).unwrap();
    let body = serde_json::to_string(&expected).unwrap();
    assert!(!body.contains(&(u64::MAX - 1).to_string()));
    assert!(!body.contains("final_frame"));
    assert_eq!(
        feed.last_upload.as_ref().unwrap().header,
        report.last_frame.unwrap()
    );
    for case in 0..7 {
        let mut changed = expected.clone();
        match case {
            0 => changed.feed_id = ArtifactId::new("other-feed").unwrap(),
            1 => changed.generation += 1,
            2 => changed.capture_epoch = AttemptId::new("replacement").unwrap(),
            3 => changed.final_observation = None,
            4 => changed.final_observation.as_mut().unwrap().media_us += 1,
            5 => {
                changed
                    .final_observation
                    .as_mut()
                    .unwrap()
                    .observation
                    .presented_frames += 1
            }
            _ => {
                changed
                    .final_observation
                    .as_mut()
                    .unwrap()
                    .observation
                    .analysis_tick_us += 1
            }
        }
        assert!(feed.validate_finish(&r, &changed).is_err(), "case {case}");
    }
    r.player_permit
        .as_ref()
        .unwrap()
        .cancelled
        .store(true, Ordering::Release);
    assert!(feed.validate_finish(&r, &expected).is_err());
}
#[test]
fn closed_input_watchdog_requires_original_live_receipts_and_never_renews_capture_deadline() {
    let (_, mut feed, mut report) = input_fixture();
    let now = Instant::now();
    let id = RequestId::new("original-close").unwrap();
    feed.capture_deadline = Some(now + Duration::from_secs(5));
    feed.close = Some(AnalysisCloseIntent {
        request_id: id.clone(),
        final_frame: report.last_frame.clone(),
        verified_at: Some(now),
    });
    report.input_close = Some(LiveCaptureInputCloseReport {
        original_request_id: id,
        final_frame: report.last_frame.clone(),
        phase: LiveCaptureInputPhase::Closing,
        disposition: None,
        worker_joined: false,
        delivery_settled: false,
    });
    report.validate().unwrap();
    assert!(feed.valid_closed_input(&report, now));
    assert!(!player_http::frame_timeout_required(
        true,
        !feed.valid_closed_input(&report, now),
        now - Duration::from_secs(4),
        now
    ));
    assert!(player_http::frame_timeout_required(
        true,
        true,
        now - Duration::from_secs(4),
        now
    ));
    // A read checked before a delayed reply cannot keep the exemption fresh.
    assert!(!feed.valid_closed_input(&report, now + Duration::from_secs(2)));
    feed.close.as_mut().unwrap().verified_at = Some(now + Duration::from_secs(4));
    assert!(feed.valid_closed_input(&report, now + Duration::from_secs(4)));
    feed.close.as_mut().unwrap().verified_at = Some(now + Duration::from_secs(5));
    assert!(!feed.valid_closed_input(&report, now + Duration::from_secs(5)));
    feed.close.as_mut().unwrap().verified_at = Some(now);
    let mut changed = report.clone();
    changed.input_close.as_mut().unwrap().original_request_id =
        RequestId::new("replacement-close").unwrap();
    assert!(!feed.valid_closed_input(&changed, now));
    changed = report.clone();
    changed
        .input_close
        .as_mut()
        .unwrap()
        .final_frame
        .as_mut()
        .unwrap()
        .sha256 = "b".repeat(64);
    assert!(!feed.valid_closed_input(&changed, now));
    changed = report.clone();
    changed.input_close.as_mut().unwrap().phase = LiveCaptureInputPhase::Aborted;
    assert!(!feed.valid_closed_input(&changed, now));
    report.active = false;
    assert!(!feed.valid_closed_input(&report, now));
}
#[cfg(unix)]
#[test]
fn duplicate_browser_finish_reuses_original_engine_request_and_status_never_closes_again() {
    use std::{
        os::unix::net::UnixListener,
        sync::{Arc, Mutex},
        thread,
    };
    let (mut request, feed, report) = input_fixture();
    request.body = serde_json::to_vec(&finish_request(&feed)).unwrap();
    request.path = "/analysis/finish".into();
    let directory = tempfile::tempdir().unwrap();
    let endpoint = directory.path().join("engine.sock");
    let listener = UnixListener::bind(&endpoint).unwrap();
    listener.set_nonblocking(true).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let mut response = report.clone();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut original = None;
        for expected_close in [true, true, false] {
            let (mut stream, _) = loop {
                assert!(Instant::now() < deadline, "expected finite-close request");
                match listener.accept() {
                    Ok(v) => break v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let wire: pulsar_protocol::Request = read_message(&mut stream).unwrap();
            match &wire.command {
                Command::CloseBrowserAnalysisInput { epoch, final_frame } => {
                    assert!(expected_close);
                    assert_eq!(epoch, &response.identity.epoch);
                    assert_eq!(final_frame, &response.last_frame);
                    if let Some(id) = &original {
                        assert_eq!(&wire.request_id, id);
                    } else {
                        original = Some(wire.request_id.clone());
                    }
                }
                Command::LiveCaptureStatus { epoch } => {
                    assert!(!expected_close);
                    assert_eq!(epoch, &response.identity.epoch);
                }
                other => panic!("unexpected operation {other:?}"),
            }
            observed.lock().unwrap().push(wire.request_id.clone());
            response.input_close = Some(LiveCaptureInputCloseReport {
                original_request_id: original.clone().unwrap(),
                final_frame: response.last_frame.clone(),
                phase: LiveCaptureInputPhase::Closing,
                disposition: None,
                worker_joined: false,
                delivery_settled: false,
            });
            response.validate().unwrap();
            write_message(
                &mut stream,
                &Response::success(
                    wire.request_id,
                    pulsar_protocol::ResponseBody::LiveCapture(Box::new(response.clone())),
                ),
            )
            .unwrap();
        }
    });
    let mut client = SessionClient::from_credentials(
        &endpoint,
        SessionId::new("synthetic-close-session").unwrap(),
        "synthetic-close-token".into(),
    )
    .unwrap();
    client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(2)));
    let mut preview = Preview {
        client,
        project: report.identity.project.clone(),
        revision: report.identity.revision,
        options: report.options.clone(),
        report: Some(report.clone()),
        analysis_feed: Some(feed),
        analysis_successor: None,
        analysis_commit: None,
        analysis_last_transfer: None,
        analysis_cancelled: false,
        bulk: None,
        sequence: 2,
        last_time: Some(1_100_000),
        blocked: false,
        playback_epoch: Some(report.identity.epoch),
        pending: None,
        eligible_after: Instant::now(),
        handoffs: 0,
        transition_drops: 0,
        recording_maximum: None,
        recordings: Vec::new(),
        retired_recordings: Vec::new(),
        candidate: None,
        export: None,
        gap: None,
    };
    let first = preview.finish_analysis(&request, false).unwrap();
    let repeated = preview.finish_analysis(&request, false).unwrap();
    let status = preview.finish_analysis(&request, true).unwrap();
    assert_eq!(first["input_close"], repeated["input_close"]);
    assert_eq!(first["input_close"], status["input_close"]);
    assert!(preview
        .analysis_feed
        .as_ref()
        .unwrap()
        .validate_frame(&request, &frame(preview.analysis_feed.as_ref().unwrap()))
        .is_err());
    let mut changed: AnalysisFinishRequest = serde_json::from_slice(&request.body).unwrap();
    changed.final_observation.as_mut().unwrap().media_us += 1;
    request.body = serde_json::to_vec(&changed).unwrap();
    assert!(preview.finish_analysis(&request, false).is_err());
    preview.report = None;
    preview.analysis_feed = None;
    server.join().unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0], calls[1]);
    assert_ne!(calls[1], calls[2]);
}

#[test]
fn finite_input_final_preview_requires_the_exact_sealed_accepted_frame_not_prior_latest() {
    let (_, feed, mut report) = input_fixture();
    let sample: LiveCaptureReport = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    let mut latest = sample.inference.unwrap().latest.unwrap();
    let header = &feed.last_upload.as_ref().unwrap().header;
    let LiveInferenceReply::Estimated {
        input, identity, ..
    } = &mut latest.result
    else {
        panic!("CPU fixture")
    };
    *identity = report.identity.clone();
    input.pair.source_epoch = report.identity.epoch.clone();
    input.pair.current_frame = header.frame;
    input.pair.current_time = header.source_time;
    input.current.sha256 = header.sha256.clone();
    latest.capture_tick = header.capture_tick;
    report.inference.as_mut().unwrap().latest = Some(latest);
    report.input_close = Some(LiveCaptureInputCloseReport {
        original_request_id: RequestId::new("original-close").unwrap(),
        final_frame: Some(header.clone()),
        phase: LiveCaptureInputPhase::Sealed,
        disposition: Some(LiveCaptureInputFinalDisposition::Accepted),
        worker_joined: true,
        delivery_settled: true,
    });
    assert!(feed.final_result_matches(&report));
    for case in 0..6 {
        let mut changed = report.clone();
        match case {
            0 => changed.input_close.as_mut().unwrap().phase = LiveCaptureInputPhase::Joining,
            1 => {
                changed.input_close.as_mut().unwrap().disposition =
                    Some(LiveCaptureInputFinalDisposition::Stale)
            }
            2 => {
                changed
                    .inference
                    .as_mut()
                    .unwrap()
                    .latest
                    .as_mut()
                    .unwrap()
                    .capture_tick = LiveClockTick(1)
            }
            _ => {
                let LiveInferenceReply::Estimated { input, .. } = &mut changed
                    .inference
                    .as_mut()
                    .unwrap()
                    .latest
                    .as_mut()
                    .unwrap()
                    .result
                else {
                    unreachable!()
                };
                match case {
                    3 => input.pair.current_frame = FrameId::new(1),
                    4 => input.pair.current_time = SourceTimestamp::new(1, 10).unwrap(),
                    _ => input.current.sha256 = "b".repeat(64),
                }
            }
        }
        assert!(!feed.final_result_matches(&changed), "case {case}");
    }
}
#[test]
fn empty_browser_input_closure_never_invents_a_final_frame() {
    let (request, mut feed, mut report) = input_fixture();
    let old = finish_request(&feed);
    feed.last_upload = None;
    report.last_frame = None;
    report.received_frames = 0;
    let finish = finish_request(&feed);
    assert!(finish.final_observation.is_none());
    feed.validate_finish(&request, &finish).unwrap();
    assert!(feed.validate_finish(&request, &old).is_err());
    assert!(!feed.final_result_matches(&report));
}

#[test]
fn analysis_continuity_is_explicit_and_cannot_be_enabled_by_unknown_or_invalid_fields() {
    let legacy = serde_json::json!({"role":"lookahead","policy":{"target_lead_us":1_000_000,"minimum_lead_us":500_000,"maximum_lead_us":2_000_000}});
    let old: BrowserAnalysisRequest = serde_json::from_value(legacy.clone()).unwrap();
    assert!(!old.continuity);
    let mut explicit = legacy.clone();
    explicit["continuity"] = serde_json::json!(true);
    let selected: BrowserAnalysisRequest = serde_json::from_value(explicit).unwrap();
    assert!(selected.continuity);
    let feed = AnalysisFeed::new(
        &request(),
        selected,
        AttemptId::new("explicit-continuity").unwrap(),
    )
    .unwrap();
    assert!(feed.continuity);
    assert!(feed.close.is_none());
    for value in [
        serde_json::json!(1),
        serde_json::json!("true"),
        serde_json::Value::Null,
    ] {
        let mut invalid = legacy.clone();
        invalid["continuity"] = value;
        assert!(serde_json::from_value::<BrowserAnalysisRequest>(invalid).is_err());
    }
    let mut unknown = legacy;
    unknown["continuous"] = serde_json::json!(true);
    assert!(serde_json::from_value::<BrowserAnalysisRequest>(unknown).is_err());
}

#[cfg(unix)]
mod successor_bulk_reservation {
    use super::*;
    use std::{
        os::unix::net::{UnixListener, UnixStream},
        sync::{atomic::AtomicBool, Arc, Mutex},
        thread,
    };

    #[derive(Clone, Copy)]
    enum ReservationReply {
        Reserved,
        Aborted,
        WrongBoundary,
    }
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Prepare(RequestId),
        Reserve(RequestId),
        Status(RequestId),
        Cancel(RequestId),
        Stop(AttemptId),
        Bulk(LiveIdentity),
    }
    type Calls = Arc<Mutex<Vec<Call>>>;

    struct Peer {
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }
    impl Peer {
        fn finish(mut self) -> thread::Result<()> {
            self.stop.store(true, Ordering::Release);
            self.worker.take().unwrap().join()
        }
    }
    impl Drop for Peer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn accept(listener: &UnixListener) -> Option<UnixStream> {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                Some(stream)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(error) => panic!("successor IPC accept: {error}"),
        }
    }

    // These framed peers model only the native wire contract. No media, worker,
    // clock qualification, or physical-device authority is created by the test.
    fn with_peer(
        mode: ReservationReply,
        check: impl FnOnce(&mut Preview, &Request, &Request, &Calls),
    ) {
        let (request, mut feed, prior) = input_fixture();
        feed.continuity = true;
        let mut child_request = super::request();
        let media = child_request.browser_media.as_mut().unwrap();
        media.preparation_request_id = RequestId::new("successor-media").unwrap();
        media.lease_id = ArtifactId::new("successor-lease").unwrap();
        let timing = child_request.browser_timing.as_mut().unwrap();
        timing.request_id = RequestId::new("successor-timing-open").unwrap();
        timing.binding_id = ArtifactId::new("successor-timing").unwrap();
        timing.browser_media = media.clone();
        child_request.path = "/analysis/prepare".into();

        let directory = tempfile::tempdir().unwrap();
        let endpoint = directory.path().join("engine.sock");
        let commands = UnixListener::bind(&endpoint).unwrap();
        let bulk = UnixListener::bind(directory.path().join("bulk.sock")).unwrap();
        commands.set_nonblocking(true).unwrap();
        bulk.set_nonblocking(true).unwrap();
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let server_prior = prior.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut prepared: Option<BrowserAnalysisSuccessorReport> = None;
            let mut reserved: Option<BrowserAnalysisTransferReport> = None;
            let mut seeded: Option<LiveCaptureReport> = None;
            // Retain the accepted channel until cleanup. A second handshake is
            // observable even when no G pixels have been sent on the first one.
            let mut channels = Vec::new();
            while !stopped.load(Ordering::Acquire) && Instant::now() < deadline {
                if let Some(mut stream) = accept(&commands) {
                    let wire: pulsar_protocol::Request = match read_message(&mut stream) {
                        Ok(wire) => wire,
                        // A locally refused request may close its connected socket
                        // before writing a frame. Keep the peer alive for Stop.
                        Err(pulsar_protocol::TransportError::Io(error))
                            if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                        {
                            continue
                        }
                        Err(error) => panic!("successor IPC frame: {error}"),
                    };
                    wire.validate().unwrap();
                    assert_eq!(wire.project.as_ref(), Some(&server_prior.identity.project));
                    assert_eq!(
                        wire.session.as_ref().unwrap().as_str(),
                        "successor-wire-session"
                    );
                    let body = match &wire.command {
                        Command::PrepareBrowserAnalysisSuccessor {
                            engine_epoch,
                            predecessor,
                            options,
                        } => {
                            assert_eq!(
                                wire.expected_revision,
                                Some(server_prior.identity.revision)
                            );
                            assert_eq!(engine_epoch, &server_prior.engine_epoch);
                            assert_eq!(predecessor, &server_prior.identity.epoch);
                            observed
                                .lock()
                                .unwrap()
                                .push(Call::Prepare(wire.request_id.clone()));
                            if let Some(original) = &prepared {
                                assert_eq!(wire.request_id, original.original_request_id);
                                assert_eq!(
                                    original.successor.options.browser_media.as_ref(),
                                    Some(&options.browser_media)
                                );
                                assert_eq!(
                                    original.successor.options.browser_analysis.as_ref(),
                                    Some(&options.browser_analysis)
                                );
                            } else {
                                let mut child = server_prior.clone();
                                child.identity.epoch = AttemptId::new("successor-capture").unwrap();
                                child.options.browser_media = Some(options.browser_media.clone());
                                child.options.browser_analysis =
                                    Some(options.browser_analysis.clone());
                                child.options.lease_ms = options.lease_ms;
                                child.clock = child.options.clock(child.identity.epoch.clone());
                                child.received_frames = 0;
                                child.last_frame = None;
                                child.queue = None;
                                child.server_elapsed = Some(LiveClockTick(1_000_000));
                                let inference = child.inference.as_mut().unwrap();
                                inference.attempt_id =
                                    AttemptId::new("successor-inference").unwrap();
                                inference.state = LiveCaptureInferenceState::Running;
                                inference.completed = 0;
                                inference.accepted = 0;
                                inference.stale = 0;
                                inference.latest = None;
                                prepared = Some(BrowserAnalysisSuccessorReport {
                                    original_request_id: wire.request_id.clone(),
                                    predecessor: server_prior.identity.clone(),
                                    successor: child,
                                    phase: BrowserAnalysisSuccessorPhase::Prepared,
                                    transfer_request_id: None,
                                });
                            }
                            let report = prepared.as_ref().unwrap();
                            report.validate().unwrap();
                            pulsar_protocol::ResponseBody::BrowserAnalysisSuccessor(Box::new(
                                report.clone(),
                            ))
                        }
                        Command::TransferBrowserAnalysisInput {
                            predecessor,
                            successor,
                            boundary,
                        } => {
                            let preparation =
                                prepared.as_ref().expect("Prepare before reservation");
                            assert_eq!(
                                wire.expected_revision,
                                Some(server_prior.identity.revision)
                            );
                            assert_eq!(predecessor, &server_prior.identity.epoch);
                            assert_eq!(successor, &preparation.successor.identity.epoch);
                            assert_eq!(Some(boundary), server_prior.last_frame.as_ref());
                            assert_ne!(wire.request_id, preparation.original_request_id);
                            observed
                                .lock()
                                .unwrap()
                                .push(Call::Reserve(wire.request_id.clone()));
                            assert!(reserved.is_none(), "only the original reservation is sent");
                            let mut report = BrowserAnalysisTransferReport {
                                original_request_id: wire.request_id.clone(),
                                preparation_request_id: preparation.original_request_id.clone(),
                                predecessor: server_prior.identity.clone(),
                                successor: preparation.successor.identity.clone(),
                                boundary: boundary.clone(),
                                phase: BrowserAnalysisTransferPhase::BoundaryReserved,
                                predecessor_worker_joined: false,
                                predecessor_delivery_settled: false,
                                successor_pair_accepted: false,
                                error: None,
                            };
                            match mode {
                                ReservationReply::Reserved => {
                                    let mut child = preparation.successor.clone();
                                    child.received_frames = 1;
                                    child.last_frame = Some(LiveFrameHeader {
                                        frame: FrameId::new(0),
                                        ..boundary.clone()
                                    });
                                    child.validate().unwrap();
                                    seeded = Some(child);
                                }
                                ReservationReply::Aborted => {
                                    report.phase = BrowserAnalysisTransferPhase::Aborted;
                                    report.error = Some(ProtocolError::new(
                                        ErrorCode::Unavailable,
                                        "original reservation refused",
                                    ));
                                }
                                ReservationReply::WrongBoundary => {
                                    report.boundary.sha256 = "b".repeat(64)
                                }
                            }
                            report.validate().unwrap();
                            reserved = Some(report.clone());
                            pulsar_protocol::ResponseBody::BrowserAnalysisTransfer(Box::new(report))
                        }
                        Command::BrowserAnalysisTransferStatus {
                            transfer_request_id,
                        } => {
                            let original = reserved.as_ref().expect("inspect original reservation");
                            assert_eq!(wire.expected_revision, None);
                            assert_eq!(transfer_request_id, &original.original_request_id);
                            assert_ne!(wire.request_id, original.original_request_id);
                            observed
                                .lock()
                                .unwrap()
                                .push(Call::Status(wire.request_id.clone()));
                            pulsar_protocol::ResponseBody::BrowserAnalysisTransfer(Box::new(
                                original.clone(),
                            ))
                        }
                        Command::CancelBrowserAnalysisTransfer {
                            transfer_request_id,
                        } => {
                            let original = reserved.as_ref().expect("cancel original reservation");
                            assert_eq!(wire.expected_revision, None);
                            assert_eq!(transfer_request_id, &original.original_request_id);
                            assert_ne!(wire.request_id, original.original_request_id);
                            observed
                                .lock()
                                .unwrap()
                                .push(Call::Cancel(wire.request_id.clone()));
                            let mut report = original.clone();
                            report.phase = BrowserAnalysisTransferPhase::Aborted;
                            report.error = Some(ProtocolError::new(
                                ErrorCode::Unavailable,
                                "explicit cancellation",
                            ));
                            report.validate().unwrap();
                            pulsar_protocol::ResponseBody::BrowserAnalysisTransfer(Box::new(report))
                        }
                        Command::StopLiveCapture { epoch } => {
                            assert_eq!(epoch, &server_prior.identity.epoch);
                            observed.lock().unwrap().push(Call::Stop(epoch.clone()));
                            let mut report = server_prior.clone();
                            report.active = false;
                            report.retirement = Some(LiveCaptureRetirement::Stopped);
                            report.admitted_pixel_bytes = 0;
                            report.inference.as_mut().unwrap().state =
                                LiveCaptureInferenceState::Retired;
                            report.validate().unwrap();
                            pulsar_protocol::ResponseBody::LiveCapture(Box::new(report))
                        }
                        other => panic!("unexpected successor operation: {other:?}"),
                    };
                    write_message(&mut stream, &Response::success(wire.request_id, body)).unwrap();
                }
                if let Some(mut stream) = accept(&bulk) {
                    let handshake: LiveBulkHandshake = read_bulk_header(&mut stream).unwrap();
                    handshake.validate().unwrap();
                    let preparation = prepared.as_ref().expect("Prepare before handshake");
                    assert_eq!(handshake.channel, LiveBulkChannel::LiveCapture);
                    assert_eq!(handshake.session.as_str(), "successor-wire-session");
                    assert_eq!(handshake.auth_token, "synthetic-successor-wire-token");
                    assert_eq!(handshake.engine_epoch, preparation.successor.engine_epoch);
                    assert_eq!(handshake.identity, preparation.successor.identity);
                    observed
                        .lock()
                        .unwrap()
                        .push(Call::Bulk(handshake.identity));
                    let reply = seeded.as_ref().map_or_else(
                        || {
                            LiveBulkReply::Error(ProtocolError::new(
                                ErrorCode::Unavailable,
                                "source transfer ingress is not open",
                            ))
                        },
                        |report| LiveBulkReply::Ready(report.clone()),
                    );
                    write_bulk_header(&mut stream, &reply).unwrap();
                    channels.push(stream);
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let peer = Peer {
            stop,
            worker: Some(server),
        };
        let mut client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("successor-wire-session").unwrap(),
            "synthetic-successor-wire-token".into(),
        )
        .unwrap();
        client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(2)));
        let mut preview = Preview {
            client,
            project: prior.identity.project.clone(),
            revision: prior.identity.revision,
            options: prior.options.clone(),
            report: Some(prior.clone()),
            analysis_feed: Some(feed),
            analysis_successor: None,
            analysis_commit: None,
            analysis_last_transfer: None,
            analysis_cancelled: false,
            bulk: None,
            sequence: 2,
            last_time: Some(1_100_000),
            blocked: false,
            playback_epoch: Some(prior.identity.epoch),
            pending: None,
            eligible_after: Instant::now(),
            handoffs: 0,
            transition_drops: 0,
            recording_maximum: None,
            recordings: Vec::new(),
            retired_recordings: Vec::new(),
            candidate: None,
            export: None,
            gap: None,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check(&mut preview, &request, &child_request, &calls);
        }));
        // There is no native capture to Stop in this authored IPC fixture.
        // Always drop the client channel and join the bounded peer, even on red.
        preview.analysis_successor = None;
        preview.analysis_feed = None;
        preview.report = None;
        drop(preview);
        let joined = peer.finish();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
        joined.unwrap();
    }

    #[test]
    fn browser_stop_cancels_reserved_transfer_without_revision_and_with_fresh_request() {
        with_peer(ReservationReply::Reserved, |preview, old, child, calls| {
            preview.prepare_analysis_successor(child).unwrap();
            preview.transfer_analysis(old, false).unwrap();
            let transfer = preview
                .analysis_successor
                .as_ref()
                .unwrap()
                .transfer_id
                .clone()
                .unwrap();
            preview.stop().unwrap();
            assert!(preview.analysis_successor.is_none());
            assert!(preview.report.is_none());
            let observed = calls.lock().unwrap();
            let cancelled: Vec<_> = observed
                .iter()
                .filter_map(|call| match call {
                    Call::Cancel(id) => Some(id),
                    _ => None,
                })
                .collect();
            assert_eq!(cancelled.len(), 1);
            assert_ne!(cancelled[0], &transfer);
            assert_eq!(
                observed
                    .iter()
                    .filter(|call| matches!(call, Call::Stop(_)))
                    .count(),
                1
            );
            drop(observed);
            preview.stop().unwrap();
            assert_eq!(
                calls.lock().unwrap().len(),
                5,
                "confirmed Stop is inert on repetition"
            );
        });
    }

    #[test]
    fn browser_successor_bulk_opens_only_after_original_reservation_and_is_reused_before_g() {
        with_peer(ReservationReply::Reserved, |preview, old, child, calls| {
            preview.prepare_analysis_successor(child).unwrap();
            let prepared_id = preview
                .analysis_successor
                .as_ref()
                .unwrap()
                .request_id
                .clone();
            preview.prepare_analysis_successor(child).unwrap();
            assert!(preview.analysis_successor.as_ref().unwrap().bulk.is_none());
            assert_eq!(
                *calls.lock().unwrap(),
                vec![
                    Call::Prepare(prepared_id.clone()),
                    Call::Prepare(prepared_id)
                ]
            );

            preview.transfer_analysis(old, false).unwrap();
            let next = preview.analysis_successor.as_ref().unwrap();
            let original = next.transfer_id.clone().unwrap();
            let boundary = next.boundary.clone().unwrap();
            let child_identity = next.report.as_ref().unwrap().successor.identity.clone();
            assert!(next.bulk.is_some());
            assert!(!next.pair_sent);
            assert_eq!(
                Some(&boundary),
                preview.report.as_ref().unwrap().last_frame.as_ref()
            );
            for _ in 0..2 {
                preview.transfer_analysis(old, true).unwrap();
                let next = preview.analysis_successor.as_ref().unwrap();
                assert_eq!(next.transfer_id.as_ref(), Some(&original));
                assert_eq!(next.boundary.as_ref(), Some(&boundary));
                assert!(next.bulk.is_some());
                assert!(!next.pair_sent);
            }
            let observed = calls.lock().unwrap();
            assert_eq!(observed.len(), 6);
            assert_eq!(observed[2], Call::Reserve(original));
            assert_eq!(observed[3], Call::Bulk(child_identity));
            assert!(matches!(observed[4], Call::Status(_)));
            assert!(matches!(observed[5], Call::Status(_)));
            assert_ne!(observed[4], observed[5]);
        });
    }

    #[test]
    fn browser_successor_bulk_never_opens_for_aborted_or_mismatched_reservation() {
        for (mode, expected) in [
            (
                ReservationReply::Aborted,
                "original analysis transfer aborted",
            ),
            (
                ReservationReply::WrongBoundary,
                "analysis transfer changed original subject",
            ),
        ] {
            with_peer(mode, |preview, old, child, calls| {
                preview.prepare_analysis_successor(child).unwrap();
                let error = preview.transfer_analysis(old, false).unwrap_err();
                assert!(error.to_string().contains(expected), "{error:#}");
                let next = preview.analysis_successor.as_ref().unwrap();
                assert!(next.bulk.is_none());
                assert!(next.transfer.is_none());
                assert!(!next.pair_sent);
                assert_eq!(
                    next.boundary.as_ref(),
                    preview.report.as_ref().unwrap().last_frame.as_ref()
                );
                let observed = calls.lock().unwrap();
                assert_eq!(observed.len(), 2);
                assert_eq!(observed[0], Call::Prepare(next.request_id.clone()));
                assert_eq!(
                    observed[1],
                    Call::Reserve(next.transfer_id.clone().unwrap())
                );
            });
        }
    }
}
