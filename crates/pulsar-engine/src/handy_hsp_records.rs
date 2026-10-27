//! Bounded HSP history, never restored dispatch authority. Only `capture` reads
//! sealed runtime effects; no DTO can construct a binding, effect or claim.
//! Digests detect identity changes relative to a retained receipt/database key;
//! they are not signatures, qualification decisions or native credential values.
use crate::handy_hsp_runtime::{
    HspBinding, HspDispatchFacts, HspEffect, HspEffectKind, HspObservedOutcome,
};
use crate::handy_hsp_wire::{
    HspEncodedRequest, HspPlayState, HspState, HspWirePoint, HSP_MAX_ADD_POINTS,
    HSP_MAX_SERVER_TIME_MS, HSP_MAX_TAIL_INDEX,
};
use crate::{handy_playback_clock::HspClockPin, playback_runtime::PlaybackClock};
use anyhow::{ensure, Result};
use pulsar_core::{
    ArtifactId, AttemptId, ClientId, DeviceId, DeviceSessionId, HspChunk, HspSample,
    HspSourceCursor, HspStreamIdentity, LiveIdentity, ProjectId, RequestId, SessionId,
    SourceVersionId,
};
use pulsar_protocol::{
    HandyClockSyncAcknowledgment, HandyDeviceClockQualification, HandyIdentity, HandyRegistration,
};
use serde::{de, Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, marker::PhantomData};

pub const HSP_STREAM_RECORD_LIMIT: usize = 8192;
pub const HSP_EFFECT_RECORD_LIMIT: usize = 65536;
pub const HSP_REQUEST_BODY_LIMIT: usize = 4096;

fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn hash<T: Serialize>(domain: &[u8], value: &T) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(serde_json::to_vec(value)?);
    Ok(format!("{:x}", digest.finalize()))
}
fn bounded_json<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= limit, "HSP history exceeds record limit");
    Ok(bytes)
}
pub(crate) fn bounded_vec<'de, D, T, const LIMIT: usize>(
    deserializer: D,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const N: usize>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> de::Visitor<'de> for Bounded<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "an array of at most {N} entries")
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|size| size > N) {
                return Err(de::Error::custom("HSP history array exceeds limit"));
            }
            let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(N));
            while values.len() < N {
                let Some(value) = seq.next_element()? else {
                    return Ok(values);
                };
                values.push(value);
            }
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("HSP history array exceeds limit"));
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Bounded::<T, LIMIT>(PhantomData))
}
fn body_bytes<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
    #[derive(Deserialize)]
    struct Bytes(
        #[serde(deserialize_with = "bounded_vec::<_, u8, HSP_REQUEST_BODY_LIMIT>")] Vec<u8>,
    );
    Ok(Option::<Bytes>::deserialize(d)?.map(|bytes| bytes.0))
}

/// Historical source identity; deliberately not the runtime's authority type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspStreamIdentityRecord {
    pub generation: u64,
    pub stream_id: u32,
    pub live: LiveIdentity,
}
impl HspStreamIdentityRecord {
    fn capture(value: &HspStreamIdentity) -> Self {
        Self {
            generation: value.generation,
            stream_id: value.stream_id,
            live: value.live.clone(),
        }
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.generation > 0 && self.stream_id > 0,
            "invalid HSP history identity"
        );
        AttemptId::new(self.live.epoch.as_str())?;
        ProjectId::new(self.live.project.as_str())?;
        SourceVersionId::new(self.live.source.as_str())?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspBindingRecord {
    pub identity: HspStreamIdentityRecord,
    pub owner: ClientId,
    pub session: DeviceSessionId,
    pub clock_domain: ArtifactId,
}
impl HspBindingRecord {
    pub fn capture(binding: &HspBinding) -> Self {
        Self {
            identity: HspStreamIdentityRecord::capture(binding.identity()),
            owner: binding.owner().clone(),
            session: binding.session().clone(),
            clock_domain: binding.clock_domain().clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.identity.validate()?;
        ClientId::new(self.owner.as_str())?;
        DeviceSessionId::new(self.session.as_str())?;
        ArtifactId::new(self.clock_domain.as_str())?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HspStreamRecord {
    pub boot: DeviceSessionId,
    pub actor_session: SessionId,
    pub request_id: RequestId,
    pub registration: HandyRegistration,
    pub device: HandyIdentity,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
    pub binding: HspBindingRecord,
    /// References to independently admitted evidence; no accepted proof is minted.
    pub qualification_sha256: String,
    pub profile_sha256: String,
}
impl<'de> Deserialize<'de> for HspStreamRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            boot: DeviceSessionId,
            actor_session: SessionId,
            request_id: RequestId,
            registration: HandyRegistration,
            device: HandyIdentity,
            route_sha256: String,
            route_namespace_sha256: String,
            binding: HspBindingRecord,
            qualification_sha256: String,
            profile_sha256: String,
        }
        let f = Fields::deserialize(deserializer)?;
        let value = Self {
            boot: f.boot,
            actor_session: f.actor_session,
            request_id: f.request_id,
            registration: f.registration,
            device: f.device,
            route_sha256: f.route_sha256,
            route_namespace_sha256: f.route_namespace_sha256,
            binding: f.binding,
            qualification_sha256: f.qualification_sha256,
            profile_sha256: f.profile_sha256,
        };
        value.validate().map_err(de::Error::custom)?;
        Ok(value)
    }
}
impl HspStreamRecord {
    pub fn validate(&self) -> Result<()> {
        DeviceSessionId::new(self.boot.as_str())?;
        SessionId::new(self.actor_session.as_str())?;
        RequestId::new(self.request_id.as_str())?;
        DeviceId::new(self.registration.device_id.as_str())?;
        self.registration.validate()?;
        self.device.validate()?;
        self.binding.validate()?;
        ensure!(
            !self.registration.disabled && self.boot != self.binding.session,
            "invalid HSP stream registration or boot binding"
        );
        for digest in [
            &self.route_sha256,
            &self.route_namespace_sha256,
            &self.qualification_sha256,
            &self.profile_sha256,
        ] {
            ensure!(digest_valid(digest), "invalid HSP history digest");
        }
        bounded_json(self, HSP_STREAM_RECORD_LIMIT)?;
        Ok(())
    }
    pub fn id(&self) -> Result<String> {
        self.validate()?;
        hash(b"pulsar.handy.hsp.stream.v1\0", self)
    }
    pub fn request_identity(&self) -> Result<String> {
        self.validate()?;
        hash(
            b"pulsar.handy.hsp.request.v1\0",
            &(&self.actor_session, &self.request_id),
        )
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= HSP_STREAM_RECORD_LIMIT,
            "HSP stream history exceeds record limit"
        );
        Ok(serde_json::from_slice(bytes)?)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bounded_json(self, HSP_STREAM_RECORD_LIMIT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HspEffectRecordKind {
    Setup,
    Append,
    Play,
    Stop,
}
impl From<HspEffectKind> for HspEffectRecordKind {
    fn from(kind: HspEffectKind) -> Self {
        match kind {
            HspEffectKind::Setup => Self::Setup,
            HspEffectKind::Append => Self::Append,
            HspEffectKind::Play => Self::Play,
            HspEffectKind::Stop => Self::Stop,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspRequestRecord {
    pub method: String,
    pub path: String,
    #[serde(deserialize_with = "body_bytes")]
    pub body: Option<Vec<u8>>,
}
impl HspRequestRecord {
    fn capture(request: &HspEncodedRequest) -> Self {
        Self {
            method: request.method().into(),
            path: request.path().into(),
            body: request.body().map(<[u8]>::to_vec),
        }
    }
}

/// Immutable source provenance, not a deserializable native consumer or grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspSourceCursorRecord {
    pub identity: LiveIdentity,
    pub attempt_id: AttemptId,
}
impl From<&HspSourceCursor> for HspSourceCursorRecord {
    fn from(source: &HspSourceCursor) -> Self {
        Self {
            identity: source.identity.clone(),
            attempt_id: source.attempt_id.clone(),
        }
    }
}
impl HspSourceCursorRecord {
    fn validate(&self, origin: &LiveIdentity) -> Result<()> {
        AttemptId::new(self.identity.epoch.as_str())?;
        AttemptId::new(self.attempt_id.as_str())?;
        ensure!(
            self.identity.project == origin.project
                && self.identity.revision == origin.revision
                && self.identity.source == origin.source,
            "HSP source cursor changed stable stream subject"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspClockMappingRecord {
    pub domain: ArtifactId,
    pub observed_at_ns: u64,
    pub valid_until_ns: u64,
    pub estimated_server_ms: u64,
    pub uncertainty_ns: u64,
}
impl From<&PlaybackClock> for HspClockMappingRecord {
    fn from(clock: &PlaybackClock) -> Self {
        Self {
            domain: clock.domain.clone(),
            observed_at_ns: clock.monotonic_ns,
            valid_until_ns: clock.valid_until_ns,
            estimated_server_ms: clock.estimated_server_ms,
            uncertainty_ns: clock.uncertainty_ns,
        }
    }
}
impl HspClockMappingRecord {
    fn validate(&self) -> Result<()> {
        ArtifactId::new(self.domain.as_str())?;
        ensure!(
            self.observed_at_ns < self.valid_until_ns
                && self.estimated_server_ms <= HSP_MAX_SERVER_TIME_MS,
            "invalid HSP retained clock mapping"
        );
        Ok(())
    }
}

/// Descriptive original native clock material. Reading this record never
/// reconstructs a pin, refreshes a clock, or accepts the recorded assumptions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspClockObservationRecord {
    pub owner_id: ArtifactId,
    pub generation: u64,
    pub valid_until_ns: u64,
    pub session_until_ns: u64,
    pub server_sent_at_ns: u64,
    pub original_server_mapping: HspClockMappingRecord,
    pub original_device_sync: HandyClockSyncAcknowledgment,
    pub device_clock: HandyDeviceClockQualification,
    pub maximum_total_error_ns: u64,
    pub reference_mapping: HspClockMappingRecord,
}
impl HspClockObservationRecord {
    fn capture(pin: &HspClockPin, at_ns: u64) -> Result<Self> {
        let native = pin.native();
        // Historical capture deliberately does not call pin.at: retirement
        // must not erase the immutable observation of an already-issued effect.
        let mut reference = native.at(at_ns)?;
        reference.valid_until_ns = reference.valid_until_ns.min(pin.valid_until_ns());
        Ok(Self {
            owner_id: pin.owner_id().clone(),
            generation: pin.generation(),
            valid_until_ns: pin.valid_until_ns(),
            session_until_ns: pin.session_until_ns(),
            server_sent_at_ns: native.server.sent_ns,
            original_server_mapping: HspClockMappingRecord::from(&native.server.clock),
            original_device_sync: native.device.acknowledgment.clone(),
            device_clock: native.device.qualified.contract.clone(),
            maximum_total_error_ns: native.device.qualified.maximum_total_error_ns,
            reference_mapping: HspClockMappingRecord::from(&reference),
        })
    }
    pub(crate) fn same_observation(&self, other: &Self) -> bool {
        self.owner_id == other.owner_id
            && self.generation == other.generation
            && self.valid_until_ns == other.valid_until_ns
            && self.session_until_ns == other.session_until_ns
            && self.server_sent_at_ns == other.server_sent_at_ns
            && self.original_server_mapping == other.original_server_mapping
            && self.original_device_sync == other.original_device_sync
            && self.device_clock == other.device_clock
            && self.maximum_total_error_ns == other.maximum_total_error_ns
    }
    fn validate(&self, facts: &HspDispatchFactsRecord) -> Result<()> {
        ArtifactId::new(self.owner_id.as_str())?;
        self.original_server_mapping.validate()?;
        self.reference_mapping.validate()?;
        self.original_device_sync.validate()?;
        self.device_clock.validate()?;
        let original = &self.original_server_mapping;
        let reference = &self.reference_mapping;
        let sync_until = self
            .original_device_sync
            .sent_boottime_ns
            .checked_add(self.device_clock.maximum_sync_age_ns)
            .ok_or_else(|| anyhow::anyhow!("HSP original synchronization ceiling overflow"))?;
        let elapsed = facts
            .at_ns
            .checked_sub(original.observed_at_ns)
            .ok_or_else(|| anyhow::anyhow!("HSP clock reference precedes original observation"))?;
        let server = original
            .estimated_server_ms
            .checked_add(elapsed / 1_000_000)
            .ok_or_else(|| anyhow::anyhow!("HSP original server mapping overflow"))?;
        ensure!(
            self.generation > 0
                && self.server_sent_at_ns <= original.observed_at_ns
                && self.original_device_sync.received_boottime_ns <= original.observed_at_ns
                && self.original_device_sync.info.last_sync_round_trip_ms
                    <= self.device_clock.maximum_sync_round_trip_ms
                && original.domain == facts.binding.clock_domain
                && reference.domain == original.domain
                && original.observed_at_ns < self.valid_until_ns
                && self.valid_until_ns <= self.session_until_ns
                && self.session_until_ns <= sync_until
                && self.valid_until_ns <= original.valid_until_ns
                && reference.observed_at_ns == facts.at_ns
                && reference.estimated_server_ms == server
                && reference.estimated_server_ms == facts.server_time_ms
                && reference.valid_until_ns
                    == original
                        .valid_until_ns
                        .min(sync_until)
                        .min(self.valid_until_ns)
                && facts.clock_until_ns <= reference.valid_until_ns
                && self.maximum_total_error_ns > 0
                && reference.uncertainty_ns >= original.uncertainty_ns
                && reference.uncertainty_ns <= self.maximum_total_error_ns,
            "HSP immutable clock observation or original ceiling differs"
        );
        let before = i128::from(facts.start_time_ms) - i128::from(facts.earliest_ms);
        let after = i128::from(facts.latest_ms) - i128::from(facts.start_time_ms);
        ensure!(
            before >= 0
                && after >= 0
                && before * 1_000_000 >= i128::from(reference.uncertainty_ns)
                && after * 1_000_000 >= i128::from(reference.uncertainty_ns),
            "HSP original clock error escapes phase bounds"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspDispatchFactsRecord {
    pub binding: HspBindingRecord,
    pub at_ns: u64,
    pub earliest_ms: i64,
    pub latest_ms: i64,
    pub start_time_ms: i32,
    pub server_time_ms: u64,
    pub lease_until_ns: u64,
    pub source_until_ns: u64,
    pub clock_until_ns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_observation: Option<HspClockObservationRecord>,
}
impl HspDispatchFactsRecord {
    fn capture(facts: &HspDispatchFacts) -> Result<Self> {
        Ok(Self {
            binding: HspBindingRecord::capture(facts.binding()),
            at_ns: facts.reference().at_ns(),
            earliest_ms: facts.reference().phase().earliest_ms,
            latest_ms: facts.reference().phase().latest_ms,
            start_time_ms: facts.reference().start_time_ms(),
            server_time_ms: facts.reference().server_time_ms(),
            lease_until_ns: facts.window().lease_until_ns(),
            source_until_ns: facts.window().source_until_ns(),
            clock_until_ns: facts.window().clock_until_ns(),
            clock_observation: facts
                .clock_pin()
                .map(|pin| HspClockObservationRecord::capture(pin, facts.reference().at_ns()))
                .transpose()?,
        })
    }
    fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        ensure!(
            self.earliest_ms >= i64::from(i32::MIN)
                && self.latest_ms <= i64::from(i32::MAX)
                && self.earliest_ms <= i64::from(self.start_time_ms)
                && i64::from(self.start_time_ms) <= self.latest_ms
                && self.server_time_ms > 0
                && self.server_time_ms <= HSP_MAX_SERVER_TIME_MS,
            "invalid HSP original clock snapshot"
        );
        ensure!(
            self.at_ns < self.lease_until_ns
                && self.at_ns < self.source_until_ns
                && self.at_ns < self.clock_until_ns,
            "invalid HSP original authority deadlines"
        );
        if let Some(clock) = &self.clock_observation {
            clock.validate(self)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspSampleRecord {
    pub time_ms: u32,
    pub position_percent: u8,
    pub source_sequence: u64,
    pub captured_at_ns: u64,
    pub accepted_at_ns: u64,
    pub continuous_from_previous: bool,
}
impl From<&HspSample> for HspSampleRecord {
    fn from(sample: &HspSample) -> Self {
        Self {
            time_ms: sample.point.time_ms,
            position_percent: sample.point.position_percent,
            source_sequence: sample.source_sequence,
            captured_at_ns: sample.captured_at_ns,
            accepted_at_ns: sample.accepted_at_ns,
            continuous_from_previous: sample.continuous_from_previous,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspChunkRecord {
    pub identity: HspStreamIdentityRecord,
    pub sequence: u64,
    pub tail_point_stream_index: u32,
    #[serde(deserialize_with = "bounded_vec::<_, HspSampleRecord, HSP_MAX_ADD_POINTS>")]
    pub samples: Vec<HspSampleRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cursor: Option<HspSourceCursorRecord>,
}
impl HspChunkRecord {
    fn capture(chunk: &HspChunk) -> Self {
        Self {
            identity: HspStreamIdentityRecord::capture(chunk.token().identity()),
            sequence: chunk.token().sequence(),
            tail_point_stream_index: chunk.token().tail_point_stream_index(),
            samples: chunk.samples().iter().map(HspSampleRecord::from).collect(),
            source_cursor: chunk.source_cursor().map(HspSourceCursorRecord::from),
        }
    }
    fn validate(&self, effect: &HspEffectRecord, facts: &HspDispatchFactsRecord) -> Result<()> {
        self.identity.validate()?;
        if let Some(source) = &self.source_cursor {
            source.validate(&self.identity.live)?;
            ensure!(
                self.sequence != 1 || source.identity == self.identity.live,
                "initial HSP chunk must name original source cursor"
            );
            ensure!(
                source.identity == self.identity.live || effect.expected_running,
                "successor source chunk requires the original playing stream"
            );
        }
        ensure!(
            self.identity == effect.binding.identity
                && self.sequence > 0
                && self.sequence < effect.sequence,
            "invalid HSP append token membership"
        );
        ensure!(
            !self.samples.is_empty()
                && self.samples.len() <= HSP_MAX_ADD_POINTS
                && self.tail_point_stream_index > 0
                && self.tail_point_stream_index <= HSP_MAX_TAIL_INDEX
                && u64::from(self.tail_point_stream_index) + 1 >= self.samples.len() as u64,
            "invalid HSP append bounds"
        );
        if self.sequence == 1 {
            ensure!(
                self.samples.len() >= 2
                    && self.tail_point_stream_index as usize + 1 == self.samples.len(),
                "invalid HSP initial append tail"
            );
        }
        for (index, sample) in self.samples.iter().enumerate() {
            ensure!(
                sample.position_percent <= 100
                    && sample.source_sequence > 0
                    && sample.captured_at_ns <= sample.accepted_at_ns
                    && sample.accepted_at_ns <= facts.at_ns,
                "invalid HSP original append sample"
            );
            ensure!(
                (self.sequence == 1 && index == 0)
                    || sample.continuous_from_previous
                    || (index == 0
                        && sample.source_sequence == 1
                        && self.source_cursor.as_ref().is_some_and(
                            |source| source.identity.epoch != self.identity.live.epoch
                        )),
                "invalid HSP source continuity snapshot"
            );
        }
        ensure!(
            self.samples
                .windows(2)
                .all(|pair| pair[0].time_ms < pair[1].time_ms
                    && pair[0].source_sequence < pair[1].source_sequence
                    && pair[0].captured_at_ns <= pair[1].captured_at_ns
                    && pair[0].accepted_at_ns <= pair[1].accepted_at_ns),
            "invalid HSP source ordering snapshot"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HspEffectRecord {
    pub stream_id: String,
    pub stream_sha256: String,
    pub sequence: u64,
    pub kind: HspEffectRecordKind,
    pub binding: HspBindingRecord,
    pub request: HspRequestRecord,
    pub original_facts: Option<HspDispatchFactsRecord>,
    pub chunk: Option<HspChunkRecord>,
    pub expected_running: bool,
}
impl<'de> Deserialize<'de> for HspEffectRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            stream_id: String,
            stream_sha256: String,
            sequence: u64,
            kind: HspEffectRecordKind,
            binding: HspBindingRecord,
            request: HspRequestRecord,
            original_facts: Option<HspDispatchFactsRecord>,
            chunk: Option<HspChunkRecord>,
            expected_running: bool,
        }
        let f = Fields::deserialize(deserializer)?;
        let value = Self {
            stream_id: f.stream_id,
            stream_sha256: f.stream_sha256,
            sequence: f.sequence,
            kind: f.kind,
            binding: f.binding,
            request: f.request,
            original_facts: f.original_facts,
            chunk: f.chunk,
            expected_running: f.expected_running,
        };
        value.validate_snapshot().map_err(de::Error::custom)?;
        Ok(value)
    }
}
impl HspEffectRecord {
    pub fn capture(stream_id: &str, stream: &HspStreamRecord, effect: &HspEffect) -> Result<Self> {
        let value = Self {
            stream_id: stream_id.into(),
            stream_sha256: stream.id()?,
            sequence: effect.sequence(),
            kind: effect.kind().into(),
            binding: HspBindingRecord::capture(effect.binding()),
            request: HspRequestRecord::capture(effect.request()),
            original_facts: effect
                .original_facts()
                .map(HspDispatchFactsRecord::capture)
                .transpose()?,
            chunk: effect.chunk().map(HspChunkRecord::capture),
            expected_running: effect.expected_running(),
        };
        value.validate(stream)?;
        Ok(value)
    }
    fn validate_snapshot(&self) -> Result<()> {
        ensure!(
            digest_valid(&self.stream_id)
                && self.stream_id == self.stream_sha256
                && self.sequence > 0,
            "invalid HSP effect history identity"
        );
        self.binding.validate()?;
        ensure!(
            self.request.method == "PUT"
                && self.request.path.len() <= 16
                && self
                    .request
                    .body
                    .as_ref()
                    .is_none_or(|body| body.len() <= HSP_REQUEST_BODY_LIMIT),
            "invalid HSP request snapshot"
        );
        if let Some(facts) = &self.original_facts {
            facts.validate()?;
            ensure!(facts.binding == self.binding, "foreign HSP original facts");
        }
        let canonical = match self.kind {
            HspEffectRecordKind::Stop => {
                ensure!(
                    self.original_facts.is_none() && self.chunk.is_none() && !self.expected_running,
                    "invalid HSP stop snapshot"
                );
                HspEncodedRequest::stop()
            }
            kind => {
                let facts = self
                    .original_facts
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing HSP original facts"))?;
                match kind {
                    HspEffectRecordKind::Setup => {
                        ensure!(
                            self.sequence == 1 && self.chunk.is_none() && !self.expected_running,
                            "invalid HSP setup snapshot"
                        );
                        HspEncodedRequest::setup(self.binding.identity.stream_id)
                    }
                    HspEffectRecordKind::Append => {
                        let chunk = self
                            .chunk
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("missing HSP append snapshot"))?;
                        chunk.validate(self, facts)?;
                        let points = chunk
                            .samples
                            .iter()
                            .map(|sample| {
                                HspWirePoint::new(sample.time_ms, sample.position_percent)
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(|_| anyhow::anyhow!("invalid HSP wire points"))?;
                        HspEncodedRequest::add(&points, chunk.tail_point_stream_index)
                    }
                    HspEffectRecordKind::Play => {
                        ensure!(
                            self.sequence > 2 && self.chunk.is_none() && self.expected_running,
                            "invalid HSP play snapshot"
                        );
                        HspEncodedRequest::play(facts.start_time_ms, facts.server_time_ms)
                    }
                    HspEffectRecordKind::Stop => unreachable!(),
                }
                .map_err(|_| anyhow::anyhow!("invalid HSP canonical request"))?
            }
        };
        ensure!(
            self.request == HspRequestRecord::capture(&canonical),
            "HSP wire bytes differ from original snapshot"
        );
        bounded_json(self, HSP_EFFECT_RECORD_LIMIT)?;
        Ok(())
    }
    pub fn validate(&self, stream: &HspStreamRecord) -> Result<()> {
        self.validate_snapshot()?;
        ensure!(
            self.stream_sha256 == stream.id()? && self.binding == stream.binding,
            "foreign HSP stream history"
        );
        Ok(())
    }
    pub(crate) fn requires_provenance_schema(&self) -> bool {
        self.chunk
            .as_ref()
            .is_some_and(|chunk| chunk.source_cursor.is_some())
            || self
                .original_facts
                .as_ref()
                .is_some_and(|facts| facts.clock_observation.is_some())
    }
    pub fn id(&self) -> Result<String> {
        self.validate_snapshot()?;
        hash(b"pulsar.handy.hsp.effect.v1\0", self)
    }
    pub fn matches_effect(&self, stream: &HspStreamRecord, effect: &HspEffect) -> Result<()> {
        ensure!(
            self == &Self::capture(&self.stream_id, stream, effect)?,
            "HSP record does not match sealed effect"
        );
        Ok(())
    }
    pub fn from_json(bytes: &[u8], stream: &HspStreamRecord) -> Result<Self> {
        ensure!(
            bytes.len() <= HSP_EFFECT_RECORD_LIMIT,
            "HSP effect history exceeds record limit"
        );
        let value: Self = serde_json::from_slice(bytes)?;
        value.validate(stream)?;
        Ok(value)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate_snapshot()?;
        bounded_json(self, HSP_EFFECT_RECORD_LIMIT)
    }
}

/// Sanitized transport facts. No response strings or peer-provided credentials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspStateRecord {
    pub play_state: u8,
    pub points: u32,
    pub max_points: u32,
    pub current_point: i32,
    pub current_time: i32,
    pub looping: bool,
    pub playback_rate: f64,
    pub first_point_time: u32,
    pub last_point_time: u32,
    pub stream_id: u32,
    pub tail_point_stream_index: i32,
    pub tail_point_stream_index_threshold: u32,
    pub pause_on_starving: bool,
}
impl HspStateRecord {
    fn capture(state: &HspState) -> Self {
        Self {
            play_state: match state.play_state {
                HspPlayState::NotInitialized => 0,
                HspPlayState::Playing => 1,
                HspPlayState::Stopped => 2,
                HspPlayState::Paused => 3,
                HspPlayState::Starving => 4,
            },
            points: state.points,
            max_points: state.max_points,
            current_point: state.current_point,
            current_time: state.current_time,
            looping: state.looping,
            playback_rate: state.playback_rate,
            first_point_time: state.first_point_time,
            last_point_time: state.last_point_time,
            stream_id: state.stream_id,
            tail_point_stream_index: state.tail_point_stream_index,
            tail_point_stream_index_threshold: state.tail_point_stream_index_threshold,
            pause_on_starving: state.pause_on_starving,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.play_state <= 4
                && self.current_point >= -1
                && self.tail_point_stream_index >= -1
                && self.playback_rate.is_finite(),
            "invalid HSP transport state history"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HspOutcomeRecord {
    Unknown,
    Acknowledged(HspStateRecord),
}
impl<'de> Deserialize<'de> for HspOutcomeRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        enum Fields {
            Unknown,
            Acknowledged(HspStateRecord),
        }
        let value = match Fields::deserialize(deserializer)? {
            Fields::Unknown => Self::Unknown,
            Fields::Acknowledged(state) => Self::Acknowledged(state),
        };
        value.validate().map_err(de::Error::custom)?;
        Ok(value)
    }
}
impl HspOutcomeRecord {
    pub fn capture(outcome: &HspObservedOutcome) -> Result<Self> {
        let value = match outcome {
            HspObservedOutcome::Unknown => Self::Unknown,
            HspObservedOutcome::Acknowledged(state) => {
                Self::Acknowledged(HspStateRecord::capture(state))
            }
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if let Self::Acknowledged(state) = self {
            state.validate()?;
        }
        Ok(())
    }
    /// Checks the correspondence present in an effect. The journal additionally
    /// checks Play's tail against preceding acknowledged append history. Runtime
    /// admission/completion remains responsible for referenced policy capacity.
    pub fn validate_for_effect(&self, effect: &HspEffectRecord) -> Result<()> {
        self.validate()?;
        effect.validate_snapshot()?;
        let Self::Acknowledged(state) = self else {
            return Ok(());
        };
        ensure!(
            state.stream_id == effect.binding.identity.stream_id,
            "foreign HSP outcome stream"
        );
        if effect.kind == HspEffectRecordKind::Stop {
            ensure!(
                state.play_state == 2,
                "HSP stop is not acknowledged stopped"
            );
            return Ok(());
        }
        ensure!(
            state.points <= state.max_points,
            "invalid HSP observed capacity"
        );
        if effect.kind == HspEffectRecordKind::Play
            || (effect.kind == HspEffectRecordKind::Append && effect.expected_running)
        {
            ensure!(
                !state.looping && state.playback_rate == 1.0 && !state.pause_on_starving,
                "HSP outcome differs from fixed play options"
            );
        }
        let matches = match effect.kind {
            HspEffectRecordKind::Setup => matches!(state.play_state, 0 | 2) && state.points == 0,
            HspEffectRecordKind::Append => effect.chunk.as_ref().is_some_and(|chunk| {
                state.tail_point_stream_index == chunk.tail_point_stream_index as i32
                    && matches!(state.play_state, 1 | 2)
                    && (state.play_state == 1) == effect.expected_running
            }),
            HspEffectRecordKind::Play => {
                state.play_state == 1 && state.tail_point_stream_index >= 1
            }
            HspEffectRecordKind::Stop => unreachable!(),
        };
        ensure!(matches, "HSP outcome does not match effect");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handy_hsp_runtime::{
        HandyHspRuntime, HspAuthorityWindow, HspPlayReference, HspSettledTransport,
    };
    use pulsar_core::{HspPhaseWindow, HspPoint, HspPolicy, RevisionId};
    use pulsar_protocol::HandyRegistrationSettings;

    fn fixture() -> (HspStreamRecord, HandyHspRuntime) {
        let binding = HspBinding::new(
            HspStreamIdentity {
                generation: 5,
                stream_id: 42,
                live: LiveIdentity {
                    epoch: AttemptId::new("epoch").unwrap(),
                    project: ProjectId::new("project").unwrap(),
                    revision: RevisionId::new(7),
                    source: SourceVersionId::new("source").unwrap(),
                },
            },
            ClientId::new("owner").unwrap(),
            DeviceSessionId::new("device-session").unwrap(),
            ArtifactId::new("suspend-aware-domain").unwrap(),
        );
        let record = HspStreamRecord {
            boot: DeviceSessionId::new("boot").unwrap(),
            actor_session: SessionId::new("actor-session").unwrap(),
            request_id: RequestId::new("request").unwrap(),
            registration: HandyRegistration {
                device_id: DeviceId::new("device").unwrap(),
                revision: 1,
                settings: HandyRegistrationSettings {
                    label: "Synthetic transport fixture".into(),
                    api_key_env: "HANDY_API_KEY".into(),
                    connection_key_env: "HANDY_CONNECTION_KEY".into(),
                },
                disabled: false,
                binding_sha256: "a".repeat(64),
            },
            device: HandyIdentity {
                firmware: "4.0".into(),
                model_number: 2,
                model_name: "Handy".into(),
                model_variant: 0,
                session_id: "native-session".into(),
                firmware_status: 0,
            },
            route_sha256: "b".repeat(64),
            route_namespace_sha256: "c".repeat(64),
            binding: HspBindingRecord::capture(&binding),
            qualification_sha256: "d".repeat(64),
            profile_sha256: "e".repeat(64),
        };
        let runtime = HandyHspRuntime::new(
            binding,
            HspPolicy {
                max_buffered_points: 8,
                device_capacity_points: 8,
                max_chunk_points: 2,
                min_lead_ms: 10,
                max_future_ms: 1000,
                max_sample_age_ns: 1000,
                max_gap_ms: 200,
            },
        )
        .unwrap();
        (record, runtime)
    }
    fn facts(binding: &HspBinding, now: u64, phase: i32, until: u64) -> HspDispatchFacts {
        HspDispatchFacts::new(
            binding.clone(),
            HspPlayReference::new(
                now,
                HspPhaseWindow {
                    earliest_ms: i64::from(phase),
                    latest_ms: i64::from(phase),
                },
                phase,
                10000 + now,
            )
            .unwrap(),
            HspAuthorityWindow::new(until, until - 1, until - 2),
        )
        .unwrap()
    }
    fn initial(runtime: &mut HandyHspRuntime) -> (HspEffect, HspDispatchFacts) {
        // The binding is read from the issued setup; independently owned binding
        // below comes only from this trusted native fixture, never deserialization.
        let binding = HspBinding::new(
            runtime.stream().identity().clone(),
            ClientId::new("owner").unwrap(),
            DeviceSessionId::new("device-session").unwrap(),
            ArtifactId::new("suspend-aware-domain").unwrap(),
        );
        let facts = facts(&binding, 100, -100, 1000);
        let effect = runtime.next_effect(100, Some(&facts)).unwrap().unwrap();
        (effect, facts)
    }
    fn complete(
        runtime: &mut HandyHspRuntime,
        effect: &HspEffect,
        tail: i32,
        state: u8,
        facts: Option<&HspDispatchFacts>,
    ) {
        let body = serde_json::to_vec(&serde_json::json!({"result":{
            "play_state":state,"points":(tail+1).max(0),"max_points":8,"current_point":-1,
            "current_time":-100,"loop":false,"playback_rate":1.0,"first_point_time":0,
            "last_point_time":tail.max(0)*100,"stream_id":42,"tail_point_stream_index":tail,
            "tail_point_stream_index_threshold":1,"pause_on_starving":false
        }}))
        .unwrap();
        let now = facts.map_or(5000, |f| f.reference().at_ns());
        let record = runtime
            .complete_settled(
                effect,
                HspSettledTransport::Response {
                    status: 200,
                    body: &body,
                },
                now,
                facts,
            )
            .unwrap();
        assert!(matches!(
            record.outcome,
            HspObservedOutcome::Acknowledged(_)
        ));
    }
    fn append(
        runtime: &mut HandyHspRuntime,
        facts: &HspDispatchFacts,
        times: &[u32],
        sequence: u64,
    ) -> HspEffect {
        for (index, time) in times.iter().enumerate() {
            runtime
                .offer(
                    &facts.binding().identity().live,
                    HspSample {
                        point: HspPoint::new(*time, (index * 100) as u8).unwrap(),
                        source_sequence: sequence + index as u64 * 2,
                        captured_at_ns: facts.reference().at_ns() - 20 + index as u64,
                        accepted_at_ns: facts.reference().at_ns() - 10 + index as u64,
                        continuous_from_previous: sequence > 1 || index > 0,
                    },
                    facts.reference().at_ns(),
                    facts,
                )
                .unwrap();
        }
        runtime
            .next_effect(facts.reference().at_ns(), Some(facts))
            .unwrap()
            .unwrap()
    }
    fn roundtrip(stream: &HspStreamRecord, effect: &HspEffect) -> HspEffectRecord {
        let record = HspEffectRecord::capture(&stream.id().unwrap(), stream, effect).unwrap();
        let bytes = record.to_json().unwrap();
        let restored = HspEffectRecord::from_json(&bytes, stream).unwrap();
        assert_eq!(record, restored);
        assert_eq!(record.id().unwrap(), restored.id().unwrap());
        restored.matches_effect(stream, effect).unwrap();
        assert_eq!(restored.request.body.as_deref(), effect.request().body());
        restored
    }
    #[test]
    fn all_operations_roundtrip_exact_bytes_tokens_and_original_timing() {
        let (stream, mut runtime) = fixture();
        assert_eq!(
            stream,
            HspStreamRecord::from_json(&stream.to_json().unwrap()).unwrap()
        );
        let (setup, original) = initial(&mut runtime);
        let setup_record = roundtrip(&stream, &setup);
        assert_eq!(
            setup_record
                .original_facts
                .as_ref()
                .unwrap()
                .source_until_ns,
            999
        );
        complete(&mut runtime, &setup, -1, 2, Some(&original));
        let add = append(&mut runtime, &original, &[0, 100], 1);
        let record = roundtrip(&stream, &add);
        let chunk = record.chunk.as_ref().unwrap();
        assert_eq!(chunk.sequence, add.chunk().unwrap().token().sequence());
        assert_eq!(chunk.samples[0].captured_at_ns, 80);
        assert_eq!(chunk.samples[1].accepted_at_ns, 91);
        assert_eq!(chunk.samples[1].source_sequence, 3);
        assert!(!chunk.samples[0].continuous_from_previous);
        complete(&mut runtime, &add, 1, 2, Some(&original));
        runtime.request_play(100, &original).unwrap();
        let newer = facts(setup.binding(), 200, -100, 2000);
        let play = runtime.next_effect(200, Some(&newer)).unwrap().unwrap();
        let play_record = roundtrip(&stream, &play);
        assert_eq!(play_record.original_facts.as_ref().unwrap().at_ns, 100);
        assert_eq!(
            play_record.original_facts.as_ref().unwrap().source_until_ns,
            999
        );
        assert_eq!(play_record.request.body.as_deref(),Some(br#"{"start_time":-100,"server_time":10100,"playback_rate":1.0,"pause_on_starving":false,"loop":false}"#.as_slice()));
        complete(&mut runtime, &play, 1, 1, Some(&newer));
        let add2 = append(&mut runtime, &newer, &[200, 300], 5);
        let add2_record = roundtrip(&stream, &add2);
        assert_eq!(add2_record.chunk.as_ref().unwrap().sequence, 2);
        assert_eq!(
            add2_record.chunk.as_ref().unwrap().tail_point_stream_index,
            3
        );
        complete(&mut runtime, &add2, 3, 1, Some(&newer));
        runtime.stop();
        let stop = runtime.next_effect(5000, None).unwrap().unwrap();
        let stop_record = roundtrip(&stream, &stop);
        assert!(stop_record.original_facts.is_none());
        assert!(stop_record.chunk.is_none());
        assert_eq!(stop_record.request.body, None);
    }
    #[test]
    fn canonical_bytes_foreign_membership_and_original_facts_tampering_are_rejected() {
        let (stream, mut runtime) = fixture();
        let (setup, original) = initial(&mut runtime);
        complete(&mut runtime, &setup, -1, 2, Some(&original));
        let add = append(&mut runtime, &original, &[0, 100], 1);
        let record = roundtrip(&stream, &add);
        let mut changed = record.clone();
        changed.request.body.as_mut().unwrap().push(b' ');
        assert!(changed.validate(&stream).is_err());
        assert!(
            serde_json::from_slice::<HspEffectRecord>(&serde_json::to_vec(&changed).unwrap())
                .is_err()
        );
        let mut changed = record.clone();
        changed.chunk.as_mut().unwrap().samples[0].time_ms = 1;
        assert!(changed.validate(&stream).is_err());
        let mut changed = record.clone();
        changed.original_facts.as_mut().unwrap().source_until_ns = 2000;
        changed.validate(&stream).unwrap(); // Valid history shape is not authenticity.
        assert_ne!(changed.id().unwrap(), record.id().unwrap());
        assert!(changed.matches_effect(&stream, &add).is_err());
        let mut changed = record.clone();
        changed.chunk.as_mut().unwrap().samples[0].captured_at_ns = 79;
        assert_ne!(changed.id().unwrap(), record.id().unwrap());
        assert!(changed.matches_effect(&stream, &add).is_err());
        let mut changed = record.clone();
        changed.chunk.as_mut().unwrap().identity.generation += 1;
        assert!(changed.validate(&stream).is_err());
        let mut other = stream.clone();
        other.binding.identity.live.source = SourceVersionId::new("replacement").unwrap();
        assert!(record.validate(&other).is_err());
        assert!(HspEffectRecord::capture(&stream.id().unwrap(), &other, &add).is_err());
    }
    #[test]
    fn strict_history_refuses_unknown_fields_invalid_registration_and_bounded_inputs() {
        let (stream, mut runtime) = fixture();
        let (setup, _) = initial(&mut runtime);
        let record = roundtrip(&stream, &setup);
        let mut value = serde_json::to_value(&stream).unwrap();
        value["native_api_key"] = serde_json::json!("forbidden");
        assert!(HspStreamRecord::from_json(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = serde_json::to_value(&record).unwrap();
        value["binding"]["native_claim"] = serde_json::json!(true);
        assert!(HspEffectRecord::from_json(&serde_json::to_vec(&value).unwrap(), &stream).is_err());
        let mut changed = stream.clone();
        changed.registration.settings.api_key_env = "native credential value with spaces".into();
        assert!(changed.validate().is_err());
        let mut changed = stream.clone();
        changed.registration.disabled = true;
        assert!(changed.validate().is_err());
        assert!(HspStreamRecord::from_json(&vec![b' '; HSP_STREAM_RECORD_LIMIT + 1]).is_err());
        assert!(
            HspEffectRecord::from_json(&vec![b' '; HSP_EFFECT_RECORD_LIMIT + 1], &stream).is_err()
        );
        let mut value = serde_json::to_value(&record).unwrap();
        value["request"]["body"] = serde_json::json!(vec![0u8; HSP_REQUEST_BODY_LIMIT + 1]);
        assert!(serde_json::from_value::<HspEffectRecord>(value).is_err());
        let mut changed = stream.clone();
        changed.profile_sha256 = "F".repeat(64);
        assert!(changed.validate().is_err());
        let mut changed = stream.clone();
        changed.device.session_id = "native session with spaces".into();
        assert!(changed.validate().is_err());
    }
    #[test]
    fn sample_count_and_order_cannot_hide_in_valid_wire_bytes() {
        let (stream, mut runtime) = fixture();
        let (setup, original) = initial(&mut runtime);
        complete(&mut runtime, &setup, -1, 2, Some(&original));
        let add = append(&mut runtime, &original, &[0, 100], 1);
        let record = roundtrip(&stream, &add);
        for field in ["source_sequence", "captured_at_ns", "accepted_at_ns"] {
            let mut value = serde_json::to_value(&record).unwrap();
            value["chunk"]["samples"][1][field] = serde_json::json!(0);
            assert!(
                serde_json::from_value::<HspEffectRecord>(value).is_err(),
                "{field}"
            );
        }
        let mut value = serde_json::to_value(&record).unwrap();
        value["chunk"]["samples"] =
            serde_json::json!(vec![
                record.chunk.as_ref().unwrap().samples[0].clone();
                HSP_MAX_ADD_POINTS + 1
            ]);
        assert!(serde_json::from_value::<HspEffectRecord>(value).is_err());
    }
    #[test]
    fn actor_request_identity_is_distinct_from_content_and_changed_record_identity() {
        let (stream, _) = fixture();
        let mut changed = stream.clone();
        changed.profile_sha256 = "f".repeat(64);
        assert_eq!(
            stream.request_identity().unwrap(),
            changed.request_identity().unwrap()
        );
        assert_ne!(stream.id().unwrap(), changed.id().unwrap());
        let mut replacement = stream.clone();
        replacement.device.session_id = "replacement-session".into();
        assert_ne!(stream.id().unwrap(), replacement.id().unwrap());
        changed.actor_session = SessionId::new("other-actor").unwrap();
        assert_ne!(
            stream.request_identity().unwrap(),
            changed.request_identity().unwrap()
        );
        assert_ne!(stream.request_identity().unwrap(), stream.id().unwrap());
    }
    #[test]
    fn outcome_snapshot_rejects_foreign_stream_and_noncorresponding_stop() {
        let (stream, mut runtime) = fixture();
        let (setup, facts) = initial(&mut runtime);
        let record = roundtrip(&stream, &setup);
        let state = HspState {
            play_state: HspPlayState::Stopped,
            points: 0,
            max_points: 8,
            current_point: -1,
            current_time: -100,
            looping: false,
            playback_rate: 1.0,
            first_point_time: 0,
            last_point_time: 0,
            stream_id: 42,
            tail_point_stream_index: -1,
            tail_point_stream_index_threshold: 1,
            pause_on_starving: false,
        };
        let good = HspOutcomeRecord::capture(&HspObservedOutcome::Acknowledged(state)).unwrap();
        good.validate_for_effect(&record).unwrap();
        let mut foreign = good.clone();
        let HspOutcomeRecord::Acknowledged(ref mut state) = foreign else {
            unreachable!()
        };
        state.stream_id = 43;
        assert!(foreign.validate_for_effect(&record).is_err());
        let mut invalid = good.clone();
        let HspOutcomeRecord::Acknowledged(ref mut state) = invalid else {
            unreachable!()
        };
        state.playback_rate = f64::NAN;
        assert!(invalid.validate().is_err());
        HspOutcomeRecord::Unknown
            .validate_for_effect(&record)
            .unwrap();
        let mut value = serde_json::to_value(&good).unwrap();
        value["acknowledged"]["play_state"] = serde_json::json!(5);
        assert!(serde_json::from_value::<HspOutcomeRecord>(value).is_err());
        complete(&mut runtime, &setup, -1, 2, Some(&facts));
        runtime.stop();
        let stop = runtime.next_effect(5000, None).unwrap().unwrap();
        let stop_record = roundtrip(&stream, &stop);
        good.validate_for_effect(&stop_record).unwrap();
        let mut playing = good;
        let HspOutcomeRecord::Acknowledged(ref mut state) = playing else {
            unreachable!()
        };
        state.play_state = 1;
        assert!(playing.validate_for_effect(&stop_record).is_err());
    }
}

/// Immutable link captured at the original native admission. Missing links on
/// historical streams cannot be inferred from digests during recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspQualificationLinkRecord {
    pub stream_id: String,
    pub qualification: pulsar_protocol::HandyQualificationBinding,
    pub subject_sha256: String,
    pub profile_sha256: String,
    pub admission_sha256: String,
}
impl HspQualificationLinkRecord {
    pub fn validate(&self, stream: &HspStreamRecord) -> Result<()> {
        self.qualification.validate()?;
        for value in [
            &self.stream_id,
            &self.subject_sha256,
            &self.profile_sha256,
            &self.admission_sha256,
        ] {
            ensure!(digest_valid(value), "invalid HSP admission link digest");
        }
        ensure!(
            self.stream_id == stream.id()?
                && self.qualification.dossier_id.as_str() == stream.qualification_sha256
                && self.profile_sha256 == stream.profile_sha256,
            "HSP qualification link differs from original admission"
        );
        bounded_json(self, 4096)?;
        Ok(())
    }
    pub fn id(&self) -> Result<String> {
        hash(b"pulsar.handy.hsp.qualification-link.v1\0", self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HspEmergencyProvenance {
    NativeNoPrewrite,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HspEmergencyRecord {
    pub stream_id: String,
    pub effect: HspEffectRecord,
    pub outcome: HspOutcomeRecord,
    pub late_observation: Option<HspOutcomeRecord>,
    pub registration: HandyRegistration,
    pub device: HandyIdentity,
    pub mode: pulsar_protocol::HandyDeviceMode,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
    pub provenance: HspEmergencyProvenance,
}
impl HspEmergencyRecord {
    pub fn capture(
        stream: &HspStreamRecord,
        observation: &crate::handy_hsp_dispatch::HspEmergencyStopObservation,
    ) -> Result<Self> {
        let record = observation.record();
        let context = observation.context();
        let value = Self {
            stream_id: stream.id()?,
            effect: HspEffectRecord::capture(&stream.id()?, stream, &record.effect)?,
            outcome: HspOutcomeRecord::capture(&record.outcome)?,
            late_observation: record
                .late_observation
                .as_ref()
                .map(HspOutcomeRecord::capture)
                .transpose()?,
            registration: context.registration().clone(),
            device: context.device().clone(),
            mode: context.mode().clone(),
            route_sha256: context.route_sha256().into(),
            route_namespace_sha256: context.route_namespace_sha256().into(),
            provenance: HspEmergencyProvenance::NativeNoPrewrite,
        };
        value.validate(stream)?;
        Ok(value)
    }
    pub fn validate(&self, stream: &HspStreamRecord) -> Result<()> {
        self.effect.validate(stream)?;
        self.outcome.validate_for_effect(&self.effect)?;
        self.mode.validate()?;
        ensure!(
            self.stream_id == stream.id()?
                && self.effect.stream_id == self.stream_id
                && self.effect.kind == HspEffectRecordKind::Stop
                && self.registration == stream.registration
                && self.device == stream.device
                && self.route_sha256 == stream.route_sha256
                && self.route_namespace_sha256 == stream.route_namespace_sha256
                && self.mode.mode == 4,
            "emergency HSP original native context differs"
        );
        if let Some(late) = &self.late_observation {
            ensure!(
                self.outcome == HspOutcomeRecord::Unknown,
                "HSP emergency late observation replaces first outcome"
            );
            late.validate_for_effect(&self.effect)?;
        }
        bounded_json(self, 16384)?;
        Ok(())
    }
    pub fn id(&self) -> Result<String> {
        hash(b"pulsar.handy.hsp.emergency.v1\0", self)
    }
}
