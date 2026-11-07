//! Boot-local browser callback brackets and declared phase. No device authority.
use super::*;
use crate::authority::browser_media::{NativeBrowserMediaSuccessor, SourceMediaPin};
use arc_swap::ArcSwapOption;
use std::{
    sync::{atomic::AtomicU64, Weak},
    time::{Duration, Instant},
};

const MAX_BINDINGS: usize = 8;
const ENTRY_MEMORY: u64 = 16 * 1024;

pub(super) fn is_history_command(command: &Command) -> bool {
    matches!(
        command,
        Command::OpenBrowserTiming { .. }
            | Command::PrepareBrowserTimingSuccessor { .. }
            | Command::BrowserTimingOpenStatus { .. }
    )
}
fn unavailable() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "browser timing binding is not current",
    )
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "browser timing scope or sequence differs",
    )
}
fn token_hash(r: &Request) -> PResult<String> {
    r.auth_token
        .as_deref()
        .map(|token| format!("{:x}", Sha256::digest(token.as_bytes())))
        .ok_or_else(|| {
            ProtocolError::new(ErrorCode::Unauthorized, "session authentication required")
        })
}
fn same_hash(expected: &str, actual: &str) -> bool {
    let difference = expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |sum, (a, b)| sum | (a ^ b));
    expected.len() == actual.len() && difference == 0
}
fn opaque_id() -> ArtifactId {
    ArtifactId::new(format!("{:x}", Sha256::digest(Uuid::new_v4().as_bytes())))
        .expect("SHA256 identifier")
}
fn deadline(issue: u64, ms: u16) -> PResult<u64> {
    issue
        .checked_add(u64::from(ms) * 1_000_000)
        .ok_or_else(unavailable)
}
fn response(report: BrowserTimingReport) -> PResult<ResponseBody> {
    report.validate()?;
    Ok(ResponseBody::BrowserTiming(Box::new(report)))
}
/// Only a consumed native probe creates this value. Copying a serialized report
/// cannot create the native lifetime, and replacing an observation retires it.
struct NativeObservation {
    value: BrowserTimingObservation,
    current: Arc<AtomicBool>,
}
impl NativeObservation {
    fn retire(&self) {
        self.current.store(false, Ordering::Release);
    }
    fn current(&self, now: u64) -> bool {
        self.current.load(Ordering::Acquire) && now < self.value.expires_at_boottime_ns
    }
}
/// One immutable publication of actual native custody. Entry and the read path
/// share the same source and observation Arcs; neither comes from a report.
struct NativePublication {
    source: Arc<SourceMediaPin>,
    observation: Option<Arc<NativeObservation>>,
}
/// One stable player/source lifetime. Individual observations may be replaced;
/// that never rejuvenates a captured start's immutable observation or expiry.
struct Lifetime {
    binding: BrowserTimingBinding,
    alive: AtomicBool,
    deadline_ns: AtomicU64,
    publication: ArcSwapOption<NativePublication>,
    successor: Option<Arc<TimingSuccessorLineage>>,
    child_committed: AtomicBool,
}
struct TimingSuccessorLineage {
    predecessor: Weak<Lifetime>,
    predecessor_binding: BrowserTimingBinding,
    baseline: BrowserTimingObservation,
    committed: AtomicBool,
}
impl TimingSuccessorLineage {
    fn predecessor_current(&self, now: u64) -> bool {
        self.predecessor.upgrade().is_some_and(|lifetime| {
            lifetime.alive.load(Ordering::Acquire)
                && now < lifetime.deadline_ns.load(Ordering::Acquire)
        })
    }
}
impl Lifetime {
    fn active_at(&self, now: u64) -> bool {
        self.alive.load(Ordering::Acquire)
            && now < self.deadline_ns.load(Ordering::Acquire)
            && self.successor.as_ref().is_none_or(|lineage| {
                lineage.committed.load(Ordering::Acquire) || lineage.predecessor_current(now)
            })
    }
    fn current(&self, now: u64) -> bool {
        self.active_at(now)
            && self
                .publication
                .load()
                .as_ref()
                .is_some_and(|published| published.source.current())
    }
    fn retire(&self) {
        self.alive.store(false, Ordering::Release);
        self.publication.store(None);
    }
}
#[derive(Clone)]
pub(super) struct BrowserTimingPin {
    binding: BrowserTimingBinding,
    registry: Arc<Mutex<Registry>>,
    lifetime: Arc<Lifetime>,
}
#[derive(Debug)]
pub(super) enum BrowserTimingClaimFailure {
    Contended,
    Refused(ProtocolError),
}
/// One selected native publication. Selection grants no authority; evaluation
/// retains this exact source and observation through their original bounds.
pub(super) struct BrowserTimingRead {
    pin: BrowserTimingPin,
    publication: Arc<NativePublication>,
}
impl BrowserTimingRead {
    pub(super) fn evaluate_at(
        &self,
        now: u64,
    ) -> Result<BrowserTimingObservation, BrowserTimingClaimFailure> {
        let refused = || BrowserTimingClaimFailure::Refused(unavailable());
        let pin = &self.pin;
        if pin.binding != pin.lifetime.binding {
            return Err(BrowserTimingClaimFailure::Refused(mismatch()));
        }
        if pin.registry.is_poisoned() || !pin.lifetime.active_at(now) {
            return Err(refused());
        }
        let published = &self.publication;
        if published.source.binding() != &pin.binding.browser_media || !published.source.current() {
            return Err(refused());
        }
        let observation = &published.observation.as_ref().ok_or_else(refused)?.value;
        // A fixed caller tick may precede this publication. Refuse without
        // falling back, resampling, or retiring a newer native observation.
        // A replaced captured-start token is deliberately not fresh-read state.
        if observation.binding_id != pin.binding.binding_id
            || now < observation.bracket.native_receive_ns
            || now >= observation.expires_at_boottime_ns
            || pin.registry.is_poisoned()
            || !pin.lifetime.active_at(now)
            || !published.source.current()
        {
            return Err(refused());
        }
        Ok(observation.clone())
    }
}
impl BrowserTimingPin {
    pub(super) fn same_owner(&self, other: &Self) -> bool {
        self.binding == other.binding
            && Arc::ptr_eq(&self.registry, &other.registry)
            && Arc::ptr_eq(&self.lifetime, &other.lifetime)
    }
    #[cfg(test)]
    pub(super) fn hold_registry_for_test(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.registry.lock().unwrap()
    }
    #[cfg(test)]
    pub(super) fn with_binding_for_test(&self, binding: BrowserTimingBinding) -> Self {
        Self {
            binding,
            registry: self.registry.clone(),
            lifetime: self.lifetime.clone(),
        }
    }
    pub(super) fn binding(&self) -> &BrowserTimingBinding {
        &self.binding
    }
    pub(super) fn current(&self, now: u64) -> bool {
        self.binding == self.lifetime.binding
            && !self.registry.is_poisoned()
            && self.lifetime.current(now)
    }
    pub(super) fn latest(&self, now: u64) -> PResult<BrowserTimingObservation> {
        self.try_snapshot(now)
    }
    /// Retain one actual publication before the caller samples its evaluation
    /// clock. No clock is read here, and selection is not a native command claim.
    pub(super) fn select_read(&self) -> Result<BrowserTimingRead, BrowserTimingClaimFailure> {
        let refused = || BrowserTimingClaimFailure::Refused(unavailable());
        if self.binding != self.lifetime.binding {
            return Err(BrowserTimingClaimFailure::Refused(mismatch()));
        }
        if self.registry.is_poisoned() || !self.lifetime.alive.load(Ordering::Acquire) {
            return Err(refused());
        }
        let publication = self.lifetime.publication.load_full().ok_or_else(refused)?;
        if publication.source.binding() != &self.binding.browser_media
            || publication.observation.is_none()
        {
            return Err(refused());
        }
        Ok(BrowserTimingRead {
            pin: self.clone(),
            publication,
        })
    }
    /// Evaluate one selected publication at the caller's original tick. This
    /// never samples a clock or renews an observation or captured-start token.
    pub(super) fn try_snapshot_native(
        &self,
        now: u64,
    ) -> Result<BrowserTimingObservation, BrowserTimingClaimFailure> {
        self.select_read()?.evaluate_at(now)
    }
    /// Legacy result shape; snapshot reads no longer acquire a copy mutex.
    pub(super) fn try_snapshot(&self, now: u64) -> PResult<BrowserTimingObservation> {
        self.try_snapshot_native(now)
            .map_err(|failure| match failure {
                BrowserTimingClaimFailure::Contended => unavailable(),
                BrowserTimingClaimFailure::Refused(error) => error,
            })
    }
    /// One bounded native acquisition. Only an unacquired registry mutex may
    /// return retry custody; poison, validation and clock faults are refusals.
    pub(super) fn try_claim_native(
        &self,
        clock: impl FnOnce() -> PResult<u64>,
    ) -> Result<BrowserTimingClaim<'_>, BrowserTimingClaimFailure> {
        let mut registry = match self.registry.try_lock() {
            Ok(registry) => registry,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(BrowserTimingClaimFailure::Contended)
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(BrowserTimingClaimFailure::Refused(unavailable()))
            }
        };
        // Sample only after this registry is held. Pre-lock time can precede a
        // concurrent callback and must not become an apparent clock regression.
        let sampled = clock();
        if registry.clock_failed
            || sampled.as_ref().is_err()
            || sampled
                .as_ref()
                .is_ok_and(|now| registry.last_tick_ns.is_some_and(|last| *now < last))
        {
            registry.clock_failed = true;
            // Only atomic retirement here: no inner mutex acquisition can block
            // the final native claim. Ordinary maintenance releases resources.
            for entry in registry.entries.values_mut() {
                entry.retired = true;
                entry.lifetime.alive.store(false, Ordering::Release);
                if let Some(observation) = &entry.observation {
                    observation.retire();
                }
            }
            return Err(BrowserTimingClaimFailure::Refused(unavailable()));
        }
        let now = sampled.map_err(BrowserTimingClaimFailure::Refused)?;
        registry.last_tick_ns = Some(now);
        BrowserTimingClaim::from_native(self, registry, now)
            .map_err(BrowserTimingClaimFailure::Refused)
    }
    /// Claim both native observations under one registry guard. Staging reports
    /// and copied bindings cannot construct the retained lineage or media proof.
    pub(super) fn try_claim_successor_native(
        &self,
        successor: &BrowserTimingPin,
        media: &NativeBrowserMediaSuccessor,
        clock: impl FnOnce() -> PResult<u64>,
    ) -> Result<BrowserTimingSuccessorClaim<'_>, BrowserTimingClaimFailure> {
        if !Arc::ptr_eq(&self.registry, &successor.registry) {
            return Err(BrowserTimingClaimFailure::Refused(mismatch()));
        }
        let claim = self.try_claim_native(clock)?;
        let at = claim.original_admitted_at_ns;
        BrowserTimingSuccessorClaim::from_native(self, successor, media, claim._registry, at)
            .map_err(BrowserTimingClaimFailure::Refused)
    }
    /// Compatibility boundary; callers requiring retry custody use the typed
    /// native result instead of interpreting a generic Unavailable response.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn try_claim(
        &self,
        clock: impl FnOnce() -> PResult<u64>,
    ) -> PResult<BrowserTimingClaim<'_>> {
        self.try_claim_native(clock)
            .map_err(|failure| match failure {
                BrowserTimingClaimFailure::Contended => unavailable(),
                BrowserTimingClaimFailure::Refused(error) => error,
            })
    }
    /// The caller completes DB/journal work before taking this guard and holds
    /// it through the Handy-registry/runtime claim. No network or database I/O
    /// is performed while it is held.
    pub(super) fn claim<'a>(
        &'a self,
        deadline: Instant,
        cancel: &AtomicBool,
        clock: impl FnOnce() -> PResult<u64>,
    ) -> PResult<BrowserTimingClaim<'a>> {
        let mut registry = loop {
            if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(unavailable());
            }
            match self.registry.try_lock() {
                Ok(registry) => break registry,
                Err(std::sync::TryLockError::Poisoned(_)) => return Err(unavailable()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(1))
                }
            }
        };
        let now = registry.sweep(clock)?;
        BrowserTimingClaim::from_native(self, registry, now)
    }
}
pub(super) struct BrowserTimingClaim<'a> {
    _registry: std::sync::MutexGuard<'a, Registry>,
    registry_identity: Arc<Mutex<Registry>>,
    binding: BrowserTimingBinding,
    lifetime: Arc<Lifetime>,
    observation_lifetime: Arc<AtomicBool>,
    original_admitted_at_ns: u64,
    pub(super) admitted_at_ns: u64,
    pub(super) observation: BrowserTimingObservation,
}
impl<'a> BrowserTimingClaim<'a> {
    #[cfg(test)]
    pub(super) fn observation_lifetime_for_test(&self) -> Arc<AtomicBool> {
        self.observation_lifetime.clone()
    }
    fn from_native(
        pin: &BrowserTimingPin,
        registry: std::sync::MutexGuard<'a, Registry>,
        admitted_at_ns: u64,
    ) -> PResult<Self> {
        let native = checked_native_observation(&registry, pin, admitted_at_ns)?;
        let observation = native.value.clone();
        let observation_lifetime = native.current.clone();
        Ok(Self {
            _registry: registry,
            registry_identity: pin.registry.clone(),
            binding: pin.binding.clone(),
            lifetime: pin.lifetime.clone(),
            observation_lifetime,
            original_admitted_at_ns: admitted_at_ns,
            admitted_at_ns,
            observation,
        })
    }
    /// Borrow the actual guarded observation, preserving its original validity.
    /// Public copies are descriptive: modifying one cannot alter this proof.
    pub(super) fn observation_for(
        &self,
        pin: &BrowserTimingPin,
        at: u64,
    ) -> PResult<&BrowserTimingObservation> {
        if !Arc::ptr_eq(&self.registry_identity, &pin.registry)
            || self.binding != pin.binding
            || !Arc::ptr_eq(&self.lifetime, &pin.lifetime)
            || self.admitted_at_ns != self.original_admitted_at_ns
            || at < self.original_admitted_at_ns
        {
            return Err(mismatch());
        }
        let native = checked_native_observation(&self._registry, pin, at)?;
        if !Arc::ptr_eq(&self.observation_lifetime, &native.current)
            || self.observation != native.value
        {
            return Err(mismatch());
        }
        Ok(&native.value)
    }
}
/// Guarded native lineage only. Committing this proof does not activate a
/// capture, controller or physical effect; the producer's joint transfer owns it.
pub(super) struct BrowserTimingSuccessorClaim<'a> {
    registry: std::sync::MutexGuard<'a, Registry>,
    predecessor: BrowserTimingPin,
    successor: BrowserTimingPin,
    media: NativeBrowserMediaSuccessor,
    lineage: Arc<TimingSuccessorLineage>,
    predecessor_observation: Arc<AtomicBool>,
    successor_observation: Arc<AtomicBool>,
    admitted_at_ns: u64,
    consumed: bool,
}
impl<'a> BrowserTimingSuccessorClaim<'a> {
    fn from_native(
        old: &BrowserTimingPin,
        new: &BrowserTimingPin,
        media: &NativeBrowserMediaSuccessor,
        registry: std::sync::MutexGuard<'a, Registry>,
        at: u64,
    ) -> PResult<Self> {
        let predecessor_observation = checked_native_observation(&registry, old, at)?
            .current
            .clone();
        let successor_observation = checked_native_observation(&registry, new, at)?
            .current
            .clone();
        let lineage = new
            .lifetime
            .successor
            .as_ref()
            .ok_or_else(mismatch)?
            .clone();
        let claim = Self {
            registry,
            predecessor: old.clone(),
            successor: new.clone(),
            media: media.clone(),
            lineage,
            predecessor_observation,
            successor_observation,
            admitted_at_ns: at,
            consumed: false,
        };
        claim.observations_for(old, new, at)?;
        Ok(claim)
    }
    pub(super) fn observations_for(
        &self,
        old: &BrowserTimingPin,
        new: &BrowserTimingPin,
        at: u64,
    ) -> PResult<(&BrowserTimingObservation, &BrowserTimingObservation)> {
        if self.consumed
            || at < self.admitted_at_ns
            || !Arc::ptr_eq(&old.registry, &self.predecessor.registry)
            || !Arc::ptr_eq(&new.registry, &self.predecessor.registry)
            || old.binding != self.predecessor.binding
            || new.binding != self.successor.binding
            || !Arc::ptr_eq(&old.lifetime, &self.predecessor.lifetime)
            || !Arc::ptr_eq(&new.lifetime, &self.successor.lifetime)
            || self.lineage.predecessor_binding != old.binding
            || self.lineage.committed.load(Ordering::Acquire)
            || old.lifetime.child_committed.load(Ordering::Acquire)
            || !self
                .lineage
                .predecessor
                .upgrade()
                .is_some_and(|lifetime| Arc::ptr_eq(&lifetime, &old.lifetime))
            || !new
                .lifetime
                .successor
                .as_ref()
                .is_some_and(|lineage| Arc::ptr_eq(lineage, &self.lineage))
            || !self.media.current()
        {
            return Err(mismatch());
        }
        let old_native = checked_native_observation(&self.registry, old, at)?;
        let new_native = checked_native_observation(&self.registry, new, at)?;
        let old_entry = self
            .registry
            .entries
            .get(&old.binding.binding_id)
            .ok_or_else(unavailable)?;
        let new_entry = self
            .registry
            .entries
            .get(&new.binding.binding_id)
            .ok_or_else(unavailable)?;
        if !Arc::ptr_eq(&old_native.current, &self.predecessor_observation)
            || !Arc::ptr_eq(&new_native.current, &self.successor_observation)
            || !self.media.matches(
                old_entry.source.as_ref().ok_or_else(unavailable)?,
                new_entry.source.as_ref().ok_or_else(unavailable)?,
            )
            || old.binding.player != new.binding.player
            || old.binding.policy != new.binding.policy
        {
            return Err(mismatch());
        }
        Ok((&old_native.value, &new_native.value))
    }
    pub(super) fn commit(&mut self, at: u64) -> PResult<()> {
        self.observations_for(&self.predecessor, &self.successor, at)?;
        self.predecessor
            .lifetime
            .child_committed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| mismatch())?;
        self.lineage.committed.store(true, Ordering::Release);
        self.consumed = true;
        Ok(())
    }
}
fn checked_native_observation<'a>(
    registry: &'a Registry,
    pin: &BrowserTimingPin,
    at: u64,
) -> PResult<&'a NativeObservation> {
    if registry.clock_failed {
        return Err(unavailable());
    }
    let entry = registry
        .entries
        .get(&pin.binding.binding_id)
        .ok_or_else(unavailable)?;
    if entry.binding != pin.binding
        || !Arc::ptr_eq(&entry.lifetime, &pin.lifetime)
        || !entry.lifetime.alive.load(Ordering::Acquire)
        || at >= entry.lifetime.deadline_ns.load(Ordering::Acquire)
        || !entry.current(at)
    {
        return Err(unavailable());
    }
    // The registry and publication share exact native custody. Final claims
    // still borrow the serialized record and require its current start token.
    let native = entry.observation.as_ref().ok_or_else(unavailable)?;
    let published = entry
        .lifetime
        .publication
        .load_full()
        .ok_or_else(unavailable)?;
    if !Arc::ptr_eq(
        native,
        published.observation.as_ref().ok_or_else(unavailable)?,
    ) || !Arc::ptr_eq(
        entry.source.as_ref().ok_or_else(unavailable)?,
        &published.source,
    ) {
        return Err(mismatch());
    }
    if native.value.binding_id != entry.binding.binding_id
        || at < native.value.bracket.native_receive_ns
        || !native.current(at)
        || !native.value.frame.is_current()
    {
        return Err(unavailable());
    }
    Ok(native)
}
struct Entry {
    owner: SessionId,
    auth_hash: String,
    binding: BrowserTimingBinding,
    source: Option<Arc<SourceMediaPin>>,
    lifetime: Arc<Lifetime>,
    pending: Option<BrowserTimingProbe>,
    observation: Option<Arc<NativeObservation>>,
    deadline_ns: u64,
    retired: bool,
    _memory: Reservation,
}
impl Entry {
    fn retire(&mut self) {
        self.retired = true;
        self.lifetime.retire();
        self.source = None;
        self.pending = None;
        if let Some(observation) = &self.observation {
            observation.retire();
        }
    }
    fn current(&self, now: u64) -> bool {
        !self.retired
            && now < self.deadline_ns
            && self.lifetime.successor.as_ref().is_none_or(|lineage| {
                lineage.committed.load(Ordering::Acquire) || lineage.predecessor_current(now)
            })
            && self.source.as_ref().is_some_and(|source| source.current())
            && self
                .pending
                .as_ref()
                .is_none_or(|probe| now < probe.expires_at_boottime_ns)
            && self
                .observation
                .as_ref()
                .is_none_or(|observation| observation.current(now))
    }
    fn report(&self) -> BrowserTimingReport {
        BrowserTimingReport {
            binding: self.binding.clone(),
            current: !self.retired,
            observation: self.observation.as_ref().map(|o| o.value.clone()),
            qualified: false,
        }
    }
    fn next_sequence(&self) -> PResult<u64> {
        self.observation
            .as_ref()
            .map_or(0, |o| o.value.sequence)
            .checked_add(1)
            .ok_or_else(unavailable)
    }
}
#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<ArtifactId, Entry>,
    last_tick_ns: Option<u64>,
    clock_failed: bool,
    successors: HashMap<ArtifactId, Weak<Lifetime>>,
}
impl Registry {
    /// Test observation of the real, intentionally retained metadata owners.
    /// It neither retires nor removes entries and returns no live capability.
    #[cfg(test)]
    pub(super) fn retired_metadata_for_test(&self) -> PResult<(usize, u64)> {
        if self.entries.len() > MAX_BINDINGS
            || self.entries.values().any(|entry| {
                !entry.retired
                    || entry.source.is_some()
                    || entry.pending.is_some()
                    || entry.lifetime.alive.load(Ordering::Acquire)
                    || entry.lifetime.publication.load().is_some()
                    || entry
                        .observation
                        .as_ref()
                        .is_some_and(|observation| observation.current.load(Ordering::Acquire))
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "browser timing still retains more than retired metadata",
            ));
        }
        let count = self.entries.len();
        // This is the same amount reserved by the sole Entry constructor.
        // The native assertion independently verifies the exact pool remainder.
        let bytes = u64::try_from(count)
            .map_err(internal)?
            .checked_mul(ENTRY_MEMORY)
            .ok_or_else(unavailable)?;
        Ok((count, bytes))
    }
    pub(super) fn cancel_all(&mut self) {
        for entry in self.entries.values_mut() {
            entry.retire();
        }
    }
    fn sweep(&mut self, clock: impl FnOnce() -> PResult<u64>) -> PResult<u64> {
        let now = clock();
        if now.as_ref().is_err()
            || now
                .as_ref()
                .is_ok_and(|now| self.last_tick_ns.is_some_and(|last| *now < last))
        {
            self.clock_failed = true;
        }
        if self.clock_failed {
            self.cancel_all();
            return Err(unavailable());
        }
        let now = now?;
        self.last_tick_ns = Some(now);
        for entry in self.entries.values_mut() {
            if !entry.current(now) {
                entry.retire();
            }
        }
        self.successors
            .retain(|_, lifetime| lifetime.strong_count() > 0);
        Ok(now)
    }
    fn admit(&mut self, actor: &SessionId, player: &HandyPlayerIdentity) -> PResult<()> {
        if self
            .entries
            .values()
            .any(|e| !e.retired && e.owner == *actor && e.binding.player == *player)
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "browser player already has current timing",
            ));
        }
        self.admit_capacity()
    }
    fn admit_capacity(&mut self) -> PResult<()> {
        if self.entries.len() >= MAX_BINDINGS {
            self.entries.retain(|_, e| !e.retired);
        }
        if self.entries.len() >= MAX_BINDINGS {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "browser timing registry is full",
            ));
        }
        Ok(())
    }
}
impl Engine {
    pub(super) fn browser_timing_scope(
        &self,
        r: &Request,
        binding_id: &ArtifactId,
    ) -> PResult<BrowserTimingPin> {
        let actor = r.session.as_ref().ok_or_else(unavailable)?;
        let hash = token_hash(r)?;
        let mut registry = self.browser_timing.lock().map_err(internal)?;
        let entry = registry.entries.get(binding_id).ok_or_else(unavailable)?;
        if entry.owner != *actor
            || !same_hash(&entry.auth_hash, &hash)
            || r.project.as_ref() != Some(&entry.binding.project_id)
        {
            return Err(mismatch());
        }
        let _ = registry.sweep(|| self.browser_timing_now());
        let entry = registry.entries.get(binding_id).ok_or_else(unavailable)?;
        Ok(BrowserTimingPin {
            binding: entry.binding.clone(),
            registry: self.browser_timing.clone(),
            lifetime: entry.lifetime.clone(),
        })
    }
    pub(super) fn capture_browser_timing_locked(
        &self,
        db: &Connection,
        r: &Request,
        binding_id: &ArtifactId,
    ) -> PResult<BrowserTimingPin> {
        let actor = r.session.as_ref().ok_or_else(unavailable)?;
        self.authenticate(db, actor, r.auth_token.as_deref())?;
        require_grant(db, actor, project(r)?, Scope::Read)?;
        require_grant(db, actor, project(r)?, Scope::Export)?;
        let pin = self.browser_timing_scope(r, binding_id)?;
        self.pin_browser_media_locked(db, r, &pin.binding.browser_media)?;
        let now = self.browser_timing_now()?;
        pin.latest(now)?;
        Ok(pin)
    }
    pub(super) fn browser_timing_now(&self) -> PResult<u64> {
        #[cfg(test)]
        {
            let tick = self.browser_timing_test_tick.load(Ordering::Acquire);
            if tick != 0 {
                return Ok(tick);
            }
        }
        crate::handy_clock::boottime_ns()
    }
    pub(super) fn sweep_browser_timing(&self) {
        let mut registry = self.browser_timing.lock().unwrap_or_else(|e| {
            let mut registry = e.into_inner();
            registry.clock_failed = true;
            registry
        });
        let _ = registry.sweep(|| self.browser_timing_now());
    }
    fn browser_timing_history_view(
        &self,
        actor: &SessionId,
        r: &Request,
        mut report: BrowserTimingReport,
    ) -> PResult<ResponseBody> {
        report.validate()?;
        if report.binding.project_id != *project(r)? || report.observation.is_some() {
            return Err(mismatch());
        }
        let hash = token_hash(r)?;
        let mut registry = self.browser_timing.lock().map_err(internal)?;
        let _ = registry.sweep(|| self.browser_timing_now());
        if let Some(entry) = registry.entries.get(&report.binding.binding_id) {
            if entry.owner == *actor
                && same_hash(&entry.auth_hash, &hash)
                && entry.binding == report.binding
            {
                return response(entry.report());
            }
        }
        report.current = false;
        response(report)
    }
    pub(super) fn browser_timing_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Command::BrowserTimingOpenStatus {
            original_request_id,
        } = &r.command
        {
            let raw: Option<String> = db.query_row(
                "SELECT CASE WHEN length(CAST(response AS BLOB))<=32768 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
                params![actor.as_str(), original_request_id.as_str()], |row| row.get(0),
            ).optional().map_err(internal)?;
            let ResponseBody::BrowserTiming(report) = decode(&raw.ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "browser timing open not found")
            })?)?
            else {
                return Err(mismatch());
            };
            if report.binding.request_id != *original_request_id {
                return Err(mismatch());
            }
            return self.browser_timing_history_view(actor, r, *report);
        }
        let (player, browser_media, policy, predecessor_id) = match &r.command {
            Command::OpenBrowserTiming {
                player,
                browser_media,
                policy,
            } => (player, browser_media, policy, None),
            Command::PrepareBrowserTimingSuccessor {
                predecessor_binding_id,
                player,
                browser_media,
                policy,
            } => (player, browser_media, policy, Some(predecessor_binding_id)),
            _ => return Err(ProtocolError::unsupported("browser timing open")),
        };
        if let Some(saved) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::BrowserTiming(report) = saved else {
                return Err(mismatch());
            };
            if report.binding.request_id != r.request_id
                || report.binding.player != *player
                || report.binding.browser_media != *browser_media
                || report.binding.policy != *policy
            {
                return Err(mismatch());
            }
            return self.browser_timing_history_view(actor, r, *report);
        }
        check_revision(&db, r)?;
        let source = self.pin_browser_media_locked(&db, r, browser_media)?;
        let predecessor = predecessor_id
            .map(|id| self.capture_browser_timing_locked(&db, r, id))
            .transpose()?;
        let media_successor = predecessor
            .as_ref()
            .map(|old| {
                if old.binding.player != *player
                    || old.binding.policy != *policy
                    || old.binding.request_id == r.request_id
                {
                    return Err(mismatch());
                }
                self.capture_browser_media_successor_locked(
                    &db,
                    r,
                    &old.binding.browser_media,
                    browser_media,
                )
            })
            .transpose()?;
        let memory = self.pool.reserve_memory(ENTRY_MEMORY)?;
        let hash = token_hash(r)?;
        let (begin, successor) = {
            let mut registry = self.browser_timing.lock().map_err(internal)?;
            let now = registry.sweep(|| self.browser_timing_now())?;
            let lineage = if let Some(old) = &predecessor {
                let native = checked_native_observation(&registry, old, now)?;
                if old.lifetime.child_committed.load(Ordering::Acquire)
                    || old
                        .lifetime
                        .successor
                        .as_ref()
                        .is_some_and(|lineage| !lineage.committed.load(Ordering::Acquire))
                    || registry
                        .successors
                        .get(&old.binding.binding_id)
                        .and_then(Weak::upgrade)
                        .is_some_and(|lifetime| lifetime.alive.load(Ordering::Acquire))
                    || !media_successor
                        .as_ref()
                        .is_some_and(NativeBrowserMediaSuccessor::current)
                {
                    return Err(mismatch());
                }
                let lineage = Arc::new(TimingSuccessorLineage {
                    predecessor: Arc::downgrade(&old.lifetime),
                    predecessor_binding: old.binding.clone(),
                    baseline: native.value.clone(),
                    committed: AtomicBool::new(false),
                });
                registry.admit_capacity()?;
                Some(lineage)
            } else {
                registry.admit(actor, player)?;
                None
            };
            (now, lineage)
        };
        let deadline_ns = deadline(begin, policy.observation_timeout_ms)?;
        let mut report = BrowserTimingReport {
            binding: BrowserTimingBinding {
                request_id: r.request_id.clone(),
                binding_id: opaque_id(),
                engine_epoch: AttemptId::new(self.engine_epoch.clone()).map_err(internal)?,
                project_id: project(r)?.clone(),
                revision: r.expected_revision.ok_or_else(mismatch)?,
                player: player.clone(),
                browser_media: browser_media.clone(),
                policy: policy.clone(),
            },
            current: true,
            observation: None,
            qualified: false,
        };
        report.validate()?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            r,
            fingerprint,
            &response(report.clone())?,
        )?;
        tx.commit().map_err(internal)?;
        let mut registry = self.browser_timing.lock().map_err(internal)?;
        report.current = registry
            .sweep(|| self.browser_timing_now())
            .is_ok_and(|now| {
                now < deadline_ns
                    && source.current()
                    && !self.is_shutting_down()
                    && successor
                        .as_ref()
                        .is_none_or(|lineage| lineage.predecessor_current(now))
                    && media_successor
                        .as_ref()
                        .is_none_or(NativeBrowserMediaSuccessor::current)
            });
        #[cfg(test)]
        if !report.current {
            eprintln!(
                "browser timing open recheck: begin_ns={begin}, recheck_ns={:?}, deadline_ns={deadline_ns}, clock_failed={}, source_current={}, shutting_down={}",
                registry.last_tick_ns,
                registry.clock_failed,
                source.current(),
                self.is_shutting_down()
            );
        }
        if report.current {
            let source = Arc::new(source);
            let lifetime = Arc::new(Lifetime {
                binding: report.binding.clone(),
                alive: AtomicBool::new(true),
                deadline_ns: AtomicU64::new(deadline_ns),
                publication: ArcSwapOption::from(Some(Arc::new(NativePublication {
                    source: source.clone(),
                    observation: None,
                }))),
                successor,
                child_committed: AtomicBool::new(false),
            });
            if let Some(old) = &predecessor {
                registry
                    .successors
                    .insert(old.binding.binding_id.clone(), Arc::downgrade(&lifetime));
            }
            registry.entries.insert(
                report.binding.binding_id.clone(),
                Entry {
                    owner: actor.clone(),
                    auth_hash: hash,
                    binding: report.binding.clone(),
                    lifetime,
                    source: Some(source),
                    pending: None,
                    observation: None,
                    deadline_ns,
                    retired: false,
                    _memory: memory,
                },
            );
        }
        response(report)
    }
    /// This route precedes all DB/live/transfer work. Authentication checks only
    /// the exact installed actor, token hash, project and browser player scope.
    pub(super) fn browser_timing_fast(&self, r: &Request) -> Option<PResult<ResponseBody>> {
        if !matches!(
            r.command,
            Command::IssueBrowserTimingProbe { .. }
                | Command::ObserveBrowserTiming { .. }
                | Command::BrowserTimingStatus { .. }
                | Command::ReleaseBrowserTiming { .. }
        ) {
            return None;
        }
        Some(self.browser_timing_fast_inner(r))
    }
    fn browser_timing_fast_inner(&self, r: &Request) -> PResult<ResponseBody> {
        r.validate()?;
        let (id, player) = match &r.command {
            Command::IssueBrowserTimingProbe {
                binding_id, player, ..
            }
            | Command::ObserveBrowserTiming {
                binding_id, player, ..
            }
            | Command::BrowserTimingStatus { binding_id, player }
            | Command::ReleaseBrowserTiming { binding_id, player } => (binding_id, player),
            _ => return Err(mismatch()),
        };
        let owner = r.session.as_ref().ok_or_else(unavailable)?;
        let hash = token_hash(r)?;
        let mut registry = self.browser_timing.lock().map_err(internal)?;
        if self.is_shutting_down() {
            registry.cancel_all();
            return Err(unavailable());
        }
        let entry = registry
            .entries
            .get(id)
            .filter(|e| {
                e.owner == *owner
                    && same_hash(&e.auth_hash, &hash)
                    && e.binding.player == *player
                    && r.project.as_ref() == Some(&e.binding.project_id)
            })
            .ok_or_else(mismatch)?;
        // Reordered messages are inert even when a newer probe/observation has
        // replaced their nonce. Only the exact current exchange can retire it.
        match &r.command {
            Command::IssueBrowserTimingProbe { sequence, .. } => {
                if entry.next_sequence()? != *sequence {
                    return Err(mismatch());
                }
            }
            Command::ObserveBrowserTiming {
                sequence, probe_id, ..
            } if !entry
                .pending
                .as_ref()
                .is_some_and(|p| p.sequence == *sequence && p.probe_id == *probe_id) =>
            {
                return Err(mismatch());
            }
            _ => {}
        }
        let now = registry.sweep(|| self.browser_timing_now());
        let entry = registry.entries.get_mut(id).ok_or_else(unavailable)?;
        if matches!(r.command, Command::BrowserTimingStatus { .. }) {
            return response(entry.report());
        }
        if matches!(r.command, Command::ReleaseBrowserTiming { .. }) {
            entry.retire();
            return response(entry.report());
        }
        let now = now?;
        if entry.retired {
            return Err(unavailable());
        }
        match &r.command {
            Command::IssueBrowserTimingProbe { sequence, .. } => {
                if let Some(probe) = &entry.pending {
                    return Ok(ResponseBody::BrowserTimingProbe(Box::new(probe.clone())));
                }
                let probe = BrowserTimingProbe {
                    binding_id: id.clone(),
                    player: player.clone(),
                    sequence: *sequence,
                    probe_id: opaque_id(),
                    issued_at_boottime_ns: now,
                    expires_at_boottime_ns: deadline(now, entry.binding.policy.probe_timeout_ms)?,
                };
                probe.validate()?;
                entry.pending = Some(probe.clone());
                Ok(ResponseBody::BrowserTimingProbe(Box::new(probe)))
            }
            Command::ObserveBrowserTiming { frame, .. } => {
                let probe = entry.pending.take().ok_or_else(mismatch)?;
                let phase = i64::try_from(frame.sample.media_time_us)
                    .ok()
                    .and_then(|us| SourceTimestamp::new(us, 1_000_000).ok())
                    .and_then(|time| {
                        entry
                            .binding
                            .browser_media
                            .clock
                            .project_at(time, &entry.binding.browser_media.selection.placement.spec)
                            .ok()
                    });
                let prior = entry
                    .observation
                    .as_ref()
                    .map(|old| &old.value)
                    .or_else(|| {
                        entry
                            .lifetime
                            .successor
                            .as_ref()
                            .map(|lineage| &lineage.baseline)
                    });
                let increasing = prior.is_none_or(|old| {
                    frame.sample.browser_tick_us > old.frame.sample.browser_tick_us
                        && frame.sample.presented_frames > old.frame.sample.presented_frames
                        && frame.sample.media_time_us > old.frame.sample.media_time_us
                        && phase.is_some_and(|phase| {
                            phase
                                .checked_cmp(old.project_phase)
                                .is_ok_and(|order| order == std::cmp::Ordering::Greater)
                        })
                });
                if !frame.is_current() || !increasing || phase.is_none() {
                    entry.retire();
                    return response(entry.report());
                }
                let observation = BrowserTimingObservation {
                    observation_id: opaque_id(),
                    binding_id: id.clone(),
                    sequence: probe.sequence,
                    frame: frame.clone(),
                    bracket: BrowserClockBracket {
                        native_issue_ns: probe.issued_at_boottime_ns,
                        native_receive_ns: now,
                        browser_sample_us: frame.sample.browser_tick_us,
                    },
                    project_phase: phase.expect("checked declared phase"),
                    expires_at_boottime_ns: deadline(
                        probe.issued_at_boottime_ns,
                        entry.binding.policy.observation_timeout_ms,
                    )?,
                };
                observation.validate()?;
                let mut next_report = entry.report();
                next_report.observation = Some(observation.clone());
                let reply = response(next_report)?;
                let native = Arc::new(NativeObservation {
                    value: observation,
                    current: Arc::new(AtomicBool::new(true)),
                });
                let publication = Arc::new(NativePublication {
                    source: entry.source.as_ref().ok_or_else(unavailable)?.clone(),
                    observation: Some(native.clone()),
                });
                // Allocate and validate before touching the old token. Registry
                // serialization excludes final claims through this whole commit.
                // Fresh readers do not test the captured-start token, so the old
                // complete publication remains readable within its original
                // bounds while that token retires before replacement is exposed.
                // No application dependency lock is acquired after retirement.
                if let Some(old) = &entry.observation {
                    old.retire();
                }
                entry.observation = Some(native.clone());
                entry.deadline_ns = native.value.expires_at_boottime_ns;
                entry
                    .lifetime
                    .deadline_ns
                    .store(native.value.expires_at_boottime_ns, Ordering::Release);
                entry.lifetime.publication.store(Some(publication));
                Ok(reply)
            }
            _ => Err(mismatch()),
        }
    }
    #[cfg(test)]
    pub(super) fn browser_timing_clock_for_test(&self) -> u64 {
        let _registry = self.browser_timing.lock().unwrap();
        let now = crate::handy_clock::boottime_ns().unwrap();
        self.browser_timing_test_tick.store(now, Ordering::Release);
        now
    }
    #[cfg(test)]
    pub(super) fn seed_browser_timing_clock_for_test(&self, tick: u64) {
        let mut registry = self.browser_timing.lock().unwrap();
        assert!(registry.entries.is_empty());
        assert!(!registry.clock_failed);
        registry.last_tick_ns = None;
        self.browser_timing_test_tick.store(tick, Ordering::Release);
    }
    #[cfg(test)]
    pub(super) fn set_browser_timing_clock_for_test(&self, tick: u64) {
        let _registry = self.browser_timing.lock().unwrap();
        self.browser_timing_test_tick.store(tick, Ordering::Release);
    }
    #[cfg(test)]
    pub(super) fn browser_timing_retention_for_test(&self) -> (usize, usize) {
        let registry = self.browser_timing.lock().unwrap();
        (
            registry.entries.len(),
            registry
                .entries
                .values()
                .filter(|e| e.source.is_some())
                .count(),
        )
    }
}
