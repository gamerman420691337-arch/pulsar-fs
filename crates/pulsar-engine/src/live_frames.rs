//! Bounded captured-pixel ownership. Callers still own authorization, acquisition,
//! clock provenance and confined inference; no device or project mutation occurs.
use crate::resources::{Reservation, ResourcePool};
use pulsar_protocol::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub const LIVE_RGB_FRAME_BYTES: usize = 64 * 64 * 3;
// Two issued pixels, two pending pixels and one transient replacement allocation.
pub const LIVE_RGB_ADMISSION_BYTES: u64 = (5 * LIVE_RGB_FRAME_BYTES) as u64;

/// The production pair worker and stream ownership use the same byte ABI.
pub struct PackedRgbPair<'a> {
    previous: &'a [u8],
    current: &'a [u8],
}
impl<'a> PackedRgbPair<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != 2 * LIVE_RGB_FRAME_BYTES {
            return Err(ProtocolError::invalid(
                "captured pair requires exactly two packed 64x64 RGB frames",
            ));
        }
        Ok(Self {
            previous: &bytes[..LIVE_RGB_FRAME_BYTES],
            current: &bytes[LIVE_RGB_FRAME_BYTES..],
        })
    }
    pub fn previous(&self) -> &[u8] {
        self.previous
    }
    pub fn current(&self) -> &[u8] {
        self.current
    }
}
struct PixelAdmission {
    _reservation: Reservation,
}
struct PixelStorage {
    bytes: Box<[u8]>,
    _admission: Arc<PixelAdmission>,
}
struct Pixels {
    frame: LiveFrame,
    storage: Arc<PixelStorage>,
}
/// Exact retained native ingress allocation. A matching public hash/header
/// cannot construct or substitute this original boundary custody.
#[derive(Clone)]
pub(crate) struct LiveRgbBoundary {
    owner: uuid::Uuid,
    pixels: Arc<Pixels>,
}
impl LiveRgbBoundary {
    pub(crate) fn frame(&self) -> &LiveFrame {
        &self.pixels.frame
    }
}
/// One successor's private seed binding. Weak pixel ownership lets the original
/// reservation retire after actual pair settlement instead of retaining history.
#[derive(Clone)]
pub(crate) struct NativeSeedToken {
    owner: uuid::Uuid,
    predecessor: uuid::Uuid,
    frame: LiveFrame,
    storage: std::sync::Weak<PixelStorage>,
}
impl NativeSeedToken {
    pub(crate) fn frame(&self) -> &LiveFrame {
        &self.frame
    }
    pub(crate) fn belongs_to(&self, boundary: &LiveRgbBoundary) -> bool {
        self.predecessor == boundary.owner
            && self
                .storage
                .ptr_eq(&Arc::downgrade(&boundary.pixels.storage))
    }
}
struct Pair {
    previous: Arc<Pixels>,
    current: Arc<Pixels>,
}
/// Opaque, non-cloneable ownership of exactly one issued pair. Retired work
/// keeps its memory admission until the consumer releases these bytes.
pub struct LiveRgbWork {
    owner: uuid::Uuid,
    continuity_generation: u64,
    token: LiveWorkToken,
    pair: Pair,
    clock: Option<LivePresentationClock>,
}
impl std::fmt::Debug for LiveRgbWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveRgbWork")
            .field("previous_frame", &self.pair.previous.frame.frame)
            .field("current_frame", &self.pair.current.frame.frame)
            .finish_non_exhaustive()
    }
}
#[derive(Debug)]
pub struct LiveRgbCompletionError {
    pub error: ProtocolError,
    pub work: LiveRgbWork,
}
/// Process-local analysis scope minted only by an owning RGB runtime. It does
/// not attest current freshness or preserve pixel admission after retirement.
#[derive(Clone)]
pub(crate) struct LiveRgbScope {
    owner: uuid::Uuid,
    identity: LiveIdentity,
    clock: Option<LivePresentationClock>,
}
impl LiveRgbScope {
    pub(crate) fn validate_scope(&self, other: &Self) -> Result<(), ProtocolError> {
        if self.owner != other.owner || self.identity != other.identity || self.clock != other.clock
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live worker belongs to another runtime scope",
            ));
        }
        Ok(())
    }
    pub(crate) fn validate_manifest(
        &self,
        manifest: &LiveInferenceManifest,
    ) -> Result<(), ProtocolError> {
        if self.identity != manifest.identity || self.clock.as_ref() != Some(&manifest.clock) {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "live inference scope differs from manifest",
            ));
        }
        Ok(())
    }
    pub(crate) fn validate_work(&self, work: &LiveRgbWork) -> Result<(), ProtocolError> {
        if self.owner != work.owner
            || self.identity != work.token.frame().identity
            || self.clock != work.clock
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "issued live work belongs to another runtime scope",
            ));
        }
        Ok(())
    }
}
impl LiveRgbWork {
    pub(crate) fn continuity_generation(&self) -> u64 {
        self.continuity_generation
    }
    pub(crate) fn previous_frame(&self) -> &LiveFrame {
        &self.pair.previous.frame
    }
    pub fn token(&self) -> &LiveWorkToken {
        &self.token
    }
    pub fn previous(&self) -> &[u8] {
        &self.pair.previous.storage.bytes
    }
    pub fn current(&self) -> &[u8] {
        &self.pair.current.storage.bytes
    }
    pub(crate) fn uses_boundary(&self, seed: &NativeSeedToken) -> bool {
        self.owner == seed.owner
            && self.pair.previous.frame == seed.frame
            && seed
                .storage
                .ptr_eq(&Arc::downgrade(&self.pair.previous.storage))
    }
    pub fn declaration(&self) -> RawFramePair {
        RawFramePair {
            source_epoch: self.pair.current.frame.identity.epoch.clone(),
            previous_frame: self.pair.previous.frame.frame,
            current_frame: self.pair.current.frame.frame,
            previous_time: self.pair.previous.frame.source_time,
            current_time: self.pair.current.frame.source_time,
            presentation_clock: self.clock.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct LiveRgbStatus {
    pub queue: LiveQueueStatus,
    pub waiting_for_pair: bool,
    pub pair_discontinuities: u64,
    pub admitted_pixel_bytes: u64,
    pub held_runtime_pixel_bytes: u64,
}
pub struct LiveRgbRuntime {
    owner: uuid::Uuid,
    identity: LiveIdentity,
    scheduler: LiveScheduler,
    clock: Option<LivePresentationClock>,
    admission: Arc<PixelAdmission>,
    last: Option<Arc<Pixels>>,
    pending: Option<Pair>,
    pair_discontinuities: u64,
    input_final: Option<Option<LiveFrameHeader>>,
    last_completed: Option<(FrameId, LiveCaptureInputFinalDisposition)>,
    last_expired: Option<FrameId>,
}
fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(format!("live pixels: {message}"))
}
fn core(error: CoreError) -> ProtocolError {
    invalid(&error.to_string())
}
impl LiveRgbRuntime {
    pub fn new(
        pool: &ResourcePool,
        identity: LiveIdentity,
        policy: LivePolicy,
        clock: Option<LivePresentationClock>,
        now: LiveClockTick,
    ) -> Result<Self, ProtocolError> {
        let scheduler = match &clock {
            Some(clock) => {
                LiveScheduler::with_presentation_clock(identity.clone(), policy, clock.clone(), now)
            }
            None => LiveScheduler::new(identity.clone(), policy, now),
        }
        .map_err(core)?;
        let reservation = pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES)?;
        Ok(Self {
            owner: uuid::Uuid::new_v4(),
            identity,
            scheduler,
            clock,
            admission: Arc::new(PixelAdmission {
                _reservation: reservation,
            }),
            last: None,
            pending: None,
            pair_discontinuities: 0,
            input_final: None,
            last_completed: None,
            last_expired: None,
        })
    }
    pub(crate) fn inference_scope(&self) -> LiveRgbScope {
        LiveRgbScope {
            owner: self.owner,
            identity: self.identity.clone(),
            clock: self.clock.clone(),
        }
    }
    /// Pixels are copied only after exact byte-size and metadata admission.
    /// Artifact identity is derived from bytes rather than accepted from a producer.
    pub fn offer(
        &mut self,
        frame: FrameId,
        source_time: SourceTimestamp,
        capture_tick: LiveClockTick,
        presentation_time: ProjectTime,
        bytes: &[u8],
        now: LiveClockTick,
    ) -> Result<(), ProtocolError> {
        if self.input_final.is_some() {
            return Err(invalid("original input is closed"));
        }
        if bytes.len() != LIVE_RGB_FRAME_BYTES {
            return Err(invalid("frame is not packed 64x64 RGB"));
        }
        let artifact = ArtifactId::new(format!("{:x}", Sha256::digest(bytes))).map_err(core)?;
        let metadata = LiveFrame {
            identity: self.identity.clone(),
            frame,
            artifact,
            source_time,
            capture_tick,
            presentation_time,
        };
        let mut discontinuity = false;
        if let Some(previous) = &self.last {
            let delta = ExactSeconds::from_source(source_time)
                .checked_sub(ExactSeconds::from_source(previous.frame.source_time))
                .map_err(core)?;
            // The scheduler rejects nonincreasing clocks. A large gap seeds a
            // new pair rather than inventing a continuous temporal observation.
            discontinuity = capture_tick.0.saturating_sub(previous.frame.capture_tick.0)
                > 500_000_000
                || delta
                    .checked_cmp(ExactSeconds::new(1, 2).expect("constant"))
                    .map_err(core)?
                    == std::cmp::Ordering::Greater;
        }
        let discontinuities = self
            .pair_discontinuities
            .checked_add(u64::from(discontinuity))
            .ok_or_else(|| invalid("discontinuity counter overflow"))?;
        self.scheduler.offer(metadata.clone(), now).map_err(core)?;
        let pixels = Arc::new(Pixels {
            frame: metadata,
            storage: Arc::new(PixelStorage {
                bytes: bytes.into(),
                _admission: self.admission.clone(),
            }),
        });
        self.pending = if discontinuity {
            None
        } else {
            self.last.as_ref().map(|previous| Pair {
                previous: previous.clone(),
                current: pixels.clone(),
            })
        };
        self.last = Some(pixels);
        self.pair_discontinuities = discontinuities;
        Ok(())
    }
    pub fn take(&mut self, now: LiveClockTick) -> Result<Option<LiveRgbWork>, ProtocolError> {
        if self.pending.is_none() {
            if self.input_final.is_some() && self.scheduler.status().in_flight_frames == 0 {
                let pending = (self.scheduler.status().pending_frames != 0).then(|| {
                    self.last
                        .as_ref()
                        .expect("pending frame pixels")
                        .frame
                        .frame
                });
                self.scheduler
                    .discard_pending_for_input_close(&self.identity, pending, now)
                    .map_err(core)?;
            }
            self.scheduler.observe(now).map_err(core)?;
            return Ok(None);
        }
        let candidate = self.pending.as_ref().map(|pair| pair.current.frame.frame);
        let token = self.scheduler.take(now).map_err(core)?;
        if let Some(token) = token {
            let pair = self.pending.take().expect("checked pending pair");
            // All metadata comes from this runtime; this is a programming invariant.
            if token.frame() != &pair.current.frame {
                return Err(invalid("scheduler/pixel identity differs"));
            }
            Ok(Some(LiveRgbWork {
                owner: self.owner,
                continuity_generation: self.pair_discontinuities,
                token,
                pair,
                clock: self.clock.clone(),
            }))
        } else {
            if self.scheduler.status().pending_frames == 0 {
                self.pending = None;
                self.last_expired = candidate;
            }
            Ok(None)
        }
    }
    /// This fences metadata completion only. It does not authenticate a model
    /// result, accept project motion, attest freshness of a command or dispatch.
    pub fn complete(
        &mut self,
        work: LiveRgbWork,
        now: LiveClockTick,
    ) -> Result<LiveCompletion, Box<LiveRgbCompletionError>> {
        if work.owner != self.owner {
            return Err(Box::new(LiveRgbCompletionError {
                error: invalid("completion belongs to another runtime"),
                work,
            }));
        }
        match self.scheduler.complete(&work.token, now) {
            Ok(completion) => {
                match &completion {
                    LiveCompletion::Accepted { frame, .. } => {
                        self.last_completed =
                            Some((frame.frame, LiveCaptureInputFinalDisposition::Accepted));
                    }
                    LiveCompletion::Stale { frame, .. } => {
                        self.last_completed =
                            Some((*frame, LiveCaptureInputFinalDisposition::Stale));
                    }
                    LiveCompletion::Retired => {}
                }
                Ok(completion)
            }
            Err(error) => Err(Box::new(LiveRgbCompletionError {
                error: core(error),
                work,
            })),
        }
    }
    /// Freeze the exact committed ingress boundary without cancelling its last
    /// issued/pending pair. Repeated close preserves the original boundary.
    pub(crate) fn close_input(
        &mut self,
        final_frame: Option<&LiveFrameHeader>,
    ) -> Result<(), ProtocolError> {
        self.check_input_close(final_frame)?;
        if self.input_final.is_none() {
            self.input_final = Some(final_frame.cloned());
        }
        Ok(())
    }
    pub(crate) fn reserve_transfer_boundary(
        &mut self,
        exact: &LiveFrameHeader,
    ) -> Result<LiveRgbBoundary, ProtocolError> {
        let boundary = self.inspect_transfer_boundary(exact)?;
        self.close_input(Some(exact))?;
        Ok(boundary)
    }
    pub(crate) fn inspect_transfer_boundary(
        &self,
        exact: &LiveFrameHeader,
    ) -> Result<LiveRgbBoundary, ProtocolError> {
        self.check_input_close(Some(exact))?;
        Ok(LiveRgbBoundary {
            owner: self.owner,
            pixels: self.last.as_ref().expect("checked exact boundary").clone(),
        })
    }
    pub(crate) fn seed_transfer_boundary(
        &mut self,
        boundary: &LiveRgbBoundary,
        capture_tick: LiveClockTick,
        presentation_time: ProjectTime,
        now: LiveClockTick,
    ) -> Result<NativeSeedToken, ProtocolError> {
        if self.owner == boundary.owner
            || self.last.is_some()
            || self.pending.is_some()
            || self.input_final.is_some()
            || self.scheduler.status().pending_frames != 0
            || self.identity.project != boundary.frame().identity.project
            || self.identity.revision != boundary.frame().identity.revision
            || self.identity.source != boundary.frame().identity.source
            || self.identity.epoch == boundary.frame().identity.epoch
            || presentation_time != boundary.frame().presentation_time
        {
            return Err(invalid(
                "successor seed differs from empty exact source scope",
            ));
        }
        let frame = LiveFrame {
            identity: self.identity.clone(),
            frame: FrameId::new(0),
            artifact: boundary.frame().artifact.clone(),
            source_time: boundary.frame().source_time,
            capture_tick,
            presentation_time,
        };
        self.scheduler.offer(frame.clone(), now).map_err(core)?;
        self.last = Some(Arc::new(Pixels {
            frame: frame.clone(),
            storage: boundary.pixels.storage.clone(),
        }));
        Ok(NativeSeedToken {
            owner: self.owner,
            predecessor: boundary.owner,
            frame,
            storage: Arc::downgrade(&boundary.pixels.storage),
        })
    }
    pub(crate) fn check_input_close(
        &self,
        final_frame: Option<&LiveFrameHeader>,
    ) -> Result<(), ProtocolError> {
        let expected = self.last.as_ref().map(|pixels| LiveFrameHeader {
            frame: pixels.frame.frame,
            source_time: pixels.frame.source_time,
            capture_tick: pixels.frame.capture_tick,
            sha256: pixels.frame.artifact.as_str().into(),
        });
        if expected.as_ref() != final_frame || !self.scheduler.status().connected {
            return Err(invalid("close differs from exact final admitted frame"));
        }
        if let Some(original) = &self.input_final {
            if original.as_ref() != final_frame {
                return Err(invalid("original input close changed"));
            }
            return Ok(());
        }
        Ok(())
    }
    pub(crate) fn input_closed(&self) -> bool {
        self.input_final.is_some()
    }
    /// A missing temporal pair is not a completed analysis. An expired or stale
    /// final frame never borrows the previous successfully accepted position.
    pub(crate) fn input_disposition(&self) -> Option<LiveCaptureInputFinalDisposition> {
        let final_frame = self.input_final.as_ref()?;
        if self.scheduler.status().in_flight_frames != 0
            || self.scheduler.status().pending_frames != 0
            || self.pending.is_some()
        {
            return None;
        }
        let Some(final_frame) = final_frame else {
            return Some(LiveCaptureInputFinalDisposition::NoFrames);
        };
        if let Some((frame, disposition)) = self.last_completed {
            if frame == final_frame.frame {
                return Some(disposition);
            }
        }
        if self.last_expired == Some(final_frame.frame) {
            return Some(LiveCaptureInputFinalDisposition::Expired);
        }
        Some(LiveCaptureInputFinalDisposition::Unpaired)
    }
    pub fn retire(&mut self, now: LiveClockTick) -> Result<(), ProtocolError> {
        self.scheduler.retire(now).map_err(core)?;
        self.last = None;
        self.pending = None;
        Ok(())
    }
    pub fn status(&self) -> LiveRgbStatus {
        // Pending current aliases last; count unique owned buffers by pointer.
        let mut held = Vec::with_capacity(3);
        if let Some(last) = &self.last {
            held.push(Arc::as_ptr(last));
        }
        if let Some(pair) = &self.pending {
            for p in [&pair.previous, &pair.current] {
                if !held.contains(&Arc::as_ptr(p)) {
                    held.push(Arc::as_ptr(p));
                }
            }
        }
        LiveRgbStatus {
            queue: self.scheduler.status(),
            waiting_for_pair: self.last.is_some() && self.pending.is_none(),
            pair_discontinuities: self.pair_discontinuities,
            admitted_pixel_bytes: LIVE_RGB_ADMISSION_BYTES,
            held_runtime_pixel_bytes: (held.len() * LIVE_RGB_FRAME_BYTES) as u64,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity(epoch: &str) -> LiveIdentity {
        LiveIdentity {
            epoch: AttemptId::new(epoch).unwrap(),
            project: ProjectId::new("p").unwrap(),
            revision: RevisionId::new(1),
            source: SourceVersionId::new("captured-source").unwrap(),
        }
    }
    fn runtime(pool: &ResourcePool) -> LiveRgbRuntime {
        LiveRgbRuntime::new(
            pool,
            identity("epoch-one"),
            LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            None,
            LiveClockTick(0),
        )
        .unwrap()
    }
    fn offer(q: &mut LiveRgbRuntime, id: u64) {
        let bytes = vec![id as u8; LIVE_RGB_FRAME_BYTES];
        q.offer(
            FrameId::new(id),
            SourceTimestamp::new(id as i64, 1000).unwrap(),
            LiveClockTick(id * 1_000_000),
            ProjectTime::from_nanos(id as i64 * 1_000_000),
            &bytes,
            LiveClockTick(id * 1_000_000),
        )
        .unwrap();
    }
    #[test]
    fn successor_seed_shares_original_pixels_and_keeps_admission_until_pair_settles() {
        let pool = ResourcePool::new(2 * LIVE_RGB_ADMISSION_BYTES, 2);
        let mut a = runtime(&pool);
        offer(&mut a, 1);
        offer(&mut a, 2);
        let old_work = a.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        a.complete(old_work, LiveClockTick(2_000_001)).unwrap();
        let original = a.last.as_ref().unwrap().frame.clone();
        let header = LiveFrameHeader {
            frame: original.frame,
            source_time: original.source_time,
            capture_tick: original.capture_tick,
            sha256: original.artifact.as_str().into(),
        };
        let boundary = a.reserve_transfer_boundary(&header).unwrap();
        let mut b = LiveRgbRuntime::new(
            &pool,
            identity("epoch-two"),
            LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            None,
            LiveClockTick(0),
        )
        .unwrap();
        let seed = b
            .seed_transfer_boundary(
                &boundary,
                LiveClockTick(1_000_000),
                original.presentation_time,
                LiveClockTick(1_000_000),
            )
            .unwrap();
        assert_eq!(seed.frame().frame, FrameId::new(0));
        assert_eq!(seed.frame().source_time, original.source_time);
        assert_ne!(seed.frame().identity, original.identity);
        assert!(
            b.take(LiveClockTick(1_000_000)).unwrap().is_none(),
            "seed is not a manufactured analysis point"
        );
        assert!(b
            .seed_transfer_boundary(
                &boundary,
                LiveClockTick(1_000_001),
                original.presentation_time,
                LiveClockTick(1_000_001)
            )
            .is_err());
        b.offer(
            FrameId::new(1),
            SourceTimestamp::new(3, 1000).unwrap(),
            LiveClockTick(2_000_000),
            ProjectTime::from_nanos(3_000_000),
            &vec![9; LIVE_RGB_FRAME_BYTES],
            LiveClockTick(2_000_000),
        )
        .unwrap();
        let work = b.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        assert!(work.uses_boundary(&seed));
        assert_eq!(
            work.previous().as_ptr(),
            boundary.pixels.storage.bytes.as_ptr()
        );
        assert_eq!(work.previous(), vec![2; LIVE_RGB_FRAME_BYTES]);
        assert_eq!(work.current(), vec![9; LIVE_RGB_FRAME_BYTES]);
        drop(a);
        drop(boundary);
        assert!(
            pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).is_err(),
            "original issued seed custody must retain old admission"
        );
        b.complete(work, LiveClockTick(2_000_001)).unwrap();
        assert!(pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).is_ok());
    }
    #[test]
    fn matching_hash_or_changed_mapping_cannot_substitute_native_boundary() {
        let pool = ResourcePool::new(3 * LIVE_RGB_ADMISSION_BYTES, 3);
        let mut a = runtime(&pool);
        offer(&mut a, 1);
        let original = a.last.as_ref().unwrap().frame.clone();
        let header = LiveFrameHeader {
            frame: original.frame,
            source_time: original.source_time,
            capture_tick: original.capture_tick,
            sha256: original.artifact.as_str().into(),
        };
        let boundary = a.inspect_transfer_boundary(&header).unwrap();
        let mut other = runtime(&pool);
        offer(&mut other, 1);
        let lookalike = other.inspect_transfer_boundary(&header).unwrap();
        let mut b = LiveRgbRuntime::new(
            &pool,
            identity("epoch-two"),
            LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            None,
            LiveClockTick(0),
        )
        .unwrap();
        assert!(b
            .seed_transfer_boundary(
                &boundary,
                LiveClockTick(1),
                ProjectTime::from_nanos(2_000_000),
                LiveClockTick(1)
            )
            .is_err());
        let seed = b
            .seed_transfer_boundary(
                &boundary,
                LiveClockTick(1),
                original.presentation_time,
                LiveClockTick(1),
            )
            .unwrap();
        assert!(seed.belongs_to(&boundary));
        assert!(!seed.belongs_to(&lookalike));
        assert!(
            a.offer(
                FrameId::new(2),
                SourceTimestamp::new(2, 1000).unwrap(),
                LiveClockTick(2_000_000),
                ProjectTime::from_nanos(2_000_000),
                &vec![2; LIVE_RGB_FRAME_BYTES],
                LiveClockTick(2_000_000)
            )
            .is_ok(),
            "inspecting a boundary does not freeze input before request reservation"
        );
        assert!(
            a.reserve_transfer_boundary(&header).is_err(),
            "old copied header cannot freeze a newer ingress boundary"
        );
    }
    #[test]
    fn closed_rgb_zero_or_unpaired_input_never_becomes_analysis() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        for count in 0..=1 {
            let mut q = runtime(&pool);
            if count == 1 {
                offer(&mut q, 1);
            }
            let final_frame = q.last.as_ref().map(|pixels| LiveFrameHeader {
                frame: pixels.frame.frame,
                source_time: pixels.frame.source_time,
                capture_tick: pixels.frame.capture_tick,
                sha256: pixels.frame.artifact.as_str().into(),
            });
            q.close_input(final_frame.as_ref()).unwrap();
            q.close_input(final_frame.as_ref()).unwrap();
            assert!(q.take(LiveClockTick(1_000_000)).unwrap().is_none());
            assert_eq!(q.status().queue.pending_frames, 0);
            assert_eq!(q.status().queue.accepted_completions, 0);
            assert_eq!(
                q.input_disposition(),
                Some(if count == 0 {
                    LiveCaptureInputFinalDisposition::NoFrames
                } else {
                    LiveCaptureInputFinalDisposition::Unpaired
                })
            );
            assert!(q
                .offer(
                    FrameId::new(2),
                    SourceTimestamp::new(2, 1000).unwrap(),
                    LiveClockTick(2_000_000),
                    ProjectTime::from_nanos(2_000_000),
                    &vec![2; LIVE_RGB_FRAME_BYTES],
                    LiveClockTick(2_000_000)
                )
                .is_err());
        }
    }
    #[test]
    fn closed_rgb_waits_for_exact_final_pair_and_keeps_expired_or_stale_disposition() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        for outcome in 0..3 {
            let mut q = runtime(&pool);
            offer(&mut q, 1);
            offer(&mut q, 2);
            let final_frame = LiveFrameHeader {
                frame: FrameId::new(2),
                source_time: SourceTimestamp::new(2, 1000).unwrap(),
                capture_tick: LiveClockTick(2_000_000),
                sha256: q.last.as_ref().unwrap().frame.artifact.as_str().into(),
            };
            let mut wrong = final_frame.clone();
            wrong.capture_tick.0 += 1;
            assert!(q.close_input(Some(&wrong)).is_err());
            assert!(!q.input_closed());
            q.close_input(Some(&final_frame)).unwrap();
            assert!(q.input_disposition().is_none());
            if outcome == 2 {
                assert!(q.take(LiveClockTick(203_000_000)).unwrap().is_none());
            } else {
                let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
                assert!(q.input_disposition().is_none());
                q.complete(
                    work,
                    LiveClockTick(if outcome == 0 { 3_000_000 } else { 203_000_000 }),
                )
                .unwrap();
            }
            assert_eq!(
                q.input_disposition(),
                Some(match outcome {
                    0 => LiveCaptureInputFinalDisposition::Accepted,
                    1 => LiveCaptureInputFinalDisposition::Stale,
                    _ => LiveCaptureInputFinalDisposition::Expired,
                })
            );
            assert_eq!(q.status().queue.pending_frames, 0);
            assert_eq!(q.status().queue.in_flight_frames, 0);
        }
    }
    #[test]
    fn closed_rgb_final_gap_never_reuses_previous_accepted_pair() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        offer(&mut q, 2);
        let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        q.complete(work, LiveClockTick(3_000_000)).unwrap();
        // The previous frame is at 2 ms; 503 ms makes the actual gap 501 ms,
        // strictly beyond the 500 ms pairing continuity boundary.
        offer(&mut q, 503);
        assert_eq!(q.status().pair_discontinuities, 1);
        let pixels = q.last.as_ref().unwrap();
        let header = LiveFrameHeader {
            frame: pixels.frame.frame,
            source_time: pixels.frame.source_time,
            capture_tick: pixels.frame.capture_tick,
            sha256: pixels.frame.artifact.as_str().into(),
        };
        q.close_input(Some(&header)).unwrap();
        assert!(q.take(LiveClockTick(503_000_000)).unwrap().is_none());
        assert_eq!(q.status().queue.accepted_completions, 1);
        assert_eq!(
            q.input_disposition(),
            Some(LiveCaptureInputFinalDisposition::Unpaired)
        );
    }
    #[test]
    fn analysis_scope_rejects_foreign_runtime_with_identical_serializable_metadata() {
        let pool = ResourcePool::new(2 * LIVE_RGB_ADMISSION_BYTES, 1);
        let manifest: LiveInferenceManifest = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/live-cpu-manifest.json"
        ))
        .unwrap();
        let create = || {
            LiveRgbRuntime::new(
                &pool,
                manifest.identity.clone(),
                LivePolicy {
                    maximum_analysis_age_ns: 200_000_000,
                    added_buffer_ns: manifest.clock.added_buffer_ns,
                },
                Some(manifest.clock.clone()),
                LiveClockTick(0),
            )
            .unwrap()
        };
        let mut owner = create();
        let mut foreign = create();
        let scope = owner.inference_scope();
        scope.validate_manifest(&manifest).unwrap();
        let mut changed = manifest.clone();
        changed.identity.revision = RevisionId::new(999);
        assert!(scope.validate_manifest(&changed).is_err());
        for q in [&mut owner, &mut foreign] {
            offer(q, 1);
            offer(q, 2);
        }
        let issued = owner.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let other = foreign.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        assert_eq!(issued.declaration(), other.declaration());
        scope.validate_work(&issued).unwrap();
        assert!(scope.validate_work(&other).is_err());
        assert!(matches!(
            owner.complete(issued, LiveClockTick(3_000_000)).unwrap(),
            LiveCompletion::Accepted { .. }
        ));
    }
    #[test]
    fn overload_pairs_latest_captures_and_retains_original_issued_pixels() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        assert!(q.take(LiveClockTick(1_000_000)).unwrap().is_none());
        offer(&mut q, 2);
        let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        for id in 3..=100 {
            offer(&mut q, id);
            assert!(q.take(LiveClockTick(id * 1_000_000)).unwrap().is_none());
            assert!(q.status().held_runtime_pixel_bytes <= 2 * LIVE_RGB_FRAME_BYTES as u64);
        }
        assert_eq!(work.previous()[0], 1);
        assert_eq!(work.current()[0], 2);
        assert_eq!(
            work.token().frame().artifact.as_str(),
            format!("{:x}", Sha256::digest(work.current()))
        );
        assert!(matches!(
            q.complete(work, LiveClockTick(110_000_000)).unwrap(),
            LiveCompletion::Accepted {
                capture_to_analysis_ns: 108_000_000,
                ..
            }
        ));
        let latest = q.take(LiveClockTick(111_000_000)).unwrap().unwrap();
        assert_eq!(latest.previous()[0], 99);
        assert_eq!(latest.current()[0], 100);
        assert_eq!(latest.declaration().previous_frame, FrameId::new(99));
        assert!(latest.declaration().elapsed().is_ok());
        assert!(pool.reserve_memory(1).is_err());
        q.complete(latest, LiveClockTick(112_000_000)).unwrap();
        drop(q);
        assert!(pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).is_ok());
    }
    #[test]
    fn retirement_holds_admission_until_issued_work_is_released() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        offer(&mut q, 2);
        let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        q.retire(LiveClockTick(3_000_000)).unwrap();
        assert_eq!(q.status().held_runtime_pixel_bytes, 0);
        drop(q);
        assert!(pool.reserve_memory(1).is_err());
        drop(work);
        assert!(pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).is_ok());
    }
    #[test]
    fn rejected_completion_retirement_retains_original_pixels_and_admission() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut runtime = runtime(&pool);
        offer(&mut runtime, 1);
        offer(&mut runtime, 2);
        let work = runtime.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let previous = work.previous().as_ptr();
        let current = work.current().as_ptr();
        let failure = runtime
            .complete(work, LiveClockTick(1_000_000))
            .unwrap_err();
        assert_eq!(failure.work.previous().as_ptr(), previous);
        assert_eq!(failure.work.current().as_ptr(), current);
        assert_eq!(failure.work.current()[0], 2);
        runtime.retire(LiveClockTick(3_000_000)).unwrap();
        drop(runtime);
        assert!(pool.reserve_memory(1).is_err());
        drop(failure);
        assert!(pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).is_ok());
        let bytes = std::mem::size_of::<Result<LiveCompletion, Box<LiveRgbCompletionError>>>();
        println!("live_completion_inline_bytes={bytes}");
    }
    #[test]
    fn source_discontinuity_and_expiration_release_pending_pair_and_reseed() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        offer(&mut q, 2);
        assert!(q.take(LiveClockTick(203_000_000)).unwrap().is_none());
        assert_eq!(
            q.status().held_runtime_pixel_bytes,
            LIVE_RGB_FRAME_BYTES as u64
        );
        offer(&mut q, 1000);
        assert_eq!(q.status().pair_discontinuities, 1);
        assert!(q.take(LiveClockTick(1_000_000_000)).unwrap().is_none());
        offer(&mut q, 1001);
        let work = q.take(LiveClockTick(1_001_000_000)).unwrap().unwrap();
        assert_eq!(work.declaration().previous_frame, FrameId::new(1000));
        q.retire(LiveClockTick(1_002_000_000)).unwrap();
        assert_eq!(
            q.complete(work, LiveClockTick(1_003_000_000)).unwrap(),
            LiveCompletion::Retired
        );
    }
    #[test]
    fn pixel_work_preserves_admitted_presentation_clock_and_refuses_wrong_project_time() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let clock = LivePresentationClock {
            epoch: identity("epoch-one").epoch,
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(500_000_000),
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(200_000_000),
            added_buffer_ns: 1_000_000_000,
        };
        let mut wrong = clock.clone();
        wrong.epoch = AttemptId::new("wrong-epoch").unwrap();
        let policy = LivePolicy {
            maximum_analysis_age_ns: 200_000_000,
            added_buffer_ns: 1_000_000_000,
        };
        assert!(LiveRgbRuntime::new(
            &pool,
            identity("epoch-one"),
            policy,
            Some(wrong),
            LiveClockTick(0)
        )
        .is_err());
        let probe = pool.reserve_memory(LIVE_RGB_ADMISSION_BYTES).unwrap();
        drop(probe);
        let mut q = LiveRgbRuntime::new(
            &pool,
            identity("epoch-one"),
            policy,
            Some(clock.clone()),
            LiveClockTick(0),
        )
        .unwrap();
        assert!(q
            .offer(
                FrameId::new(1),
                SourceTimestamp::new(1, 1000).unwrap(),
                LiveClockTick(1_000_000),
                ProjectTime::from_nanos(1_000_000),
                &vec![1; LIVE_RGB_FRAME_BYTES],
                LiveClockTick(1_000_000)
            )
            .is_err());
        assert_eq!(q.status().queue.received, 0);
        for id in 1..=2 {
            q.offer(
                FrameId::new(id),
                SourceTimestamp::new(id as i64, 1000).unwrap(),
                LiveClockTick(id * 1_000_000),
                ProjectTime::from_nanos(500_000_000 + id as i64 * 1_000_000),
                &vec![id as u8; LIVE_RGB_FRAME_BYTES],
                LiveClockTick(id * 1_000_000),
            )
            .unwrap();
        }
        let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let declaration = work.declaration();
        assert_eq!(declaration.presentation_clock, Some(clock));
        let mapping = declaration.presentation_mapping().unwrap().unwrap();
        assert_eq!(
            mapping[0].project.time,
            ProjectTime::from_nanos(501_000_000)
        );
        assert_eq!(mapping[1].presentation_tick, LiveClockTick(1_202_000_000));
        assert_eq!(work.token().presentation(), Some(&mapping[1]));
        q.complete(work, LiveClockTick(3_000_000)).unwrap();
        assert_eq!(
            q.status().queue.last_capture_to_analysis_ns,
            Some(1_000_000)
        );
        assert_eq!(q.status().queue.last_presentation_lateness_ns, Some(0));
    }
    #[test]
    fn same_declared_epoch_cannot_alias_another_runtime_owner() {
        let pool = ResourcePool::new(2 * LIVE_RGB_ADMISSION_BYTES, 1);
        let mut a = runtime(&pool);
        let mut b = runtime(&pool);
        for q in [&mut a, &mut b] {
            offer(q, 1);
            offer(q, 2);
        }
        let work = a.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let other = b.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let rejected = b.complete(work, LiveClockTick(3_000_000)).unwrap_err();
        assert_eq!(b.status().queue.in_flight_frames, 1);
        assert_eq!(b.status().queue.accepted_completions, 0);
        assert!(matches!(
            a.complete(rejected.work, LiveClockTick(3_000_000)).unwrap(),
            LiveCompletion::Accepted { .. }
        ));
        assert!(matches!(
            b.complete(other, LiveClockTick(3_000_000)).unwrap(),
            LiveCompletion::Accepted { .. }
        ));
    }
    #[test]
    fn rejected_completion_returns_issued_pixels_for_retry_and_capture_gap_reseeds() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        assert!(q.take(LiveClockTick(0)).is_err());
        offer(&mut q, 2);
        let work = q.take(LiveClockTick(2_000_000)).unwrap().unwrap();
        let error = q.complete(work, LiveClockTick(1_000_000)).unwrap_err();
        assert_eq!(error.work.current()[0], 2);
        assert_eq!(q.status().queue.in_flight_frames, 1);
        assert!(matches!(
            q.complete(error.work, LiveClockTick(3_000_000)).unwrap(),
            LiveCompletion::Accepted { .. }
        ));
        q.offer(
            FrameId::new(3),
            SourceTimestamp::new(3, 1000).unwrap(),
            LiveClockTick(600_000_000),
            ProjectTime::from_nanos(3_000_000),
            &vec![3; LIVE_RGB_FRAME_BYTES],
            LiveClockTick(600_000_000),
        )
        .unwrap();
        assert_eq!(q.status().pair_discontinuities, 1);
        assert!(q.take(LiveClockTick(600_000_000)).unwrap().is_none());
    }
    #[test]
    fn rejected_pixels_and_metadata_do_not_replace_admitted_pair() {
        let pool = ResourcePool::new(LIVE_RGB_ADMISSION_BYTES, 1);
        let mut q = runtime(&pool);
        offer(&mut q, 1);
        offer(&mut q, 2);
        let before = q.status().queue;
        assert!(q
            .offer(
                FrameId::new(3),
                SourceTimestamp::new(3, 1000).unwrap(),
                LiveClockTick(3_000_000),
                ProjectTime::from_nanos(3_000_000),
                &[0],
                LiveClockTick(3_000_000)
            )
            .is_err());
        assert!(q
            .offer(
                FrameId::new(2),
                SourceTimestamp::new(3, 1000).unwrap(),
                LiveClockTick(3_000_000),
                ProjectTime::from_nanos(3_000_000),
                &vec![0; LIVE_RGB_FRAME_BYTES],
                LiveClockTick(3_000_000)
            )
            .is_err());
        assert_eq!(q.status().queue, before);
        let work = q.take(LiveClockTick(3_000_000)).unwrap().unwrap();
        assert_eq!(work.current()[0], 2);
        assert!(PackedRgbPair::new(&[]).is_err());
        let bytes = vec![7; 2 * LIVE_RGB_FRAME_BYTES];
        let pair = PackedRgbPair::new(&bytes).unwrap();
        assert_eq!(pair.previous().len(), LIVE_RGB_FRAME_BYTES);
        assert_eq!(pair.current().len(), LIVE_RGB_FRAME_BYTES);
    }
}
