//! Additive modality, review, and diagnostics contracts. Input declarations do
//! not authorize source reads, approve candidates, or qualify inference quality.

use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_PROMPT_BYTES: usize = 4096;
pub const MAX_REVIEW_FLAGS: usize = 1024;
pub const MAX_DIAGNOSTIC_ISSUES: usize = 1024;

/// Audio is decoded from the immutable video container unless a separately
/// captured source and explicit clock correspondence are declared.
/// Selection retains original rational clocks rather than zero-based indices.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixedAudioSettings {
    pub intent: MultimodalIntent,
    pub mapping: AudioMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent: Option<IndependentAudio>,
}

/// Producer-declared clock correspondence; not measured alignment.
/// audio_time = audio_anchor + rate * (video_time - video_anchor).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndependentAudio {
    pub source_version: SourceVersionId,
    pub alignment: AudioAlignment,
}

/// Exact affine correspondence between original source clocks. A missing rate
/// preserves the historical unit-rate mapping and serialized representation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioAlignment {
    pub video_anchor: SourceTimestamp,
    pub audio_anchor: SourceTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<AudioClockRate>,
}

/// Positive audio seconds per video second. This declaration does not measure
/// drift, change the visual lattice, or repair discontinuous audio timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AudioClockRateWire")]
pub struct AudioClockRate {
    numerator: i64,
    denominator: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioClockRateWire {
    numerator: i64,
    denominator: u32,
}

impl TryFrom<AudioClockRateWire> for AudioClockRate {
    type Error = ProtocolError;
    fn try_from(value: AudioClockRateWire) -> Result<Self, Self::Error> {
        Self::new(value.numerator, value.denominator)
    }
}

impl AudioClockRate {
    pub fn new(numerator: i64, denominator: u32) -> Result<Self, ProtocolError> {
        if numerator <= 0 || denominator == 0 {
            return Err(ProtocolError::invalid(
                "audio clock rate must be positive with a nonzero denominator",
            ));
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }
    pub fn numerator(self) -> i64 {
        self.numerator
    }
    pub fn denominator(self) -> u32 {
        self.denominator
    }
}

impl AudioAlignment {
    pub fn map(&self, video: ExactSeconds) -> Result<ExactSeconds, ProtocolError> {
        let rate = self.rate.unwrap_or(AudioClockRate {
            numerator: 1,
            denominator: 1,
        });
        video
            .checked_sub(ExactSeconds::from_source(self.video_anchor))
            .and_then(|elapsed| elapsed.checked_ratio(rate.numerator, rate.denominator))
            .and_then(|elapsed| elapsed.checked_add(ExactSeconds::from_source(self.audio_anchor)))
            .map_err(|error| ProtocolError::invalid(error.to_string()))
    }
}

/// Two captured, already-decoded 64x64 RGB frames in one immutable artifact.
/// Source clocks/epoch are producer declarations, not engine capture evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawFramePair {
    pub source_epoch: AttemptId,
    pub previous_frame: FrameId,
    pub current_frame: FrameId,
    pub previous_time: SourceTimestamp,
    pub current_time: SourceTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_clock: Option<LivePresentationClock>,
}
impl RawFramePair {
    pub fn presentation_mapping(
        &self,
    ) -> Result<Option<[LivePresentationMapping; 2]>, ProtocolError> {
        self.elapsed()?;
        let Some(clock) = &self.presentation_clock else {
            return Ok(None);
        };
        if clock.epoch != self.source_epoch {
            return Err(ProtocolError::invalid(
                "presentation clock differs from captured source epoch",
            ));
        }
        let previous = clock
            .map(self.previous_time)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        let current = clock
            .map(self.current_time)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if previous.project.time < ProjectTime::ZERO
            || current.project.time <= previous.project.time
            || current.presentation_tick <= previous.presentation_tick
        {
            return Err(ProtocolError::invalid(
                "presentation clock loses valid ordered project or presentation times",
            ));
        }
        Ok(Some([previous, current]))
    }
    pub fn elapsed(&self) -> Result<ExactTimeQuantization, ProtocolError> {
        if self.current_frame <= self.previous_frame {
            return Err(ProtocolError::invalid(
                "raw frame pair IDs must strictly increase",
            ));
        }
        let delta = ExactSeconds::from_source(self.current_time)
            .checked_sub(ExactSeconds::from_source(self.previous_time))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if delta.numerator() <= 0
            || delta
                .checked_cmp(ExactSeconds::new(1, 2).expect("constant"))
                .map_err(|e| ProtocolError::invalid(e.to_string()))?
                == std::cmp::Ordering::Greater
        {
            return Err(ProtocolError::invalid(
                "raw frame pair requires an increasing source-clock interval at most 500ms",
            ));
        }
        let time = delta
            .quantize_nanoseconds()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if time.time <= ProjectTime::ZERO {
            return Err(ProtocolError::invalid(
                "raw frame pair loses project timestamp precision",
            ));
        }
        Ok(time)
    }
}

/// Explicit synthesized audio mapping for one neutral axis. No physical inference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioChannelRecipe {
    pub mapping: AudioMapping,
    pub mode: AudioMode,
    pub analysis: AudioAnalysisOptions,
}

/// Checked six-axis declaration shared by every audio-bundle consumer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "AudioBundleRecipeWire", into = "AudioBundleRecipeWire")]
pub struct AudioBundleRecipe {
    channels: Vec<AudioChannelRecipe>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioBundleRecipeWire {
    channels: Vec<AudioChannelRecipe>,
}
impl AudioBundleRecipe {
    pub fn new(channels: Vec<AudioChannelRecipe>) -> Result<Self, ProtocolError> {
        let axes: BTreeSet<_> = channels
            .iter()
            .map(|channel| channel.mapping.axis())
            .collect();
        if channels.len() != Axis::ALL.len() || axes.len() != Axis::ALL.len() {
            return Err(ProtocolError::invalid("audio bundle requires exactly one explicit recipe for each of the six neutral axes"));
        }
        Ok(Self { channels })
    }
    pub fn channels(&self) -> &[AudioChannelRecipe] {
        &self.channels
    }
}
impl TryFrom<AudioBundleRecipeWire> for AudioBundleRecipe {
    type Error = ProtocolError;
    fn try_from(wire: AudioBundleRecipeWire) -> Result<Self, Self::Error> {
        Self::new(wire.channels)
    }
}
impl From<AudioBundleRecipe> for AudioBundleRecipeWire {
    fn from(recipe: AudioBundleRecipe) -> Self {
        Self {
            channels: recipe.channels,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "modality",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GenerationInput {
    Text {
        prompt: String,
    },
    Image {
        source_version: SourceVersionId,
        prompt: String,
    },
    Patterns {
        patterns: Vec<PatternSpec>,
    },
    AudioBundle {
        source_version: SourceVersionId,
        recipe: AudioBundleRecipe,
    },
    Audio {
        source_version: SourceVersionId,
        mapping: AudioMapping,
        mode: AudioMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        analysis: Option<AudioAnalysisOptions>,
    },
}

impl std::fmt::Debug for GenerationInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Text { .. } => "Text",
            Self::Image { .. } => "Image",
            Self::Patterns { .. } => "Patterns",
            Self::Audio { .. } => "Audio",
            Self::AudioBundle { .. } => "AudioBundle",
        })
    }
}

impl GenerationInput {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Text { prompt } | Self::Image { prompt, .. } => {
                bounded_text(prompt, MAX_PROMPT_BYTES, "generation prompt")?;
            }
            Self::Patterns { patterns } => {
                if patterns.is_empty() || patterns.len() > Axis::ALL.len() {
                    return Err(ProtocolError::invalid(
                        "pattern input requires between one and six axes",
                    ));
                }
                let mut axes = BTreeSet::new();
                if patterns.iter().any(|pattern| !axes.insert(pattern.axis())) {
                    return Err(ProtocolError::invalid(
                        "pattern input contains duplicate axes",
                    ));
                }
                validate_patterns(patterns).map_err(|e| ProtocolError::invalid(e.to_string()))?;
            }
            Self::Audio { .. } | Self::AudioBundle { .. } => {}
        }
        Ok(())
    }

    /// Text and explicit patterns use an engine-materialized immutable source
    /// artifact. Media modalities must bind to the supplied source snapshot.
    pub fn source_version(&self) -> Option<&SourceVersionId> {
        match self {
            Self::Image { source_version, .. }
            | Self::Audio { source_version, .. }
            | Self::AudioBundle { source_version, .. } => Some(source_version),
            Self::Text { .. } | Self::Patterns { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMode {
    Envelope,
    Beats,
    Vibe,
}

/// Stable machine-readable code, not a free-form user prompt or private trace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReviewReason(String);

impl ReviewReason {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'.')
            })
        {
            return Err(ProtocolError::invalid(
                "review reason must be a bounded lowercase machine-readable code",
            ));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ReviewReason {
    type Error = ProtocolError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<ReviewReason> for String {
    fn from(value: ReviewReason) -> Self {
        value.0
    }
}
impl std::fmt::Display for ReviewReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "ReviewFlagWire", into = "ReviewFlagWire")]
pub struct ReviewFlag {
    pub axis: Option<Axis>,
    pub range: TimeRange,
    pub reason: ReviewReason,
    pub evidence: EvidenceKind,
    pub confidence: Option<Confidence>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewFlagWire {
    axis: Option<Axis>,
    range: TimeRange,
    reason: ReviewReason,
    evidence: EvidenceKind,
    confidence: Option<Confidence>,
}

impl ReviewFlag {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.range.start() < ProjectTime::ZERO || self.range.end() <= self.range.start() {
            return Err(ProtocolError::invalid(
                "review span must be nonnegative and nonempty",
            ));
        }
        if self.confidence.as_ref().is_some_and(|confidence| {
            !matches!(
                confidence.calibration(),
                ConfidenceCalibration::Uncalibrated
            )
        }) {
            return Err(ProtocolError::invalid(
                "review confidence is uncalibrated; this contract cannot claim calibration",
            ));
        }
        Ok(())
    }
}
impl TryFrom<ReviewFlagWire> for ReviewFlag {
    type Error = ProtocolError;
    fn try_from(value: ReviewFlagWire) -> Result<Self, Self::Error> {
        let flag = Self {
            axis: value.axis,
            range: value.range,
            reason: value.reason,
            evidence: value.evidence,
            confidence: value.confidence,
        };
        flag.validate()?;
        Ok(flag)
    }
}
impl From<ReviewFlag> for ReviewFlagWire {
    fn from(value: ReviewFlag) -> Self {
        Self {
            axis: value.axis,
            range: value.range,
            reason: value.reason,
            evidence: value.evidence,
            confidence: value.confidence,
        }
    }
}

pub fn validate_reviews(review: &[ReviewFlag]) -> Result<(), ProtocolError> {
    if review.len() > MAX_REVIEW_FLAGS {
        return Err(ProtocolError::invalid("too many candidate review spans"));
    }
    for flag in review {
        flag.validate()?;
    }
    Ok(())
}

pub fn validate_merge_axes(axes: &[Axis]) -> Result<(), ProtocolError> {
    if axes.is_empty() || axes.len() > Axis::ALL.len() {
        return Err(ProtocolError::invalid(
            "candidate merge requires between one and six axes",
        ));
    }
    let mut unique = BTreeSet::new();
    if axes.iter().any(|axis| !unique.insert(*axis)) {
        return Err(ProtocolError::invalid(
            "candidate merge contains duplicate axes",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticIssue {
    pub code: String,
    pub message: String,
    pub axis: Option<Axis>,
    pub range: Option<TimeRange>,
}

impl DiagnosticIssue {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        bounded_text(&self.code, 64, "diagnostic code")?;
        bounded_text(&self.message, 512, "diagnostic message")?;
        if self
            .range
            .as_ref()
            .is_some_and(|range| range.start() < ProjectTime::ZERO || range.end() <= range.start())
        {
            return Err(ProtocolError::invalid(
                "diagnostic span must be nonnegative and nonempty",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsReport {
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub candidate_id: Option<CandidateId>,
    pub base_revision: Option<RevisionId>,
    pub protected_regions: Vec<ProtectedRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_speed: Option<SegmentSpeedScan>,
    pub issues: Vec<DiagnosticIssue>,
}

impl DiagnosticsReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.candidate_id.is_some() != self.base_revision.is_some() {
            return Err(ProtocolError::invalid(
                "candidate diagnostics require candidate and base revision together",
            ));
        }
        if self.protected_regions.len() > 1024 || self.issues.len() > MAX_DIAGNOSTIC_ISSUES {
            return Err(ProtocolError::invalid(
                "diagnostics exceed the bounded control path",
            ));
        }
        if let Some(scan) = &self.segment_speed {
            scan.validate()
                .map_err(|_| ProtocolError::invalid("invalid segment-speed diagnostics"))?;
        }
        for issue in &self.issues {
            issue.validate()?;
        }
        // Reserve room for the outer response identity and tagged envelope. The
        // engine must additionally bound the fully serialized response.
        let encoded = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("diagnostics cannot be serialized"))?;
        if encoded.len() > MAX_CONTROL_BYTES.saturating_sub(4096) {
            return Err(ProtocolError::invalid(
                "diagnostics exceed the bounded control frame",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod raw_pair_tests {
    use super::*;
    #[test]
    fn audio_bundle_requires_explicit_distinct_six_axes_and_checked_wire_values() {
        let channels: Vec<_> = Axis::ALL
            .iter()
            .map(|axis| AudioChannelRecipe {
                mapping: AudioMapping::new(*axis, 0.2, NormalizedPosition::new(0.4).unwrap())
                    .unwrap(),
                mode: AudioMode::Envelope,
                analysis: AudioAnalysisOptions::new(AudioBand::Bass, 0.2).unwrap(),
            })
            .collect();
        assert!(AudioBundleRecipe::new(channels[..5].to_vec()).is_err());
        let mut duplicates = channels.clone();
        duplicates[5] = duplicates[0].clone();
        assert!(AudioBundleRecipe::new(duplicates).is_err());
        let recipe = AudioBundleRecipe::new(channels).unwrap();
        let value = serde_json::to_value(&recipe).unwrap();
        assert_eq!(
            serde_json::from_value::<AudioBundleRecipe>(value.clone())
                .unwrap()
                .channels()
                .len(),
            6
        );
        for bad in ["axis", "analysis", "extra"] {
            let mut value = value.clone();
            match bad {
                "axis" => value["channels"][5]["mapping"]["axis"] = serde_json::json!("stroke"),
                "analysis" => value["channels"][0]["analysis"]["threshold"] = serde_json::json!(2),
                _ => value["channels"][0]["extra"] = serde_json::json!(true),
            }
            assert!(serde_json::from_value::<AudioBundleRecipe>(value).is_err());
        }
        let source_version = SourceVersionId::new("audio-bundle-source").unwrap();
        let input = GenerationInput::AudioBundle {
            source_version: source_version.clone(),
            recipe,
        };
        input.validate().unwrap();
        assert_eq!(input.source_version(), Some(&source_version));
        assert!(
            serde_json::from_slice::<GenerationInput>(&serde_json::to_vec(&input).unwrap()).is_ok()
        );
    }
    #[test]
    fn independent_audio_alignment_is_exact_signed_and_fail_closed() {
        let alignment = AudioAlignment {
            rate: None,
            video_anchor: SourceTimestamp::new(1, 3).unwrap(),
            audio_anchor: SourceTimestamp::new(-2, 7).unwrap(),
        };
        let mapped = alignment.map(ExactSeconds::new(5, 6).unwrap()).unwrap();
        assert_eq!(mapped, ExactSeconds::new(3, 14).unwrap());
        assert_eq!(
            alignment.map(ExactSeconds::new(0, 1).unwrap()).unwrap(),
            ExactSeconds::new(-13, 21).unwrap()
        );
        assert!(alignment
            .map(ExactSeconds::new(i128::MAX, 1).unwrap())
            .is_err());
        assert!(serde_json::from_value::<IndependentAudio>(serde_json::json!({"source_version":"audio","alignment":{"video_anchor":{"numerator":0,"denominator":1}}})).is_err());
    }
    #[test]
    fn audio_rate_is_exact_positive_and_preserves_legacy_wire() {
        let legacy = serde_json::json!({"video_anchor":{"numerator":1,"denominator":3},"audio_anchor":{"numerator":-2,"denominator":7}});
        let mut alignment: AudioAlignment = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(&alignment).unwrap(), legacy);
        alignment.rate = Some(AudioClockRate::new(3, 2).unwrap());
        assert_eq!(
            alignment.map(ExactSeconds::new(5, 6).unwrap()).unwrap(),
            ExactSeconds::new(13, 28).unwrap()
        );
        assert_eq!(
            alignment.map(ExactSeconds::new(0, 1).unwrap()).unwrap(),
            ExactSeconds::new(-11, 14).unwrap()
        );
        alignment.video_anchor = SourceTimestamp::new(0, 1).unwrap();
        alignment.audio_anchor = SourceTimestamp::new(0, 1).unwrap();
        alignment.rate = Some(AudioClockRate::new(1_000_001, 1_000_000).unwrap());
        assert_eq!(
            alignment.map(ExactSeconds::new(3600, 1).unwrap()).unwrap(),
            ExactSeconds::new(9_000_009, 2500).unwrap()
        );
        assert!(alignment
            .map(ExactSeconds::new(i128::MAX, 1).unwrap())
            .is_err());
        alignment.rate = Some(AudioClockRate::new(1, 2).unwrap());
        assert_eq!(
            alignment
                .map(ExactSeconds::new(i128::MAX - 1, 1).unwrap())
                .unwrap(),
            ExactSeconds::new((i128::MAX - 1) / 2, 1).unwrap()
        );
        for rate in [
            serde_json::json!({"numerator":0,"denominator":1}),
            serde_json::json!({"numerator":-1,"denominator":1}),
            serde_json::json!({"numerator":1,"denominator":0}),
            serde_json::json!({"numerator":1,"denominator":1,"extra":true}),
        ] {
            let mut invalid = legacy.clone();
            invalid["rate"] = rate;
            assert!(serde_json::from_value::<AudioAlignment>(invalid).is_err());
        }
        let roundtrip: AudioAlignment =
            serde_json::from_value(serde_json::to_value(&alignment).unwrap()).unwrap();
        assert_eq!(roundtrip.rate, alignment.rate);
    }
    fn pair() -> RawFramePair {
        RawFramePair {
            source_epoch: AttemptId::new("declared-epoch").unwrap(),
            previous_frame: FrameId::new(10),
            current_frame: FrameId::new(11),
            previous_time: SourceTimestamp::new(-1, 3).unwrap(),
            current_time: SourceTimestamp::new(0, 1).unwrap(),
            presentation_clock: None,
        }
    }
    #[test]
    fn pair_presentation_binds_epoch_and_preserves_absolute_clock_remainders() {
        let mut p = pair();
        p.presentation_clock = Some(LivePresentationClock {
            epoch: p.source_epoch.clone(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(1_000_000_000),
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(1_000_000_000),
            added_buffer_ns: 1_000_000_000,
        });
        let mapped = p.presentation_mapping().unwrap().unwrap();
        assert_eq!(mapped[0].project.time, ProjectTime::from_nanos(666_666_666));
        assert_eq!(mapped[0].project.remainder_numerator, "2");
        assert_eq!(mapped[0].project.denominator, "3");
        assert_eq!(
            mapped[1].project.time,
            ProjectTime::from_nanos(1_000_000_000)
        );
        assert_eq!(mapped[0].presentation_tick, LiveClockTick(1_666_666_666));
        p.presentation_clock.as_mut().unwrap().epoch = AttemptId::new("changed-epoch").unwrap();
        assert!(p.presentation_mapping().is_err());
        p.presentation_clock.as_mut().unwrap().epoch = p.source_epoch.clone();
        p.presentation_clock.as_mut().unwrap().project_anchor = ProjectTime::ZERO;
        assert!(p.presentation_mapping().is_err());
    }
    #[test]
    fn captured_pair_preserves_rational_difference_and_refuses_ambiguous_order() {
        let p = pair();
        let elapsed = p.elapsed().unwrap();
        assert_eq!(elapsed.time, ProjectTime::from_nanos(333_333_333));
        let mut p = pair();
        p.current_frame = p.previous_frame;
        assert!(p.elapsed().is_err());
        let mut p = pair();
        p.current_time = SourceTimestamp::new(1, 1).unwrap();
        assert!(p.elapsed().is_err());
        let mut p = pair();
        p.current_time = p.previous_time;
        assert!(p.elapsed().is_err());
        let mut p = pair();
        p.previous_time = SourceTimestamp::new(0, 1).unwrap();
        p.current_time = SourceTimestamp::new(1, 2_000_000_000).unwrap();
        assert!(p.elapsed().is_err());
        let mut p = pair();
        p.previous_time = SourceTimestamp::new(0, 1).unwrap();
        p.current_time = SourceTimestamp::new(1, 2).unwrap();
        assert_eq!(
            p.elapsed().unwrap().time,
            ProjectTime::from_nanos(500_000_000)
        );
    }
}
