//! Parent-owned encoded-source transport. Delivery is not camera exposure time,
//! a capture lease, model qualification or permission to dispatch a device.
#![cfg(target_os = "linux")]
use crate::live_worker_lifetime::deadline as launch_deadline;
use crate::{resources::ResourcePool, EngineConfig};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn error(message: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message.to_string())
}
fn active(stop: &AtomicBool, cancel: &AtomicBool, deadline: Instant) -> Result<(), ProtocolError> {
    if stop.load(Ordering::Acquire) || cancel.load(Ordering::Acquire) {
        return Err(ProtocolError::new(
            ErrorCode::Cancelled,
            "native source retired or cancelled",
        ));
    }
    if Instant::now() >= deadline {
        return Err(error("native source absolute deadline exceeded"));
    }
    Ok(())
}

/// An immutable validated decoded body. Its timestamp describes native stdout
/// delivery in the worker clock, not source exposure or authenticated capture.
pub struct NativeSourceFrame {
    facts: NativeSourceDeliveryFacts,
    origin: Instant,
    pixels: Box<[u8]>,
}
impl NativeSourceFrame {
    pub fn facts(&self) -> &NativeSourceDeliveryFacts {
        &self.facts
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    /// Conservative native delivery age. This does not include unknown source
    /// exposure/encoding/decoding age and grants no capture lease authority.
    pub fn delivery_age_at(&self, now: Instant) -> Result<u64, ProtocolError> {
        if now > Instant::now() {
            return Err(ProtocolError::invalid(
                "native delivery age sample is in the future",
            ));
        }
        let tick = parent_tick(self.origin, now)?;
        if tick < self.facts.parent_receipt || tick >= self.facts.probe.parent_end {
            return Err(ProtocolError::invalid(
                "native delivery age sample precedes receipt or exceeds lifetime",
            ));
        }
        tick.0
            .checked_sub(self.facts.bounds.earliest.0)
            .ok_or_else(|| error("native delivery age regressed"))
    }
}
fn parent_tick(origin: Instant, event: Instant) -> Result<LiveClockTick, ProtocolError> {
    let delta = event
        .checked_duration_since(origin)
        .ok_or_else(|| ProtocolError::invalid("native parent clock precedes origin"))?;
    Ok(LiveClockTick(
        u64::try_from(delta.as_nanos()).map_err(error)?,
    ))
}

#[derive(Clone)]
struct Intake {
    bytes: u64,
    sha256: String,
}
/// A unique input handle. Drive this on a separate producer thread while the
/// owning worker reads frames; OS pipes deliberately apply bounded backpressure.
/// Dropping without explicit finish permanently retires the worker.
pub struct LiveSourceInput {
    stdin: Option<std::process::ChildStdin>,
    bytes: u64,
    hash: Sha256,
    maximum: u64,
    sealed: Arc<Mutex<Option<Intake>>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
}
impl LiveSourceInput {
    /// Local chunk/total-bound refusal leaves the input usable. A failed or
    /// ambiguous pipe write permanently retires it; bytes are never replayed.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ProtocolError> {
        active(&self.stop, &self.cancel, self.deadline)?;
        if bytes.is_empty() || bytes.len() > 65536 || bytes.len() as u64 > self.maximum - self.bytes
        {
            return Err(ProtocolError::invalid(
                "native source input chunk exceeds bounds",
            ));
        }
        if let Err(failure) = self
            .stdin
            .as_mut()
            .ok_or_else(|| error("native source input closed"))?
            .write_all(bytes)
        {
            self.stop.store(true, Ordering::Release);
            self.stdin.take();
            return Err(error(failure));
        }
        self.bytes += bytes.len() as u64;
        self.hash.update(bytes);
        if let Err(failure) = active(&self.stop, &self.cancel, self.deadline) {
            self.stop.store(true, Ordering::Release);
            self.stdin.take();
            return Err(failure);
        }
        Ok(())
    }
    /// Seals exact successfully forwarded bytes, then closes native stdin.
    pub fn finish(mut self) -> Result<(), ProtocolError> {
        active(&self.stop, &self.cancel, self.deadline)?;
        if self.bytes == 0 {
            return Err(ProtocolError::invalid("native source input is empty"));
        }
        *self.sealed.lock().map_err(error)? = Some(Intake {
            bytes: self.bytes,
            sha256: format!("{:x}", self.hash.clone().finalize()),
        });
        self.stdin.take();
        Ok(())
    }
}
impl Drop for LiveSourceInput {
    fn drop(&mut self) {
        if self.stdin.is_some() {
            self.stop.store(true, Ordering::Release);
        }
    }
}

struct Packet {
    reply: LiveDecodeReply,
    pixels: Box<[u8]>,
    received: Instant,
}
struct Sequence {
    manifest: LiveDecodeManifest,
    ready: Option<u64>,
    frames: u32,
    source: Option<SourceTimestamp>,
    delivered: u64,
    ended: bool,
}
impl Sequence {
    fn accept(&mut self, packet: &Packet, sealed: Option<&Intake>) -> Result<(), ProtocolError> {
        packet.reply.validate(&self.manifest)?;
        if self.ended {
            return Err(error("native source reply after completion"));
        }
        match &packet.reply {
            LiveDecodeReply::Ready {
                worker_elapsed_ns, ..
            } if self.ready.is_none() && packet.pixels.is_empty() => {
                self.ready = Some(*worker_elapsed_ns);
                self.delivered = *worker_elapsed_ns;
            }
            LiveDecodeReply::Frame {
                frame,
                source_time,
                delivered_elapsed_ns,
                pixels,
                ..
            } if self.ready.is_some() => {
                let increasing = self.source.is_none_or(|old| {
                    i128::from(source_time.numerator()) * i128::from(old.denominator())
                        > i128::from(old.numerator()) * i128::from(source_time.denominator())
                });
                if frame.value() != u64::from(self.frames)
                    || !increasing
                    || *delivered_elapsed_ns < self.delivered
                    || packet.pixels.len() != LIVE_FRAME_BYTES
                    || format!("{:x}", Sha256::digest(&packet.pixels)) != pixels.sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "native source frame sequence/clock/body differs",
                    ));
                }
                self.frames += 1;
                self.source = Some(*source_time);
                self.delivered = *delivered_elapsed_ns;
            }
            LiveDecodeReply::End {
                frames,
                input_sha256,
                input_bytes,
                ..
            } if self.ready.is_some() && packet.pixels.is_empty() => {
                let intake = sealed
                    .ok_or_else(|| error("native source completed before parent input sealed"))?;
                if *frames != self.frames
                    || *input_bytes != intake.bytes
                    || *input_sha256 != intake.sha256
                {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "native source completion differs from parent intake",
                    ));
                }
                self.ended = true;
            }
            LiveDecodeReply::Error(failure) => return Err(failure.clone()),
            _ => return Err(error("native source reply out of order")),
        }
        Ok(())
    }
}

pub struct LiveSourceWorker {
    sequence: Sequence,
    origin: Instant,
    clock: Option<LiveDecodeClockProbe>,
    completion: Option<NativeSourceCompletionFacts>,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<std::process::Child>>,
    group: Arc<crate::cgroup::AttemptCgroup>,
    input: Option<LiveSourceInput>,
    sealed: Arc<Mutex<Option<Intake>>>,
    output: Option<mpsc::Receiver<Result<Packet, ProtocolError>>>,
    custody: Option<crate::worker_retirement::WorkerCustody>,
}
impl LiveSourceWorker {
    /// Snapshot exact admitted bytes and launch with independent confinement,
    /// resource admission and expiry. The caller owns source/capture authority.
    pub fn start(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveDecodeManifest,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, ProtocolError> {
        manifest.validate()?;
        let origin = Instant::now();
        let end = origin + Duration::from_millis(manifest.budget.wall_time_ms);
        Self::start_in_lease(config, pool, manifest, cancel, origin, end)
    }
    /// Launch in an existing process-local lease clock. The absolute end is
    /// never reset by staging, readiness, input or output. These Instant values
    /// are runtime facts; callers separately enforce authenticated authority.
    pub fn start_in_lease(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveDecodeManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
    ) -> Result<Self, ProtocolError> {
        Self::start_owned(config, pool, manifest, cancel, origin, lease_end, None)
    }
    pub(crate) fn start_tracked_in_lease(
        config: &EngineConfig,
        pool: &ResourcePool,
        manifest: LiveDecodeManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
        fence: Arc<crate::worker_fence::WorkerFence>,
    ) -> Result<Self, ProtocolError> {
        Self::start_owned(
            config,
            pool,
            manifest,
            cancel,
            origin,
            lease_end,
            Some(fence),
        )
    }
    fn start_owned(
        config: &EngineConfig,
        pool: &ResourcePool,
        mut manifest: LiveDecodeManifest,
        cancel: Arc<AtomicBool>,
        origin: Instant,
        lease_end: Instant,
        fence: Option<Arc<crate::worker_fence::WorkerFence>>,
    ) -> Result<Self, ProtocolError> {
        use std::os::unix::fs::PermissionsExt;
        manifest.validate()?;
        let deadline = launch_deadline(
            origin,
            lease_end,
            manifest.budget.wall_time_ms,
            Instant::now(),
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        active(&stop, &cancel, deadline)?;
        if !crate::launch::available() {
            return Err(ProtocolError::unsupported("native source confinement"));
        }
        if manifest.budget.memory_bytes > config.max_ram_bytes
            || Duration::from_millis(manifest.budget.wall_time_ms) > config.worker_timeout
        {
            return Err(ProtocolError::invalid(
                "native source exceeds configured limits",
            ));
        }
        let refs = [
            ArtifactReference {
                path: config.worker_executable.clone(),
                identity: manifest.worker.clone(),
            },
            manifest.ffmpeg.clone(),
        ];
        let prepared = crate::live_staging::prepare(
            config,
            pool,
            refs,
            crate::live_staging::PreparationBudget {
                memory_bytes: manifest.budget.memory_bytes,
                cpu_threads: manifest.budget.cpu_threads,
            },
            cancel.clone(),
            deadline,
            fence,
        )?;
        active(&stop, &cancel, deadline)?;
        let staged = prepared.paths().clone();
        std::fs::set_permissions(&staged[1], std::fs::Permissions::from_mode(0o500))
            .map_err(error)?;
        let mut config = config.clone();
        config.worker_executable = staged[0].clone();
        manifest.ffmpeg.path = "/input/ffmpeg".into();
        let mut custody = prepared.into_worker(
            manifest.attempt_id.as_str(),
            manifest.budget.memory_bytes,
            manifest.budget.cpu_threads,
        )?;
        let group = custody.group_handle();
        let mut command = crate::confinement::command(
            &config,
            &manifest.budget,
            manifest.budget.memory_bytes,
            &group,
            &[(staged[1].clone(), manifest.ffmpeg.path.clone())],
            std::path::Path::new("/work"),
            "capture-worker",
        )
        .map_err(error)?;
        active(&stop, &cancel, deadline)?;
        custody.spawn(&mut command).map_err(error)?;
        let child = custody.child_handle();
        let pipes = {
            let mut child = custody.child();
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        };
        let (Some(mut stdin), Some(mut stdout), Some(stderr)) = pipes else {
            return Err(error("native source pipes absent"));
        };
        let watchdog = {
            let child = child.clone();
            let group = group.clone();
            let stop = stop.clone();
            let cancel = cancel.clone();
            std::thread::Builder::new()
                .name("pulsar-live-watchdog".into())
                .spawn(move || {
                    while active(&stop, &cancel, deadline).is_ok() {
                        if child
                            .try_lock()
                            .is_ok_and(|mut c| c.try_wait().map_or(true, |s| s.is_some()))
                        {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    crate::worker_retirement::signal_and_poll_child(
                        &group,
                        &child,
                        Instant::now() + Duration::from_millis(250),
                    );
                })
                .map_err(error)?
        };
        custody.retain_thread(watchdog);
        let (tx, output) = mpsc::sync_channel(1);
        let guest = manifest.clone();
        let reader = std::thread::Builder::new()
            .name("pulsar-live-reader".into())
            .spawn(move || {
                loop {
                    let result = (|| {
                        let reply: LiveDecodeReply =
                            read_bulk_header(&mut stdout).map_err(error)?;
                        reply.validate(&guest)?;
                        let pixels = if matches!(reply, LiveDecodeReply::Frame { .. }) {
                            let mut body = vec![0; LIVE_FRAME_BYTES].into_boxed_slice();
                            stdout.read_exact(&mut body).map_err(error)?;
                            body
                        } else {
                            Box::default()
                        };
                        // Completion requires terminal EOF, refusing appended output.
                        if matches!(reply, LiveDecodeReply::End { .. }) {
                            let mut extra = [0];
                            if stdout.read(&mut extra).map_err(error)? != 0 {
                                return Err(error("native source trailing output"));
                            }
                        }
                        Ok(Packet {
                            reply,
                            pixels,
                            received: Instant::now(),
                        })
                    })();
                    let terminal = result.as_ref().map_or(true, |p| {
                        matches!(
                            p.reply,
                            LiveDecodeReply::End { .. } | LiveDecodeReply::Error(_)
                        )
                    });
                    if tx.send(result).is_err() || terminal {
                        break;
                    }
                }
            })
            .map_err(error)?;
        custody.retain_thread(reader);
        let diagnostics = std::thread::Builder::new()
            .name("pulsar-live-diagnostics".into())
            .spawn(move || {
                let mut bytes = Vec::new();
                let _ = stderr.take(16 * 1024).read_to_end(&mut bytes);
            })
            .map_err(error)?;
        custody.retain_thread(diagnostics);
        let sealed = Arc::new(Mutex::new(None));
        let mut worker = Self {
            origin,
            clock: None,
            completion: None,
            sequence: Sequence {
                manifest: manifest.clone(),
                ready: None,
                frames: 0,
                source: None,
                delivered: 0,
                ended: false,
            },
            deadline,
            cancel: cancel.clone(),
            stop: stop.clone(),
            child,
            group,
            input: None,
            sealed: sealed.clone(),
            output: Some(output),
            custody: Some(custody),
        };
        let sent = Instant::now();
        write_bulk_header(&mut stdin, &manifest).map_err(error)?;
        let ready = worker.receive()?;
        worker.sequence.accept(&ready, None)?;
        let LiveDecodeReply::Ready {
            worker_elapsed_ns, ..
        } = ready.reply
        else {
            return Err(error("native source readiness absent"));
        };
        let clock = LiveDecodeClockProbe {
            parent_send: parent_tick(origin, sent)?,
            parent_receive: parent_tick(origin, ready.received)?,
            worker_ready_elapsed_ns: worker_elapsed_ns,
            parent_end: parent_tick(origin, deadline)?,
            maximum_uncertainty_ns: 1_000_000_000,
        };
        clock
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        worker.clock = Some(clock);
        worker.input = Some(LiveSourceInput {
            stdin: Some(stdin),
            bytes: 0,
            hash: Sha256::new(),
            maximum: manifest.maximum_input_bytes,
            sealed,
            stop,
            cancel,
            deadline,
        });
        Ok(worker)
    }
    /// Transfers the sole input handle; no clone or reacquisition is available.
    pub fn take_input(&mut self) -> Result<LiveSourceInput, ProtocolError> {
        active(&self.stop, &self.cancel, self.deadline)?;
        self.input
            .take()
            .ok_or_else(|| error("native source input already transferred"))
    }
    pub fn clock_probe(&self) -> &LiveDecodeClockProbe {
        self.clock.as_ref().expect("accepted readiness")
    }
    pub fn completion(&self) -> Option<&NativeSourceCompletionFacts> {
        self.completion.as_ref()
    }
    fn receive(&self) -> Result<Packet, ProtocolError> {
        loop {
            active(&self.stop, &self.cancel, self.deadline)?;
            match self
                .output
                .as_ref()
                .ok_or_else(|| error("native source closed"))?
                .recv_timeout(Duration::from_millis(10))
            {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(error("native source stream closed")),
            }
        }
    }
    /// A successful terminal None proves exact input/output correspondence and
    /// native exit success. Every failure permanently closes the worker.
    pub fn next_frame(&mut self) -> Result<Option<NativeSourceFrame>, ProtocolError> {
        let result = self.next_inner();
        if result.is_err() {
            self.close();
        }
        result
    }
    fn next_inner(&mut self) -> Result<Option<NativeSourceFrame>, ProtocolError> {
        if self.sequence.ended {
            return Err(error("native source already completed"));
        }
        let packet = self.receive()?;
        let sealed = self.sealed.lock().map_err(error)?.clone();
        self.sequence.accept(&packet, sealed.as_ref())?;
        match packet.reply {
            LiveDecodeReply::Frame {
                frame,
                source_time,
                delivered_elapsed_ns,
                pixels,
                attempt_id,
                identity,
            } => {
                let probe = self.clock_probe().clone();
                let parent_receipt = parent_tick(self.origin, packet.received)?;
                let bounds = probe
                    .bounds(delivered_elapsed_ns, parent_receipt)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                let facts = NativeSourceDeliveryFacts {
                    attempt_id,
                    identity,
                    worker: self.sequence.manifest.worker.clone(),
                    ffmpeg: self.sequence.manifest.ffmpeg.identity.clone(),
                    frame,
                    source_time,
                    worker_delivered_elapsed_ns: delivered_elapsed_ns,
                    parent_receipt,
                    pixels,
                    probe,
                    bounds,
                    age_basis: NativeDeliveryAgeBasis::DecodedStdoutLocalMonotonicEqualRate,
                };
                facts.validate(&self.sequence.manifest)?;
                active(&self.stop, &self.cancel, self.deadline)?;
                Ok(Some(NativeSourceFrame {
                    facts,
                    origin: self.origin,
                    pixels: packet.pixels,
                }))
            }
            LiveDecodeReply::End { .. } => {
                loop {
                    active(&self.stop, &self.cancel, self.deadline)?;
                    let status = self
                        .child
                        .lock()
                        .map_err(error)?
                        .try_wait()
                        .map_err(error)?;
                    if let Some(status) = status {
                        if !status.success() {
                            return Err(error("native source exited unsuccessfully"));
                        }
                        self.custody
                            .as_mut()
                            .ok_or_else(|| error("native source custody absent"))?
                            .retire(self.deadline)
                            .map_err(error)?;
                        let intake =
                            self.sealed.lock().map_err(error)?.clone().ok_or_else(|| {
                                error("native source completion lacks parent seal")
                            })?;
                        self.completion = Some(NativeSourceCompletionFacts {
                            attempt_id: self.sequence.manifest.attempt_id.clone(),
                            identity: self.sequence.manifest.identity.clone(),
                            frames: self.sequence.frames,
                            input_sha256: intake.sha256,
                            input_bytes: intake.bytes,
                        });
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            _ => Err(error("native source unexpected reply")),
        }
    }
    pub fn process_id(&self) -> Result<u32, ProtocolError> {
        Ok(self.child.lock().map_err(error)?.id())
    }
    pub fn peak_memory_bytes(&self) -> Result<u64, ProtocolError> {
        self.group.peak_memory_bytes().map_err(error)
    }
    fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.input.take();
        self.output.take();
        if let Some(mut custody) = self.custody.take() {
            let _ = custody.retire(Instant::now() + Duration::from_millis(250));
        }
    }
}
impl Drop for LiveSourceWorker {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sequence() -> Sequence {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 100,
        };
        Sequence {
            manifest: LiveDecodeManifest {
                version: PROTOCOL_VERSION,
                attempt_id: AttemptId::new("attempt").unwrap(),
                identity: LiveIdentity {
                    epoch: AttemptId::new("epoch").unwrap(),
                    project: ProjectId::new("project").unwrap(),
                    revision: RevisionId::new(0),
                    source: SourceVersionId::new("source").unwrap(),
                },
                ffmpeg: ArtifactReference {
                    path: "/input/ffmpeg".into(),
                    identity: artifact.clone(),
                },
                worker: artifact,
                format: LiveEncodedFormat::Yuv4mpegPipe,
                maximum_input_bytes: 1000,
                maximum_frames: 10,
                budget: ResourceBudget {
                    memory_bytes: 1 << 30,
                    output_bytes: 1 << 20,
                    wall_time_ms: 3000,
                    cpu_threads: 1,
                },
            },
            ready: None,
            frames: 0,
            source: None,
            delivered: 0,
            ended: false,
        }
    }
    fn ready(s: &Sequence) -> Packet {
        Packet {
            received: Instant::now(),
            reply: LiveDecodeReply::Ready {
                attempt_id: s.manifest.attempt_id.clone(),
                identity: s.manifest.identity.clone(),
                format: s.manifest.format,
                worker_elapsed_ns: 10,
            },
            pixels: Box::default(),
        }
    }
    fn frame(s: &Sequence, n: u64, time: SourceTimestamp, delivery: u64) -> Packet {
        let pixels = vec![7; LIVE_FRAME_BYTES].into_boxed_slice();
        let sha256 = format!("{:x}", Sha256::digest(&pixels));
        Packet {
            received: Instant::now(),
            reply: LiveDecodeReply::Frame {
                attempt_id: s.manifest.attempt_id.clone(),
                identity: s.manifest.identity.clone(),
                frame: FrameId::new(n),
                source_time: time,
                delivered_elapsed_ns: delivery,
                pixels: ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                    sha256,
                    byte_len: LIVE_FRAME_BYTES as u64,
                },
            },
            pixels,
        }
    }
    fn end(s: &Sequence) -> Packet {
        Packet {
            received: Instant::now(),
            reply: LiveDecodeReply::End {
                attempt_id: s.manifest.attempt_id.clone(),
                identity: s.manifest.identity.clone(),
                frames: 1,
                input_sha256: "b".repeat(64),
                input_bytes: 100,
            },
            pixels: Box::default(),
        }
    }
    #[test]
    fn native_launch_does_not_extend_an_existing_lease() {
        let now = Instant::now();
        let origin = now - Duration::from_secs(10);
        let end = origin + Duration::from_secs(30);
        assert_eq!(launch_deadline(origin, end, 30000, now).unwrap(), end);
        assert_eq!(
            launch_deadline(origin, end, 1000, now).unwrap(),
            now + Duration::from_secs(1)
        );
        assert!(launch_deadline(now + Duration::from_millis(1), end, 30000, now).is_err());
        assert!(launch_deadline(origin, now, 30000, now).is_err());
        assert!(launch_deadline(origin, origin + Duration::from_secs(31), 30000, now).is_err());
        assert!(launch_deadline(origin, end, 0, now).is_err());
    }
    #[test]
    fn immutable_native_delivery_age_is_conservative_and_lifetime_bound() {
        let sequence = sequence();
        let manifest = &sequence.manifest;
        let packet = frame(&sequence, 0, SourceTimestamp::new(-1, 3).unwrap(), 20);
        let LiveDecodeReply::Frame { pixels, .. } = packet.reply else {
            unreachable!()
        };
        let probe = LiveDecodeClockProbe {
            parent_send: LiveClockTick(100),
            parent_receive: LiveClockTick(200),
            worker_ready_elapsed_ns: 10,
            parent_end: LiveClockTick(3000000000),
            maximum_uncertainty_ns: 100,
        };
        let facts = NativeSourceDeliveryFacts {
            attempt_id: manifest.attempt_id.clone(),
            identity: manifest.identity.clone(),
            worker: manifest.worker.clone(),
            ffmpeg: manifest.ffmpeg.identity.clone(),
            frame: FrameId::new(0),
            source_time: SourceTimestamp::new(-1, 3).unwrap(),
            worker_delivered_elapsed_ns: 20,
            parent_receipt: LiveClockTick(300),
            pixels,
            bounds: probe.bounds(20, LiveClockTick(300)).unwrap(),
            probe,
            age_basis: NativeDeliveryAgeBasis::DecodedStdoutLocalMonotonicEqualRate,
        };
        facts.validate(manifest).unwrap();
        let now = Instant::now();
        let origin = now - Duration::from_nanos(1000);
        let mut f = NativeSourceFrame {
            facts,
            origin,
            pixels: packet.pixels,
        };
        assert_eq!(f.delivery_age_at(now).unwrap(), 890);
        assert!(f
            .delivery_age_at(origin + Duration::from_nanos(299))
            .is_err());
        assert!(f
            .delivery_age_at(Instant::now() + Duration::from_secs(1))
            .is_err());
        assert!(f.delivery_age_at(origin - Duration::from_nanos(1)).is_err());
        f.origin = now - Duration::from_secs(4);
        assert!(f.delivery_age_at(now).is_err());
    }
    #[test]
    fn source_sequence_rejects_frames_before_ready_and_duplicate_ready() {
        let mut s = sequence();
        assert!(s
            .accept(&frame(&s, 0, SourceTimestamp::new(0, 1).unwrap(), 20), None)
            .is_err());
        s.accept(&ready(&s), None).unwrap();
        assert!(s.accept(&ready(&s), None).is_err());
    }
    #[test]
    fn source_sequence_requires_exact_ordinal_and_monotonic_rational_pts() {
        let mut s = sequence();
        s.accept(&ready(&s), None).unwrap();
        assert!(s
            .accept(
                &frame(&s, 1, SourceTimestamp::new(-1, 3).unwrap(), 20),
                None
            )
            .is_err());
        s.accept(
            &frame(&s, 0, SourceTimestamp::new(-1, 3).unwrap(), 20),
            None,
        )
        .unwrap();
        assert!(s
            .accept(
                &frame(&s, 1, SourceTimestamp::new(-2, 6).unwrap(), 21),
                None
            )
            .is_err());
        assert!(s
            .accept(
                &frame(&s, 1, SourceTimestamp::new(-1, 2).unwrap(), 21),
                None
            )
            .is_err());
        s.accept(
            &frame(&s, 1, SourceTimestamp::new(-1, 4).unwrap(), 21),
            None,
        )
        .unwrap();
    }
    #[test]
    fn source_sequence_refuses_delivery_regression_and_mutated_pixel_body() {
        let mut s = sequence();
        s.accept(&ready(&s), None).unwrap();
        assert!(s
            .accept(&frame(&s, 0, SourceTimestamp::new(0, 1).unwrap(), 9), None)
            .is_err());
        let mut p = frame(&s, 0, SourceTimestamp::new(0, 1).unwrap(), 20);
        p.pixels[0] ^= 1;
        assert!(s.accept(&p, None).is_err());
        let mut p = frame(&s, 0, SourceTimestamp::new(0, 1).unwrap(), 20);
        p.pixels = Box::default();
        assert!(s.accept(&p, None).is_err());
    }
    #[test]
    fn source_end_requires_parent_seal_exact_counts_hash_and_no_later_reply() {
        let mut s = sequence();
        s.accept(&ready(&s), None).unwrap();
        s.accept(&frame(&s, 0, SourceTimestamp::new(0, 1).unwrap(), 20), None)
            .unwrap();
        assert!(s.accept(&end(&s), None).is_err());
        assert!(s
            .accept(
                &end(&s),
                Some(&Intake {
                    bytes: 101,
                    sha256: "b".repeat(64)
                })
            )
            .is_err());
        assert!(s
            .accept(
                &end(&s),
                Some(&Intake {
                    bytes: 100,
                    sha256: "c".repeat(64)
                })
            )
            .is_err());
        let mut wrong = end(&s);
        if let LiveDecodeReply::End { frames, .. } = &mut wrong.reply {
            *frames = 2;
        }
        assert!(s
            .accept(
                &wrong,
                Some(&Intake {
                    bytes: 100,
                    sha256: "b".repeat(64)
                })
            )
            .is_err());
        s.accept(
            &end(&s),
            Some(&Intake {
                bytes: 100,
                sha256: "b".repeat(64),
            }),
        )
        .unwrap();
        assert!(s
            .accept(
                &end(&s),
                Some(&Intake {
                    bytes: 100,
                    sha256: "b".repeat(64)
                })
            )
            .is_err());
    }
}
