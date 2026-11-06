// Included in live_capture_delivery_tests. CPU replies and join acknowledgments
// are authored here; actual worker custody is exercised by the native campaign.
mod successor_tests {
    use super::*;
    use crate::authority::live_captures::{
        NativeCaptureEnd, NativeCaptureSuccessor, NativeCaptureTransfer,
    };

    struct TransferSink {
        engine: Weak<Engine>,
        offers: Mutex<Vec<NativeAcceptedCapture>>,
        transfers: Mutex<Vec<NativeCaptureTransfer>>,
        ends: AtomicUsize,
        retired: AtomicUsize,
    }
    impl TransferSink {
        fn new(x: &DeliveryFixture) -> Arc<Self> {
            Arc::new(Self {
                engine: Arc::downgrade(&x.f.engine),
                offers: Mutex::new(Vec::new()),
                transfers: Mutex::new(Vec::new()),
                ends: AtomicUsize::new(0),
                retired: AtomicUsize::new(0),
            })
        }
        fn unlocked(&self) {
            let engine = self.engine.upgrade().unwrap();
            assert!(engine.db.try_lock().is_ok());
            assert!(engine.live_captures.try_lock().is_ok());
        }
    }
    impl NativeCaptureSink for TransferSink {
        fn offer(&self, stamp: NativeAcceptedCapture) -> PResult<()> {
            self.unlocked();
            self.offers.lock().unwrap().push(stamp);
            Ok(())
        }
        fn transfer_input(&self, proof: NativeCaptureTransfer) -> PResult<()> {
            self.unlocked();
            self.transfers.lock().unwrap().push(proof);
            Ok(())
        }
        fn finish_input(&self, _: NativeCaptureEnd) -> PResult<()> {
            self.unlocked();
            self.ends.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn retire(&self) {
            self.retired.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn successor(x: &DeliveryFixture) -> NativeCaptureSuccessor {
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .authored_successor_for_test(&x.f.engine, &x.capture.identity.epoch)
            .unwrap()
    }
    fn report(x: &DeliveryFixture, epoch: &AttemptId) -> LiveCaptureReport {
        capture_report(
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::LiveCaptureStatus {
                        epoch: epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }
    fn upload(
        x: &DeliveryFixture,
        capture: &LiveCaptureReport,
        frame: u64,
        source_ms: u64,
    ) -> LiveFrameHeader {
        let handshake = live_handshake(&x.f, capture);
        let ready = x.f.engine.live_bulk_ready(&handshake, None).unwrap();
        let bytes = vec![frame as u8; LIVE_FRAME_BYTES];
        let header = LiveFrameHeader {
            frame: FrameId::new(frame),
            source_time: SourceTimestamp::new(i64::try_from(source_ms).unwrap(), 1000).unwrap(),
            capture_tick: ready.server_elapsed.unwrap(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        };
        assert!(
            !x.f.engine
                .live_bulk_upload(&handshake, &header, &bytes)
                .unwrap()
                .1
        );
        header
    }
    fn prepare_final(x: &DeliveryFixture) -> LiveFrameHeader {
        upload(x, &x.capture, 0, 100);
        let final_frame = upload(x, &x.capture, 1, 200);
        x.complete(1);
        final_frame
    }
    fn transfer_request(
        x: &DeliveryFixture,
        cap: &NativeCaptureSuccessor,
        boundary: LiveFrameHeader,
    ) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            Some(1),
            Command::TransferBrowserAnalysisInput {
                predecessor: x.capture.identity.epoch.clone(),
                successor: cap.successor().identity.epoch.clone(),
                boundary,
            },
        )
    }
    fn transfer_report(body: ResponseBody) -> BrowserAnalysisTransferReport {
        let ResponseBody::BrowserAnalysisTransfer(report) = body else {
            panic!("transfer report");
        };
        *report
    }
    fn status(x: &DeliveryFixture, original: &RequestId) -> BrowserAnalysisTransferReport {
        transfer_report(
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    None,
                    Command::BrowserAnalysisTransferStatus {
                        transfer_request_id: original.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }
    fn complete_successor(x: &DeliveryFixture, cap: &NativeCaptureSuccessor) {
        let work =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .take_for_test(&cap.successor().identity.epoch);
        let artifact = |bytes: &[u8]| {
            let sha256 = format!("{:x}", Sha256::digest(bytes));
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: bytes.len() as u64,
            }
        };
        let reply = LiveInferenceReply::Estimated {
            attempt_id: cap.successor().attempt_id.clone(),
            identity: cap.successor().identity.clone(),
            profile_sha256: cap.successor().profile_sha256.clone(),
            input: LiveInferenceInput {
                sequence: 1,
                pair: work.declaration(),
                previous: artifact(work.previous()),
                current: artifact(work.current()),
            },
            position: NormalizedPosition::new(0.25).unwrap(),
            pair_analysis_elapsed_ns: 1,
        };
        assert!(x
            .f
            .engine
            .live_analysis_complete(
                &cap.successor().identity.epoch,
                &cap.successor().attempt_id,
                work,
                reply
            )
            .unwrap());
    }
    fn join(x: &DeliveryFixture) {
        assert!(x
            .f
            .engine
            .authored_cpu_input_join_for_test(
                &x.capture.identity.epoch,
                &x.manifest.attempt_id,
                true
            )
            .unwrap());
    }

    #[test]
    fn preview_successor_requires_shared_pair_and_predecessor_join_before_promotion() {
        let x = delivery_fixture();
        let cap = successor(&x);
        assert!(x.status().active);
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_basis(cap.successor())
            .is_err());
        assert!(x
            .f
            .engine
            .live_bulk_ready(
                &live_handshake(&x.f, &report(&x, &cap.successor().identity.epoch)),
                None
            )
            .is_err());
        let final_frame = prepare_final(&x);
        let original = transfer_request(&x, &cap, final_frame.clone());
        let initial = transfer_report(x.f.engine.handle(original.clone()).result.unwrap());
        assert_eq!(
            initial.phase,
            BrowserAnalysisTransferPhase::BoundaryReserved
        );
        let seeded = report(&x, &cap.successor().identity.epoch);
        assert_eq!(seeded.received_frames, 1);
        assert_eq!(
            seeded.last_frame.as_ref().unwrap().sha256,
            final_frame.sha256
        );
        assert_eq!(
            seeded.last_frame.as_ref().unwrap().source_time,
            final_frame.source_time
        );
        assert_eq!(seeded.inference.as_ref().unwrap().completed, 0);
        upload(&x, &seeded, 1, 300);
        complete_successor(&x, &cap);
        let before_join = status(&x, &original.request_id);
        assert!(before_join.successor_pair_accepted);
        assert!(!before_join.predecessor_worker_joined);
        assert!(x.status().active);
        join(&x);
        let transferred = status(&x, &original.request_id);
        assert_eq!(transferred.phase, BrowserAnalysisTransferPhase::Transferred);
        assert!(transferred.predecessor_worker_joined && transferred.predecessor_delivery_settled);
        assert!(!x.status().active);
        assert!(report(&x, &cap.successor().identity.epoch).active);
        assert!(report(&x, &cap.successor().identity.epoch)
            .input_close
            .is_none());
        for _ in 0..3 {
            assert_eq!(
                transfer_report(x.f.engine.handle(original.clone()).result.unwrap()).phase,
                BrowserAnalysisTransferPhase::Transferred
            );
        }
        cap.cancel(); // Consumed proof cannot cancel the now-active successor.
        assert!(report(&x, &cap.successor().identity.epoch).active);
        upload(&x, &seeded, 2, 400);
    }

    #[test]
    fn attached_successor_retains_exact_proof_until_one_successful_native_commit() {
        let x = delivery_fixture();
        let a = TransferSink::new(&x);
        let b = TransferSink::new(&x);
        let a_sub =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .register_native_delivery(x.selection(), a.clone())
                .unwrap();
        let cap = successor(&x);
        let b_sub =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .register_native_successor_delivery(&cap, b.clone())
                .unwrap();
        let final_frame = prepare_final(&x);
        let original = transfer_request(&x, &cap, final_frame.clone());
        x.f.engine.handle(original.clone()).result.unwrap();
        let seeded = report(&x, &cap.successor().identity.epoch);
        upload(&x, &seeded, 1, 300);
        complete_successor(&x, &cap);
        join(&x);
        let proof = a.transfers.lock().unwrap()[0].clone();
        assert_eq!(a.transfers.lock().unwrap().len(), 1);
        assert_eq!(b.offers.lock().unwrap().len(), 1);
        assert!(!proof.successor_first().continuous_from_previous);
        assert_eq!(
            proof.successor_first().previous_frame.frame,
            FrameId::new(0)
        );
        assert_eq!(proof.successor_first().frame.frame, FrameId::new(1));
        assert_eq!(
            proof.predecessor_final().frame.artifact,
            proof.successor_first().previous_frame.artifact
        );
        assert_eq!(
            proof.boundary().predecessor_capture,
            proof.predecessor_final().capture_boottime
        );
        assert_eq!(
            proof.boundary().successor_capture,
            proof.successor_first().previous_capture_boottime
        );
        assert_eq!(
            status(&x, &original.request_id).phase,
            BrowserAnalysisTransferPhase::ReadyForTransfer
        );
        assert_eq!(
            x.status().inference.as_ref().unwrap().state,
            LiveCaptureInferenceState::TransferJoined
        );
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .native_source_snapshot(&a_sub)
            .is_ok());
        assert!(x
            .f
            .engine
            .live_bulk_ready(&live_handshake(&x.f, &seeded), None)
            .is_err());
        let timing =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .native_successor_basis(&cap)
                .unwrap()
                .browser_timing;
        {
            let _held = timing.hold_registry_for_test();
            let mut captures = x.f.engine.live_captures.lock().unwrap();
            let guard = captures.try_claim_native_transfer(&proof).unwrap();
            let withheld: PResult<()> = guard.commit_with(|_, _| {
                assert!(matches!(
                    timing.try_claim_native(|| panic!("contended claim sampled clock")),
                    Err(crate::authority::browser_timing::BrowserTimingClaimFailure::Contended)
                ));
                Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "actual timing publication guard is held",
                ))
            });
            assert!(withheld.is_err());
            assert!(a_sub.current() && b_sub.current());
        }
        let failed: PResult<()> =
            x.f.engine
                .live_captures
                .lock()
                .unwrap()
                .claim_native_transfer(&proof)
                .unwrap()
                .commit_with(|_, _| {
                    Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "authored current gate contention",
                    ))
                });
        assert!(failed.is_err());
        assert!(a_sub.current() && b_sub.current());
        assert_eq!(
            status(&x, &original.request_id).phase,
            BrowserAnalysisTransferPhase::ReadyForTransfer
        );
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .claim_native_transfer(&proof)
            .unwrap()
            .commit_with(|prior, next| {
                assert_eq!(prior.selection, proof.predecessor().clone());
                assert_eq!(next.selection, proof.successor().clone());
                Ok(())
            })
            .unwrap();
        assert!(!a_sub.current() && b_sub.current());
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .claim_native_transfer(&proof)
            .is_err());
        assert_eq!(
            a.ends.load(Ordering::Acquire) + b.ends.load(Ordering::Acquire),
            0
        );
        assert_eq!(a.retired.load(Ordering::Acquire), 1);
        assert_eq!(b.retired.load(Ordering::Acquire), 0);
        cap.cancel();
        assert!(b_sub.current());
    }

    #[test]
    fn boundary_refusal_preserves_open_predecessor_and_pending_successor_can_be_cancelled() {
        let x = delivery_fixture();
        let cap = successor(&x);
        let final_frame = prepare_final(&x);
        let mut changed = final_frame.clone();
        changed.capture_tick.0 += 1;
        assert!(x
            .f
            .engine
            .handle(transfer_request(&x, &cap, changed))
            .result
            .is_err());
        assert!(!cap.transfer_started());
        assert!(x.status().active);
        assert!(x
            .f
            .engine
            .live_bulk_ready(&live_handshake(&x.f, &x.capture), None)
            .is_ok());
        for epoch in [
            x.capture.identity.epoch.clone(),
            cap.successor().identity.epoch.clone(),
        ] {
            if epoch == x.capture.identity.epoch {
                continue;
            }
            assert!(x
                .f
                .engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::CloseBrowserAnalysisInput {
                        epoch,
                        final_frame: None
                    }
                ))
                .result
                .is_err());
        }
        cap.cancel();
        assert!(cap.cancelled_worker_joined()); // Authored fixture launched no worker.
        cap.join_cancelled(Instant::now() + Duration::from_secs(1))
            .unwrap();
        assert!(x.status().active);
        assert!(!report(&x, &cap.successor().identity.epoch).active);
        upload(&x, &x.capture, 2, 300);
    }

    #[test]
    fn retired_original_transfer_cannot_commit_or_reopen_either_input() {
        let x = delivery_fixture();
        let a = TransferSink::new(&x);
        let b = TransferSink::new(&x);
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .register_native_delivery(x.selection(), a.clone())
            .unwrap();
        let cap = successor(&x);
        x.f.engine
            .live_captures
            .lock()
            .unwrap()
            .register_native_successor_delivery(&cap, b.clone())
            .unwrap();
        let boundary = prepare_final(&x);
        let original = transfer_request(&x, &cap, boundary);
        x.f.engine.handle(original.clone()).result.unwrap();
        let seeded = report(&x, &cap.successor().identity.epoch);
        upload(&x, &seeded, 1, 300);
        complete_successor(&x, &cap);
        join(&x);
        let proof = a.transfers.lock().unwrap()[0].clone();
        let deadline = proof.expires_at_ns();
        cap.cancel();
        assert!(x
            .f
            .engine
            .live_captures
            .lock()
            .unwrap()
            .claim_native_transfer(&proof)
            .is_err());
        assert_eq!(
            status(&x, &original.request_id).phase,
            BrowserAnalysisTransferPhase::Aborted
        );
        assert_eq!(
            transfer_report(x.f.engine.handle(original).result.unwrap()).phase,
            BrowserAnalysisTransferPhase::Aborted
        );
        assert_eq!(proof.expires_at_ns(), deadline);
        assert!(!x.status().active);
        assert!(!report(&x, &cap.successor().identity.epoch).active);
        assert_eq!(a.transfers.lock().unwrap().len(), 1);
    }
}
