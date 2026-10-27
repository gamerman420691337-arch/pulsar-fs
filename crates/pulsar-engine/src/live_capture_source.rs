//! Lease-owned encoded input and native publication. Native I/O is outside
//! authority locks; every input/frame effect rechecks current authority.
use super::*;
pub(super) fn check_available(engine: &Engine) -> PResult<()> {
    if !cfg!(target_os = "linux") || !launch::available() {
        return Err(ProtocolError::unsupported("encoded source confinement"));
    }
    engine.tools.as_ref().map_err(unavailable)?;
    Ok(())
}
#[cfg(target_os = "linux")]
use crate::live_source::{LiveSourceInput, LiveSourceWorker, NativeSourceFrame};
#[cfg(target_os = "linux")]
use std::sync::{mpsc, Weak};

#[cfg(target_os = "linux")]
pub(crate) struct EncodedConnection {
    engine: Weak<Engine>,
    handshake: LiveBulkHandshake,
    attempt: AttemptId,
    input: Option<LiveSourceInput>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    maximum: u64,
    sequence: u64,
    bytes: u64,
    finished: bool,
    done: mpsc::Receiver<PResult<()>>,
}
#[cfg(target_os = "linux")]
impl EncodedConnection {
    pub(crate) fn reject(&self, error: ProtocolError) {
        if let Some(engine) = self.engine.upgrade() {
            let _ = engine.source_failed(&self.handshake.identity.epoch, &self.attempt, error);
        }
    }
    fn engine(&self) -> PResult<Arc<Engine>> {
        self.engine
            .upgrade()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unavailable, "source engine retired"))
    }
    pub(crate) fn ready(&self) -> PResult<LiveCaptureReport> {
        self.engine()?
            .source_guard(&self.handshake, &self.attempt, |entry| report(entry))
    }
    pub(crate) fn validate(&self, request: &LiveEncodedRequest) -> PResult<()> {
        request.validate()?;
        if request.sequence() != self.sequence || self.finished {
            return Err(ProtocolError::invalid(
                "encoded input sequence differs or completed",
            ));
        }
        if let LiveEncodedRequest::Push { byte_len, .. } = request {
            if u64::from(*byte_len) > self.maximum - self.bytes {
                return Err(ProtocolError::invalid(
                    "encoded input total exceeds admission",
                ));
            }
        }
        self.engine()?
            .source_guard(&self.handshake, &self.attempt, |entry| {
                if entry
                    .report
                    .encoded_source
                    .as_ref()
                    .expect("admitted")
                    .state
                    != LiveEncodedSourceState::Running
                {
                    return Err(ProtocolError::invalid("encoded source is not running"));
                }
                Ok(())
            })
    }
    pub(crate) fn push(
        &mut self,
        request: &LiveEncodedRequest,
        bytes: &[u8],
    ) -> PResult<LiveCaptureReport> {
        self.validate(request)?;
        let LiveEncodedRequest::Push {
            byte_len, sha256, ..
        } = request
        else {
            return Err(ProtocolError::invalid("encoded push required"));
        };
        if bytes.len() != *byte_len as usize || format!("{:x}", Sha256::digest(bytes)) != *sha256 {
            return Err(ProtocolError::invalid("encoded input body differs"));
        }
        self.input
            .as_mut()
            .ok_or_else(|| ProtocolError::invalid("encoded input closed"))?
            .push(bytes)?;
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ProtocolError::invalid("encoded intake overflow"))?;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| ProtocolError::invalid("encoded sequence overflow"))?;
        self.engine()?
            .source_guard(&self.handshake, &self.attempt, |entry| {
                entry
                    .report
                    .encoded_source
                    .as_mut()
                    .expect("admitted")
                    .input_bytes = self.bytes;
                report(entry)
            })
    }
    pub(crate) fn finish(&mut self, request: &LiveEncodedRequest) -> PResult<LiveCaptureReport> {
        self.validate(request)?;
        if !matches!(request, LiveEncodedRequest::Finish { .. }) {
            return Err(ProtocolError::invalid("encoded finish required"));
        }
        self.input
            .take()
            .ok_or_else(|| ProtocolError::invalid("encoded input closed"))?
            .finish()?;
        loop {
            if self.cancel.load(Ordering::Acquire) || Instant::now() >= self.deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Cancelled,
                    "encoded source retired or expired",
                ));
            }
            match self.done.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => {
                    result?;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    return Err(ProtocolError::new(
                        ErrorCode::Unavailable,
                        "encoded source reader closed",
                    ))
                }
            }
        }
        let report = self.ready()?;
        if report
            .encoded_source
            .as_ref()
            .is_none_or(|s| s.state != LiveEncodedSourceState::Finished)
        {
            return Err(ProtocolError::invalid(
                "encoded source lacks verified completion",
            ));
        }
        self.finished = true;
        Ok(report)
    }
}
#[cfg(target_os = "linux")]
impl Drop for EncodedConnection {
    fn drop(&mut self) {
        self.input.take();
        if !self.finished {
            self.cancel.store(true, Ordering::Release);
            if let Some(engine) = self.engine.upgrade() {
                let _ = engine.source_failed(
                    &self.handshake.identity.epoch,
                    &self.attempt,
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "encoded connection ended without verified completion",
                    ),
                );
            }
        }
        // The Engine task group retains and joins the reader and its worker custody.
    }
}
#[cfg(not(target_os = "linux"))]
pub(crate) struct EncodedConnection;
#[cfg(not(target_os = "linux"))]
impl EncodedConnection {
    pub(crate) fn reject(&self, _: ProtocolError) {}
    pub(crate) fn ready(&self) -> PResult<LiveCaptureReport> {
        Err(ProtocolError::unsupported("encoded source confinement"))
    }
    pub(crate) fn validate(&self, _: &LiveEncodedRequest) -> PResult<()> {
        Err(ProtocolError::unsupported("encoded source confinement"))
    }
    pub(crate) fn push(&mut self, _: &LiveEncodedRequest, _: &[u8]) -> PResult<LiveCaptureReport> {
        self.ready()
    }
    pub(crate) fn finish(&mut self, _: &LiveEncodedRequest) -> PResult<LiveCaptureReport> {
        self.ready()
    }
}
impl Engine {
    pub(crate) fn encoded_source_open(
        self: &Arc<Self>,
        handshake: &LiveBulkHandshake,
    ) -> PResult<EncodedConnection> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = handshake;
            return Err(ProtocolError::unsupported("encoded source confinement"));
        }
        #[cfg(target_os = "linux")]
        {
            check_available(self)?;
            let (manifest, origin, end, cancel) = {
                let db = self.db.lock().map_err(internal)?;
                let mut registry = self.live_captures.lock().map_err(internal)?;
                registry.sweep(&db)?;
                self.authorize_live_bulk(&db, handshake)?;
                self.require_worker_fence(&db)?;
                if handshake.channel != LiveBulkChannel::LiveEncodedSource {
                    return Err(ProtocolError::invalid("encoded channel marker required"));
                }
                let entry = live_entry(&mut registry, handshake)?;
                let options = entry
                    .report
                    .options
                    .encoded_source
                    .as_ref()
                    .ok_or_else(|| ProtocolError::invalid("capture did not admit encoded input"))?;
                if entry
                    .report
                    .encoded_source
                    .as_ref()
                    .expect("admitted")
                    .state
                    != LiveEncodedSourceState::Pending
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "encoded source already attached; no rearm",
                    ));
                }
                let manifest = LiveDecodeManifest {
                    version: PROTOCOL_VERSION,
                    attempt_id: id!(AttemptId),
                    identity: entry.report.identity.clone(),
                    ffmpeg: self.tools.as_ref().map_err(unavailable)?.ffmpeg.clone(),
                    worker: self.binary_identity.clone(),
                    format: options.format,
                    maximum_input_bytes: options.maximum_input_bytes,
                    maximum_frames: options.maximum_frames,
                    budget: ResourceBudget {
                        memory_bytes: 1024 * 1024 * 1024,
                        output_bytes: 1024 * 1024,
                        wall_time_ms: u64::from(entry.report.options.lease_ms),
                        cpu_threads: 2,
                    },
                };
                manifest.validate()?;
                let info = entry.report.encoded_source.as_mut().expect("admitted");
                info.state = LiveEncodedSourceState::Starting;
                info.manifest = Some(manifest.clone());
                (
                    manifest,
                    entry.created,
                    entry.created + Duration::from_millis(u64::from(entry.report.options.lease_ms)),
                    entry.cancel.clone(),
                )
            };
            let attempt = manifest.attempt_id.clone();
            let opened = (|| -> PResult<EncodedConnection> {
                let mut worker = LiveSourceWorker::start_tracked_in_lease(
                    &self.config,
                    &self.pool,
                    manifest.clone(),
                    cancel.clone(),
                    origin,
                    end,
                    self.worker_fence.clone(),
                )?;
                self.source_guard(handshake, &attempt, |entry| {
                    entry
                        .report
                        .encoded_source
                        .as_mut()
                        .expect("admitted")
                        .state = LiveEncodedSourceState::Running;
                    Ok(())
                })?;
                let input = worker.take_input()?;
                let weak = Arc::downgrade(self);
                let scope = handshake.clone();
                let native_attempt = attempt.clone();
                let (tx, done) = mpsc::sync_channel(1);
                self.spawn_task("pulsar-live-source", None, move || {
                    let result = (|| -> PResult<()> {
                        while let Some(frame) = worker.next_frame()? {
                            let engine = weak.upgrade().ok_or_else(|| {
                                ProtocolError::new(ErrorCode::Unavailable, "source engine retired")
                            })?;
                            engine.source_frame(&scope, &native_attempt, &frame)?;
                        }
                        let end = worker
                            .completion()
                            .ok_or_else(|| ProtocolError::invalid("native completion absent"))?
                            .clone();
                        let engine = weak.upgrade().ok_or_else(|| {
                            ProtocolError::new(ErrorCode::Unavailable, "source engine retired")
                        })?;
                        engine.source_guard(&scope, &native_attempt, |entry| {
                            let info = entry.report.encoded_source.as_mut().expect("admitted");
                            end.validate(info.manifest.as_ref().expect("launched"))?;
                            if end.input_bytes != info.input_bytes
                                || u64::from(end.frames) != entry.report.received_frames
                            {
                                return Err(ProtocolError::invalid(
                                    "source completion differs from published intake/frames",
                                ));
                            }
                            info.state = LiveEncodedSourceState::Finished;
                            info.completion = Some(end);
                            report(entry)?;
                            Ok(())
                        })
                    })();
                    drop(worker);
                    if let Err(error) = &result {
                        if let Some(engine) = weak.upgrade() {
                            let _ = engine.source_failed(
                                &scope.identity.epoch,
                                &native_attempt,
                                error.clone(),
                            );
                        }
                    }
                    let _ = tx.send(result);
                })
                .map_err(internal)?;
                Ok(EncodedConnection {
                    engine: Arc::downgrade(self),
                    handshake: handshake.clone(),
                    attempt: attempt.clone(),
                    input: Some(input),
                    cancel,
                    deadline: end,
                    maximum: manifest.maximum_input_bytes,
                    sequence: 0,
                    bytes: 0,
                    finished: false,
                    done,
                })
            })();
            if let Err(error) = &opened {
                let _ = self.source_failed(&handshake.identity.epoch, &attempt, error.clone());
            }
            opened
        }
    }
    #[cfg(target_os = "linux")]
    fn source_guard<R>(
        &self,
        handshake: &LiveBulkHandshake,
        attempt: &AttemptId,
        action: impl FnOnce(&mut Entry) -> PResult<R>,
    ) -> PResult<R> {
        let db = self.db.lock().map_err(internal)?;
        let mut registry = self.live_captures.lock().map_err(internal)?;
        registry.sweep(&db)?;
        self.authorize_live_bulk(&db, handshake)?;
        let entry = live_entry(&mut registry, handshake)?;
        if handshake.channel != LiveBulkChannel::LiveEncodedSource
            || entry
                .report
                .encoded_source
                .as_ref()
                .and_then(|s| s.manifest.as_ref())
                .is_none_or(|m| m.attempt_id != *attempt)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "encoded attempt/channel differs",
            ));
        }
        action(entry)
    }
    #[cfg(target_os = "linux")]
    fn source_frame(
        &self,
        handshake: &LiveBulkHandshake,
        attempt: &AttemptId,
        frame: &NativeSourceFrame,
    ) -> PResult<()> {
        self.source_guard(handshake, attempt, |entry| {
            let info = entry.report.encoded_source.as_ref().expect("admitted");
            if info.state != LiveEncodedSourceState::Running {
                return Err(ProtocolError::invalid("encoded source not running"));
            }
            let facts = frame.facts();
            facts.validate(info.manifest.as_ref().expect("launched"))?;
            if facts.probe.parent_end.0 != u64::from(entry.report.options.lease_ms) * 1_000_000
                || facts.frame.value() != entry.report.received_frames
            {
                return Err(ProtocolError::invalid(
                    "native frame differs from exact lease/order",
                ));
            }
            let header = LiveFrameHeader {
                frame: facts.frame,
                source_time: facts.source_time,
                capture_tick: facts.bounds.earliest,
                sha256: facts.pixels.sha256.clone(),
            };
            check_frame(entry, &header)?;
            let now = tick(entry)?;
            if facts.parent_receipt > now {
                return Err(ProtocolError::invalid(
                    "native receipt exceeds the entry clock",
                ));
            }
            let presentation = entry
                .report
                .clock
                .map(header.source_time)
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                .project
                .time;
            entry.runtime.as_mut().expect("active").offer(
                header.frame,
                header.source_time,
                header.capture_tick,
                presentation,
                frame.pixels(),
                now,
            )?;
            if entry.native_accepted.as_ref().is_some_and(|stamp| {
                stamp.continuity_generation
                    != entry
                        .runtime
                        .as_ref()
                        .expect("active")
                        .status()
                        .pair_discontinuities
            }) {
                entry.native_accepted = None;
            }
            entry.report.received_frames += 1;
            entry.report.last_frame = Some(header);
            entry
                .report
                .encoded_source
                .as_mut()
                .expect("admitted")
                .latest_delivery = Some(facts.clone());
            report(entry)?;
            Ok(())
        })
    }
    #[cfg(target_os = "linux")]
    fn source_failed(
        &self,
        epoch: &AttemptId,
        attempt: &AttemptId,
        error: ProtocolError,
    ) -> PResult<()> {
        // Native retirement must remain possible without a healthy database.
        let mut registry = self.live_captures.lock().map_err(internal)?;
        if let Some(entry) = registry.entries.get_mut(epoch.as_str()).filter(|e| {
            e.runtime.is_some()
                && !finishing(e)
                && e.report
                    .encoded_source
                    .as_ref()
                    .and_then(|s| s.manifest.as_ref())
                    .is_some_and(|m| m.attempt_id == *attempt)
        }) {
            if entry.created.elapsed()
                >= Duration::from_millis(u64::from(entry.report.options.lease_ms))
            {
                return retire(entry, LiveCaptureRetirement::Expired);
            }
            let mut error = error;
            error.message = error.message.chars().take(512).collect();
            entry
                .report
                .encoded_source
                .as_mut()
                .expect("admitted")
                .error = Some(error.clone());
            inference::fail_entry(entry, error)?;
        }
        Ok(())
    }
}
