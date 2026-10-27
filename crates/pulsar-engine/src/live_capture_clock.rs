//! Native correspondence for capture-relative Instant ticks. The immutable
//! bracket preserves uncertainty and never turns producer-declared receipt or
//! decoded stdout delivery into camera exposure, qualification or device authority.
use pulsar_protocol::*;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{sync::Mutex, time::Instant};

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        format!("native capture clock: {message}"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::authority) struct BootBounds {
    pub earliest_ns: u64,
    pub latest_ns: u64,
}
#[derive(Clone, Copy, Debug)]
struct Sample {
    elapsed: LiveClockTick,
    boot: BootBounds,
}
#[derive(Debug)]
struct Correspondence {
    origin: BootBounds,
    last: Sample,
    failed: bool,
}
impl Correspondence {
    fn new(sample: Sample) -> Result<Self, ProtocolError> {
        let origin = Self::offset(sample)?;
        Ok(Self {
            origin,
            last: sample,
            failed: false,
        })
    }
    fn offset(sample: Sample) -> Result<BootBounds, ProtocolError> {
        if sample.boot.earliest_ns > sample.boot.latest_ns {
            return Err(invalid("reversed native observation bracket"));
        }
        Ok(BootBounds {
            earliest_ns: sample
                .boot
                .earliest_ns
                .checked_sub(sample.elapsed.0)
                .ok_or_else(|| invalid("native origin lower bound underflow"))?,
            latest_ns: sample
                .boot
                .latest_ns
                .checked_sub(sample.elapsed.0)
                .ok_or_else(|| invalid("native origin upper bound underflow"))?,
        })
    }
    fn observe(&mut self, sample: Sample) -> Result<(), ProtocolError> {
        let valid = (|| {
            if self.failed {
                return Err(invalid("clock correspondence permanently retired"));
            }
            let offset = Self::offset(sample)?;
            if sample.elapsed < self.last.elapsed
                || sample.boot.earliest_ns < self.last.boot.earliest_ns
                || sample.boot.latest_ns < self.last.boot.latest_ns
                || offset.latest_ns < self.origin.earliest_ns
                || offset.earliest_ns > self.origin.latest_ns
            {
                return Err(invalid("clock regression, suspend or discontinuity"));
            }
            Ok(())
        })();
        if valid.is_err() {
            self.failed = true;
        }
        valid?;
        self.last = sample;
        Ok(())
    }
    fn map(&self, tick: LiveClockTick) -> Result<BootBounds, ProtocolError> {
        if self.failed || tick > self.last.elapsed {
            return Err(invalid("retired correspondence or future capture tick"));
        }
        Ok(BootBounds {
            earliest_ns: self
                .origin
                .earliest_ns
                .checked_add(tick.0)
                .ok_or_else(|| invalid("native capture lower bound overflow"))?,
            latest_ns: self
                .origin
                .latest_ns
                .checked_add(tick.0)
                .ok_or_else(|| invalid("native capture upper bound overflow"))?,
        })
    }
}
#[derive(Debug)]
struct State {
    correspondence: Option<Correspondence>,
    last_tick: LiveClockTick,
    failed: bool,
}

/// One bridge per capture epoch. Unsupported/unavailable clock acquisition at
/// creation preserves ordinary preview, but no native stamp can ever be minted
/// for that epoch. A measured bridge that later fails stays retired.
#[derive(Debug)]
pub(super) struct NativeCaptureClock {
    state: Mutex<State>,
    #[cfg(test)]
    boot_advance_ns: AtomicU64,
}
impl NativeCaptureClock {
    /// Map the SAME admitted event into a successor's original elapsed domain.
    /// No current receipt substitutes for the predecessor's capture instant.
    pub(super) fn shared_boundary(
        &self,
        created: Instant,
        predecessor_created: Instant,
        predecessor_tick: LiveClockTick,
        original: BootBounds,
    ) -> Result<NativeEpochBoundaryStamp, ProtocolError> {
        let instant = predecessor_created
            .checked_add(std::time::Duration::from_nanos(predecessor_tick.0))
            .ok_or_else(|| invalid("shared capture instant overflow"))?;
        let elapsed = instant
            .checked_duration_since(created)
            .ok_or_else(|| invalid("boundary predates successor admission"))?;
        let tick = LiveClockTick(
            u64::try_from(elapsed.as_nanos())
                .map_err(|_| invalid("shared capture tick overflow"))?,
        );
        let mapped = self
            .map(tick)?
            .ok_or_else(|| invalid("successor native correspondence absent"))?;
        if original.earliest_ns > original.latest_ns
            || mapped.latest_ns < original.earliest_ns
            || original.latest_ns < mapped.earliest_ns
        {
            return Err(invalid("shared capture native correspondences disagree"));
        }
        Ok(NativeEpochBoundaryStamp {
            tick,
            original,
            mapped,
        })
    }
    pub fn new(created: Instant) -> Self {
        let sample = Self::read(created, 0);
        let correspondence = sample.ok().and_then(|s| Correspondence::new(s).ok());
        let last_tick = correspondence
            .as_ref()
            .map_or(LiveClockTick(0), |c| c.last.elapsed);
        Self {
            state: Mutex::new(State {
                correspondence,
                last_tick,
                failed: false,
            }),
            #[cfg(test)]
            boot_advance_ns: AtomicU64::new(0),
        }
    }
    fn read(created: Instant, advance: u64) -> Result<Sample, ProtocolError> {
        let before = crate::handy_clock::boottime_ns()?;
        let instant = Instant::now();
        let after = crate::handy_clock::boottime_ns()?;
        let elapsed = instant
            .checked_duration_since(created)
            .ok_or_else(|| invalid("Instant precedes the capture origin"))?;
        Ok(Sample {
            elapsed: LiveClockTick(
                u64::try_from(elapsed.as_nanos())
                    .map_err(|_| invalid("capture duration overflow"))?,
            ),
            boot: BootBounds {
                earliest_ns: before
                    .checked_add(advance)
                    .ok_or_else(|| invalid("native lower bound overflow"))?,
                latest_ns: after
                    .checked_add(advance)
                    .ok_or_else(|| invalid("native upper bound overflow"))?,
            },
        })
    }
    pub fn tick(&self, created: Instant) -> Result<LiveClockTick, ProtocolError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid("clock state poisoned"))?;
        if state.failed {
            return Err(invalid("clock correspondence permanently retired"));
        }
        if state.correspondence.is_none() {
            let elapsed = Instant::now()
                .checked_duration_since(created)
                .ok_or_else(|| invalid("Instant precedes the capture origin"))?;
            let tick = LiveClockTick(
                u64::try_from(elapsed.as_nanos())
                    .map_err(|_| invalid("capture duration overflow"))?,
            );
            if tick < state.last_tick {
                state.failed = true;
                return Err(invalid("capture clock regressed"));
            }
            state.last_tick = tick;
            return Ok(tick);
        }
        #[cfg(test)]
        let advance = self.boot_advance_ns.load(Ordering::Acquire);
        #[cfg(not(test))]
        let advance = 0;
        let sample = match Self::read(created, advance) {
            Ok(sample) => sample,
            Err(error) => {
                state.failed = true;
                return Err(error);
            }
        };
        if let Err(error) = state
            .correspondence
            .as_mut()
            .expect("measured")
            .observe(sample)
        {
            state.failed = true;
            return Err(error);
        }
        state.last_tick = sample.elapsed;
        Ok(sample.elapsed)
    }
    /// Cleanup uses the last admitted tick even after a clock fault. A rejected
    /// later observation must not keep a runtime alive by breaking retirement.
    pub fn retirement_tick(&self) -> LiveClockTick {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .last_tick
    }
    pub fn map(&self, tick: LiveClockTick) -> Result<Option<BootBounds>, ProtocolError> {
        let state = self
            .state
            .lock()
            .map_err(|_| invalid("clock state poisoned"))?;
        if state.failed {
            return Err(invalid("clock correspondence permanently retired"));
        }
        state
            .correspondence
            .as_ref()
            .map(|c| c.map(tick))
            .transpose()
    }
    pub fn latest_native(&self) -> Result<BootBounds, ProtocolError> {
        let state = self
            .state
            .lock()
            .map_err(|_| invalid("clock state poisoned"))?;
        if state.failed {
            return Err(invalid("clock correspondence permanently retired"));
        }
        state
            .correspondence
            .as_ref()
            .map(|c| c.last.boot)
            .ok_or_else(|| invalid("suspend-aware correspondence unavailable for this epoch"))
    }
    /// Conservative expiry of the original capture lease. This adds the fixed
    /// lease duration to the immutable origin's EARLIEST native bound; it does
    /// not map a future capture observation or refresh the origin. The caller
    /// must first tick the same original Instant clock and check its lease.
    pub fn lease_deadline(&self, lease_ns: u64) -> Result<u64, ProtocolError> {
        let state = self
            .state
            .lock()
            .map_err(|_| invalid("clock state poisoned"))?;
        if state.failed || lease_ns == 0 || state.last_tick.0 >= lease_ns {
            return Err(invalid("capture lease expired or clock retired"));
        }
        state
            .correspondence
            .as_ref()
            .ok_or_else(|| invalid("suspend-aware correspondence unavailable for this epoch"))?
            .origin
            .earliest_ns
            .checked_add(lease_ns)
            .ok_or_else(|| invalid("native lease deadline overflow"))
    }
    #[cfg(test)]
    pub fn advance_for_test(&self, nanos: u64) {
        self.boot_advance_ns.fetch_add(nanos, Ordering::AcqRel);
    }
    #[cfg(test)]
    pub fn unavailable_for_test(&self) {
        self.state.lock().unwrap().correspondence = None;
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct NativeEpochBoundaryStamp {
    pub tick: LiveClockTick,
    pub original: BootBounds,
    pub mapped: BootBounds,
}

/// Native-only acceptance metadata retained for a future producer. CPU
/// acceptance and timestamp correspondence alone do not qualify model output,
/// source acquisition or HSP dispatch. No serde constructor exists.
#[derive(Clone, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::authority) struct NativeAcceptedCapture {
    pub previous_frame: LiveFrame,
    pub frame: LiveFrame,
    pub estimate: LiveCaptureEstimate,
    pub previous_capture_boottime: BootBounds,
    pub capture_boottime: BootBounds,
    pub accepted_boottime: BootBounds,
    pub continuity_generation: u64,
    pub continuous_from_previous: bool,
}
struct CpuResultFields<'a> {
    attempt: &'a AttemptId,
    identity: &'a LiveIdentity,
    profile: &'a String,
    input: &'a LiveInferenceInput,
    manifest: Option<&'a String>,
}
fn cpu_result_fields(result: &LiveInferenceReply) -> Option<CpuResultFields<'_>> {
    match result {
        LiveInferenceReply::Estimated {
            attempt_id,
            identity,
            profile_sha256,
            input,
            ..
        } => Some(CpuResultFields {
            attempt: attempt_id,
            identity,
            profile: profile_sha256,
            input,
            manifest: None,
        }),
        LiveInferenceReply::CapturedCpuEstimated {
            attempt_id,
            identity,
            profile_sha256,
            input,
            manifest_sha256,
            ..
        } => Some(CpuResultFields {
            attempt: attempt_id,
            identity,
            profile: profile_sha256,
            input,
            manifest: Some(manifest_sha256),
        }),
        _ => None,
    }
}
impl NativeAcceptedCapture {
    pub(super) fn capture(
        clock: &NativeCaptureClock,
        previous_frame: LiveFrame,
        frame: LiveFrame,
        estimate: LiveCaptureEstimate,
        continuity_generation: u64,
        previous: Option<&Self>,
    ) -> Result<Option<Self>, ProtocolError> {
        // The inference owner already checked this result against its admitted
        // runtime mode. Clock correspondence retains that mode across samples.
        let Some(fields) = cpu_result_fields(&estimate.result) else {
            return Ok(None);
        };
        let identity = fields.identity;
        let input = fields.input;
        if previous_frame.identity != frame.identity
            || input.pair.previous_frame != previous_frame.frame
            || input.pair.previous_time != previous_frame.source_time
            || input.previous.artifact_id != previous_frame.artifact
            || input.current.artifact_id != frame.artifact
            || estimate.qualified
            || estimate.evidence != EvidenceKind::Inferred
            || !estimate.fresh
            || *identity != frame.identity
            || input.pair.current_frame != frame.frame
            || input.pair.current_time != frame.source_time
            || estimate.capture_tick != frame.capture_tick
            || estimate.capture_tick > estimate.accepted_at
            || estimate.accepted_at.0.checked_sub(estimate.capture_tick.0)
                != Some(estimate.capture_to_analysis_ns)
        {
            return Err(invalid(
                "accepted result differs from its original issued frame",
            ));
        }
        let Some(previous_capture_boottime) = clock.map(previous_frame.capture_tick)? else {
            return Ok(None);
        };
        let Some(capture_boottime) = clock.map(estimate.capture_tick)? else {
            return Ok(None);
        };
        let Some(accepted_boottime) = clock.map(estimate.accepted_at)? else {
            return Ok(None);
        };
        let continuous_from_previous = previous.is_some_and(|prior| {
            let Some(old) = cpu_result_fields(&prior.estimate.result) else {
                return false;
            };
            prior.frame == previous_frame
                && prior.frame.identity == frame.identity
                && old.attempt == fields.attempt
                && old.profile == fields.profile
                && old.manifest == fields.manifest
                && prior.continuity_generation == continuity_generation
                && Some(input.sequence) == old.input.sequence.checked_add(1)
                && input.pair.previous_frame == prior.frame.frame
                && input.pair.previous_time == prior.frame.source_time
                && capture_boottime.earliest_ns >= prior.capture_boottime.earliest_ns
                && accepted_boottime.earliest_ns >= prior.accepted_boottime.earliest_ns
        });
        Ok(Some(Self {
            previous_frame,
            frame,
            estimate,
            previous_capture_boottime,
            capture_boottime,
            accepted_boottime,
            continuity_generation,
            continuous_from_previous,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(elapsed: u64, low: u64, high: u64) -> Sample {
        Sample {
            elapsed: LiveClockTick(elapsed),
            boot: BootBounds {
                earliest_ns: low,
                latest_ns: high,
            },
        }
    }
    #[test]
    fn shared_boundary_preserves_both_original_intervals_without_clock_renewal() {
        let a = Instant::now();
        let b = a + std::time::Duration::from_nanos(10);
        let clock = NativeCaptureClock {
            state: Mutex::new(State {
                correspondence: Some(Correspondence::new(sample(30, 2030, 2040)).unwrap()),
                last_tick: LiveClockTick(30),
                failed: false,
            }),
            boot_advance_ns: AtomicU64::new(0),
        };
        let original = BootBounds {
            earliest_ns: 2005,
            latest_ns: 2015,
        };
        let expiry = clock.lease_deadline(100).unwrap();
        let mapped = clock
            .shared_boundary(b, a, LiveClockTick(20), original)
            .unwrap();
        assert_eq!(mapped.tick, LiveClockTick(10));
        assert_eq!(mapped.original, original);
        assert_eq!(
            mapped.mapped,
            BootBounds {
                earliest_ns: 2010,
                latest_ns: 2020
            }
        );
        assert_eq!(clock.lease_deadline(100).unwrap(), expiry);
        assert!(clock
            .shared_boundary(b, a, LiveClockTick(5), original)
            .is_err());
        assert!(clock
            .shared_boundary(b, a, LiveClockTick(100), original)
            .is_err());
        assert!(clock
            .shared_boundary(
                b,
                a,
                LiveClockTick(20),
                BootBounds {
                    earliest_ns: 3000,
                    latest_ns: 3010
                }
            )
            .is_err());
        assert!(clock
            .shared_boundary(
                b,
                a,
                LiveClockTick(20),
                BootBounds {
                    earliest_ns: 2015,
                    latest_ns: 2005
                }
            )
            .is_err());
    }
    #[test]
    fn capture_clock_maps_original_ticks_with_full_immutable_uncertainty() {
        let mut c = Correspondence::new(sample(10, 1000, 1010)).unwrap();
        c.observe(sample(50, 1040, 1055)).unwrap();
        let capture = c.map(LiveClockTick(20)).unwrap();
        assert_eq!(
            capture,
            BootBounds {
                earliest_ns: 1010,
                latest_ns: 1020
            }
        );
        assert_eq!(
            c.map(LiveClockTick(50)).unwrap(),
            BootBounds {
                earliest_ns: 1040,
                latest_ns: 1050
            }
        );
        c.observe(sample(100, 1095, 1096)).unwrap();
        assert_eq!(c.map(LiveClockTick(20)).unwrap(), capture);
        for actual_origin in 990..=1000 {
            assert!(
                capture.earliest_ns <= actual_origin + 20
                    && actual_origin + 20 <= capture.latest_ns
            );
            assert!(1096 - capture.earliest_ns >= 1096 - (actual_origin + 20));
        }
        assert!(c.map(LiveClockTick(101)).is_err());
    }
    #[test]
    fn capture_clock_lease_deadline_uses_original_earliest_origin_without_future_capture() {
        let clock = NativeCaptureClock {
            state: Mutex::new(State {
                correspondence: Some(Correspondence::new(sample(10, 1000, 1010)).unwrap()),
                last_tick: LiveClockTick(10),
                failed: false,
            }),
            boot_advance_ns: AtomicU64::new(0),
        };
        assert_eq!(clock.lease_deadline(100).unwrap(), 1090);
        assert!(clock.map(LiveClockTick(100)).is_err());
        assert!(clock.lease_deadline(10).is_err());
        assert_eq!(
            clock.map(LiveClockTick(10)).unwrap().unwrap(),
            BootBounds {
                earliest_ns: 1000,
                latest_ns: 1010,
            }
        );
        assert_eq!(clock.lease_deadline(100).unwrap(), 1090);
    }
    #[test]
    fn capture_clock_suspend_regression_and_reversed_brackets_retire_permanently() {
        for changed in [
            sample(51, 2041, 2051),
            sample(49, 1040, 1050),
            sample(60, 1030, 1040),
            sample(60, 1060, 1050),
        ] {
            let mut c = Correspondence::new(sample(10, 1000, 1010)).unwrap();
            c.observe(sample(50, 1040, 1050)).unwrap();
            assert!(c.observe(changed).is_err());
            assert!(c.map(LiveClockTick(20)).is_err());
            assert!(c.observe(sample(60, 1050, 1060)).is_err());
            assert_eq!(c.last.elapsed, LiveClockTick(50));
        }
    }
    #[test]
    fn capture_clock_checks_origin_underflow_and_mapping_overflow() {
        assert!(Correspondence::new(sample(10, 9, 20)).is_err());
        assert!(Correspondence::new(sample(0, 20, 19)).is_err());
        let mut c = Correspondence::new(sample(0, u64::MAX - 1, u64::MAX)).unwrap();
        c.observe(sample(1, u64::MAX, u64::MAX)).unwrap();
        assert!(c.map(LiveClockTick(1)).is_err());
    }
    #[test]
    fn capture_clock_unavailable_origin_preserves_preview_without_later_native_reanchor() {
        let created = Instant::now();
        let c = NativeCaptureClock::new(created);
        c.unavailable_for_test();
        let tick = c.tick(created).unwrap();
        assert!(c.map(tick).unwrap().is_none());
        assert!(c.latest_native().is_err());
        assert!(c.map(c.tick(created).unwrap()).unwrap().is_none());
    }
}
