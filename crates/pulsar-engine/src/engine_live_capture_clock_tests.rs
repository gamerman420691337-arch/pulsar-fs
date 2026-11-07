// Authored CPU/CUDA reply fixtures only; no model quality, acquisition clock or
// physical device qualification is established by these timing checks.
#[cfg(target_os = "linux")]
fn clock_reply_for_work(
    template: &LiveInferenceReply,
    work: &crate::live_frames::LiveRgbWork,
    sequence: u64,
) -> LiveInferenceReply {
    let mut result = template.clone();
    let LiveInferenceReply::Estimated { input, .. } = &mut result else {
        panic!("CPU fixture");
    };
    input.sequence = sequence;
    input.pair = work.declaration();
    let artifact = |bytes: &[u8]| {
        let hash = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: bytes.len() as u64,
        }
    };
    input.previous = artifact(work.previous());
    input.current = artifact(work.current());
    result
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_cpu_stamp_preserves_exact_pair_original_ticks_and_age_basis() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    let previous = work.previous_frame().clone();
    let current = work.token().frame().clone();
    let pair = work.declaration();
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let stamped = {
        let registry = f.engine.live_captures.lock().unwrap();
        let stamp = registry
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .unwrap();
        assert_eq!(stamp.previous_frame, previous);
        assert_eq!(stamp.frame, current);
        assert_eq!(stamp.estimate.capture_tick, current.capture_tick);
        assert_eq!(
            stamp.estimate.age_basis,
            LiveCaptureAgeBasis::ProducerDeclared
        );
        assert!(!stamp.estimate.qualified && !stamp.continuous_from_previous);
        let LiveInferenceReply::Estimated { input, .. } = &stamp.estimate.result else {
            panic!();
        };
        assert_eq!(input.pair, pair);
        assert!(stamp.previous_capture_boottime.earliest_ns <= stamp.capture_boottime.earliest_ns);
        assert!(stamp.capture_boottime.earliest_ns <= stamp.accepted_boottime.earliest_ns);
        assert_eq!(
            stamp.accepted_boottime.earliest_ns - stamp.capture_boottime.earliest_ns,
            stamp.estimate.capture_to_analysis_ns
        );
        stamp.clone()
    };
    {
        let registry = f.engine.live_captures.lock().unwrap();
        let again = registry
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .unwrap();
        assert_eq!(again.capture_boottime, stamped.capture_boottime);
        assert_eq!(again.accepted_boottime, stamped.accepted_boottime);
        assert_eq!(again.estimate.accepted_at, stamped.estimate.accepted_at);
        assert_eq!(
            registry
                .native_accepted(&SessionId::new("foreign").unwrap(), &capture.identity)
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .age_for_test(&capture.identity.epoch);
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .is_none());
}

#[test]
fn live_native_clock_unavailable_preserves_ordinary_cpu_preview_and_refuses_native_stamp() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .unavailable_clock_for_test(&capture.identity.epoch);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    let status = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap(),
    );
    assert!(status.active);
    assert_eq!(status.inference.as_ref().unwrap().accepted, 1);
    assert!(status.inference.unwrap().latest.is_some());
    assert_eq!(
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .native_accepted(&f.session, &capture.identity)
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_suspend_retires_before_completion_and_clears_existing_stamp() {
    for accepted_first in [false, true] {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        let pending = if accepted_first {
            assert!(f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
            assert!(f
                .engine
                .live_captures
                .lock()
                .unwrap()
                .native_accepted(&f.session, &capture.identity)
                .unwrap()
                .is_some());
            None
        } else {
            Some((work, result))
        };
        f.engine
            .live_captures
            .lock()
            .unwrap()
            .suspend_for_test(&capture.identity.epoch);
        if let Some((work, result)) = pending {
            assert!(!f
                .engine
                .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
                .unwrap());
        }
        let status = capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        assert!(!status.active);
        assert_eq!(
            status.retirement,
            Some(LiveCaptureRetirement::AuthorityLost)
        );
        assert!(status.inference.unwrap().latest.is_none());
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .is_none());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_cuda_provisional_and_stale_cpu_never_mint_native_acceptance() {
    let f = fixture();
    let (capture, attempt, work, result) = cuda_analysis_work(&f);
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .is_none());
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    f.engine
        .live_captures
        .lock()
        .unwrap()
        .age_for_test(&capture.identity.epoch);
    // Completion true means handled; the result is stale, not accepted.
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .is_none());
    let status = capture_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::LiveCaptureStatus {
                    epoch: capture.identity.epoch,
                },
            ))
            .result
            .unwrap(),
    );
    assert_eq!(status.inference.as_ref().unwrap().accepted, 0);
    assert_eq!(status.inference.unwrap().stale, 1);
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_pair_continuity_requires_exact_preceding_accepted_frame() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    let template = result.clone();
    let handshake = live_handshake(&f, &capture);
    f.engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap();
    for (frames, sequence, expected) in [(vec![2], 2, true), (vec![3, 4], 3, false)] {
        for frame in frames {
            f.engine
                .live_bulk_upload(
                    &handshake,
                    &live_header(frame, frame as u8),
                    &vec![frame as u8; LIVE_FRAME_BYTES],
                )
                .unwrap();
        }
        let work = f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .take_for_test(&capture.identity.epoch);
        let result = clock_reply_for_work(&template, &work, sequence);
        f.engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap();
        let registry = f.engine.live_captures.lock().unwrap();
        assert_eq!(
            registry
                .native_accepted(&f.session, &capture.identity)
                .unwrap()
                .unwrap()
                .continuous_from_previous,
            expected
        );
    }
    f.engine
        .live_bulk_upload(
            &handshake,
            &live_header(1000, 5),
            &vec![5; LIVE_FRAME_BYTES],
        )
        .unwrap();
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .is_none());
    f.engine
        .live_bulk_upload(
            &handshake,
            &live_header(1001, 6),
            &vec![6; LIVE_FRAME_BYTES],
        )
        .unwrap();
    let work = f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .take_for_test(&capture.identity.epoch);
    let result = clock_reply_for_work(&template, &work, 4);
    f.engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap();
    let registry = f.engine.live_captures.lock().unwrap();
    let stamp = registry
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .unwrap();
    assert_eq!(stamp.continuity_generation, 1);
    assert!(!stamp.continuous_from_previous);
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_issued_result_cannot_bridge_newer_source_discontinuity() {
    let f = fixture();
    let (capture, attempt, work, result) = analysis_work(&f);
    let handshake = live_handshake(&f, &capture);
    for (frame, byte) in [(1000, 3), (1001, 4)] {
        f.engine
            .live_bulk_upload(
                &handshake,
                &live_header(frame, byte),
                &vec![byte; LIVE_FRAME_BYTES],
            )
            .unwrap();
    }
    // The scheduler can accept original fresh work despite newer pending pixels,
    // but this old pair cannot restore the native stamp cleared by the gap.
    assert!(f
        .engine
        .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
        .unwrap());
    assert!(f
        .engine
        .live_captures
        .lock()
        .unwrap()
        .native_accepted(&f.session, &capture.identity)
        .unwrap()
        .is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn live_native_clock_failure_and_explicit_retirement_clear_native_stamp() {
    for failed in [false, true] {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        f.engine
            .live_analysis_complete(&capture.identity.epoch, &attempt, work, result)
            .unwrap();
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .is_some());
        if failed {
            f.engine
                .live_analysis_failure_for_test(&capture.identity.epoch, &attempt)
                .unwrap();
        } else {
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(0),
                    Command::StopLiveCapture {
                        epoch: capture.identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap();
        }
        assert!(f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_accepted(&f.session, &capture.identity)
            .unwrap()
            .is_none());
    }
}
