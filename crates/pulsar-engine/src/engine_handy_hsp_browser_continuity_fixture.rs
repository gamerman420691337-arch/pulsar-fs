// Included inside finite_tail_native_tests. A real browser and confined CPU
// worker feed this same Engine. Only localhost device behavior and acceptance
// contracts are authored. Durable dossiers remain Pending/Synthetic.
mod rendered_browser_continuity_fixture {
    use super::*;
    use crate::authority::handy_hsp_preparation::HspPreparationIo;
    use std::{
        collections::{BTreeMap, VecDeque},
        ffi::CString,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        process::{Child, Command as ProcessCommand, Stdio},
    };

    const SERVER_SECONDS: u16 = 180;
    const SYNC_CEILING_NS: u64 = 90_000_000_000;
    const PERMIT_NS: u64 = 120_000_000_000;
    const CONTINUITY_MS: u32 = 60_000;
    const CAPTURE_AGE_NS: u64 = 1_000_000_000;
    const CAPACITY: usize = 32;
    const MAX_REQUESTS: usize = 8192;
    const MAX_EVENTS: usize = 2048;

    fn ram_directory(path: &std::path::Path) {
        assert!(path.is_absolute());
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(
            metadata.permissions().mode() & 0o077,
            0,
            "private RAM directory required"
        );
        let path = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let mut info = std::mem::MaybeUninit::<libc::statfs>::uninit();
        assert_eq!(unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) }, 0);
        assert_eq!(
            unsafe { info.assume_init() }.f_type,
            libc::TMPFS_MAGIC,
            "fixture secrets must remain in tmpfs"
        );
    }
    fn record(root: &std::path::Path, name: &str, value: &impl serde::Serialize) {
        assert!(name.ends_with(".json") && !name.contains('/'));
        let tmp = root.join(format!(".{name}.tmp"));
        let bytes = serde_json::to_vec_pretty(value).unwrap();
        assert!(bytes.len() <= 8 * 1024 * 1024);
        fs::write(&tmp, bytes).unwrap();
        fs::rename(tmp, root.join(name)).unwrap();
    }
    fn stopped(root: &std::path::Path) -> bool {
        root.join("STOP").is_file()
    }
    fn server_ms() -> u64 {
        1_000_000 + crate::handy_clock::boottime_ns().unwrap() / 1_000_000
    }

    struct SustainedState {
        identity: HandyIdentity,
        mode_session: u64,
        stream: u32,
        queue: VecDeque<(i32, u32)>,
        tail: i32,
        consumed: i32,
        first: u32,
        last: u32,
        playing: bool,
        play: Option<(i32, u64)>,
        phase: i32,
        calls: Vec<Value>,
        setups: u32,
        plays: u32,
        stops: u32,
        peak_points: usize,
    }
    impl SustainedState {
        fn advance(&mut self) {
            if self.playing {
                let (start, server) = self.play.expect("original Play reference");
                let phase = i128::from(start) + i128::from(server_ms()) - i128::from(server);
                self.phase = i32::try_from(phase).expect("bounded authored playback clock");
                while self
                    .queue
                    .front()
                    .is_some_and(|(_, time)| i64::from(*time) <= i64::from(self.phase))
                {
                    self.consumed = self.queue.pop_front().unwrap().0;
                }
            }
        }
        fn remote(&mut self) -> HandyHspObservation {
            self.advance();
            HandyHspObservation {
                play_state: if self.playing {
                    HandyRemotePlayState::Playing
                } else {
                    HandyRemotePlayState::Stopped
                },
                stream_id: self.stream,
                current_time_ms: self.phase,
                points: self.queue.len().try_into().unwrap(),
                max_points: CAPACITY as u32,
                current_point: self.consumed,
                playback_rate: 1.0,
                looping: false,
                pause_on_starving: false,
                first_point_time_ms: self.queue.front().map_or(self.first, |p| p.1),
                last_point_time_ms: self.last,
                tail_point_stream_index: self.tail,
                tail_point_stream_index_threshold: 1,
            }
        }
        fn observation(&mut self) -> HandySetupDeviceObservation {
            let mut value = hsp_qualification_sample();
            value.identity = self.identity.clone();
            value.mode = HandyDeviceMode {
                mode: 4,
                mode_session_id: self.mode_session,
            };
            value.remote_hsp = Some(self.remote());
            value.validate().unwrap();
            value
        }
        fn wire(&mut self, method: &str, path: &str, body: &[u8]) -> Value {
            assert!(
                self.calls.len() < MAX_REQUESTS,
                "bounded sustained peer requests"
            );
            self.advance();
            let decoded: Value = if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(body).unwrap()
            };
            if method == "PUT" {
                match path {
                    "/hsp/setup" => {
                        self.setups += 1;
                        assert_eq!(self.setups, 1, "one original Setup");
                        self.stream =
                            u32::try_from(decoded["stream_id"].as_u64().unwrap()).unwrap();
                        assert_ne!(self.stream, 0);
                        self.mode_session = 8;
                        self.queue.clear();
                        self.tail = -1;
                        self.consumed = -1;
                        self.first = 0;
                        self.last = 0;
                        self.playing = false;
                        self.play = None;
                        self.phase = -10_000;
                    }
                    "/hsp/add" => {
                        assert_eq!(decoded["flush"], false);
                        let points = decoded["points"].as_array().unwrap();
                        assert!(!points.is_empty() && points.len() <= 2);
                        assert_eq!(
                            decoded["tail_point_stream_index"].as_i64().unwrap(),
                            i64::from(self.tail) + points.len() as i64
                        );
                        for point in points {
                            let at = u32::try_from(point["t"].as_u64().unwrap()).unwrap();
                            assert!(point["x"].as_u64().is_some_and(|x| x <= 100));
                            if self.tail >= 0 {
                                assert!(at > self.last, "original cumulative timestamps");
                            } else {
                                self.first = at;
                            }
                            self.tail = self.tail.checked_add(1).unwrap();
                            self.last = at;
                            self.queue.push_back((self.tail, at));
                        }
                        self.peak_points = self.peak_points.max(self.queue.len());
                        assert!(
                            self.queue.len() <= CAPACITY,
                            "fixed authored device ring capacity"
                        );
                    }
                    "/hsp/play" => {
                        self.plays += 1;
                        assert_eq!(self.plays, 1, "one original Play");
                        assert_eq!(decoded["playback_rate"], 1.0);
                        assert_eq!(decoded["loop"], false);
                        assert_eq!(decoded["pause_on_starving"], false);
                        self.play = Some((
                            i32::try_from(decoded["start_time"].as_i64().unwrap()).unwrap(),
                            decoded["server_time"].as_u64().unwrap(),
                        ));
                        self.playing = true;
                    }
                    "/hsp/stop" => {
                        self.stops += 1;
                        self.playing = false;
                    }
                    _ => panic!("unexpected authored peer PUT"),
                }
            } else {
                assert_eq!(method, "GET");
            }
            let remote = self.remote();
            self.calls.push(json!({"method":method,"path":path,"at_boottime_ns":crate::handy_clock::boottime_ns().unwrap(),"stream":self.stream,"tail":self.tail,"consumed":self.consumed,"points":remote.points,"playing":self.playing}));
            match path {
                "/info" => {
                    json!({"result":{"fw_version":self.identity.firmware,"fw_status":self.identity.firmware_status,"hw_model_no":self.identity.model_number,"hw_model_name":self.identity.model_name,"hw_model_variant":self.identity.model_variant,"session_id":self.identity.session_id}})
                }
                "/mode" => json!({"result":{"mode":4,"mode_session_id":self.mode_session}}),
                "/hsp/state" | "/hsp/setup" | "/hsp/add" | "/hsp/play" | "/hsp/stop" => {
                    json!({"result":{"play_state":if self.playing {1} else {2},"points":remote.points,"max_points":remote.max_points,"current_point":remote.current_point,"current_time":remote.current_time_ms,"loop":false,"playback_rate":1.0,"first_point_time":remote.first_point_time_ms,"last_point_time":remote.last_point_time_ms,"stream_id":remote.stream_id,"tail_point_stream_index":remote.tail_point_stream_index,"tail_point_stream_index_threshold":1,"pause_on_starving":false}})
                }
                _ => panic!("unexpected authored peer GET"),
            }
        }
    }
    struct SustainedPeer {
        origin: String,
        state: Arc<Mutex<SustainedState>>,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl SustainedPeer {
        fn new(identity: HandyIdentity) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let state = Arc::new(Mutex::new(SustainedState {
                identity,
                mode_session: 7,
                stream: 9,
                queue: VecDeque::new(),
                tail: -1,
                consumed: -1,
                first: 0,
                last: 0,
                playing: false,
                play: None,
                phase: -10_000,
                calls: Vec::new(),
                setups: 0,
                plays: 0,
                stops: 0,
                peak_points: 0,
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let closing = stop.clone();
            let shared = state.clone();
            let thread = std::thread::Builder::new()
                .name("pulsar-rendered-localhost-peer".into())
                .spawn(move || {
                    while !closing.load(Ordering::Acquire) {
                        let (mut socket, address) = match listener.accept() {
                            Ok(v) => v,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(2));
                                continue;
                            }
                            Err(_) => panic!("owned peer accept failed"),
                        };
                        assert!(address.ip().is_loopback());
                        socket
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let Some((method, path, body)) = read_request(&mut socket) else {
                            continue;
                        };
                        let bytes =
                            serde_json::to_vec(&shared.lock().unwrap().wire(&method, &path, &body))
                                .unwrap();
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        let _ = socket.write_all(header.as_bytes());
                        let _ = socket.write_all(&bytes);
                    }
                })
                .unwrap();
            Self {
                origin,
                state,
                stop,
                thread: Some(thread),
            }
        }
        fn finish(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(t) = self.thread.take() {
                t.join().unwrap();
            }
        }
    }
    impl Drop for SustainedPeer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
    struct SustainedIo {
        peer: Arc<Mutex<SustainedState>>,
        origin: String,
        syncs: AtomicU64,
        reads: AtomicU64,
    }
    impl SustainedIo {
        fn read(
            &self,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            if Instant::now() >= deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "authored read deadline elapsed",
                ));
            }
            let before = self.peer.lock().unwrap().observation();
            let mut sample = || -> PResult<HandyDeviceTimingSample> {
                let at = clock()?;
                Ok(HandyDeviceTimingSample {
                    info: AuthoredIo::info(at),
                    user_offset: HandyUserTimeOffset { offset_ms: 0 },
                    read_started_boottime_ns: at,
                    read_finished_boottime_ns: clock()?,
                })
            };
            let first = sample()?;
            let second = sample()?;
            let after = self.peer.lock().unwrap().observation();
            Ok(crate::handy_online::DeviceTimingRead {
                before,
                first,
                second,
                after,
            })
        }
    }
    impl HspPreparationIo for SustainedIo {
        fn now(&self) -> PResult<u64> {
            crate::handy_clock::boottime_ns()
        }
        fn inspect(
            &self,
            _: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.read(deadline, clock)
        }
        fn synchronize(
            &self,
            _: Arc<crate::handy_credentials::Credentials>,
            _: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            assert_eq!(
                self.syncs.fetch_add(1, Ordering::AcqRel),
                0,
                "refresh must never synchronize again"
            );
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
            _: Instant,
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
        fn inspect_hsp_stream(
            &self,
            _: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
            stream: u32,
        ) -> PResult<crate::handy_online::PlayingTimingRead> {
            let started_boottime_ns = clock()?;
            assert_eq!(self.peer.lock().unwrap().stream, stream);
            self.reads.fetch_add(1, Ordering::AcqRel);
            let device = self.read(deadline, clock)?;
            Ok(crate::handy_online::PlayingTimingRead {
                device,
                started_boottime_ns,
                finished_boottime_ns: clock()?,
            })
        }
    }
    fn rendered_source_dossier(
        x: &PreparationFixture,
        device_permit: &HandyQualificationBinding,
        media: &BrowserMediaBinding,
        profile: &DeclaredInferenceProfile,
    ) -> (
        HandyHspSourceQualificationBinding,
        HandyHspSourceQualificationReport,
    ) {
        let mut evidence = Vec::new();
        let mut environment = None;
        for kind in HandyHspSourceEvidenceKind::ALL
            .into_iter()
            .chain([HandyHspSourceEvidenceKind::FiniteInputClosure])
        {
            let path = x.f.dir.path().join(format!("native-source-{kind:?}.txt"));
            fs::write(&path, format!("AUTHORED SOFTWARE FIXTURE: {kind:?}. Actual rendered browser supplies rVFC timing and RGB via normal HTTP/IPC; timing error assumptions and localhost device remain authored, not physical qualification.\n")).unwrap();
            let id = artifact(&path);
            if kind == HandyHspSourceEvidenceKind::EnvironmentConfiguration {
                environment = Some(id.clone());
            }
            evidence.push(HandyHspSourceQualificationInputEvidence {
                kind,
                path,
                sha256: id.sha256,
                byte_len: id.byte_len,
            });
        }
        let input = HandyHspSourceQualificationInput {
            subject: HandyHspSourceSubject {
                cpu_runtime_image: None,
                schema_version: 1,
                finite_drain_policy_sha256: x
                    .accepted
                    .manifest
                    .subject
                    .hsp_finite_drain_policy_sha256()
                    .unwrap(),
                origin: HandyEvidenceOrigin::Synthetic,
                context: x.accepted.manifest.subject.context.clone(),
                device_qualification: device_permit.clone(),
                browser_environment: environment.unwrap(),
                project_id: x.f.project.clone(),
                revision: RevisionId::new(1),
                selection: media.selection.clone(),
                clock: media.clock,
                inference_profile: profile.clone(),
                source_time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
                recipe: HandyHspSourceRecipe::RvfcRgb64x64NearestStretchV1,
                age_basis: LiveCaptureAgeBasis::ProducerDeclared,
                age_observation: HandyHspSourceAgeObservation::HttpReceiptMonotonic,
                lookahead: BrowserAnalysisLookaheadPolicy {
                    target_lead_us: 500_000,
                    minimum_lead_us: 250_000,
                    maximum_lead_us: 1_000_000,
                },
                maximum_analysis_age_ns: CAPTURE_AGE_NS,
                maximum_capture_to_engine_receipt_ns: 50_000_000,
                timing: HandyBrowserStartTiming {
                    clock: BrowserClockAssumptions {
                        timestamp_error_ns: 1_000,
                        relative_rate_error_ppm: 20,
                        maximum_age_ns: 1_000_000_000,
                        maximum_interval_ns: 100_000_000,
                    },
                    reference: HandyBrowserStartReference::Callback,
                    display_early_ns: 2_000_000,
                    display_late_ns: 3_000_000,
                    source_correspondence_error_ns: 1_000_000,
                    discrete_media_phase_error_ns: 1_000_000,
                    media_rate_error_ppm: 20,
                    physical_execution_error_ns: 1_000_000,
                    maximum_start_age_ns: 500_000_000,
                    maximum_alignment_error_ns: 50_000_000,
                },
            },
            evidence,
        };
        input.validate().unwrap();
        let path = x.f.dir.path().join("native-source-dossier.json");
        let bytes = serde_json::to_vec(&input).unwrap();
        fs::write(&path, &bytes).unwrap();
        let ResponseBody::HandyHspSourceQualificationSaved(receipt) =
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::ImportHandyHspSourceQualification {
                        path,
                        manifest_sha256: format!("{:x}", Sha256::digest(&bytes)),
                        owner_proof: x.f.engine.handy_owner_token.clone(),
                    },
                ))
                .result
                .unwrap()
        else {
            panic!("source import");
        };
        assert_eq!(receipt.report.state, HandyQualificationState::Pending);
        assert_eq!(
            receipt.report.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let binding =
            handy_hsp_source_qualification::install_authored_hsp_source_acceptance_for_test(
                &x.f.engine.db.lock().unwrap(),
                &receipt.report.dossier_id,
            )
            .unwrap();
        (binding, receipt.report)
    }
    fn device_fixture(
        f: Fixture,
    ) -> (
        PreparationFixture,
        SustainedPeer,
        Arc<SustainedIo>,
        HandyQualificationBinding,
    ) {
        let (path, mut input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Synthetic);
        let s = &mut input.subject;
        s.profile.maximum_command_age_ns = 10_000_000_000;
        s.profile.maximum_clock_uncertainty_ns = 100_000_000;
        s.server_clock.as_mut().unwrap().timestamp_error_ns = 1_000_000;
        let device = s.device_clock.as_mut().unwrap();
        device.maximum_sync_age_ns = SYNC_CEILING_NS;
        device.device_error_ns = 1_000;
        device.maximum_sync_round_trip_ms = 10;
        let p = policy();
        let hsp = s.hsp.as_mut().unwrap();
        hsp.policy = HandyHspPolicy {
            max_buffered_points: p.max_buffered_points as u32,
            device_capacity_points: p.device_capacity_points as u32,
            max_chunk_points: p.max_chunk_points as u32,
            min_lead_ms: p.min_lead_ms,
            max_future_ms: p.max_future_ms,
            max_sample_age_ns: p.max_sample_age_ns,
            max_gap_ms: p.max_gap_ms,
        };
        hsp.finite_drain = Some(HandyHspFiniteDrainQualification {
            schema_version: 1,
            model: HandyHspFiniteDrainModel::EarliestPhaseTailThenOwnedStop,
            maximum_drain_duration_ns: 3_000_000_000,
            maximum_terminal_overrun_ns: 250_000_000,
            maximum_stop_dispatch_delay_ns: 100_000_000,
        });
        let evidence_path = f.dir.path().join("rendered-authored-device-contract.txt");
        fs::write(&evidence_path,b"AUTHORED SOFTWARE ONLY. Fixed32 queue, one original Play reference, native-boottime authored server offset, 90second initial synchronization ceiling,120second TEMP permit,60second continuity admission. No real Handy, physical response or stopping bound.\n").unwrap();
        let evidence = artifact(&evidence_path);
        input.evidence.push(HandyQualificationInputEvidence {
            kind: HandyEvidenceKind::FiniteTailDrain,
            path: evidence_path,
            sha256: evidence.sha256,
            byte_len: evidence.byte_len,
        });
        input.validate().unwrap();
        let pending = qualification_saved(
            f.engine
                .handle(qualification_import(
                    &f,
                    "rendered-device-dossier",
                    &path,
                    &input,
                ))
                .result
                .unwrap(),
        );
        assert_eq!(pending.state, HandyQualificationState::Pending);
        assert_eq!(
            pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let (_, connection) = hsp_qualification_connection(&f);
        let permit = handy_qualification::install_authored_hsp_device_acceptance_for_test(
            &f.engine.db.lock().unwrap(),
            &pending.dossier_id,
            crate::handy_clock::boottime_ns()
                .unwrap()
                .checked_add(PERMIT_NS)
                .unwrap(),
        )
        .unwrap();
        let peer = SustainedPeer::new(pending.manifest.subject.identity.clone());
        let io = Arc::new(SustainedIo {
            peer: peer.state.clone(),
            origin: peer.origin.clone(),
            syncs: AtomicU64::new(0),
            reads: AtomicU64::new(0),
        });
        (
            PreparationFixture {
                f,
                accepted: pending,
                connection,
            },
            peer,
            io,
            permit,
        )
    }
    struct BrowserChild(Option<Child>);
    impl BrowserChild {
        fn finish(&mut self) {
            if let Some(mut child) = self.0.take() {
                if child.try_wait().unwrap().is_none() {
                    unsafe {
                        libc::kill(child.id().try_into().unwrap(), libc::SIGTERM);
                    }
                }
                let end = Instant::now() + Duration::from_secs(5);
                while child.try_wait().unwrap().is_none() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if child.try_wait().unwrap().is_none() {
                    child.kill().unwrap();
                }
                let _ = child.wait().unwrap();
            }
        }
    }
    impl Drop for BrowserChild {
        fn drop(&mut self) {
            self.finish();
        }
    }
    fn ui_stage(output: &std::path::Path, stage: &str, ids: &UiOriginals) {
        record(
            output,
            "ui-stage.json",
            &json!({"stage":stage,"originals":ids,
            "observed_at_boottime_ns":crate::handy_clock::boottime_ns().unwrap(),
            "public_ui_controls":true,"qualified":false,"physical_device":false}),
        );
    }
    fn prepare_contract_media(f: &Fixture) -> BrowserMediaBinding {
        let ResponseBody::BrowserMediaCatalog(catalog) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("original source catalog")
        };
        assert_eq!(catalog.entries.len(), 1);
        let intent = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(0, 1_000_000).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1_000_000).unwrap(),
                },
                lease_ms: 30_000,
                allow_declared_clock: true,
            },
        );
        f.engine.handle(intent.clone()).result.unwrap();
        let until = Instant::now() + WAIT;
        loop {
            let ResponseBody::BrowserMediaPreparation(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: intent.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("contract media preparation")
            };
            if report.state == BrowserMediaPreparationState::Ready {
                return report.binding.unwrap();
            }
            assert_eq!(
                report.state,
                BrowserMediaPreparationState::Pending,
                "{report:?}"
            );
            assert!(
                Instant::now() < until,
                "original contract media did not prepare"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[derive(Clone, Default, serde::Serialize)]
    struct UiOriginals {
        preparation: Option<RequestId>,
        stream: Option<RequestId>,
        play: Option<RequestId>,
        continuity: Option<RequestId>,
        stop: Option<RequestId>,
    }
    fn retain_ui_id(slot: &mut Option<RequestId>, observed: &RequestId) {
        if let Some(original) = slot {
            assert_eq!(
                original, observed,
                "positive UI run replaced an original action"
            );
        } else {
            *slot = Some(observed.clone());
        }
    }
    fn discover_ui(f: &Fixture, ids: &mut UiOriginals) {
        let db = f.engine.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT response FROM requests WHERE session=?1 LIMIT 4097")
            .unwrap();
        let rows = query
            .query_map([f.session.as_str()], |row| row.get::<_, String>(0))
            .unwrap();
        let mut total = 0;
        let mut selected_preparation = None;
        for (index, row) in rows.enumerate() {
            assert!(index < 4096);
            let raw = row.unwrap();
            total += raw.len();
            assert!(raw.len() <= 1024 * 1024 && total <= 32 * 1024 * 1024);
            match serde_json::from_str::<ResponseBody>(&raw).unwrap() {
                ResponseBody::HandyHspPreparation(value) if value.project_id == f.project => {
                    retain_ui_id(&mut ids.preparation, &value.preparation_request_id)
                }
                ResponseBody::HandyHspLive(value) if value.project_id == f.project => {
                    retain_ui_id(&mut ids.stream, &value.stream_request_id);
                    retain_ui_id(
                        &mut selected_preparation,
                        &value.selection.preparation_request_id,
                    );
                }
                ResponseBody::HandyHspContinuity(value) if value.project_id == f.project => {
                    retain_ui_id(&mut ids.continuity, &value.enable_request_id);
                    retain_ui_id(&mut ids.stream, &value.stream_request_id);
                }
                _ => {}
            }
        }
        if let Some(selected) = selected_preparation {
            assert_eq!(ids.preparation.as_ref(), Some(&selected));
        }
    }
    fn public_preparation(f: &Fixture, id: &RequestId) -> HandyHspPreparationStatus {
        let ResponseBody::HandyHspPreparation(value) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::HandyHspPreparationStatus {
                    preparation_request_id: id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("original public preparation status")
        };
        *value
    }
    fn public_action_history(f: &Fixture, ids: &UiOriginals) -> Value {
        let db = f.engine.db.lock().unwrap();
        let mut observed = Vec::new();
        for (kind, id) in [
            ("prepare", ids.preparation.as_ref().unwrap()),
            ("bind", ids.stream.as_ref().unwrap()),
            ("play", ids.play.as_ref().unwrap()),
            ("enable", ids.continuity.as_ref().unwrap()),
        ] {
            let raw: String = db
                .query_row(
                    "SELECT response FROM requests WHERE session=?1 AND request=?2",
                    [f.session.as_str(), id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(raw.len() <= 1024 * 1024);
            match serde_json::from_str::<ResponseBody>(&raw).unwrap() {
                ResponseBody::HandyHspPreparation(value) if kind == "prepare" => {
                    assert_eq!(value.project_id, f.project);
                    assert_eq!(&value.preparation_request_id, id);
                }
                ResponseBody::HandyHspLive(value) if kind == "bind" || kind == "play" => {
                    assert_eq!(value.project_id, f.project);
                    assert_eq!(Some(&value.stream_request_id), ids.stream.as_ref());
                }
                ResponseBody::HandyHspContinuity(value) if kind == "enable" => {
                    assert_eq!(value.project_id, f.project);
                    assert_eq!(&value.enable_request_id, id);
                    assert_eq!(Some(&value.stream_request_id), ids.stream.as_ref());
                }
                _ => panic!("original UI history kind differs"),
            }
            observed.push(json!({"action":kind,"original_request_id":id,"actor":f.session,"project":f.project}));
        }
        json!(observed)
    }
    fn discover(f: &Fixture) -> (BTreeMap<String, LiveIdentity>, BTreeMap<String, RequestId>) {
        let db = f.engine.db.lock().unwrap();
        let mut query = db
            .prepare("SELECT response FROM requests WHERE session=?1 LIMIT 4097")
            .unwrap();
        let rows = query
            .query_map([f.session.as_str()], |r| r.get::<_, String>(0))
            .unwrap();
        let mut captures = BTreeMap::new();
        let mut transfers = BTreeMap::new();
        let mut bytes = 0;
        for (index, row) in rows.enumerate() {
            assert!(index < 4096);
            let raw = row.unwrap();
            bytes += raw.len();
            assert!(raw.len() <= 1024 * 1024 && bytes <= 32 * 1024 * 1024);
            let value: Value = serde_json::from_str(&raw).unwrap();
            let payload = &value["value"];
            let capture = match value["kind"].as_str() {
                Some("live_capture") => Some(payload),
                Some("browser_analysis_successor") => Some(&payload["successor"]),
                _ => None,
            };
            if let Some(capture) = capture {
                let identity: LiveIdentity =
                    serde_json::from_value(capture["identity"].clone()).unwrap();
                if identity.project == f.project {
                    captures.insert(identity.epoch.to_string(), identity);
                }
            }
            if value["kind"] == "browser_analysis_transfer" {
                let id: RequestId =
                    serde_json::from_value(payload["original_request_id"].clone()).unwrap();
                transfers.insert(id.to_string(), id);
            }
        }
        assert!(captures.len() <= 8 && transfers.len() <= 8);
        (captures, transfers)
    }
    fn snapshot(f: &Fixture, identity: &LiveIdentity) -> LiveCaptureReport {
        capture_report(
            f.engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    Some(identity.revision.value()),
                    Command::LiveCaptureStatus {
                        epoch: identity.epoch.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }
    fn accepted_observation(f: &Fixture, capture: &LiveCaptureReport) -> Option<Value> {
        let registry = f.engine.live_captures.lock().unwrap();
        let stamp = registry
            .native_accepted(&f.session, &capture.identity)
            .ok()
            .flatten()?;
        let LiveInferenceReply::Estimated {
            input, position, ..
        } = &stamp.estimate.result
        else {
            return None;
        };
        Some(
            json!({"epoch":capture.identity.epoch,"previous_frame":stamp.previous_frame,"frame":stamp.frame,"previous_rgb_sha256":input.previous.sha256,"current_rgb_sha256":input.current.sha256,"position":position,"capture_bounds":[stamp.capture_boottime.earliest_ns,stamp.capture_boottime.latest_ns],"accepted_bounds":[stamp.accepted_boottime.earliest_ns,stamp.accepted_boottime.latest_ns],"native_continuity_generation":stamp.continuity_generation,"actual_cpu":true,"qualified":false}),
        )
    }
    fn public_continuity(f: &Fixture, id: &RequestId) -> HandyHspContinuityStatus {
        let ResponseBody::HandyHspContinuity(value) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::HandyHspContinuityStatus {
                    original_request_id: id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("continuity status")
        };
        *value
    }
    fn bounded_wait(root: &std::path::Path, deadline: Instant) {
        assert!(!stopped(root), "manual Stop before completion");
        assert!(
            Instant::now() < deadline,
            "original rendered fixture deadline elapsed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    #[test]
    #[ignore = "requires explicit private tmpfs supervisor, pinned app/worker/runtime/40second neutral media and real CUA browser interaction; never run as a default test"]
    fn native_rendered_browser_hsp_public_ui_continuity() {
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
        engine
            .install_handy_hsp_preparation_io_for_test(
                &f.session,
                &x.connection.engine_epoch,
                &x.connection.device_session,
                io.clone(),
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
        ui_stage(
            &output,
            "synthetic_environment_ready_for_public_ui",
            &originals,
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
        let mut endpoints = crate::daemon::TestEndpoints::start(engine.clone()).unwrap();
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
            &json!({"ready":true,"page_url":url,"project_id":f.project,"engine_epoch":engine.engine_epoch,"state_is_private_tmpfs":true,"one_engine":true,"source_seconds":40,"capture_lease_ms":30000,"media_lease_ms":30000,"server_seconds":SERVER_SECONDS,"required_lead_ms":500,"required_continuity":true,"public_hsp_controls":true,"direct_fixture_hsp_actions":false,"preparation_timeout_ms":5000,"continuity_duration_ms":CONTINUITY_MS,"physical_device":false,"qualified":false}),
        );
        let first = loop {
            let (captures, _) = discover(f);
            let mut eligible = None;
            for identity in captures.values() {
                let capture = snapshot(f, identity);
                if capture.active
                    && capture.options.browser_analysis.is_some()
                    && accepted_observation(f, &capture).is_some()
                {
                    eligible = Some(capture);
                    break;
                }
            }
            if let Some(capture) = eligible {
                break capture;
            }
            bounded_wait(&root, deadline);
        };
        let original_seen = Instant::now();
        assert_eq!(first.engine_epoch.as_str(), engine.engine_epoch.as_str());
        let original_media = first.options.browser_media.clone().unwrap();
        let analysis = first.options.browser_analysis.clone().unwrap();
        assert_eq!(original_media.lease_ms, 30000);
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
        let original_player = analysis.player.clone();
        record(
            &output,
            "first-browser-cpu.json",
            &accepted_observation(f, &first).unwrap(),
        );
        assert_eq!(original_media.selection, contract_media.selection);
        assert_eq!(original_media.clock, contract_media.clock);
        assert_ne!(
            original_media.preparation_request_id,
            contract_media.preparation_request_id
        );
        ui_stage(&output, "awaiting_ui_inspect_and_prepare", &originals);
        // These loops only discover original public UI intents and observe status.
        // No fixture Prepare, Bind, Play, Enable or replacement action is sent.
        loop {
            discover_ui(f, &mut originals);
            if let Some(id) = &originals.preparation {
                let value = public_preparation(f, id);
                assert!(
                    value.native.as_ref().is_none_or(|n| n.error.is_none()),
                    "public preparation refused: {value:?}"
                );
                if value.native.as_ref().is_some_and(|n| {
                    n.phase == HandyHspPreparationPhase::Ready
                        && !n.worker_active
                        && !n.metadata_pending
                }) {
                    record(&output, "preparation-ready.json", &value);
                    break;
                }
            }
            bounded_wait(&root, deadline);
        }
        ui_stage(&output, "awaiting_ui_bind", &originals);
        let origin = loop {
            discover_ui(f, &mut originals);
            if let Some(id) = &originals.stream {
                let value = live_status(f, id);
                assert!(value.error.is_none(), "public Bind refused: {value:?}");
                assert_eq!(
                    value.selection.capture_epoch, first.identity.epoch,
                    "positive UI Bind must precede the first original capture transfer"
                );
                if value
                    .stream
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| {
                        n.post_setup_bound
                            && !n.native_active
                            && !n.metadata_pending
                            && n.total_added >= 2
                    })
                {
                    record(&output, "bound-and-prefilled.json", &value);
                    break value.origin.unwrap();
                }
            }
            bounded_wait(&root, deadline);
        };
        let bind_id = originals.stream.as_ref().unwrap().clone();
        ui_stage(
            &output,
            "awaiting_ui_continuity_enable_and_play",
            &originals,
        );
        let mut last_ui_diagnostic = Instant::now() - Duration::from_secs(1);
        let mut ui_diagnostic_count = 0u32;
        let enable_id = loop {
            discover_ui(f, &mut originals);
            let stream = live_status(f, &bind_id);
            if last_ui_diagnostic.elapsed() >= Duration::from_millis(500) {
                let clock = originals
                    .continuity
                    .as_ref()
                    .map(|id| public_continuity(f, id));
                let (identities, _) = discover(f);
                let captures: Vec<_> = identities.values().map(|id| snapshot(f, id)).collect();
                record(
                    &output,
                    &format!("ui-admission-{ui_diagnostic_count:03}.json"),
                    &json!({"elapsed_ms":started.elapsed().as_millis(),"originals":originals,"live":stream,"clock":clock,"captures":captures,"qualified":false}),
                );
                ui_diagnostic_count += 1;
                assert!(ui_diagnostic_count <= 361);
                last_ui_diagnostic = Instant::now();
            }
            assert_eq!(stream.origin.as_ref(), Some(&origin));
            assert!(
                stream.error.is_none(),
                "public HSP action refused: {stream:?}"
            );
            if let Some(play) = stream
                .native
                .as_ref()
                .and_then(|n| n.original_play_request_id.as_ref())
            {
                retain_ui_id(&mut originals.play, play);
            }
            if let Some(id) = &originals.continuity {
                let clock = public_continuity(f, id);
                assert!(
                    clock.error.is_none(),
                    "public continuity refused: {clock:?}"
                );
                if clock
                    .native
                    .as_ref()
                    .is_some_and(|n| n.current && !n.retired)
                    && stream
                        .stream
                        .as_ref()
                        .and_then(|s| s.native.as_ref())
                        .is_some_and(|n| {
                            n.runtime_state == HandyHspRuntimeState::Playing
                                && !n.native_active
                                && !n.metadata_pending
                        })
                {
                    assert!(originals.play.is_some());
                    record(
                        &output,
                        "public-actions-playing.json",
                        &json!({"live":stream,"clock":clock}),
                    );
                    break id.clone();
                }
            }
            bounded_wait(&root, deadline);
        };
        record(&output, "public-original-ids.json", &originals);
        record(
            &output,
            "public-action-history.json",
            &public_action_history(f, &originals),
        );
        ui_stage(
            &output,
            "public_ui_playing_observing_actual_transfers",
            &originals,
        );
        let mut captures = BTreeMap::new();
        let mut transfer_ids = BTreeMap::new();
        let mut transfers = BTreeMap::new();
        let mut events = Vec::new();
        let mut signatures = BTreeMap::new();
        let mut accepted_epochs = std::collections::BTreeSet::new();
        let mut crossed_media = false;
        let mut max_generation = 0;
        let mut last_census = Instant::now() - Duration::from_secs(2);
        let mut sealed = None;
        let mut original_ceiling = None;
        loop {
            if last_census.elapsed() >= Duration::from_millis(500) {
                let (c, t) = discover(&x.f);
                captures.extend(c);
                transfer_ids.extend(t);
                last_census = Instant::now();
            }
            let stream = live_status(&x.f, &bind_id);
            assert_eq!(stream.origin.as_ref(), Some(&origin));
            assert_eq!(stream.selection.capture_epoch, first.identity.epoch);
            assert_eq!(
                Some(&stream.selection.preparation_request_id),
                originals.preparation.as_ref()
            );
            if let Some(native) = &stream.native {
                assert_eq!(
                    native.original_play_request_id.as_ref(),
                    originals.play.as_ref()
                );
                if let Some(stop) = &native.original_stop_request_id {
                    retain_ui_id(&mut originals.stop, stop);
                }
            }
            assert!(stream.error.is_none());
            let clock = public_continuity(&x.f, &enable_id);
            if let Some(history) = &clock.history {
                if let Some(expected) = original_ceiling {
                    assert_eq!(history.synchronization_ceiling_boottime_ns, expected);
                } else {
                    original_ceiling = Some(history.synchronization_ceiling_boottime_ns);
                }
            }
            if let Some(native) = &clock.native {
                max_generation = max_generation.max(native.generation);
            }
            for identity in captures.values() {
                let capture = snapshot(&x.f, identity);
                if let Some(declared) = &capture.options.browser_analysis {
                    assert_eq!(declared.player, original_player);
                    assert_eq!(declared.policy, analysis.policy);
                    assert_eq!(
                        serde_json::to_value(&capture.options.inference.as_ref().unwrap().profile)
                            .unwrap(),
                        serde_json::to_value(&profile).unwrap()
                    );
                    assert_eq!(
                        capture.options.browser_media.as_ref().unwrap().clock,
                        original_media.clock
                    );
                    assert_eq!(
                        capture.options.browser_media.as_ref().unwrap().selection,
                        original_media.selection
                    );
                }
                if capture.active
                    && capture.identity.epoch != first.identity.epoch
                    && original_seen.elapsed()
                        > Duration::from_millis(u64::from(original_media.lease_ms))
                    && stream
                        .stream
                        .as_ref()
                        .and_then(|s| s.native.as_ref())
                        .is_some_and(|n| n.runtime_state == HandyHspRuntimeState::Playing)
                {
                    crossed_media = true;
                }
                if capture.input_close.as_ref().is_some_and(|close| {
                    close.phase == LiveCaptureInputPhase::Sealed
                        && close.worker_joined
                        && close.delivery_settled
                        && close.disposition == Some(LiveCaptureInputFinalDisposition::Accepted)
                }) {
                    sealed = Some(capture.clone());
                }
                if !accepted_epochs.contains(identity.epoch.as_str()) {
                    if let Some(stamp) = accepted_observation(&x.f, &capture) {
                        accepted_epochs.insert(identity.epoch.to_string());
                        events.push(json!({"elapsed_ms":started.elapsed().as_millis(),"actual_cpu_stamp":stamp}));
                    }
                }
                let signature = (
                    capture.active,
                    capture.retirement.clone(),
                    capture.input_close.clone(),
                );
                if signatures.get(identity.epoch.as_str()) != Some(&signature) {
                    signatures.insert(identity.epoch.to_string(), signature);
                    events.push(json!({"elapsed_ms":started.elapsed().as_millis(),"capture":capture,"actual_cpu_stamp":accepted_observation(&x.f,&capture)}));
                }
            }
            for (id, request_id) in &transfer_ids {
                let ResponseBody::BrowserAnalysisTransfer(value) = engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        None,
                        Command::BrowserAnalysisTransferStatus {
                            transfer_request_id: request_id.clone(),
                        },
                    ))
                    .result
                    .unwrap()
                else {
                    panic!("transfer report")
                };
                assert_ne!(value.phase, BrowserAnalysisTransferPhase::Aborted);
                if value.phase == BrowserAnalysisTransferPhase::Transferred {
                    assert!(
                        value.predecessor_worker_joined
                            && value.predecessor_delivery_settled
                            && value.successor_pair_accepted
                    );
                    transfers.insert(id.clone(), *value);
                }
            }
            assert!(events.len() < MAX_EVENTS);
            record(
                &output,
                "progress.json",
                &json!({"elapsed_ms":started.elapsed().as_millis(),"captures":captures.len(),"transfers":transfers.len(),"crossed_original_media_deadline":crossed_media,"clock_generation":max_generation,"stream":stream,"clock":clock,"qualified":false}),
            );
            if let Some(native) = stream.stream.as_ref().and_then(|s| s.native.as_ref()) {
                if native.runtime_state == HandyHspRuntimeState::StopAcknowledged
                    && !native.native_active
                    && !native.metadata_pending
                {
                    if transfers.len() != 2 {
                        let final_captures: Vec<_> =
                            captures.values().map(|id| snapshot(&x.f, id)).collect();
                        let remote = peer.state.lock().unwrap();
                        record(
                            &output,
                            "early-stop-native.json",
                            &json!({
                                "live":stream,"clock":clock,"captures":final_captures,"events":events,
                                "peer":{"setups":remote.setups,"plays":remote.plays,"stops":remote.stops,"tail":remote.tail,"consumed":remote.consumed,"points":remote.queue.len(),"calls":remote.calls},
                                "qualified":false
                            }),
                        );
                    }
                    assert_eq!(
                        transfers.len(),
                        2,
                        "actual browser must complete two native transfers"
                    );
                    assert!(crossed_media);
                    assert!(
                        accepted_epochs.len() >= 3,
                        "each epoch requires a real accepted CPU result"
                    );
                    assert!(max_generation >= 3);
                    let final_capture = sealed
                        .as_ref()
                        .expect("actual final browser input must seal");
                    let active_source = stream
                        .native
                        .as_ref()
                        .unwrap()
                        .active_source
                        .as_ref()
                        .unwrap();
                    assert_eq!(active_source.generation, 3);
                    assert_eq!(active_source.capture_epoch, final_capture.identity.epoch);
                    let drain = native
                        .finite_drain
                        .as_ref()
                        .expect("selected original finite drain");
                    assert!(!drain.stop_deadline_missed);
                    assert!(drain.tail_reached_at_boottime_ns.is_some());
                    record(&output, "final-capture.json", final_capture);
                    record(&output, "final-stream.json", &stream);
                    record(&output, "final-clock.json", &clock);
                    break;
                }
                assert!(
                    !matches!(
                        native.runtime_state,
                        HandyHspRuntimeState::StopUnknown | HandyHspRuntimeState::StopUnclaimed
                    ),
                    "local peer Stop did not acknowledge"
                );
            }
            bounded_wait(&root, deadline);
        }
        let final_clock = loop {
            let value = public_continuity(&x.f, &enable_id);
            if value
                .native
                .as_ref()
                .is_some_and(|n| n.worker_joined && !n.worker_active)
            {
                break value;
            }
            bounded_wait(&root, deadline);
        };
        record(&output, "final-clock-joined.json", &final_clock);
        ui_stage(&output, "public_ui_finite_tail_stop_observed", &originals);
        let ResponseBody::HandyQualification(device_still_pending) =
            qualification_status(&x.f, &x.accepted.dossier_id).unwrap()
        else {
            panic!("retained device dossier")
        };
        assert_eq!(device_still_pending.state, HandyQualificationState::Pending);
        assert_eq!(
            device_still_pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let ResponseBody::HandyHspSourceQualification(source_still_pending) = engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::HandyHspSourceQualificationStatus {
                    dossier_id: pending_source.dossier_id.clone(),
                    owner_proof: engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("retained source dossier")
        };
        assert_eq!(source_still_pending.state, HandyQualificationState::Pending);
        assert_eq!(
            source_still_pending.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        record(
            &output,
            "dossiers-still-pending.json",
            &json!({"device":device_still_pending,"source":source_still_pending}),
        );
        record(&output, "public-original-ids.json", &originals);
        record(&output, "capture-events.json", &events);
        record(&output, "transfers.json", &transfers);
        {
            let state = peer.state.lock().unwrap();
            assert_eq!((state.setups, state.plays, state.stops), (1, 1, 1));
            assert!(state.tail > CAPACITY as i32 && state.consumed > CAPACITY as i32);
            assert!(state.peak_points <= CAPACITY);
            record(&output, "localhost-requests.json", &state.calls);
        }
        assert_eq!(io.syncs.load(Ordering::Acquire), 1);
        assert!(io.reads.load(Ordering::Acquire) >= 2);
        record(
            &output,
            "result-ready.json",
            &json!({"software_checks_passed":true,"public_ui_actions_observed":true,"originals":originals,"awaiting_supervisor_stop":true,"actual_rendered_browser":true,"actual_confined_cpu":true,"one_engine":true,"fault_scenarios":"pending","two_native_transfers":true,"crossed_original_media_deadline":crossed_media,"clock_generation":max_generation,"qualified":false,"physical_device":false}),
        );
        while !stopped(&root) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        browser.finish();
        endpoints.finish().unwrap();
        shutdown_after_peer.finish();
        peer.finish();
        let fence = assert_joined_worker_fence(&x.f);
        record(&output, "joined-worker-fence.json", &fence);
        drop(endpoints);
        drop(engine);
        assert_eq!(Arc::strong_count(&x.f.engine), 1);
        let PreparationFixture { f, .. } = x;
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = Engine::open(config).unwrap();
        let reopened_fixture = Fixture {
            engine: reopened,
            session,
            project,
            dir,
        };
        let replay_live = live_status(&reopened_fixture, &bind_id);
        assert_eq!(replay_live.origin.as_ref(), Some(&origin));
        assert!(replay_live.native.is_none());
        let replay_clock = public_continuity(&reopened_fixture, &enable_id);
        assert_eq!(replay_clock.history, final_clock.history);
        assert!(replay_clock.native.is_none());
        let replay_preparation =
            public_preparation(&reopened_fixture, originals.preparation.as_ref().unwrap());
        assert!(replay_preparation.history.is_some() && replay_preparation.native.is_none());
        let temp_permits:u64=reopened_fixture.engine.db.lock().unwrap().query_row(
            "SELECT count(*) FROM sqlite_temp_master WHERE type='table' AND name IN ('pulsar_test_hsp_device_acceptance','pulsar_test_hsp_source_acceptance')",[],|row|row.get(0)).unwrap();
        assert_eq!(temp_permits, 0);
        record(
            &output,
            "restart-history-only.json",
            &json!({"preparation":replay_preparation,"live":replay_live,
            "clock":replay_clock,"originals":originals,"temporary_permits":temp_permits,
            "source_authority_restored":false,"clock_authority_restored":false}),
        );
        {
            let state = peer.state.lock().unwrap();
            assert_eq!((state.setups, state.plays, state.stops), (1, 1, 1));
        }
        reopened_fixture.engine.finish_shutdown().unwrap();
        let Fixture { engine, dir, .. } = reopened_fixture;
        Engine::retire_for_test(engine);
        drop(dir);
        ui_stage(
            &output,
            "public_ui_campaign_joined_history_only_restart_verified",
            &originals,
        );
        record(
            &output,
            "completion.json",
            &json!({"passed":true,"public_ui_controls":true,"direct_fixture_hsp_actions":false,"originals":originals,"history_only_restart":true,"fault_scenarios":"pending","source_authority_restored":false,"clock_authority_restored":false,"temporary_permits_restored":false,"clock_worker_joined":true,"browser_joined":true,"ipc_joined":true,"engine_joined":true,"localhost_peer_joined":true,"actual_rendered_browser":true,"one_engine":true,"two_transfers":transfers.len(),"crossed_original_media_deadline":crossed_media,"clock_generation":max_generation,"synchronizations":io.syncs.load(Ordering::Acquire),"clock_observations":io.reads.load(Ordering::Acquire),"qualified":false,"physical_device":false}),
        );
    }

    include!("engine_handy_hsp_browser_fault_fixture.rs");
}
