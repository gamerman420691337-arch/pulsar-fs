//! Process-local cancellation consumers for already-admitted fallback attempts.
//! Registration does not validate acceptance, grant execution, or restore leases.
use super::*;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::AtomicU8, Weak},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_CONSUMERS: usize = 128;
const MAX_BUDGET_MS: u64 = 86_400_000;
const WATCH_INTERVAL: Duration = Duration::from_millis(25);

/// Only the Engine's admission path constructs this after its accepted-record,
/// held-environment, captured-image and committed-attempt checks have succeeded.
/// Serialized timestamps/references alone do not supply any of that authority.
pub(super) struct Registration {
    pub(super) job: JobId,
    pub(super) project: ProjectId,
    pub(super) owner: SessionId,
    pub(super) attempt: AttemptId,
    pub(super) reference: InferenceFallbackReference,
    pub(super) admitted_at_unix_ms: u64,
    pub(super) expires_at_unix_ms: u64,
    pub(super) attempt_budget_ms: u64,
    pub(super) original_deadline: Instant,
    pub(super) cancel: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum InvalidReason {
    Expired = 1,
    ClockRollback = 2,
    ClockUnavailable = 3,
    DossierInvalidated = 4,
    ActorInvalidated = 5,
    ProjectInvalidated = 6,
    RegistryUnavailable = 7,
    WatchdogFailed = 8,
    Cancelled = 9,
    Retired = 10,
}
impl InvalidReason {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => None,
            1 => Some(Self::Expired),
            2 => Some(Self::ClockRollback),
            3 => Some(Self::ClockUnavailable),
            4 => Some(Self::DossierInvalidated),
            5 => Some(Self::ActorInvalidated),
            6 => Some(Self::ProjectInvalidated),
            7 => Some(Self::RegistryUnavailable),
            8 => Some(Self::WatchdogFailed),
            9 => Some(Self::Cancelled),
            10 => Some(Self::Retired),
            _ => Some(Self::WatchdogFailed),
        }
    }
    fn error(self) -> ProtocolError {
        let (code, message) = match self {
            Self::Expired => (ErrorCode::Cancelled, "fallback attempt lifetime expired"),
            Self::ClockRollback => (
                ErrorCode::Unavailable,
                "fallback lifetime clock moved backwards",
            ),
            Self::ClockUnavailable => (
                ErrorCode::Unavailable,
                "fallback lifetime clock unavailable",
            ),
            Self::DossierInvalidated => (
                ErrorCode::Forbidden,
                "fallback equivalence decision was invalidated",
            ),
            Self::ActorInvalidated => (
                ErrorCode::Forbidden,
                "fallback attempt owner authority was invalidated",
            ),
            Self::ProjectInvalidated => (
                ErrorCode::Forbidden,
                "fallback project authority was invalidated",
            ),
            Self::RegistryUnavailable => (
                ErrorCode::Unavailable,
                "fallback lifetime registry unavailable",
            ),
            Self::WatchdogFailed => (ErrorCode::Unavailable, "fallback lifetime watchdog failed"),
            Self::Cancelled => (ErrorCode::Cancelled, "fallback attempt was cancelled"),
            Self::Retired => (ErrorCode::Cancelled, "fallback attempt lease retired"),
        };
        ProtocolError::new(code, message)
    }
}

#[derive(Default)]
struct Entries {
    consumers: HashMap<AttemptId, Arc<Consumer>>,
}
#[derive(Default)]
struct Shared {
    entries: Mutex<Entries>,
    failed: AtomicBool,
}
/// Invalidators only latch cancellation. They never join a thread or poll a DB.
#[derive(Default)]
pub(super) struct Registry {
    shared: Arc<Shared>,
}

struct ClockSample {
    wall: u64,
    monotonic: Instant,
}
/// Implementations are private and bounded: two local clock observations in
/// production, atomic fields in tests. No arbitrary caller callbacks or I/O.
trait Clock: Send + Sync {
    fn sample(&self) -> Result<ClockSample, ()>;
}
struct SystemClock;
impl Clock for SystemClock {
    fn sample(&self) -> Result<ClockSample, ()> {
        // Sampling the monotonic clock first can shorten a converted wall lease;
        // latency between the samples must never extend it.
        let monotonic = Instant::now();
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| u64::try_from(value.as_millis()).ok())
            .filter(|value| *value > 0 && *value <= i64::MAX as u64)
            .ok_or(())?;
        Ok(ClockSample { wall, monotonic })
    }
}
struct Consumer {
    job: JobId,
    project: ProjectId,
    owner: SessionId,
    attempt: AttemptId,
    reference: InferenceFallbackReference,
    expires_at_unix_ms: u64,
    deadline: Instant,
    clock: Arc<dyn Clock>,
    last_sample: Mutex<ClockSample>,
    registry: Weak<Shared>,
    reason: AtomicU8,
    cancel: Arc<AtomicBool>,
    stop: AtomicBool,
}
impl Consumer {
    fn invalid_reason(&self) -> Option<InvalidReason> {
        InvalidReason::from_byte(self.reason.load(Ordering::Acquire))
    }
    fn invalidate(&self, reason: InvalidReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
        self.cancel.store(true, Ordering::Release);
    }
    fn reject(&self, reason: InvalidReason) -> ProtocolError {
        self.invalidate(reason);
        self.invalid_reason().unwrap_or(reason).error()
    }
    fn check(&self) -> PResult<()> {
        match catch_unwind(AssertUnwindSafe(|| self.check_inner())) {
            Ok(result) => result,
            Err(_) => Err(self.reject(InvalidReason::WatchdogFailed)),
        }
    }
    fn check_inner(&self) -> PResult<()> {
        if let Some(reason) = self.invalid_reason() {
            return Err(reason.error());
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(self.reject(InvalidReason::Cancelled));
        }
        let shared = self
            .registry
            .upgrade()
            .ok_or_else(|| self.reject(InvalidReason::RegistryUnavailable))?;
        if shared.failed.load(Ordering::Acquire) {
            return Err(self.reject(InvalidReason::RegistryUnavailable));
        }
        {
            let entries = match shared.entries.lock() {
                Ok(entries) => entries,
                Err(error) => {
                    shared.failed.store(true, Ordering::Release);
                    let entries = error.into_inner();
                    for consumer in entries.consumers.values() {
                        consumer.invalidate(InvalidReason::RegistryUnavailable);
                    }
                    return Err(self.reject(InvalidReason::RegistryUnavailable));
                }
            };
            if !entries
                .consumers
                .get(&self.attempt)
                .is_some_and(|entry| std::ptr::eq(entry.as_ref(), self))
            {
                return Err(self.reject(InvalidReason::RegistryUnavailable));
            }
        }
        // Serialize taking samples, not merely updating their watermark: two
        // concurrent checks must not confuse scheduling order with clock rollback.
        let mut previous = self
            .last_sample
            .lock()
            .map_err(|_| self.reject(InvalidReason::WatchdogFailed))?;
        let now = self
            .clock
            .sample()
            .map_err(|_| self.reject(InvalidReason::ClockUnavailable))?;
        if now.wall == 0 || now.wall > i64::MAX as u64 {
            return Err(self.reject(InvalidReason::ClockUnavailable));
        }
        if now.wall < previous.wall || now.monotonic < previous.monotonic {
            return Err(self.reject(InvalidReason::ClockRollback));
        }
        if now.wall >= self.expires_at_unix_ms || now.monotonic >= self.deadline {
            return Err(self.reject(InvalidReason::Expired));
        }
        *previous = now;
        drop(previous);
        if let Some(reason) = self.invalid_reason() {
            return Err(reason.error());
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(self.reject(InvalidReason::Cancelled));
        }
        Ok(())
    }
}

/// An exact process-local consumer and its owned watchdog. Non-Clone, no serde.
/// Drop/retire must run outside authority DB locks; registry invalidation is safe
/// while such locks are held because invalidation only sets atomic cancellation.
pub(super) struct Lease {
    consumer: Arc<Consumer>,
    monitor: Option<JoinHandle<()>>,
}
impl Lease {
    pub(super) fn reference(&self) -> &InferenceFallbackReference {
        &self.consumer.reference
    }
    pub(super) fn deadline(&self) -> Instant {
        self.consumer.deadline
    }
    #[cfg(test)]
    pub(super) fn invalid_reason(&self) -> Option<InvalidReason> {
        self.consumer.invalid_reason()
    }
    pub(super) fn check(&self) -> PResult<()> {
        if self.monitor.as_ref().is_none_or(JoinHandle::is_finished)
            && self.consumer.invalid_reason().is_none()
        {
            self.consumer.invalidate(InvalidReason::WatchdogFailed);
        }
        self.consumer.check()
    }
    /// Explicit retirement can report an unexpected watcher panic; Drop still
    /// provides cancellation, joining and exact-handle removal on every path.
    pub(super) fn retire(mut self) -> PResult<()> {
        self.finish()
    }
    fn finish(&mut self) -> PResult<()> {
        self.consumer.invalidate(InvalidReason::Retired);
        self.consumer.stop.store(true, Ordering::Release);
        let joined = if let Some(monitor) = self.monitor.take() {
            monitor.thread().unpark();
            monitor.join().map_err(|_| {
                self.consumer.invalidate(InvalidReason::WatchdogFailed);
                InvalidReason::WatchdogFailed.error()
            })
        } else {
            Ok(())
        };
        if let Some(shared) = self.consumer.registry.upgrade() {
            let mut entries = shared.entries.lock().unwrap_or_else(|error| {
                shared.failed.store(true, Ordering::Release);
                let entries = error.into_inner();
                for consumer in entries.consumers.values() {
                    consumer.invalidate(InvalidReason::RegistryUnavailable);
                }
                entries
            });
            if entries
                .consumers
                .get(&self.consumer.attempt)
                .is_some_and(|entry| Arc::ptr_eq(entry, &self.consumer))
            {
                entries.consumers.remove(&self.consumer.attempt);
            }
        }
        joined
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
struct WatchdogExit(Arc<Consumer>);
impl Drop for WatchdogExit {
    fn drop(&mut self) {
        if !self.0.stop.load(Ordering::Acquire) && self.0.invalid_reason().is_none() {
            self.0.invalidate(InvalidReason::WatchdogFailed);
        }
    }
}
fn watch(consumer: Arc<Consumer>) {
    let _exit = WatchdogExit(consumer.clone());
    let result = catch_unwind(AssertUnwindSafe(|| {
        while !consumer.stop.load(Ordering::Acquire) {
            if consumer.check().is_err() {
                return;
            }
            let wait =
                WATCH_INTERVAL.min(consumer.deadline.saturating_duration_since(Instant::now()));
            if !wait.is_zero() {
                thread::park_timeout(wait);
            }
        }
    }));
    if result.is_err() {
        consumer.invalidate(InvalidReason::WatchdogFailed);
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum WatchdogFault {
    Spawn,
    EarlyExit,
    Panic,
}
#[cfg(test)]
thread_local! { static WATCHDOG_FAULT: std::cell::Cell<Option<WatchdogFault>> = const { std::cell::Cell::new(None) }; }

impl Registry {
    pub(super) fn register(&self, registration: Registration) -> PResult<Lease> {
        self.register_inner(registration, Arc::new(SystemClock))
    }
    fn register_inner(&self, registration: Registration, clock: Arc<dyn Clock>) -> PResult<Lease> {
        let cancel = registration.cancel.clone();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.register_checked(registration, clock)
        }));
        match result {
            Ok(Ok(lease)) => Ok(lease),
            Ok(Err(error)) => {
                cancel.store(true, Ordering::Release);
                Err(error)
            }
            Err(_) => {
                cancel.store(true, Ordering::Release);
                Err(InvalidReason::WatchdogFailed.error())
            }
        }
    }
    fn register_checked(
        &self,
        registration: Registration,
        clock: Arc<dyn Clock>,
    ) -> PResult<Lease> {
        registration.reference.validate()?;
        if registration.admitted_at_unix_ms == 0
            || registration.admitted_at_unix_ms > i64::MAX as u64
            || registration.expires_at_unix_ms <= registration.admitted_at_unix_ms
            || registration.expires_at_unix_ms > i64::MAX as u64
            || !(1..=MAX_BUDGET_MS).contains(&registration.attempt_budget_ms)
        {
            return Err(ProtocolError::invalid(
                "fallback lifetime admission time or budget is invalid",
            ));
        }
        if registration.cancel.load(Ordering::Acquire) {
            return Err(InvalidReason::Cancelled.error());
        }
        let now = clock
            .sample()
            .map_err(|_| InvalidReason::ClockUnavailable.error())?;
        if now.wall == 0 || now.wall > i64::MAX as u64 {
            return Err(InvalidReason::ClockUnavailable.error());
        }
        let maximum_wall = registration
            .admitted_at_unix_ms
            .checked_add(registration.attempt_budget_ms)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| ProtocolError::invalid("fallback lifetime budget overflow"))?;
        let expires_at_unix_ms = registration.expires_at_unix_ms.min(maximum_wall);
        if now.wall < registration.admitted_at_unix_ms {
            return Err(InvalidReason::ClockRollback.error());
        }
        let remaining = expires_at_unix_ms
            .checked_sub(now.wall)
            .filter(|value| *value > 0)
            .ok_or_else(|| InvalidReason::Expired.error())?;
        let original_remaining = registration
            .original_deadline
            .checked_duration_since(now.monotonic)
            .filter(|value| {
                !value.is_zero() && *value <= Duration::from_millis(registration.attempt_budget_ms)
            })
            .ok_or_else(|| {
                ProtocolError::invalid(
                    "fallback original monotonic deadline is expired or exceeds its budget",
                )
            })?;
        let deadline = now
            .monotonic
            .checked_add(Duration::from_millis(remaining).min(original_remaining))
            .ok_or_else(|| ProtocolError::invalid("fallback monotonic deadline overflow"))?;
        let consumer = Arc::new(Consumer {
            job: registration.job,
            project: registration.project,
            owner: registration.owner,
            attempt: registration.attempt,
            reference: registration.reference,
            expires_at_unix_ms,
            deadline,
            clock,
            last_sample: Mutex::new(now),
            registry: Arc::downgrade(&self.shared),
            reason: AtomicU8::new(0),
            cancel: registration.cancel,
            stop: AtomicBool::new(false),
        });
        {
            let mut entries = self.shared.entries.lock().map_err(|error| {
                self.shared.failed.store(true, Ordering::Release);
                for consumer in error.into_inner().consumers.values() {
                    consumer.invalidate(InvalidReason::RegistryUnavailable);
                }
                InvalidReason::RegistryUnavailable.error()
            })?;
            if self.shared.failed.load(Ordering::Acquire) {
                return Err(InvalidReason::RegistryUnavailable.error());
            }
            if entries.consumers.contains_key(&consumer.attempt) {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "fallback attempt already has a lifetime consumer",
                ));
            }
            if entries.consumers.len() >= MAX_CONSUMERS {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "fallback lifetime consumer capacity exhausted",
                ));
            }
            entries
                .consumers
                .insert(consumer.attempt.clone(), consumer.clone());
        }
        // Install RAII cleanup before thread creation, including injected spawn
        // failures, so no cancelled map entry can leak an admission slot.
        let mut lease = Lease {
            consumer: consumer.clone(),
            monitor: None,
        };
        #[cfg(test)]
        let fault = WATCHDOG_FAULT.with(|value| value.replace(None));
        #[cfg(test)]
        if matches!(fault, Some(WatchdogFault::Spawn)) {
            consumer.invalidate(InvalidReason::WatchdogFailed);
            return Err(InvalidReason::WatchdogFailed.error());
        }
        let monitor = thread::Builder::new()
            .name("pulsar-fallback-expiry".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                #[cfg(test)]
                if matches!(fault, Some(WatchdogFault::EarlyExit | WatchdogFault::Panic)) {
                    let _exit = WatchdogExit(consumer.clone());
                    if matches!(fault, Some(WatchdogFault::Panic)) {
                        panic!("authored watchdog fault");
                    }
                    return;
                }
                watch(consumer);
            })
            .map_err(|_| lease.consumer.reject(InvalidReason::WatchdogFailed))?;
        lease.monitor = Some(monitor);
        lease.check()?;
        Ok(lease)
    }
    fn invalidate_matching(&self, matches: impl Fn(&Consumer) -> bool, reason: InvalidReason) {
        let entries = self.shared.entries.lock().unwrap_or_else(|error| {
            self.shared.failed.store(true, Ordering::Release);
            let entries = error.into_inner();
            for consumer in entries.consumers.values() {
                consumer.invalidate(InvalidReason::RegistryUnavailable);
            }
            entries
        });
        for consumer in entries
            .consumers
            .values()
            .filter(|consumer| matches(consumer))
        {
            consumer.invalidate(reason);
        }
    }
    pub(super) fn invalidate_dossier(&self, dossier: &ArtifactId) {
        self.invalidate_matching(
            |consumer| &consumer.reference.dossier_id == dossier,
            InvalidReason::DossierInvalidated,
        );
    }
    #[cfg(test)]
    pub(super) fn invalidate_actor(&self, actor: &SessionId) {
        self.invalidate_matching(
            |consumer| &consumer.owner == actor,
            InvalidReason::ActorInvalidated,
        );
    }
    #[cfg(test)]
    pub(super) fn invalidate_project(&self, project: &ProjectId) {
        self.invalidate_matching(
            |consumer| &consumer.project == project,
            InvalidReason::ProjectInvalidated,
        );
    }
    pub(super) fn invalidate_actor_project(&self, actor: &SessionId, project: &ProjectId) {
        self.invalidate_matching(
            |consumer| &consumer.owner == actor && &consumer.project == project,
            InvalidReason::ActorInvalidated,
        );
    }
    pub(super) fn invalidate_job(&self, job: &JobId) {
        self.invalidate_matching(|consumer| &consumer.job == job, InvalidReason::Cancelled);
    }
    pub(super) fn cancel_all(&self) {
        self.invalidate_matching(|_| true, InvalidReason::RegistryUnavailable);
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.shared.failed.store(true, Ordering::Release);
        self.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    struct TestClock {
        origin: Instant,
        wall: AtomicU64,
        elapsed_ms: AtomicU64,
        fault: AtomicU8,
    }
    impl TestClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                origin: Instant::now(),
                wall: AtomicU64::new(10_000),
                elapsed_ms: AtomicU64::new(0),
                fault: AtomicU8::new(0),
            })
        }
    }
    impl Clock for TestClock {
        fn sample(&self) -> Result<ClockSample, ()> {
            match self.fault.load(Ordering::Acquire) {
                1 => return Err(()),
                2 => panic!("authored bounded clock fault"),
                _ => {}
            }
            Ok(ClockSample {
                wall: self.wall.load(Ordering::Acquire),
                monotonic: self.origin
                    + Duration::from_millis(self.elapsed_ms.load(Ordering::Acquire)),
            })
        }
    }
    fn registration(index: usize, clock: &TestClock) -> Registration {
        Registration {
            job: JobId::new(format!("job-{index}")).unwrap(),
            project: ProjectId::new("project").unwrap(),
            owner: SessionId::new("actor").unwrap(),
            attempt: AttemptId::new(format!("attempt-{index}")).unwrap(),
            reference: InferenceFallbackReference {
                dossier_id: ArtifactId::new("authored-reference-no-acceptance").unwrap(),
                expected_revision: 3,
                expected_report_sha256: "a".repeat(64),
            },
            admitted_at_unix_ms: 10_000,
            expires_at_unix_ms: 20_000,
            attempt_budget_ms: 10_000,
            original_deadline: clock.origin + Duration::from_secs(10),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    fn wait_cancelled(lease: &Lease, expected: InvalidReason) {
        let end = Instant::now() + Duration::from_secs(2);
        while !lease.consumer.cancel.load(Ordering::Acquire) {
            assert!(Instant::now() < end, "expiry watchdog did not cancel");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(lease.invalid_reason(), Some(expected));
        assert!(lease.check().is_err());
    }
    fn count(registry: &Registry) -> usize {
        registry
            .shared
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .consumers
            .len()
    }

    #[test]
    fn lifetime_preserves_exact_reference_and_retirement_joins_and_releases_handle() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let registration = registration(1, &clock);
        let original = registration.original_deadline;
        let cancel = registration.cancel.clone();
        let lease = registry.register_inner(registration, clock).unwrap();
        lease.check().unwrap();
        assert_eq!(lease.reference().expected_revision, 3);
        assert_eq!(lease.reference().expected_report_sha256, "a".repeat(64));
        assert_eq!(lease.deadline(), original);
        let weak = Arc::downgrade(&lease.consumer);
        assert_eq!(count(&registry), 1);
        lease.retire().unwrap();
        assert!(cancel.load(Ordering::Acquire));
        assert_eq!(count(&registry), 0);
        assert!(
            weak.upgrade().is_none(),
            "watchdog or registry retained retired consumer"
        );
    }
    #[test]
    fn expiry_watchdog_cancels_without_a_dispatch_or_publication_poll() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let lease = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        clock.wall.store(20_000, Ordering::Release);
        wait_cancelled(&lease, InvalidReason::Expired);
        assert_eq!(lease.check().unwrap_err().code, ErrorCode::Cancelled);
        drop(lease);
        assert_eq!(count(&registry), 0);
    }
    #[test]
    fn original_monotonic_budget_is_independent_of_a_static_wall_clock() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let lease = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        clock.elapsed_ms.store(10_000, Ordering::Release);
        wait_cancelled(&lease, InvalidReason::Expired);
    }
    #[test]
    fn delayed_registration_cannot_renew_the_original_wall_or_monotonic_budget() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let mut admitted = registration(1, &clock);
        admitted.attempt_budget_ms = 1_000;
        admitted.expires_at_unix_ms = 100_000;
        admitted.original_deadline = clock.origin + Duration::from_secs(1);
        clock.wall.store(10_800, Ordering::Release);
        clock.elapsed_ms.store(800, Ordering::Release);
        let lease = registry.register_inner(admitted, clock.clone()).unwrap();
        assert_eq!(lease.deadline(), clock.origin + Duration::from_secs(1));
        clock.wall.store(11_000, Ordering::Release);
        wait_cancelled(&lease, InvalidReason::Expired);
    }
    #[test]
    fn decision_expiry_can_only_shorten_an_original_attempt_deadline() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let mut admitted = registration(1, &clock);
        admitted.expires_at_unix_ms = 10_250;
        let lease = registry.register_inner(admitted, clock.clone()).unwrap();
        assert_eq!(lease.deadline(), clock.origin + Duration::from_millis(250));
        clock.elapsed_ms.store(250, Ordering::Release);
        wait_cancelled(&lease, InvalidReason::Expired);
    }
    #[test]
    fn clock_rollback_and_clock_failure_latch_before_any_later_recovery() {
        for mode in [0, 1, 2] {
            let registry = Registry::default();
            let clock = TestClock::new();
            let lease = registry
                .register_inner(registration(mode, &clock), clock.clone())
                .unwrap();
            clock.wall.store(10_100, Ordering::Release);
            clock.elapsed_ms.store(100, Ordering::Release);
            lease.check().unwrap();
            let expected = match mode {
                0 => {
                    clock.wall.store(10_099, Ordering::Release);
                    InvalidReason::ClockRollback
                }
                1 => {
                    clock.elapsed_ms.store(99, Ordering::Release);
                    InvalidReason::ClockRollback
                }
                _ => {
                    clock.fault.store(1, Ordering::Release);
                    InvalidReason::ClockUnavailable
                }
            };
            wait_cancelled(&lease, expected);
            clock.wall.store(10_200, Ordering::Release);
            clock.elapsed_ms.store(200, Ordering::Release);
            clock.fault.store(0, Ordering::Release);
            assert_eq!(lease.invalid_reason(), Some(expected));
            assert!(lease.check().is_err());
        }
    }
    #[test]
    fn withdrawal_immediately_cancels_exact_dossier_and_preserves_first_reason() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let first = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        let mut other = registration(2, &clock);
        other.reference.dossier_id = ArtifactId::new("another-dossier").unwrap();
        let second = registry.register_inner(other, clock).unwrap();
        registry.invalidate_dossier(&first.reference().dossier_id);
        assert!(first.consumer.cancel.load(Ordering::Acquire));
        assert!(!second.consumer.cancel.load(Ordering::Acquire));
        registry.invalidate_actor(&first.consumer.owner);
        assert_eq!(
            first.invalid_reason(),
            Some(InvalidReason::DossierInvalidated)
        );
        assert_eq!(
            second.invalid_reason(),
            Some(InvalidReason::ActorInvalidated)
        );
    }
    #[test]
    fn actor_project_and_job_invalidation_do_not_cancel_unrelated_consumers() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let first = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        let mut other_project = registration(2, &clock);
        other_project.project = ProjectId::new("other-project").unwrap();
        let second = registry
            .register_inner(other_project, clock.clone())
            .unwrap();
        let mut other_actor = registration(3, &clock);
        other_actor.owner = SessionId::new("other-actor").unwrap();
        let third = registry.register_inner(other_actor, clock).unwrap();
        registry.invalidate_actor_project(&first.consumer.owner, &first.consumer.project);
        assert_eq!(
            first.invalid_reason(),
            Some(InvalidReason::ActorInvalidated)
        );
        second.check().unwrap();
        third.check().unwrap();
        registry.invalidate_job(&second.consumer.job);
        assert_eq!(second.invalid_reason(), Some(InvalidReason::Cancelled));
        third.check().unwrap();
        registry.invalidate_project(&third.consumer.project);
        assert_eq!(
            third.invalid_reason(),
            Some(InvalidReason::ProjectInvalidated)
        );
    }
    #[test]
    fn dropping_registry_cancels_live_consumers_without_joining_under_its_lock() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let lease = registry
            .register_inner(registration(1, &clock), clock)
            .unwrap();
        drop(registry);
        assert!(lease.consumer.cancel.load(Ordering::Acquire));
        assert_eq!(
            lease.invalid_reason(),
            Some(InvalidReason::RegistryUnavailable)
        );
        let weak = Arc::downgrade(&lease.consumer);
        lease.retire().unwrap();
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn predecessor_retirement_cannot_remove_a_different_successor_handle() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let predecessor = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        // A private adversarial replacement exercises pointer-identity cleanup;
        // the normal register API refuses every duplicate live attempt.
        registry
            .shared
            .entries
            .lock()
            .unwrap()
            .consumers
            .remove(&predecessor.consumer.attempt);
        let successor = registry
            .register_inner(registration(1, &clock), clock)
            .unwrap();
        drop(predecessor);
        assert_eq!(count(&registry), 1);
        successor.check().unwrap();
        assert!(!successor.consumer.cancel.load(Ordering::Acquire));
    }
    #[test]
    fn duplicate_and_capacity_refusals_release_only_the_new_registration() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let mut leases = Vec::new();
        for index in 0..MAX_CONSUMERS {
            leases.push(
                registry
                    .register_inner(registration(index, &clock), clock.clone())
                    .unwrap(),
            );
        }
        assert_eq!(count(&registry), MAX_CONSUMERS);
        let duplicate = registration(0, &clock);
        let duplicate_cancel = duplicate.cancel.clone();
        assert!(
            matches!(registry.register_inner(duplicate, clock.clone()), Err(error) if error.code==ErrorCode::RequestConflict)
        );
        assert!(duplicate_cancel.load(Ordering::Acquire));
        leases[0].check().unwrap();
        let overflow = registration(MAX_CONSUMERS, &clock);
        let overflow_cancel = overflow.cancel.clone();
        assert!(
            matches!(registry.register_inner(overflow, clock), Err(error) if error.code==ErrorCode::ResourceExhausted)
        );
        assert!(overflow_cancel.load(Ordering::Acquire));
        drop(leases);
        assert_eq!(count(&registry), 0);
    }
    #[test]
    fn invalid_timestamps_original_deadlines_and_budgets_never_issue_a_lease() {
        for mutate in [
            |r: &mut Registration| r.attempt_budget_ms = 0,
            |r: &mut Registration| r.attempt_budget_ms = MAX_BUDGET_MS + 1,
            |r: &mut Registration| r.admitted_at_unix_ms = 0,
            |r: &mut Registration| r.admitted_at_unix_ms = 10_001,
            |r: &mut Registration| r.expires_at_unix_ms = 10_000,
            |r: &mut Registration| r.expires_at_unix_ms = u64::MAX,
            |r: &mut Registration| r.original_deadline -= Duration::from_secs(10),
            |r: &mut Registration| r.original_deadline += Duration::from_secs(1),
            |r: &mut Registration| r.reference.expected_revision = 0,
            |r: &mut Registration| r.reference.expected_report_sha256 = "invalid".into(),
        ] {
            let registry = Registry::default();
            let clock = TestClock::new();
            let mut admitted = registration(1, &clock);
            mutate(&mut admitted);
            let cancel = admitted.cancel.clone();
            assert!(registry.register_inner(admitted, clock).is_err());
            assert!(cancel.load(Ordering::Acquire));
            assert_eq!(count(&registry), 0);
        }
    }
    #[test]
    fn watchdog_spawn_early_exit_and_panic_fail_closed_without_orphaned_handles() {
        for fault in [
            WatchdogFault::Spawn,
            WatchdogFault::EarlyExit,
            WatchdogFault::Panic,
        ] {
            let registry = Registry::default();
            let clock = TestClock::new();
            let admitted = registration(1, &clock);
            let cancel = admitted.cancel.clone();
            WATCHDOG_FAULT.with(|value| value.set(Some(fault)));
            if let Ok(lease) = registry.register_inner(admitted, clock) {
                wait_cancelled(&lease, InvalidReason::WatchdogFailed);
                let _ = lease.retire();
            }
            assert!(cancel.load(Ordering::Acquire));
            assert_eq!(count(&registry), 0);
        }
    }
    #[test]
    fn clock_panic_and_poisoned_registry_cancel_instead_of_silently_losing_watchdog() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let lease = registry
            .register_inner(registration(1, &clock), clock.clone())
            .unwrap();
        clock.fault.store(2, Ordering::Release);
        wait_cancelled(&lease, InvalidReason::WatchdogFailed);
        drop(lease);
        assert_eq!(count(&registry), 0);
        let clock = TestClock::new();
        let lease = registry
            .register_inner(registration(2, &clock), clock.clone())
            .unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = registry.shared.entries.lock().unwrap();
            panic!("authored registry poisoning");
        }));
        wait_cancelled(&lease, InvalidReason::RegistryUnavailable);
        assert!(registry
            .register_inner(registration(3, &clock), clock)
            .is_err());
        drop(lease);
        assert_eq!(count(&registry), 0);
    }
    #[test]
    fn external_attempt_cancellation_is_observed_without_replacing_its_flag() {
        let registry = Registry::default();
        let clock = TestClock::new();
        let admitted = registration(1, &clock);
        let cancel = admitted.cancel.clone();
        let lease = registry.register_inner(admitted, clock).unwrap();
        cancel.store(true, Ordering::Release);
        let end = Instant::now() + Duration::from_secs(2);
        while lease.invalid_reason().is_none() {
            assert!(Instant::now() < end);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(lease.invalid_reason(), Some(InvalidReason::Cancelled));
        assert!(lease.check().is_err());
    }
}
