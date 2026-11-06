// Included inside live_capture_delivery_tests so the original native media and
// authored CPU fixtures remain shared. Native process join is tested separately.
mod input_close_tests {
    use super::*;
    use crate::authority::live_captures::NativeCaptureEnd;

    struct EndSink {
        engine: Weak<Engine>,
        offers: Mutex<Vec<NativeAcceptedCapture>>,
        ends: Mutex<Vec<NativeCaptureEnd>>,
        retired: AtomicUsize,
    }
    impl EndSink {
        fn new(x: &DeliveryFixture) -> Arc<Self> {
            Arc::new(Self {
                engine: Arc::downgrade(&x.f.engine),
                offers: Mutex::new(Vec::new()),
                ends: Mutex::new(Vec::new()),
                retired: AtomicUsize::new(0),
            })
        }
        fn unlocked(&self) {
            let engine = self.engine.upgrade().unwrap();
            assert!(engine.db.try_lock().is_ok());
            assert!(engine.live_captures.try_lock().is_ok());
        }
    }
    impl NativeCaptureSink for EndSink {
        fn offer(&self, stamp: NativeAcceptedCapture) -> PResult<()> {
            self.unlocked();
            self.offers.lock().unwrap().push(stamp);
            Ok(())
        }
        fn finish_input(&self, end: NativeCaptureEnd) -> PResult<()> {
            self.unlocked();
            self.ends.lock().unwrap().push(end);
            Ok(())
        }
        fn retire(&self) {
            self.retired.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn subscribe(x: &DeliveryFixture, sink: Arc<EndSink>) -> NativeCaptureSubscription {
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .register_native_delivery(x.selection(), sink)
            .unwrap()
    }
    fn close_request(x: &DeliveryFixture, final_frame: Option<LiveFrameHeader>) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::CloseBrowserAnalysisInput {
                epoch: x.capture.identity.epoch.clone(),
                final_frame,
            },
        )
    }
    fn close(x: &DeliveryFixture) -> (Request, LiveCaptureReport) {
        let request = close_request(x, x.status().last_frame);
        let report = capture_report(x.f.engine.handle(request.clone()).result.unwrap());
        (request, report)
    }
    fn join(x: &DeliveryFixture, actual_fixture_success: bool) -> bool {
        x.f.engine
            .authored_cpu_input_join_for_test(
                &x.capture.identity.epoch,
                &x.manifest.attempt_id,
                actual_fixture_success,
            )
            .unwrap()
    }

    #[test]
    fn browser_input_close_exact_boundary_replay_and_preflight_upload_race() {
        let x = delivery_fixture();
        x.offer(0);
        x.offer(1);
        x.complete(1);
        let original = x.status();
        let mut wrong = original.last_frame.clone().unwrap();
        wrong.capture_tick.0 += 1;
        assert!(x
            .f
            .engine
            .handle(close_request(&x, Some(wrong)))
            .result
            .is_err());
        assert!(x.status().input_close.is_none());
        let ready = live_handshake(&x.f, &x.capture);
        x.f.engine
            .live_bulk_ready(&ready, Some(&live_header(2, 2)))
            .unwrap();
        let (request, closing) = close(&x);
        assert_eq!(
            closing.input_close.as_ref().unwrap().phase,
            LiveCaptureInputPhase::Closing
        );
        for id in [1, 2] {
            assert_eq!(
                x.f.engine
                    .live_bulk_upload(
                        &ready,
                        &live_header(id, id as u8),
                        &vec![id as u8; LIVE_FRAME_BYTES]
                    )
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        assert!(x.f.engine.live_bulk_ready(&ready, None).is_err());
        assert!(x
            .f
            .engine
            .handle(close_request(&x, original.last_frame.clone()))
            .result
            .is_err());
        assert!(join(&x, true));
        let sealed = x.status();
        for _ in 0..3 {
            let replay = capture_report(x.f.engine.handle(request.clone()).result.unwrap());
            assert_eq!(replay.input_close, sealed.input_close);
            assert_eq!(replay.received_frames, original.received_frames);
            assert_eq!(replay.inference.as_ref().unwrap().completed, 1);
        }
        let mut changed = request;
        if let Command::CloseBrowserAnalysisInput { final_frame, .. } = &mut changed.command {
            *final_frame = None;
        }
        assert_eq!(
            x.f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
    }

    #[test]
    fn browser_input_close_drains_already_issued_exact_final_cpu_work() {
        let x = delivery_fixture();
        let sink = EndSink::new(&x);
        subscribe(&x, sink.clone());
        x.offer(0);
        x.offer(1);
        let (work, reply) = x.work(1);
        let (request, closing) = close(&x);
        assert_eq!(closing.queue.as_ref().unwrap().in_flight_frames, 1);
        assert!(!join(&x, true));
        assert!(x
            .f
            .engine
            .live_analysis_complete(
                &x.capture.identity.epoch,
                &x.manifest.attempt_id,
                work,
                reply
            )
            .unwrap());
        assert_eq!(sink.offers.lock().unwrap().len(), 1);
        assert!(join(&x, true));
        let report = x.status();
        assert_eq!(
            report.input_close.as_ref().unwrap().disposition,
            Some(LiveCaptureInputFinalDisposition::Accepted)
        );
        let ends = sink.ends.lock().unwrap();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].close_request_id(), &request.request_id);
        assert_eq!(ends[0].final_frame(), report.last_frame.as_ref());
    }

    #[test]
    fn browser_input_close_zero_and_one_frame_are_explicit_without_motion() {
        for count in 0..=1 {
            let x = delivery_fixture();
            let sink = EndSink::new(&x);
            let subscription = subscribe(&x, sink.clone());
            if count == 1 {
                x.offer(0);
            }
            let (request, _) = close(&x);
            assert!(join(&x, true));
            let report = x.status();
            let input = report.input_close.as_ref().unwrap();
            let expected = if count == 0 {
                LiveCaptureInputFinalDisposition::NoFrames
            } else {
                LiveCaptureInputFinalDisposition::Unpaired
            };
            assert_eq!(input.phase, LiveCaptureInputPhase::Sealed);
            assert_eq!(input.disposition, Some(expected));
            assert!(input.worker_joined && input.delivery_settled && report.active);
            assert_eq!(report.queue.as_ref().unwrap().pending_frames, 0);
            assert_eq!(report.inference.as_ref().unwrap().completed, 0);
            assert!(report.inference.as_ref().unwrap().latest.is_none());
            assert!(sink.offers.lock().unwrap().is_empty());
            let ends = sink.ends.lock().unwrap();
            assert_eq!(ends.len(), 1);
            assert_eq!(ends[0].close_request_id(), &request.request_id);
            assert_eq!(ends[0].final_frame(), report.last_frame.as_ref());
            assert_eq!(ends[0].disposition(), expected);
            assert!(ends[0].final_accepted().is_none());
            assert!(subscription.current());
        }
    }

    #[test]
    fn browser_input_close_waits_for_original_callback_then_emits_one_unlocked_end() {
        let x = delivery_fixture();
        let sink = EndSink::new(&x);
        let subscription = subscribe(&x, sink.clone());
        let hold =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .hold_next_native_delivery_for_test(&x.capture.identity);
        x.offer(0);
        x.offer(1);
        let (work, reply) = x.work(1);
        let engine = x.f.engine.clone();
        let epoch = x.capture.identity.epoch.clone();
        let attempt = x.manifest.attempt_id.clone();
        let worker = std::thread::spawn(move || {
            engine.live_analysis_complete(&epoch, &attempt, work, reply)
        });
        assert!(hold.wait_entered(Duration::from_secs(2)));
        let (request, closing) = close(&x);
        assert!(!closing.input_close.as_ref().unwrap().delivery_settled);
        assert!(!join(&x, true));
        assert_eq!(
            x.status().input_close.unwrap().phase,
            LiveCaptureInputPhase::Closing
        );
        assert!(sink.ends.lock().unwrap().is_empty());
        hold.release();
        assert!(worker.join().unwrap().unwrap());
        assert!(hold.settled());
        assert!(!join(&x, false));
        let joining = x.status();
        assert_eq!(
            joining.input_close.as_ref().unwrap().phase,
            LiveCaptureInputPhase::Joining
        );
        assert!(!joining.input_close.as_ref().unwrap().worker_joined);
        assert!(join(&x, true));
        let accepted = sink.offers.lock().unwrap()[0].clone();
        let end = sink.ends.lock().unwrap()[0].clone();
        assert_eq!(end.selection(), &x.selection());
        assert_eq!(end.close_request_id(), &request.request_id);
        assert_eq!(end.final_accepted().unwrap().frame, accepted.frame);
        assert_eq!(
            end.final_accepted().unwrap().capture_boottime,
            accepted.capture_boottime
        );
        assert_eq!(
            end.final_accepted().unwrap().accepted_boottime,
            accepted.accepted_boottime
        );
        assert_eq!(end.continuity_generation(), accepted.continuity_generation);
        assert!(end.closed_at_boottime_ns() >= accepted.capture_boottime.earliest_ns);
        for _ in 0..3 {
            x.status().validate().unwrap();
            assert!(!join(&x, true));
        }
        assert_eq!(sink.ends.lock().unwrap().len(), 1);
        assert_eq!(sink.offers.lock().unwrap().len(), 1);
        assert!(subscription.current());
    }

    #[test]
    fn browser_input_close_sealed_source_keeps_original_lease_but_blocks_new_admission() {
        let x = delivery_fixture();
        let sink = EndSink::new(&x);
        let subscription = subscribe(&x, sink.clone());
        x.offer(0);
        x.offer(1);
        x.complete(1);
        let original =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_source_snapshot(&subscription)
                .unwrap();
        close(&x);
        assert!(join(&x, true));
        {
            let mut registry = x.f.engine.live_captures.lock().unwrap();
            for _ in 0..16 {
                registry.age_for_test(&x.capture.identity.epoch);
            }
        }
        let report = x.status();
        assert!(report.active);
        assert!(
            !report
                .inference
                .as_ref()
                .unwrap()
                .latest
                .as_ref()
                .unwrap()
                .fresh
        );
        let registry = x.f.engine.live_captures.lock().unwrap();
        let current = registry.native_source_snapshot(&subscription).unwrap();
        assert_eq!(
            current.lease_deadline_boottime_ns,
            original.lease_deadline_boottime_ns
        );
        assert_eq!(
            current.latest.as_ref().unwrap().capture_boottime,
            original.latest.as_ref().unwrap().capture_boottime
        );
        assert!(registry.native_source_basis(&x.selection()).is_err());
        assert!(
            subscription.current(),
            "new admission refusal must not retire existing stream"
        );
        drop(registry);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                Some(1),
                Command::StopLiveCapture {
                    epoch: x.capture.identity.epoch.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!subscription.current());
        assert!(!x.status().active);
        assert_eq!(sink.retired.load(Ordering::Acquire), 1);
    }

    #[test]
    fn browser_input_close_stop_or_worker_failure_before_join_never_seals() {
        for failure in [false, true] {
            let x = delivery_fixture();
            let sink = EndSink::new(&x);
            let subscription = subscribe(&x, sink.clone());
            close(&x);
            assert!(!join(&x, false));
            if failure {
                x.f.engine
                    .live_analysis_failure_for_test(
                        &x.capture.identity.epoch,
                        &x.manifest.attempt_id,
                    )
                    .unwrap();
            } else {
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        Some(1),
                        Command::StopLiveCapture {
                            epoch: x.capture.identity.epoch.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            }
            assert!(!join(&x, true));
            let report = x.status();
            let input = report.input_close.unwrap();
            assert_eq!(input.phase, LiveCaptureInputPhase::Aborted);
            assert!(!input.worker_joined);
            assert!(!subscription.current());
            assert!(sink.ends.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn browser_input_close_rejects_sealed_report_with_prior_or_missing_final_estimate() {
        let x = delivery_fixture();
        x.offer(0);
        x.offer(1);
        x.complete(1);
        close(&x);
        assert!(join(&x, true));
        let original = x.status();
        for mutation in 0..6 {
            let mut report = original.clone();
            match mutation {
                0 => report.input_close.as_mut().unwrap().worker_joined = false,
                1 => report.input_close.as_mut().unwrap().delivery_settled = false,
                2 => report.queue.as_mut().unwrap().pending_frames = 1,
                3 => report.inference.as_mut().unwrap().latest = None,
                4 => {
                    report.input_close.as_mut().unwrap().disposition =
                        Some(LiveCaptureInputFinalDisposition::Unpaired)
                }
                _ => {
                    report
                        .input_close
                        .as_mut()
                        .unwrap()
                        .final_frame
                        .as_mut()
                        .unwrap()
                        .capture_tick
                        .0 += 1
                }
            }
            assert!(report.validate().is_err(), "mutation {mutation}");
        }
    }
}
