// Included in hsp_preparation_tests. Uses its explicitly simulated decision and
// real localhost HSP owner; no physical device acceptance is produced.
mod continuity_clock_tests {
    use super::*;
    use handy_hsp_preparation::{HspClockAdmission, HspContinuityClockOwner, HspPreparationIo};
    struct RefreshIo {
        inner: Arc<RealPreparationIo>,
        reads: AtomicU64,
        hold: Arc<Hold>,
    }
    struct ReleaseSetupReply<'a>(&'a Peer);
    impl Drop for ReleaseSetupReply<'_> {
        fn drop(&mut self) {
            self.0.release();
        }
    }
    impl HspPreparationIo for RefreshIo {
        fn now(&self) -> PResult<u64> {
            self.inner.now()
        }
        fn inspect(
            &self,
            c: Arc<crate::handy_credentials::Credentials>,
            d: Instant,
            t: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.inner.inspect(c, d, t)
        }
        fn synchronize(
            &self,
            c: Arc<crate::handy_credentials::Credentials>,
            d: Instant,
            t: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            self.inner.synchronize(c, d, t)
        }
        fn server_sample(
            &self,
            d: Instant,
            t: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            self.inner.server_sample(d, t)
        }
        fn hsp_origin(&self) -> Option<String> {
            self.inner.hsp_origin()
        }
        fn inspect_hsp_stream(
            &self,
            c: Arc<crate::handy_credentials::Credentials>,
            d: Instant,
            t: &mut dyn FnMut() -> PResult<u64>,
            stream: u32,
        ) -> PResult<crate::handy_online::PlayingTimingRead> {
            self.reads.fetch_add(1, Ordering::AcqRel);
            self.hold.enter()?;
            let started = t()?;
            let read = self.inner.inspect(c, d, t)?;
            assert_eq!(read.before.remote_hsp.as_ref().unwrap().stream_id, stream);
            Ok(crate::handy_online::PlayingTimingRead {
                device: read,
                started_boottime_ns: started,
                finished_boottime_ns: t()?,
            })
        }
    }
    fn enable(x: &PreparationFixture, child: &RequestId, id: &str, duration: u32) -> Request {
        let mut request = request(
            &x.f.session,
            Some(&x.f.project),
            Some(0),
            Command::EnableHandyHspContinuity {
                stream_request_id: child.clone(),
                duration_ms: duration,
                allow_online_observations: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        request.request_id = RequestId::new(id).unwrap();
        request
    }
    fn admit(
        x: &PreparationFixture,
        prep: &RequestId,
        binding: &HspBinding,
        request: &Request,
    ) -> PResult<Arc<HspContinuityClockOwner>> {
        let at = crate::handy_clock::boottime_ns()?;
        let instant = Instant::now();
        let db = x.f.engine.db.lock().unwrap();
        x.f.engine.admit_handy_hsp_continuity_clock_locked(
            &db,
            HspClockAdmission {
                request,
                preparation_request_id: prep,
                binding,
                admitted_at_ns: at,
                admitted_at: instant,
            },
        )
    }
    fn handoff(
        x: &PreparationFixture,
        prep: &RequestId,
        child: &RequestId,
        binding: &HspBinding,
        source: Arc<PreparedSourceAuthority>,
    ) -> crate::handy_hsp_execution::HspSessionHandle {
        x.f.engine
            .handoff_handy_hsp_preparation(
                &x.f.session,
                &x.f.project,
                prep,
                handy_hsp_preparation::HspSourceHandoff {
                    child_request: child.clone(),
                    allow_online_control: true,
                    binding: binding.clone(),
                    authority: source,
                },
            )
            .unwrap()
    }
    #[test]
    fn engine_continuity_admission_requires_consumed_post_setup_child_and_preserves_original_enable(
    ) {
        let (x, peer, base) = real_handoff_fixture();
        let io = Arc::new(RefreshIo {
            inner: base.clone(),
            reads: AtomicU64::new(0),
            hold: Arc::new(Hold::default()),
        });
        let prep = x.prepare("clock-owner-prepare");
        x.call(&prep, io.clone()).unwrap();
        x.ready(&prep.request_id);
        let (binding, source) = prepared_source(&x, &prep.request_id, Arc::new(Hold::default()));
        let child = RequestId::new("clock-owner-stream").unwrap();
        let request = enable(&x, &child, "clock-owner-enable", 3000);
        assert!(admit(&x, &prep.request_id, &binding, &request).is_err());
        // Hold the actual response after claim guards have been released. A
        // preclaim callback owns DB/session guards and cannot be held while
        // this synchronous admission request acquires those same guards.
        peer.hold_next("/hsp/setup");
        let _release = ReleaseSetupReply(&peer);
        let session = handoff(&x, &prep.request_id, &child, &binding, source.clone());
        peer.wait_held();
        assert!(x.f.engine.db.try_lock().is_ok());
        assert!(x.f.engine.handy_sessions.try_lock().is_ok());
        assert!(admit(&x, &prep.request_id, &binding, &request).is_err());
        assert_eq!(peer.puts(), ["/hsp/setup"]);
        peer.release();
        wait_child(&x, &child, |s| {
            s.post_setup_bound && !s.native_active && !s.metadata_pending
        });
        let excessive = enable(&x, &child, "clock-owner-excessive", u32::MAX);
        assert!(admit(&x, &prep.request_id, &binding, &excessive).is_err());
        let owner = admit(&x, &prep.request_id, &binding, &request).unwrap();
        let history = owner.history().clone();
        let original = owner
            .snapshot(crate::handy_clock::boottime_ns().unwrap())
            .unwrap();
        assert_eq!(original.generation(), 1);
        let replay = admit(&x, &prep.request_id, &binding, &request).unwrap();
        assert!(Arc::ptr_eq(&owner, &replay));
        assert_eq!(replay.history(), &history);
        let mut changed = request.clone();
        if let Command::EnableHandyHspContinuity { duration_ms, .. } = &mut changed.command {
            *duration_ms += 1;
        }
        assert_eq!(
            admit(&x, &prep.request_id, &binding, &changed)
                .err()
                .unwrap()
                .code,
            ErrorCode::RequestConflict
        );
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            owner.poll().unwrap();
            if owner
                .status(crate::handy_clock::boottime_ns().unwrap())
                .generation
                >= 2
            {
                break;
            }
            assert!(Instant::now() < end, "refresh did not publish");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            original.valid_until_ns(),
            history.initial_clock_valid_until_boottime_ns
        );
        assert!(original.at(original.valid_until_ns()).is_err());
        assert_eq!(
            base.syncs.load(Ordering::Acquire),
            1,
            "refresh must never resynchronize"
        );
        assert!(io.reads.load(Ordering::Acquire) >= 1);
        owner.request_stop();
        owner.join().unwrap();
        assert!(owner.is_joined());
        assert!(owner
            .snapshot(crate::handy_clock::boottime_ns().unwrap())
            .is_err());
        session.cancel();
        x.f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn engine_continuity_selected_clock_keeps_original_pin_across_publication_and_expiry() {
        let (x, _peer, base) = real_handoff_fixture();
        let io = Arc::new(RefreshIo {
            inner: base.clone(),
            reads: AtomicU64::new(0),
            hold: Arc::new(Hold::default()),
        });
        let prep = x.prepare("clock-selected-prepare");
        x.call(&prep, io).unwrap();
        x.ready(&prep.request_id);
        let (binding, source) = prepared_source(&x, &prep.request_id, Arc::new(Hold::default()));
        let child = RequestId::new("clock-selected-stream").unwrap();
        let session = handoff(&x, &prep.request_id, &child, &binding, source);
        wait_child(&x, &child, |s| {
            s.post_setup_bound && !s.native_active && !s.metadata_pending
        });
        let owner = admit(
            &x,
            &prep.request_id,
            &binding,
            // This distinct session ceiling outlives the preparation's five
            // seconds, so replacement cannot hide the selected pin's expiry.
            &enable(&x, &child, "clock-selected-enable", 8000),
        )
        .unwrap();
        let original = owner
            .snapshot(crate::handy_clock::boottime_ns().unwrap())
            .unwrap();
        let selected = owner.select_read();
        let mut calls = 0;
        let mut clock = || {
            calls += 1;
            let end = Instant::now() + Duration::from_secs(3);
            loop {
                owner.poll().unwrap();
                if owner
                    .status(crate::handy_clock::boottime_ns().unwrap())
                    .generation
                    >= 2
                {
                    break;
                }
                assert!(
                    Instant::now() < end,
                    "selected-clock fixture refresh timed out"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            crate::handy_clock::boottime_ns().unwrap()
        };
        let sampled_at = clock();
        assert_eq!(calls, 1);
        assert_eq!(selected.evaluate_at(sampled_at).unwrap(), original);
        let replacement = owner.snapshot(sampled_at).unwrap();
        assert_eq!(replacement.generation(), 2);
        assert_ne!(replacement, original);
        assert!(owner
            .snapshot(replacement.native().server.clock.monotonic_ns - 1)
            .is_err());
        assert!(selected.evaluate_at(original.valid_until_ns()).is_err());
        assert!(replacement.at(original.valid_until_ns()).is_ok());
        assert_eq!(
            original.valid_until_ns(),
            owner.history().initial_clock_valid_until_boottime_ns
        );
        assert_eq!(base.syncs.load(Ordering::Acquire), 1);
        owner.request_stop();
        assert!(selected.evaluate_at(sampled_at).is_err());
        owner.join().unwrap();
        session.cancel();
        x.f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn engine_continuity_cancel_during_held_read_requires_actual_join_and_cannot_publish_late() {
        let (x, _peer, base) = real_handoff_fixture();
        let hold = Arc::new(Hold::default());
        let _release = ReleaseHold(hold.clone());
        let io = Arc::new(RefreshIo {
            inner: base.clone(),
            reads: AtomicU64::new(0),
            hold: hold.clone(),
        });
        let prep = x.prepare("clock-held-prepare");
        x.call(&prep, io.clone()).unwrap();
        x.ready(&prep.request_id);
        let (binding, source) = prepared_source(&x, &prep.request_id, Arc::new(Hold::default()));
        let child = RequestId::new("clock-held-stream").unwrap();
        let session = handoff(&x, &prep.request_id, &child, &binding, source.clone());
        wait_child(&x, &child, |s| {
            s.post_setup_bound && !s.native_active && !s.metadata_pending
        });
        let owner = admit(
            &x,
            &prep.request_id,
            &binding,
            &enable(&x, &child, "clock-held-enable", 3000),
        )
        .unwrap();
        hold.arm();
        let end = Instant::now() + Duration::from_secs(3);
        while !owner
            .status(crate::handy_clock::boottime_ns().unwrap())
            .worker_active
        {
            owner.poll().unwrap();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
        }
        hold.wait_entered();
        owner.request_stop();
        assert!(!owner.is_joined());
        let pending = owner.status(crate::handy_clock::boottime_ns().unwrap());
        assert!(pending.retired && !pending.current && !pending.worker_joined);
        let reads = io.reads.load(Ordering::Acquire);
        assert_eq!(reads, 1);
        assert!(owner.poll().is_err());
        assert!(!owner.is_joined(), "held worker cannot be reported joined");
        hold.release();
        let deadline = Instant::now() + WAIT;
        loop {
            assert!(
                owner.poll().is_err(),
                "retired poll retains its original refusal"
            );
            if owner.is_joined() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "ordinary retired polling did not join the completed read"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(owner.is_joined());
        let stopped = owner.status(crate::handy_clock::boottime_ns().unwrap());
        assert_eq!(stopped.generation, 1);
        assert!(stopped.worker_joined && !stopped.current);
        assert_eq!(base.syncs.load(Ordering::Acquire), 1);
        assert_eq!(
            io.reads.load(Ordering::Acquire),
            reads,
            "retired polling started another read"
        );
        session.cancel();
        x.f.engine.finish_shutdown().unwrap();
    }
}
