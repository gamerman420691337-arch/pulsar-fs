/// Explicit native CPU campaign on authored neutral inputs. No device or model qualification.
#[cfg(target_os = "linux")]
pub(super) mod captured_cpu_live_native_tests {
    use super::*;
    use anyhow::{bail, ensure, Context, Result};
    use serde_json::{json, Value};
    use std::{
        io::{Read, Seek, SeekFrom},
        os::unix::fs::OpenOptionsExt,
        time::{Duration, Instant},
    };

    const MODEL_SHA: &str = "9a10ed332a69f241c880c0ff8b08f8d234371e6702a59f868510fef7550078c8";
    const RUNTIME_SHA: &str = "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7";
    const MAX_AGE_NS: u64 = 200_000_000;
    const LEASE_MS: u32 = 30_000;
    const CASES: [&str; 5] = [
        "start_continue_stop",
        "authority_cancel",
        "lease_expiry",
        "missing_alias",
        "changed_module",
    ];

    // Diagnostic-only trace: bounded, thread-local and never written during a
    // poll. No token-bearing Request, auth value or free-form error is retained.
    const STARTUP_TRACE_LIMIT: usize = 64;
    #[derive(Clone, Debug, serde::Serialize)]
    struct StartupReportObservation {
        active: bool,
        retirement: Option<LiveCaptureRetirement>,
        server_elapsed_ns: Option<u64>,
        inference_state: Option<LiveCaptureInferenceState>,
        inference_error_code: Option<ErrorCode>,
        completed: Option<u64>,
        accepted: Option<u64>,
        stale: Option<u64>,
        capture_lease_ms: u32,
    }
    impl StartupReportObservation {
        fn read(report: &LiveCaptureReport) -> Self {
            let info = report.inference.as_ref();
            Self {
                active: report.active,
                retirement: report.retirement.clone(),
                server_elapsed_ns: report.server_elapsed.map(|tick| tick.0),
                inference_state: info.map(|value| value.state),
                inference_error_code: info
                    .and_then(|value| value.error.as_ref().map(|error| error.code)),
                completed: info.map(|value| value.completed),
                accepted: info.map(|value| value.accepted),
                stale: info.map(|value| value.stale),
                capture_lease_ms: report.options.lease_ms,
            }
        }
    }
    #[derive(Clone, Debug, serde::Serialize)]
    struct StartupPollObservation {
        returned_at_ns: u64,
        report: StartupReportObservation,
    }
    #[derive(Debug, serde::Serialize)]
    struct StartupWaitObservation {
        origin: &'static str,
        entered_at_ns: u64,
        observation_deadline_ns: u64,
        entry_deadline_passed: bool,
        poll_count: u64,
        first_status_begin_ns: Option<u64>,
        last_status_begin_ns: Option<u64>,
        last_status_end_ns: Option<u64>,
        last_status_ok: Option<bool>,
        max_status_duration_ns: u64,
        max_status_gap_ns: u64,
        first: Option<StartupPollObservation>,
        last: Option<StartupPollObservation>,
        finished_at_ns: Option<u64>,
        result_ok: Option<bool>,
    }
    #[derive(Debug, serde::Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum StartupEvent {
        Phase {
            phase: &'static str,
            edge: &'static str,
            at_ns: u64,
            ok: Option<bool>,
            observation_deadline_ns: Option<u64>,
            requested_lease_ms: Option<u32>,
            report: Option<StartupReportObservation>,
        },
        WaitReady(Box<StartupWaitObservation>),
    }
    #[derive(Debug, serde::Serialize)]
    struct StartupTraceReceipt {
        schema_version: u16,
        clock: &'static str,
        event_limit: usize,
        dropped_events: u64,
        events: Vec<StartupEvent>,
    }
    struct StartupTrace {
        origin: Instant,
        receipt: StartupTraceReceipt,
    }
    impl StartupTrace {
        fn new(origin: Instant) -> Self {
            Self {
                origin,
                receipt: StartupTraceReceipt {
                    schema_version: 1,
                    clock: "case-relative std::time::Instant; diagnostic observation only",
                    event_limit: STARTUP_TRACE_LIMIT,
                    dropped_events: 0,
                    events: Vec::with_capacity(STARTUP_TRACE_LIMIT),
                },
            }
        }
        fn push(&mut self, event: StartupEvent) {
            if self.receipt.events.len() < STARTUP_TRACE_LIMIT {
                self.receipt.events.push(event);
            } else {
                self.receipt.dropped_events = self.receipt.dropped_events.saturating_add(1);
            }
        }
    }
    std::thread_local! {
        static STARTUP_TRACE: std::cell::RefCell<Option<StartupTrace>> = const { std::cell::RefCell::new(None) };
    }
    fn startup_offset(origin: Instant, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(origin).as_nanos()).unwrap_or(u64::MAX)
    }
    struct StartupTraceScope {
        previous: Option<StartupTrace>,
        finished: bool,
    }
    impl StartupTraceScope {
        fn begin() -> Self {
            let trace = StartupTrace::new(Instant::now());
            let previous = STARTUP_TRACE.with(|slot| slot.replace(Some(trace)));
            Self {
                previous,
                finished: false,
            }
        }
        fn finish(mut self) -> Option<StartupTraceReceipt> {
            let trace = STARTUP_TRACE.with(|slot| slot.replace(self.previous.take()));
            self.finished = true;
            trace.map(|value| value.receipt)
        }
    }
    impl Drop for StartupTraceScope {
        fn drop(&mut self) {
            if !self.finished {
                STARTUP_TRACE.with(|slot| {
                    slot.replace(self.previous.take());
                });
            }
        }
    }
    fn startup_rpc(
        phase: &'static str,
        edge: &'static str,
        at: Instant,
        deadline: Option<Instant>,
        lease_ms: Option<u32>,
        ok: Option<bool>,
        report: Option<&LiveCaptureReport>,
    ) {
        STARTUP_TRACE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(trace) = slot.as_mut() {
                trace.push(StartupEvent::Phase {
                    phase,
                    edge,
                    at_ns: startup_offset(trace.origin, at),
                    ok,
                    observation_deadline_ns: deadline.map(|end| startup_offset(trace.origin, end)),
                    requested_lease_ms: lease_ms,
                    report: report.map(StartupReportObservation::read),
                });
            }
        });
    }
    pub(in crate::authority) fn startup_phase(
        phase: &'static str,
        edge: &'static str,
        ok: Option<bool>,
    ) {
        startup_rpc(phase, edge, Instant::now(), None, None, ok, None);
    }
    fn startup_response(response: &Result<ResponseBody>) -> Option<&LiveCaptureReport> {
        match response {
            Ok(ResponseBody::LiveCapture(report)) => Some(report),
            _ => None,
        }
    }
    struct StartupReadyTrace {
        case_origin: Option<Instant>,
        observation: Option<StartupWaitObservation>,
    }
    impl StartupReadyTrace {
        fn begin(origin: &'static str, deadline: Instant) -> Self {
            let case_origin =
                STARTUP_TRACE.with(|slot| slot.borrow().as_ref().map(|trace| trace.origin));
            let now = Instant::now();
            let observation = case_origin.map(|start| StartupWaitObservation {
                origin,
                entered_at_ns: startup_offset(start, now),
                observation_deadline_ns: startup_offset(start, deadline),
                entry_deadline_passed: now >= deadline,
                poll_count: 0,
                first_status_begin_ns: None,
                last_status_begin_ns: None,
                last_status_end_ns: None,
                last_status_ok: None,
                max_status_duration_ns: 0,
                max_status_gap_ns: 0,
                first: None,
                last: None,
                finished_at_ns: None,
                result_ok: None,
            });
            Self {
                case_origin,
                observation,
            }
        }
        fn poll_begin(&mut self) {
            if let (Some(start), Some(value)) = (self.case_origin, self.observation.as_mut()) {
                let now = startup_offset(start, Instant::now());
                if let Some(prior_end) = value.last_status_end_ns {
                    value.max_status_gap_ns =
                        value.max_status_gap_ns.max(now.saturating_sub(prior_end));
                }
                value.poll_count = value.poll_count.saturating_add(1);
                value.first_status_begin_ns.get_or_insert(now);
                value.last_status_begin_ns = Some(now);
                value.last_status_end_ns = None;
                value.last_status_ok = None;
            }
        }
        fn poll_end(&mut self, result: &Result<LiveCaptureReport>) {
            if let (Some(start), Some(value)) = (self.case_origin, self.observation.as_mut()) {
                let now = startup_offset(start, Instant::now());
                if let Some(begin) = value.last_status_begin_ns {
                    value.max_status_duration_ns =
                        value.max_status_duration_ns.max(now.saturating_sub(begin));
                }
                value.last_status_end_ns = Some(now);
                value.last_status_ok = Some(result.is_ok());
                if let Ok(report) = result {
                    let observation = StartupPollObservation {
                        returned_at_ns: now,
                        report: StartupReportObservation::read(report),
                    };
                    value.first.get_or_insert_with(|| observation.clone());
                    value.last = Some(observation);
                }
            }
        }
        fn finish(&mut self, ok: bool) {
            if let (Some(start), Some(value)) = (self.case_origin, self.observation.as_mut()) {
                value.finished_at_ns = Some(startup_offset(start, Instant::now()));
                value.result_ok = Some(ok);
            }
        }
    }
    impl Drop for StartupReadyTrace {
        fn drop(&mut self) {
            if let Some(observation) = self.observation.take() {
                STARTUP_TRACE.with(|slot| {
                    if let Some(trace) = slot.borrow_mut().as_mut() {
                        trace.push(StartupEvent::WaitReady(Box::new(observation)));
                    }
                });
            }
        }
    }

    struct Inputs {
        worker: PathBuf,
        worker_identity: ArtifactIdentity,
        policy: PathBuf,
        policy_identity: ArtifactIdentity,
        policy_value: crate::cpu_owner_policy::CpuOwnerPolicy,
        model: PathBuf,
        model_identity: ArtifactIdentity,
    }
    fn environment_path(name: &str) -> Result<PathBuf> {
        let path =
            PathBuf::from(std::env::var_os(name).with_context(|| format!("{name} required"))?);
        ensure!(path.is_absolute(), "{name} must be absolute");
        Ok(path)
    }
    fn file_identity(path: &Path) -> Result<ArtifactIdentity> {
        let (sha256, byte_len) = artifacts::digest_file(path, 512 * 1024 * 1024)?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len,
        })
    }
    fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
    fn save(path: &Path, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "captured live receipt exceeds bound"
        );
        private_write(path, &bytes)
    }
    impl Inputs {
        fn load() -> Result<Self> {
            let worker = environment_path("PULSAR_CAPTURED_CPU_LIVE_WORKER")?;
            let policy = environment_path("PULSAR_CAPTURED_CPU_LIVE_POLICY")?;
            let model = environment_path("PULSAR_CAPTURED_CPU_LIVE_MODEL")?;
            let worker_identity = file_identity(&worker)?;
            let policy_identity = file_identity(&policy)?;
            let model_identity = file_identity(&model)?;
            let expected_worker = std::env::var("PULSAR_CAPTURED_CPU_LIVE_WORKER_SHA256")?;
            let expected_policy = std::env::var("PULSAR_CAPTURED_CPU_LIVE_POLICY_SHA256")?;
            validate_sha256(&expected_worker)?;
            validate_sha256(&expected_policy)?;
            ensure!(
                worker_identity.sha256 == expected_worker
                    && policy_identity.sha256 == expected_policy
                    && model_identity.sha256 == MODEL_SHA,
                "native subjects differ from explicit pins"
            );
            let mut file = artifacts::open_regular(&policy)?;
            ensure!(
                file.metadata()?.len() <= MAX_CPU_OWNER_POLICY_BYTES as u64,
                "CPU policy byte bound"
            );
            let mut raw = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_CPU_OWNER_POLICY_BYTES as u64 + 1)
                .read_to_end(&mut raw)?;
            let policy_value: crate::cpu_owner_policy::CpuOwnerPolicy =
                serde_json::from_slice(&raw)?;
            policy_value.validate()?;
            ensure!(
                policy_value.runtime_identity()?.sha256 == RUNTIME_SHA,
                "authored campaign runtime pin differs"
            );
            Ok(Self {
                worker,
                worker_identity,
                policy,
                policy_identity,
                policy_value,
                model,
                model_identity,
            })
        }
        fn verify_unchanged(&self) -> Result<()> {
            ensure!(
                file_identity(&self.worker)? == self.worker_identity
                    && file_identity(&self.policy)? == self.policy_identity
                    && file_identity(&self.model)? == self.model_identity,
                "borrowed native subjects changed"
            );
            Ok(())
        }
        fn subjects(&self) -> Value {
            json!({"worker":{"path":self.worker,"identity":self.worker_identity},"policy":{"path":self.policy,"identity":self.policy_identity},"model":{"path":self.model,"identity":self.model_identity},"authored_neutral_model":true,"authored_rgb_pixels":true,"physical_device":false,"qualified":false})
        }
        fn profile(&self) -> Result<DeclaredInferenceProfile> {
            Ok(DeclaredInferenceProfile {
                schema_version: 1,
                model: self.model_identity.clone(),
                runtime: self.policy_value.runtime_identity()?,
                preprocessing: InferencePreprocessing {
                    input: ModelInputContract {
                        width: 64,
                        height: 64,
                        channels: 6,
                        layout: TensorLayout::Nchw,
                        class_count: 1,
                        decoder: DetectorDecoder::TemporalScriptPosition,
                    },
                    resize: InferenceResize::NearestStretch,
                    color: InferenceColor::RgbFramePair,
                    scale: vec![1.0 / 255.0; 6],
                    bias: vec![0.0; 6],
                    padding_byte: 0,
                },
                provider: InferenceProvider::Cpu,
                minimum_memory_bytes: 128 << 20,
                minimum_output_bytes: 1 << 20,
                cpu_threads: 2,
                cuda_resources: None,
            })
        }
        fn options(&self, lease_ms: u32) -> Result<LiveCaptureOptions> {
            let mut options = capture_options();
            options.inference = Some(LiveCaptureInference {
                model_path: self.model.clone(),
                profile: self.profile()?,
            });
            options.lease_ms = lease_ms;
            ensure!(
                options.policy.maximum_analysis_age_ns == MAX_AGE_NS
                    && options.policy.added_buffer_ns == 0,
                "original freshness policy differs"
            );
            options.validate()?;
            Ok(options)
        }
    }
    fn call(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: Option<&ProjectId>,
        revision: Option<u64>,
        command: Command,
    ) -> Result<ResponseBody> {
        Ok(engine
            .handle(request(actor, project, revision, command))
            .result?)
    }
    fn report(body: ResponseBody) -> Result<LiveCaptureReport> {
        let ResponseBody::LiveCapture(value) = body else {
            bail!("capture response missing")
        };
        value.validate()?;
        Ok(*value)
    }
    fn status(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        epoch: &AttemptId,
    ) -> Result<LiveCaptureReport> {
        report(call(
            engine,
            actor,
            Some(project),
            Some(0),
            Command::LiveCaptureStatus {
                epoch: epoch.clone(),
            },
        )?)
    }
    fn snapshot(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
    ) -> Result<ProjectSnapshot> {
        let ResponseBody::Project(value) =
            call(engine, actor, Some(project), None, Command::GetSnapshot)?
        else {
            bail!("snapshot missing")
        };
        Ok(*value)
    }
    fn create_project(engine: &Arc<Engine>) -> Result<(SessionId, ProjectId, ProjectSnapshot)> {
        startup_phase("pair", "begin", None);
        let actor = pair(engine, "Authored captured CPU live native fixture");
        startup_phase("pair", "end", Some(true));
        startup_phase("create_project", "begin", None);
        let response = call(
            engine,
            &actor,
            None,
            None,
            Command::CreateProject {
                name: "Authored captured CPU live fixture".into(),
            },
        );
        startup_phase("create_project", "end", Some(response.is_ok()));
        let ResponseBody::Project(project) = response? else {
            bail!("project missing");
        };
        Ok((actor, project.project_id.clone(), *project))
    }
    fn start(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        options: LiveCaptureOptions,
    ) -> Result<(LiveCaptureReport, Instant)> {
        let started = Instant::now();
        let end = started + Duration::from_millis(u64::from(options.lease_ms));
        let lease_ms = options.lease_ms;
        startup_rpc(
            "public_start",
            "begin",
            started,
            Some(end),
            Some(lease_ms),
            None,
            None,
        );
        let response = call(
            engine,
            actor,
            Some(project),
            Some(0),
            Command::StartLiveCapture {
                engine_epoch: AttemptId::new(engine.engine_epoch.clone())?,
                options,
            },
        );
        startup_rpc(
            "public_start",
            "end",
            Instant::now(),
            Some(end),
            Some(lease_ms),
            Some(response.is_ok()),
            startup_response(&response),
        );
        let report = report(response?)?;
        Ok((report, end))
    }
    fn wait_ready(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        initial: &LiveCaptureReport,
        end: Instant,
    ) -> Result<LiveCaptureReport> {
        wait_ready_with_origin(engine, actor, project, initial, end, "initial")
    }
    fn wait_ready_with_origin(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        initial: &LiveCaptureReport,
        end: Instant,
        origin: &'static str,
    ) -> Result<LiveCaptureReport> {
        let mut trace = StartupReadyTrace::begin(origin, end);
        let result = (|| -> Result<LiveCaptureReport> {
            let original = initial
                .inference
                .as_ref()
                .and_then(|i| i.cpu_manifest.as_ref())
                .context("initial captured CPU mode absent")?
                .identity_sha256()?;
            loop {
                trace.poll_begin();
                let observed = status(engine, actor, project, &initial.identity.epoch);
                trace.poll_end(&observed);
                let current = observed?;
                let info = current
                    .inference
                    .as_ref()
                    .context("capture inference absent")?;
                ensure!(
                    info.cpu_manifest
                        .as_ref()
                        .context("captured mode lost")?
                        .identity_sha256()?
                        == original,
                    "captured mode rebound while starting"
                );
                if info.state == LiveCaptureInferenceState::Running {
                    ensure!(
                        current.active && info.cuda_manifest_sha256.is_none(),
                        "captured Ready state differs"
                    );
                    return Ok(current);
                }
                ensure!(
                    current.active && info.error.is_none(),
                    "captured Ready failed: {:?}",
                    info.error
                );
                ensure!(
                    Instant::now() < end,
                    "captured Ready exceeded original lease observation"
                );
                std::thread::park_timeout(Duration::from_millis(2));
            }
        })();
        trace.finish(result.is_ok());
        result
    }
    fn handshake(actor: &SessionId, report: &LiveCaptureReport) -> LiveBulkHandshake {
        LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: actor.clone(),
            auth_token: TOKENS.with(|tokens| tokens.borrow()[actor.as_str()].clone()),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        }
    }
    fn pixels(index: u64) -> Vec<u8> {
        (0..LIVE_FRAME_BYTES)
            .map(|offset| ((offset as u64 * 17 + index * 23) % 256) as u8)
            .collect()
    }
    fn pixel_identity(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    fn feed(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        ready: &LiveCaptureReport,
        source_base: u64,
        count: u64,
        end: Instant,
    ) -> Result<Value> {
        ensure!(count >= 3, "multiple actual estimates required");
        let manifest = ready
            .inference
            .as_ref()
            .and_then(|i| i.cpu_manifest.as_ref())
            .context("captured Ready manifest absent")?;
        manifest.validate()?;
        ensure!(
            manifest.stream.worker == engine.binary_identity
                && manifest.image.identity()?
                    == engine
                        .cpu_owner_policy
                        .as_ref()
                        .context("CPU policy absent")?
                        .image()?
                        .identity()?,
            "Ready differs from original worker/image"
        );
        let mut accepted = Vec::new();
        let mut previous: Option<(LiveFrameHeader, Vec<u8>)> = None;
        for index in 0..count {
            ensure!(Instant::now() < end, "frame input exceeded original lease");
            let current = status(engine, actor, project, &ready.identity.epoch)?;
            let data = pixels(source_base + index);
            let header = LiveFrameHeader {
                frame: FrameId::new(index),
                source_time: SourceTimestamp::new((source_base + index) as i64, 30)?,
                capture_tick: current.server_elapsed.context("active lease tick absent")?,
                sha256: format!("{:x}", Sha256::digest(&data)),
            };
            let handshake = handshake(actor, &current);
            engine.live_bulk_ready(&handshake, Some(&header))?;
            let (received, replayed) = engine.live_bulk_upload(&handshake, &header, &data)?;
            ensure!(
                !replayed && received.last_frame.as_ref() == Some(&header),
                "original RGB ingress differs"
            );
            if let Some((old, old_data)) = &previous {
                let expected = LiveInferenceInput {
                    sequence: index,
                    pair: RawFramePair {
                        source_epoch: ready.identity.epoch.clone(),
                        previous_frame: old.frame,
                        current_frame: header.frame,
                        previous_time: old.source_time,
                        current_time: header.source_time,
                        presentation_clock: Some(ready.clock.clone()),
                    },
                    previous: pixel_identity(old_data),
                    current: pixel_identity(&data),
                };
                let completed = loop {
                    let current = status(engine, actor, project, &ready.identity.epoch)?;
                    let info = current.inference.as_ref().context("inference absent")?;
                    ensure!(
                        current.active && info.error.is_none(),
                        "actual inference retired: {:?}",
                        info.error
                    );
                    if info.completed >= index {
                        break current;
                    }
                    ensure!(
                        Instant::now() < end,
                        "actual captured estimate exceeded original lease"
                    );
                    std::thread::park_timeout(Duration::from_millis(2));
                };
                let info = completed.inference.as_ref().unwrap();
                ensure!(
                    info.completed == index && info.accepted == index && info.stale == 0,
                    "controlled fresh inputs did not each produce one accepted estimate"
                );
                let latest = info
                    .latest
                    .as_ref()
                    .context("actual accepted estimate absent")?;
                ensure!(
                    latest.fresh
                        && !latest.qualified
                        && latest.capture_tick == header.capture_tick
                        && latest.capture_to_analysis_ns <= MAX_AGE_NS,
                    "original freshness/qualification boundary differs"
                );
                manifest.validate_estimated(&expected, &latest.result)?;
                ensure!(
                    matches!(
                        latest.result,
                        LiveInferenceReply::CapturedCpuEstimated { .. }
                    ),
                    "host estimate substituted for captured result"
                );
                accepted.push(serde_json::to_value(latest)?);
                let (_, replay) = engine.live_bulk_upload(&handshake, &header, &data)?;
                ensure!(replay, "exact original frame retry was not replayed");
            }
            previous = Some((header, data));
        }
        Ok(
            json!({"epoch":ready.identity.epoch,"attempt":manifest.stream.attempt_id,"manifest_sha256":manifest.identity_sha256()?,"image":manifest.image.identity()?,"uploaded_frames":count,"observed_actual_estimates":accepted.len(),"estimates":accepted,"source_time_basis":"authored_producer_declared","freshness_limit_ns":MAX_AGE_NS,"finite_receipt_inference_calls_claimed":false,"qualified":false}),
        )
    }
    fn await_custody_clear(engine: &Arc<Engine>) -> Result<()> {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if !engine.worker_fence.has_pending_worker_custody()?
                && !engine.worker_fence.has_pending_storage_custody()?
                && engine.pool.reserved_storage() == 0
                && engine.pool.reserved_cpu_threads()? == 0
                && engine
                    .workers
                    .lock()
                    .map_err(|_| anyhow::anyhow!("worker map unavailable"))?
                    .is_empty()
                && engine
                    .pool
                    .reserve_memory(engine.config.max_ram_bytes)
                    .is_ok()
            {
                return Ok(());
            }
            ensure!(
                Instant::now() < end,
                "captured live process/resource custody did not clear"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    fn wait_retired(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
        epoch: &AttemptId,
        end: Instant,
    ) -> Result<LiveCaptureReport> {
        loop {
            let value = status(engine, actor, project, epoch)?;
            if !value.active {
                return Ok(value);
            }
            ensure!(
                Instant::now() < end,
                "capture did not retire within observation bound"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn direct_host_refusal(engine: &Arc<Engine>, inputs: &Inputs) -> Result<Value> {
        let profile = inputs.profile()?;
        let epoch = AttemptId::new("authored-direct-host-refusal")?;
        let stream = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-direct-attempt")?,
            identity: LiveIdentity {
                epoch: epoch.clone(),
                project: ProjectId::new("authored-direct-project")?,
                revision: RevisionId::new(0),
                source: SourceVersionId::new("authored-direct-source")?,
            },
            clock: LivePresentationClock {
                epoch,
                source_anchor: SourceTimestamp::new(0, 1)?,
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            model: ArtifactReference {
                path: inputs.model.clone(),
                identity: profile.model.clone(),
            },
            runtime: engine
                .cpu_owner_policy
                .as_ref()
                .context("owner absent")?
                .runtime_reference()?,
            worker: engine.binary_identity.clone(),
            profile,
            budget: ResourceBudget {
                memory_bytes: 1536 << 20,
                output_bytes: 1 << 20,
                wall_time_ms: LEASE_MS as u64,
                cpu_threads: 2,
            },
        };
        let storage = engine.pool.reserved_storage();
        let cpu = engine.pool.reserved_cpu_threads()?;
        let error = match crate::live_inference::LiveInferenceWorker::start(
            &engine.config,
            &engine.pool,
            stream,
            Arc::new(AtomicBool::new(false)),
        ) {
            Ok(_) => bail!("ordinary host worker bypassed configured captured mode"),
            Err(error) => error,
        };
        ensure!(
            error.code == ErrorCode::Unsupported && error.message.contains("captured CPU"),
            "ordinary live refusal differs: {error}"
        );
        ensure!(
            engine.pool.reserved_storage() == storage && engine.pool.reserved_cpu_threads()? == cpu,
            "host refusal staged or admitted resources"
        );
        Ok(
            json!({"refused":true,"error":error,"storage_unchanged":true,"cpu_claims_unchanged":true}),
        )
    }
    fn positive_case(engine: &Arc<Engine>, inputs: &Inputs, mode: &str) -> Result<Value> {
        let ordinary_refusal = direct_host_refusal(engine, inputs)?;
        let (actor, project, original) = create_project(engine)?;
        let lease = if mode == "lease_expiry" {
            10_000
        } else {
            LEASE_MS
        };
        let (initial, end) = start(engine, &actor, &project, inputs.options(lease)?)?;
        let ready = wait_ready(engine, &actor, &project, &initial, end)?;
        let first = feed(engine, &actor, &project, &ready, 0, 4, end)?;
        let mut continuation = Value::Null;
        let terminal = if mode == "start_continue_stop" {
            let prior = status(engine, &actor, &project, &ready.identity.epoch)?;
            let options = prior.continuation_options(SourceTimestamp::new(4, 30)?, LEASE_MS)?;
            let started = Instant::now();
            let next_end = started + Duration::from_millis(u64::from(LEASE_MS));
            startup_rpc(
                "public_continue",
                "begin",
                started,
                Some(next_end),
                Some(LEASE_MS),
                None,
                None,
            );
            let response = call(
                engine,
                &actor,
                Some(&project),
                Some(0),
                Command::ContinueLiveCapture {
                    engine_epoch: prior.engine_epoch.clone(),
                    predecessor: prior.identity.epoch.clone(),
                    options,
                },
            );
            startup_rpc(
                "public_continue",
                "end",
                Instant::now(),
                Some(next_end),
                Some(LEASE_MS),
                Some(response.is_ok()),
                startup_response(&response),
            );
            let next = report(response?)?;
            let next_ready =
                wait_ready_with_origin(engine, &actor, &project, &next, next_end, "continuation")?;
            ensure!(
                next_ready.identity.source == prior.identity.source
                    && next_ready.identity.epoch != prior.identity.epoch,
                "continuation source/epoch differs"
            );
            let old_manifest = prior
                .inference
                .as_ref()
                .unwrap()
                .cpu_manifest
                .as_ref()
                .unwrap();
            let next_manifest = next_ready
                .inference
                .as_ref()
                .unwrap()
                .cpu_manifest
                .as_ref()
                .unwrap();
            ensure!(
                old_manifest.image.identity()? == next_manifest.image.identity()?
                    && old_manifest.identity_sha256()? != next_manifest.identity_sha256()?
                    && old_manifest.stream.attempt_id != next_manifest.stream.attempt_id,
                "continuation mode changed or original attempt reused"
            );
            let samples = feed(engine, &actor, &project, &next_ready, 4, 4, next_end)?;
            let old = status(engine, &actor, &project, &prior.identity.epoch)?;
            ensure!(
                !old.active && old.retirement == Some(LiveCaptureRetirement::Superseded),
                "predecessor did not retire"
            );
            ensure!(
                engine
                    .live_bulk_ready(&handshake(&actor, &prior), None)
                    .is_err(),
                "superseded ingress remained usable"
            );
            let stopped = report(call(
                engine,
                &actor,
                Some(&project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: next_ready.identity.epoch.clone(),
                },
            )?)?;
            ensure!(
                !stopped.active && stopped.retirement == Some(LiveCaptureRetirement::Stopped),
                "public Stop did not retire capture"
            );
            ensure!(
                engine
                    .live_bulk_ready(&handshake(&actor, &next_ready), None)
                    .is_err(),
                "stopped ingress remained usable"
            );
            continuation = json!({"ready":next_ready,"samples":samples,"predecessor_retired":old,"clean_eof_claimed":false});
            stopped
        } else if mode == "authority_cancel" {
            let admin = pair(engine, "Authored native cancellation administrator");
            call(
                engine,
                &actor,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            )?;
            call(
                engine,
                &admin,
                None,
                None,
                Command::Revoke {
                    session: actor.clone(),
                    project_id: project.clone(),
                },
            )?;
            await_custody_clear(engine)?;
            ensure!(
                engine
                    .live_bulk_ready(&handshake(&actor, &ready), None)
                    .is_err(),
                "revoked actor still has ingress"
            );
            call(
                engine,
                &admin,
                None,
                None,
                Command::Grant {
                    session: actor.clone(),
                    project_id: project.clone(),
                    scopes: vec![Scope::Read],
                },
            )?;
            let retired = status(engine, &actor, &project, &ready.identity.epoch)?;
            ensure!(
                !retired.active && retired.retirement == Some(LiveCaptureRetirement::AuthorityLost),
                "grant restoration rearmed revoked capture"
            );
            retired
        } else {
            let retired = wait_retired(
                engine,
                &actor,
                &project,
                &ready.identity.epoch,
                end + Duration::from_secs(2),
            )?;
            ensure!(
                retired.retirement == Some(LiveCaptureRetirement::Expired),
                "capture did not expire at original lease"
            );
            ensure!(
                engine
                    .live_bulk_ready(&handshake(&actor, &ready), None)
                    .is_err(),
                "expired ingress remained usable"
            );
            retired
        };
        await_custody_clear(engine)?;
        let after = snapshot(engine, &actor, &project)?;
        ensure!(
            after.revision == original.revision
                && encode(&after.motion)? == encode(&original.motion)?,
            "live test changed committed motion"
        );
        let db = engine
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("database unavailable"))?;
        let jobs: i64 = db.query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))?;
        let candidates: i64 =
            db.query_row("SELECT COUNT(*) FROM candidates", [], |row| row.get(0))?;
        ensure!(
            jobs == 0 && candidates == 0,
            "live stream created unrelated finite work"
        );
        Ok(
            json!({"case":mode,"ordinary_host_refusal":ordinary_refusal,"initial":initial,"ready":ready,"first_stream":first,"continuation":continuation,"terminal":terminal,"master_preserved":true,"finite_jobs":jobs,"candidates":candidates,"custody_clear":true,"physical_device":false,"qualified":false}),
        )
    }
    fn missing_alias_case(inputs: &Inputs, directory: &Path) -> Result<Value> {
        let mut policy = inputs.policy_value.clone();
        policy.native_dependencies.libraries.retain(|library| {
            !library
                .names
                .iter()
                .any(|name| name == "libonnxruntime.so.1")
        });
        let policy_path = directory.join("missing-alias-policy.json");
        private_write(&policy_path, &serde_json::to_vec(&policy)?)?;
        let state = directory.join("unopened-state");
        let mut config = EngineConfig::new(state.clone(), inputs.worker.clone());
        config.cpu_owner_policy = Some(policy_path);
        startup_phase("engine_open", "begin", None);
        let opened = Engine::open(config);
        startup_phase("engine_open", "end", Some(opened.is_ok()));
        let error = match opened {
            Ok(engine) => {
                engine.finish_shutdown()?;
                bail!("missing required runtime alias opened an Engine")
            }
            Err(error) => error,
        };
        ensure!(
            format!("{error:#}").contains("required alias") && !state.exists(),
            "missing alias refusal did not occur before state admission: {error:#}"
        );
        Ok(
            json!({"case":"missing_alias","refused":true,"refusal_error":format!("{error:#}"),"state_created":false,"worker_started":false,"physical_device":false,"qualified":false}),
        )
    }
    fn changed_policy(inputs: &Inputs, directory: &Path) -> Result<PathBuf> {
        let mut policy = inputs.policy_value.clone();
        let library = policy
            .native_dependencies
            .libraries
            .iter_mut()
            .find(|library| library.names.iter().any(|name| name == "libc.so.6"))
            .context("pinned libc source absent")?;
        let original = file_identity(&library.path)?;
        ensure!(
            original.sha256 == library.sha256
                && original.byte_len == library.byte_len
                && original.byte_len <= 32 * 1024 * 1024,
            "private mutated module copy admission differs"
        );
        let copy = directory.join("authored-changed-libc.so");
        let mut source = artifacts::open_regular(&library.path)?;
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&copy)?;
        let count = std::io::copy(
            &mut Read::by_ref(&mut source).take(original.byte_len + 1),
            &mut target,
        )?;
        ensure!(
            count == original.byte_len,
            "supplier changed while making private test copy"
        );
        target.sync_all()?;
        ensure!(
            file_identity(&copy)? == original,
            "private copy differs before fault"
        );
        target.seek(SeekFrom::Start(0))?;
        target.write_all(&[0])?;
        target.sync_all()?;
        ensure!(
            file_identity(&copy)?.sha256 != library.sha256,
            "authored module mutation did not change bytes"
        );
        ensure!(
            file_identity(&library.path)? == original,
            "borrowed module was modified"
        );
        library.path = copy;
        let path = directory.join("changed-module-policy.json");
        private_write(&path, &serde_json::to_vec(&policy)?)?;
        Ok(path)
    }
    fn changed_module_case(engine: &Arc<Engine>, inputs: &Inputs) -> Result<Value> {
        let (actor, project, original) = create_project(engine)?;
        let (initial, end) = start(engine, &actor, &project, inputs.options(LEASE_MS)?)?;
        let terminal = wait_retired(engine, &actor, &project, &initial.identity.epoch, end)?;
        let info = terminal
            .inference
            .as_ref()
            .context("failed capture metadata absent")?;
        let error = info
            .error
            .as_ref()
            .context("changed module failure absent")?;
        ensure!(
            terminal.retirement == Some(LiveCaptureRetirement::WorkerFailed)
                && info.state == LiveCaptureInferenceState::Failed
                && info.completed == 0
                && info.accepted == 0
                && info.latest.is_none()
                && error.message.contains("native dependency content mismatch"),
            "changed module did not fail in native capture before Ready: {error}"
        );
        await_custody_clear(engine)?;
        let after = snapshot(engine, &actor, &project)?;
        ensure!(
            after.revision == original.revision
                && encode(&after.motion)? == encode(&original.motion)?,
            "refused capture changed committed motion"
        );
        Ok(
            json!({"case":"changed_module","initial":initial,"terminal":terminal,"captured_estimates":0,"host_fallback_performed":false,"native_capture_refusal":true,"borrowed_module_modified":false,"custody_clear":true,"physical_device":false,"qualified":false}),
        )
    }
    fn campaign_case(inputs: &Inputs, mode: &str) -> Value {
        let diagnostic = StartupTraceScope::begin();
        startup_phase("fixture_directory", "begin", None);
        let directory = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => {
                startup_phase("fixture_directory", "end", Some(false));
                return json!({"case":mode,"passed":false,"error":error.to_string(),"startup_diagnostics":diagnostic.finish()});
            }
        };
        startup_phase("fixture_directory", "end", Some(true));
        let fixture_directory = directory.path().to_path_buf();
        let mut engine: Option<Arc<Engine>> = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
            if mode == "missing_alias" {
                return missing_alias_case(inputs, directory.path());
            }
            let policy = if mode == "changed_module" {
                changed_policy(inputs, directory.path())?
            } else {
                inputs.policy.clone()
            };
            let mut config =
                EngineConfig::new(directory.path().join("state"), inputs.worker.clone());
            config.cpu_owner_policy = Some(policy);
            config.worker_timeout = Duration::from_secs(60);
            startup_phase("engine_open", "begin", None);
            let opened = Engine::open(config);
            startup_phase("engine_open", "end", Some(opened.is_ok()));
            engine = Some(opened?);
            let engine = engine.as_ref().unwrap();
            if mode == "changed_module" {
                changed_module_case(engine, inputs)
            } else {
                positive_case(engine, inputs, mode)
            }
        }));
        startup_phase(
            "cleanup_shutdown",
            if engine.is_some() { "begin" } else { "skipped" },
            None,
        );
        let shutdown = engine
            .as_ref()
            .map(|engine| engine.finish_shutdown())
            .transpose();
        startup_phase("cleanup_shutdown", "end", Some(shutdown.is_ok()));
        startup_phase(
            "cleanup_custody",
            if engine.is_some() { "begin" } else { "skipped" },
            None,
        );
        let custody = engine.as_ref().map(await_custody_clear).transpose();
        startup_phase("cleanup_custody", "end", Some(custody.is_ok()));
        let joined = shutdown.is_ok() && custody.is_ok();
        startup_phase("cleanup_engine_drop", "begin", None);
        drop(engine);
        startup_phase("cleanup_engine_drop", "end", Some(true));
        startup_phase(
            "cleanup_directory",
            if joined { "begin" } else { "retain" },
            None,
        );
        let (removed, retained, cleanup_error) = if joined {
            match directory.close() {
                Ok(()) => (true, None, None),
                Err(error) => (false, None, Some(error.to_string())),
            }
        } else {
            (false, Some(directory.keep()), None)
        };
        startup_phase("cleanup_directory", "end", Some(removed));
        let mut value = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => json!({"case":mode,"error":format!("{error:#}")}),
            Err(_) => json!({"case":mode,"error":"native campaign panicked; see test output"}),
        };
        value["passed"] = json!(value.get("error").is_none() && joined && removed);
        value["fixture_directory"] = json!(fixture_directory);
        value["shutdown_joined"] = json!(shutdown.is_ok());
        value["custody_clear_after_shutdown"] = json!(custody.is_ok());
        value["private_state_removed"] = json!(removed);
        value["retained_scratch"] = json!(retained);
        value["shutdown_error"] = json!(shutdown.err().map(|error| format!("{error:#}")));
        value["custody_error"] = json!(custody.err().map(|error| format!("{error:#}")));
        value["cleanup_error"] = json!(cleanup_error);
        value["startup_diagnostics"] = json!(diagnostic.finish());
        value
    }

    #[test]
    fn startup_diagnostic_is_bounded_case_relative_and_scoped() {
        assert!(STARTUP_TRACE.with(|slot| slot.borrow().is_none()));
        let scope = StartupTraceScope::begin();
        let origin = STARTUP_TRACE.with(|slot| slot.borrow().as_ref().unwrap().origin);
        for index in 0..STARTUP_TRACE_LIMIT + 9 {
            startup_rpc(
                "bounded_trace_test",
                "sample",
                origin + Duration::from_millis(index as u64),
                None,
                None,
                Some(true),
                None,
            );
        }
        let receipt = scope.finish().expect("active diagnostic scope");
        assert_eq!(receipt.events.len(), STARTUP_TRACE_LIMIT);
        assert_eq!(receipt.dropped_events, 9);
        for (index, event) in receipt.events.iter().enumerate() {
            let StartupEvent::Phase { at_ns, .. } = event else {
                panic!("phase expected");
            };
            assert_eq!(*at_ns, index as u64 * 1_000_000);
        }
        assert!(serde_json::to_vec(&receipt).unwrap().len() < 32 * 1024);
        assert!(STARTUP_TRACE.with(|slot| slot.borrow().is_none()));
        // Early case exit must not leak a trace into the included reader test
        // or the following campaign case on this same test thread.
        {
            let _unfinished = StartupTraceScope::begin();
            startup_phase("early_exit_test", "begin", None);
        }
        assert!(STARTUP_TRACE.with(|slot| slot.borrow().is_none()));
        let next = StartupTraceScope::begin();
        let next_receipt = next.finish().unwrap();
        assert!(next_receipt.events.is_empty());
        assert_eq!(next_receipt.dropped_events, 0);
        let phases = StartupTraceScope::begin();
        drop(super::super::StartupPhaseGuard::begin("incomplete"));
        super::super::StartupPhaseGuard::begin("complete").complete();
        let events = phases.finish().unwrap().events;
        let observed: Vec<_> = events
            .iter()
            .map(|event| match event {
                StartupEvent::Phase {
                    phase, edge, ok, ..
                } => (*phase, *edge, *ok),
                _ => panic!("phase expected"),
            })
            .collect();
        assert_eq!(
            observed,
            [
                ("incomplete", "begin", None),
                ("incomplete", "end", Some(false)),
                ("complete", "begin", None),
                ("complete", "end", Some(true)),
            ]
        );
        assert!(STARTUP_TRACE.with(|slot| slot.borrow().is_none()));
    }

    include!("engine_live_cpu_reader_native_tests.rs");

    #[test]
    #[ignore = "requires pinned authored temporal model, current captured CPU worker and closure policy, delegated cgroups and an owned namespace supervisor; run exactly and serially"]
    fn native_captured_cpu_live_runs_public_epochs_and_refuses_unowned_bytes() {
        let output =
            environment_path("PULSAR_CAPTURED_CPU_LIVE_OUTPUT").expect("owned output directory");
        let mut receipt = json!({"schema_version":1,"passed":false,"qualified":false,"physical_device":false,"public_equivalence_acceptance_called":false,"simulated_eligibility":false,"authored_neutral_inputs":true,"scope":"Actual public Engine captured-CPU live epochs and opaque RGB ingress on authored neutral input; no useful-model, browser timing or device qualification","cases":[],"not_exercised":["native browser-bound input EOF/clean-completion publication","browser analysis successor transfer","rendered browser/camera capture","physical Handy synchronization"],"error":null});
        let result = (|| -> Result<()> {
            ensure!(output.is_dir(), "output directory must already exist");
            let inputs = Inputs::load()?;
            receipt["subjects"] = inputs.subjects();
            for mode in CASES {
                let value = campaign_case(&inputs, mode);
                let passed = value["passed"] == true;
                receipt["cases"].as_array_mut().unwrap().push(value);
                inputs.verify_unchanged()?;
                ensure!(passed, "captured live case failed: {mode}");
            }
            receipt["borrowed_subjects_unchanged"] = json!(true);
            Ok(())
        })();
        receipt["passed"] = json!(result.is_ok());
        receipt["error"] = json!(result.as_ref().err().map(|error| format!("{error:#}")));
        save(&output.join("campaign.json"), &receipt)
            .expect("retain original native campaign result");
        result.unwrap();
    }
}
