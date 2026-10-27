//! Project-time selection on the original sampled source lattice.
use crate::{CoreError, ExactSeconds, PlacedGenerationPlan, ProjectTime, TimeRange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationIntervalSample {
    pub source_frame_index: u64,
    pub project_time: ProjectTime,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationIntervalPlan {
    pub interval: TimeRange,
    /// Original source/decode order, including reverse placement.
    pub samples: Vec<GenerationIntervalSample>,
    /// Indices into samples, in increasing project time.
    pub project_order: Vec<usize>,
    pub uncovered_boundaries: Vec<TimeRange>,
}

/// Select half-open project coverage without shifting clocks or resetting the
/// source stride. Boundary coverage without an actual sample remains unavailable.
pub fn plan_generation_interval(
    pts: &[i64],
    numerator: u32,
    denominator: u32,
    stride: u32,
    placed: Option<&PlacedGenerationPlan>,
    interval: TimeRange,
    limit: usize,
) -> Result<GenerationIntervalPlan, CoreError> {
    let invalid =
        || CoreError::InvalidMotion("invalid generation interval or source lattice".into());
    if pts.is_empty()
        || numerator == 0
        || denominator == 0
        || stride == 0
        || limit == 0
        || interval.start() < ProjectTime::ZERO
        || pts.windows(2).any(|p| p[0] >= p[1])
    {
        return Err(invalid());
    }
    let mut samples = Vec::new();
    for ordinal in (0..pts.len()).step_by(stride as usize) {
        let time = if let Some(plan) = placed {
            let Ok(index) = plan
                .samples
                .binary_search_by_key(&(ordinal as u64), |s| s.source_frame_index)
            else {
                continue;
            };
            plan.samples[index].quantization.time
        } else {
            ExactSeconds::new(
                (i128::from(pts[ordinal]) - i128::from(pts[0]))
                    .checked_mul(i128::from(numerator))
                    .ok_or(CoreError::Overflow)?,
                u64::from(denominator),
            )?
            .quantize_nanoseconds()?
            .time
        };
        if !interval.contains(time) {
            continue;
        }
        if samples.len() == limit {
            return Err(CoreError::InvalidMotion(
                "generation interval exceeds admitted sample count".into(),
            ));
        }
        samples.push(GenerationIntervalSample {
            source_frame_index: ordinal as u64,
            project_time: time,
        });
    }
    if samples.is_empty() {
        return Err(CoreError::InvalidMotion(
            "generation interval contains no sampled decoded support".into(),
        ));
    }
    let mut project_order: Vec<_> = (0..samples.len()).collect();
    project_order.sort_by_key(|&index| samples[index].project_time);
    if project_order
        .windows(2)
        .any(|p| samples[p[0]].project_time >= samples[p[1]].project_time)
    {
        return Err(invalid());
    }
    let first = samples[project_order[0]].project_time;
    let last = samples[*project_order.last().ok_or_else(invalid)?].project_time;
    let mut uncovered_boundaries = Vec::new();
    if interval.start() < first {
        uncovered_boundaries.push(TimeRange::new(interval.start(), first)?);
    }
    let after_last = last.checked_add(1)?;
    if after_last < interval.end() {
        uncovered_boundaries.push(TimeRange::new(after_last, interval.end())?);
    }
    Ok(GenerationIntervalPlan {
        interval,
        samples,
        project_order,
        uncovered_boundaries,
    })
}
