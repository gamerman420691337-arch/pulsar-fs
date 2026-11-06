//! One declared, unit-rate source/project/presentation correspondence.
//! These anchors do not attest clock synchronization or authorize dispatch.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePresentationClock {
    pub epoch: AttemptId,
    pub source_anchor: SourceTimestamp,
    pub project_anchor: ProjectTime,
    /// Optional exact seconds at the source anchor; the integer anchor is its floor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_project_anchor: Option<ExactSeconds>,
    pub presentation_anchor: LiveClockTick,
    pub added_buffer_ns: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePresentationMapping {
    pub source_time: SourceTimestamp,
    pub project: ExactTimeQuantization,
    pub presentation_tick: LiveClockTick,
    pub presentation_remainder_numerator: String,
    pub presentation_remainder_denominator: String,
    pub added_buffer_ns: u64,
}
impl LivePresentationClock {
    pub fn validate(&self) -> Result<(), CoreError> {
        if let Some(exact) = self.exact_project_anchor {
            if exact.quantize_nanoseconds()?.time != self.project_anchor {
                return Err(CoreError::InvalidTransition(
                    "live exact project anchor differs from its integer floor".into(),
                ));
            }
        }
        if self.added_buffer_ns > 1_000_000_000 {
            return Err(CoreError::InvalidTransition(
                "live presentation buffer exceeds one second".into(),
            ));
        }
        self.presentation_anchor
            .0
            .checked_add(self.added_buffer_ns)
            .ok_or(CoreError::Overflow)?;
        Ok(())
    }
    /// Exact project seconds. Quantize only the final mapping, never a rebase.
    pub fn project_at_source(
        &self,
        source_time: SourceTimestamp,
    ) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        self.exact_project_anchor
            .unwrap_or_else(|| ExactSeconds::from_project(self.project_anchor))
            .checked_add(
                ExactSeconds::from_source(source_time)
                    .checked_sub(ExactSeconds::from_source(self.source_anchor))?,
            )
    }
    pub fn map(&self, source_time: SourceTimestamp) -> Result<LivePresentationMapping, CoreError> {
        self.validate()?;
        let delta = ExactSeconds::from_source(source_time)
            .checked_sub(ExactSeconds::from_source(self.source_anchor))?;
        let elapsed = delta.quantize_nanoseconds()?;
        // Floor the complete project expression once. Never add a previously
        // rounded source origin or interval to project time.
        let project = self
            .project_at_source(source_time)?
            .quantize_nanoseconds()?;
        let presentation = i128::from(self.presentation_anchor.0)
            .checked_add(i128::from(elapsed.time.as_nanos()))
            .and_then(|v| v.checked_add(i128::from(self.added_buffer_ns)))
            .ok_or(CoreError::Overflow)?;
        Ok(LivePresentationMapping {
            source_time,
            project,
            presentation_tick: LiveClockTick(
                u64::try_from(presentation).map_err(|_| CoreError::Overflow)?,
            ),
            presentation_remainder_numerator: elapsed.remainder_numerator,
            presentation_remainder_denominator: elapsed.denominator,
            added_buffer_ns: self.added_buffer_ns,
        })
    }
}
impl LivePresentationMapping {
    /// This scheduling lateness is independent of capture-to-analysis age and
    /// physical response. Future presentation returns zero, not a negative age.
    pub fn lateness_at(&self, now: LiveClockTick) -> u64 {
        now.0.saturating_sub(self.presentation_tick.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn clock() -> LivePresentationClock {
        LivePresentationClock {
            epoch: AttemptId::new("epoch-one").unwrap(),
            source_anchor: SourceTimestamp::new(-1, 3).unwrap(),
            project_anchor: ProjectTime::from_nanos(10),
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(20),
            added_buffer_ns: 1_000_000_000,
        }
    }
    #[test]
    fn project_and_buffered_presentation_use_exact_signed_source_difference() {
        let c = clock();
        let m = c.map(SourceTimestamp::new(0, 1).unwrap()).unwrap();
        assert_eq!(m.project.time, ProjectTime::from_nanos(333_333_343));
        assert_eq!(
            ExactSeconds::new(
                m.project.remainder_numerator.parse().unwrap(),
                m.project.denominator.parse().unwrap()
            )
            .unwrap(),
            ExactSeconds::new(1, 3).unwrap()
        );
        assert_eq!(m.presentation_tick, LiveClockTick(1_333_333_353));
        assert_eq!(m.presentation_remainder_numerator, "1");
        assert_eq!(m.lateness_at(LiveClockTick(1)), 0);
        assert_eq!(m.lateness_at(LiveClockTick(1_333_333_363)), 10);
        let mut unbuffered = c.clone();
        unbuffered.added_buffer_ns = 0;
        let n = unbuffered.map(m.source_time).unwrap();
        assert_eq!(n.project, m.project);
        assert_eq!(m.presentation_tick.0 - n.presentation_tick.0, 1_000_000_000);
    }
    #[test]
    fn repeated_fractional_epoch_rebases_preserve_original_project_phase() {
        for (step, fps) in [(1, 30), (1, 24), (1001, 30000), (1001, 24000)] {
            let original = LivePresentationClock {
                epoch: AttemptId::new("original").unwrap(),
                source_anchor: SourceTimestamp::new(-1, 3).unwrap(),
                project_anchor: ProjectTime::from_nanos(10),
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            };
            let mut current = original.clone();
            for index in 1..=120 {
                let source = SourceTimestamp::new(index * step, fps).unwrap();
                let exact = current.project_at_source(source).unwrap();
                current.source_anchor = source;
                current.project_anchor = exact.quantize_nanoseconds().unwrap().time;
                current.exact_project_anchor = Some(exact);
                current.epoch = AttemptId::new(format!("epoch-{index}")).unwrap();
                for offset in 0..=5 {
                    let later = SourceTimestamp::new((index + offset) * step, fps).unwrap();
                    assert_eq!(
                        current.map(later).unwrap().project,
                        original.map(later).unwrap().project
                    );
                    assert_eq!(
                        current.project_at_source(later).unwrap(),
                        original.project_at_source(later).unwrap()
                    );
                    assert_eq!(
                        current.map(later).unwrap().presentation_tick,
                        LiveClockTick((offset * step * 1_000_000_000 / i64::from(fps)) as u64)
                    );
                }
            }
        }
        let mut c = clock();
        c.source_anchor = SourceTimestamp::new(1, 30).unwrap();
        c.project_anchor = ProjectTime::from_nanos(33_333_333);
        c.exact_project_anchor = Some(ExactSeconds::new(1, 30).unwrap());
        c.added_buffer_ns = 0;
        c.presentation_anchor = LiveClockTick(0);
        assert_eq!(
            c.map(SourceTimestamp::new(3, 30).unwrap())
                .unwrap()
                .project
                .time
                .as_nanos(),
            100_000_000
        );
    }
    #[test]
    fn exact_anchor_is_strict_checked_and_legacy_integer_wire_is_unchanged() {
        let c = clock();
        let value = serde_json::to_value(&c).unwrap();
        assert!(value.get("exact_project_anchor").is_none());
        assert_eq!(
            serde_json::from_value::<LivePresentationClock>(value).unwrap(),
            c
        );
        let mut bad = c.clone();
        bad.exact_project_anchor = Some(ExactSeconds::new(1, 30).unwrap());
        assert!(bad.validate().is_err());
        bad.exact_project_anchor = Some(ExactSeconds::new(i128::MAX, 1).unwrap());
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(c).unwrap();
        value["exact_project_anchor"] = serde_json::json!({"numerator":"1","denominator":"0"});
        assert!(serde_json::from_value::<LivePresentationClock>(value).is_err());
    }
    #[test]
    fn negative_offsets_and_tick_overflow_refuse_without_relabeling() {
        let mut c = clock();
        c.added_buffer_ns = 0;
        assert!(c.map(SourceTimestamp::new(-1, 1).unwrap()).is_err());
        c.presentation_anchor = LiveClockTick(u64::MAX);
        assert!(c.map(SourceTimestamp::new(0, 1).unwrap()).is_err());
        c.added_buffer_ns = 1;
        assert!(c.validate().is_err());
        c.presentation_anchor = LiveClockTick(0);
        c.added_buffer_ns = 1_000_000_001;
        assert!(c.validate().is_err());
        c.added_buffer_ns = 0;
        c.project_anchor = ProjectTime::from_nanos(i64::MAX);
        assert!(c.map(SourceTimestamp::new(0, 1).unwrap()).is_err());
    }
}
