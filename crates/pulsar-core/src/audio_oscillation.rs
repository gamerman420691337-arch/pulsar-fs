//! Energy-modulated carrier synthesis; no decode, retiming or device authority.
use crate::*;

pub const AUDIO_OSCILLATION_HZ: u32 = 8;
pub const AUDIO_OSCILLATION_PERIOD_NS: i64 = 125_000_000;
pub const MAX_AUDIO_OSCILLATION_INTERVAL_NS: i64 = AUDIO_OSCILLATION_PERIOD_NS / 4;

/// The mapping's offset and amplitude bound the carrier's complete range.
/// Zero RMS holds its center; quiet RMS remains quiet without normalization.
/// Sample timestamps and endpoint are preserved; sparse envelopes are refused.
pub fn synthesize_audio_oscillation(
    samples: &[AudioEnvelopeSample],
    mapping: &AudioMapping,
) -> Result<MotionProgram, CoreError> {
    if samples.is_empty() || samples.len() > MAX_SYNTHESIS_ACTIONS {
        return Err(CoreError::InvalidMotion(
            "oscillation requires a bounded nonempty envelope".into(),
        ));
    }
    for pair in samples.windows(2) {
        let dt = pair[1]
            .time()
            .as_nanos()
            .checked_sub(pair[0].time().as_nanos())
            .ok_or(CoreError::Overflow)?;
        if dt <= 0 || dt > MAX_AUDIO_OSCILLATION_INTERVAL_NS {
            return Err(CoreError::InvalidMotion(
                "8 Hz oscillation requires increasing feature times no more than 31.25 ms apart"
                    .into(),
            ));
        }
    }
    let center = mapping.offset().value() + mapping.amplitude() / 2.0;
    let half_span = mapping.amplitude() / 2.0;
    let actions = samples
        .iter()
        .map(|sample| {
            let phase = sample.time().as_nanos() % AUDIO_OSCILLATION_PERIOD_NS;
            let carrier =
                (std::f64::consts::TAU * phase as f64 / AUDIO_OSCILLATION_PERIOD_NS as f64).sin();
            MotionAction::new(
                sample.time(),
                NormalizedPosition::new(center + half_span * sample.level() * carrier)?,
                EvidenceKind::Synthesized,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let gaps = if samples[0].time() > ProjectTime::ZERO {
        vec![TimeRange::new(ProjectTime::ZERO, samples[0].time())?]
    } else {
        vec![]
    };
    MotionProgram::new(vec![MotionTrack::with_gaps(mapping.axis(), actions, gaps)?])
}
