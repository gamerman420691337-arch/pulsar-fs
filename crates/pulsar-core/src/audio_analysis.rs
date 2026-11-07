//! Causal spectral RMS analysis; source clocks are never retimed.
use crate::{synthesis::extract_audio_features_impl, AudioFeatures, CoreError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBand {
    Full,
    Bass,
    Mid,
    High,
}

impl AudioBand {
    pub fn cutoffs_hz(self) -> Option<(u32, u32)> {
        match self {
            Self::Full => None,
            Self::Bass => Some((20, 250)),
            Self::Mid => Some((250, 2500)),
            Self::High => Some((2500, 8000)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawOptions", into = "RawOptions")]
pub struct AudioAnalysisOptions {
    band: AudioBand,
    threshold: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOptions {
    band: AudioBand,
    threshold: f64,
}

impl AudioAnalysisOptions {
    pub fn new(band: AudioBand, threshold: f64) -> Result<Self, CoreError> {
        if !threshold.is_finite() {
            return Err(CoreError::NonFinite);
        }
        if !(0.05..=0.9).contains(&threshold) {
            return Err(CoreError::InvalidMotion(
                "audio threshold must be in [0.05,0.9]".into(),
            ));
        }
        Ok(Self { band, threshold })
    }
    pub fn band(self) -> AudioBand {
        self.band
    }
    pub fn threshold(self) -> f64 {
        self.threshold
    }
}
impl TryFrom<RawOptions> for AudioAnalysisOptions {
    type Error = CoreError;
    fn try_from(raw: RawOptions) -> Result<Self, Self::Error> {
        Self::new(raw.band, raw.threshold)
    }
}
impl From<AudioAnalysisOptions> for RawOptions {
    fn from(options: AudioAnalysisOptions) -> Self {
        Self {
            band: options.band,
            threshold: options.threshold,
        }
    }
}

pub struct AudioAnalysis {
    pub features: AudioFeatures,
    pub saturated_windows: usize,
}

// W3C Audio EQ Cookbook LPF/HPF, Q = 1/sqrt(2), normalized by a0.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}
impl Biquad {
    fn new(rate: u32, cutoff: u32, high: bool) -> Self {
        let w = std::f64::consts::TAU * f64::from(cutoff) / f64::from(rate);
        let c = w.cos();
        let alpha = w.sin() / std::f64::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let v = if high {
            (1.0 + c) / 2.0
        } else {
            (1.0 - c) / 2.0
        };
        Self {
            b: [v / a0, (if high { -2.0 * v } else { 2.0 * v }) / a0, v / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }
    fn sample(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Explicit analysis gates envelope levels below threshold. Filter state
/// spans all windows; endpoint and onset timestamps retain original clocks.
pub fn analyze_audio(
    pcm: &[f32],
    rate: u32,
    window: u32,
    options: AudioAnalysisOptions,
) -> Result<AudioAnalysis, CoreError> {
    let mut filters = if let Some((lower, upper)) = options.band.cutoffs_hz() {
        if rate < 2 * upper || rate <= 2 * lower {
            return Err(CoreError::InvalidMotion(
                "audio band exceeds the sample-rate Nyquist limit".into(),
            ));
        }
        Some((
            Biquad::new(rate, lower, true),
            (rate > 2 * upper).then(|| Biquad::new(rate, upper, false)),
        ))
    } else {
        None
    };
    let mut filter = |x| match &mut filters {
        Some((high, low)) => {
            let x = high.sample(x);
            low.as_mut().map_or(x, |low| low.sample(x))
        }
        None => x,
    };
    let (features, saturated_windows) =
        extract_audio_features_impl(pcm, rate, window, options.threshold, true, &mut filter)?;
    Ok(AudioAnalysis {
        features,
        saturated_windows,
    })
}
