// Native captured-CPU browser lifecycle on sterile decoded RGB and authored
// client timing observations. No rendered browser, source acceptance or device.
#[cfg(target_os = "linux")]
mod captured_cpu_browser_native_tests {
    use super::*;
    use anyhow::{bail, ensure, Context, Result};
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};
    use std::{
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        sync::atomic::AtomicU64,
        time::{Duration, Instant},
    };

    const MODEL_SHA: &str = "9a10ed332a69f241c880c0ff8b08f8d234371e6702a59f868510fef7550078c8";
    const RUNTIME_SHA: &str = "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7";
    const MEDIA_SHA: &str = "cfaafb105b7db1b5406ddfcaa913d9d635c990ec3f4dacd6818dee0b3c4eef12";
    const MAX_AGE_NS: u64 = 200_000_000;
    const LEASE_MS: u32 = 30_000;
    const CASES: [&str; 2] = ["clean_input_eof", "prepared_successor_transfer"];

    fn path_env(name: &str) -> Result<PathBuf> {
        let path =
            PathBuf::from(std::env::var_os(name).with_context(|| format!("{name} required"))?);
        ensure!(path.is_absolute(), "{name} must be absolute");
        Ok(path)
    }
    fn identity(path: &Path) -> Result<ArtifactIdentity> {
        let (sha256, byte_len) = artifacts::digest_file(path, 512 * 1024 * 1024)?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len,
        })
    }
    fn reference(path: PathBuf) -> Result<ArtifactReference> {
        Ok(ArtifactReference {
            identity: identity(&path)?,
            path,
        })
    }
    fn pinned(path: PathBuf, expected: &str) -> Result<ArtifactReference> {
        validate_sha256(expected)?;
        let value = reference(path)?;
        ensure!(
            value.identity.sha256 == expected,
            "explicit native subject pin differs"
        );
        Ok(value)
    }
    fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
        let mut file = artifacts::open_regular(path)?;
        ensure!(
            file.metadata()?.len() <= maximum as u64,
            "fixture exceeds bounded input"
        );
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= maximum, "fixture changed beyond bound");
        Ok(bytes)
    }
    fn pixel_identity(bytes: &[u8]) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        }
    }
    #[derive(Clone, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct DecodedFrame {
        index: u32,
        offset: u64,
        source_time_us: u64,
        sha256: String,
    }
    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct DecodedFixture {
        schema_version: u16,
        media: ArtifactReference,
        rgb: ArtifactReference,
        decoder: ArtifactReference,
        arguments: Vec<String>,
        frames: Vec<DecodedFrame>,
    }
    struct Inputs {
        worker: ArtifactReference,
        policy: ArtifactReference,
        policy_value: crate::cpu_owner_policy::CpuOwnerPolicy,
        model: ArtifactReference,
        media: ArtifactReference,
        fixture: ArtifactReference,
        decoded: DecodedFixture,
        pixels: Vec<u8>,
        cpu_threads: u16,
        owner_cpu_threads: u16,
        producer_join_wait_ms: u64,
    }
    impl Inputs {
        fn load() -> Result<Self> {
            let worker = pinned(
                path_env("PULSAR_CAPTURED_CPU_LIVE_WORKER")?,
                &std::env::var("PULSAR_CAPTURED_CPU_LIVE_WORKER_SHA256")?,
            )?;
            let policy = pinned(
                path_env("PULSAR_CAPTURED_CPU_LIVE_POLICY")?,
                &std::env::var("PULSAR_CAPTURED_CPU_LIVE_POLICY_SHA256")?,
            )?;
            let model = pinned(path_env("PULSAR_CAPTURED_CPU_LIVE_MODEL")?, MODEL_SHA)?;
            let media = pinned(path_env("PULSAR_CAPTURED_CPU_BROWSER_MEDIA")?, MEDIA_SHA)?;
            let fixture = pinned(
                path_env("PULSAR_CAPTURED_CPU_BROWSER_RGB_FIXTURE")?,
                &std::env::var("PULSAR_CAPTURED_CPU_BROWSER_RGB_FIXTURE_SHA256")?,
            )?;
            let policy_value: crate::cpu_owner_policy::CpuOwnerPolicy =
                serde_json::from_slice(&read_bounded(&policy.path, MAX_CPU_OWNER_POLICY_BYTES)?)?;
            policy_value.validate()?;
            ensure!(
                policy_value.runtime_identity()?.sha256 == RUNTIME_SHA,
                "runtime differs from explicit authored campaign subject"
            );
            let decoded: DecodedFixture =
                serde_json::from_slice(&read_bounded(&fixture.path, 256 * 1024)?)?;
            ensure!(
                decoded.schema_version == 1
                    && decoded.media.path == media.path
                    && decoded.media.identity == media.identity
                    && decoded.frames.len() == 400,
                "RGB fixture must cover exact sterile40s at 10Hz"
            );
            ensure!(
                decoded.rgb.path.is_absolute() && decoded.decoder.path.is_absolute(),
                "RGB fixture paths must be absolute"
            );
            ensure!(
                identity(&decoded.rgb.path)? == decoded.rgb.identity
                    && identity(&decoded.decoder.path)? == decoded.decoder.identity,
                "decoded input or decoder pin differs"
            );
            let expected_args = vec![
                "-nostdin",
                "-v",
                "error",
                "-i",
                media.path.to_str().context("media path UTF-8")?,
                "-map",
                "0:v:0",
                "-vf",
                "scale=64:64:flags=neighbor",
                "-frames:v",
                "400",
                "-pix_fmt",
                "rgb24",
                "-f",
                "rawvideo",
                "pipe:1",
            ];
            ensure!(
                decoded.arguments == expected_args,
                "fixture decoder recipe differs"
            );
            let pixels = read_bounded(&decoded.rgb.path, 400 * LIVE_FRAME_BYTES)?;
            ensure!(
                pixels.len() == 400 * LIVE_FRAME_BYTES,
                "exact RGB frame count differs"
            );
            for (index, frame) in decoded.frames.iter().enumerate() {
                ensure!(
                    frame.index == index as u32
                        && frame.offset == (index * LIVE_FRAME_BYTES) as u64
                        && frame.source_time_us == index as u64 * 100_000,
                    "decoded frame order/time differs"
                );
                validate_sha256(&frame.sha256)?;
                ensure!(
                    pixel_identity(
                        &pixels[index * LIVE_FRAME_BYTES..(index + 1) * LIVE_FRAME_BYTES]
                    )
                    .sha256
                        == frame.sha256,
                    "decoded frame bytes differ"
                );
            }
            Ok(Self {
                worker,
                policy,
                policy_value,
                model,
                media,
                fixture,
                decoded,
                pixels,
                cpu_threads: std::env::var("PULSAR_CAPTURED_CPU_BROWSER_THREADS")
                    .unwrap_or_else(|_| "2".into())
                    .parse()?,
                owner_cpu_threads: std::env::var("PULSAR_CAPTURED_CPU_BROWSER_OWNER_THREADS")
                    .unwrap_or_else(|_| "4".into())
                    .parse()?,
                producer_join_wait_ms: std::env::var("PULSAR_CAPTURED_CPU_BROWSER_JOIN_WAIT_MS")
                    .unwrap_or_else(|_| "0".into()).parse()?,
            })
        }
        fn verify_unchanged(&self) -> Result<()> {
            for original in [
                &self.worker,
                &self.policy,
                &self.model,
                &self.media,
                &self.fixture,
                &self.decoded.rgb,
                &self.decoded.decoder,
            ] {
                ensure!(
                    identity(&original.path)? == original.identity,
                    "borrowed browser native input changed"
                );
            }
            Ok(())
        }
        fn subjects(&self) -> Value {
            json!({"worker":self.worker,"policy":self.policy,"model":self.model,"media":self.media,"rgb_fixture":self.fixture,"rgb":self.decoded.rgb,"decoder":self.decoded.decoder,"decoder_arguments":self.decoded.arguments,"cpu_threads":self.cpu_threads,"owner_cpu_threads":self.owner_cpu_threads,"producer_join_wait_ms":self.producer_join_wait_ms,"latency_qualification":false,"authored_neutral_model":true,"authored_browser_observations":true,"browser_capture":false,"physical_device":false,"qualified":false})
        }
        fn bytes(&self, index: usize) -> Result<&[u8]> {
            ensure!(
                index < self.decoded.frames.len(),
                "selected media frame outside retained fixture"
            );
            Ok(&self.pixels[index * LIVE_FRAME_BYTES..(index + 1) * LIVE_FRAME_BYTES])
        }
        fn profile(&self) -> Result<DeclaredInferenceProfile> {
            let profile = DeclaredInferenceProfile {
                schema_version: 1,
                model: self.model.identity.clone(),
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
                cpu_threads: self.cpu_threads,
                cuda_resources: None,
            };
            profile.validate()?;
            Ok(profile)
        }
    }
    struct Client<'a> {
        engine: &'a Arc<Engine>,
        actor: SessionId,
        project: ProjectId,
    }
    impl<'a> Client<'a> {
        fn new(engine: &'a Arc<Engine>) -> Result<Self> {
            let actor = pair(engine, "Authored captured CPU browser native fixture");
            let ResponseBody::Project(project) = engine
                .handle(request(
                    &actor,
                    None,
                    None,
                    Command::CreateProject {
                        name: "Sterile captured CPU browser lifecycle".into(),
                    },
                ))
                .result?
            else {
                bail!("project missing")
            };
            Ok(Self {
                engine,
                actor,
                project: project.project_id.clone(),
            })
        }
        fn intent(&self, revision: Option<u64>, command: Command) -> Request {
            request(&self.actor, Some(&self.project), revision, command)
        }
        fn call(&self, revision: Option<u64>, command: Command) -> Result<ResponseBody> {
            Ok(self.engine.handle(self.intent(revision, command)).result?)
        }
        fn capture(&self, epoch: &AttemptId) -> Result<LiveCaptureReport> {
            capture(self.call(
                Some(1),
                Command::LiveCaptureStatus {
                    epoch: epoch.clone(),
                },
            )?)
        }
        fn snapshot(&self) -> Result<ProjectSnapshot> {
            let ResponseBody::Project(value) = self.call(None, Command::GetSnapshot)? else {
                bail!("snapshot missing")
            };
            Ok(*value)
        }
    }
    fn capture(body: ResponseBody) -> Result<LiveCaptureReport> {
        let ResponseBody::LiveCapture(value) = body else {
            bail!("capture report missing")
        };
        value.validate()?;
        Ok(*value)
    }
    fn prepare_media(client: &Client<'_>, inputs: &Inputs) -> Result<BrowserMediaBinding> {
        let ResponseBody::Source { source_version } = client.call(
            None,
            Command::ImportSource {
                path: inputs.media.path.clone(),
            },
        )?
        else {
            bail!("imported source missing")
        };
        client.call(
            Some(0),
            Command::SetSourcePlacement {
                spec: SourcePlacementSpec {
                    source_version,
                    source_start: SourceTimestamp::new(0, 1)?,
                    source_end: SourceTimestamp::new(40, 1)?,
                    source_anchor: SourceTimestamp::new(0, 1)?,
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
                replace: None,
            },
        )?;
        let ResponseBody::BrowserMediaCatalog(catalog) =
            client.call(Some(1), Command::BrowserMediaCatalog)?
        else {
            bail!("media catalog missing")
        };
        ensure!(catalog.entries.len() == 1, "one original source expected");
        let intent = client.intent(
            Some(1),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(0, 1)?,
                    elapsed_anchor: SourceTimestamp::new(0, 1)?,
                },
                lease_ms: 60_000,
                allow_declared_clock: true,
            },
        );
        client.engine.handle(intent.clone()).result?;
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let ResponseBody::BrowserMediaPreparation(report) = client.call(
                None,
                Command::BrowserMediaPreparationStatus {
                    original_request_id: intent.request_id.clone(),
                },
            )?
            else {
                bail!("media preparation missing")
            };
            if report.state == BrowserMediaPreparationState::Ready {
                ensure!(report.current, "native media ownership not current");
                let binding = report.binding.context("media binding absent")?;
                binding.validate()?;
                return Ok(binding);
            }
            ensure!(
                report.state == BrowserMediaPreparationState::Pending && Instant::now() < end,
                "media preparation refused or timed out: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
    }
    fn timing_policy() -> BrowserTimingPolicy {
        BrowserTimingPolicy {
            probe_timeout_ms: 100,
            observation_timeout_ms: 500,
        }
    }
    fn lookahead() -> BrowserAnalysisLookaheadPolicy {
        BrowserAnalysisLookaheadPolicy {
            target_lead_us: 10_000_000,
            minimum_lead_us: 5_000_000,
            maximum_lead_us: 15_000_000,
        }
    }
    struct AuthoredPlayer {
        binding: BrowserTimingBinding,
        origin: Instant,
        sequence: u64,
        frames: Arc<AtomicU64>,
        last_tick_us: u64,
        last_observation: Option<BrowserTimingObservation>,
    }
    impl AuthoredPlayer {
        fn open(client: &Client<'_>, media: &BrowserMediaBinding) -> Result<Self> {
            let player = HandyPlayerIdentity {
                launch_id: AttemptId::new(format!("authored-launch-{}", uuid::Uuid::new_v4()))?,
                page_id: AttemptId::new(format!("authored-page-{}", uuid::Uuid::new_v4()))?,
                player_id: AttemptId::new(format!("authored-player-{}", uuid::Uuid::new_v4()))?,
                source_id: AttemptId::new(format!("authored-source-{}", uuid::Uuid::new_v4()))?,
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            };
            let ResponseBody::BrowserTiming(report) = client.call(
                Some(1),
                Command::OpenBrowserTiming {
                    player,
                    browser_media: media.clone(),
                    policy: timing_policy(),
                },
            )?
            else {
                bail!("timing binding missing")
            };
            report.validate()?;
            Ok(Self {
                binding: report.binding,
                origin: Instant::now(),
                sequence: 0,
                frames: Arc::new(AtomicU64::new(0)),
                last_tick_us: 0,
                last_observation: None,
            })
        }
        fn observe(&mut self, client: &Client<'_>) -> Result<()> {
            self.sequence = self
                .sequence
                .checked_add(1)
                .context("timing sequence overflow")?;
            let ResponseBody::BrowserTimingProbe(probe) = client.call(
                None,
                Command::IssueBrowserTimingProbe {
                    binding_id: self.binding.binding_id.clone(),
                    player: self.binding.player.clone(),
                    sequence: self.sequence,
                },
            )?
            else {
                bail!("timing probe missing")
            };
            let presented_frames = self
                .frames
                .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
                .map_err(|_| anyhow::anyhow!("authored frame counter exhausted"))?
                + 1;
            let media_time_us = u64::try_from(self.origin.elapsed().as_micros())?;
            let tick = 1_000_000 + media_time_us;
            ensure!(
                tick > self.last_tick_us,
                "authored observation clock did not advance"
            );
            self.last_tick_us = tick;
            let frame = BrowserTimingFrame {
                sample: HandyPlayerSample {
                    browser_tick_us: tick,
                    media_time_us,
                    presented_frames,
                    playing: true,
                    seeking: false,
                    ended: false,
                    visible: true,
                    playback_rate_milli: 1000,
                },
                callback_tick_us: tick,
                presentation_time_us: Some(tick),
                expected_display_time_us: None,
            };
            let ResponseBody::BrowserTiming(report) = client.call(
                None,
                Command::ObserveBrowserTiming {
                    binding_id: self.binding.binding_id.clone(),
                    player: self.binding.player.clone(),
                    sequence: probe.sequence,
                    probe_id: probe.probe_id,
                    frame,
                },
            )?
            else {
                bail!("timing observation missing")
            };
            report.validate()?;
            ensure!(
                report.current && report.binding == self.binding && !report.qualified,
                "authored observation lost original native timing ownership"
            );
            self.last_observation = Some(report.observation.context("native observation absent")?);
            Ok(())
        }
        fn frame_base(&self) -> Result<usize> {
            let base = usize::try_from(self.origin.elapsed().as_micros() / 100_000)? + 100;
            ensure!(
                base + 8 < 400,
                "original timing/lease left insufficient sterile source lookahead"
            );
            Ok(base)
        }
    }
    fn options(
        inputs: &Inputs,
        media: &BrowserMediaBinding,
        player: &AuthoredPlayer,
    ) -> Result<LiveCaptureOptions> {
        let mut options = capture_options();
        ensure!(
            options.policy.maximum_analysis_age_ns == MAX_AGE_NS
                && options.policy.added_buffer_ns == 0
                && options.lease_ms == LEASE_MS,
            "original capture freshness or lease changed"
        );
        options.inference = Some(LiveCaptureInference {
            model_path: inputs.model.path.clone(),
            profile: inputs.profile()?,
        });
        options.browser_media = Some(media.clone());
        options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
            feed_id: ArtifactId::new(format!("authored-feed-{}", uuid::Uuid::new_v4()))?,
            player: player.binding.player.clone(),
            timing_binding_id: player.binding.binding_id.clone(),
            policy: lookahead(),
        });
        options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
        let anchor = media
            .clock
            .project_at(options.source_anchor, &media.selection.placement.spec)?;
        options.project_anchor = anchor.quantize_nanoseconds()?.time;
        options.exact_project_anchor = Some(anchor);
        options.validate()?;
        Ok(options)
    }
    fn captured_manifest(report: &LiveCaptureReport) -> Result<&LiveCpuInferenceManifest> {
        report
            .inference
            .as_ref()
            .and_then(|i| i.cpu_manifest.as_deref())
            .context("original captured mode absent")
    }
    fn assert_scope(
        engine: &Arc<Engine>,
        original: &LiveCaptureReport,
        current: &LiveCaptureReport,
    ) -> Result<()> {
        current.validate()?;
        let expected = captured_manifest(original)?;
        let actual = captured_manifest(current)?;
        actual.validate()?;
        ensure!(
            current.identity == original.identity
                && current.clock == original.clock
                && encode(&current.options)? == encode(&original.options)?
                && actual.identity_sha256()? == expected.identity_sha256()?,
            "capture source/profile/mode/clock/lease rebound"
        );
        ensure!(
            actual.stream.worker == engine.binary_identity
                && actual.image.identity()?
                    == engine
                        .cpu_owner_policy
                        .as_ref()
                        .context("CPU owner absent")?
                        .image()?
                        .identity()?
                && actual.stream.budget.wall_time_ms == u64::from(LEASE_MS)
                && current.options.policy.maximum_analysis_age_ns == MAX_AGE_NS,
            "original captured image/worker/budget differs"
        );
        ensure!(
            current
                .inference
                .as_ref()
                .unwrap()
                .cuda_manifest_sha256
                .is_none(),
            "CUDA substituted in CPU browser mode"
        );
        Ok(())
    }
    fn start(
        client: &Client<'_>,
        inputs: &Inputs,
        media: &BrowserMediaBinding,
        player: &mut AuthoredPlayer,
    ) -> Result<(LiveCaptureReport, Instant)> {
        player.observe(client)?;
        let end = Instant::now() + Duration::from_millis(u64::from(LEASE_MS));
        let initial = capture(client.call(
            Some(1),
            Command::StartLiveCapture {
                engine_epoch: media.engine_epoch.clone(),
                options: options(inputs, media, player)?,
            },
        )?)?;
        loop {
            player.observe(client)?;
            let current = client.capture(&initial.identity.epoch)?;
            assert_scope(client.engine, &initial, &current)?;
            let info = current.inference.as_ref().unwrap();
            ensure!(
                current.active && info.error.is_none(),
                "captured browser worker refused: {:?}",
                info.error
            );
            if info.state == LiveCaptureInferenceState::Running {
                return Ok((current, end));
            }
            ensure!(
                Instant::now() < end,
                "Ready exceeded original capture lease"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
    }
    fn handshake(client: &Client<'_>, report: &LiveCaptureReport) -> LiveBulkHandshake {
        LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveCapture,
            session: client.actor.clone(),
            auth_token: TOKENS.with(|tokens| tokens.borrow()[client.actor.as_str()].clone()),
            engine_epoch: report.engine_epoch.clone(),
            identity: report.identity.clone(),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn upload(
        client: &Client<'_>,
        inputs: &Inputs,
        original: &LiveCaptureReport,
        player: &mut AuthoredPlayer,
        decoded_index: usize,
        frame: u64,
        end: Instant,
    ) -> Result<LiveFrameHeader> {
        ensure!(
            Instant::now() < end,
            "RGB admission exceeded original lease"
        );
        player.observe(client)?;
        let current = client.capture(&original.identity.epoch)?;
        assert_scope(client.engine, original, &current)?;
        let data = inputs.bytes(decoded_index)?;
        let header = LiveFrameHeader {
            frame: FrameId::new(frame),
            source_time: SourceTimestamp::new(
                i64::try_from(inputs.decoded.frames[decoded_index].source_time_us)?,
                1_000_000,
            )?,
            capture_tick: current
                .server_elapsed
                .context("active native capture tick absent")?,
            sha256: inputs.decoded.frames[decoded_index].sha256.clone(),
        };
        let hello = handshake(client, &current);
        client.engine.live_bulk_ready(&hello, Some(&header))?;
        let (received, replayed) = client.engine.live_bulk_upload(&hello, &header, data)?;
        ensure!(
            !replayed && received.last_frame.as_ref() == Some(&header),
            "original opaque RGB input differs"
        );
        Ok(header)
    }
    #[allow(clippy::too_many_arguments)]
    fn estimate(
        client: &Client<'_>,
        inputs: &Inputs,
        original: &LiveCaptureReport,
        player: &mut AuthoredPlayer,
        previous: (&LiveFrameHeader, usize),
        current: (&LiveFrameHeader, usize),
        sequence: u64,
        end: Instant,
    ) -> Result<Value> {
        let (old, old_index) = previous;
        let (new, new_index) = current;
        let gap = new
            .capture_tick
            .0
            .checked_sub(old.capture_tick.0)
            .context("capture tick regressed")?;
        ensure!(
            gap <= 500_000_000
                && new.source_time
                    == SourceTimestamp::new(
                        i64::try_from(inputs.decoded.frames[new_index].source_time_us)?,
                        1_000_000
                    )?,
            "original pair no longer within native correspondence bound"
        );
        let expected = LiveInferenceInput {
            sequence,
            pair: RawFramePair {
                source_epoch: original.identity.epoch.clone(),
                previous_frame: old.frame,
                current_frame: new.frame,
                previous_time: old.source_time,
                current_time: new.source_time,
                presentation_clock: Some(original.clock.clone()),
            },
            previous: pixel_identity(inputs.bytes(old_index)?),
            current: pixel_identity(inputs.bytes(new_index)?),
        };
        loop {
            player.observe(client)?;
            let current_report = client.capture(&original.identity.epoch)?;
            assert_scope(client.engine, original, &current_report)?;
            let info = current_report.inference.as_ref().unwrap();
            ensure!(
                current_report.active && info.error.is_none(),
                "actual captured pair retired: {:?}",
                info.error
            );
            if info.completed >= sequence {
                ensure!(
                    info.completed == sequence && info.accepted == sequence && info.stale == 0,
                    "controlled fresh input did not yield exactly one accepted pair"
                );
                let latest = info
                    .latest
                    .as_ref()
                    .context("actual accepted pair absent")?;
                ensure!(
                    latest.fresh
                        && !latest.qualified
                        && latest.capture_tick == new.capture_tick
                        && latest.capture_to_analysis_ns <= MAX_AGE_NS,
                    "original 200ms freshness or unqualified status differs"
                );
                captured_manifest(original)?.validate_estimated(&expected, &latest.result)?;
                let LiveInferenceReply::CapturedCpuEstimated { position, .. } = &latest.result
                else {
                    bail!("ordinary host or CUDA estimate substituted")
                };
                let sum: u64 = inputs
                    .bytes(old_index)?
                    .iter()
                    .chain(inputs.bytes(new_index)?)
                    .map(|value| u64::from(*value))
                    .sum();
                let expected_position =
                    0.25 + 0.5 * sum as f64 / (2 * LIVE_FRAME_BYTES) as f64 / 255.0;
                ensure!(
                    (position.value() - expected_position).abs() < 0.0001,
                    "actual authored model output differs from exact frame bytes"
                );
                return Ok(
                    json!({"previous_frame":old,"frame":new,"decoded_previous":old_index,"decoded_current":new_index,"expected_input":expected,"estimate":latest,"expected_authored_model_position":expected_position,"capture_gap_ns":gap}),
                );
            }
            ensure!(
                Instant::now() < end,
                "pair completion exceeded original lease"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn close_input(
        client: &Client<'_>,
        original: &LiveCaptureReport,
        player: &mut AuthoredPlayer,
        final_frame: &LiveFrameHeader,
        end: Instant,
    ) -> Result<LiveCaptureReport> {
        player.observe(client)?;
        let intent = client.intent(
            Some(1),
            Command::CloseBrowserAnalysisInput {
                epoch: original.identity.epoch.clone(),
                final_frame: Some(final_frame.clone()),
            },
        );
        let pending = capture(client.engine.handle(intent.clone()).result?)?;
        ensure!(
            pending
                .input_close
                .as_ref()
                .is_some_and(|value| value.original_request_id == intent.request_id
                    && value.final_frame.as_ref() == Some(final_frame)),
            "original close intent not retained"
        );
        ensure!(
            client
                .engine
                .live_bulk_ready(&handshake(client, &pending), Some(final_frame))
                .is_err(),
            "closed input still admitted frames"
        );
        loop {
            player.observe(client)?;
            let current = client.capture(&original.identity.epoch)?;
            assert_scope(client.engine, original, &current)?;
            let info = current.inference.as_ref().unwrap();
            ensure!(
                current.active && info.error.is_none(),
                "clean EOF failed: {:?}",
                info.error
            );
            let close = current
                .input_close
                .as_ref()
                .context("input close status absent")?;
            if close.phase == LiveCaptureInputPhase::Sealed {
                ensure!(
                    close.worker_joined
                        && close.delivery_settled
                        && close.disposition == Some(LiveCaptureInputFinalDisposition::Accepted)
                        && close.final_frame.as_ref() == Some(final_frame)
                        && info.state == LiveCaptureInferenceState::InputSealed,
                    "captured clean completion proof absent"
                );
                let replay = capture(client.engine.handle(intent.clone()).result?)?;
                ensure!(
                    replay
                        .input_close
                        .as_ref()
                        .is_some_and(|value| value.original_request_id == intent.request_id
                            && value.final_frame.as_ref() == Some(final_frame)),
                    "close replay changed original intent"
                );
                return Ok(current);
            }
            ensure!(
                matches!(
                    close.phase,
                    LiveCaptureInputPhase::Closing | LiveCaptureInputPhase::Joining
                ) && Instant::now() < end,
                "input did not seal within original lease"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn stop(client: &Client<'_>, epoch: &AttemptId) -> Result<LiveCaptureReport> {
        let stopped = capture(client.call(
            Some(1),
            Command::StopLiveCapture {
                epoch: epoch.clone(),
            },
        )?)?;
        ensure!(
            !stopped.active && stopped.retirement == Some(LiveCaptureRetirement::Stopped),
            "public Stop did not retire capture"
        );
        Ok(stopped)
    }
    fn release(
        client: &Client<'_>,
        media: &BrowserMediaBinding,
        player: &AuthoredPlayer,
    ) -> Result<()> {
        client.call(
            None,
            Command::ReleaseBrowserTiming {
                binding_id: player.binding.binding_id.clone(),
                player: player.binding.player.clone(),
            },
        )?;
        client.call(
            None,
            Command::ReleaseBrowserMedia {
                original_request_id: media.preparation_request_id.clone(),
            },
        )?;
        Ok(())
    }
    fn clean_eof(
        client: &Client<'_>,
        inputs: &Inputs,
        media: &BrowserMediaBinding,
        player: &mut AuthoredPlayer,
    ) -> Result<Value> {
        let (ready, end) = start(client, inputs, media, player)?;
        let base = player.frame_base()?;
        let first = upload(client, inputs, &ready, player, base, 0, end)?;
        let second = upload(client, inputs, &ready, player, base + 1, 1, end)?;
        let one = estimate(
            client,
            inputs,
            &ready,
            player,
            (&first, base),
            (&second, base + 1),
            1,
            end,
        )?;
        let third = upload(client, inputs, &ready, player, base + 2, 2, end)?;
        let two = estimate(
            client,
            inputs,
            &ready,
            player,
            (&second, base + 1),
            (&third, base + 2),
            2,
            end,
        )?;
        let sealed = close_input(client, &ready, player, &third, end)?;
        ensure!(
            sealed.server_elapsed.context("sealed lease tick absent")?.0 >= third.capture_tick.0
                && Instant::now() < end,
            "EOF renewed original epoch clock/lease"
        );
        let stopped = stop(client, &ready.identity.epoch)?;
        release(client, media, player)?;
        Ok(
            json!({"case":"clean_input_eof","ready":ready,"estimates":[one,two],"observed_actual_estimates":2,"sealed":sealed,"stopped":stopped,"media":media,"timing_binding":player.binding,"last_authored_timing_observation":player.last_observation,"captured_clean_completion_observed":true,"clean_completion_basis":"Sealed with captured manifest requires private CapturedClean proof after worker final-byte verification, output EOF, successful child exit and joined custody","ingress_refused_after_close":true,"original_lease_ms":LEASE_MS,"freshness_limit_ns":MAX_AGE_NS,"source_acceptance_called":false,"physical_device":false,"qualified":false} ),
        )
    }
    fn prepare_successor(
        client: &Client<'_>,
        original: &LiveCaptureReport,
        media: &BrowserMediaBinding,
        player: &mut AuthoredPlayer,
        end: Instant,
    ) -> Result<(
        BrowserMediaBinding,
        AuthoredPlayer,
        Request,
        LiveCaptureReport,
    )> {
        player.observe(client)?;
        let media_intent = client.intent(
            Some(1),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: media.clone(),
                lease_ms: 60_000,
            },
        );
        client.engine.handle(media_intent.clone()).result?;
        let child_media = loop {
            player.observe(client)?;
            let ResponseBody::BrowserMediaPreparation(report) = client.call(
                None,
                Command::BrowserMediaPreparationStatus {
                    original_request_id: media_intent.request_id.clone(),
                },
            )?
            else {
                bail!("successor media response absent")
            };
            if report.state == BrowserMediaPreparationState::Ready {
                ensure!(
                    report.current,
                    "successor native media ownership not current"
                );
                break report.binding.context("successor media binding absent")?;
            }
            ensure!(
                report.state == BrowserMediaPreparationState::Pending && Instant::now() < end,
                "successor media failed within original overlap lease: {report:?}"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        };
        ensure!(
            child_media.selection == media.selection
                && child_media.clock == media.clock
                && child_media.preparation_request_id != media.preparation_request_id,
            "successor changed source or copied original media capability"
        );
        let ResponseBody::BrowserTiming(timing) = client.call(
            Some(1),
            Command::PrepareBrowserTimingSuccessor {
                predecessor_binding_id: player.binding.binding_id.clone(),
                player: player.binding.player.clone(),
                browser_media: child_media.clone(),
                policy: timing_policy(),
            },
        )?
        else {
            bail!("successor timing absent")
        };
        timing.validate()?;
        ensure!(
            timing.binding.binding_id != player.binding.binding_id,
            "successor reused original timing binding"
        );
        let mut child_player = AuthoredPlayer {
            binding: timing.binding,
            origin: player.origin,
            sequence: 0,
            frames: player.frames.clone(),
            last_tick_us: player.last_tick_us,
            last_observation: None,
        };
        child_player.observe(client)?;
        let mut declaration = original
            .options
            .browser_analysis
            .clone()
            .context("browser analysis declaration absent")?;
        declaration.feed_id =
            ArtifactId::new(format!("authored-successor-feed-{}", uuid::Uuid::new_v4()))?;
        declaration.timing_binding_id = child_player.binding.binding_id.clone();
        let intent = client.intent(
            Some(1),
            Command::PrepareBrowserAnalysisSuccessor {
                engine_epoch: child_media.engine_epoch.clone(),
                predecessor: original.identity.epoch.clone(),
                options: BrowserAnalysisSuccessorOptions {
                    browser_media: child_media.clone(),
                    browser_analysis: declaration,
                    lease_ms: LEASE_MS,
                },
            },
        );
        let mut first_manifest = None;
        let child = loop {
            player.observe(client)?;
            child_player.observe(client)?;
            ensure!(
                Instant::now() < end,
                "successor preparation exceeded original predecessor lease"
            );
            let ResponseBody::BrowserAnalysisSuccessor(value) =
                client.engine.handle(intent.clone()).result?
            else {
                bail!("successor preparation absent")
            };
            value.validate()?;
            ensure!(
                value.original_request_id == intent.request_id
                    && value.predecessor == original.identity,
                "successor intent rebound"
            );
            let digest = captured_manifest(&value.successor)?.identity_sha256()?;
            if let Some(previous) = &first_manifest {
                ensure!(
                    &digest == previous,
                    "prepared successor captured declaration changed"
                );
            } else {
                first_manifest = Some(digest);
            }
            if value.phase == BrowserAnalysisSuccessorPhase::Prepared {
                break value.successor;
            }
            ensure!(
                value.phase == BrowserAnalysisSuccessorPhase::Preparing && value.successor.active,
                "captured successor failed before Prepared: {value:?}"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        };
        let old = captured_manifest(original)?;
        let new = captured_manifest(&child)?;
        ensure!(
            old.image.identity()? == new.image.identity()?
                && old.stream.worker == new.stream.worker
                && old.stream.model.identity == new.stream.model.identity
                && encode(&old.stream.profile)? == encode(&new.stream.profile)?
                && old.dependencies == new.dependencies,
            "successor changed captured image/dependency mode"
        );
        ensure!(
            old.identity_sha256()? != new.identity_sha256()?
                && old.stream.attempt_id != new.stream.attempt_id
                && original.identity.epoch != child.identity.epoch
                && original.identity.source == child.identity.source
                && child.received_frames == 0
                && child.last_frame.is_none(),
            "successor did not retain a fresh exact stream declaration"
        );
        assert_scope(client.engine, &child, &child)?;
        ensure!(
            client
                .engine
                .live_bulk_ready(&handshake(client, &child), None)
                .is_err(),
            "prepared successor admitted pixels before original boundary"
        );
        Ok((child_media, child_player, intent, child))
    }
    fn transfer_status(
        client: &Client<'_>,
        request_id: &RequestId,
    ) -> Result<BrowserAnalysisTransferReport> {
        let ResponseBody::BrowserAnalysisTransfer(report) = client.call(
            None,
            Command::BrowserAnalysisTransferStatus {
                transfer_request_id: request_id.clone(),
            },
        )?
        else {
            bail!("transfer status absent")
        };
        report.validate()?;
        Ok(*report)
    }
    fn successor_transfer(
        client: &Client<'_>,
        inputs: &Inputs,
        media: &BrowserMediaBinding,
        player: &mut AuthoredPlayer,
    ) -> Result<Value> {
        let mut phases = Vec::new();
        let result = successor_transfer_traced(client, inputs, media, player, &mut phases);
        match result {
            Ok(mut value) => {
                value["native_transfer_phase_trace"] = json!(phases);
                Ok(value)
            }
            Err(error) => Err(error.context(format!(
                "native_transfer_phase_trace={}",
                serde_json::to_string(&phases)?
            ))),
        }
    }
    fn transfer_phase(
        phases: &mut Vec<Value>,
        name: &'static str,
        report: Option<&BrowserAnalysisTransferReport>,
    ) -> Result<()> {
        ensure!(phases.len() < 32, "bounded native phase trace exhausted");
        phases.push(
            json!({"phase":name,"boottime_ns":crate::handy_clock::boottime_ns()?,
            "transfer_phase":report.map(|r| r.phase),
            "predecessor_worker_joined":report.map(|r| r.predecessor_worker_joined),
            "successor_pair_accepted":report.map(|r| r.successor_pair_accepted)}),
        );
        Ok(())
    }
    fn successor_transfer_traced(
        client: &Client<'_>,
        inputs: &Inputs,
        media: &BrowserMediaBinding,
        player: &mut AuthoredPlayer,
        phases: &mut Vec<Value>,
    ) -> Result<Value> {
        let (ready, end) = start(client, inputs, media, player)?;
        transfer_phase(phases, "predecessor_ready", None)?;
        // The actual successor must exist before F is captured. Preparing before
        // any accepted input also avoids aging an original stamp during startup.
        let (child_media, mut child_player, prepare, child) =
            prepare_successor(client, &ready, media, player, end)?;
        transfer_phase(phases, "successor_prepared", None)?;
        child_player.observe(client)?;
        let base = player.frame_base()?;
        let first = upload(client, inputs, &ready, player, base, 0, end)?;
        let second = upload(client, inputs, &ready, player, base + 1, 1, end)?;
        let predecessor_estimate = estimate(
            client,
            inputs,
            &ready,
            player,
            (&first, base),
            (&second, base + 1),
            1,
            end,
        )?;
        transfer_phase(phases, "predecessor_pair_observed", None)?;
        child_player.observe(client)?;
        let transfer = client.intent(
            Some(1),
            Command::TransferBrowserAnalysisInput {
                predecessor: ready.identity.epoch.clone(),
                successor: child.identity.epoch.clone(),
                boundary: second.clone(),
            },
        );
        let ResponseBody::BrowserAnalysisTransfer(reserved) =
            client.engine.handle(transfer.clone()).result?
        else {
            bail!("boundary reservation absent")
        };
        reserved.validate()?;
        transfer_phase(phases, "boundary_reserved", Some(&reserved))?;
        ensure!(
            reserved.phase == BrowserAnalysisTransferPhase::BoundaryReserved
                && reserved.boundary == second
                && !reserved.predecessor_worker_joined
                && !reserved.successor_pair_accepted,
            "Ready falsely substituted for actual joined/accepted transfer"
        );
        ensure!(
            client
                .engine
                .live_bulk_ready(&handshake(client, &ready), None)
                .is_err(),
            "predecessor admitted input after boundary reservation"
        );
        let seeded = client.capture(&child.identity.epoch)?;
        assert_scope(client.engine, &child, &seeded)?;
        let seed = seeded
            .last_frame
            .clone()
            .context("native shared seed missing")?;
        ensure!(
            seeded.received_frames == 1
                && seed.frame == FrameId::new(0)
                && seed.source_time == second.source_time
                && seed.sha256 == second.sha256,
            "Engine did not retain the exact original boundary bytes/time"
        );
        let old_project = ready.clock.map(second.source_time)?.project.time;
        let child_project = child.clock.map(seed.source_time)?.project.time;
        ensure!(
            old_project == child_project,
            "shared frame changed exact project phase"
        );
        // Wait for the actual predecessor join before capturing G. Otherwise
        // G can be accepted fresh and age out while A's custody is retiring.
        ensure!(inputs.producer_join_wait_ms <= 350, "controlled producer pause exceeds test bound");
        let wait_started = Instant::now();
        let mut join_state = None;
        loop {
            player.observe(client)?;
            child_player.observe(client)?;
            let status = transfer_status(client, &transfer.request_id)?;
            let state = (status.phase, status.predecessor_worker_joined);
            if join_state != Some(state) {
                transfer_phase(phases, "predecessor_join_observed", Some(&status))?;
                join_state = Some(state);
            }
            ensure!(status.phase != BrowserAnalysisTransferPhase::Aborted
                && status.error.is_none() && Instant::now() < end,
                "predecessor join did not settle before original lease: {status:?}");
            ensure!(!status.successor_pair_accepted, "successor pair exists before G capture");
            if status.predecessor_worker_joined
                && wait_started.elapsed() >= Duration::from_millis(inputs.producer_join_wait_ms) {
                ensure!(status.predecessor_delivery_settled, "join precedes settled delivery");
                break;
            }
            std::thread::park_timeout(Duration::from_millis(2));
        }
        // Only frame1 may enter while transfer is pending. Do not resend F.
        transfer_phase(phases, "successor_upload_started", None)?;
        let next = upload(client, inputs, &child, &mut child_player, base + 2, 1, end)?;
        transfer_phase(phases, "successor_uploaded", None)?;
        let successor_first = estimate(
            client,
            inputs,
            &child,
            &mut child_player,
            (&seed, base + 1),
            (&next, base + 2),
            1,
            end,
        )?;
        transfer_phase(phases, "successor_pair_observed", None)?;
        let mut previous_state = None;
        let transferred = loop {
            child_player.observe(client)?;
            let value = transfer_status(client, &transfer.request_id)?;
            let state = (
                value.phase,
                value.predecessor_worker_joined,
                value.successor_pair_accepted,
            );
            if previous_state != Some(state) {
                transfer_phase(phases, "transfer_state_observed", Some(&value))?;
                previous_state = Some(state);
            }
            if value.phase == BrowserAnalysisTransferPhase::Transferred {
                break value;
            }
            ensure!(
                value.phase != BrowserAnalysisTransferPhase::Aborted
                    && value.error.is_none()
                    && Instant::now() < end,
                "original transfer did not commit: {value:?}"
            );
            // Actual predecessor completion may take more than one status turn;
            // keep both original client timing lifetimes current until commit.
            player.observe(client)?;
            std::thread::park_timeout(Duration::from_millis(2));
        };
        ensure!(
            transferred.predecessor_worker_joined
                && transferred.predecessor_delivery_settled
                && transferred.successor_pair_accepted
                && transferred.boundary == second
                && transferred.preparation_request_id == prepare.request_id,
            "transfer lacks original clean predecessor join or shared-pair acceptance"
        );
        let retired = client.capture(&ready.identity.epoch)?;
        assert_scope(client.engine, &ready, &retired)?;
        let retired_info = retired
            .inference
            .as_ref()
            .context("predecessor history absent")?;
        ensure!(
            !retired.active
                && retired.retirement == Some(LiveCaptureRetirement::Superseded)
                && retired.admitted_pixel_bytes == 0
                && retired.queue.is_none()
                && retired_info.state == LiveCaptureInferenceState::Retired
                && retired_info.latest.is_none()
                && retired_info.error.is_none()
                && retired.input_close.is_none(),
            "predecessor did not retire at committed transfer"
        );
        let ResponseBody::BrowserAnalysisSuccessor(active) = client.engine.handle(prepare).result?
        else {
            bail!("prepared successor replay absent")
        };
        active.validate()?;
        ensure!(
            active.phase == BrowserAnalysisSuccessorPhase::Active
                && active.successor.identity == child.identity
                && active.transfer_request_id.as_ref() == Some(&transfer.request_id),
            "prepared successor did not promote under original request"
        );
        // The preview adapter releases predecessor custody after native promotion.
        release(client, media, player)?;
        child_player.observe(client).context(
            "successor timing must remain current after predecessor source/timing release",
        )?;
        let later = upload(client, inputs, &child, &mut child_player, base + 3, 2, end)?;
        let successor_second = estimate(
            client,
            inputs,
            &child,
            &mut child_player,
            (&next, base + 2),
            (&later, base + 3),
            2,
            end,
        )?;
        let sealed = close_input(client, &child, &mut child_player, &later, end)?;
        let stopped = stop(client, &child.identity.epoch)?;
        release(client, &child_media, &child_player)?;
        Ok(
            json!({"case":"prepared_successor_transfer","predecessor_source_and_timing_released_before_successor_refresh":true,"ready":ready,"prepared":child,"boundary":second,"mapped_seed":seed,"boundary_reserved":reserved,"transferred":transferred,"predecessor_retired":retired,"successor_sealed":sealed,"successor_stopped":stopped,"observed_actual_estimates":3,"estimates":[predecessor_estimate,successor_first,successor_second],"original_media":media,"successor_media":child_media,"original_timing_binding":player.binding,"successor_timing_binding":child_player.binding,"last_authored_timing_observation":child_player.last_observation,"captured_mode_preserved":true,"original_boundary_seeded_by_engine":true,"boundary_uploaded_to_successor":false,"predecessor_clean_join_observed":true,"promoted_successor_clean_eof_observed":true,"prepared_ingress_refused":true,"predecessor_ingress_refused_after_transfer":true,"original_lease_ms":LEASE_MS,"freshness_limit_ns":MAX_AGE_NS,"source_acceptance_called":false,"physical_device":false,"qualified":false}),
        )
    }
    fn custody_diagnostic_error(error: impl std::fmt::Display) -> String {
        error.to_string().chars().take(512).collect()
    }
    // One terminal observation only. A ResourcePool clone retains accounting,
    // not any reservation, native source pin, or Engine ownership.
    fn custody_pool_diagnostic(pool: &crate::resources::ResourcePool, maximum: u64) -> Value {
        let storage = pool.reserved_storage();
        let cpu = pool.reserved_cpu_threads();
        let ram = match pool.reserve_memory(maximum) {
            Ok(claim) => {
                drop(claim);
                json!({"requested_bytes":maximum,"readmitted":true,"error_code":null,"error":null})
            }
            Err(error) => json!({"requested_bytes":maximum,"readmitted":false,
                "error_code":error.code,"error":custody_diagnostic_error(&error)}),
        };
        json!({"reserved_storage_bytes":storage,
            "reserved_cpu_threads":cpu.as_ref().ok(),
            "cpu_error":cpu.as_ref().err().map(custody_diagnostic_error),
            "full_ram":ram})
    }
    // This is deliberately outside the worker-custody polling loop.
    // It cannot change either stage result or release registry metadata.
    fn custody_terminal_diagnostic(engine: &Arc<Engine>) -> Value {
        let worker_fence = engine.worker_fence.has_pending_worker_custody();
        let storage_fence = engine.worker_fence.has_pending_storage_custody();
        let worker_map = match engine.workers.try_lock() {
            Ok(workers) => json!({"count":workers.len(),"error":null}),
            Err(error) => json!({"count":null,"error":custody_diagnostic_error(error)}),
        };
        json!({"atomic_snapshot":false,"observation":"after_worker_custody_wait",
            "pending_worker_custody":worker_fence.as_ref().ok(),
            "worker_fence_error":worker_fence.as_ref().err().map(custody_diagnostic_error),
            "pending_storage_custody":storage_fence.as_ref().ok(),
            "storage_fence_error":storage_fence.as_ref().err().map(custody_diagnostic_error),
            "worker_map":worker_map,"pool":custody_pool_diagnostic(&engine.pool,engine.config.max_ram_bytes)})
    }
    fn await_worker_custody_clear(
        engine: &Arc<Engine>,
        expected_timing_entries: usize,
        started: Instant,
        deadline: Instant,
    ) -> Result<Value> {
        let (metadata_entries, metadata_bytes) = engine
            .browser_timing
            .lock()
            .map_err(|_| anyhow::anyhow!("browser timing registry unavailable"))?
            .retired_metadata_for_test()?;
        ensure!(
            metadata_entries == expected_timing_entries && metadata_bytes > 0,
            "retained browser timing metadata count differs from actual case"
        );
        let available = engine
            .config
            .max_ram_bytes
            .checked_sub(metadata_bytes)
            .filter(|bytes| *bytes > 0)
            .context("timing metadata exceeds configured RAM")?;
        let mut last_memory_error = None;
        loop {
            ensure!(Instant::now() < deadline,
                "browser worker/process custody did not clear within shared 20s cleanup budget; last memory error: {last_memory_error:?}");
            if !engine.worker_fence.has_pending_worker_custody()?
                && !engine.worker_fence.has_pending_storage_custody()?
                && engine.pool.reserved_storage() == 0
                && engine.pool.reserved_cpu_threads()? == 0
                && engine
                    .workers
                    .lock()
                    .map_err(|_| anyhow::anyhow!("worker map unavailable"))?
                    .is_empty()
            {
                match engine.pool.reserve_memory(available) {
                    Ok(claim) => {
                        drop(claim);
                        let extra = match engine.pool.reserve_memory(available + 1) {
                            Ok(claim) => {
                                drop(claim);
                                bail!("retained timing metadata does not occupy its exact declared pool budget");
                            }
                            Err(error) => error,
                        };
                        ensure!(
                            extra.code == ErrorCode::ResourceExhausted
                                && extra.message == "aggregate worker admission budget exhausted",
                            "one-byte overshoot was not refused by aggregate accounting: {extra}"
                        );
                        let completed = Instant::now();
                        ensure!(
                            completed < deadline,
                            "browser worker release completed after shared cleanup deadline"
                        );
                        return Ok(
                            json!({"scope":"worker_process_storage_cpu_and_nonmetadata_ram_before_engine_drop",
                            "pending_worker_custody":false,"pending_storage_custody":false,
                            "reserved_storage_bytes":0,"reserved_cpu_threads":0,"worker_map_count":0,
                            "retained_timing_metadata":{"entry_count":metadata_entries,"reserved_bytes":metadata_bytes,
                                "all_entries_retired":true,"sources_pending_probes_and_live_publications_absent":true,
                                "observations_invalidated":true},
                            "configured_ram_bytes":engine.config.max_ram_bytes,"available_ram_bytes":available,
                            "available_ram_readmitted":true,"one_extra_byte_rejected":true,
                            "one_extra_byte_error":extra,"exact_metadata_budget_accounted":true,
                            "completed_elapsed_ns":u64::try_from(completed.duration_since(started).as_nanos())?,
                            "shared_cleanup_budget_ms":20_000}),
                        );
                    }
                    Err(error) => last_memory_error = Some(custody_diagnostic_error(error)),
                }
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    fn await_all_resources_after_drop(
        pool: &crate::resources::ResourcePool,
        maximum: u64,
        engine: &std::sync::Weak<Engine>,
        started: Instant,
        deadline: Instant,
    ) -> Result<Value> {
        let mut last_memory_error = None;
        loop {
            ensure!(Instant::now() < deadline,
                "browser metadata custody did not clear within the same 20s cleanup budget; last memory error: {last_memory_error:?}");
            if engine.strong_count() == 0
                && pool.reserved_storage() == 0
                && pool.reserved_cpu_threads()? == 0
            {
                match pool.reserve_memory(maximum) {
                    Ok(claim) => {
                        drop(claim);
                        let completed = Instant::now();
                        ensure!(
                            completed < deadline,
                            "browser metadata release completed after shared cleanup deadline"
                        );
                        return Ok(
                            json!({"scope":"all_resource_accounting_after_last_engine_owner_drop",
                            "engine_strong_count":0,"reserved_storage_bytes":0,"reserved_cpu_threads":0,
                            "configured_ram_bytes":maximum,"full_ram_readmitted":true,
                            "completed_elapsed_ns":u64::try_from(completed.duration_since(started).as_nanos())?,
                            "shared_cleanup_budget_ms":20_000}),
                        );
                    }
                    Err(error) => last_memory_error = Some(custody_diagnostic_error(error)),
                }
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    fn campaign_case(inputs: &Inputs, mode: &str) -> Value {
        let directory = match tempfile::tempdir() {
            Ok(value) => value,
            Err(error) => return json!({"case":mode,"passed":false,"error":error.to_string()}),
        };
        let fixture_directory = directory.path().to_path_buf();
        let mut engine: Option<Arc<Engine>> = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
            let mut config =
                EngineConfig::new(directory.path().join("state"), inputs.worker.path.clone());
            config.cpu_owner_policy = Some(inputs.policy.path.clone());
            config.max_cpu_threads = inputs.owner_cpu_threads;
            config.worker_timeout = Duration::from_secs(60);
            engine = Some(Engine::open(config)?);
            let client = Client::new(engine.as_ref().unwrap())?;
            let media = prepare_media(&client, inputs)?;
            let original = client.snapshot()?;
            let mut player = AuthoredPlayer::open(&client, &media)?;
            let mut value = match mode {
                "clean_input_eof" => clean_eof(&client, inputs, &media, &mut player)?,
                "prepared_successor_transfer" => {
                    successor_transfer(&client, inputs, &media, &mut player)?
                }
                _ => bail!("unknown browser campaign case"),
            };
            let after = client.snapshot()?;
            ensure!(
                after.revision == original.revision
                    && encode(&after.motion)? == encode(&original.motion)?,
                "ephemeral browser native campaign changed committed motion"
            );
            value["committed_motion_unchanged"] = json!(true);
            Ok(value)
        }));
        let shutdown = engine
            .as_ref()
            .map(|engine| engine.finish_shutdown())
            .transpose();
        // One original cleanup allowance, starting where the former custody
        // wait started after finish_shutdown. Both stages share this deadline.
        let cleanup_started = Instant::now();
        let cleanup_deadline = cleanup_started + Duration::from_secs(20);
        let expected_timing_entries = match mode {
            "clean_input_eof" => 1,
            "prepared_successor_transfer" => 2,
            _ => 0,
        };
        let worker_custody = engine
            .as_ref()
            .map(|engine| {
                await_worker_custody_clear(
                    engine,
                    expected_timing_entries,
                    cleanup_started,
                    cleanup_deadline,
                )
            })
            .transpose();
        let custody_terminal = engine.as_ref().map(custody_terminal_diagnostic);
        let pool_observer = engine.as_ref().map(|engine| {
            (
                engine.pool.clone(),
                engine.config.max_ram_bytes,
                Arc::downgrade(engine),
            )
        });
        drop(engine);
        let all_resources = pool_observer
            .as_ref()
            .map(|(pool, maximum, engine)| {
                await_all_resources_after_drop(
                    pool,
                    *maximum,
                    engine,
                    cleanup_started,
                    cleanup_deadline,
                )
            })
            .transpose();
        // Owner destruction must not hide a failed pre-drop worker/byte proof.
        let joined = shutdown.is_ok() && worker_custody.is_ok() && all_resources.is_ok();
        let custody_after_engine_drop = pool_observer.as_ref().map(|(pool, maximum, engine)| {
            json!({"atomic_snapshot":false,"observation":"after_original_engine_owner_drop",
                "engine_strong_count":engine.strong_count(),"pool":custody_pool_diagnostic(pool,*maximum),
                "cannot_override_pre_drop_failure":true})
        });
        drop(pool_observer);
        let (removed, retained, cleanup_error) = if joined {
            match directory.close() {
                Ok(()) => (true, None, None),
                Err(error) => (false, None, Some(error.to_string())),
            }
        } else {
            (false, Some(directory.keep()), None)
        };
        let mut value = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => json!({"case":mode,"error":format!("{error:#}")}),
            Err(_) => {
                json!({"case":mode,"error":"native browser campaign panicked; see test output"})
            }
        };
        value["passed"] = json!(value.get("error").is_none() && joined && removed);
        value["fixture_directory"] = json!(fixture_directory);
        value["shutdown_joined"] = json!(shutdown.is_ok());
        value["worker_custody_clear_before_engine_drop"] = json!(worker_custody.is_ok());
        value["all_resources_released_after_engine_drop"] = json!(all_resources.is_ok());
        value["worker_release_before_engine_drop"] = json!(worker_custody.as_ref().ok());
        value["metadata_release_after_engine_drop"] = json!(all_resources.as_ref().ok());
        value["shared_cleanup_budget_ms"] = json!(20_000);
        value["both_cleanup_stages_required"] = json!(true);
        value["custody_terminal_diagnostic"] = json!(custody_terminal);
        value["custody_after_engine_drop_diagnostic"] = json!(custody_after_engine_drop);
        value["private_state_removed"] = json!(removed);
        value["retained_scratch"] = json!(retained);
        value["shutdown_error"] = json!(shutdown.err().map(|error| format!("{error:#}")));
        value["worker_custody_error"] =
            json!(worker_custody.err().map(|error| format!("{error:#}")));
        value["metadata_custody_error"] =
            json!(all_resources.err().map(|error| format!("{error:#}")));
        value["cleanup_error"] = json!(cleanup_error);
        value
    }
    fn save(path: &Path, value: &Value) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "native browser receipt exceeds bound"
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }
    #[test]
    #[ignore = "requires explicitly pinned sterile40s decoded fixture, current captured CPU worker/closure, delegated cgroups and owned host-parent supervisor; run exactly and serially"]
    fn native_captured_cpu_browser_input_seals_and_transfers_successor() {
        let output =
            path_env("PULSAR_CAPTURED_CPU_BROWSER_OUTPUT").expect("owned browser output directory");
        let mut receipt = json!({"schema_version":1,"passed":false,"scope":"Actual public Engine browser media/timing ownership, captured CPU RGB inference, clean input EOF and prepared native successor transfer over authored browser callback observations; no rendered-browser or device qualification","browser_capture":false,"authored_browser_observations":true,"physical_device":false,"qualified":false,"public_equivalence_acceptance_called":false,"source_acceptance_called":false,"simulated_eligibility":false,"cases":[],"not_exercised":["rendered browser/rVFC origin of submitted observations","trained-model usefulness","physical Handy synchronization","source/device qualification"],"error":null});
        let result = (|| -> Result<()> {
            ensure!(output.is_dir(), "output directory must already exist");
            let inputs = Inputs::load()?;
            receipt["subjects"] = inputs.subjects();
            let selected_case = std::env::var("PULSAR_CAPTURED_CPU_BROWSER_CASE").ok();
            ensure!(
                selected_case
                    .as_deref()
                    .is_none_or(|case| CASES.contains(&case)),
                "unknown selected native browser case"
            );
            for mode in CASES {
                if selected_case.as_deref().is_some_and(|case| case != mode) {
                    continue;
                }
                let value = campaign_case(&inputs, mode);
                let passed = value["passed"] == true;
                receipt["cases"].as_array_mut().unwrap().push(value);
                inputs.verify_unchanged()?;
                ensure!(passed, "captured browser native case failed: {mode}");
            }
            receipt["borrowed_subjects_unchanged"] = json!(true);
            Ok(())
        })();
        receipt["passed"] = json!(result.is_ok());
        receipt["error"] = json!(result.as_ref().err().map(|error| format!("{error:#}")));
        save(&output.join("browser-campaign.json"), &receipt)
            .expect("retain actual browser native campaign result");
        result.unwrap();
    }
}
