//! Authored local IPC only. Synthetic decisions are never imported or accepted.
use super::*;
use std::{
    io,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
};

fn artifact(c: char) -> ArtifactIdentity {
    let hash = c.to_string().repeat(64);
    ArtifactIdentity {
        artifact_id: ArtifactId::new(hash.clone()).unwrap(),
        sha256: hash,
        byte_len: 1,
    }
}
fn fixture() -> (
    Config,
    BrowserTimingBinding,
    HandyBrowserStartDecision,
    HandyControllerLease,
) {
    let controller: HandyControllerReceipt = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-controller.json"
    ))
    .unwrap();
    let lease = controller.lease;
    let source = SourceVersionId::new("authored-source").unwrap();
    let media = BrowserMediaBinding {
        preparation_request_id: RequestId::new("media-prepare").unwrap(),
        lease_id: ArtifactId::new("media-lease").unwrap(),
        engine_epoch: AttemptId::new(lease.engine_epoch.as_str()).unwrap(),
        project_id: lease.project_id.clone(),
        revision: lease.revision,
        selection: BrowserMediaSelection {
            source_version: source.clone(),
            source: artifact('a'),
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("placement").unwrap(),
                created_revision: lease.revision,
                spec: SourcePlacementSpec {
                    source_version: source,
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
        lease_ms: 60000,
    };
    let binding = BrowserTimingBinding {
        request_id: RequestId::new("timing-open").unwrap(),
        binding_id: ArtifactId::new("timing-binding").unwrap(),
        engine_epoch: media.engine_epoch.clone(),
        project_id: media.project_id.clone(),
        revision: media.revision,
        player: HandyPlayerIdentity {
            launch_id: AttemptId::new("launch").unwrap(),
            page_id: AttemptId::new("page").unwrap(),
            player_id: AttemptId::new("player").unwrap(),
            source_id: AttemptId::new("source-selection").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        },
        browser_media: media.clone(),
        policy: BrowserTimingPolicy {
            probe_timeout_ms: 1000,
            observation_timeout_ms: 3000,
        },
    };
    let subject = HandyBrowserStartSubject {
        schema_version: 1,
        origin: HandyEvidenceOrigin::Synthetic,
        context: HandyQualificationContext {
            schema_version: 1,
            control_image: artifact('b'),
            platform: "linux/authored-client-fixture".into(),
            transport: ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap(),
        },
        device_qualification: HandyQualificationBinding {
            dossier_id: ArtifactId::new("d".repeat(64)).unwrap(),
            revision: 2,
        },
        browser_environment: artifact('c'),
        project_id: media.project_id.clone(),
        revision: media.revision,
        selection: media.selection.clone(),
        clock: media.clock,
        master_sha256: "e".repeat(64),
        script_actor_session: SessionId::new("script-owner").unwrap(),
        script_request_id: RequestId::new("script-original").unwrap(),
        script: artifact('f'),
        timing: HandyBrowserStartTiming {
            clock: BrowserClockAssumptions {
                timestamp_error_ns: 1000,
                relative_rate_error_ppm: 100,
                maximum_age_ns: 1_000_000_000,
                maximum_interval_ns: 10_000_000,
            },
            reference: HandyBrowserStartReference::ExpectedDisplay,
            display_early_ns: 1000,
            display_late_ns: 1000,
            source_correspondence_error_ns: 1000,
            discrete_media_phase_error_ns: 1000,
            media_rate_error_ppm: 100,
            physical_execution_error_ns: 1000,
            maximum_start_age_ns: 500_000_000,
            maximum_alignment_error_ns: 100_000_000,
        },
    };
    let observation = BrowserTimingObservation {
        observation_id: ArtifactId::new("observed-frame").unwrap(),
        binding_id: binding.binding_id.clone(),
        sequence: 1,
        frame: BrowserTimingFrame {
            sample: HandyPlayerSample {
                browser_tick_us: 1_000_000,
                media_time_us: 1_000_000,
                presented_frames: 1,
                playing: true,
                seeking: false,
                ended: false,
                visible: true,
                playback_rate_milli: 1000,
            },
            callback_tick_us: 1_000_000,
            presentation_time_us: Some(1_000_000),
            expected_display_time_us: Some(1_000_000),
        },
        bracket: BrowserClockBracket {
            native_issue_ns: 1_000_000_000,
            native_receive_ns: 1_000_001_000,
            browser_sample_us: 1_000_000,
        },
        project_phase: ExactSeconds::new(1, 1).unwrap(),
        expires_at_boottime_ns: 4_000_000_000,
    };
    let config = Config {
        acquisition_request_id: lease.acquisition_request_id.clone(),
        controller_id: lease.controller_id.clone(),
        expected_generation: lease.generation,
        clock_request_id: RequestId::new("combined-clock").unwrap(),
        qualification: HandyBrowserStartQualificationBinding {
            dossier_id: ArtifactId::new("9".repeat(64)).unwrap(),
            revision: 2,
        },
        renewal_interval_ms: 50,
        device_refresh_interval_ms: 100,
    };
    let reference = 1_001_000_000;
    let alignment = subject
        .timing
        .phase_request(&observation, reference, 1000, 1000, 1_100_000_000)
        .unwrap()
        .calculate()
        .unwrap();
    let decision = HandyBrowserStartDecision {
        qualification: config.qualification.clone(),
        subject,
        timing_binding: binding.clone(),
        observation,
        reference_boottime_ns: reference,
        alignment,
    };
    decision.validate().unwrap();
    config.validate().unwrap();
    // Validate the complete cross-domain fixture before any mock IPC worker starts.
    history(
        RequestId::new("fixture-play").unwrap(),
        &config,
        &decision,
        &lease,
    );
    (config, binding, decision, lease)
}
fn history(
    request: RequestId,
    config: &Config,
    decision: &HandyBrowserStartDecision,
    lease: &HandyControllerLease,
) -> HandyPlaybackStatus {
    let report = HandyPlaybackStatus {
        project_id: lease.project_id.clone(),
        play_request_id: request,
        history: Some(HandyPlaybackHistory {
            record_id: ArtifactId::new("8".repeat(64)).unwrap(),
            original_controller: lease.clone(),
            qualification: decision.subject.device_qualification.clone(),
            setup_record_id: ArtifactId::new("7".repeat(64)).unwrap(),
            clock_request_id: config.clock_request_id.clone(),
            start_time_ms: decision.alignment.start_time_ms,
            state: HandyPlaybackOperationState::Acknowledged,
            browser_start: Some(decision.clone()),
        }),
    };
    report.validate().unwrap();
    report
}
fn owner(
    request: RequestId,
    decision: &HandyBrowserStartDecision,
    lease: &HandyControllerLease,
    current: bool,
    generation: u64,
) -> HandyBrowserPlaybackReport {
    let mut controller = lease.clone();
    controller.generation = generation;
    controller.state = if current {
        HandyControllerState::Playing
    } else {
        HandyControllerState::Retired
    };
    controller.armed = current;
    controller.physical_stop_pending = true;
    let report = HandyBrowserPlaybackReport {
        project_id: lease.project_id.clone(),
        play_request_id: request,
        browser_timing_binding_id: decision.timing_binding.binding_id.clone(),
        play_record_id: Some(ArtifactId::new("8".repeat(64)).unwrap()),
        controller: Some(controller),
        current,
        physical_stop_pending: true,
        browser_observation_id: Some(decision.observation.observation_id.clone()),
        alignment: Some(decision.alignment.clone()),
    };
    report.validate().unwrap();
    report
}
fn client(directory: &Path) -> SessionClient {
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let proof = directory.join("handy-owner.token");
    std::fs::write(&proof, "a".repeat(64)).unwrap();
    std::fs::set_permissions(proof, std::fs::Permissions::from_mode(0o600)).unwrap();
    SessionClient::from_credentials(
        &directory.join("engine.sock"),
        SessionId::new("authored-session").unwrap(),
        "synthetic-auth-never-log".into(),
    )
    .unwrap()
}
fn inspected(playback: &Playback, binding: &BrowserTimingBinding) {
    state(&playback.shared).inspected = Some(Inspected {
        player: binding.player.clone(),
        timing: binding.clone(),
        media: binding.browser_media.clone(),
        view: json!({"selection_checked":true}),
    });
}
fn consent() -> Vec<u8> {
    serde_json::to_vec(&json!({"acknowledge_current_environment":true,"allow_play":true,"allow_online_inspection":true})).unwrap()
}
fn wait_until(mut test: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(2);
    while !test() {
        assert!(Instant::now() < end, "authored client deadline");
        thread::sleep(Duration::from_millis(2));
    }
}
fn send(socket: &mut UnixStream, request: RequestId, body: pulsar_protocol::ResponseBody) {
    write_message(socket, &Response::success(request, body)).unwrap();
}
struct Joined(Option<thread::JoinHandle<usize>>);
impl Drop for Joined {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn explicit_consent_and_inspection_refuse_before_any_original_start_is_reserved() {
    let directory = tempfile::tempdir().unwrap();
    let (config, binding, _, _) = fixture();
    let mut playback = Playback::new(
        &client(directory.path()),
        binding.project_id.clone(),
        binding.revision,
        config,
    )
    .unwrap();
    let call = |body: &[u8]| {
        playback.handle(
            "/player/handy-start",
            &binding.player,
            Arc::new(AtomicBool::new(false)),
            Some(binding.browser_media.clone()),
            Some(binding.clone()),
            body,
            1,
        )
    };
    assert!(call(br#"{"acknowledge_current_environment":false,"allow_play":true,"allow_online_inspection":true}"#).is_err());
    assert!(call(&consent()).is_err());
    assert!(state(&playback.shared).intent.is_none());
    playback.shutdown().unwrap();
}

#[test]
fn renewal_and_exact_stop_continue_while_start_and_device_refresh_replies_are_held() {
    let directory = tempfile::tempdir().unwrap();
    let client = client(directory.path());
    let (config, binding, decision, lease) = fixture();
    let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let renewals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let refresh_seen = Arc::new(AtomicBool::new(false));
    let renew_count = renewals.clone();
    let refreshed = refresh_seen.clone();
    let fixture_config = config.clone();
    let selected = decision.clone();
    let original_lease = lease.clone();
    let (released_tx, released) = mpsc::sync_channel(1);
    let (unblock, unblocked) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        let mut original = None;
        let mut held_start: Option<(UnixStream, RequestId)> = None;
        let mut held_refresh: Option<(UnixStream, RequestId)> = None;
        let mut starts = 0;
        while Instant::now() < end {
            let (mut socket, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => panic!("authored socket accept {e}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let request: pulsar_protocol::Request = read_message(&mut socket).unwrap();
            match request.command {
                Command::StartHandyPlaybackFromBrowser {
                    browser_timing_binding_id,
                    ..
                } => {
                    assert_eq!(
                        browser_timing_binding_id,
                        selected.timing_binding.binding_id
                    );
                    starts += 1;
                    assert_eq!(starts, 1);
                    original = Some(request.request_id.clone());
                    held_start = Some((socket, request.request_id));
                }
                Command::RenewHandyBrowserPlayback {
                    play_request_id, ..
                } => {
                    if original.as_ref() != Some(&play_request_id) {
                        write_message(
                            &mut socket,
                            &Response::failure(
                                request.request_id,
                                ProtocolError::new(
                                    ErrorCode::Unavailable,
                                    "original admission pending",
                                ),
                            ),
                        )
                        .unwrap();
                        continue;
                    }
                    let generation = renew_count.fetch_add(1, Ordering::AcqRel) + 1;
                    send(
                        &mut socket,
                        request.request_id,
                        pulsar_protocol::ResponseBody::HandyBrowserPlayback(Box::new(owner(
                            play_request_id,
                            &selected,
                            &original_lease,
                            true,
                            generation as u64,
                        ))),
                    );
                }
                Command::HandyPlaybackStatus {
                    play_request_id, ..
                } => send(
                    &mut socket,
                    request.request_id,
                    pulsar_protocol::ResponseBody::HandyPlayback(Box::new(history(
                        play_request_id,
                        &fixture_config,
                        &selected,
                        &original_lease,
                    ))),
                ),
                Command::RefreshHandyBrowserPlaybackTiming {
                    play_request_id,
                    allow_online_inspection,
                    ..
                } => {
                    assert_eq!(Some(&play_request_id), original.as_ref());
                    assert!(allow_online_inspection);
                    assert!(held_refresh.is_none());
                    held_refresh = Some((socket, request.request_id));
                    refreshed.store(true, Ordering::Release);
                }
                Command::ReleaseHandyBrowserPlayback {
                    play_request_id,
                    browser_timing_binding_id,
                    ..
                } => {
                    assert_eq!(Some(&play_request_id), original.as_ref());
                    assert_eq!(
                        browser_timing_binding_id,
                        selected.timing_binding.binding_id
                    );
                    send(
                        &mut socket,
                        request.request_id,
                        pulsar_protocol::ResponseBody::HandyBrowserPlayback(Box::new(owner(
                            play_request_id.clone(),
                            &selected,
                            &original_lease,
                            false,
                            renew_count.load(Ordering::Acquire) as u64,
                        ))),
                    );
                    released_tx.send(()).unwrap();
                    unblocked.recv_timeout(Duration::from_secs(2)).unwrap();
                    if let Some((mut socket, id)) = held_start.take() {
                        send(
                            &mut socket,
                            id,
                            pulsar_protocol::ResponseBody::HandyPlayback(Box::new(history(
                                play_request_id,
                                &fixture_config,
                                &selected,
                                &original_lease,
                            ))),
                        );
                    }
                    if let Some((mut socket, id)) = held_refresh.take() {
                        write_message(
                            &mut socket,
                            &Response::failure(
                                id,
                                ProtocolError::new(
                                    ErrorCode::Cancelled,
                                    "authored late refresh retired",
                                ),
                            ),
                        )
                        .unwrap();
                    }
                    return starts;
                }
                _ => panic!("unexpected authored client operation"),
            }
        }
        panic!("authored server deadline");
    });
    let mut joined = Joined(Some(worker));
    let mut playback = Playback::new(
        &client,
        binding.project_id.clone(),
        binding.revision,
        config,
    )
    .unwrap();
    inspected(&playback, &binding);
    let reply = playback
        .handle(
            "/player/handy-start",
            &binding.player,
            Arc::new(AtomicBool::new(false)),
            Some(binding.browser_media.clone()),
            Some(binding.clone()),
            &consent(),
            1,
        )
        .unwrap();
    let ControlReply::Ready(reserved) = reply else {
        panic!("reservation must be immediate")
    };
    assert!(reserved["original_request_id"].is_string());
    wait_until(|| refresh_seen.load(Ordering::Acquire) && renewals.load(Ordering::Acquire) >= 2);
    let mut other = binding.player.clone();
    other.player_id = AttemptId::new("replacement-browser").unwrap();
    playback.retire(&other);
    assert!(
        !state(&playback.shared)
            .intent
            .as_ref()
            .unwrap()
            .stop_requested
    );
    playback.retire(&binding.player);
    released.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        !state(&playback.shared)
            .intent
            .as_ref()
            .unwrap()
            .start_finished
    );
    unblock.send(()).unwrap();
    playback.shutdown().unwrap();
    assert_eq!(joined.0.take().unwrap().join().unwrap(), 1);
    let snapshot = playback.snapshot(&binding.player).unwrap();
    assert_eq!(snapshot["state"], "released");
    assert_eq!(snapshot["physical_stop_pending"], true);
    assert!(snapshot["controller_generation"].is_string());
    assert!(playback
        .handle(
            "/player/handy-start",
            &binding.player,
            Arc::new(AtomicBool::new(false)),
            Some(binding.browser_media.clone()),
            Some(binding.clone()),
            &consent(),
            2
        )
        .is_err());
}

#[test]
fn original_history_and_owner_responses_reject_changed_binding_without_exposing_native_times() {
    let directory = tempfile::tempdir().unwrap();
    let (config, binding, decision, lease) = fixture();
    let native = Native {
        client: Arc::new(client(directory.path())),
        project: binding.project_id.clone(),
        revision: binding.revision,
        config: config.clone(),
        proof: "a".repeat(64),
    };
    let request = RequestId::new("original-start").unwrap();
    let mut report = history(request.clone(), &config, &decision, &lease);
    native.check_history(&report, &request, &binding).unwrap();
    report.history.as_mut().unwrap().clock_request_id = RequestId::new("foreign-clock").unwrap();
    assert!(native.check_history(&report, &request, &binding).is_err());
    let mut bad_owner = owner(request.clone(), &decision, &lease, true, 1);
    bad_owner.browser_timing_binding_id = ArtifactId::new("foreign-timing").unwrap();
    assert!(native
        .check_owner(
            pulsar_protocol::ResponseBody::HandyBrowserPlayback(Box::new(bad_owner)),
            &request,
            &binding,
            None
        )
        .is_err());
    let intent = Intent {
        request,
        player: binding.player.clone(),
        timing: binding,
        cancelled: Arc::new(AtomicBool::new(false)),
        reserved: Instant::now(),
        dispatched: false,
        start_finished: false,
        stop_requested: false,
        released: false,
        history: None,
        owner: Some(owner(
            RequestId::new("original-start").unwrap(),
            &decision,
            &lease,
            true,
            1,
        )),
        refresh_request: None,
        issue: None,
    };
    let value = view(&intent);
    assert!(value["alignment"]["phase_earliest_ns"].is_string());
    assert!(value["alignment"]["total_error_ns"].is_string());
    assert!(!value.to_string().contains("boottime"));
    assert!(!value.to_string().contains("owner_proof"));
}
