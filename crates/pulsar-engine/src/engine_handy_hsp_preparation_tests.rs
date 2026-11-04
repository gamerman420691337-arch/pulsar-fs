// Public preparation tests use authored device observations, clocks and an
// explicitly simulated qualification decision. They do not qualify a real
// Handy, physical synchronization/stopping, or human acceptance.
#[cfg(target_os = "linux")]
mod hsp_preparation_tests {
    use super::*;
    use std::{
        sync::{atomic::AtomicU64, Condvar},
        time::{Duration, Instant},
    };

    const WAIT: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct HoldState {
        entered: bool,
        released: bool,
        timed_out: bool,
    }
    #[derive(Default)]
    struct Hold {
        enabled: AtomicBool,
        state: Mutex<HoldState>,
        changed: Condvar,
    }
    impl Hold {
        fn arm(&self) {
            self.enabled.store(true, Ordering::Release);
        }
        fn enter(&self) -> PResult<()> {
            if !self.enabled.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut state = self.state.lock().unwrap();
            state.entered = true;
            self.changed.notify_all();
            let (mut state, _) = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(2), |s| !s.released)
                .unwrap();
            if !state.released {
                state.timed_out = true;
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "bounded preparation fixture hook expired",
                ));
            }
            Ok(())
        }
        fn wait_entered(&self) {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .changed
                .wait_timeout_while(state, WAIT, |s| !s.entered)
                .unwrap();
            assert!(
                state.entered && !state.timed_out,
                "preparation worker did not reach the original bounded hook"
            );
        }
        fn assert_not_timed_out(&self) {
            assert!(
                !self.state.lock().unwrap().timed_out,
                "fixture hook timed out instead of the requested causal release"
            );
        }
        fn release(&self) {
            self.state.lock().unwrap().released = true;
            self.changed.notify_all();
        }
    }
    struct ReleaseHold(Arc<Hold>);
    impl Drop for ReleaseHold {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    struct AuthoredIo {
        tick: AtomicU64,
        inspections: AtomicU64,
        syncs: AtomicU64,
        servers: AtomicU64,
        lose_sync: AtomicBool,
        before_claim: Arc<Hold>,
        after_claim: Arc<Hold>,
    }
    impl AuthoredIo {
        fn new() -> Self {
            Self {
                tick: AtomicU64::new(1_000_000_000),
                inspections: AtomicU64::new(0),
                syncs: AtomicU64::new(0),
                servers: AtomicU64::new(0),
                lose_sync: AtomicBool::new(false),
                before_claim: Arc::new(Hold::default()),
                after_claim: Arc::new(Hold::default()),
            }
        }
        fn count(&self) -> (u64, u64, u64) {
            (
                self.inspections.load(Ordering::Acquire),
                self.syncs.load(Ordering::Acquire),
                self.servers.load(Ordering::Acquire),
            )
        }
        fn now_value(&self) -> PResult<u64> {
            self.tick
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
                .map_err(|_| ProtocolError::new(ErrorCode::Unavailable, "authored clock exhausted"))
        }
        fn info(at_ns: u64) -> HandyDeviceTimeInfo {
            HandyDeviceTimeInfo {
                device_uptime_ms: at_ns / 1_000_000,
                reported_clock_offset_ms: 1_000_000,
                last_sync_round_trip_ms: 0,
            }
        }
    }
    impl handy_hsp_preparation::HspPreparationIo for AuthoredIo {
        fn now(&self) -> PResult<u64> {
            self.now_value()
        }
        fn inspect(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.inspections.fetch_add(1, Ordering::AcqRel);
            let mut sample = || -> PResult<HandyDeviceTimingSample> {
                let started = clock()?;
                Ok(HandyDeviceTimingSample {
                    info: Self::info(started),
                    user_offset: HandyUserTimeOffset { offset_ms: 0 },
                    read_started_boottime_ns: started,
                    read_finished_boottime_ns: clock()?,
                })
            };
            Ok(crate::handy_online::DeviceTimingRead {
                before: hsp_qualification_sample(),
                first: sample()?,
                second: sample()?,
                after: hsp_qualification_sample(),
            })
        }
        fn synchronize(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            self.syncs.fetch_add(1, Ordering::AcqRel);
            if self.lose_sync.load(Ordering::Acquire) {
                return crate::device_effect_journal::ClockSyncCompletion::Unknown;
            }
            let sent = clock().unwrap();
            crate::device_effect_journal::ClockSyncCompletion::Acknowledged(
                HandyClockSyncAcknowledgment {
                    info: Self::info(sent),
                    sent_boottime_ns: sent,
                    received_boottime_ns: clock().unwrap(),
                },
            )
        }
        fn server_sample(
            &self,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            self.servers.fetch_add(1, Ordering::AcqRel);
            let sent_ns = clock()?;
            Ok(crate::handy_server_clock::Sample {
                sent_ns,
                received_ns: clock()?,
                server_ms: 1_000_000 + sent_ns / 1_000_000,
            })
        }
        fn before_claim(&self) -> PResult<()> {
            self.before_claim.enter()
        }
        fn after_claim(&self) -> PResult<()> {
            self.after_claim.enter()
        }
    }

    struct PreparationFixture {
        f: Fixture,
        accepted: HandyQualificationReport,
        connection: HandyConnectionSession,
    }
    impl PreparationFixture {
        fn new() -> Self {
            Self::with_subject(|_| {})
        }
        fn with_subject(change: impl FnOnce(&mut HandyQualificationSubject)) -> Self {
            let f = fixture();
            let (path, mut input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
            change(&mut input.subject);
            input.validate().unwrap();
            let imported = qualification_saved(
                f.engine
                    .handle(qualification_import(
                        &f,
                        "preparation-fixture-import",
                        &path,
                        &input,
                    ))
                    .result
                    .unwrap(),
            );
            let (_, connection) = hsp_qualification_connection(&f);
            let accepted = qualification_saved(
                qualification_call(
                    &f,
                    &qualification_accept(&f, &imported.dossier_id, &connection),
                    |_| Ok(hsp_qualification_sample()),
                )
                .unwrap(),
            );
            Self {
                f,
                accepted,
                connection,
            }
        }
        fn prepare(&self, id: &str) -> Request {
            let mut r = request(
                &self.f.session,
                Some(&self.f.project),
                Some(0),
                Command::PrepareHandyHsp {
                    engine_epoch: self.connection.engine_epoch.clone(),
                    device_session: self.connection.device_session.clone(),
                    qualification: HandyQualificationBinding {
                        dossier_id: self.accepted.dossier_id.clone(),
                        revision: self.accepted.revision,
                    },
                    timeout_ms: 5_000,
                    allow_online_synchronization: true,
                    owner_proof: self.f.engine.handy_owner_token.clone(),
                },
            );
            r.request_id = RequestId::new(id).unwrap();
            r
        }
        fn call(
            &self,
            original: &Request,
            io: Arc<dyn handy_hsp_preparation::HspPreparationIo>,
        ) -> PResult<HandyHspPreparationStatus> {
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(request_fingerprint(original).unwrap())
            );
            self.f
                .engine
                .handy_hsp_preparation_with_io(original, &self.f.session, &fingerprint, io)
                .map(report)
        }
        fn ready(&self, original: &RequestId) -> HandyHspPreparationStatus {
            self.wait(
                original,
                "original preparation must reach native readiness",
                |s| {
                    s.native.as_ref().is_some_and(|n| {
                        n.phase == HandyHspPreparationPhase::Ready
                            && !n.worker_active
                            && !n.metadata_pending
                    })
                },
            )
        }
        fn retired(&self, original: &RequestId) -> HandyHspPreparationStatus {
            self.wait(
                original,
                "original preparation worker must join after retirement",
                |s| {
                    s.native.as_ref().is_some_and(|n| {
                        n.phase == HandyHspPreparationPhase::Retired
                            && !n.worker_active
                            && !n.metadata_pending
                    })
                },
            )
        }
        fn retired_with_cancel_retry(
            &self,
            original: &RequestId,
            cancellation: &Request,
        ) -> HandyHspPreparationStatus {
            let deadline = Instant::now() + WAIT;
            loop {
                let status = self.status(original);
                let native = status.native.as_ref().unwrap();
                assert_eq!(native.phase, HandyHspPreparationPhase::Retired);
                if !native.worker_active {
                    if !native.metadata_pending {
                        return status;
                    }
                    // Status is read-only. A failed nonblocking journal write
                    // needs the same explicit cancellation's metadata retry.
                    report(self.f.engine.handle(cancellation.clone()).result.unwrap());
                }
                assert!(
                    Instant::now() < deadline,
                    "cancel retry did not settle: {status:?}"
                );
                std::thread::park_timeout(Duration::from_millis(2));
            }
        }
        fn status_request(&self, original: &RequestId) -> Request {
            request(
                &self.f.session,
                Some(&self.f.project),
                None,
                Command::HandyHspPreparationStatus {
                    preparation_request_id: original.clone(),
                    owner_proof: self.f.engine.handy_owner_token.clone(),
                },
            )
        }
        fn cancel_request(&self, original: &RequestId, id: &str) -> Request {
            let mut r = request(
                &self.f.session,
                Some(&self.f.project),
                None,
                Command::CancelHandyHspPreparation {
                    preparation_request_id: original.clone(),
                    owner_proof: self.f.engine.handy_owner_token.clone(),
                },
            );
            r.request_id = RequestId::new(id).unwrap();
            r
        }
        fn status(&self, original: &RequestId) -> HandyHspPreparationStatus {
            report(
                self.f
                    .engine
                    .handle(self.status_request(original))
                    .result
                    .unwrap(),
            )
        }
        fn wait(
            &self,
            original: &RequestId,
            reason: &str,
            predicate: impl Fn(&HandyHspPreparationStatus) -> bool,
        ) -> HandyHspPreparationStatus {
            let deadline = Instant::now() + WAIT;
            loop {
                let status = self.status(original);
                if predicate(&status) {
                    return status;
                }
                assert!(Instant::now() < deadline, "{reason}: {status:?}");
                std::thread::park_timeout(Duration::from_millis(2));
            }
        }
    }
    struct RealPreparationIo {
        peer: Arc<Mutex<PeerState>>,
        origin: String,
        identity: HandyIdentity,
        syncs: AtomicU64,
    }
    impl handy_hsp_preparation::HspPreparationIo for RealPreparationIo {
        fn now(&self) -> PResult<u64> {
            crate::handy_clock::boottime_ns()
        }
        fn inspect(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            let before = authored_stopped_hsp_observation(&self.peer, &self.identity);
            let mut sample = || -> PResult<HandyDeviceTimingSample> {
                let sent = clock()?;
                Ok(HandyDeviceTimingSample {
                    info: AuthoredIo::info(sent),
                    user_offset: HandyUserTimeOffset { offset_ms: 0 },
                    read_started_boottime_ns: sent,
                    read_finished_boottime_ns: clock()?,
                })
            };
            let first = sample()?;
            let second = sample()?;
            let after = authored_stopped_hsp_observation(&self.peer, &self.identity);
            Ok(crate::handy_online::DeviceTimingRead {
                before,
                first,
                second,
                after,
            })
        }
        fn synchronize(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            self.syncs.fetch_add(1, Ordering::AcqRel);
            let sent = clock().unwrap();
            crate::device_effect_journal::ClockSyncCompletion::Acknowledged(
                HandyClockSyncAcknowledgment {
                    info: AuthoredIo::info(sent),
                    sent_boottime_ns: sent,
                    received_boottime_ns: clock().unwrap(),
                },
            )
        }
        fn server_sample(
            &self,
            _deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            let sent_ns = clock()?;
            Ok(crate::handy_server_clock::Sample {
                sent_ns,
                received_ns: clock()?,
                server_ms: 1_000_000 + sent_ns / 1_000_000,
            })
        }
        fn hsp_origin(&self) -> Option<String> {
            Some(self.origin.clone())
        }
    }
    // Separately supplied authored source authority. The preparation supplies
    // actual native clock objects; neither history nor a public DTO grants it.
    struct PreparedSourceAuthority {
        clock: crate::handy_playback_clock::NativePlaybackClock,
        binding: HspBinding,
        source_start: u64,
        source_until: u64,
        before_claim: Arc<Hold>,
    }
    impl HspNativeAuthority for PreparedSourceAuthority {
        fn try_authorize_with_db<'a>(
            &self,
            _: &rusqlite::Connection,
            deadline: Instant,
            cancel: &AtomicBool,
            gate: HspClaimGate<'a>,
            owner: &HspNativeOwner,
        ) -> Result<crate::handy_hsp_dispatch::HspPreclaimProgress<'a>, HspDispatchFailure>
        {
            self.before_claim
                .enter()
                .map_err(|_| HspDispatchFailure::Cancelled)?;
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(HspDispatchFailure::Cancelled);
            }
            gate.try_apply(owner, |now| self.current_facts(now))
        }
        fn current_facts(&self, now: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            if now >= self.source_until {
                return Ok(None);
            }
            let clock = self
                .clock
                .at(now)
                .map_err(|_| HspDispatchFailure::InvalidClock)?;
            let elapsed_ms = now
                .checked_sub(self.source_start)
                .ok_or(HspDispatchFailure::InvalidClock)?
                / 1_000_000;
            let phase = -10_000
                + i64::try_from(elapsed_ms).map_err(|_| HspDispatchFailure::InvalidClock)?;
            let start = i32::try_from(phase).map_err(|_| HspDispatchFailure::InvalidClock)?;
            let phase_error = i64::try_from(
                self.clock
                    .device
                    .qualified
                    .maximum_total_error_ns
                    .div_ceil(1_000_000),
            )
            .map_err(|_| HspDispatchFailure::InvalidClock)?;
            let reference = HspPlayReference::new(
                now,
                HspPhaseWindow {
                    earliest_ms: phase
                        .checked_sub(phase_error)
                        .ok_or(HspDispatchFailure::InvalidClock)?,
                    latest_ms: phase
                        .checked_add(phase_error)
                        .ok_or(HspDispatchFailure::InvalidClock)?,
                },
                start,
                clock.estimated_server_ms,
            )
            .map_err(HspDispatchFailure::Authority)?;
            HspDispatchFacts::new(
                self.binding.clone(),
                reference,
                HspAuthorityWindow::new(
                    self.source_until,
                    self.source_until,
                    clock.valid_until_ns.min(self.source_until),
                ),
            )
            .map(Some)
            .map_err(HspDispatchFailure::Authority)
        }
        fn authorize(
            &self,
            deadline: Instant,
            cancel: &AtomicBool,
            gate: HspClaimGate<'_>,
            owner: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            self.before_claim
                .enter()
                .map_err(|_| HspDispatchFailure::Cancelled)?;
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(HspDispatchFailure::Cancelled);
            }
            gate.apply(owner, |now| self.current_facts(now))
        }
    }
    fn real_handoff_fixture() -> (PreparationFixture, Peer, Arc<RealPreparationIo>) {
        // A separate, explicitly authored real-clock contract for native owner
        // composition. The strict 1ms virtual-clock fixtures above are unchanged.
        let x = PreparationFixture::with_subject(|s| {
            s.profile.maximum_command_age_ns = 10_000_000_000;
            s.profile.maximum_clock_uncertainty_ns = 100_000_000;
            s.server_clock.as_mut().unwrap().timestamp_error_ns = 1_000_000;
            let device = s.device_clock.as_mut().unwrap();
            device.maximum_sync_age_ns = 10_000_000_000;
            device.device_error_ns = 1_000;
            device.maximum_sync_round_trip_ms = 10;
            let p = policy();
            s.hsp.as_mut().unwrap().policy = HandyHspPolicy {
                max_buffered_points: p.max_buffered_points as u32,
                device_capacity_points: p.device_capacity_points as u32,
                max_chunk_points: p.max_chunk_points as u32,
                min_lead_ms: p.min_lead_ms,
                max_future_ms: p.max_future_ms,
                max_sample_age_ns: p.max_sample_age_ns,
                max_gap_ms: p.max_gap_ms,
            };
        });
        let peer = Peer::new(false);
        peer.state.lock().unwrap().native_session =
            x.accepted.manifest.subject.identity.session_id.clone();
        let io = Arc::new(RealPreparationIo {
            peer: peer.state.clone(),
            origin: peer.origin.clone(),
            identity: x.accepted.manifest.subject.identity.clone(),
            syncs: AtomicU64::new(0),
        });
        (x, peer, io)
    }
    fn prepared_source(
        x: &PreparationFixture,
        prep: &RequestId,
        hold: Arc<Hold>,
    ) -> (HspBinding, Arc<PreparedSourceAuthority>) {
        let (clock, record) = x
            .f
            .engine
            .with_handy_hsp_readiness_for_test(&x.f.session, &x.f.project, prep, |clock, record| {
                (
                    crate::handy_playback_clock::NativePlaybackClock {
                        server: clock.server.clone(),
                        device: clock.device.clone(),
                    },
                    record.clone(),
                )
            })
            .unwrap();
        let binding = HspBinding::new(
            HspStreamIdentity {
                generation: 1,
                stream_id: 42,
                live: LiveIdentity {
                    epoch: AttemptId::new("prepared-child-live").unwrap(),
                    project: record.project_id.clone(),
                    revision: record.revision,
                    source: SourceVersionId::new("prepared-child-source").unwrap(),
                },
            },
            ClientId::new(x.f.session.as_str()).unwrap(),
            record.device_session,
            clock.server.clock.domain.clone(),
        );
        let source_start = crate::handy_clock::boottime_ns().unwrap();
        let source_until = record
            .deadline_boottime_ns
            .min(source_start + 4_000_000_000);
        let authority = Arc::new(PreparedSourceAuthority {
            clock,
            binding: binding.clone(),
            source_start,
            source_until,
            before_claim: hold,
        });
        (binding, authority)
    }
    fn wait_child(
        x: &PreparationFixture,
        child: &RequestId,
        predicate: impl Fn(&HspSessionSnapshot) -> bool,
    ) -> HspSessionSnapshot {
        let handle = x.f.engine.ensure_handy_hsp_execution().unwrap();
        let deadline = Instant::now() + WAIT;
        let mut last = None;
        loop {
            match handle.snapshot_for_session(&x.f.session, &x.f.project, child) {
                Ok(snapshot) => {
                    if predicate(&snapshot) {
                        return snapshot;
                    }
                    last = Some(snapshot);
                }
                Err(HspExecutionError::Busy) => {}
                Err(error) => panic!("child status: {error:?}"),
            }
            assert!(Instant::now() < deadline, "child did not settle: {last:?}");
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn report(body: ResponseBody) -> HandyHspPreparationStatus {
        let ResponseBody::HandyHspPreparation(status) = body else {
            panic!("HSP preparation status expected");
        };
        status.validate().unwrap();
        assert!(!status.armed && !status.playback_enabled);
        *status
    }

    #[test]
    fn engine_hsp_preparation_public_composition_syncs_once_and_exact_replay_cannot_rearm() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        let original = x.prepare("prepare-once");
        let first_tick = io.tick.load(Ordering::Acquire);
        x.call(&original, io.clone()).unwrap();
        let ready = x.ready(&original.request_id);
        let history = ready.history.as_ref().unwrap();
        assert_eq!(history.state, HandyClockSyncState::Acknowledged);
        assert!(history.acknowledgment.is_some());
        assert!(
            !history.custody_transferred && history.linked_stream_id.is_none() && !history.resolved
        );
        let original_capture = history
            .deadline_boottime_ns
            .checked_sub(5_000_000_000)
            .unwrap();
        assert!((first_tick..=io.tick.load(Ordering::Acquire)).contains(&original_capture));
        assert!(
            ready
                .native
                .as_ref()
                .unwrap()
                .clock_valid_until_boottime_ns
                .unwrap()
                <= history.deadline_boottime_ns
        );
        let counts = io.count();
        assert_eq!(counts.1, 1, "one original effectful synchronization");
        assert!(
            counts.0 >= 2 && counts.2 >= 1,
            "readiness requires independent pre/post device and server samples"
        );
        let unused = Arc::new(AuthoredIo::new());
        let repeated = x.call(&original, unused.clone()).unwrap();
        assert_eq!(repeated.history, ready.history);
        assert_eq!(unused.count(), (0, 0, 0));
        assert_eq!(io.count(), counts);
        let cancel = x.cancel_request(&original.request_id, "cancel-ready-preparation");
        report(x.f.engine.handle(cancel.clone()).result.unwrap());
        let retired = x.retired(&original.request_id);
        assert_eq!(
            retired.history, ready.history,
            "retirement cannot rewrite acknowledged synchronization"
        );
        let replay = x.call(&original, unused.clone()).unwrap();
        assert_eq!(replay.history, ready.history);
        assert_eq!(
            replay.native.as_ref().unwrap().phase,
            HandyHspPreparationPhase::Retired
        );
        report(x.f.engine.handle(cancel).result.unwrap());
        assert_eq!(io.count(), counts);
        assert_eq!(unused.count(), (0, 0, 0));
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_expired_original_clock_never_refreshes_on_status_or_replay() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        let original = x.prepare("prepare-clock-expiry");
        x.call(&original, io.clone()).unwrap();
        let ready = x.ready(&original.request_id);
        let until = ready
            .native
            .as_ref()
            .unwrap()
            .clock_valid_until_boottime_ns
            .unwrap();
        let counts = io.count();
        io.tick
            .store(until.checked_add(1).unwrap(), Ordering::Release);
        let retired = x.retired(&original.request_id);
        assert_eq!(retired.history, ready.history);
        let new_io = Arc::new(AuthoredIo::new());
        let replay = x.call(&original, new_io.clone()).unwrap();
        assert_eq!(replay.history, ready.history);
        assert_eq!(
            replay.native.unwrap().phase,
            HandyHspPreparationPhase::Retired
        );
        assert_eq!(io.count(), counts);
        assert_eq!(new_io.count(), (0, 0, 0));
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_cancel_before_claim_joins_and_cancels_only_original_unsent_record() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        io.before_claim.arm();
        let _release = ReleaseHold(io.before_claim.clone());
        let original = x.prepare("prepare-before-claim");
        x.call(&original, io.clone()).unwrap();
        io.before_claim.wait_entered();
        let prepared = x.status(&original.request_id);
        assert_eq!(
            prepared.history.as_ref().unwrap().state,
            HandyClockSyncState::Prepared
        );
        assert_eq!(io.syncs.load(Ordering::Acquire), 0);
        let cancellation = x.cancel_request(&original.request_id, "cancel-before-claim");
        report(x.f.engine.handle(cancellation.clone()).result.unwrap());
        io.before_claim.release();
        let retired = x.retired_with_cancel_retry(&original.request_id, &cancellation);
        let history = retired.history.unwrap();
        assert_eq!(history.record_id, prepared.history.unwrap().record_id);
        assert_eq!(history.state, HandyClockSyncState::Cancelled);
        assert!(history.acknowledgment.is_none() && !history.custody_transferred);
        assert_eq!(io.syncs.load(Ordering::Acquire), 0);
        let unused = Arc::new(AuthoredIo::new());
        assert_eq!(
            x.call(&original, unused.clone())
                .unwrap()
                .history
                .unwrap()
                .state,
            HandyClockSyncState::Cancelled
        );
        assert_eq!(unused.count(), (0, 0, 0));
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_busy_journal_keeps_status_read_only_until_exact_cancel_retry() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        io.before_claim.arm();
        let _release = ReleaseHold(io.before_claim.clone());
        let original = x.prepare("prepare-busy-cancel-metadata");
        x.call(&original, io.clone()).unwrap();
        io.before_claim.wait_entered();
        let prepared = x.status(&original.request_id);
        assert_eq!(
            prepared.history.as_ref().unwrap().state,
            HandyClockSyncState::Prepared
        );
        let cancellation = x.cancel_request(&original.request_id, "cancel-busy-metadata");
        report(x.f.engine.handle(cancellation.clone()).result.unwrap());
        let journal = x.f.engine._device_effect_journal.lock().unwrap();
        io.before_claim.assert_not_timed_out();
        io.before_claim.release();
        // This joins the actual worker while its original metadata write is
        // deliberately unavailable. It does not set the Engine shutdown flag.
        assert_eq!(
            x.f.engine
                .shutdown_handy_hsp_preparations()
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        io.before_claim.assert_not_timed_out();
        assert_eq!(
            journal
                .hsp_preparation_history(&x.f.session, &original.request_id)
                .unwrap()
                .unwrap()
                .state,
            HandyClockSyncState::Prepared
        );
        drop(journal);
        let calls = io.count();
        assert_eq!(calls, (1, 0, 0));
        for _ in 0..3 {
            let status = x.status(&original.request_id);
            assert_eq!(status.history, prepared.history);
            let native = status.native.unwrap();
            assert_eq!(native.phase, HandyHspPreparationPhase::Retired);
            assert!(!native.worker_active && native.metadata_pending);
        }
        let settled = x.retired_with_cancel_retry(&original.request_id, &cancellation);
        let history = settled.history.unwrap();
        assert_eq!(history.record_id, prepared.history.unwrap().record_id);
        assert_eq!(history.state, HandyClockSyncState::Cancelled);
        assert!(history.acknowledgment.is_none() && !history.custody_transferred);
        assert_eq!(
            io.count(),
            calls,
            "metadata retry must not acquire clocks or synchronize"
        );
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_cancel_after_claim_keeps_unknown_and_never_reissues_sync() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        io.after_claim.arm();
        io.lose_sync.store(true, Ordering::Release);
        let _release = ReleaseHold(io.after_claim.clone());
        let original = x.prepare("prepare-after-claim");
        x.call(&original, io.clone()).unwrap();
        io.after_claim.wait_entered();
        assert_eq!(
            x.status(&original.request_id).history.unwrap().state,
            HandyClockSyncState::Claimed
        );
        let cancellation = x.cancel_request(&original.request_id, "cancel-claimed-preparation");
        report(x.f.engine.handle(cancellation.clone()).result.unwrap());
        let calls_before_retirement = io.count();
        io.after_claim.release();
        let retired = x.retired_with_cancel_retry(&original.request_id, &cancellation);
        assert_eq!(
            retired.history.as_ref().unwrap().state,
            HandyClockSyncState::Unknown
        );
        assert!(retired.history.as_ref().unwrap().acknowledgment.is_none());
        let counts = io.count();
        assert_eq!(
            counts, calls_before_retirement,
            "cancellation metadata retry cannot reacquire clocks or synchronize"
        );
        assert!(
            counts.1 <= 1,
            "cancelled claimed synchronization cannot be retried"
        );
        let unused = Arc::new(AuthoredIo::new());
        assert_eq!(
            x.call(&original, unused.clone()).unwrap().history,
            retired.history
        );
        assert_eq!(unused.count(), (0, 0, 0));
        assert_eq!(io.count(), counts);
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_current_epoch_actor_revision_and_owner_are_required_before_io() {
        let x = PreparationFixture::new();
        for case in 0..5 {
            let io = Arc::new(AuthoredIo::new());
            let mut bad = x.prepare(&format!("invalid-prepare-{case}"));
            let Command::PrepareHandyHsp {
                engine_epoch,
                owner_proof,
                allow_online_synchronization,
                ..
            } = &mut bad.command
            else {
                unreachable!();
            };
            match case {
                0 => *engine_epoch = DeviceSessionId::new("retired-engine-epoch").unwrap(),
                1 => *owner_proof = "invalid-owner-proof".into(),
                2 => *allow_online_synchronization = false,
                3 => bad.expected_revision = Some(RevisionId::new(2)),
                _ => bad.auth_token = None,
            }
            let expected = match case {
                0 => ErrorCode::DependencyMismatch,
                1 | 2 => ErrorCode::Forbidden,
                3 => ErrorCode::RevisionConflict,
                _ => ErrorCode::Unauthorized,
            };
            assert_eq!(
                x.call(&bad, io.clone()).unwrap_err().code,
                expected,
                "case {case}"
            );
            assert_eq!(io.count(), (0, 0, 0), "case {case} entered device IO");
        }
        let stranger = pair(&x.f.engine, "preparation foreign actor");
        x.f.engine
            .handle(request(
                &x.f.session,
                None,
                None,
                Command::Grant {
                    session: stranger.clone(),
                    project_id: x.f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Export],
                },
            ))
            .result
            .unwrap();
        let mut foreign = x.prepare("foreign-actor-prepare");
        foreign.session = Some(stranger.clone());
        foreign.auth_token = TOKENS.with(|t| t.borrow().get(stranger.as_str()).cloned());
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&foreign).unwrap())
        );
        let io = Arc::new(AuthoredIo::new());
        assert!(x
            .f
            .engine
            .handy_hsp_preparation_with_io(&foreign, &stranger, &fingerprint, io.clone())
            .is_err());
        assert_eq!(io.count(), (0, 0, 0));
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_dependency_changes_retire_without_rewriting_or_resampling() {
        for change in 0..4 {
            let x = PreparationFixture::new();
            let io = Arc::new(AuthoredIo::new());
            let original = x.prepare("prepare-dependency-change");
            x.call(&original, io.clone()).unwrap();
            let ready = x.ready(&original.request_id);
            let counts = io.count();
            let command = match change {
                0 => request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(0),
                    commit_program(&x.f, 0, program(0.3)),
                ),
                1 => qualification_withdraw(&x.f, &x.accepted.dossier_id, x.accepted.revision),
                2 => request(
                    &x.f.session,
                    None,
                    None,
                    Command::CloseHandySession {
                        engine_epoch: x.connection.engine_epoch.clone(),
                        device_session: x.connection.device_session.clone(),
                        owner_proof: x.f.engine.handy_owner_token.clone(),
                    },
                ),
                _ => request(
                    &x.f.session,
                    None,
                    None,
                    Command::DisableHandyRegistration {
                        device_id: x.connection.registration.device_id.clone(),
                        expected_registration_revision: x.connection.registration.revision,
                        owner_proof: x.f.engine.handy_owner_token.clone(),
                    },
                ),
            };
            x.f.engine.handle(command).result.unwrap();
            let retired = x.retired(&original.request_id);
            assert_eq!(retired.history, ready.history, "dependency change {change}");
            let unused = Arc::new(AuthoredIo::new());
            assert_eq!(
                x.call(&original, unused.clone()).unwrap().history,
                ready.history
            );
            assert_eq!(unused.count(), (0, 0, 0));
            assert_eq!(io.count(), counts);
            x.f.engine.finish_shutdown().unwrap();
        }
    }

    #[test]
    fn engine_hsp_preparation_restart_exposes_history_without_native_readiness_or_sync_replay() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        let original = x.prepare("prepare-restart");
        x.call(&original, io.clone()).unwrap();
        let ready = x.ready(&original.request_id);
        let query = x.status_request(&original.request_id);
        let actor = x.f.session.clone();
        x.f.engine.finish_shutdown().unwrap();
        let PreparationFixture { f, .. } = x;
        let Fixture { engine, dir, .. } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = Engine::open(config).unwrap();
        let after = report(reopened.handle(query).result.unwrap());
        assert_eq!(after.history, ready.history);
        assert!(after.native.is_none());
        let unused = Arc::new(AuthoredIo::new());
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&original).unwrap())
        );
        let replay = report(
            reopened
                .handy_hsp_preparation_with_io(&original, &actor, &fingerprint, unused.clone())
                .unwrap(),
        );
        assert_eq!(replay.history, ready.history);
        assert!(replay.native.is_none());
        assert_eq!(unused.count(), (0, 0, 0));
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        reopened.finish_shutdown().unwrap();
        Engine::retire_for_test(reopened);
        drop(dir);
    }

    #[test]
    fn engine_hsp_preparation_only_public_recovery_resolves_unknown_without_restoring_readiness() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        io.lose_sync.store(true, Ordering::Release);
        let original = x.prepare("prepare-unknown-recovery");
        x.call(&original, io.clone()).unwrap();
        let before = x.retired(&original.request_id);
        assert_eq!(
            before.history.as_ref().unwrap().state,
            HandyClockSyncState::Unknown
        );
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        let plan_request = request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::PlanHandyReconciliation {
                selection: HandyRecoverySelection {
                    device_id: x.connection.registration.device_id.clone(),
                    registration_revision: x.connection.registration.revision,
                    engine_epoch: x.connection.engine_epoch.clone(),
                    device_session: x.connection.device_session.clone(),
                    qualification: HandyQualificationBinding {
                        dossier_id: x.accepted.dossier_id.clone(),
                        revision: x.accepted.revision,
                    },
                },
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        let ResponseBody::HandyRecoveryPlan(plan) = x.f.engine.handle(plan_request).result.unwrap()
        else {
            panic!("preparation-only recovery plan");
        };
        assert_eq!(plan.contract.transport, HandyQualificationTransport::Hsp);
        let calls = Arc::new(AtomicU64::new(0));
        let observed = calls.clone();
        let start = reconciliation_start(&x.f, &plan, "recover-preparation-only");
        reconciliation_call(&x.f, &start, move |_, _| {
            observed.fetch_add(1, Ordering::AcqRel);
            Ok(hsp_qualification_sample())
        })
        .unwrap();
        let completed = reconciliation_wait(&x.f, &start.request_id);
        assert_eq!(
            completed.state,
            HandyRecoveryState::Completed,
            "{:?}",
            completed.error
        );
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert!(!completed.authority_restored && !completed.playback_enabled);
        let after = x.status(&original.request_id);
        let history = after.history.unwrap();
        assert_eq!(
            history.record_id,
            before.history.as_ref().unwrap().record_id
        );
        assert_eq!(history.state, HandyClockSyncState::Unknown);
        assert!(history.resolved && history.acknowledgment.is_none());
        assert_ne!(
            after.native.as_ref().map(|n| n.phase),
            Some(HandyHspPreparationPhase::Ready)
        );
        let unused = Arc::new(AuthoredIo::new());
        assert!(
            x.call(&original, unused.clone())
                .unwrap()
                .history
                .unwrap()
                .resolved
        );
        assert_eq!(unused.count(), (0, 0, 0));
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        let replay = reconciliation_call(&x.f, &start, |_, _| {
            panic!("resolved preparation must not inspect again")
        })
        .unwrap();
        assert_eq!(encode(&replay).unwrap(), encode(&completed).unwrap());
        x.f.engine.finish_shutdown().unwrap();
    }

    fn held_original_history(
        x: &PreparationFixture,
        original: &RequestId,
        record_id: &ArtifactId,
    ) -> HandyHspPreparationHistory {
        // The authorizer holds the project DB across this test hook. Read the
        // independent journal directly using the already authenticated original
        // identity; public Engine status is queried only outside the hold.
        let journal = x.f.engine._device_effect_journal.lock().unwrap();
        let observed = journal
            .hsp_preparation_observation(record_id.as_str())
            .unwrap();
        assert_eq!(observed.record.actor_session, x.f.session);
        assert_eq!(&observed.record.request_id, original);
        assert_eq!(observed.id, record_id.as_str());
        observed.history().unwrap()
    }

    fn assert_original_route_is_held(
        x: &PreparationFixture,
        original: &RequestId,
        transferred: bool,
    ) {
        let journal = x.f.engine._device_effect_journal.lock().unwrap();
        let parent = journal
            .hsp_preparation_history(&x.f.session, original)
            .unwrap()
            .unwrap();
        assert_eq!(parent.state, HandyClockSyncState::Acknowledged);
        assert_eq!(parent.custody_transferred, transferred);
        assert!(!parent.resolved);
        assert!(journal
            .has_device_effects(&parent.record.registration.device_id)
            .unwrap());
        assert!(
            journal
                .clock_sync_route_available(
                    &parent.record.route_sha256,
                    &parent.record.route_namespace_sha256,
                    &parent.record.registration.device_id,
                    None
                )
                .is_err(),
            "handoff must not create a shared-exclusion gap"
        );
    }

    #[test]
    fn engine_hsp_preparation_actual_first_setup_claim_transfers_exact_custody_without_gap() {
        let (x, peer, io) = real_handoff_fixture();
        let original = x.prepare("prepare-real-child");
        x.call(&original, io.clone()).unwrap();
        let original_record_id = x.ready(&original.request_id).history.unwrap().record_id;
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        assert_original_route_is_held(&x, &original.request_id, false);
        let hold = Arc::new(Hold::default());
        hold.arm();
        let _release = ReleaseHold(hold.clone());
        let (binding, authority) = prepared_source(&x, &original.request_id, hold.clone());
        let child = RequestId::new("prepared-real-child").unwrap();
        assert!(
            x.f.engine
                .handoff_handy_hsp_preparation(
                    &x.f.session,
                    &x.f.project,
                    &original.request_id,
                    handy_hsp_preparation::HspSourceHandoff {
                        child_request: child.clone(),
                        allow_online_control: false,
                        binding: binding.clone(),
                        authority: authority.clone(),
                    },
                )
                .is_err(),
            "synchronization consent cannot authorize Setup"
        );
        assert!(peer.puts().is_empty());
        let session =
            x.f.engine
                .handoff_handy_hsp_preparation(
                    &x.f.session,
                    &x.f.project,
                    &original.request_id,
                    handy_hsp_preparation::HspSourceHandoff {
                        child_request: child.clone(),
                        allow_online_control: true,
                        binding: binding.clone(),
                        authority: authority.clone(),
                    },
                )
                .unwrap();
        hold.wait_entered();
        let linked = held_original_history(&x, &original.request_id, &original_record_id);
        let child_id = linked.linked_stream_id.clone().unwrap();
        assert!(!linked.custody_transferred);
        {
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(child_id.as_str()).unwrap();
            assert_eq!(stream.record.request_id, child);
            assert_eq!(stream.record.actor_session, x.f.session);
            assert_eq!(stream.effect_ids.len(), 1);
            let setup = journal.hsp_effect(&stream.effect_ids[0]).unwrap();
            assert_eq!(setup.record.kind, HspEffectRecordKind::Setup);
            assert_eq!(setup.state, HspJournalState::Prepared);
        }
        assert!(peer.puts().is_empty());
        assert_original_route_is_held(&x, &original.request_id, false);
        hold.assert_not_timed_out();
        hold.release();
        wait_child(&x, &child, |s| {
            s.post_setup_bound && !s.native_active && !s.metadata_pending
        });
        hold.assert_not_timed_out();
        let transferred = x.status(&original.request_id).history.unwrap();
        assert_eq!(transferred.record_id, linked.record_id);
        assert_eq!(transferred.linked_stream_id, Some(child_id));
        assert!(transferred.custody_transferred);
        assert_original_route_is_held(&x, &original.request_id, true);
        assert_eq!(peer.puts(), ["/hsp/setup"]);
        assert!(x
            .f
            .engine
            .handoff_handy_hsp_preparation(
                &x.f.session,
                &x.f.project,
                &original.request_id,
                handy_hsp_preparation::HspSourceHandoff {
                    child_request: RequestId::new("forbidden-second-child").unwrap(),
                    allow_online_control: true,
                    binding,
                    authority,
                }
            )
            .is_err());
        report(
            x.f.engine
                .handle(x.cancel_request(&original.request_id, "cancel-consumed-parent"))
                .result
                .unwrap(),
        );
        let cancelled_parent = x.status(&original.request_id).history.unwrap();
        assert_eq!(
            cancelled_parent, transferred,
            "consumed parent cancellation cannot remove transferred child history"
        );
        assert_original_route_is_held(&x, &original.request_id, true);
        session.cancel();
        wait_child(&x, &child, |s| {
            s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
                && !s.native_active
                && !s.metadata_pending
        });
        assert_eq!(peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        assert_original_route_is_held(&x, &original.request_id, true);
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_cancelled_prepared_child_never_transfers_or_releases_parent() {
        let (x, peer, io) = real_handoff_fixture();
        let original = x.prepare("prepare-cancelled-child");
        x.call(&original, io.clone()).unwrap();
        let original_record_id = x.ready(&original.request_id).history.unwrap().record_id;
        let hold = Arc::new(Hold::default());
        hold.arm();
        let _release = ReleaseHold(hold.clone());
        let (binding, authority) = prepared_source(&x, &original.request_id, hold.clone());
        let child = RequestId::new("child-cancel-before-setup-claim").unwrap();
        let session =
            x.f.engine
                .handoff_handy_hsp_preparation(
                    &x.f.session,
                    &x.f.project,
                    &original.request_id,
                    handy_hsp_preparation::HspSourceHandoff {
                        child_request: child.clone(),
                        allow_online_control: true,
                        binding: binding.clone(),
                        authority: authority.clone(),
                    },
                )
                .unwrap();
        hold.wait_entered();
        let parent_before = held_original_history(&x, &original.request_id, &original_record_id);
        assert!(!parent_before.custody_transferred);
        let child_id = parent_before.linked_stream_id.clone().unwrap();
        assert_original_route_is_held(&x, &original.request_id, false);
        session.cancel();
        hold.assert_not_timed_out();
        hold.release();
        wait_child(&x, &child, |s| {
            matches!(
                s.runtime_state,
                crate::handy_hsp_runtime::HspRuntimeState::SetupCancelled
                    | crate::handy_hsp_runtime::HspRuntimeState::SetupUnsent
            ) && !s.native_active
                && !s.metadata_pending
        });
        hold.assert_not_timed_out();
        {
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let stream = journal.hsp_stream(child_id.as_str()).unwrap();
            assert_eq!(stream.state, HspStreamJournalState::Cancelled);
            assert_eq!(stream.effect_ids.len(), 1);
            assert_eq!(
                journal.hsp_effect(&stream.effect_ids[0]).unwrap().state,
                HspJournalState::Cancelled
            );
        }
        assert!(
            peer.puts().is_empty(),
            "a joined unsent Setup must never cause Setup or Stop transport"
        );
        report(
            x.f.engine
                .handle(x.cancel_request(&original.request_id, "cancel-parent-with-unused-child"))
                .result
                .unwrap(),
        );
        assert_eq!(
            x.status(&original.request_id).history.unwrap(),
            parent_before
        );
        assert_original_route_is_held(&x, &original.request_id, false);
        assert!(x
            .f
            .engine
            .handoff_handy_hsp_preparation(
                &x.f.session,
                &x.f.project,
                &original.request_id,
                handy_hsp_preparation::HspSourceHandoff {
                    child_request: RequestId::new("forbidden-replacement-child").unwrap(),
                    allow_online_control: true,
                    binding,
                    authority,
                }
            )
            .is_err());
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        x.f.engine.finish_shutdown().unwrap();
    }

    #[test]
    fn engine_hsp_preparation_abandoned_original_claim_converges_to_unknown_without_sync() {
        let x = PreparationFixture::new();
        let io = Arc::new(AuthoredIo::new());
        io.before_claim.arm();
        let _release = ReleaseHold(io.before_claim.clone());
        let original = x.prepare("prepare-abandoned-claim");
        x.call(&original, io.clone()).unwrap();
        io.before_claim.wait_entered();
        let public = x.status(&original.request_id).history.unwrap();
        assert_eq!(public.state, HandyClockSyncState::Prepared);
        let original_record = {
            let mut journal = x.f.engine._device_effect_journal.lock().unwrap();
            let observed = journal
                .hsp_preparation_observation(public.record_id.as_str())
                .unwrap();
            assert_eq!(observed.history().unwrap(), public);
            let prepared = journal.prepare_hsp_preparation(&observed.record).unwrap();
            assert_eq!(prepared.id(), public.record_id.as_str());
            let abandoned = journal.claim_hsp_preparation(&prepared).unwrap();
            assert_eq!(abandoned.id(), public.record_id.as_str());
            // Deterministically reproduce an original durable claim whose local
            // receipt was lost. This is not a simulated OS write/commit failure,
            // and no synchronization or transport callback is executed here.
            drop(abandoned);
            observed.record
        };
        assert_eq!(
            x.status(&original.request_id).history.unwrap().state,
            HandyClockSyncState::Claimed
        );
        assert_eq!(io.syncs.load(Ordering::Acquire), 0);
        io.before_claim.release();
        let settled = x.retired(&original.request_id);
        let history = settled.history.as_ref().unwrap();
        assert_eq!(history.record_id, public.record_id);
        assert_eq!(history.state, HandyClockSyncState::Unknown);
        assert!(history.acknowledgment.is_none());
        let native = settled.native.as_ref().unwrap();
        assert!(!native.worker_active && !native.metadata_pending);
        {
            let journal = x.f.engine._device_effect_journal.lock().unwrap();
            let observed = journal
                .hsp_preparation_observation(history.record_id.as_str())
                .unwrap();
            assert_eq!(
                observed.record, original_record,
                "settling ambiguity must preserve the exact original request"
            );
            assert_eq!(observed.state, HandyClockSyncState::Unknown);
        }
        let unused = Arc::new(AuthoredIo::new());
        assert_eq!(
            x.call(&original, unused.clone()).unwrap().history,
            settled.history
        );
        assert_eq!(unused.count(), (0, 0, 0));
        assert_eq!(io.syncs.load(Ordering::Acquire), 0);
        x.f.engine.finish_shutdown().unwrap();
    }

    struct AlteredSourceFacts {
        original: Arc<PreparedSourceAuthority>,
        case: u8,
    }
    impl HspNativeAuthority for AlteredSourceFacts {
        fn current_facts(&self, now: u64) -> Result<Option<HspDispatchFacts>, HspDispatchFailure> {
            let Some(facts) = self.original.current_facts(now)? else {
                return Ok(None);
            };
            let reference = facts.reference();
            let mut binding = facts.binding().clone();
            let mut phase = reference.phase();
            let start = reference.start_time_ms();
            let mut server = reference.server_time_ms();
            match self.case {
                0 => {
                    binding = HspBinding::new(
                        binding.identity().clone(),
                        binding.owner().clone(),
                        binding.session().clone(),
                        ArtifactId::new("foreign-native-clock-domain").unwrap(),
                    )
                }
                1 => {
                    server = server
                        .checked_add(1)
                        .ok_or(HspDispatchFailure::InvalidClock)?
                }
                2 => {
                    let width = phase.latest_ms - phase.earliest_ms;
                    phase.earliest_ms = i64::from(start);
                    phase.latest_ms = i64::from(start) + width;
                }
                3 => {
                    let width = phase.latest_ms - phase.earliest_ms;
                    phase.latest_ms = i64::from(start);
                    phase.earliest_ms = i64::from(start) - width;
                }
                _ => unreachable!(),
            }
            let altered = HspPlayReference::new(reference.at_ns(), phase, start, server)
                .map_err(HspDispatchFailure::Authority)?;
            HspDispatchFacts::new(binding, altered, facts.window().clone())
                .map(Some)
                .map_err(HspDispatchFailure::Authority)
        }
        fn authorize(
            &self,
            _deadline: Instant,
            _cancel: &AtomicBool,
            gate: HspClaimGate<'_>,
            owner: &HspNativeOwner,
        ) -> Result<HspClaimedHandoff, HspDispatchFailure> {
            gate.apply(owner, |now| self.current_facts(now))
        }
    }

    #[test]
    fn engine_hsp_preparation_rejects_foreign_or_one_sided_source_clock_without_consuming_readiness(
    ) {
        let (x, peer, io) = real_handoff_fixture();
        let original = x.prepare("prepare-source-facts-refusals");
        x.call(&original, io.clone()).unwrap();
        x.ready(&original.request_id);
        for case in 0..4 {
            let (binding, authority) =
                prepared_source(&x, &original.request_id, Arc::new(Hold::default()));
            let altered = Arc::new(AlteredSourceFacts {
                original: authority,
                case,
            });
            let result = x.f.engine.handoff_handy_hsp_preparation(
                &x.f.session,
                &x.f.project,
                &original.request_id,
                handy_hsp_preparation::HspSourceHandoff {
                    child_request: RequestId::new(format!("rejected-source-facts-{case}")).unwrap(),
                    allow_online_control: true,
                    binding,
                    authority: altered,
                },
            );
            assert!(
                result.is_err(),
                "source fact case {case} must refuse before readiness consumption"
            );
            let still_original =
                x.f.engine
                    .with_handy_hsp_readiness_for_test(
                        &x.f.session,
                        &x.f.project,
                        &original.request_id,
                        |_, record| record.request_id.clone(),
                    )
                    .unwrap();
            assert_eq!(still_original, original.request_id);
            assert!(
                peer.puts().is_empty(),
                "source fact case {case} emitted Setup"
            );
            let status = x.status(&original.request_id);
            assert_eq!(
                status.native.as_ref().unwrap().phase,
                HandyHspPreparationPhase::Ready
            );
            assert!(status.history.as_ref().unwrap().linked_stream_id.is_none());
            assert!(!status.history.unwrap().custody_transferred);
        }
        report(
            x.f.engine
                .handle(x.cancel_request(&original.request_id, "cancel-source-refusals"))
                .result
                .unwrap(),
        );
        x.retired(&original.request_id);
        assert!(peer.puts().is_empty());
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        x.f.engine.finish_shutdown().unwrap();
    }

    struct UnsupportedStartingClockIo;
    impl handy_hsp_preparation::HspPreparationIo for UnsupportedStartingClockIo {
        fn now(&self) -> PResult<u64> {
            Err(ProtocolError::unsupported(
                "authored unavailable starting clock",
            ))
        }
        fn inspect(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            _clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            panic!("metadata/rejected fresh preparation must not inspect replacement IO")
        }
        fn synchronize(
            &self,
            _credentials: Arc<crate::handy_credentials::Credentials>,
            _deadline: Instant,
            _clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            panic!("metadata/rejected fresh preparation must not synchronize replacement IO")
        }
        fn server_sample(
            &self,
            _deadline: Instant,
            _clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            panic!("metadata/rejected fresh preparation must not sample replacement server IO")
        }
    }

    #[test]
    fn engine_hsp_preparation_metadata_survives_unsupported_replacement_clock_but_fresh_prepare_refuses(
    ) {
        let x = PreparationFixture::new();
        let original_io = Arc::new(AuthoredIo::new());
        let original = x.prepare("prepare-original-clock");
        x.call(&original, original_io.clone()).unwrap();
        let ready = x.ready(&original.request_id);
        let original_calls = original_io.count();
        assert_eq!(original_calls.1, 1);
        let unsupported = Arc::new(UnsupportedStartingClockIo);
        // The original entry owns its successful clock. Replacement starting
        // clock failure cannot make its metadata unreadable or trigger new IO.
        let status = x
            .call(&x.status_request(&original.request_id), unsupported.clone())
            .unwrap();
        assert_eq!(status.history, ready.history);
        assert_eq!(
            status.native.as_ref().unwrap().phase,
            HandyHspPreparationPhase::Ready
        );
        let replay = x.call(&original, unsupported.clone()).unwrap();
        assert_eq!(replay.history, ready.history);
        assert_eq!(
            replay.native.as_ref().unwrap().phase,
            HandyHspPreparationPhase::Ready
        );
        assert_eq!(
            replay
                .native
                .as_ref()
                .unwrap()
                .clock_valid_until_boottime_ns,
            ready.native.as_ref().unwrap().clock_valid_until_boottime_ns
        );
        let cancel = x.cancel_request(
            &original.request_id,
            "cancel-with-unavailable-starting-clock",
        );
        let cancelled = x.call(&cancel, unsupported.clone()).unwrap();
        assert_eq!(cancelled.history, ready.history);
        let retired = x.retired(&original.request_id);
        assert_eq!(retired.history, ready.history);
        assert_eq!(
            retired.native.as_ref().unwrap().phase,
            HandyHspPreparationPhase::Retired
        );
        assert_eq!(
            x.call(&original, unsupported.clone()).unwrap().history,
            ready.history
        );
        assert_eq!(
            original_io.count(),
            original_calls,
            "metadata must not repeat original acquisition or sync"
        );

        // Use a fresh Engine/route so an unrelated original custody refusal
        // cannot conceal whether fresh admission actually requires a clock.
        let fresh = PreparationFixture::new();
        let fresh_request = fresh.prepare("fresh-prepare-clock-unavailable");
        assert_eq!(
            fresh
                .call(&fresh_request, unsupported.clone())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert!(fresh
            .f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .hsp_preparation_history(&fresh.f.session, &fresh_request.request_id)
            .unwrap()
            .is_none());
        assert_eq!(
            fresh
                .call(
                    &fresh.status_request(&fresh_request.request_id),
                    unsupported
                )
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "rejected fresh preparation must not create a reservation or readiness"
        );
        fresh.f.engine.finish_shutdown().unwrap();
        x.f.engine.finish_shutdown().unwrap();
    }
    include!("engine_handy_hsp_preclaim_tests.rs");
    include!("engine_handy_hsp_continuity_clock_tests.rs");
    include!("engine_handy_hsp_live_cpu_native_tests.rs");
}
