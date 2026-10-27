//! Original-clock audio windows on the visual sample lattice. No hidden retiming.
use super::*;

pub(super) struct MixedAudio {
    pub timeline: audio::AudioTimeline,
    features: AudioFeatures,
    resampled_samples: usize,
    settings: MixedAudioSettings,
}

#[derive(Serialize, serde::Deserialize)]
pub(super) struct MixedSample {
    pub original_video_time: ExactSeconds,
    pub aligned_audio_time: ExactSeconds,
    pub audio_elapsed: ExactSeconds,
    pub audio_window_index: Option<usize>,
    pub visual_position: Option<NormalizedPosition>,
    pub visual_evidence: EvidenceKind,
    pub contribution: MotionContribution,
}

impl MixedAudio {
    pub fn prepare(
        request: &WorkerRequest,
        settings: &MixedAudioSettings,
        deadline: Instant,
    ) -> Result<Self> {
        let decoded = audio::decode_source(
            request,
            request
                .independent_audio
                .as_ref()
                .unwrap_or(&request.source),
            deadline,
        )?;
        let features = extract_audio_features(&decoded.samples, audio::SAMPLE_RATE, 160)?;
        Ok(Self {
            resampled_samples: decoded.samples.len(),
            timeline: decoded.timeline,
            features,
            settings: settings.clone(),
        })
    }

    pub fn select(
        &self,
        pts: i64,
        index: &media::MediaIndex,
        position: NormalizedPosition,
        evidence: EvidenceKind,
    ) -> Result<(Option<(NormalizedPosition, EvidenceKind)>, MixedSample)> {
        let source_time = ExactSeconds::new(
            i128::from(pts) * i128::from(index.time_base_num),
            u64::from(index.time_base_den),
        )?;
        let audio_time = if let Some(declaration) = &self.settings.independent {
            declaration.alignment.map(source_time)?
        } else {
            source_time
        };
        let elapsed = audio_time.checked_sub(self.timeline.origin()?)?;
        let duration = ExactSeconds::new(
            i128::try_from(self.resampled_samples)?,
            u64::from(audio::SAMPLE_RATE),
        )?;
        let window = if elapsed.numerator() >= 0
            && elapsed.checked_cmp(duration)? == std::cmp::Ordering::Less
        {
            // 160 samples / 16000 Hz = exact 1/100 second windows. Compare
            // rational clocks before flooring once to the containing window.
            Some(usize::try_from(
                elapsed
                    .numerator()
                    .checked_mul(100)
                    .context("audio window clock overflow")?
                    / i128::from(elapsed.denominator()),
            )?)
        } else {
            None
        };
        let audio = window
            .and_then(|i| self.features.envelope.get(i))
            .map(|sample| {
                NormalizedPosition::new(
                    self.settings.mapping.offset().value()
                        + self.settings.mapping.amplitude() * sample.level(),
                )
            })
            .transpose()?;
        let visual = (evidence != EvidenceKind::Unavailable).then_some((position, evidence));
        let (selected, contribution) =
            select_multimodal_sample(self.settings.intent, visual, audio);
        Ok((
            selected,
            MixedSample {
                original_video_time: source_time,
                aligned_audio_time: audio_time,
                audio_elapsed: elapsed,
                audio_window_index: window,
                visual_position: visual.map(|v| v.0),
                visual_evidence: evidence,
                contribution,
            },
        ))
    }
}
