//! Exact local Stroke sample encoding. No dynamics, hosting or playback authority.
use pulsar_protocol::*;
use std::io::{self, Write};

struct Output {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Output {
    fn write(&mut self, part: &[u8]) -> io::Result<usize> {
        if part.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("Handy script exceeds local output limit"));
        }
        self.bytes.extend_from_slice(part);
        Ok(part.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn prepare(
    program: &MotionProgram,
) -> Result<(Vec<u8>, HandyStrokeQuantization), ProtocolError> {
    encode(program, HANDY_STROKE_SCRIPT_MAX_BYTES as usize)
}
fn encode(
    program: &MotionProgram,
    limit: usize,
) -> Result<(Vec<u8>, HandyStrokeQuantization), ProtocolError> {
    let track = program
        .track(Axis::Stroke)
        .ok_or_else(|| ProtocolError::invalid("Handy preparation requires Stroke motion"))?;
    if !track.gaps().is_empty() {
        return Err(ProtocolError::unsupported(
            "Handy preparation requires explicit resolution of Stroke evidence gaps",
        ));
    }
    let count = track.actions().len() as u64;
    if !(2..=HANDY_STROKE_SCRIPT_MAX_ACTIONS).contains(&count) {
        return Err(ProtocolError::invalid(
            "Handy preparation requires 2..=1000000 Stroke actions",
        ));
    }
    let mut output = Output {
        bytes: Vec::new(),
        limit,
    };
    let resource = |_| {
        ProtocolError::new(
            ErrorCode::ResourceExhausted,
            "Handy script exceeds local output limit",
        )
    };
    output
        .write_all(b"# Pulsar stroke samples v1\n")
        .map_err(resource)?;
    let mut summary = HandyStrokeQuantization {
        action_count: count,
        first_time_ms: 0,
        last_time_ms: 0,
        max_time_error_ns: 0,
        max_position_error_ppm: 0,
        omitted_axes: program
            .tracks()
            .iter()
            .map(|t| t.axis())
            .filter(|a| *a != Axis::Stroke)
            .collect(),
    };
    let mut last = None;
    for action in track.actions() {
        let ns = u64::try_from(action.time().as_nanos()).map_err(|_| {
            ProtocolError::invalid("Handy script cannot encode negative project time")
        })?;
        let ms = ns
            .checked_add(500_000)
            .ok_or_else(|| ProtocolError::invalid("Handy timestamp overflow"))?
            / 1_000_000;
        if ms > (1u64 << 53) - 1 || last.is_some_and(|v| v >= ms) {
            return Err(ProtocolError::invalid(
                "Handy millisecond quantization merges actions; explicit resampling required",
            ));
        }
        let pos = (action.position().value() * 100.0).round() as u32;
        let position_error = ((action.position().value() - f64::from(pos) / 100.0).abs()
            * 1_000_000.0)
            .ceil() as u32;
        summary.max_position_error_ppm = summary.max_position_error_ppm.max(position_error);
        summary.max_time_error_ns = summary.max_time_error_ns.max(ns.abs_diff(ms * 1_000_000));
        if last.is_none() {
            summary.first_time_ms = ms;
        }
        summary.last_time_ms = ms;
        last = Some(ms);
        writeln!(output, "{ms},{pos}").map_err(resource)?;
    }
    summary.validate()?;
    Ok((output.bytes, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action(ns: i64, p: f64) -> MotionAction {
        MotionAction::new(
            ProjectTime::from_nanos(ns),
            NormalizedPosition::new(p).unwrap(),
            EvidenceKind::Synthesized,
        )
        .unwrap()
    }
    fn program(points: &[(i64, f64)]) -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            points.iter().map(|(t, p)| action(*t, *p)).collect(),
        )
        .unwrap()])
        .unwrap()
    }
    #[test]
    fn handy_script_preserves_ordered_samples_and_reports_rounding() {
        let p = program(&[(499_999, 0.0), (1_500_000, 0.125), (4_400_000, 1.0)]);
        let (bytes, q) = prepare(&p).unwrap();
        assert_eq!(bytes, b"# Pulsar stroke samples v1\n0,0\n2,13\n4,100\n");
        assert_eq!(q.max_time_error_ns, 500_000);
        assert!((5_000..=5_001).contains(&q.max_position_error_ppm));
        assert_eq!((q.first_time_ms, q.last_time_ms, q.action_count), (0, 4, 3));
        let before = serde_json::to_vec(&p).unwrap();
        prepare(&p).unwrap();
        assert_eq!(serde_json::to_vec(&p).unwrap(), before);
    }
    #[test]
    fn handy_script_refuses_gaps_collisions_negative_time_and_small_output_budget() {
        assert!(MotionAction::new(
            ProjectTime::from_nanos(-1),
            NormalizedPosition::new(0.0).unwrap(),
            EvidenceKind::Synthesized
        )
        .is_err());
        for p in [
            MotionProgram::default(),
            program(&[]),
            program(&[(0, 0.5)]),
            program(&[(0, 0.0), (499_999, 1.0)]),
        ] {
            assert!(prepare(&p).is_err());
        }
        let gap = MotionTrack::with_gaps(
            Axis::Stroke,
            vec![action(0, 0.0), action(10_000_000, 1.0)],
            vec![TimeRange::new(
                ProjectTime::from_nanos(1_000_000),
                ProjectTime::from_nanos(2_000_000),
            )
            .unwrap()],
        )
        .unwrap();
        assert!(prepare(&MotionProgram::new(vec![gap]).unwrap()).is_err());
        assert_eq!(
            encode(&program(&[(0, 0.0), (1_000_000, 1.0)]), 30)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn handy_script_selects_stroke_explicitly_and_discloses_other_axes() {
        let p = MotionProgram::new(vec![
            MotionTrack::new(Axis::Stroke, vec![action(0, 0.0), action(2_000_000, 1.0)]).unwrap(),
            MotionTrack::new(Axis::Roll, vec![action(0, 0.7)]).unwrap(),
        ])
        .unwrap();
        let (bytes, q) = prepare(&p).unwrap();
        assert_eq!(q.omitted_axes, vec![Axis::Roll]);
        assert_eq!(bytes, b"# Pulsar stroke samples v1\n0,0\n2,100\n");
        let (_, q) = prepare(&program(&[(i64::MAX - 2_000_000, 0.0), (i64::MAX, 1.0)])).unwrap();
        assert_eq!(q.last_time_ms, 9_223_372_036_855);
        assert!(q.max_time_error_ns <= 500_000);
    }
}
