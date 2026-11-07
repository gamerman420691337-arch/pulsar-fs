//! Real framed client transport over authored Engine reports. No native worker,
//! browser rendering, accepted equivalence or physical qualification is claimed.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

fn artifact(n: u8) -> ArtifactIdentity {
    let sha256 = format!("{n:064x}");
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: 10,
    }
}
fn reports() -> (AnalysisFeed, LiveCaptureReport, LiveCaptureReport) {
    let (_, mut feed, mut target) = input_fixture();
    target.received_frames = 0;
    target.last_frame = None;
    target.queue = None;
    let mut cpu = super::super::browser_captured_cpu_tests::captured_report()
        .inference
        .unwrap()
        .cpu_manifest
        .unwrap();
    cpu.stream.identity = target.identity.clone();
    cpu.stream.clock = target.clock.clone();
    cpu.stream.attempt_id = target.inference.as_ref().unwrap().attempt_id.clone();
    cpu.stream.budget.wall_time_ms = u64::from(target.options.lease_ms);
    let selection = BrowserInferenceFallbackSelection {
        schema_version: 1,
        tier: InferenceEquivalenceTier::Default,
        axis: Axis::Stroke,
        equivalents: vec![InferenceFallbackReference {
            dossier_id: ArtifactId::new("1".repeat(64)).unwrap(),
            expected_revision: 1,
            expected_report_sha256: "2".repeat(64),
        }],
        maximum_fallbacks: 1,
        source_policy: BrowserInferenceFallbackSourcePolicy::any_bound_media(),
        view_policy: BrowserInferenceFallbackViewPolicy {
            schema_version: 1,
            namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
            applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
        },
    };
    target.options.inference_fallback = Some(selection.clone());
    let info = target.inference.as_mut().unwrap();
    info.cpu_manifest = Some(cpu.clone());
    info.completed = 0;
    info.accepted = 0;
    info.stale = 0;
    info.latest = None;
    info.fallback = Some(BrowserInferenceFallbackAdmission {
        schema_version: 1,
        selection: selection.identity().unwrap(),
        reference: selection.equivalents[0].clone(),
        semantic_identity: browser_inference_semantic_identity(&target.options).unwrap(),
        observed_environment: InferenceEquivalenceEnvironment {
            engine: artifact(101),
            worker: cpu.stream.worker.clone(),
            os: "linux".into(),
            architecture: "x86_64".into(),
            platform_build: artifact(102),
            hardware: artifact(103),
            dependencies: vec![cpu.stream.profile.runtime.clone()],
        },
        admitted_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        ordinal: 0,
        predecessor: None,
        history: vec![],
    });
    let mut previous = target.clone();
    let mut manifest: LiveCudaInferenceManifest = serde_json::from_str(include_str!(
        "../../pulsar-engine/tests/fixtures/authored-live-cuda-manifest.json"
    ))
    .unwrap();
    let old = manifest.stream.model.identity.clone();
    let model = cpu.stream.profile.model.clone();
    manifest.stream.model.identity = model.clone();
    manifest.stream.profile.model = model.clone();
    manifest.dependencies.retain(|id| *id != old);
    if !manifest.dependencies.contains(&model) {
        manifest.dependencies.push(model);
    }
    previous.identity.epoch = AttemptId::new("failed-original").unwrap();
    previous.clock = previous.options.clock(previous.identity.epoch.clone());
    manifest.stream.identity = previous.identity.clone();
    manifest.stream.clock = previous.clock.clone();
    manifest.stream.attempt_id = AttemptId::new("failed-attempt").unwrap();
    previous.options.lease_ms = u32::try_from(manifest.stream.budget.wall_time_ms).unwrap();
    previous.options.inference.as_mut().unwrap().profile = manifest.stream.profile.clone();
    let profile_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest.stream.profile).unwrap())
    );
    let failure = BrowserInferenceFallbackFailure {
        manifest: Box::new(manifest),
        observation: InferenceProviderUnavailable {
            schema_version: 1,
            provider: InferenceProvider::Cuda,
            profile_sha256: profile_sha256.clone(),
            check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
        },
    };
    previous.active = false;
    previous.admitted_pixel_bytes = 0;
    previous.retirement = Some(LiveCaptureRetirement::WorkerFailed);
    let info = previous.inference.as_mut().unwrap();
    info.cpu_manifest = None;
    info.state = LiveCaptureInferenceState::Failed;
    info.attempt_id = failure.manifest.stream.attempt_id.clone();
    info.profile_sha256 = profile_sha256;
    info.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "authored provider failure",
    ));
    info.fallback.as_mut().unwrap().observed_environment.worker =
        failure.manifest.stream.worker.clone();
    info.fallback
        .as_mut()
        .unwrap()
        .observed_environment
        .dependencies = vec![failure.manifest.stream.profile.runtime.clone()];
    info.fallback_transition = Some(BrowserInferenceFallbackTransition {
        failure: failure.clone(),
        phase: BrowserInferenceFallbackPhase::Started,
        next_epoch: Some(target.identity.epoch.clone()),
        error: None,
    });
    feed.binding.capture_epoch = previous.identity.epoch.clone();
    feed.cpu_mode = AnalysisCpuMode::from_report(&previous).unwrap();
    target.options.browser_analysis.as_mut().unwrap().feed_id =
        ArtifactId::new("fresh-target-feed").unwrap();
    let info = target.inference.as_mut().unwrap();
    let admission = info.fallback.as_mut().unwrap();
    admission.ordinal = 1;
    admission.predecessor = Some(previous.identity.clone());
    admission.history.push(BrowserInferenceFallbackStep {
        failure,
        reference: admission.reference.clone(),
        next_profile_sha256: info.profile_sha256.clone(),
    });
    previous.validate().unwrap();
    target.validate().unwrap();
    validate_browser_fallback_target(&previous, &target).unwrap();
    (feed, previous, target)
}
fn recording(target: &LiveCaptureReport, id: RequestId) -> LiveRecordingPage {
    LiveRecordingPage {
        fallback: BrowserInferenceRecordingProvenance::from_capture(target)
            .unwrap()
            .map(Box::new),
        cpu_manifest: target.inference.as_ref().unwrap().cpu_manifest.clone(),
        browser_media: target.options.browser_media.clone(),
        model_pack: target.options.model_pack.clone(),
        source_time_basis: target.options.source_time_basis,
        schema: 1,
        engine_epoch: target.engine_epoch.clone(),
        identity: target.identity.clone(),
        clock: target.clock.clone(),
        profile: target.options.inference.as_ref().unwrap().profile.clone(),
        inference_attempt: target.inference.as_ref().unwrap().attempt_id.clone(),
        recording_request_id: id,
        maximum_samples: 4,
        total_samples: 0,
        discarded: false,
        exhausted: false,
        active: true,
        retirement: None,
        after: 0,
        next: None,
        samples: vec![],
        qualified: false,
    }
}
fn accept(listener: &UnixListener) -> Option<UnixStream> {
    match listener.accept() {
        Ok((s, _)) => {
            // Accepted sockets may inherit the listener's nonblocking flag on BSD.
            s.set_nonblocking(false).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            s.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
            Some(s)
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(e) => panic!("{e}"),
    }
}
struct Peer {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}

fn scenario(
    lost_record_ack: bool,
    lost_bulk_ack: bool,
    wrong_stop_ack: bool,
    check: impl FnOnce(&mut Preview, &Arc<Mutex<Vec<String>>>, Instant),
) {
    let (feed, previous, target) = reports();
    let directory = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = directory.path().join("engine.sock");
    let command = UnixListener::bind(&endpoint).unwrap();
    command.set_nonblocking(true).unwrap();
    let bulk = UnixListener::bind(directory.path().join("bulk.sock")).unwrap();
    bulk.set_nonblocking(true).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let server_target = target.clone();
    let server_previous = previous.clone();
    let worker = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(8);
        let mut channels = Vec::new();
        while !stopping.load(Ordering::Acquire) && Instant::now() < end {
            if let Some(mut stream) = accept(&command) {
                let wire: pulsar_protocol::Request = read_message(&mut stream).unwrap();
                wire.validate().unwrap();
                assert_eq!(
                    wire.session.as_ref().unwrap().as_str(),
                    "fallback-wire-session"
                );
                let body = match wire.command {
                    Command::LiveCaptureStatus { epoch } => {
                        assert_eq!(epoch, server_target.identity.epoch);
                        observed.lock().unwrap().push("status".into());
                        pulsar_protocol::ResponseBody::LiveCapture(Box::new(server_target.clone()))
                    }
                    Command::StartLiveRecording {
                        epoch,
                        maximum_samples,
                    } => {
                        assert_eq!(epoch, server_target.identity.epoch);
                        assert_eq!(maximum_samples, 4);
                        observed
                            .lock()
                            .unwrap()
                            .push(format!("record:{}", wire.request_id));
                        if lost_record_ack {
                            continue;
                        }
                        pulsar_protocol::ResponseBody::LiveRecordingPage(Box::new(recording(
                            &server_target,
                            wire.request_id.clone(),
                        )))
                    }
                    Command::StopLiveCapture { epoch } => {
                        observed.lock().unwrap().push(format!("stop:{epoch}"));
                        let mut r = if epoch == server_target.identity.epoch || wrong_stop_ack {
                            server_target.clone()
                        } else {
                            assert_eq!(epoch, server_previous.identity.epoch);
                            server_previous.clone()
                        };
                        r.active = false;
                        r.admitted_pixel_bytes = 0;
                        r.retirement = Some(LiveCaptureRetirement::Stopped);
                        r.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Retired;
                        r.inference.as_mut().unwrap().error = None;
                        pulsar_protocol::ResponseBody::LiveCapture(Box::new(r))
                    }
                    other => panic!("fallback must never create replacement Start: {other:?}"),
                };
                write_message(&mut stream, &Response::success(wire.request_id, body)).unwrap();
            }
            if let Some(mut stream) = accept(&bulk) {
                let handshake: LiveBulkHandshake = read_bulk_header(&mut stream).unwrap();
                assert_eq!(handshake.identity, server_target.identity);
                observed.lock().unwrap().push("bulk".into());
                if lost_bulk_ack {
                    continue;
                }
                write_bulk_header(&mut stream, &LiveBulkReply::Ready(server_target.clone()))
                    .unwrap();
                channels.push(stream);
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let _peer = Peer {
        stop,
        worker: Some(worker),
    };
    let mut client = SessionClient::from_credentials(
        &endpoint,
        SessionId::new("fallback-wire-session").unwrap(),
        "authored-wire-token".into(),
    )
    .unwrap();
    client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(4)));
    let deadline = feed.capture_deadline.unwrap();
    let mut preview = Preview {
        client,
        project: previous.identity.project.clone(),
        revision: previous.identity.revision,
        options: previous.options.clone(),
        report: Some(previous.clone()),
        analysis_feed: Some(feed),
        analysis_successor: None,
        analysis_commit: None,
        analysis_last_transfer: None,
        analysis_cancelled: false,
        bulk: None,
        sequence: 7,
        last_time: Some(100),
        blocked: false,
        playback_epoch: Some(previous.identity.epoch),
        pending: None,
        eligible_after: Instant::now(),
        handoffs: 0,
        transition_drops: 0,
        recording_maximum: Some(4),
        recordings: vec![],
        retired_recordings: vec![],
        candidate: None,
        export: None,
        gap: None,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check(&mut preview, &calls, deadline)
    }));
    preview.report = None;
    preview.analysis_feed = None;
    preview.bulk = None;
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
#[test]
fn framed_target_handover_records_before_bulk_and_preserves_deadline_and_playback() {
    scenario(false, false, false, |p, calls, deadline| {
        let playback = p.playback_epoch.clone();
        assert!(fallback::reconcile(p).unwrap());
        assert_eq!(
            p.analysis_feed.as_ref().unwrap().capture_deadline,
            Some(deadline)
        );
        assert_eq!(p.playback_epoch, playback);
        assert_eq!(p.sequence, 0);
        assert_eq!(p.last_time, None);
        assert!(p.analysis_feed.as_ref().unwrap().last_upload.is_none());
        let view = fallback::discarded_view(p).unwrap();
        assert_eq!(view["browser"]["analysis_frame_discarded"], true);
        let log = calls.lock().unwrap();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0], "status");
        assert!(log[1].starts_with("record:"));
        assert_eq!(log[2], "bulk");
        drop(log);
        assert!(!fallback::reconcile(p).unwrap());
        p.stop().unwrap();
        assert!(calls.lock().unwrap().last().unwrap().starts_with("stop:"));
    });
}
#[test]
fn lost_recording_ack_latches_original_intent_and_opens_no_bulk() {
    scenario(true, false, false, |p, calls, _| {
        assert!(fallback::reconcile(p).is_err());
        assert!(p.blocked);
        assert_eq!(p.recordings.len(), 1);
        let id = p.recordings[0].request.clone();
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["status", format!("record:{id}").as_str()]
        );
        assert!(p.bulk.is_none());
        p.stop().unwrap();
        assert_eq!(p.recordings[0].request, id);
    });
}

#[test]
fn lost_bulk_ack_keeps_recording_intent_and_stops_the_known_target() {
    scenario(false, true, false, |preview, calls, _| {
        assert!(fallback::reconcile(preview).is_err());
        assert!(preview.blocked);
        assert!(preview.bulk.is_none());
        assert_eq!(preview.recordings.len(), 1);
        let id = preview.recordings[0].request.clone();
        preview.stop().unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[1], format!("record:{id}"));
        assert_eq!(calls[2], "bulk");
        assert!(calls[3].starts_with("stop:"));
    });
}

#[test]
fn expired_deadline_or_changed_native_timing_refuses_before_recording_or_bulk() {
    for fault in ["deadline", "timing"] {
        scenario(false, false, false, |preview, calls, _| {
            let feed = preview.analysis_feed.as_mut().unwrap();
            if fault == "deadline" {
                feed.capture_deadline = Some(Instant::now());
            } else {
                feed.browser_timing.binding_id = ArtifactId::new("foreign-timing").unwrap();
            }
            assert!(fallback::reconcile(preview).is_err(), "{fault}");
            assert_eq!(calls.lock().unwrap().as_slice(), ["status"]);
            assert!(preview.recordings.is_empty());
            assert!(preview.bulk.is_none());
        });
    }
}

#[test]
fn preparing_original_transition_waits_without_creating_any_new_request() {
    scenario(false, false, false, |preview, calls, deadline| {
        let transition = preview
            .report
            .as_mut()
            .unwrap()
            .inference
            .as_mut()
            .unwrap()
            .fallback_transition
            .as_mut()
            .unwrap();
        transition.phase = BrowserInferenceFallbackPhase::Preparing;
        transition.next_epoch = None;
        assert!(fallback::reconcile(preview).unwrap());
        assert!(calls.lock().unwrap().is_empty());
        assert!(preview.recordings.is_empty());
        assert_eq!(preview.sequence, 7);
        assert_eq!(
            preview.analysis_feed.as_ref().unwrap().capture_deadline,
            Some(deadline)
        );
    });
}

#[test]
fn stop_of_preparing_ancestor_requires_its_exact_original_acknowledgement() {
    for wrong in [false, true] {
        scenario(false, false, wrong, |preview, calls, _| {
            let original = preview.report.as_ref().unwrap().identity.clone();
            let result = preview.stop();
            assert_eq!(result.is_err(), wrong);
            assert_eq!(preview.blocked, wrong);
            assert_eq!(preview.report.is_some(), wrong);
            assert_eq!(
                calls.lock().unwrap().as_slice(),
                [format!("stop:{}", original.epoch)]
            );
        });
    }
}
