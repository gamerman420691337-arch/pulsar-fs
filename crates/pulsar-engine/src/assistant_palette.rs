//! Exact native creative choices shared by worker grammar and decoder.
use pulsar_protocol::{CreativePatternScope, ProtocolError};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct HoldPalette {
    pub(crate) frequency_hz: f64,
    pub(crate) amplitude: f64,
    pub(crate) phase_cycles: f64,
    pub(crate) duty_cycle: f64,
}
#[derive(Serialize)]
pub(crate) struct PulsePalette {
    pub(crate) duty_cycle: f64,
    pub(crate) frequencies_hz: Vec<f64>,
}
#[derive(Serialize)]
pub(crate) struct CreativePalette {
    pub(crate) patterns: [&'static str; 4],
    pub(crate) frequencies_hz: Vec<f64>,
    pub(crate) amplitudes: [f64; 3],
    pub(crate) phase_cycles: [f64; 4],
    pub(crate) offset: f64,
    pub(crate) hold: HoldPalette,
    pub(crate) pulse: Vec<PulsePalette>,
}
impl CreativePalette {
    pub(crate) fn new(scope: &CreativePatternScope) -> Result<Self, ProtocolError> {
        scope.validate()?;
        let frequencies_hz = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 50.0]
            .into_iter()
            .filter(|f| *f <= (f64::from(scope.sample_hz) / 4.0).min(50.0))
            .collect::<Vec<_>>();
        let pulse = [0.25f64, 0.5, 0.75]
            .into_iter()
            .filter_map(|duty_cycle| {
                let allowed = frequencies_hz
                    .iter()
                    .copied()
                    .filter(|f| {
                        f64::from(scope.sample_hz) / *f * duty_cycle.min(1.0 - duty_cycle) >= 2.0
                    })
                    .collect::<Vec<_>>();
                (!allowed.is_empty()).then_some(PulsePalette {
                    duty_cycle,
                    frequencies_hz: allowed,
                })
            })
            .collect();
        Ok(Self {
            patterns: ["sine", "triangle", "hold", "pulse"],
            frequencies_hz,
            amplitudes: [0.0, scope.max_amplitude / 2.0, scope.max_amplitude],
            phase_cycles: [0.0, 0.25, 0.5, 0.75],
            offset: 0.5,
            hold: HoldPalette {
                frequency_hz: 0.0,
                amplitude: 0.0,
                phase_cycles: 0.0,
                duty_cycle: 0.5,
            },
            pulse,
        })
    }
    pub(crate) fn validate_patterns(
        &self,
        patterns: &[pulsar_core::PatternSpec],
    ) -> Result<(), ProtocolError> {
        use pulsar_core::PatternKind;
        for spec in patterns {
            let p = spec.parameters();
            let offered = p.offset.value() == self.offset
                && match p.pattern {
                    PatternKind::Hold => {
                        p.frequency_hz == self.hold.frequency_hz
                            && p.amplitude == self.hold.amplitude
                            && p.phase_cycles == self.hold.phase_cycles
                            && p.duty_cycle == self.hold.duty_cycle
                    }
                    PatternKind::Sine | PatternKind::Triangle => {
                        self.frequencies_hz.contains(&p.frequency_hz)
                            && self.amplitudes.contains(&p.amplitude)
                            && self.phase_cycles.contains(&p.phase_cycles)
                            && p.duty_cycle == self.hold.duty_cycle
                    }
                    PatternKind::Pulse => {
                        self.amplitudes.contains(&p.amplitude)
                            && self.phase_cycles.contains(&p.phase_cycles)
                            && self.pulse.iter().any(|pulse| {
                                pulse.duty_cycle == p.duty_cycle
                                    && pulse.frequencies_hz.contains(&p.frequency_hz)
                            })
                    }
                };
            if !offered {
                return Err(ProtocolError::invalid(
                    "native creative completion uses an unoffered choice",
                ));
            }
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn choices(&self, kind: &str) -> Vec<(f64, Vec<f64>)> {
        if kind == "pulse" {
            self.pulse
                .iter()
                .map(|p| (p.duty_cycle, p.frequencies_hz.clone()))
                .collect()
        } else {
            vec![(self.hold.duty_cycle, self.frequencies_hz.clone())]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::{Axis, ProjectTime};
    fn scope(sample_hz: u32) -> CreativePatternScope {
        CreativePatternScope {
            axes: vec![Axis::Stroke],
            duration: ProjectTime::from_nanos(2_000_000_000),
            sample_hz,
            max_amplitude: 0.1,
        }
    }
    #[test]
    fn admitted_choices_match_authored_sampling_and_pulse_boundaries() {
        let standard = CreativePalette::new(&scope(8)).unwrap();
        assert_eq!(standard.frequencies_hz, [0.25, 0.5, 1.0, 2.0]);
        assert_eq!(standard.amplitudes, [0.0, 0.05, 0.1]);
        assert_eq!(
            standard
                .pulse
                .iter()
                .map(|p| (p.duty_cycle, p.frequencies_hz.clone()))
                .collect::<Vec<_>>(),
            vec![
                (0.25, vec![0.25, 0.5, 1.0]),
                (0.5, vec![0.25, 0.5, 1.0, 2.0]),
                (0.75, vec![0.25, 0.5, 1.0])
            ]
        );
        let slow = CreativePalette::new(&scope(1)).unwrap();
        assert_eq!(slow.frequencies_hz, [0.25]);
        assert_eq!(slow.pulse.len(), 1);
        assert_eq!(slow.pulse[0].duty_cycle, 0.5);
        assert_eq!(slow.pulse[0].frequencies_hz, [0.25]);
        let fast = CreativePalette::new(&scope(1000)).unwrap();
        assert_eq!(fast.frequencies_hz.last(), Some(&50.0));
        assert_eq!(fast.pulse.len(), 3);
        assert!(fast
            .pulse
            .iter()
            .all(|p| p.frequencies_hz == fast.frequencies_hz));
    }
    #[test]
    fn invalid_scope_cannot_produce_capability_facts() {
        for changed in 0..3 {
            let mut value = scope(8);
            match changed {
                0 => value.sample_hz = 0,
                1 => value.max_amplitude = f64::NAN,
                _ => value.axes.push(Axis::Stroke),
            }
            assert!(CreativePalette::new(&value).is_err());
        }
    }
}
