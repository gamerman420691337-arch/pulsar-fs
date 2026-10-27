// Authored clock/source qualification and localhost transport exercise native
// ownership only. These cases do not qualify hardware, timing or physical Stop.
mod preclaim_acquisition_tests {
    use super::*;
    use crate::authority::handy_hsp_preparation::{HspPreclaimTestHold, HspPreclaimTestSite};
    use crate::handy_hsp_records::HspEffectRecord;

    struct Pending {
        x: PreparationFixture,
        peer: Peer,
        io: Arc<RealPreparationIo>,
        original: Request,
        child: RequestId,
        session: HspSessionHandle,
        execution: HspExecutionHandle,
        hold: HspPreclaimTestHold,
        effect_id: String,
        effect: HspEffectRecord,
        stream: HspStreamRecord,
        preflight_requests: usize,
    }
    impl Pending {
        fn new(name: &str) -> Self {
            Self::with_source(name, |source| source)
        }
        fn with_source(
            name: &str,
            transform: impl FnOnce(Arc<PreparedSourceAuthority>) -> Arc<dyn HspNativeAuthority>,
        ) -> Self {
            let (x, peer, io) = real_handoff_fixture();
            let original = x.prepare(name);
            x.call(&original, io.clone()).unwrap();
            x.ready(&original.request_id);
            let hold =
                x.f.engine
                    .hold_next_handy_hsp_preclaim_for_test(&x.f.session, &original.request_id)
                    .unwrap();
            let (binding, authority) =
                prepared_source(&x, &original.request_id, Arc::new(Hold::default()));
            let authority = transform(authority);
            let child = RequestId::new(format!("{name}-child")).unwrap();
            let session =
                x.f.engine
                    .handoff_handy_hsp_preparation(
                        &x.f.session,
                        &x.f.project,
                        &original.request_id,
                        handy_hsp_preparation::HspSourceHandoff {
                            child_request: child.clone(),
                            allow_online_control: true,
                            binding,
                            authority,
                        },
                    )
                    .unwrap();
            let execution = x.f.engine.ensure_handy_hsp_execution().unwrap();
            assert!(hold.wait_entered(WAIT));
            let (effect_id, effect, stream) = {
                let journal = x.f.engine._device_effect_journal.lock().unwrap();
                let parent = journal
                    .hsp_preparation_history(&x.f.session, &original.request_id)
                    .unwrap()
                    .unwrap();
                assert!(!parent.custody_transferred);
                let stream = journal
                    .hsp_stream(parent.linked_stream_id.as_deref().unwrap())
                    .unwrap();
                assert_eq!(stream.record.request_id, child);
                assert_eq!(stream.effect_ids.len(), 1);
                let effect_id = stream.effect_ids[0].clone();
                let effect = journal.hsp_effect(&effect_id).unwrap();
                assert_eq!(effect.state, HspJournalState::Prepared);
                (effect_id, effect.record, stream.record)
            };
            assert!(peer.puts().is_empty());
            let preflight_requests = peer.state.lock().unwrap().requests.len();
            Self {
                x,
                peer,
                io,
                original,
                child,
                session,
                execution,
                hold,
                effect_id,
                effect,
                stream,
                preflight_requests,
            }
        }
        fn wait(&self, predicate: impl Fn(&HspSessionSnapshot) -> bool) -> HspSessionSnapshot {
            let until = Instant::now() + WAIT;
            loop {
                match self.execution.snapshot_for_session(
                    &self.x.f.session,
                    &self.x.f.project,
                    &self.child,
                ) {
                    Ok(snapshot) => {
                        if predicate(&snapshot) {
                            return snapshot;
                        }
                        assert!(
                            Instant::now() < until,
                            "original preclaim did not settle: {snapshot:?}; waits={:?}",
                            self.hold.counts()
                        );
                    }
                    Err(HspExecutionError::Busy) if Instant::now() < until => {}
                    other => panic!("original preclaim status failed: {other:?}"),
                }
                thread::park_timeout(Duration::from_millis(2));
            }
        }
        fn unchanged(&self, state: HspJournalState) {
            let journal = self.x.f.engine._device_effect_journal.lock().unwrap();
            let effect = journal.hsp_effect(&self.effect_id).unwrap();
            assert_eq!(effect.record, self.effect);
            assert_eq!(effect.state, state);
            assert_eq!(
                journal
                    .hsp_stream(&self.stream.id().unwrap())
                    .unwrap()
                    .record,
                self.stream
            );
        }
        fn waiting(&self, site: HspPreclaimTestSite) {
            assert!(self.hold.wait_contended(site, WAIT));
            assert_eq!(self.hold.claimed(), 0);
            assert!(self.peer.puts().is_empty());
            assert_eq!(
                self.peer.state.lock().unwrap().requests.len(),
                self.preflight_requests,
                "contention cannot repeat native preflight"
            );
        }
        fn completed(&self) {
            self.wait(|s| s.post_setup_bound && !s.native_active && !s.metadata_pending);
            assert!(!self.hold.timed_out());
            assert_eq!(self.hold.claimed(), 1);
            assert_eq!(self.peer.puts(), ["/hsp/setup"]);
            self.unchanged(HspJournalState::Acknowledged);
            let requests = self.peer.state.lock().unwrap().requests.clone();
            let setup = requests
                .iter()
                .find(|r| r.0 == "PUT" && r.1 == "/hsp/setup")
                .unwrap();
            assert_eq!(Some(&setup.2), self.effect.request.body.as_ref());
            let parent = self.x.status(&self.original.request_id).history.unwrap();
            assert!(parent.custody_transferred);
            assert_eq!(
                parent.linked_stream_id.as_ref().map(ArtifactId::as_str),
                Some(self.stream.id().unwrap().as_str())
            );
            assert_eq!(self.io.syncs.load(Ordering::Acquire), 1);
            self.session.cancel();
            self.wait(|s| {
                s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
                    && !s.native_active
                    && !s.metadata_pending
            });
            assert_eq!(self.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
            self.x.f.engine.finish_shutdown().unwrap();
        }
        fn unclaimed(&self) {
            self.wait(|s| {
                !s.native_active
                    && !s.metadata_pending
                    && matches!(
                        s.runtime_state,
                        crate::handy_hsp_runtime::HspRuntimeState::SetupUnsent
                            | crate::handy_hsp_runtime::HspRuntimeState::SetupCancelled
                    )
            });
            assert_eq!(self.hold.claimed(), 0);
            assert!(self.peer.puts().is_empty());
            self.unchanged(HspJournalState::Cancelled);
            assert_eq!(self.io.syncs.load(Ordering::Acquire), 1);
        }
    }

    #[test]
    fn original_setup_waits_for_db_then_claims_once_without_repeating_preflight() {
        let p = Pending::new("preclaim-db");
        let db = p.x.f.engine.db.lock().unwrap();
        p.hold.release();
        p.waiting(HspPreclaimTestSite::Db);
        assert!(p.x.f.engine.handy_sessions.try_lock().is_ok());
        assert!(p.x.f.engine.live_captures.try_lock().is_ok());
        assert!(p.x.f.engine.browser_timing.try_lock().is_ok());
        let runtime = p.session.runtime_for_test();
        let until = Instant::now() + Duration::from_millis(50);
        loop {
            if let Ok(guard) = runtime.try_lock() {
                drop(guard);
                break;
            }
            assert!(Instant::now() < until, "runtime held across DB wait");
            thread::yield_now();
        }
        p.unchanged(HspJournalState::Prepared);
        drop(db);
        p.completed();
    }

    #[test]
    fn original_setup_session_contention_releases_db_before_waiting() {
        let p = Pending::new("preclaim-sessions");
        let sessions = p.x.f.engine.handy_sessions.lock().unwrap();
        p.hold.release();
        p.waiting(HspPreclaimTestSite::Sessions);
        // A wait notification may race the next bounded acquisition. Eventually
        // acquiring DB while sessions remain held proves no nested wait holds it.
        let until = Instant::now() + Duration::from_millis(50);
        loop {
            if let Ok(db) = p.x.f.engine.db.try_lock() {
                drop(db);
                break;
            }
            assert!(Instant::now() < until, "DB held across session wait");
            thread::yield_now();
        }
        assert!(p.x.f.engine.live_captures.try_lock().is_ok());
        assert!(p.x.f.engine.browser_timing.try_lock().is_ok());
        p.unchanged(HspJournalState::Prepared);
        drop(sessions);
        p.completed();
    }

    #[test]
    fn original_setup_cancel_interrupts_db_wait_before_lock_release() {
        let p = Pending::new("preclaim-cancel");
        let db = p.x.f.engine.db.lock().unwrap();
        p.hold.release();
        p.waiting(HspPreclaimTestSite::Db);
        assert!(p.hold.cancel_original_for_test().unwrap());
        assert!(!p.hold.cancel_original_for_test().unwrap());
        p.unclaimed();
        drop(db);
        assert!(!p.hold.timed_out());
        p.x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn original_setup_shutdown_interrupts_sessions_wait_without_claiming() {
        let p = Pending::new("preclaim-shutdown");
        let sessions = p.x.f.engine.handy_sessions.lock().unwrap();
        p.hold.release();
        p.waiting(HspPreclaimTestSite::Sessions);
        p.x.f.engine.request_shutdown();
        p.unclaimed();
        drop(sessions);
        assert!(!p.hold.timed_out());
        p.x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn original_setup_poison_is_terminal_without_contention_retry() {
        for db_lock in [true, false] {
            let p = Pending::new(if db_lock {
                "preclaim-poison-db"
            } else {
                "preclaim-poison-sessions"
            });
            let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if db_lock {
                    let _guard = p.x.f.engine.db.lock().unwrap();
                    panic!("authored DB mutex poison without data mutation");
                } else {
                    let _guard = p.x.f.engine.handy_sessions.lock().unwrap();
                    panic!("authored session mutex poison without data mutation");
                }
            }));
            assert!(poisoned.is_err());
            p.hold.release();
            p.unclaimed();
            assert_eq!(p.hold.counts(), [0, 0, 0]);
            assert!(!p.hold.timed_out());
            // Only fixture cleanup clears the deliberately poisoned, unmodified
            // mutex, after proving the production attempt terminal and unsent.
            if db_lock {
                p.x.f.engine.db.clear_poison();
            } else {
                p.x.f.engine.handy_sessions.clear_poison();
            }
            p.x.f.engine.finish_shutdown().unwrap();
        }
    }

    struct LegacyBusy {
        source: Arc<PreparedSourceAuthority>,
        calls: Arc<AtomicU64>,
    }
    impl HspNativeAuthority for LegacyBusy {
        fn current_facts(&self, now: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            self.source.current_facts(now)
        }
        fn authorize(
            &self,
            _: Instant,
            _: &AtomicBool,
            _: HspClaimGate<'_>,
            _: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Err(HspDispatchFailure::Busy)
        }
    }

    #[test]
    fn legacy_busy_error_never_returns_preclaim_retry_custody() {
        let calls = Arc::new(AtomicU64::new(0));
        let observed = calls.clone();
        let p = Pending::with_source("preclaim-legacy-busy", move |source| {
            Arc::new(LegacyBusy {
                source,
                calls: observed,
            })
        });
        p.hold.release();
        p.unclaimed();
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(p.hold.counts(), [0, 0, 0]);
        assert!(!p.hold.timed_out());
        p.x.f.engine.finish_shutdown().unwrap();
    }
}
