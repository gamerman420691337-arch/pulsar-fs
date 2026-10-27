//! Per-sample precedence. Clock alignment and source reads belong to workers.
use crate::{EvidenceKind, NormalizedPosition};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultimodalIntent {
    Faithful,
    CreativeAudio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionContribution {
    Visual,
    Audio,
    Unavailable,
}

/// Faithful intent preserves every available visual sample, including uncertain
/// visual inference. Audio only assists where visual motion is unavailable.
/// Creative audio never silently falls back to a different visual recipe.
pub fn select_multimodal_sample(
    intent: MultimodalIntent,
    visual: Option<(NormalizedPosition, EvidenceKind)>,
    audio: Option<NormalizedPosition>,
) -> (
    Option<(NormalizedPosition, EvidenceKind)>,
    MotionContribution,
) {
    let visual = visual.filter(|(_, evidence)| *evidence != EvidenceKind::Unavailable);
    if intent == MultimodalIntent::Faithful {
        if let Some(sample) = visual {
            return (Some(sample), MotionContribution::Visual);
        }
    }
    if let Some(position) = audio {
        return (
            Some((position, EvidenceKind::Synthesized)),
            MotionContribution::Audio,
        );
    }
    (None, MotionContribution::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicting_audio_cannot_replace_visual_without_creative_intent() {
        let visual = NormalizedPosition::new(0.2).unwrap();
        let audio = NormalizedPosition::new(0.9).unwrap();
        for evidence in [
            EvidenceKind::Observed,
            EvidenceKind::Inferred,
            EvidenceKind::Predicted,
            EvidenceKind::Synthesized,
        ] {
            assert_eq!(
                select_multimodal_sample(
                    MultimodalIntent::Faithful,
                    Some((visual, evidence)),
                    Some(audio)
                ),
                (Some((visual, evidence)), MotionContribution::Visual)
            );
        }
        assert_eq!(
            select_multimodal_sample(
                MultimodalIntent::CreativeAudio,
                Some((visual, EvidenceKind::Observed)),
                Some(audio)
            ),
            (
                Some((audio, EvidenceKind::Synthesized)),
                MotionContribution::Audio
            )
        );
        assert_eq!(
            select_multimodal_sample(
                MultimodalIntent::CreativeAudio,
                Some((visual, EvidenceKind::Observed)),
                None
            ),
            (None, MotionContribution::Unavailable)
        );
        assert_eq!(
            select_multimodal_sample(MultimodalIntent::Faithful, None, Some(audio)),
            (
                Some((audio, EvidenceKind::Synthesized)),
                MotionContribution::Audio
            )
        );
        assert_eq!(
            select_multimodal_sample(MultimodalIntent::Faithful, None, None),
            (None, MotionContribution::Unavailable)
        );
    }
}
