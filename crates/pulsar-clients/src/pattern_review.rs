//! Disposable, bounded review of exact deterministic generation samples.
use eframe::egui;
use pulsar_protocol::{validate_patterns, PatternParameters, PatternSpec};

const BINS: usize = 128;
const EXACT_POINTS: usize = 256;
const CACHE_ENTRIES: usize = 16;

#[derive(Clone, Copy, Debug)]
struct Sample {
    ns: i64,
    value: f64,
}
#[derive(Debug)]
struct Bucket {
    first: i64,
    last: i64,
    min: f64,
    max: f64,
}
#[derive(Debug)]
struct TrackReview {
    parameters: PatternParameters,
    count: usize,
    first: Sample,
    last: Sample,
    min: f64,
    max: f64,
    exact: Option<Vec<Sample>>,
    buckets: Vec<Bucket>,
}
#[derive(Debug)]
struct PatternReview {
    tracks: Vec<TrackReview>,
}
impl PatternReview {
    fn build(patterns: &[PatternSpec]) -> anyhow::Result<Self> {
        validate_patterns(patterns)?;
        let mut tracks = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            let count = pattern.action_count()?;
            let mut exact = (count <= EXACT_POINTS).then(|| Vec::with_capacity(count));
            let mut buckets: Vec<Bucket> = Vec::with_capacity(BINS.min(count));
            let mut first = None;
            let mut last = None;
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for (index, action) in pattern.sampled_actions()?.enumerate() {
                let action = action?;
                let sample = Sample {
                    ns: action.time().as_nanos(),
                    value: action.position().value(),
                };
                first.get_or_insert(sample);
                last = Some(sample);
                min = min.min(sample.value);
                max = max.max(sample.value);
                if let Some(points) = &mut exact {
                    points.push(sample);
                }
                let bin = index * BINS.min(count) / count;
                if bin == buckets.len() {
                    buckets.push(Bucket {
                        first: sample.ns,
                        last: sample.ns,
                        min: sample.value,
                        max: sample.value,
                    });
                } else {
                    let bucket = &mut buckets[bin];
                    bucket.last = sample.ns;
                    bucket.min = bucket.min.min(sample.value);
                    bucket.max = bucket.max.max(sample.value);
                }
            }
            tracks.push(TrackReview {
                parameters: pattern.parameters().clone(),
                count,
                first: first.ok_or_else(|| anyhow::anyhow!("Pattern has no samples"))?,
                last: last.ok_or_else(|| anyhow::anyhow!("Pattern has no samples"))?,
                min,
                max,
                exact,
                buckets,
            });
        }
        Ok(Self { tracks })
    }

    fn show(&self, ui: &mut egui::Ui) {
        ui.label("Synthetic motion preview");
        ui.small("This reviews the proposed generation samples. Generation creates a candidate; commit and device playback require separate actions.");
        for track in &self.tracks {
            let p = &track.parameters;
            ui.label(format!(
                "{} · {:?} · {} synthesized actions",
                p.axis.as_str(),
                p.pattern,
                track.count
            ));
            ui.label(format!(
                "Duration {} ns ({} s) · {} samples/s · {} cycles/s",
                p.duration.as_nanos(),
                p.duration.as_nanos() as f64 / 1e9,
                p.sample_hz,
                p.frequency_hz
            ));
            ui.label(format!(
                "Offset {} · amplitude ±{} · phase {} cycles · duty {}",
                p.offset.value(),
                p.amplitude,
                p.phase_cycles,
                p.duty_cycle
            ));
            ui.label(format!(
                "Sampled positions {:.6}–{:.6} · first {:.6} · last {:.6}",
                track.min, track.max, track.first.value, track.last.value
            ));
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().clamp(16.0, 720.0), 112.0),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
            let plot = rect.shrink(5.0);
            let position = |ns: i64, value: f64| {
                egui::pos2(
                    plot.left() + plot.width() * (ns as f64 / p.duration.as_nanos() as f64) as f32,
                    plot.bottom() - plot.height() * value as f32,
                )
            };
            let stroke = egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color);
            if let Some(points) = &track.exact {
                painter.add(egui::Shape::line(
                    points.iter().map(|s| position(s.ns, s.value)).collect(),
                    stroke,
                ));
            } else {
                for bucket in &track.buckets {
                    let left = position(bucket.first, bucket.max);
                    let right = position(bucket.last, bucket.min);
                    painter.rect_filled(
                        egui::Rect::from_min_max(left, right),
                        0.0,
                        stroke.color.gamma_multiply(0.35),
                    );
                    painter.line_segment(
                        [
                            position(bucket.first, bucket.min),
                            position(bucket.first, bucket.max),
                        ],
                        stroke,
                    );
                }
            }
            for sample in [track.first, track.last] {
                painter.circle_filled(position(sample.ns, sample.value), 2.5, stroke.color);
            }
            ui.small("Horizontal: full duration. Vertical: normalized position 0–1; no automatic amplitude scaling.");
            if track.exact.is_some() {
                ui.small("All discrete generation samples are shown; connecting lines are a visual guide.");
            } else {
                ui.small("Dense overview: each band retains the minimum and maximum of every sample in its interval. It does not show each oscillation; use the exact frequency and sampling rate above.");
            }
        }
    }
}

type Cached = (Vec<PatternSpec>, Result<PatternReview, String>);
#[derive(Default)]
pub(crate) struct PatternReviews {
    entries: Vec<Cached>,
}
impl PatternReviews {
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, patterns: &[PatternSpec]) {
        if let Err(error) = validate_patterns(patterns) {
            ui.colored_label(
                egui::Color32::RED,
                format!("Pattern preview refused: {error}"),
            );
            return;
        }
        let index = match self.entries.iter().position(|(key, _)| key == patterns) {
            Some(index) => index,
            None => {
                if self.entries.len() == CACHE_ENTRIES {
                    let _ = self.entries.remove(0);
                }
                let result = PatternReview::build(patterns).map_err(|e| e.to_string());
                self.entries.push((patterns.to_vec(), result));
                self.entries.len() - 1
            }
        };
        match &self.entries[index].1 {
            Ok(review) => review.show(ui),
            Err(error) => {
                ui.colored_label(
                    egui::Color32::RED,
                    format!("Pattern preview refused: {error}"),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::{parse_pattern_prompt, Axis};

    #[test]
    fn six_axes_keep_exact_samples_parameters_and_headless_graphs() {
        let patterns = Axis::ALL.map(|axis| parse_pattern_prompt(&format!("pattern=triangle axis={} duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8", axis.as_str())).unwrap());
        let before = serde_json::to_vec(&patterns).unwrap();
        let review = PatternReview::build(&patterns).unwrap();
        assert_eq!(review.tracks.len(), 6);
        for (index, track) in review.tracks.iter().enumerate() {
            assert_eq!(track.parameters.axis, Axis::ALL[index]);
            assert_eq!(track.count, 17);
            assert_eq!(track.first.ns, 0);
            assert_eq!(track.last.ns, 2_000_000_000);
            assert!((track.min - 0.4).abs() < 1e-12);
            assert!((track.max - 0.6).abs() < 1e-12);
            let exact = track.exact.as_ref().unwrap();
            for (sample, expected) in exact.iter().zip([
                0.5, 0.55, 0.6, 0.55, 0.5, 0.45, 0.4, 0.45, 0.5, 0.55, 0.6, 0.55, 0.5, 0.45, 0.4,
                0.45, 0.5,
            ]) {
                assert!((sample.value - expected).abs() < 1e-12);
            }
        }
        let context = egui::Context::default();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 2500.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| review.show(ui));
            },
        );
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|s| matches!(&s.shape,egui::Shape::Path(p) if p.points.len()==17))
                .count(),
            6
        );
        assert_eq!(serde_json::to_vec(&patterns).unwrap(), before);
    }

    #[test]
    fn dense_maximum_budget_retains_all_sample_extrema_with_bounded_storage() {
        let pattern = parse_pattern_prompt("pattern=sine axis=stroke duration=999.999s frequency=50hz amplitude=0.2 offset=0.5 sample_hz=1000").unwrap();
        let review = PatternReview::build(&[pattern]).unwrap();
        let track = &review.tracks[0];
        assert_eq!(track.count, 1_000_000);
        assert!(track.exact.is_none());
        assert_eq!(track.buckets.len(), BINS);
        assert_eq!(track.first.ns, 0);
        assert_eq!(track.last.ns, 999_999_000_000);
        assert!((track.min - 0.3).abs() < 1e-8);
        assert!((track.max - 0.7).abs() < 1e-8);
        assert!(track
            .buckets
            .iter()
            .all(|b| (b.min - 0.3).abs() < 1e-8 && (b.max - 0.7).abs() < 1e-8));
        assert!(track.buckets.windows(2).all(|b| b[0].last < b[1].first));
    }

    #[test]
    fn rejected_and_changed_patterns_do_not_reuse_another_preview_or_grow_cache() {
        let context = egui::Context::default();
        let mut cache = PatternReviews::default();
        let hold = |offset: &str| {
            parse_pattern_prompt(&format!(
                "pattern=hold axis=stroke duration=1s frequency=0hz amplitude=0 offset={offset}"
            ))
            .unwrap()
        };
        for offset in 1..=18 {
            let patterns = [hold(&format!("0.{offset:02}"))];
            let _ = context.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    cache.show(ui, &patterns);
                    cache.show(ui, &patterns);
                });
            });
            assert_eq!(cache.entries.len(), offset.min(CACHE_ENTRIES));
            let review = cache.entries.last().unwrap().1.as_ref().unwrap();
            assert_eq!(review.tracks[0].count, 2);
            assert_eq!(review.tracks[0].min, offset as f64 / 100.0);
        }
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                cache.show(ui, &[]);
                cache.show(ui, &[hold("0.5"), hold("0.6")]);
            });
        });
        assert_eq!(cache.entries.len(), CACHE_ENTRIES);
        assert!(PatternReview::build(&[]).is_err());
    }
}
