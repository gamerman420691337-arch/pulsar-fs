// Included inside the rendered browser fixture so all setup, clocks and peer
// contracts are identical. Fault controls never dispatch an HSP action.
mod public_ui_fault_fixture {
    use super::*;
    use std::sync::atomic::AtomicU8;

    const QUIET_MS: u64 = 1500;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FaultCase {
        PrepareCancel,
        DelayedBind,
        StaleInspect,
        Pause,
        Seek,
        PageClose,
        SourceWithdrawal,
        DeviceWithdrawal,
    }
    impl FaultCase {
        fn selected() -> Self {
            match std::env::var("PULSAR_HSP_RENDERED_FAULT_CASE")
                .unwrap()
                .as_str()
            {
                "prepare-cancel" => Self::PrepareCancel,
                "delayed-bind" => Self::DelayedBind,
                "stale-inspection" => Self::StaleInspect,
                "pause" => Self::Pause,
                "seek" => Self::Seek,
                "page-close" => Self::PageClose,
                "source-withdrawal" => Self::SourceWithdrawal,
                "device-withdrawal" => Self::DeviceWithdrawal,
                _ => panic!("unknown rendered HSP fault case"),
            }
        }
        fn name(self) -> &'static str {
            match self {
                Self::PrepareCancel => "prepare-cancel",
                Self::DelayedBind => "delayed-bind",
                Self::StaleInspect => "stale-inspection",
                Self::Pause => "pause",
                Self::Seek => "seek",
                Self::PageClose => "page-close",
                Self::SourceWithdrawal => "source-withdrawal",
                Self::DeviceWithdrawal => "device-withdrawal",
            }
        }
        fn withdrawal(self) -> bool {
            matches!(self, Self::SourceWithdrawal | Self::DeviceWithdrawal)
        }
        fn required_ui(self) -> &'static str {
            match self {
                Self::PrepareCancel => "Start prepared Engine video with 500ms lookahead and analysis continuity; wait for actual CPU output; Inspect HSP selection; consent and Prepare synchronization; while fault-stage says awaiting-public-cancel click Stop ORIGINAL HSP stream. Do not Bind, Enable or Play HSP.",
                Self::DelayedBind => "Start the prepared Engine video with 500ms lookahead and analysis continuity; wait for actual CPU output; Inspect and consent to Prepare synchronization, then consent to Bind. Click Bind current analysis, wait for public HSP status stream phase prefilling while the original Bind is pending, immediately click Stop ORIGINAL HSP stream. While video still plays, wait for cleanup status and verify Bind/Play/Enable remain disabled after the late reply; then click Stop preview. The original Bind response gate expires at three seconds from native request ingress. Do not Enable continuity or Play HSP. After cleanup inspect ORIGINAL status and verify Play/Bind remain disabled.",
                Self::StaleInspect => "Start prepared Engine video with 500ms lookahead, without analysis continuity; wait for actual CPU output. Click Inspect HSP selection only. After native inspection-reply-held, click Stop ORIGINAL HSP stream and capture first cleanup public_stop with null Stop/cancel IDs, then click Stop preview. After source-a-settled, promptly use native video Play to create player B. The original native reply gate remains bounded to three seconds from ingress. After replacement-reply-written, verify empty HSP selection, cleared consent and disabled actions on B; checking synchronization consent must not enable Prepare. Then explicitly Inspect B once, observe fresh selection and cleared consent. Do not Prepare, Bind, Enable or Play HSP. Stop preview B for source cleanup.",
                Self::Pause => "Use public Inspect, Prepare, Bind, Enable bounded HSP continuity and Play. Promptly after playing-awaiting-interruption, pause the visible video using its native controls. Do not resume or create another HSP lifecycle.",
                Self::Seek => "Use public Inspect, Prepare, Bind, Enable bounded HSP continuity and Play. Promptly after playing-awaiting-interruption, seek the visible video with its native controls. Do not press Play again or create another HSP lifecycle.",
                Self::PageClose => "Use public Inspect, Prepare, Bind, Enable bounded HSP continuity and Play. Promptly after playing-awaiting-interruption, close this browser tab. Do not reopen or create another HSP lifecycle.",
                Self::SourceWithdrawal | Self::DeviceWithdrawal => "Use public Inspect, Prepare, Bind, Enable bounded HSP continuity and Play. After playing-awaiting-withdrawal, send the named supervisor fault trigger. After fault-observed, click Stop ORIGINAL HSP stream and inspect ORIGINAL status. Do not create another HSP lifecycle.",
            }
        }
    }

    // One held first preparation read; all returned observations and time samples
    // still come from the existing real-boottime authored device implementation.
    struct FaultIo {
        base: Arc<SustainedIo>,
        engine: std::sync::Weak<Engine>,
        root: PathBuf,
        enabled: bool,
        entered: AtomicBool,
        public_cancel_observed: AtomicBool,
        late_read_returned: AtomicBool,
        // 0 held/not entered; 1 named release; 2 actual public cancellation;
        // 3 original deadline; 4 shutdown. No outcome synthesizes acceptance.
        exit: AtomicU8,
    }
    impl FaultIo {
        fn new(
            base: Arc<SustainedIo>,
            engine: &Arc<Engine>,
            root: PathBuf,
            case: FaultCase,
        ) -> Self {
            Self {
                base,
                engine: Arc::downgrade(engine),
                root,
                enabled: case == FaultCase::PrepareCancel,
                entered: AtomicBool::new(false),
                public_cancel_observed: AtomicBool::new(false),
                late_read_returned: AtomicBool::new(false),
                exit: AtomicU8::new(0),
            }
        }
        fn error(message: &str) -> ProtocolError {
            ProtocolError::new(ErrorCode::Unavailable, message)
        }
        fn hold(&self, deadline: Instant, clock: &mut dyn FnMut() -> PResult<u64>) -> PResult<()> {
            if !self.enabled || self.entered.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            loop {
                clock()?;
                if Instant::now() >= deadline {
                    self.exit.store(3, Ordering::Release);
                    return Err(Self::error(
                        "original preparation deadline elapsed during authored read hold",
                    ));
                }
                if self
                    .engine
                    .upgrade()
                    .is_none_or(|engine| engine.is_shutting_down())
                {
                    self.exit.store(4, Ordering::Release);
                    return Err(Self::error(
                        "Engine shutdown ended authored preparation read hold",
                    ));
                }
                if self.public_cancel_observed.load(Ordering::Acquire) {
                    self.exit.store(2, Ordering::Release);
                    // Return the real authored read after public cancellation;
                    // Engine must reject this late successful IO completion.
                    return Ok(());
                }
                let release = self.root.join("RELEASE_PREPARATION");
                if release.is_file() {
                    assert_eq!(fs::read(&release).unwrap(), b"prepare-cancel\n");
                    self.exit.store(1, Ordering::Release);
                    return Ok(());
                }
                std::thread::park_timeout(Duration::from_millis(5));
            }
        }
    }
    impl HspPreparationIo for FaultIo {
        fn now(&self) -> PResult<u64> {
            self.base.now()
        }
        fn inspect(
            &self,
            credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.hold(deadline, clock)?;
            let result = self.base.inspect(credentials, deadline, clock);
            if self.enabled && self.exit.load(Ordering::Acquire) == 2 && result.is_ok() {
                self.late_read_returned.store(true, Ordering::Release);
            }
            result
        }
        fn synchronize(
            &self,
            credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            self.base.synchronize(credentials, deadline, clock)
        }
        fn server_sample(
            &self,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            self.base.server_sample(deadline, clock)
        }
        fn hsp_origin(&self) -> Option<String> {
            self.base.hsp_origin()
        }
        fn inspect_hsp_stream(
            &self,
            credentials: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
            stream: u32,
        ) -> PResult<crate::handy_online::PlayingTimingRead> {
            self.base
                .inspect_hsp_stream(credentials, deadline, clock, stream)
        }
    }

    fn ids_value(ids: &UiOriginals, cancel: &Option<RequestId>) -> Value {
        let mut value = serde_json::to_value(ids).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("cancel".into(), json!(cancel));
        value
    }
    fn fault_stage(
        output: &std::path::Path,
        case: FaultCase,
        stage: &str,
        ids: &UiOriginals,
        cancel: &Option<RequestId>,
    ) {
        record(
            output,
            "fault-stage.json",
            &json!({"case":case.name(),"stage":stage,"originals":ids_value(ids,cancel),"required_ui":case.required_ui(),"qualified":false,"physical_device":false}),
        );
    }
    fn effect_counts(peer: &SustainedPeer) -> Value {
        let state = peer.state.lock().unwrap();
        json!({"setup":state.setups,"play":state.plays,"stop":state.stops})
    }
    fn check_effect_counts(peer: &SustainedPeer, case: FaultCase) {
        let expected = match case {
            FaultCase::PrepareCancel | FaultCase::StaleInspect => {
                json!({"setup":0,"play":0,"stop":0})
            }
            FaultCase::DelayedBind => json!({"setup":1,"play":0,"stop":1}),
            _ => json!({"setup":1,"play":1,"stop":1}),
        };
        assert_eq!(effect_counts(peer), expected);
    }
    // Status is read-only. The only additional remembered preparation response
    // with a different SQL request identity is the public Cancel command; it
    // stores the original preparation with native=None before retiring it.
    fn discover_cancel(f: &Fixture, prep: &RequestId, cancel: &mut Option<RequestId>) {
        let db = f.engine.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT request,response FROM requests WHERE session=?1 LIMIT 4097")
            .unwrap();
        let rows = query
            .query_map([f.session.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap();
        let mut bytes = 0;
        for (index, row) in rows.enumerate() {
            assert!(index < 4096);
            let (id, raw) = row.unwrap();
            bytes += raw.len();
            assert!(raw.len() <= 1024 * 1024 && bytes <= 32 * 1024 * 1024);
            if let ResponseBody::HandyHspPreparation(value) = serde_json::from_str(&raw).unwrap() {
                if value.project_id == f.project
                    && &value.preparation_request_id == prep
                    && id != prep.as_str()
                {
                    assert!(value.native.is_none());
                    retain_ui_id(cancel, &RequestId::new(id).unwrap());
                }
            }
        }
    }
    fn observe_ids(f: &Fixture, ids: &mut UiOriginals, cancel: &mut Option<RequestId>) {
        discover_ui(f, ids);
        if let Some(prep) = &ids.preparation {
            discover_cancel(f, prep, cancel);
        }
        if let Some(stream) = &ids.stream {
            if let Some(native) = &live_status(f, stream).native {
                if let Some(id) = &native.original_play_request_id {
                    retain_ui_id(&mut ids.play, id);
                }
                if let Some(id) = &native.original_stop_request_id {
                    retain_ui_id(&mut ids.stop, id);
                }
            }
        }
    }

    #[derive(serde::Serialize)]
    struct InterruptionChain {
        captures: BTreeMap<String, LiveCaptureReport>,
        preparations: BTreeMap<String, BrowserAnalysisSuccessorReport>,
        transfers: BTreeMap<String, BrowserAnalysisTransferReport>,
        pending_worker_custody: bool,
        pending_storage_custody: bool,
        worker_fence: Value,
    }
    impl InterruptionChain {
        fn identities(&self) -> Value {
            json!({"captures":self.captures.keys().collect::<Vec<_>>(),
                "preparations":self.preparations.keys().collect::<Vec<_>>(),
                "transfers":self.transfers.keys().collect::<Vec<_>>()})
        }
        fn settled(&self) -> bool {
            self.captures.values().all(|c| {
                !c.active
                    && c.admitted_pixel_bytes == 0
                    && c.queue.is_none()
                    && c.inference.as_ref().is_some_and(|i| {
                        matches!(
                            i.state,
                            LiveCaptureInferenceState::Retired | LiveCaptureInferenceState::Failed
                        )
                    })
            }) && self.transfers.values().all(|t| {
                matches!(
                    t.phase,
                    BrowserAnalysisTransferPhase::Transferred
                        | BrowserAnalysisTransferPhase::Aborted
                )
            }) && !self.pending_worker_custody
                && !self.pending_storage_custody
                && ["groups", "retired", "staging"].iter().all(|key| {
                    self.worker_fence[*key]
                        .as_object()
                        .is_some_and(|m| m.is_empty())
                })
                && self.worker_fence["creation"]["records"]
                    .as_object()
                    .is_some_and(|m| m.is_empty())
        }
    }
    // Take retained identities in one DB snapshot, then inspect their current
    // native status. These reads neither cancel nor promote a source. Every
    // observation is written before assertions, retaining a failing chain too.
    fn interruption_chain(f: &Fixture, output: &std::path::Path, name: &str) -> InterruptionChain {
        let (identities, preparations, transfer_ids) = {
            let db = f.engine.db.lock().unwrap();
            let mut query = db
                .prepare("SELECT request,response FROM requests WHERE session=?1 LIMIT 4097")
                .unwrap();
            let rows = query
                .query_map([f.session.as_str()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap();
            let mut identities = BTreeMap::new();
            let mut preparations = BTreeMap::new();
            let mut transfer_ids = BTreeMap::new();
            let mut bytes = 0;
            for (index, row) in rows.enumerate() {
                assert!(index < 4096);
                let (id, raw) = row.unwrap();
                bytes += raw.len();
                assert!(raw.len() <= 1024 * 1024 && bytes <= 32 * 1024 * 1024);
                match serde_json::from_str::<ResponseBody>(&raw).unwrap() {
                    ResponseBody::LiveCapture(c) if c.identity.project == f.project => {
                        identities.insert(c.identity.epoch.to_string(), c.identity.clone());
                    }
                    ResponseBody::BrowserAnalysisSuccessor(p)
                        if p.predecessor.project == f.project =>
                    {
                        assert_eq!(id, p.original_request_id.as_str());
                        identities.insert(p.predecessor.epoch.to_string(), p.predecessor.clone());
                        identities.insert(
                            p.successor.identity.epoch.to_string(),
                            p.successor.identity.clone(),
                        );
                        preparations.insert(id, *p);
                    }
                    ResponseBody::BrowserAnalysisTransfer(t)
                        if t.predecessor.project == f.project =>
                    {
                        identities.insert(t.predecessor.epoch.to_string(), t.predecessor.clone());
                        identities.insert(t.successor.epoch.to_string(), t.successor.clone());
                        transfer_ids.insert(
                            t.original_request_id.to_string(),
                            t.original_request_id.clone(),
                        );
                    }
                    _ => {}
                }
            }
            (identities, preparations, transfer_ids)
        };
        // The existing fixture discovery limit remains unchanged; this is a
        // bounded single original continuity chain, not arbitrary new captures.
        assert!(identities.len() <= 8 && preparations.len() <= 8 && transfer_ids.len() <= 8);
        let captures = identities
            .into_iter()
            .map(|(key, id)| (key, snapshot(f, &id)))
            .collect();
        let transfers = transfer_ids
            .into_iter()
            .map(|(key, id)| {
                let ResponseBody::BrowserAnalysisTransfer(value) = f
                    .engine
                    .handle(request(
                        &f.session,
                        Some(&f.project),
                        None,
                        Command::BrowserAnalysisTransferStatus {
                            transfer_request_id: id,
                        },
                    ))
                    .result
                    .unwrap()
                else {
                    panic!("original interruption transfer status");
                };
                (key, *value)
            })
            .collect();
        let result = InterruptionChain {
            captures,
            preparations,
            transfers,
            pending_worker_custody: f.engine.worker_fence.has_pending_worker_custody().unwrap(),
            pending_storage_custody: f.engine.worker_fence.has_pending_storage_custody().unwrap(),
            worker_fence: serde_json::from_slice(
                &fs::read(f.engine.config.state_dir.join("worker-fence.json")).unwrap(),
            )
            .unwrap(),
        };
        record(output, name, &result);
        result
    }
    fn validate_interruption_chain(
        chain: &InterruptionChain,
        first: &LiveCaptureReport,
    ) -> Vec<String> {
        let original_media = first.options.browser_media.as_ref().unwrap();
        let original_analysis = first.options.browser_analysis.as_ref().unwrap();
        let first_epoch = first.identity.epoch.to_string();
        assert!(chain.captures.contains_key(&first_epoch));
        let mut children = BTreeMap::new();
        let mut parents = BTreeMap::new();
        for preparation in chain.preparations.values() {
            preparation.validate().unwrap();
            let predecessor = preparation.predecessor.epoch.to_string();
            let successor = preparation.successor.identity.epoch.to_string();
            assert_eq!(
                chain.captures[&predecessor].identity,
                preparation.predecessor
            );
            assert_eq!(
                chain.captures[&successor].identity,
                preparation.successor.identity
            );
            assert!(
                children
                    .insert(predecessor.clone(), successor.clone())
                    .is_none(),
                "original continuity must not fork"
            );
            assert!(
                parents.insert(successor, predecessor).is_none(),
                "successor cannot change predecessor"
            );
        }
        assert!(!parents.contains_key(&first_epoch));
        let mut order = vec![first_epoch];
        while let Some(child) = children.get(order.last().unwrap()) {
            assert!(!order.contains(child), "source continuation cycle");
            order.push(child.clone());
            assert!(order.len() <= 8);
        }
        assert_eq!(
            order.len(),
            chain.captures.len(),
            "capture outside original source chain"
        );
        for capture in chain.captures.values() {
            capture.validate().unwrap();
            assert_eq!(capture.identity.project, first.identity.project);
            assert_eq!(capture.identity.revision, first.identity.revision);
            assert_eq!(capture.identity.source, first.identity.source);
            assert_eq!(capture.engine_epoch, first.engine_epoch);
            let analysis = capture.options.browser_analysis.as_ref().unwrap();
            let media = capture.options.browser_media.as_ref().unwrap();
            assert_eq!(analysis.player, original_analysis.player);
            assert_eq!(analysis.policy, original_analysis.policy);
            assert_eq!(media.selection, original_media.selection);
            assert_eq!(media.clock, original_media.clock);
            assert_eq!(media.lease_ms, original_media.lease_ms);
            assert_eq!(capture.options.lease_ms, first.options.lease_ms);
            assert_eq!(
                capture.options.source_time_basis,
                first.options.source_time_basis
            );
            assert_eq!(
                serde_json::to_value(capture.options.policy).unwrap(),
                serde_json::to_value(first.options.policy).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&capture.options.inference).unwrap(),
                serde_json::to_value(&first.options.inference).unwrap()
            );
            assert_eq!(
                capture.inference.as_ref().unwrap().profile_sha256,
                first.inference.as_ref().unwrap().profile_sha256
            );
            assert_eq!(
                capture
                    .clock
                    .project_at_source(capture.options.source_anchor)
                    .unwrap(),
                first
                    .clock
                    .project_at_source(capture.options.source_anchor)
                    .unwrap(),
                "successor changed original project correspondence"
            );
            assert!(
                capture.input_close.is_none(),
                "finite input closure cannot substitute for interruption"
            );
        }
        let mut transferred_preparations = std::collections::BTreeSet::new();
        for transfer in chain.transfers.values() {
            transfer.validate().unwrap();
            let preparation = &chain.preparations[transfer.preparation_request_id.as_str()];
            assert_eq!(transfer.predecessor, preparation.predecessor);
            assert_eq!(transfer.successor, preparation.successor.identity);
            assert!(
                transferred_preparations.insert(transfer.preparation_request_id.clone()),
                "original successor acquired another transfer"
            );
            if transfer.phase == BrowserAnalysisTransferPhase::Transferred {
                assert!(
                    transfer.predecessor_worker_joined
                        && transfer.predecessor_delivery_settled
                        && transfer.successor_pair_accepted
                );
                assert!(transfer.error.is_none());
            }
            if transfer.phase == BrowserAnalysisTransferPhase::Aborted {
                assert!(transfer.error.is_some());
                // Aborted reports retain their original partial join flags.
                // Independent real worker custody below proves final cleanup.
            }
        }
        order
    }
    fn validate_final_source(
        chain: &InterruptionChain,
        order: &[String],
        first: &LiveCaptureReport,
        live: &HandyHspLiveStatus,
        ids: &UiOriginals,
        origin: &Value,
        starting_source: &HandyHspLiveActiveSource,
    ) -> HandyHspLiveActiveSource {
        assert_eq!(
            serde_json::to_value(live.origin.as_ref().unwrap()).unwrap(),
            *origin
        );
        assert_eq!(Some(&live.stream_request_id), ids.stream.as_ref());
        assert_eq!(
            Some(&live.selection.preparation_request_id),
            ids.preparation.as_ref()
        );
        assert_eq!(live.selection.capture_epoch, first.identity.epoch);
        let native = live.native.as_ref().unwrap();
        assert_eq!(native.original_play_request_id.as_ref(), ids.play.as_ref());
        assert_eq!(native.original_stop_request_id.as_ref(), ids.stop.as_ref());
        assert!(native.retired);
        let active = native.active_source.as_ref().unwrap();
        active.validate().unwrap();
        let index = order
            .iter()
            .position(|epoch| epoch == active.capture_epoch.as_str())
            .expect("final HSP source must belong to original chain");
        assert_eq!(active.generation, index as u64 + 1);
        assert!(active.generation >= starting_source.generation);
        let starting_index = order
            .iter()
            .position(|epoch| epoch == starting_source.capture_epoch.as_str())
            .expect("Admitted source must belong to the same original chain");
        assert_eq!(starting_source.generation, starting_index as u64 + 1);
        let played = &chain.captures[starting_source.capture_epoch.as_str()];
        assert_eq!(
            starting_source.inference_attempt_id,
            played.inference.as_ref().unwrap().attempt_id
        );
        assert_eq!(
            starting_source.browser_timing,
            played
                .options
                .browser_analysis
                .as_ref()
                .unwrap()
                .timing_binding_id
        );
        assert_eq!(
            starting_source.media_preparation_request_id,
            played
                .options
                .browser_media
                .as_ref()
                .unwrap()
                .preparation_request_id
        );
        let capture = &chain.captures[active.capture_epoch.as_str()];
        assert_eq!(
            active.inference_attempt_id,
            capture.inference.as_ref().unwrap().attempt_id
        );
        assert_eq!(
            active.browser_timing,
            capture
                .options
                .browser_analysis
                .as_ref()
                .unwrap()
                .timing_binding_id
        );
        assert_eq!(
            active.media_preparation_request_id,
            capture
                .options
                .browser_media
                .as_ref()
                .unwrap()
                .preparation_request_id
        );
        assert!(
            !capture.active,
            "retiring an old predecessor cannot prove the final source retired"
        );
        assert!(
            order.len() <= index + 2,
            "at most one uncommitted original successor may remain"
        );
        for (edge, epochs) in order.windows(2).enumerate() {
            let transfer = chain.transfers.values().find(|t| {
                t.predecessor.epoch.as_str() == epochs[0] && t.successor.epoch.as_str() == epochs[1]
            });
            if edge < index {
                assert_eq!(
                    transfer
                        .expect("promoted source lacks original transfer")
                        .phase,
                    BrowserAnalysisTransferPhase::Transferred
                );
            } else if let Some(transfer) = transfer {
                assert_eq!(
                    transfer.phase,
                    BrowserAnalysisTransferPhase::Aborted,
                    "pending successor cannot promote after original Stop"
                );
            }
        }
        if index > 0 {
            let transfer = chain
                .transfers
                .values()
                .find(|t| t.successor.epoch == active.capture_epoch)
                .unwrap();
            assert_eq!(
                active.last_transfer_request_id.as_ref(),
                Some(&transfer.original_request_id)
            );
            assert_eq!(
                active.predecessor_capture_epoch.as_ref(),
                Some(&transfer.predecessor.epoch)
            );
        }
        let runtime = live
            .stream
            .as_ref()
            .and_then(|s| s.native.as_ref())
            .unwrap();
        assert_eq!(
            runtime.runtime_state,
            HandyHspRuntimeState::StopAcknowledged
        );
        assert!(!runtime.native_active && !runtime.metadata_pending);
        assert!(
            runtime.finite_drain.is_none(),
            "ordinary finite drain is not interruption evidence"
        );
        active.clone()
    }
    fn check_quiet_chain(
        f: &Fixture,
        output: &std::path::Path,
        first: &LiveCaptureReport,
        expected: &Value,
    ) {
        let chain = interruption_chain(f, output, "interruption-chain-latest.json");
        validate_interruption_chain(&chain, first);
        assert_eq!(
            chain.identities(),
            *expected,
            "new capture or transfer admitted after interruption settled"
        );
        assert!(
            chain.settled(),
            "retired original chain reacquired worker or delivery custody"
        );
    }

    include!("engine_handy_hsp_browser_delayed_bind_fixture.rs");
    include!("engine_handy_hsp_browser_stale_inspect_fixture.rs");

    #[test]
    #[ignore = "requires explicit private tmpfs supervisor, exact fault allowlist, pinned ordinary app/worker/runtime/media and real CUA public UI; never run as a default test"]
    fn native_rendered_browser_hsp_public_ui_fault() {
        let case = FaultCase::selected();
        let root = env_path("PULSAR_HSP_RENDERED_ROOT");
        ram_directory(&root);
        let output = root.join("redacted");
        fs::create_dir(&output).unwrap();
        let worker = env_path("PULSAR_HSP_LIVE_NATIVE_WORKER");
        let binary = env_path("PULSAR_HSP_RENDERED_BINARY");
        let fixture = env_path("PULSAR_HSP_LIVE_NATIVE_FIXTURE");
        let runtime = env_path("PULSAR_ORT_DYLIB");
        assert_eq!(artifact(&runtime).sha256, ORT_SHA);
        let inputs: Value = serde_json::from_slice(&fs::read(&fixture).unwrap()).unwrap();
        assert_eq!(
            inputs["kind"],
            "authored-sterile-rendered-cpu-software-fixture"
        );
        assert_eq!(inputs["duration_seconds"], 40);
        assert_eq!(inputs["trained_model"], false);
        assert_eq!(inputs["qualified"], false);
        let media_path = input_path(&inputs, "media");
        let model = input_path(&inputs, "model");
        assert_eq!(artifact(&model).sha256, MODEL_SHA);
        let f = native_fixture(worker.clone());
        ram_directory(f.dir.path());
        let engine = f.engine.clone();
        let mut shutdown = ShutdownGuard(Some(engine.clone()));
        let ResponseBody::Source { source_version } = engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path: media_path },
            ))
            .result
            .unwrap()
        else {
            panic!("media source")
        };
        engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(0, 1).unwrap(),
                        source_end: SourceTimestamp::new(40, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        project_anchor: ProjectTime::ZERO,
                        rate_numerator: 1,
                        rate_denominator: 1,
                    },
                    replace: None,
                },
            ))
            .result
            .unwrap();
        let runtime_identity = engine
            .tools
            .as_ref()
            .unwrap()
            .onnx_runtime
            .as_ref()
            .unwrap()
            .identity
            .clone();
        let profile = profile(&model, runtime_identity);
        let (x, mut peer, io, device_permit) = device_fixture(f);
        let mut shutdown_after_peer = ShutdownGuard(Some(engine.clone()));
        shutdown.0 = None;
        let f = &x.f;
        let contract_media = prepare_contract_media(f);
        let (source_qualification, pending_source) =
            rendered_source_dossier(&x, &device_permit, &contract_media, &profile);
        record(&output, "device-dossier-pending.json", &x.accepted);
        record(&output, "source-dossier-pending.json", &pending_source);
        let finite_hash = x
            .accepted
            .manifest
            .subject
            .hsp_finite_drain_policy_sha256()
            .unwrap()
            .unwrap();
        engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: contract_media.preparation_request_id.clone(),
                },
            ))
            .result
            .unwrap();
        let fault_io = Arc::new(FaultIo::new(io.clone(), &engine, root.clone(), case));
        engine
            .install_handy_hsp_preparation_io_for_test(
                &f.session,
                &x.connection.engine_epoch,
                &x.connection.device_session,
                fault_io.clone(),
            )
            .unwrap();
        let hsp_config = json!({"engine_epoch":x.connection.engine_epoch,"device_session":x.connection.device_session,
            "device_qualification":device_permit,"source_qualification":source_qualification,
            "preparation_timeout_ms":5000,"finite_drain_policy_sha256":finite_hash,
            "continuity_duration_ms":CONTINUITY_MS});
        let hsp_config_path = root.join("handy-hsp.json");
        let mut config_file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&hsp_config_path)
            .unwrap();
        config_file
            .write_all(&serde_json::to_vec(&hsp_config).unwrap())
            .unwrap();
        drop(config_file);
        record(&output, "public-hsp-selection.json", &hsp_config);
        let mut originals = UiOriginals::default();
        let mut cancel_id = None;
        fault_stage(
            &output,
            case,
            "awaiting-public-preparation",
            &originals,
            &cancel_id,
        );
        let mut options = capture_options();
        options.policy.maximum_analysis_age_ns = CAPTURE_AGE_NS;
        options.lease_ms = 30_000;
        options.inference = Some(LiveCaptureInference {
            model_path: model,
            profile: profile.clone(),
        });
        options.validate().unwrap();
        let options_path = root.join("options.json");
        fs::write(&options_path, serde_json::to_vec(&options).unwrap()).unwrap();
        let credentials = json!({"session":f.session,"auth_token":TOKENS.with(|tokens|tokens.borrow().get(f.session.as_str()).cloned().unwrap())});
        let mut session = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(engine.config.state_dir.join("first-party.session.json"))
            .unwrap();
        session
            .write_all(&serde_json::to_vec(&credentials).unwrap())
            .unwrap();
        drop(session);
        let (mut endpoints, bind_gate) =
            if matches!(case, FaultCase::DelayedBind | FaultCase::StaleInspect) {
                let (endpoints, gate) = crate::daemon::TestEndpoints::start_with_response_gate(
                    engine.clone(),
                    crate::daemon::TestResponseGateScope {
                        actor: f.session.clone(),
                        project: f.project.clone(),
                        target: if case == FaultCase::StaleInspect {
                            crate::daemon::TestResponseGateTarget::SourceQualificationStatus {
                                dossier_id: source_qualification.dossier_id.clone(),
                            }
                        } else {
                            crate::daemon::TestResponseGateTarget::BindHandyHspLive
                        },
                        request_id: None,
                    },
                    Duration::from_secs(3),
                )
                .unwrap();
                (endpoints, Some(gate))
            } else {
                (
                    crate::daemon::TestEndpoints::start(engine.clone()).unwrap(),
                    None,
                )
            };
        let log_path = root.join("browser.raw.log");
        let log = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&log_path)
            .unwrap();
        let mut browser = BrowserChild(Some(
            ProcessCommand::new(&binary)
                .args(["browser-live", f.project.as_str(), "1"])
                .arg(&options_path)
                .arg("--handy-hsp")
                .arg(&hsp_config_path)
                .arg("--allow-handy-hsp-control")
                .args([
                    "--port",
                    "0",
                    "--duration-seconds",
                    "180",
                    "--media-lease-ms",
                    "30000",
                ])
                .env("PULSAR_STATE_DIR", &engine.config.state_dir)
                .env("PULSAR_WORKER_EXECUTABLE", &worker)
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        ));
        let started = Instant::now();
        let deadline = started + Duration::from_secs(u64::from(SERVER_SECONDS));
        let url = loop {
            assert!(browser.0.as_mut().unwrap().try_wait().unwrap().is_none());
            let text = fs::read_to_string(&log_path).unwrap();
            assert!(text.len() < 4 * 1024 * 1024);
            if let Some(line) = text
                .lines()
                .find_map(|line| line.strip_prefix("Browser preview: "))
            {
                let url = line.trim();
                assert!(
                    url.starts_with("http://127.0.0.1:")
                        && url.ends_with('/')
                        && !url.contains('?')
                        && !url.contains('#')
                );
                break url.to_owned();
            }
            bounded_wait(&root, started + Duration::from_secs(30));
        };
        record(
            &output,
            "ready.json",
            &json!({"ready":true,"case":case.name(),"page_url":url,"required_ui":case.required_ui(),"project_id":f.project,"engine_epoch":engine.engine_epoch,"state_is_private_tmpfs":true,"one_engine":true,"source_seconds":40,"capture_lease_ms":30000,"media_lease_ms":30000,"server_seconds":SERVER_SECONDS,"required_lead_ms":500,"required_continuity":case!=FaultCase::StaleInspect,"public_hsp_controls":true,"direct_fixture_hsp_actions":false,"preparation_timeout_ms":5000,"continuity_duration_ms":CONTINUITY_MS,"physical_device":false,"qualified":false}),
        );
        let first = loop {
            let (captures, _) = discover(f);
            let capture = captures.values().map(|id| snapshot(f, id)).find(|capture| {
                capture.active
                    && capture.options.browser_analysis.is_some()
                    && accepted_observation(f, capture).is_some()
            });
            if let Some(capture) = capture {
                break capture;
            }
            bounded_wait(&root, deadline);
        };
        let original_media = first.options.browser_media.as_ref().unwrap();
        let analysis = first.options.browser_analysis.as_ref().unwrap();
        assert_eq!(first.engine_epoch.as_str(), engine.engine_epoch.as_str());
        assert_eq!(original_media.lease_ms, 30000);
        assert_eq!(original_media.selection, contract_media.selection);
        assert_eq!(original_media.clock, contract_media.clock);
        assert_eq!(
            analysis.policy,
            BrowserAnalysisLookaheadPolicy {
                target_lead_us: 500_000,
                minimum_lead_us: 250_000,
                maximum_lead_us: 1_000_000
            }
        );
        assert_eq!(
            serde_json::to_value(&first.options.inference.as_ref().unwrap().profile).unwrap(),
            serde_json::to_value(&profile).unwrap()
        );
        record(
            &output,
            "first-browser-cpu.json",
            &accepted_observation(f, &first).unwrap(),
        );
        if case == FaultCase::StaleInspect {
            let mut proof = run_stale_inspect(StaleInspectRun {
                f,
                root: &root,
                output: &output,
                deadline,
                gate: bind_gate.as_ref().unwrap(),
                peer: &peer,
                io: &io,
                first: &first,
                dossier: &pending_source,
            });
            browser.finish();
            endpoints.finish().unwrap();
            shutdown_after_peer.finish();
            peer.finish();
            record(
                &output,
                "joined-worker-fence.json",
                &assert_joined_worker_fence(f),
            );
            check_effect_counts(&peer, case);
            assert_eq!(io.syncs.load(Ordering::Acquire), 0);
            record(
                &output,
                "localhost-requests.json",
                &peer.state.lock().unwrap().calls,
            );
            drop(endpoints);
            drop(engine);
            assert_eq!(Arc::strong_count(&x.f.engine), 1);
            let PreparationFixture { f, .. } = x;
            let Fixture { engine, dir, .. } = f;
            Engine::retire_for_test(engine);
            drop(dir);
            let value = proof.as_object_mut().unwrap();
            for field in [
                "passed",
                "browser_joined",
                "ipc_joined",
                "engine_joined",
                "localhost_peer_joined",
                "all_engine_workers_joined",
            ] {
                value.insert(field.into(), json!(true));
            }
            value.remove("awaiting_supervisor_stop");
            record(&output, "completion.json", &proof);
            return;
        }
        let mut withdrawal = Value::Null;
        let mut public_cancel_while_held = false;
        let mut interruption_chain_ids = None;
        let mut interruption_source_generations = Value::Null;
        let mut delayed_bind_proof = Value::Null;
        if case == FaultCase::PrepareCancel {
            let mut entered_recorded = false;
            loop {
                observe_ids(f, &mut originals, &mut cancel_id);
                assert!(
                    originals.stream.is_none()
                        && originals.play.is_none()
                        && originals.continuity.is_none(),
                    "pending preparation case must not Bind, Enable or Play"
                );
                assert_eq!(effect_counts(&peer), json!({"setup":0,"play":0,"stop":0}));
                if fault_io.entered.load(Ordering::Acquire) && !entered_recorded {
                    assert!(originals.preparation.is_some());
                    record(
                        &output,
                        "preparation-held.json",
                        &json!({"case":case.name(),"originals":ids_value(&originals,&cancel_id),"held":true,"original_timeout_ms":5000,"native_clock":true,"shared_locks_held":false}),
                    );
                    fault_stage(&output, case, "preparation-held", &originals, &cancel_id);
                    entered_recorded = true;
                    fault_stage(
                        &output,
                        case,
                        "awaiting-public-cancel",
                        &originals,
                        &cancel_id,
                    );
                }
                if let Some(prep) = &originals.preparation {
                    let value = public_preparation(f, prep);
                    if cancel_id.is_some()
                        && value.native.as_ref().is_some_and(|n| n.cancel_requested)
                    {
                        assert!(entered_recorded);
                        assert_eq!(fault_io.exit.load(Ordering::Acquire),0,"public Stop must cancel while the original preparation read is still held");
                        assert!(value.native.as_ref().unwrap().worker_active);
                        public_cancel_while_held = true;
                        record(
                            &output,
                            "public-cancel-while-held.json",
                            &json!({"preparation":value,"originals":ids_value(&originals,&cancel_id),"public_stop_cancelled_original":true,"native_gate_still_held":true,"direct_fixture_cancel":false}),
                        );
                        fault_io
                            .public_cancel_observed
                            .store(true, Ordering::Release);
                        break;
                    }
                }
                assert_eq!(
                    fault_io.exit.load(Ordering::Acquire),
                    0,
                    "preparation hold ended before actual public cancellation"
                );
                bounded_wait(&root, deadline);
            }
            loop {
                observe_ids(f, &mut originals, &mut cancel_id);
                let value = public_preparation(f, originals.preparation.as_ref().unwrap());
                if value.native.as_ref().is_some_and(|n| {
                    n.phase == HandyHspPreparationPhase::Retired
                        && n.cancel_requested
                        && !n.worker_active
                        && !n.metadata_pending
                }) {
                    assert_eq!(fault_io.exit.load(Ordering::Acquire), 2);
                    assert!(fault_io.late_read_returned.load(Ordering::Acquire));
                    assert!(value
                        .native
                        .as_ref()
                        .unwrap()
                        .clock_valid_until_boottime_ns
                        .is_none());
                    record(&output, "final-preparation.json", &value);
                    break;
                }
                bounded_wait(&root, deadline);
            }
            assert_eq!(io.syncs.load(Ordering::Acquire), 0);
            fault_stage(&output, case, "fault-observed", &originals, &cancel_id);
        } else if case == FaultCase::DelayedBind {
            let outcome = DelayedBindRun {
                f,
                root: &root,
                output: &output,
                deadline,
                gate: bind_gate.as_ref().unwrap(),
                peer: &peer,
                first: &first,
                originals: &mut originals,
                cancel_id: &mut cancel_id,
            }
            .run();
            delayed_bind_proof = outcome.proof;
            interruption_chain_ids = Some(outcome.chain_ids);
            interruption_source_generations = outcome.source_generations;
            assert_eq!(io.syncs.load(Ordering::Acquire), 1);
            fault_stage(&output, case, "fault-observed", &originals, &cancel_id);
        } else {
            fault_stage(
                &output,
                case,
                "awaiting-public-playing",
                &originals,
                &cancel_id,
            );
            let (origin, bind_id, enable_id, playing_source) = loop {
                observe_ids(f, &mut originals, &mut cancel_id);
                if let (Some(bind), Some(enable)) = (&originals.stream, &originals.continuity) {
                    let live = live_status(f, bind);
                    let clock = public_continuity(f, enable);
                    assert!(
                        live.error.is_none(),
                        "public HSP admission refused before Playing: {live:?}"
                    );
                    assert!(
                        clock.error.is_none(),
                        "public HSP continuity refused before Playing: {clock:?}"
                    );
                    if originals.play.is_some()
                        && live
                            .native
                            .as_ref()
                            .is_some_and(|n| !n.retired && n.error.is_none())
                        && clock
                            .native
                            .as_ref()
                            .is_some_and(|n| n.current && !n.retired)
                        && live
                            .stream
                            .as_ref()
                            .and_then(|s| s.native.as_ref())
                            .is_some_and(|n| {
                                n.runtime_state == HandyHspRuntimeState::Playing
                                    && n.post_setup_bound
                            })
                    {
                        assert_eq!(effect_counts(&peer), json!({"setup":1,"play":1,"stop":0}));
                        assert_eq!(
                            live.selection.capture_epoch, first.identity.epoch,
                            "original Bind selection changed its initial capture identity"
                        );
                        record(
                            &output,
                            "public-actions-playing.json",
                            &json!({"live":live,"clock":clock,"originals":ids_value(&originals,&cancel_id)}),
                        );
                        let playing_source = live
                            .native
                            .as_ref()
                            .unwrap()
                            .active_source
                            .as_ref()
                            .unwrap()
                            .clone();
                        if !case.withdrawal() {
                            interruption_chain(f, &output, "interruption-chain-at-playing.json");
                        }
                        break (
                            live.origin.unwrap(),
                            bind.clone(),
                            enable.clone(),
                            playing_source,
                        );
                    }
                }
                bounded_wait(&root, deadline);
            };
            record(
                &output,
                "public-action-history.json",
                &public_action_history(f, &originals),
            );
            fault_stage(
                &output,
                case,
                if case.withdrawal() {
                    "playing-awaiting-withdrawal"
                } else {
                    "playing-awaiting-interruption"
                },
                &originals,
                &cancel_id,
            );
            if case.withdrawal() {
                loop {
                    if root.join("FAULT").is_file() {
                        assert_eq!(
                            fs::read(root.join("FAULT")).unwrap(),
                            format!("{}\n", case.name()).as_bytes()
                        );
                        // The trigger is accepted only while this original stream
                        // is still Playing, never as a substitute for UI startup.
                        let live = live_status(f, &bind_id);
                        if live.native.as_ref().is_none_or(|n| n.retired)
                            || live
                                .stream
                                .as_ref()
                                .and_then(|s| s.native.as_ref())
                                .is_none_or(|n| n.runtime_state != HandyHspRuntimeState::Playing)
                        {
                            record(
                                &output,
                                "withdrawal-trigger-refused.json",
                                &json!({"case":case.name(),"live":live,"originals":ids_value(&originals,&cancel_id)}),
                            );
                            if let Some(id) = &originals.continuity {
                                let observed = engine
                                    .handle(request(
                                        &f.session,
                                        Some(&f.project),
                                        None,
                                        Command::HandyHspContinuityStatus {
                                            original_request_id: id.clone(),
                                            owner_proof: engine.handy_owner_token.clone(),
                                        },
                                    ))
                                    .result;
                                record(
                                    &output,
                                    "withdrawal-trigger-continuity.json",
                                    &json!({"case":case.name(),"original_request_id":id,"result":observed}),
                                );
                            }
                        }
                        assert!(live.native.as_ref().is_some_and(|n| !n.retired));
                        assert_eq!(
                            live.stream
                                .as_ref()
                                .and_then(|s| s.native.as_ref())
                                .unwrap()
                                .runtime_state,
                            HandyHspRuntimeState::Playing
                        );
                        break;
                    }
                    bounded_wait(&root, deadline);
                }
                let (kind, dossier_id, command) = if case == FaultCase::SourceWithdrawal {
                    (
                        "source",
                        pending_source.dossier_id.clone(),
                        request(
                            &f.session,
                            Some(&f.project),
                            None,
                            Command::WithdrawHandyHspSourceQualification {
                                dossier_id: pending_source.dossier_id.clone(),
                                expected_record_revision: pending_source.revision,
                                reason: "withdraw authored rendered fault applicability".into(),
                                owner_proof: engine.handy_owner_token.clone(),
                            },
                        ),
                    )
                } else {
                    (
                        "device",
                        x.accepted.dossier_id.clone(),
                        qualification_withdraw(f, &x.accepted.dossier_id, x.accepted.revision),
                    )
                };
                let response = engine.handle(command.clone()).result.unwrap();
                withdrawal = json!({"kind":kind,"dossier_id":dossier_id,"command_request_id":command.request_id,"response":response,"direct_fixture_withdrawal":true,"direct_fixture_hsp_action":false});
                record(&output, "withdrawal.json", &withdrawal);
                fault_stage(&output, case, "fault-observed", &originals, &cancel_id);
            } else {
                assert!(
                    !root.join("FAULT").exists(),
                    "this case accepts only its actual browser UI interruption"
                );
            }
            let final_live = loop {
                observe_ids(f, &mut originals, &mut cancel_id);
                let live = live_status(f, &bind_id);
                if !case.withdrawal() {
                    interruption_chain(f, &output, "interruption-chain-latest.json");
                }
                assert_eq!(live.origin.as_ref(), Some(&origin));
                if let Some(native) = live.stream.as_ref().and_then(|s| s.native.as_ref()) {
                    assert!(
                        !matches!(
                            native.runtime_state,
                            HandyHspRuntimeState::StopUnknown | HandyHspRuntimeState::StopUnclaimed
                        ),
                        "authored peer Stop did not acknowledge"
                    );
                    if native.runtime_state == HandyHspRuntimeState::StopAcknowledged
                        && !native.native_active
                        && !native.metadata_pending
                        && live
                            .native
                            .as_ref()
                            .is_some_and(|n| n.retired && n.original_stop_request_id.is_some())
                    {
                        assert!(originals.stop.is_some());
                        record(&output, "final-stream.json", &live);
                        break live;
                    }
                }
                bounded_wait(&root, deadline);
            };
            let final_clock =
                loop {
                    let value = public_continuity(f, &enable_id);
                    if value.native.as_ref().is_some_and(|n| {
                        n.retired && !n.current && n.worker_joined && !n.worker_active
                    }) {
                        record(&output, "final-clock-joined.json", &value);
                        break value;
                    }
                    bounded_wait(&root, deadline);
                };
            assert!(final_live.native.as_ref().unwrap().retired);
            assert!(final_clock.native.as_ref().unwrap().worker_joined);
            if !case.withdrawal() {
                record(
                    &output,
                    "interruption-source-generations.json",
                    &json!({"playing":playing_source,"final_stop":final_live.native.as_ref().and_then(|n|n.active_source.as_ref()),"originals":ids_value(&originals,&cancel_id)}),
                );
                loop {
                    let chain = interruption_chain(f, &output, "interruption-chain-latest.json");
                    let order = validate_interruption_chain(&chain, &first);
                    if chain.settled() {
                        let final_source = validate_final_source(
                            &chain,
                            &order,
                            &first,
                            &final_live,
                            &originals,
                            &serde_json::to_value(&origin).unwrap(),
                            &playing_source,
                        );
                        assert!(
                            !engine.is_shutting_down(),
                            "chain settlement must precede Engine teardown"
                        );
                        interruption_chain_ids = Some(chain.identities());
                        interruption_source_generations =
                            json!({"playing":playing_source,"final_stop":final_source});
                        record(
                            &output,
                            "interruption-chain-settled.json",
                            &json!({"chain":chain,"source_generations":interruption_source_generations,"ordered_epochs":order,"originals":ids_value(&originals,&cancel_id),"origin":origin,"before_engine_teardown":true,"all_original_captures_retired":true,"all_cpu_worker_custody_settled":true,"cpu_delivery_settled_by_native_owner_fence":true,"delivery_proof":"PendingNativeDelivery runs synchronously before its owning CPU worker can release tracked process/transport/storage custody; terminal transfer reports retain their original join disposition.","finite_input_closure":false,"actual_cua_interruption_evidence_required":true}),
                        );
                        break;
                    }
                    bounded_wait(&root, deadline);
                }
            }
            assert_eq!(io.syncs.load(Ordering::Acquire), 1);
            fault_stage(&output, case, "fault-observed", &originals, &cancel_id);
        }
        // Stream Stop may settle before the public cleanup lane records its
        // cancellation of the original preparation. That original cleanup is
        // required before freezing IDs; it is not a replacement lifecycle.
        loop {
            observe_ids(f, &mut originals, &mut cancel_id);
            let preparation = public_preparation(f, originals.preparation.as_ref().unwrap());
            record(&output, "public-cleanup-preparation.json", &preparation);
            if cancel_id.is_some()
                && preparation.native.as_ref().is_some_and(|n| {
                    n.phase == HandyHspPreparationPhase::Retired
                        && n.cancel_requested
                        && !n.worker_active
                        && !n.metadata_pending
                })
            {
                break;
            }
            bounded_wait(&root, deadline);
        }
        check_effect_counts(&peer, case);
        if case == FaultCase::DelayedBind {
            let proof = delayed_bind_proof.as_object_mut().unwrap();
            proof.insert("public_preparation_cancel_settled".into(), json!(true));
            proof.insert(
                "original_preparation_cancel_request_id".into(),
                json!(cancel_id),
            );
        }
        let retained_ids = ids_value(&originals, &cancel_id);
        let quiet_until = Instant::now() + Duration::from_millis(QUIET_MS);
        while Instant::now() < quiet_until {
            observe_ids(f, &mut originals, &mut cancel_id);
            assert_eq!(
                ids_value(&originals, &cancel_id),
                retained_ids,
                "fault cleanup must not admit an automatic replacement lifecycle"
            );
            check_effect_counts(&peer, case);
            if case == FaultCase::DelayedBind {
                check_delayed_bind_quiet(
                    f,
                    &output,
                    bind_gate.as_ref().unwrap(),
                    &originals,
                    &delayed_bind_proof,
                );
            }
            if let Some(expected) = &interruption_chain_ids {
                check_quiet_chain(f, &output, &first, expected);
            }
            bounded_wait(&root, deadline);
        }
        let ResponseBody::HandyQualification(device_dossier) =
            qualification_status(f, &x.accepted.dossier_id).unwrap()
        else {
            panic!("device dossier");
        };
        let ResponseBody::HandyHspSourceQualification(source_dossier) = engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::HandyHspSourceQualificationStatus {
                    dossier_id: pending_source.dossier_id.clone(),
                    owner_proof: engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("source dossier");
        };
        assert_eq!(
            device_dossier.state,
            if case == FaultCase::DeviceWithdrawal {
                HandyQualificationState::Withdrawn
            } else {
                HandyQualificationState::Pending
            }
        );
        assert_eq!(
            source_dossier.state,
            if case == FaultCase::SourceWithdrawal {
                HandyQualificationState::Withdrawn
            } else {
                HandyQualificationState::Pending
            }
        );
        assert_eq!(
            device_dossier.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        assert_eq!(
            source_dossier.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        record(
            &output,
            "final-dossiers.json",
            &json!({"device":device_dossier,"source":source_dossier}),
        );
        record(&output, "public-original-ids.json", &retained_ids);
        record(
            &output,
            "localhost-requests.json",
            &peer.state.lock().unwrap().calls,
        );
        if case.withdrawal() {
            withdrawal
                .as_object_mut()
                .unwrap()
                .insert("original_authority_retired".into(), json!(true));
            withdrawal
                .as_object_mut()
                .unwrap()
                .insert("fresh_admission_check".into(), json!("pending"));
            record(&output, "withdrawal.json", &withdrawal);
        }
        let coverage_scope = if case.withdrawal() {
            "original-retirement-only"
        } else {
            "original-fault-response"
        };
        let remaining_fault_scenarios: Vec<_> = [
            "delayed-bind",
            "stale-inspection",
            "readiness-expiry",
            "fresh-ui-lifecycle-after-history-only-restart",
            "fresh-explicit-withdrawn-selection-admission",
        ]
        .into_iter()
        .filter(|name| case != FaultCase::DelayedBind || *name != "delayed-bind")
        .collect();
        let counts = effect_counts(&peer);
        let syncs = io.syncs.load(Ordering::Acquire);
        record(
            &output,
            "result-ready.json",
            &json!({"case":case.name(),"software_checks_passed":true,"coverage_scope":coverage_scope,"public_ui_controls":true,"actual_rendered_browser":true,"actual_confined_cpu":true,"direct_fixture_hsp_actions":false,"withdrawal":withdrawal,"originals":retained_ids,"peer_effects":counts,"synchronizations":syncs,"public_stop_cancelled_held_preparation":public_cancel_while_held,"public_cancel_observed":public_cancel_while_held||case==FaultCase::DelayedBind,"late_completion_rejected":public_cancel_while_held,"public_play_observed":!matches!(case,FaultCase::PrepareCancel|FaultCase::DelayedBind),"delayed_bind":delayed_bind_proof,"original_runtime_retired":case!=FaultCase::PrepareCancel,"one_engine":true,"original_ids_retained":true,"no_automatic_readmission":true,"no_automatic_readmission_observation_ms":QUIET_MS,"interruption_chain_identities":interruption_chain_ids,"interruption_source_generations":interruption_source_generations,"interruption_chain_settled_before_teardown":interruption_chain_ids.is_some(),"awaiting_supervisor_stop":true,"history_only_restart":false,"qualified":false,"physical_device":false}),
        );
        fault_stage(
            &output,
            case,
            "awaiting-supervisor-stop",
            &originals,
            &cancel_id,
        );
        while !stopped(&root) {
            assert!(
                Instant::now() < deadline,
                "supervisor cleanup request missed original campaign deadline"
            );
            observe_ids(f, &mut originals, &mut cancel_id);
            assert_eq!(ids_value(&originals, &cancel_id), retained_ids);
            check_effect_counts(&peer, case);
            if case == FaultCase::DelayedBind {
                check_delayed_bind_quiet(
                    f,
                    &output,
                    bind_gate.as_ref().unwrap(),
                    &originals,
                    &delayed_bind_proof,
                );
            }
            if let Some(expected) = &interruption_chain_ids {
                check_quiet_chain(f, &output, &first, expected);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        browser.finish();
        endpoints.finish().unwrap();
        shutdown_after_peer.finish();
        peer.finish();
        let fence = assert_joined_worker_fence(f);
        record(&output, "joined-worker-fence.json", &fence);
        check_effect_counts(&peer, case);
        assert_eq!(io.syncs.load(Ordering::Acquire), syncs);
        record(
            &output,
            "localhost-requests.json",
            &peer.state.lock().unwrap().calls,
        );
        drop(endpoints);
        drop(engine);
        assert_eq!(Arc::strong_count(&x.f.engine), 1);
        let PreparationFixture { f, .. } = x;
        let Fixture { engine, dir, .. } = f;
        Engine::retire_for_test(engine);
        drop(dir);
        record(
            &output,
            "completion.json",
            &json!({"case":case.name(),"passed":true,"coverage_scope":coverage_scope,"actual_rendered_browser":true,"actual_confined_cpu":true,"public_ui_controls":true,"direct_fixture_hsp_actions":false,"withdrawal":withdrawal,"originals":retained_ids,"peer_effects":counts,"synchronizations":syncs,"public_stop_cancelled_held_preparation":public_cancel_while_held,"public_cancel_observed":public_cancel_while_held||case==FaultCase::DelayedBind,"late_completion_rejected":public_cancel_while_held,"public_play_observed":!matches!(case,FaultCase::PrepareCancel|FaultCase::DelayedBind),"delayed_bind":delayed_bind_proof,"original_runtime_retired":case!=FaultCase::PrepareCancel,"one_engine":true,"original_ids_retained":true,"no_automatic_readmission":true,"no_automatic_readmission_observation_ms":QUIET_MS,"interruption_chain_identities":interruption_chain_ids,"interruption_source_generations":interruption_source_generations,"interruption_chain_settled_before_teardown":interruption_chain_ids.is_some(),"history_only_restart":false,"all_engine_workers_joined":true,"browser_joined":true,"ipc_joined":true,"engine_joined":true,"localhost_peer_joined":true,"qualified":false,"physical_device":false,"remaining_fault_scenarios":remaining_fault_scenarios}),
        );
    }
}
