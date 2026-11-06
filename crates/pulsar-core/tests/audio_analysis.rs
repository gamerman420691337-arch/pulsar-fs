use pulsar_core::*;

fn tone(hz: f64) -> Vec<f32> {
    (0..32_000)
        .map(|n| (0.8 * (std::f64::consts::TAU * hz * f64::from(n) / 16_000.0).sin()) as f32)
        .collect()
}
fn levels(pcm: &[f32], band: AudioBand, window: u32, threshold: f64) -> AudioFeatures {
    analyze_audio(
        pcm,
        16_000,
        window,
        AudioAnalysisOptions::new(band, threshold).unwrap(),
    )
    .unwrap()
    .features
}
fn steady(features: &AudioFeatures) -> f64 {
    let retained: Vec<_> = features
        .envelope
        .iter()
        .filter(|s| s.time().as_nanos() >= 1_000_000_000)
        .collect();
    retained.iter().map(|s| s.level()).sum::<f64>() / retained.len() as f64
}

#[test]
fn independent_tones_are_selected_by_band_without_peak_normalization() {
    for (hz, band) in [
        (100.0, AudioBand::Bass),
        (1000.0, AudioBand::Mid),
        (5000.0, AudioBand::High),
    ] {
        let pcm = tone(hz);
        let pass = steady(&levels(&pcm, band, 160, 0.05));
        assert!(pass > 0.5 && pass < 0.58, "{hz}: {pass}");
        for other in [AudioBand::Bass, AudioBand::Mid, AudioBand::High] {
            if other != band {
                assert!(
                    steady(&levels(&pcm, other, 160, 0.05)) < 0.1,
                    "{hz} leaked into {other:?}"
                );
            }
        }
    }
    assert!(steady(&levels(&vec![0.8; 32000], AudioBand::Bass, 160, 0.05)) < 1e-6);
}

#[test]
fn threshold_changes_envelope_and_onsets_but_never_clocks() {
    let pcm: Vec<_> = (0..16000)
        .map(|i| if !(3200..9600).contains(&i) { 0.0 } else { 0.4 })
        .collect();
    let low = levels(&pcm, AudioBand::Full, 160, 0.2);
    let high = levels(&pcm, AudioBand::Full, 160, 0.6);
    assert_eq!(
        low.amplitude_onsets,
        vec![ProjectTime::from_nanos(200_000_000)]
    );
    assert!(high.amplitude_onsets.is_empty());
    assert!(high.envelope.iter().all(|s| s.level() == 0.0));
    assert_eq!(low.duration, high.duration);
    assert_eq!(low.duration.as_nanos(), 1_000_000_000);
    assert_eq!(
        low.envelope.iter().map(|s| s.time()).collect::<Vec<_>>(),
        high.envelope.iter().map(|s| s.time()).collect::<Vec<_>>()
    );
    let old = extract_audio_features(&pcm, 16000, 160).unwrap();
    assert_eq!(old, low);
    let quiet = vec![0.1; 16000];
    assert!(extract_audio_features(&quiet, 16000, 160).unwrap().envelope[0].level() > 0.09);
    assert_eq!(
        levels(&quiet, AudioBand::Full, 160, 0.2).envelope[0].level(),
        0.0
    );
}

#[test]
fn filter_state_crosses_windows_and_preserves_partial_endpoint() {
    let mut pcm = tone(100.0);
    pcm.truncate(31003);
    let small = levels(&pcm, AudioBand::Bass, 160, 0.05);
    let large = levels(&pcm, AudioBand::Bass, 320, 0.05);
    // Two small RMS windows combine into one large RMS window when state is continuous.
    for (i, s) in large.envelope.iter().take(90).enumerate().skip(5) {
        let x = small.envelope[2 * i].level();
        let y = small.envelope[2 * i + 1].level();
        assert!((s.level() - ((x * x + y * y) / 2.0).sqrt()).abs() < 1e-12);
    }
    assert_eq!(small.duration.as_nanos(), 31003 * 1_000_000_000_i64 / 16000);
    assert_eq!(small.envelope.last().unwrap().time(), small.duration);
    assert_eq!(
        small.envelope.last().unwrap().level(),
        small.envelope[small.envelope.len() - 2].level()
    );
}

#[test]
fn invalid_options_pcm_and_rates_fail_closed() {
    for threshold in [f64::NAN, f64::INFINITY, 0.0, 0.049, 0.901] {
        assert!(AudioAnalysisOptions::new(AudioBand::Full, threshold).is_err());
    }
    let options = AudioAnalysisOptions::new(AudioBand::High, 0.35).unwrap();
    for rate in [0, 8000, 15999, 192001] {
        assert!(analyze_audio(&[0.0; 160], rate, 160, options).is_err());
    }
    for pcm in [vec![], vec![f32::NAN], vec![1.01]] {
        assert!(analyze_audio(&pcm, 16000, 160, options).is_err());
    }
    assert!(analyze_audio(&[0.0; 160], 16000, 0, options).is_err());
    assert!(analyze_audio(&[0.0; 160], 16000, 160, options)
        .unwrap()
        .features
        .envelope
        .iter()
        .all(|s| s.level() == 0.0));
}
