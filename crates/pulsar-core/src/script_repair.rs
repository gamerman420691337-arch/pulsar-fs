//! Numerical neutral-script repair; not device or observation authority.
use crate::*;

#[derive(Clone, Copy, Debug)]
pub struct ScriptRepairOptions {
    maximum: f64,
    jitter_ns: i64,
    jitter_percent: i64,
}
impl ScriptRepairOptions {
    pub fn new(
        maximum: f64,
        jitter_window_ms: i64,
        jitter_percent: i32,
    ) -> Result<Self, CoreError> {
        validate_segment_speed_limit(maximum)?;
        if jitter_window_ms < 0 || !(0..=100).contains(&jitter_percent) {
            return Err(CoreError::OutOfRange);
        }
        let jitter_ns = jitter_window_ms
            .checked_mul(1_000_000)
            .ok_or(CoreError::Overflow)?;
        Ok(Self {
            maximum,
            jitter_ns,
            jitter_percent: i64::from(jitter_percent),
        })
    }
}
pub struct ScriptRepair {
    pub program: MotionProgram,
    pub removed_jitters: usize,
    pub clamped_positions: usize,
}

/// Retain endpoint times, then bound integer-percent deltas on retained edges.
/// Only standard single-axis, gap-free millisecond/percent input is admitted.
pub fn repair_standard_script(
    program: &MotionProgram,
    options: ScriptRepairOptions,
) -> Result<ScriptRepair, CoreError> {
    let [track] = program.tracks() else {
        return Err(CoreError::InvalidMotion(
            "repair requires one stroke track".into(),
        ));
    };
    if track.axis() != Axis::Stroke
        || !track.gaps().is_empty()
        || track.actions().is_empty()
        || track.actions().len() > EDIT_VALUES_MAX_ACTIONS
    {
        return Err(CoreError::InvalidMotion(
            "repair requires bounded gap-free stroke actions".into(),
        ));
    }
    let mut points = Vec::with_capacity(track.actions().len());
    for a in track.actions() {
        let time = a.time().as_nanos();
        let percent = a.position().value() * 100.0;
        if time < 0 || time % 1_000_000 != 0 || (percent - percent.round()).abs() > 1e-10 {
            return Err(CoreError::InvalidMotion(
                "repair requires standard millisecond/integer-percent grid".into(),
            ));
        }
        points.push((a.time(), percent.round() as i64));
    }
    let mut retained: Vec<(ProjectTime, i64)> = Vec::with_capacity(points.len());
    let mut removed_jitters = 0;
    for (i, point) in points.iter().enumerate() {
        if i > 0 && i + 1 < points.len() {
            let (time, position) = *retained
                .last()
                .ok_or_else(|| CoreError::InvalidMotion("missing repair predecessor".into()))?;
            if point.0.as_nanos() - time.as_nanos() < options.jitter_ns
                && (point.1 - position).abs() <= options.jitter_percent
            {
                removed_jitters += 1;
                continue;
            }
        }
        retained.push(*point);
    }
    let mut clamped_positions = 0;
    for i in 1..retained.len() {
        let dt = (retained[i].0.as_nanos() - retained[i - 1].0.as_nanos()) / 1_000_000;
        if dt <= 0 {
            return Err(CoreError::InvalidMotion("unordered repair input".into()));
        }
        let mut allowed = (options.maximum * dt as f64 / 1000.0).floor().min(100.0) as i64;
        if allowed > 0 && (allowed * 1000) as f64 / dt as f64 > options.maximum {
            allowed -= 1;
        }
        let previous = retained[i - 1].1;
        let next = retained[i].1.clamp(previous - allowed, previous + allowed);
        if next != retained[i].1 {
            clamped_positions += 1;
            retained[i].1 = next;
        }
    }
    let actions = retained
        .into_iter()
        .map(|(time, position)| {
            MotionAction::new(
                time,
                NormalizedPosition::new(position as f64 / 100.0)?,
                EvidenceKind::Synthesized,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ScriptRepair {
        program: MotionProgram::new(vec![MotionTrack::new(Axis::Stroke, actions)?])?,
        removed_jitters,
        clamped_positions,
    })
}
