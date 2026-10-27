//! Producer-declared browser/source correspondence, never clock calibration.
use crate::{CoreError, ExactSeconds, SourcePlacementSpec, SourceTimestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredSourceClock {
    pub presentation_anchor: SourceTimestamp,
    pub elapsed_anchor: SourceTimestamp,
}
impl DeclaredSourceClock {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.presentation_anchor.numerator() < 0 || self.elapsed_anchor.numerator() < 0 {
            return Err(CoreError::InvalidSourceSeek(
                "negative declared clock anchor".into(),
            ));
        }
        Ok(())
    }
    pub fn elapsed_at(&self, presentation: SourceTimestamp) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        ExactSeconds::from_source(self.elapsed_anchor).checked_add(
            ExactSeconds::from_source(presentation)
                .checked_sub(ExactSeconds::from_source(self.presentation_anchor))?,
        )
    }
    pub fn presentation_at(&self, elapsed: ExactSeconds) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        ExactSeconds::from_source(self.presentation_anchor)
            .checked_add(elapsed.checked_sub(ExactSeconds::from_source(self.elapsed_anchor))?)
    }
    pub fn project_at(
        &self,
        presentation: SourceTimestamp,
        placement: &SourcePlacementSpec,
    ) -> Result<ExactSeconds, CoreError> {
        placement.project_at_source(self.elapsed_at(presentation)?)
    }
    pub fn presentation_trim_micros(
        &self,
        placement: &SourcePlacementSpec,
    ) -> Result<(u64, u64), CoreError> {
        self.validate()?;
        placement.validate()?;
        let start =
            ceil_micros(self.presentation_at(ExactSeconds::from_source(placement.source_start))?)?;
        let end =
            ceil_micros(self.presentation_at(ExactSeconds::from_source(placement.source_end))?)?;
        if start >= end {
            return Err(CoreError::InvalidSourceSeek(
                "declared trim has no microsecond sample".into(),
            ));
        }
        Ok((start, end))
    }
}
fn ceil_micros(time: ExactSeconds) -> Result<u64, CoreError> {
    let numerator = u128::try_from(time.numerator())
        .map_err(|_| CoreError::InvalidSourceSeek("negative presentation trim".into()))?;
    let denominator = u128::from(time.denominator());
    // Divide before scaling so representable results do not overflow merely
    // because the exact numerator has a large denominator.
    let whole = (numerator / denominator)
        .checked_mul(1_000_000)
        .ok_or(CoreError::Overflow)?;
    let fraction = ((numerator % denominator) * 1_000_000).div_ceil(denominator);
    u64::try_from(whole.checked_add(fraction).ok_or(CoreError::Overflow)?)
        .map_err(|_| CoreError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectTime, SourceVersionId};
    fn placement() -> SourcePlacementSpec {
        SourcePlacementSpec {
            source_version: SourceVersionId::new("source").unwrap(),
            source_start: SourceTimestamp::new(1, 3).unwrap(),
            source_end: SourceTimestamp::new(7, 3).unwrap(),
            source_anchor: SourceTimestamp::new(1, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(-500_000_000),
            rate_numerator: 2,
            rate_denominator: 3,
        }
    }
    fn clock() -> DeclaredSourceClock {
        DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(1, 1).unwrap(),
        }
    }
    #[test]
    fn declared_source_clock_exact_offsets_inverse_and_signed_project_mapping() {
        let c = clock();
        let p = placement();
        for t in [
            SourceTimestamp::new(13, 3).unwrap(),
            SourceTimestamp::new(5, 1).unwrap(),
        ] {
            assert_eq!(
                c.presentation_at(c.elapsed_at(t).unwrap()).unwrap(),
                ExactSeconds::from_source(t)
            );
        }
        assert_eq!(
            c.project_at(SourceTimestamp::new(5, 1).unwrap(), &p)
                .unwrap(),
            ExactSeconds::new(-1, 2).unwrap()
        );
        assert_eq!(
            c.project_at(SourceTimestamp::new(16, 3).unwrap(), &p)
                .unwrap(),
            ExactSeconds::new(0, 1).unwrap()
        );
        let mut reverse = p;
        reverse.rate_numerator = -2;
        assert_eq!(
            c.project_at(SourceTimestamp::new(16, 3).unwrap(), &reverse)
                .unwrap(),
            ExactSeconds::new(-1, 1).unwrap()
        );
    }
    #[test]
    fn declared_source_clock_half_open_trim_ceil_and_submicrosecond_empty_refusal() {
        let c = clock();
        let mut p = placement();
        assert_eq!(
            c.presentation_trim_micros(&p).unwrap(),
            (4_333_334, 6_333_334)
        );
        assert!(c
            .project_at(SourceTimestamp::new(13, 3).unwrap(), &p)
            .is_ok());
        assert!(c
            .project_at(SourceTimestamp::new(19, 3).unwrap(), &p)
            .is_err());
        p.source_start = SourceTimestamp::new(1, 1_000_000_000).unwrap();
        p.source_end = SourceTimestamp::new(2, 1_000_000_000).unwrap();
        p.source_anchor = p.source_start;
        assert!(c.presentation_trim_micros(&p).is_err());
    }
    #[test]
    fn declared_source_clock_negative_trim_anchor_and_overflow_refuse() {
        let mut c = clock();
        c.presentation_anchor = SourceTimestamp::new(-1, 1).unwrap();
        assert!(c.validate().is_err());
        c = clock();
        c.elapsed_anchor = SourceTimestamp::new(-1, 1).unwrap();
        assert!(c.elapsed_at(SourceTimestamp::new(5, 1).unwrap()).is_err());
        c = clock();
        c.presentation_anchor = SourceTimestamp::new(0, 1).unwrap();
        assert!(c.presentation_trim_micros(&placement()).is_err());
        c = clock();
        c.presentation_anchor = SourceTimestamp::new(i64::MAX, 1).unwrap();
        assert!(c.presentation_trim_micros(&placement()).is_err());
        assert!(c
            .presentation_at(ExactSeconds::new(i128::MAX, 1).unwrap())
            .is_err());
        assert_eq!(
            ceil_micros(ExactSeconds::new(u64::MAX as i128, u64::MAX).unwrap()).unwrap(),
            1_000_000
        );
    }
}
