//! Exact placed-source sample scheduling, independent of decoding/perception.
use crate::{
    CoreError, ExactSeconds, ExactTimeQuantization, ProjectTime, SourcePlacement, SourceTimestamp,
    TimeRange,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedGenerationSample {
    pub source_frame_index: u64,
    pub source_time: SourceTimestamp,
    pub elapsed_source_time: ExactSeconds,
    pub project_time: ExactSeconds,
    pub quantization: ExactTimeQuantization,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedGenerationPlan {
    pub placement: SourcePlacement,
    pub source_origin: SourceTimestamp,
    /// Natural source/decode order. Reverse playback does not change source identity.
    pub samples: Vec<PlacedGenerationSample>,
    /// Indices into samples in strictly increasing quantized project time.
    pub project_order: Vec<usize>,
    pub project_trim: TimeRange,
    /// No decoded support is asserted between the trim edge and first/last sample.
    pub uncovered_boundaries: Vec<TimeRange>,
}
fn invalid(message: &str) -> CoreError {
    CoreError::InvalidSourceSeek(message.into())
}

/// Select stride-aligned original frame ordinals inside the half-open source
/// trim and map them exactly. The caller supplies its admitted sample bound.
/// Negative project coverage and colliding nanosecond samples are refused;
/// neither clipping nor sample deletion is implicit.
pub fn plan_placed_generation(
    placement: &SourcePlacement,
    pts: &[i64],
    time_base_num: u32,
    time_base_den: u32,
    stride: u32,
    max_samples: usize,
) -> Result<PlacedGenerationPlan, CoreError> {
    placement.spec.validate()?;
    if pts.is_empty()
        || time_base_num == 0
        || time_base_den == 0
        || stride == 0
        || max_samples == 0
        || pts.windows(2).any(|p| p[0] >= p[1])
    {
        return Err(invalid(
            "invalid placed-generation source index, stride or admission",
        ));
    }
    let source_timestamp = |ticks: i64| -> Result<SourceTimestamp, CoreError> {
        SourceTimestamp::new(
            ticks
                .checked_mul(i64::from(time_base_num))
                .ok_or(CoreError::Overflow)?,
            time_base_den,
        )
    };
    let source_origin = source_timestamp(pts[0])?;
    let source_start = ExactSeconds::from_source(placement.spec.source_start);
    let source_end = ExactSeconds::from_source(placement.spec.source_end);
    let a = placement.spec.project_at_source_boundary(source_start)?;
    let b = placement.spec.project_at_source_boundary(source_end)?;
    let (low, high) = if a.checked_cmp(b)? == Ordering::Less {
        (a, b)
    } else {
        (b, a)
    };
    if low.checked_cmp(ExactSeconds::from_project(ProjectTime::ZERO))? == Ordering::Less {
        return Err(invalid(
            "placed generation trim extends into negative motion time",
        ));
    }
    let low_q = low.quantize_nanoseconds()?;
    let high_q = high.quantize_nanoseconds()?;
    // Forward source end is exclusive. Reverse source start is inclusive, so
    // its quantized sample needs one nanosecond of representable coverage.
    let upper = if placement.spec.rate_numerator < 0 || high_q.remainder_numerator != "0" {
        high_q.time.checked_add(1)?
    } else {
        high_q.time
    };
    let project_trim = TimeRange::new(low_q.time, upper)?;
    let mut samples = Vec::new();
    for (ordinal, &tick) in pts.iter().enumerate() {
        // Validate every actual source timestamp, including omitted frames.
        let source_time = source_timestamp(tick)?;
        if ordinal % stride as usize != 0 {
            continue;
        }
        let elapsed = ExactSeconds::new(
            (i128::from(tick) - i128::from(pts[0]))
                .checked_mul(i128::from(time_base_num))
                .ok_or(CoreError::Overflow)?,
            u64::from(time_base_den),
        )?;
        if elapsed.checked_cmp(source_start)? == Ordering::Less
            || elapsed.checked_cmp(source_end)? != Ordering::Less
        {
            continue;
        }
        if samples.len() >= max_samples {
            return Err(invalid("placed generation exceeds admitted sample count"));
        }
        let project_time = placement.spec.project_at_source(elapsed)?;
        let quantization = project_time.quantize_nanoseconds()?;
        samples.push(PlacedGenerationSample {
            source_frame_index: u64::try_from(ordinal).map_err(|_| CoreError::Overflow)?,
            source_time,
            elapsed_source_time: elapsed,
            project_time,
            quantization,
        });
    }
    if samples.is_empty() {
        return Err(invalid("placement trim contains no selected source frame"));
    }
    let mut project_order: Vec<_> = (0..samples.len()).collect();
    if placement.spec.rate_numerator < 0 {
        project_order.reverse();
    }
    if project_order
        .windows(2)
        .any(|p| samples[p[0]].quantization.time >= samples[p[1]].quantization.time)
    {
        return Err(invalid(
            "placed samples collide at motion nanosecond precision",
        ));
    }
    let first = samples[project_order[0]].quantization.time;
    let last = samples[*project_order
        .last()
        .ok_or_else(|| invalid("missing placed sample order"))?]
    .quantization
    .time;
    if !project_trim.contains(first) || !project_trim.contains(last) {
        return Err(invalid("placed samples fall outside quantized trim"));
    }
    let mut uncovered_boundaries = Vec::new();
    if project_trim.start() < first {
        uncovered_boundaries.push(TimeRange::new(project_trim.start(), first)?);
    }
    let after_last = last.checked_add(1)?;
    if after_last < project_trim.end() {
        uncovered_boundaries.push(TimeRange::new(after_last, project_trim.end())?);
    }
    Ok(PlacedGenerationPlan {
        placement: placement.clone(),
        source_origin,
        samples,
        project_order,
        project_trim,
        uncovered_boundaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RevisionId, SourcePlacementId, SourcePlacementSpec, SourceVersionId};
    fn placement(rate: i32) -> SourcePlacement {
        SourcePlacement {
            placement_id: SourcePlacementId::new("placement.fixture").unwrap(),
            created_revision: RevisionId::new(2),
            spec: SourcePlacementSpec {
                source_version: SourceVersionId::new("source.fixture").unwrap(),
                source_start: SourceTimestamp::new(1, 8).unwrap(),
                source_end: SourceTimestamp::new(1, 1).unwrap(),
                source_anchor: SourceTimestamp::new(3, 8).unwrap(),
                project_anchor: ProjectTime::from_nanos(6_000_000_001),
                rate_numerator: rate,
                rate_denominator: 13,
            },
        }
    }
    #[test]
    fn placed_sampling_preserves_irregular_source_identity_and_reverse_project_order() {
        let pts = [5000, 5125, 5375, 5500, 5875, 6000];
        for rate in [2, -2] {
            let p = placement(rate);
            let plan = plan_placed_generation(&p, &pts, 1, 1000, 1, 20).unwrap();
            assert_eq!(
                plan.source_origin,
                SourceTimestamp::new(5000, 1000).unwrap()
            );
            assert_eq!(
                plan.samples
                    .iter()
                    .map(|s| s.source_frame_index)
                    .collect::<Vec<_>>(),
                vec![1, 2, 3, 4]
            );
            assert_eq!(
                plan.project_order,
                if rate > 0 {
                    vec![0, 1, 2, 3]
                } else {
                    vec![3, 2, 1, 0]
                }
            );
            for s in &plan.samples {
                assert_eq!(
                    s.project_time,
                    p.spec.project_at_source(s.elapsed_source_time).unwrap()
                );
                assert_eq!(
                    s.quantization,
                    s.project_time.quantize_nanoseconds().unwrap()
                );
                assert!(!plan
                    .uncovered_boundaries
                    .iter()
                    .any(|g| g.contains(s.quantization.time)));
            }
            assert_eq!(plan.uncovered_boundaries.len(), 1);
            let gap = plan.uncovered_boundaries[0];
            if rate > 0 {
                assert_eq!(gap.end(), plan.project_trim.end());
            } else {
                assert_eq!(gap.start(), plan.project_trim.start());
            }
        }
        let sparse = plan_placed_generation(&placement(2), &pts, 1, 1000, 2, 20).unwrap();
        assert_eq!(
            sparse
                .samples
                .iter()
                .map(|s| s.source_frame_index)
                .collect::<Vec<_>>(),
            vec![2, 4]
        );
        assert_eq!(sparse.uncovered_boundaries.len(), 2);
    }
    #[test]
    fn exact_mapping_does_not_quantize_source_time_before_project_mapping() {
        let mut p = placement(1);
        p.spec.source_start = SourceTimestamp::new(0, 1).unwrap();
        p.spec.source_anchor = SourceTimestamp::new(0, 1).unwrap();
        p.spec.rate_denominator = 1;
        let plan = plan_placed_generation(&p, &[15, 16, 17], 1, 3, 1, 3).unwrap();
        assert_eq!(
            plan.samples[1].project_time,
            p.spec
                .project_at_source(ExactSeconds::new(1, 3).unwrap())
                .unwrap()
        );
        assert_eq!(
            plan.samples[1].quantization.remainder_numerator,
            "1000000000"
        );
        assert_eq!(plan.samples[1].quantization.denominator, "3000000000");
    }
    #[test]
    fn generation_refuses_precision_loss_negative_coverage_invalid_indices_and_admission() {
        let p = placement(2);
        assert!(plan_placed_generation(&p, &[5, 5], 1, 1, 1, 2).is_err());
        assert!(plan_placed_generation(&p, &[5, 6], 0, 1, 1, 2).is_err());
        assert!(plan_placed_generation(&p, &[5, 6], 1, 1, 0, 2).is_err());
        assert!(plan_placed_generation(&p, &[5000, 5125, 5375], 1, 1000, 1, 1).is_err());
        assert!(plan_placed_generation(&p, &[5000, 5001], 1, 1000, 1, 2).is_err());
        let mut negative = p.clone();
        negative.spec.project_anchor = ProjectTime::from_nanos(0);
        assert!(plan_placed_generation(&negative, &[5000, 5125, 5375], 1, 1000, 1, 3).is_err());
        let mut collision = p;
        collision.spec.source_start = SourceTimestamp::new(0, 1).unwrap();
        collision.spec.source_anchor = SourceTimestamp::new(0, 1).unwrap();
        collision.spec.rate_numerator = i32::MAX;
        collision.spec.rate_denominator = 1;
        assert!(plan_placed_generation(&collision, &[0, 1, 2], 1, 1_000_000_000, 1, 3).is_err());
        assert!(plan_placed_generation(&collision, &[i64::MAX - 1, i64::MAX], 2, 1, 1, 3).is_err());
    }
}
