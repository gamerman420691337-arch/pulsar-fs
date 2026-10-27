//! Bounded scheduling facts. The engine supplies clocks, authority and captured
//! artifact identities; this kernel neither acquires pixels nor dispatches devices.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LiveClockTick(pub u64);
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveIdentity {
    pub epoch: AttemptId,
    pub project: ProjectId,
    pub revision: RevisionId,
    pub source: SourceVersionId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveFrame {
    pub identity: LiveIdentity,
    pub frame: FrameId,
    pub artifact: ArtifactId,
    pub source_time: SourceTimestamp,
    pub capture_tick: LiveClockTick,
    pub presentation_time: ProjectTime,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePolicy {
    /// Caller-selected admission limit, not an agreed statistical latency gate.
    pub maximum_analysis_age_ns: u64,
    pub added_buffer_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveWorkToken {
    frame: LiveFrame,
    serial: u64,
    presentation: Option<LivePresentationMapping>,
}
impl LiveWorkToken {
    pub fn presentation(&self) -> Option<&LivePresentationMapping> {
        self.presentation.as_ref()
    }
    pub fn frame(&self) -> &LiveFrame {
        &self.frame
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveCompletion {
    Accepted {
        frame: LiveFrame,
        capture_to_analysis_ns: u64,
    },
    Stale {
        frame: FrameId,
        age_ns: u64,
    },
    Retired,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveQueueStatus {
    pub connected: bool,
    pub pending_frames: u8,
    pub in_flight_frames: u8,
    pub received: u64,
    pub replaced_pending: u64,
    pub expired: u64,
    pub retired_completions: u64,
    pub accepted_completions: u64,
    pub last_capture_to_analysis_ns: Option<u64>,
    pub last_presentation_lateness_ns: Option<u64>,
}
pub struct LiveScheduler {
    identity: LiveIdentity,
    policy: LivePolicy,
    presentation_clock: Option<LivePresentationClock>,
    pending: Option<LiveFrame>,
    in_flight: Option<LiveWorkToken>,
    last_frame: Option<FrameId>,
    last_capture: Option<LiveClockTick>,
    last_source: Option<SourceTimestamp>,
    last_presentation: Option<ProjectTime>,
    now: LiveClockTick,
    serial: u64,
    status: LiveQueueStatus,
}
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidTransition(format!("live scheduling: {message}"))
}
impl LiveScheduler {
    pub fn new(
        identity: LiveIdentity,
        policy: LivePolicy,
        now: LiveClockTick,
    ) -> Result<Self, CoreError> {
        if policy.maximum_analysis_age_ns == 0
            || policy.maximum_analysis_age_ns > 10_000_000_000
            || policy.added_buffer_ns > 1_000_000_000
        {
            return Err(invalid(
                "age must be 1ns..10s and optional added buffer at most 1s",
            ));
        }
        Ok(Self {
            identity,
            policy,
            presentation_clock: None,
            pending: None,
            in_flight: None,
            last_frame: None,
            last_capture: None,
            last_source: None,
            last_presentation: None,
            now,
            serial: 0,
            status: LiveQueueStatus {
                connected: true,
                ..Default::default()
            },
        })
    }
    /// Bind frame presentation times to one immutable declared clock. New
    /// anchors or buffering require a fresh scheduler epoch.
    pub fn with_presentation_clock(
        identity: LiveIdentity,
        policy: LivePolicy,
        clock: LivePresentationClock,
        now: LiveClockTick,
    ) -> Result<Self, CoreError> {
        clock.validate()?;
        if identity.epoch != clock.epoch || policy.added_buffer_ns != clock.added_buffer_ns {
            return Err(invalid(
                "clock epoch or buffering differs from scheduler admission",
            ));
        }
        let mut scheduler = Self::new(identity, policy, now)?;
        scheduler.presentation_clock = Some(clock);
        Ok(scheduler)
    }
    fn clock(&self, now: LiveClockTick) -> Result<(), CoreError> {
        if now < self.now {
            Err(invalid("clock moved backwards; retire the epoch"))
        } else {
            Ok(())
        }
    }
    /// Advance the same monotonic clock while an acquisition owner is waiting
    /// for enough pixels to issue analysis. This never issues work or rearms.
    pub fn observe(&mut self, now: LiveClockTick) -> Result<(), CoreError> {
        self.clock(now)?;
        self.now = now;
        Ok(())
    }
    /// Replace only pending work. In-flight work can complete with its original
    /// identity if it remains young enough; newer pixels never relabel it.
    pub fn offer(
        &mut self,
        frame: LiveFrame,
        now: LiveClockTick,
    ) -> Result<Option<LiveFrame>, CoreError> {
        self.clock(now)?;
        if !self.status.connected
            || frame.identity != self.identity
            || frame.capture_tick > now
            || frame.presentation_time < ProjectTime::ZERO
            || self.last_frame.is_some_and(|id| frame.frame <= id)
            || self
                .last_presentation
                .is_some_and(|time| frame.presentation_time <= time)
            || self
                .last_capture
                .is_some_and(|tick| frame.capture_tick < tick)
            || self
                .last_source
                .map(|last| {
                    ExactSeconds::from_source(frame.source_time)
                        .checked_cmp(ExactSeconds::from_source(last))
                })
                .transpose()?
                .is_some_and(|order| order != std::cmp::Ordering::Greater)
        {
            return Err(invalid("retired, changed, unordered or future frame"));
        }
        if let Some(clock) = &self.presentation_clock {
            if clock.map(frame.source_time)?.project.time != frame.presentation_time {
                return Err(invalid(
                    "frame project time differs from declared presentation clock",
                ));
            }
        }
        let received = self
            .status
            .received
            .checked_add(1)
            .ok_or(CoreError::Overflow)?;
        let replaced = self
            .status
            .replaced_pending
            .checked_add(u64::from(self.pending.is_some()))
            .ok_or(CoreError::Overflow)?;
        self.now = now;
        self.last_frame = Some(frame.frame);
        self.last_capture = Some(frame.capture_tick);
        self.last_source = Some(frame.source_time);
        self.last_presentation = Some(frame.presentation_time);
        self.status.received = received;
        self.status.replaced_pending = replaced;
        Ok(self.pending.replace(frame))
    }
    pub fn take(&mut self, now: LiveClockTick) -> Result<Option<LiveWorkToken>, CoreError> {
        self.clock(now)?;
        if !self.status.connected || self.in_flight.is_some() {
            self.now = now;
            return Ok(None);
        }
        let Some(frame) = self.pending.as_ref() else {
            self.now = now;
            return Ok(None);
        };
        if now.0 - frame.capture_tick.0 > self.policy.maximum_analysis_age_ns {
            let expired = self
                .status
                .expired
                .checked_add(1)
                .ok_or(CoreError::Overflow)?;
            self.pending = None;
            self.status.expired = expired;
            self.now = now;
            return Ok(None);
        }
        let serial = self.serial.checked_add(1).ok_or(CoreError::Overflow)?;
        let presentation = self
            .presentation_clock
            .as_ref()
            .map(|clock| clock.map(frame.source_time))
            .transpose()?;
        let token = LiveWorkToken {
            frame: self.pending.take().expect("checked"),
            serial,
            presentation,
        };
        self.serial = serial;
        self.now = now;
        self.in_flight = Some(token.clone());
        Ok(Some(token))
    }
    /// Settle only the exact unanalysed pending input after its owner closes
    /// ingress. This is not acceptance, a stale result, retirement or renewal.
    pub fn discard_pending_for_input_close(
        &mut self,
        identity: &LiveIdentity,
        expected: Option<FrameId>,
        now: LiveClockTick,
    ) -> Result<(), CoreError> {
        self.clock(now)?;
        if identity != &self.identity
            || !self.status.connected
            || self.in_flight.is_some()
            || self.pending.as_ref().map(|frame| frame.frame) != expected
        {
            return Err(invalid("input close differs from exact pending source"));
        }
        self.pending = None;
        self.now = now;
        Ok(())
    }
    pub fn complete(
        &mut self,
        token: &LiveWorkToken,
        now: LiveClockTick,
    ) -> Result<LiveCompletion, CoreError> {
        self.clock(now)?;
        if !self.status.connected || self.in_flight.as_ref() != Some(token) {
            let retired = self
                .status
                .retired_completions
                .checked_add(1)
                .ok_or(CoreError::Overflow)?;
            self.status.retired_completions = retired;
            self.now = now;
            return Ok(LiveCompletion::Retired);
        }
        let age = now.0 - token.frame.capture_tick.0;
        let result = if age > self.policy.maximum_analysis_age_ns {
            self.status.expired = self
                .status
                .expired
                .checked_add(1)
                .ok_or(CoreError::Overflow)?;
            LiveCompletion::Stale {
                frame: token.frame.frame,
                age_ns: age,
            }
        } else {
            self.status.accepted_completions = self
                .status
                .accepted_completions
                .checked_add(1)
                .ok_or(CoreError::Overflow)?;
            self.status.last_capture_to_analysis_ns = Some(age);
            self.status.last_presentation_lateness_ns = token
                .presentation
                .as_ref()
                .map(|mapping| mapping.lateness_at(now));
            LiveCompletion::Accepted {
                frame: token.frame.clone(),
                capture_to_analysis_ns: age,
            }
        };
        self.in_flight = None;
        self.now = now;
        Ok(result)
    }
    /// Disconnect, revision switch, revocation or a clock reset requires a new
    /// identity/epoch. Closing never acknowledges a physical device stop.
    pub fn retire(&mut self, now: LiveClockTick) -> Result<(), CoreError> {
        self.clock(now)?;
        self.pending = None;
        self.in_flight = None;
        self.status.connected = false;
        self.now = now;
        Ok(())
    }
    pub fn status(&self) -> LiveQueueStatus {
        let mut status = self.status.clone();
        status.pending_frames = u8::from(self.pending.is_some());
        status.in_flight_frames = u8::from(self.in_flight.is_some());
        status
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
            source: SourceVersionId::new("camera-epoch").unwrap(),
        }
    }
    fn frame(id: u64) -> LiveFrame {
        LiveFrame {
            identity: identity("epoch-one"),
            frame: FrameId::new(id),
            artifact: ArtifactId::new(format!("pixels-{id}")).unwrap(),
            source_time: SourceTimestamp::new(id as i64, 1000).unwrap(),
            capture_tick: LiveClockTick(id),
            presentation_time: ProjectTime::from_nanos(id as i64 * 1_000_000),
        }
    }
    fn queue() -> LiveScheduler {
        LiveScheduler::new(
            identity("epoch-one"),
            LivePolicy {
                maximum_analysis_age_ns: 200,
                added_buffer_ns: 1_000_000_000,
            },
            LiveClockTick(0),
        )
        .unwrap()
    }
    #[test]
    fn input_close_discards_only_exact_unissued_pending_frame_without_acceptance() {
        let mut q = queue();
        q.offer(frame(1), LiveClockTick(1)).unwrap();
        let original = q.status();
        for (scope, id, tick) in [
            (identity("foreign"), Some(FrameId::new(1)), LiveClockTick(2)),
            (
                identity("epoch-one"),
                Some(FrameId::new(2)),
                LiveClockTick(2),
            ),
            (identity("epoch-one"), None, LiveClockTick(2)),
            (
                identity("epoch-one"),
                Some(FrameId::new(1)),
                LiveClockTick(0),
            ),
        ] {
            assert!(q.discard_pending_for_input_close(&scope, id, tick).is_err());
            assert_eq!(q.status(), original);
        }
        q.discard_pending_for_input_close(
            &identity("epoch-one"),
            Some(FrameId::new(1)),
            LiveClockTick(2),
        )
        .unwrap();
        let mut expected = original;
        expected.pending_frames = 0;
        assert_eq!(q.status(), expected);
        assert!(q.take(LiveClockTick(2)).unwrap().is_none());
        q.retire(LiveClockTick(3)).unwrap();
        assert!(q
            .discard_pending_for_input_close(&identity("epoch-one"), None, LiveClockTick(3))
            .is_err());
    }
    #[test]
    fn input_close_cannot_discard_original_inflight_work() {
        let mut q = queue();
        q.offer(frame(1), LiveClockTick(1)).unwrap();
        let work = q.take(LiveClockTick(1)).unwrap().unwrap();
        q.offer(frame(2), LiveClockTick(2)).unwrap();
        let original = q.status();
        assert!(q
            .discard_pending_for_input_close(
                &identity("epoch-one"),
                Some(FrameId::new(2)),
                LiveClockTick(3)
            )
            .is_err());
        assert_eq!(q.status(), original);
        assert!(matches!(
            q.complete(&work, LiveClockTick(3)).unwrap(),
            LiveCompletion::Accepted { .. }
        ));
        q.discard_pending_for_input_close(
            &identity("epoch-one"),
            Some(FrameId::new(2)),
            LiveClockTick(4),
        )
        .unwrap();
        assert_eq!(q.status().pending_frames, 0);
    }
    #[test]
    fn overload_retains_one_latest_pending_and_original_inflight_identity() {
        let mut q = queue();
        q.offer(frame(1), LiveClockTick(1)).unwrap();
        let work = q.take(LiveClockTick(1)).unwrap().unwrap();
        for id in 2..=100 {
            q.offer(frame(id), LiveClockTick(id)).unwrap();
            assert_eq!(q.status().pending_frames, 1);
            assert_eq!(q.status().in_flight_frames, 1);
            assert!(q.take(LiveClockTick(id)).unwrap().is_none());
        }
        assert_eq!(q.status().replaced_pending, 98);
        assert_eq!(
            q.complete(&work, LiveClockTick(120)).unwrap(),
            LiveCompletion::Accepted {
                frame: frame(1),
                capture_to_analysis_ns: 119
            }
        );
        let latest = q.take(LiveClockTick(121)).unwrap().unwrap();
        assert_eq!(latest.frame(), &frame(100));
        assert_eq!(
            q.complete(&work, LiveClockTick(122)).unwrap(),
            LiveCompletion::Retired
        );
        assert_eq!(q.status().in_flight_frames, 1);
        assert!(matches!(
            q.complete(&latest, LiveClockTick(130)).unwrap(),
            LiveCompletion::Accepted {
                capture_to_analysis_ns: 30,
                ..
            }
        ));
    }
    #[test]
    fn stale_work_and_retired_epochs_cannot_become_current_or_rearm() {
        let mut q = queue();
        q.offer(frame(1), LiveClockTick(1)).unwrap();
        let work = q.take(LiveClockTick(2)).unwrap().unwrap();
        assert_eq!(
            q.complete(&work, LiveClockTick(202)).unwrap(),
            LiveCompletion::Stale {
                frame: FrameId::new(1),
                age_ns: 201
            }
        );
        q.offer(frame(203), LiveClockTick(203)).unwrap();
        assert!(q.take(LiveClockTick(404)).unwrap().is_none());
        assert_eq!(q.status().expired, 2);
        q.offer(frame(405), LiveClockTick(405)).unwrap();
        let work = q.take(LiveClockTick(406)).unwrap().unwrap();
        q.retire(LiveClockTick(407)).unwrap();
        assert_eq!(
            q.complete(&work, LiveClockTick(408)).unwrap(),
            LiveCompletion::Retired
        );
        assert!(q.offer(frame(409), LiveClockTick(409)).is_err());
        let mut fresh = LiveScheduler::new(
            identity("epoch-two"),
            LivePolicy {
                maximum_analysis_age_ns: 200,
                added_buffer_ns: 0,
            },
            LiveClockTick(0),
        )
        .unwrap();
        assert!(fresh.offer(frame(1), LiveClockTick(1)).is_err());
        assert_eq!(
            fresh.complete(&work, LiveClockTick(1)).unwrap(),
            LiveCompletion::Retired
        );
    }
    #[test]
    fn clock_bound_queue_refuses_retimed_frames_and_reports_age_separately_from_buffer() {
        let clock = LivePresentationClock {
            epoch: identity("epoch-one").epoch,
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            added_buffer_ns: 100,
        };
        let policy = LivePolicy {
            maximum_analysis_age_ns: 200,
            added_buffer_ns: 100,
        };
        let mut q = LiveScheduler::with_presentation_clock(
            identity("epoch-one"),
            policy,
            clock.clone(),
            LiveClockTick(0),
        )
        .unwrap();
        let mut wrong = frame(1);
        wrong.presentation_time = ProjectTime::from_nanos(1);
        assert!(q.offer(wrong, LiveClockTick(1)).is_err());
        assert_eq!(q.status().received, 0);
        q.offer(frame(1), LiveClockTick(1)).unwrap();
        let work = q.take(LiveClockTick(2)).unwrap().unwrap();
        assert_eq!(
            work.presentation().unwrap().presentation_tick,
            LiveClockTick(1_000_100)
        );
        assert!(matches!(
            q.complete(&work, LiveClockTick(151)).unwrap(),
            LiveCompletion::Accepted {
                capture_to_analysis_ns: 150,
                ..
            }
        ));
        assert_eq!(q.status().last_capture_to_analysis_ns, Some(150));
        assert_eq!(q.status().last_presentation_lateness_ns, Some(0));
        let mut collision = frame(2);
        collision.source_time = SourceTimestamp::new(2_000_001, 2_000_000_000).unwrap();
        collision.presentation_time = ProjectTime::from_nanos(1_000_000);
        assert!(q.offer(collision, LiveClockTick(152)).is_err());
        assert_eq!(q.status().received, 1);
        q.offer(frame(152), LiveClockTick(152)).unwrap();
        let stale = q.take(LiveClockTick(153)).unwrap().unwrap();
        assert!(matches!(
            q.complete(&stale, LiveClockTick(353)).unwrap(),
            LiveCompletion::Stale { age_ns: 201, .. }
        ));
        assert!(LiveScheduler::with_presentation_clock(
            identity("another-epoch"),
            policy,
            clock.clone(),
            LiveClockTick(0)
        )
        .is_err());
        assert!(LiveScheduler::with_presentation_clock(
            identity("epoch-one"),
            LivePolicy {
                added_buffer_ns: 0,
                ..policy
            },
            clock,
            LiveClockTick(0)
        )
        .is_err());
    }
    #[test]
    fn invalid_frames_and_clock_steps_leave_queued_work_unchanged() {
        let mut q = queue();
        q.offer(frame(10), LiveClockTick(10)).unwrap();
        let before = q.status();
        assert!(q.offer(frame(10), LiveClockTick(11)).is_err());
        let mut future = frame(12);
        future.capture_tick = LiveClockTick(20);
        assert!(q.offer(future, LiveClockTick(12)).is_err());
        let mut revision = frame(12);
        revision.identity.revision = RevisionId::new(2);
        assert!(q.offer(revision, LiveClockTick(12)).is_err());
        assert!(q.take(LiveClockTick(9)).is_err());
        assert_eq!(q.status(), before);
        assert!(LiveScheduler::new(
            identity("e"),
            LivePolicy {
                maximum_analysis_age_ns: 1,
                added_buffer_ns: 1_000_000_001
            },
            LiveClockTick(0)
        )
        .is_err());
    }
}
