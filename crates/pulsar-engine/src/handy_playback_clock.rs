//! Native composition of separately qualified clock errors. No JSON constructor grants authority.
use crate::{handy_server_clock::NativeServerClock, playback_runtime::PlaybackClock};
use pulsar_protocol::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
type Result<T> = std::result::Result<T, ProtocolError>;
fn invalid() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Unavailable,
        "device or combined clock is stale, changed or outside accepted bounds",
    )
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct QualifiedDeviceClock {
    pub(crate) contract: HandyDeviceClockQualification,
    pub(crate) maximum_total_error_ns: u64,
}
#[derive(Clone)]
pub(crate) struct NativeDeviceClock {
    pub(crate) acknowledgment: HandyClockSyncAcknowledgment,
    pub(crate) qualified: QualifiedDeviceClock,
}
impl NativeDeviceClock {
    pub(crate) fn new(
        acknowledgment: HandyClockSyncAcknowledgment,
        qualified: QualifiedDeviceClock,
    ) -> Result<Self> {
        acknowledgment.validate()?;
        qualified.contract.validate()?;
        if acknowledgment.info.last_sync_round_trip_ms
            > qualified.contract.maximum_sync_round_trip_ms
        {
            return Err(invalid());
        }
        let value = Self {
            acknowledgment,
            qualified,
        };
        value.error_at(value.acknowledgment.received_boottime_ns)?;
        Ok(value)
    }
    pub(crate) fn error_at(&self, now: u64) -> Result<u64> {
        let age = now
            .checked_sub(self.acknowledgment.sent_boottime_ns)
            .ok_or_else(invalid)?;
        let elapsed = now
            .checked_sub(self.acknowledgment.received_boottime_ns)
            .ok_or_else(invalid)?;
        let error = u128::from(self.qualified.contract.device_error_ns)
            + (u128::from(elapsed) * u128::from(self.qualified.contract.drift_ppm))
                .div_ceil(1_000_000);
        if age > self.qualified.contract.maximum_sync_age_ns
            || error > u128::from(self.qualified.maximum_total_error_ns)
        {
            return Err(invalid());
        }
        error.try_into().map_err(|_| invalid())
    }
    pub(crate) fn check_observation(&self, sample: &HandyDeviceTimingSample) -> Result<()> {
        sample.validate()?;
        self.error_at(sample.read_finished_boottime_ns)?;
        if sample.read_started_boottime_ns < self.acknowledgment.received_boottime_ns
            || sample.user_offset.offset_ms != self.qualified.contract.user_offset_ms
            || sample.info.device_uptime_ms < self.acknowledgment.info.device_uptime_ms
            || sample.info.reported_clock_offset_ms
                != self.acknowledgment.info.reported_clock_offset_ms
            || sample.info.last_sync_round_trip_ms
                != self.acknowledgment.info.last_sync_round_trip_ms
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(crate) struct NativePlaybackClock {
    pub(crate) server: NativeServerClock,
    pub(crate) device: NativeDeviceClock,
}
impl NativePlaybackClock {
    pub(crate) fn synchronization_until_ns(&self) -> Result<u64> {
        self.device
            .acknowledgment
            .sent_boottime_ns
            .checked_add(self.device.qualified.contract.maximum_sync_age_ns)
            .ok_or_else(invalid)
    }
    pub(crate) fn at(&self, now: u64) -> Result<PlaybackClock> {
        let mut clock = self.server.at(now)?;
        clock.valid_until_ns = clock.valid_until_ns.min(
            self.device
                .acknowledgment
                .sent_boottime_ns
                .saturating_add(self.device.qualified.contract.maximum_sync_age_ns),
        );
        clock.uncertainty_ns = clock
            .uncertainty_ns
            .checked_add(self.device.error_at(now)?)
            .ok_or_else(invalid)?;
        if clock.uncertainty_ns > self.device.qualified.maximum_total_error_ns {
            return Err(invalid());
        }
        Ok(clock)
    }
}

struct HspClockObservation {
    owner_id: ArtifactId,
    generation: u64,
    clock: Arc<NativePlaybackClock>,
    valid_until_ns: u64,
    session_until_ns: u64,
    retired: Arc<AtomicBool>,
}
/// Immutable native observation identity. Cloning preserves the exact original
/// clock and expiry; no report/JSON constructor can supply this capability.
#[derive(Clone)]
pub(crate) struct HspClockPin(Arc<HspClockObservation>);
impl std::fmt::Debug for HspClockPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HspClockPin")
            .field("owner_id", &self.0.owner_id)
            .field("generation", &self.0.generation)
            .field("valid_until_ns", &self.0.valid_until_ns)
            .field("session_until_ns", &self.0.session_until_ns)
            .finish()
    }
}
impl PartialEq for HspClockPin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for HspClockPin {}
impl HspClockPin {
    pub(crate) fn new(
        owner_id: ArtifactId,
        generation: u64,
        clock: Arc<NativePlaybackClock>,
        valid_until_ns: u64,
        session_until_ns: u64,
        retired: Arc<AtomicBool>,
    ) -> Result<Self> {
        let at = clock.server.clock.monotonic_ns;
        if generation == 0
            || valid_until_ns <= at
            || valid_until_ns > session_until_ns
            || valid_until_ns > clock.at(at)?.valid_until_ns
            || session_until_ns > clock.synchronization_until_ns()?
        {
            return Err(invalid());
        }
        Ok(Self(Arc::new(HspClockObservation {
            owner_id,
            generation,
            clock,
            valid_until_ns,
            session_until_ns,
            retired,
        })))
    }
    pub(crate) fn at(&self, now_ns: u64) -> Result<PlaybackClock> {
        if self.0.retired.load(Ordering::Acquire)
            || now_ns >= self.0.valid_until_ns
            || now_ns >= self.0.session_until_ns
        {
            return Err(invalid());
        }
        let mut clock = self.0.clock.at(now_ns)?;
        clock.valid_until_ns = clock.valid_until_ns.min(self.0.valid_until_ns);
        Ok(clock)
    }
    pub(crate) fn generation(&self) -> u64 {
        self.0.generation
    }
    pub(crate) fn owner_id(&self) -> &ArtifactId {
        &self.0.owner_id
    }
    pub(crate) fn valid_until_ns(&self) -> u64 {
        self.0.valid_until_ns
    }
    pub(crate) fn session_until_ns(&self) -> u64 {
        self.0.session_until_ns
    }
    pub(crate) fn native(&self) -> &NativePlaybackClock {
        &self.0.clock
    }
    pub(crate) fn belongs_to(&self, owner: &ArtifactId, retired: &Arc<AtomicBool>) -> bool {
        &self.0.owner_id == owner && Arc::ptr_eq(&self.0.retired, retired)
    }
    pub(crate) fn validate_facts(
        &self,
        now_ns: u64,
        facts: &crate::handy_hsp_runtime::HspDispatchFacts,
    ) -> Result<()> {
        let reference = facts.reference();
        if reference.at_ns() > now_ns
            || facts.binding().clock_domain() != &self.0.clock.server.clock.domain
        {
            return Err(invalid());
        }
        let observed = self.at(reference.at_ns())?;
        self.at(now_ns)?;
        let center = i128::from(reference.start_time_ms());
        let margin = |value: Option<i128>| {
            value
                .and_then(|n| u128::try_from(n).ok())
                .and_then(|n| n.checked_mul(1_000_000))
                .ok_or_else(invalid)
        };
        if reference.server_time_ms() != observed.estimated_server_ms
            || facts.window().clock_until_ns() > observed.valid_until_ns
            || margin(center.checked_sub(i128::from(reference.phase().earliest_ms)))?
                < u128::from(observed.uncertainty_ns)
            || margin(i128::from(reference.phase().latest_ms).checked_sub(center))?
                < u128::from(observed.uncertainty_ns)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handy_server_clock::{QualifiedServerClock, Sample};
    fn device(error: u64, budget: u64) -> NativeDeviceClock {
        NativeDeviceClock::new(
            HandyClockSyncAcknowledgment {
                info: HandyDeviceTimeInfo {
                    device_uptime_ms: 100,
                    reported_clock_offset_ms: -125,
                    last_sync_round_trip_ms: 10,
                },
                sent_boottime_ns: 100,
                received_boottime_ns: 110,
            },
            QualifiedDeviceClock {
                contract: HandyDeviceClockQualification {
                    model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
                    user_offset_ms: -50,
                    device_error_ns: error,
                    maximum_sync_age_ns: 2_000_000,
                    maximum_sync_round_trip_ms: 10,
                    drift_ppm: 20,
                },
                maximum_total_error_ns: budget,
            },
        )
        .unwrap()
    }
    fn pin_clock(sent: u64, server_ms: u64) -> Arc<NativePlaybackClock> {
        let mut device = device(1_000, 20_000_000);
        device.qualified.contract.maximum_sync_age_ns = 10_000_000_000;
        Arc::new(NativePlaybackClock {
            device,
            server: NativeServerClock::new(
                ArtifactId::new("pin-domain").unwrap(),
                Sample {
                    sent_ns: sent,
                    received_ns: sent + 1,
                    server_ms,
                },
                QualifiedServerClock {
                    contract: HandyServerClockQualification {
                        model: HandyServerClockModel::RequestBracketMonotonic,
                        timestamp_error_ns: 1_000_000,
                    },
                    max_age_ns: 1_000_000_000,
                    max_uncertainty_ns: 20_000_000,
                    drift_ppm: 0,
                },
            )
            .unwrap(),
        })
    }
    fn pin_facts(pin: &HspClockPin, at: u64) -> crate::handy_hsp_runtime::HspDispatchFacts {
        use crate::handy_hsp_runtime::*;
        let clock = pin.at(at).unwrap();
        let binding = HspBinding::new(
            HspStreamIdentity {
                generation: 1,
                stream_id: 42,
                live: LiveIdentity {
                    epoch: AttemptId::new("capture").unwrap(),
                    project: ProjectId::new("project").unwrap(),
                    revision: RevisionId::new(1),
                    source: SourceVersionId::new("source").unwrap(),
                },
            },
            ClientId::new("owner").unwrap(),
            DeviceSessionId::new("session").unwrap(),
            clock.domain.clone(),
        );
        HspDispatchFacts::new(
            binding,
            HspPlayReference::new(
                at,
                HspPhaseWindow {
                    earliest_ms: 0,
                    latest_ms: 1000,
                },
                500,
                clock.estimated_server_ms,
            )
            .unwrap(),
            HspAuthorityWindow::new(3_000_000_000, 3_000_000_000, pin.valid_until_ns()),
        )
        .unwrap()
    }
    #[test]
    fn hsp_clock_pin_preserves_original_facts_and_cannot_be_replaced_by_refresh() {
        let retired = Arc::new(AtomicBool::new(false));
        let id = ArtifactId::new("owner-pin").unwrap();
        let first = HspClockPin::new(
            id.clone(),
            1,
            pin_clock(1_000_000, 1000),
            900_000_000,
            3_000_000_000,
            retired.clone(),
        )
        .unwrap();
        let facts = pin_facts(&first, 10_000_000)
            .with_clock_pin(first.clone())
            .unwrap();
        assert_eq!(facts.clock_pin(), Some(&first));
        assert_eq!(facts.clone().clock_pin(), Some(&first));
        let later = HspClockPin::new(
            id,
            2,
            pin_clock(100_000_000, 1099),
            1_000_000_000,
            3_000_000_000,
            retired.clone(),
        )
        .unwrap();
        assert!(facts.clone().with_clock_pin(later.clone()).is_err());
        assert!(facts.valid_at(900_000_000).is_err());
        assert!(later.at(900_000_000).is_ok());
        assert_eq!(first.valid_until_ns(), 900_000_000);
        retired.store(true, Ordering::Release);
        assert!(first.at(10_000_000).is_err());
        assert!(later.at(100_000_001).is_err());
    }
    #[test]
    fn hsp_clock_pin_refuses_uncovered_phase_domain_and_original_sync_overflow() {
        use crate::handy_hsp_runtime::*;
        let pin = HspClockPin::new(
            ArtifactId::new("owner-pin").unwrap(),
            1,
            pin_clock(1_000_000, 1000),
            900_000_000,
            3_000_000_000,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let good = pin_facts(&pin, 10_000_000);
        for case in 0..3 {
            let reference = HspPlayReference::new(
                10_000_000,
                if case == 0 {
                    HspPhaseWindow {
                        earliest_ms: 500,
                        latest_ms: 500,
                    }
                } else {
                    good.reference().phase()
                },
                500,
                good.reference().server_time_ms() + u64::from(case == 1),
            )
            .unwrap();
            let window = if case == 2 {
                HspAuthorityWindow::new(3_000_000_000, 3_000_000_000, 900_000_001)
            } else {
                good.window().clone()
            };
            let changed = HspDispatchFacts::new(good.binding().clone(), reference, window).unwrap();
            assert!(changed.with_clock_pin(pin.clone()).is_err());
        }
        let mut clock = NativePlaybackClock {
            server: pin.native().server.clone(),
            device: pin.native().device.clone(),
        };
        clock.device.acknowledgment.sent_boottime_ns = u64::MAX;
        assert!(clock.synchronization_until_ns().is_err());
        assert!(HspClockPin::new(
            ArtifactId::new("too-long").unwrap(),
            1,
            pin_clock(1_000_000, 1000),
            900_000_000,
            11_000_000_000,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
    }
    #[test]
    fn handy_playback_clock_adds_independent_uncertainties_and_keeps_original_sync_age() {
        let server = NativeServerClock::new(
            ArtifactId::new("fixture").unwrap(),
            Sample {
                sent_ns: 120,
                received_ns: 123,
                server_ms: 1000,
            },
            QualifiedServerClock {
                contract: HandyServerClockQualification {
                    model: HandyServerClockModel::RequestBracketMonotonic,
                    timestamp_error_ns: 1000,
                },
                max_age_ns: 2_000_000,
                max_uncertainty_ns: 2_000_000,
                drift_ppm: 10,
            },
        )
        .unwrap();
        let c = NativePlaybackClock {
            server,
            device: device(2000, 2_000_000),
        };
        assert_eq!(c.at(123).unwrap().uncertainty_ns, 3005);
        assert_eq!(c.at(123).unwrap().valid_until_ns, 2_000_100);
        for age in [0, 1, 999_999, 1_000_000] {
            let now = 123 + age;
            let actual = c.at(now).unwrap();
            assert_eq!(actual.valid_until_ns, 2_000_100);
            assert_eq!(
                actual.uncertainty_ns,
                c.server.at(now).unwrap().uncertainty_ns + c.device.error_at(now).unwrap()
            );
        }
        assert!(c.at(2_000_101).is_err());
        assert!(c.device.error_at(109).is_err());
        assert!(c.device.error_at(2_000_100).is_ok());
    }
    #[test]
    fn handy_playback_clock_rejects_sum_even_when_each_component_fits_its_own_budget() {
        let server = NativeServerClock::new(
            ArtifactId::new("fixture").unwrap(),
            Sample {
                sent_ns: 110,
                received_ns: 110,
                server_ms: 1000,
            },
            QualifiedServerClock {
                contract: HandyServerClockQualification {
                    model: HandyServerClockModel::RequestBracketMonotonic,
                    timestamp_error_ns: 3000,
                },
                max_age_ns: 2_000_000,
                max_uncertainty_ns: 4000,
                drift_ppm: 10,
            },
        )
        .unwrap();
        let c = NativePlaybackClock {
            server,
            device: device(3000, 4000),
        };
        assert!(c.server.at(110).is_ok());
        assert!(c.device.error_at(110).is_ok());
        assert!(c.at(110).is_err());
    }
    #[test]
    fn handy_playback_clock_device_error_overflow_and_changed_observation_refuse() {
        let mut d = device(2000, 10_000);
        let good = HandyDeviceTimingSample {
            info: d.acknowledgment.info.clone(),
            user_offset: HandyUserTimeOffset { offset_ms: -50 },
            read_started_boottime_ns: 110,
            read_finished_boottime_ns: 111,
        };
        d.check_observation(&good).unwrap();
        for case in 0..4 {
            let mut bad = good.clone();
            match case {
                0 => bad.user_offset.offset_ms = 0,
                1 => bad.info.reported_clock_offset_ms += 1,
                2 => bad.info.device_uptime_ms = 99,
                _ => bad.info.last_sync_round_trip_ms = 9,
            };
            assert!(d.check_observation(&bad).is_err());
        }
        d.qualified.contract.device_error_ns = u64::MAX;
        d.qualified.maximum_total_error_ns = u64::MAX;
        assert!(d.error_at(111).is_err());
        let mut ack = d.acknowledgment.clone();
        ack.info.last_sync_round_trip_ms = 11;
        assert!(NativeDeviceClock::new(ack, d.qualified).is_err());
    }
}
