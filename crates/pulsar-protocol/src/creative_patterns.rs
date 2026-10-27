//! Host constraints on untrusted learned synthetic-motion proposals.
//! This contract grants no generation, project, media or device authority.
use crate::ProtocolError;
use pulsar_core::{Axis, PatternKind, PatternParameters, PatternSpec, ProjectTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreativePatternScope {
    pub axes: Vec<Axis>,
    pub duration: ProjectTime,
    pub sample_hz: u32,
    pub max_amplitude: f64,
}

/// Approval of one retained synthetic proposal; authenticated Engine dispatch
/// must independently check ownership, revision, epoch and fresh context.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantCreativeApproval {
    pub attempt_id: crate::AttemptId,
    pub context_sha256: String,
    pub proposal_sha256: String,
}
impl AssistantCreativeApproval {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::AssistantGenerationApproval {
            attempt_id: self.attempt_id.clone(),
            context_sha256: self.context_sha256.clone(),
            proposal_sha256: self.proposal_sha256.clone(),
        }
        .validate()
    }
    pub fn from_report(report: &crate::AssistantReport) -> Result<Self, ProtocolError> {
        use sha2::{Digest, Sha256};
        report.validate()?;
        if report.state != crate::AssistantState::Completed
            || !matches!(
                report.proposal,
                Some(crate::AssistantProposal::SynthesizePatterns { .. })
            )
        {
            return Err(ProtocolError::invalid(
                "completed creative proposal required",
            ));
        }
        Ok(Self {
            attempt_id: report.attempt_id.clone(),
            context_sha256: report.context_sha256.clone(),
            proposal_sha256: format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&report.proposal)
                        .map_err(|_| ProtocolError::invalid("creative proposal encoding"))?
                )
            ),
        })
    }
}

impl CreativePatternScope {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.axes.is_empty()
            || self.axes.len() > 6
            || self.axes.iter().collect::<BTreeSet<_>>().len() != self.axes.len()
            || !self.max_amplitude.is_finite()
            || !(0.0..=0.5).contains(&self.max_amplitude)
        {
            return Err(ProtocolError::invalid("invalid creative pattern scope"));
        }
        // A sine with zero amplitude checks the worst-case sampled family
        // budget, including a non-lattice endpoint, before native inference.
        let specs = self
            .axes
            .iter()
            .map(|axis| {
                PatternSpec::new(PatternParameters {
                    pattern: PatternKind::Sine,
                    axis: *axis,
                    duration: self.duration,
                    frequency_hz: f64::from(self.sample_hz).min(4.0) / 4.0,
                    amplitude: 0.0,
                    offset: pulsar_core::NormalizedPosition::new(0.5)?,
                    phase_cycles: 0.0,
                    sample_hz: self.sample_hz,
                    duty_cycle: 0.5,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        pulsar_core::validate_patterns(&specs).map_err(|e| ProtocolError::invalid(e.to_string()))
    }

    pub fn validate_proposal(&self, patterns: &[PatternSpec]) -> Result<(), ProtocolError> {
        self.validate()?;
        pulsar_core::validate_patterns(patterns)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if patterns.len() != self.axes.len()
            || patterns.iter().any(|spec| {
                let p = spec.parameters();
                !self.axes.contains(&p.axis)
                    || p.duration != self.duration
                    || p.sample_hz != self.sample_hz
                    || p.amplitude > self.max_amplitude
            })
        {
            return Err(ProtocolError::invalid(
                "creative proposal differs from host constraints",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssistantProposal, AssistantProposalScope};

    fn scope() -> CreativePatternScope {
        CreativePatternScope {
            axes: vec![Axis::Stroke],
            duration: ProjectTime::from_nanos(2_000_000_000),
            sample_hz: 8,
            max_amplitude: 0.1,
        }
    }
    fn body() -> serde_json::Value {
        serde_json::json!({"kind":"synthesize_patterns","patterns":[{"pattern":"triangle","axis":"stroke","duration":2_000_000_000i64,"frequency_hz":1.0,"amplitude":0.1,"offset":0.5,"phase_cycles":0.0,"sample_hz":8,"duty_cycle":0.5}]})
    }
    #[test]
    fn proposals_require_explicit_scope_and_exact_host_constraints() {
        let scope = AssistantProposalScope {
            creative_patterns: Some(scope()),
            ..Default::default()
        };
        let decode = |value: &serde_json::Value| {
            AssistantProposal::decode(&serde_json::to_vec(value).unwrap(), &scope)
        };
        assert!(decode(&body()).is_ok());
        assert!(AssistantProposal::decode(
            &serde_json::to_vec(&body()).unwrap(),
            &AssistantProposalScope::default()
        )
        .is_err());
        for (field, value) in [
            ("axis", serde_json::json!("yaw")),
            ("duration", serde_json::json!(3_000_000_000i64)),
            ("sample_hz", serde_json::json!(16)),
            ("amplitude", serde_json::json!(0.11)),
            ("frequency_hz", serde_json::json!(3)),
            ("offset", serde_json::json!(0.95)),
            ("approved", serde_json::json!(true)),
        ] {
            let mut raw = body();
            raw["patterns"][0][field] = value;
            assert!(decode(&raw).is_err(), "{field}");
        }
        let mut raw = body();
        raw["patterns"] = serde_json::json!([]);
        assert!(decode(&raw).is_err());
        let mut raw = body();
        let duplicate = raw["patterns"][0].clone();
        raw["patterns"].as_array_mut().unwrap().push(duplicate);
        assert!(decode(&raw).is_err());
    }
    #[test]
    fn host_scope_refuses_duplicate_axes_invalid_numbers_and_aggregate_budget() {
        for update in 0..6 {
            let mut s = scope();
            match update {
                0 => s.axes.clear(),
                1 => s.axes.push(Axis::Stroke),
                2 => s.max_amplitude = f64::NAN,
                3 => s.max_amplitude = 0.51,
                4 => s.sample_hz = 0,
                _ => {
                    s.axes = vec![
                        Axis::Stroke,
                        Axis::Sway,
                        Axis::Surge,
                        Axis::Roll,
                        Axis::Pitch,
                        Axis::Yaw,
                    ];
                    s.duration = ProjectTime::from_nanos(20_000_000_000_000);
                    s.sample_hz = 10;
                }
            }
            assert!(s.validate().is_err());
        }
        let mut s = scope();
        s.axes = vec![
            Axis::Stroke,
            Axis::Sway,
            Axis::Surge,
            Axis::Roll,
            Axis::Pitch,
            Axis::Yaw,
        ];
        s.validate().unwrap();
    }
}
