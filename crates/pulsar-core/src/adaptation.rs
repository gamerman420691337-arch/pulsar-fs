//! Timing-preserving physical mapping, without device authority or transport.
//! Each segment is a rest-to-rest quintic: s(u)=10u^3-15u^4+6u^5.
//! Its analytic velocity/acceleration/jerk bounds apply between actions, too.
use crate::{Axis, CoreError, EvidenceKind, MotionProgram, ProjectTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisDynamics {
    pub axis: Axis,
    /// Physical units selected by this profile (e.g. mm or radians).
    pub unit: String,
    pub minimum: f64,
    pub maximum: f64,
    pub max_velocity: f64,
    pub max_acceleration: f64,
    pub max_jerk: f64,
}

/// Weighted L1 envelope about each axis's usable-range midpoint. Coefficients
/// explicitly convert the axes' physical units into this constraint's units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoupledDynamics {
    pub coefficients: Vec<(Axis, f64)>,
    pub max_displacement: f64,
    pub max_velocity: f64,
    pub max_acceleration: f64,
    pub max_jerk: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicsProfile {
    pub axes: Vec<AxisDynamics>,
    pub coupled: Vec<CoupledDynamics>,
    /// A user-selected lower bound; infeasible adaptation is an error.
    /// No universal acceptable compression is assumed by the implementation.
    pub minimum_gain: f64,
}

impl DynamicsProfile {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.axes.is_empty() || self.axes.len() > 6 || self.coupled.len() > 64 {
            return Err(invalid(
                "profile requires 1..=6 axes and at most 64 coupled constraints",
            ));
        }
        if !self.minimum_gain.is_finite() || !(0.0..=1.0).contains(&self.minimum_gain) {
            return Err(invalid("minimum gain must be finite and within [0,1]"));
        }
        let mut axes = BTreeSet::new();
        for a in &self.axes {
            if !axes.insert(a.axis)
                || a.unit.trim().is_empty()
                || a.unit.len() > 64
                || a.unit.chars().any(char::is_control)
            {
                return Err(invalid("duplicate axis or invalid physical unit"));
            }
            if !a.minimum.is_finite()
                || !a.maximum.is_finite()
                || a.minimum >= a.maximum
                || !(a.maximum - a.minimum).is_finite()
            {
                return Err(invalid(
                    "physical range must be finite, ordered and representable",
                ));
            }
            positive(&[a.max_velocity, a.max_acceleration, a.max_jerk])?;
        }
        for c in &self.coupled {
            positive(&[
                c.max_displacement,
                c.max_velocity,
                c.max_acceleration,
                c.max_jerk,
            ])?;
            if c.coefficients.is_empty() || c.coefficients.len() > 6 {
                return Err(invalid("coupled constraint requires 1..=6 coefficients"));
            }
            let mut seen = BTreeSet::new();
            for (axis, weight) in &c.coefficients {
                if !axes.contains(axis)
                    || !seen.insert(*axis)
                    || !weight.is_finite()
                    || *weight == 0.0
                {
                    return Err(invalid("coupled coefficients must be nonzero, finite, unique and name configured axes"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhysicalAction {
    pub time: ProjectTime,
    pub position: f64,
    pub evidence: EvidenceKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct DynamicsBounds {
    pub displacement: f64,
    pub velocity: f64,
    pub acceleration: f64,
    pub jerk: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PhysicalTrack {
    pub axis: Axis,
    pub unit: String,
    pub actions: Vec<PhysicalAction>,
    pub bounds: DynamicsBounds,
}

/// This is an analytic motion proposal, never qualification or admission.
/// Entry must occur stopped at the first position; entry/exit transport is not
/// modeled. A driver must execute the quintic curve, not linearly interpolate.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AdaptedMotion {
    pub interpolation: &'static str,
    pub requires_stopped_entry: bool,
    pub gain: f64,
    pub amplitude_reduced: bool,
    pub tracks: Vec<PhysicalTrack>,
    pub coupled_bounds: Vec<DynamicsBounds>,
}

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidMotion(message.into())
}
fn positive(values: &[f64]) -> Result<(), CoreError> {
    if values.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(invalid("dynamics limits must be positive and finite"));
    }
    Ok(())
}
fn finite(b: DynamicsBounds) -> Result<DynamicsBounds, CoreError> {
    if [b.displacement, b.velocity, b.acceleration, b.jerk]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(invalid("dynamics computation exceeds representable range"));
    }
    Ok(b)
}
fn scaled(b: DynamicsBounds, gain: f64) -> DynamicsBounds {
    DynamicsBounds {
        displacement: b.displacement * gain,
        velocity: b.velocity * gain,
        acceleration: b.acceleration * gain,
        jerk: b.jerk * gain,
    }
}
fn constrain(gain: &mut f64, actual: f64, limit: f64) {
    if actual > limit {
        *gain = gain.min(limit / actual);
    }
}

pub fn adapt_device_motion(
    program: &MotionProgram,
    profile: &DynamicsProfile,
) -> Result<AdaptedMotion, CoreError> {
    profile.validate()?;
    if program.tracks().is_empty() || program.has_unresolved_gaps() {
        return Err(invalid(
            "adaptation requires nonempty motion with resolved gaps",
        ));
    }
    let mut bounds = Vec::new();
    let mut gain = 1.0;
    for track in program.tracks() {
        let p = profile
            .axes
            .iter()
            .find(|p| p.axis == track.axis())
            .ok_or_else(|| invalid("motion axis has no device mapping"))?;
        if track.actions().is_empty() {
            return Err(invalid("cannot adapt an unavailable axis"));
        }
        let range = p.maximum - p.minimum;
        let mut b = DynamicsBounds::default();
        for action in track.actions() {
            b.displacement = b
                .displacement
                .max((action.position().value() - 0.5).abs() * range);
        }
        for pair in track.actions().windows(2) {
            let seconds = pair[1].time().checked_sub(pair[0].time())? as f64 * 1e-9;
            let distance = (pair[1].position().value() - pair[0].position().value()).abs() * range;
            b.velocity = b.velocity.max(1.875 * distance / seconds);
            // 6 is a conservative upper bound on 10*sqrt(3)/3.
            b.acceleration = b.acceleration.max(6.0 * distance / seconds / seconds);
            b.jerk = b.jerk.max(60.0 * distance / seconds / seconds / seconds);
        }
        b = finite(b)?;
        constrain(&mut gain, b.velocity, p.max_velocity);
        constrain(&mut gain, b.acceleration, p.max_acceleration);
        constrain(&mut gain, b.jerk, p.max_jerk);
        bounds.push((track.axis(), b));
    }
    let mut coupled_bounds = Vec::new();
    for c in &profile.coupled {
        let mut b = DynamicsBounds::default();
        for (axis, weight) in &c.coefficients {
            let a = bounds
                .iter()
                .find(|(a, _)| a == axis)
                .map(|(_, b)| *b)
                .ok_or_else(|| {
                    invalid("coupled axis is unavailable in motion; cannot assume a position")
                })?;
            let a = scaled(a, weight.abs());
            b.displacement += a.displacement;
            b.velocity += a.velocity;
            b.acceleration += a.acceleration;
            b.jerk += a.jerk;
        }
        b = finite(b)?;
        constrain(&mut gain, b.displacement, c.max_displacement);
        constrain(&mut gain, b.velocity, c.max_velocity);
        constrain(&mut gain, b.acceleration, c.max_acceleration);
        constrain(&mut gain, b.jerk, c.max_jerk);
        coupled_bounds.push(b);
    }
    // Leave numerical headroom whenever a constraint caused compression.
    if gain < 1.0 {
        gain *= 1.0 - 1e-12;
    }
    if gain <= 0.0 || gain < profile.minimum_gain {
        return Err(invalid(
            "motion is infeasible within the selected minimum amplitude gain",
        ));
    }
    let mut tracks = Vec::new();
    for track in program.tracks() {
        let p = profile
            .axes
            .iter()
            .find(|p| p.axis == track.axis())
            .expect("validated mapping");
        let range = p.maximum - p.minimum;
        // Avoid overflowing minimum+maximum for large finite coordinates.
        let midpoint = p.minimum + range * 0.5;
        let actions = track
            .actions()
            .iter()
            .map(|a| PhysicalAction {
                time: a.time(),
                position: midpoint + (a.position().value() - 0.5) * range * gain,
                evidence: a.evidence(),
            })
            .collect::<Vec<_>>();
        if actions
            .iter()
            .any(|a| !a.position.is_finite() || a.position < p.minimum || a.position > p.maximum)
        {
            return Err(invalid(
                "mapped physical position is not representable within travel",
            ));
        }
        if track
            .actions()
            .windows(2)
            .zip(actions.windows(2))
            .any(|(neutral, physical)| {
                neutral[0].position() != neutral[1].position()
                    && physical[0].position == physical[1].position
            })
        {
            return Err(invalid(
                "nonzero motion collapses below physical coordinate precision",
            ));
        }
        tracks.push(PhysicalTrack {
            axis: track.axis(),
            unit: p.unit.clone(),
            actions,
            bounds: scaled(
                bounds.iter().find(|(a, _)| *a == track.axis()).unwrap().1,
                gain,
            ),
        });
    }
    Ok(AdaptedMotion {
        interpolation: "rest_to_rest_quintic",
        requires_stopped_entry: true,
        gain,
        amplitude_reduced: gain < 1.0,
        tracks,
        coupled_bounds: coupled_bounds
            .into_iter()
            .map(|b| scaled(b, gain))
            .collect(),
    })
}

/// Evaluate a physical segment using the same curve the bounds certify.
pub fn quintic_position(from: f64, to: f64, fraction: f64) -> Result<f64, CoreError> {
    if !from.is_finite()
        || !to.is_finite()
        || !fraction.is_finite()
        || !(0.0..=1.0).contains(&fraction)
    {
        return Err(invalid("invalid physical interpolation arguments"));
    }
    let u = fraction;
    let s = (u * u * u * (10.0 + u * (-15.0 + 6.0 * u))).clamp(0.0, 1.0);
    let value = from * (1.0 - s) + to * s;
    if !value.is_finite() {
        return Err(CoreError::Overflow);
    }
    Ok(value)
}
