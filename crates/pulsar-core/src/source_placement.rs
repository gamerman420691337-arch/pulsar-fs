//! Exact, reversible source/project mappings. No files or project authority.
use crate::{
    CoreError, ProjectTime, RevisionId, SourcePlacementId, SourceTimestamp, SourceVersionId,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExactSecondsWire", into = "ExactSecondsWire")]
pub struct ExactSeconds {
    numerator: i128,
    denominator: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactSecondsWire {
    numerator: String,
    denominator: String,
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
impl ExactSeconds {
    pub fn new(numerator: i128, denominator: u64) -> Result<Self, CoreError> {
        if denominator == 0 {
            return Err(CoreError::InvalidTimebase);
        }
        let d = gcd(numerator.unsigned_abs(), u128::from(denominator));
        Ok(Self {
            numerator: numerator / i128::try_from(d).map_err(|_| CoreError::Overflow)?,
            denominator: denominator / u64::try_from(d).map_err(|_| CoreError::Overflow)?,
        })
    }
    pub fn from_source(t: SourceTimestamp) -> Self {
        Self::new(i128::from(t.numerator()), u64::from(t.denominator()))
            .expect("checked source denominator")
    }
    pub fn from_project(t: ProjectTime) -> Self {
        Self::new(i128::from(t.as_nanos()), 1_000_000_000).expect("constant denominator")
    }
    pub fn numerator(self) -> i128 {
        self.numerator
    }
    pub fn denominator(self) -> u64 {
        self.denominator
    }
    pub fn checked_cmp(self, other: Self) -> Result<std::cmp::Ordering, CoreError> {
        let d = gcd(u128::from(self.denominator), u128::from(other.denominator)) as u64;
        let a = self
            .numerator
            .checked_mul(i128::from(other.denominator / d))
            .ok_or(CoreError::Overflow)?;
        let b = other
            .numerator
            .checked_mul(i128::from(self.denominator / d))
            .ok_or(CoreError::Overflow)?;
        Ok(a.cmp(&b))
    }
    pub fn checked_add(self, other: Self) -> Result<Self, CoreError> {
        let d = gcd(u128::from(self.denominator), u128::from(other.denominator)) as u64;
        let numerator = self
            .numerator
            .checked_mul(i128::from(other.denominator / d))
            .and_then(|a| {
                other
                    .numerator
                    .checked_mul(i128::from(self.denominator / d))
                    .and_then(|b| a.checked_add(b))
            })
            .ok_or(CoreError::Overflow)?;
        let denominator = u128::from(self.denominator / d) * u128::from(other.denominator);
        let reduction = gcd(numerator.unsigned_abs(), denominator);
        Self::new(
            numerator / i128::try_from(reduction).map_err(|_| CoreError::Overflow)?,
            u64::try_from(denominator / reduction).map_err(|_| CoreError::Overflow)?,
        )
    }
    pub fn checked_sub(self, other: Self) -> Result<Self, CoreError> {
        self.checked_add(Self {
            numerator: other.numerator.checked_neg().ok_or(CoreError::Overflow)?,
            denominator: other.denominator,
        })
    }
    pub fn checked_ratio(self, num: i64, den: u32) -> Result<Self, CoreError> {
        if den == 0 {
            return Err(CoreError::InvalidTimebase);
        }
        let a = gcd(self.numerator.unsigned_abs(), u128::from(den));
        let b = gcd(u128::from(num.unsigned_abs()), u128::from(self.denominator));
        let numerator = (self.numerator / (a as i128))
            .checked_mul(i128::from(num) / (b as i128))
            .ok_or(CoreError::Overflow)?;
        let denominator = (self.denominator / (b as u64))
            .checked_mul(u64::from(den) / (a as u64))
            .ok_or(CoreError::Overflow)?;
        Self::new(numerator, denominator)
    }
    pub fn quantize_nanoseconds(self) -> Result<ExactTimeQuantization, CoreError> {
        let scaled = self
            .numerator
            .checked_mul(1_000_000_000)
            .ok_or(CoreError::Overflow)?;
        let den = i128::from(self.denominator);
        Ok(ExactTimeQuantization {
            time: ProjectTime::from_nanos(
                i64::try_from(scaled.div_euclid(den)).map_err(|_| CoreError::Overflow)?,
            ),
            remainder_numerator: scaled.rem_euclid(den).to_string(),
            denominator: self.denominator.to_string(),
        })
    }
}
impl TryFrom<ExactSecondsWire> for ExactSeconds {
    type Error = CoreError;
    fn try_from(w: ExactSecondsWire) -> Result<Self, CoreError> {
        if w.numerator.len() > 40 || w.denominator.len() > 20 {
            return Err(CoreError::Overflow);
        }
        Self::new(
            w.numerator
                .parse()
                .map_err(|_| CoreError::InvalidSourceSeek("invalid exact numerator".into()))?,
            w.denominator
                .parse()
                .map_err(|_| CoreError::InvalidTimebase)?,
        )
    }
}
impl From<ExactSeconds> for ExactSecondsWire {
    fn from(v: ExactSeconds) -> Self {
        Self {
            numerator: v.numerator.to_string(),
            denominator: v.denominator.to_string(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactTimeQuantization {
    pub time: ProjectTime,
    pub remainder_numerator: String,
    pub denominator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePlacementSpec {
    pub source_version: SourceVersionId,
    /// Elapsed source seconds from its first actual decoded frame. Half-open trim.
    pub source_start: SourceTimestamp,
    pub source_end: SourceTimestamp,
    pub source_anchor: SourceTimestamp,
    pub project_anchor: ProjectTime,
    /// Source elapsed seconds per project second; negative means reverse mapping.
    pub rate_numerator: i32,
    pub rate_denominator: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePlacement {
    pub placement_id: SourcePlacementId,
    pub created_revision: RevisionId,
    pub spec: SourcePlacementSpec,
}
impl SourcePlacementSpec {
    /// Whether generated project timestamps remain original elapsed-source time.
    pub fn preserves_source_clock(&self) -> Result<bool, CoreError> {
        self.validate()?;
        Ok(
            i64::from(self.rate_numerator) == i64::from(self.rate_denominator)
                && ExactSeconds::from_project(self.project_anchor)
                    .checked_cmp(ExactSeconds::from_source(self.source_anchor))?
                    == std::cmp::Ordering::Equal,
        )
    }
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.rate_numerator == 0
            || self.rate_denominator == 0
            || self.source_start.numerator() < 0
            || self.source_end.numerator() < 0
        {
            return Err(CoreError::InvalidSourceSeek(
                "invalid placement trim or rate".into(),
            ));
        }
        let start = ExactSeconds::from_source(self.source_start);
        let end = ExactSeconds::from_source(self.source_end);
        if start.checked_cmp(end)? != std::cmp::Ordering::Less {
            return Err(CoreError::InvalidSourceSeek(
                "placement trim must be nonempty and ordered".into(),
            ));
        }
        self.require_in_trim(ExactSeconds::from_source(self.source_anchor))
    }
    fn require_in_trim(&self, s: ExactSeconds) -> Result<(), CoreError> {
        if s.checked_cmp(ExactSeconds::from_source(self.source_start))? == std::cmp::Ordering::Less
            || s.checked_cmp(ExactSeconds::from_source(self.source_end))?
                != std::cmp::Ordering::Less
        {
            return Err(CoreError::InvalidSourceSeek(
                "source position is outside placement trim".into(),
            ));
        }
        Ok(())
    }
    pub fn source_at_project(&self, p: ProjectTime) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        let delta = ExactSeconds::from_project(p)
            .checked_sub(ExactSeconds::from_project(self.project_anchor))?;
        let source = ExactSeconds::from_source(self.source_anchor).checked_add(
            delta.checked_ratio(i64::from(self.rate_numerator), self.rate_denominator)?,
        )?;
        self.require_in_trim(source)?;
        Ok(source)
    }
    pub fn project_at_source(&self, s: ExactSeconds) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        self.require_in_trim(s)?;
        self.map_source_value(s)
    }
    /// Map a declared trim boundary, including the exclusive source end.
    /// This does not assert that a decoded frame exists at that boundary.
    pub fn project_at_source_boundary(&self, s: ExactSeconds) -> Result<ExactSeconds, CoreError> {
        self.validate()?;
        if s.checked_cmp(ExactSeconds::from_source(self.source_start))? == std::cmp::Ordering::Less
            || s.checked_cmp(ExactSeconds::from_source(self.source_end))?
                == std::cmp::Ordering::Greater
        {
            return Err(CoreError::InvalidSourceSeek(
                "position is outside declared placement boundaries".into(),
            ));
        }
        self.map_source_value(s)
    }
    fn map_source_value(&self, s: ExactSeconds) -> Result<ExactSeconds, CoreError> {
        let delta = s.checked_sub(ExactSeconds::from_source(self.source_anchor))?;
        let num = i64::from(self.rate_denominator) * if self.rate_numerator < 0 { -1 } else { 1 };
        ExactSeconds::from_project(self.project_anchor)
            .checked_add(delta.checked_ratio(num, self.rate_numerator.unsigned_abs())?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn spec(rate: i32) -> SourcePlacementSpec {
        SourcePlacementSpec {
            source_version: SourceVersionId::new("source").unwrap(),
            source_start: SourceTimestamp::new(1, 3).unwrap(),
            source_end: SourceTimestamp::new(3, 1).unwrap(),
            source_anchor: SourceTimestamp::new(1, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(-2_000_000_000),
            rate_numerator: rate,
            rate_denominator: 3,
        }
    }
    #[test]
    fn rational_forward_reverse_mapping_and_half_open_trim() {
        for rate in [2, -2] {
            let s = spec(rate);
            for ns in [
                -2_500_000_000,
                -2_000_000_000,
                -1_999_999_999,
                -1_500_000_000,
            ] {
                let p = ProjectTime::from_nanos(ns);
                let src = s.source_at_project(p).unwrap();
                assert_eq!(
                    s.project_at_source(src).unwrap(),
                    ExactSeconds::from_project(p)
                );
            }
        }
        let s = spec(2);
        assert!(s
            .project_at_source(ExactSeconds::new(3, 1).unwrap())
            .is_err());
        assert!(s
            .source_at_project(ProjectTime::from_nanos(-9_000_000_000))
            .is_err());
    }
    #[test]
    fn exact_wire_and_explicit_quantization_preserve_subnanoseconds() {
        let t = ExactSeconds::new(-1, 3).unwrap();
        let q = t.quantize_nanoseconds().unwrap();
        assert_eq!(q.time.as_nanos(), -333_333_334);
        assert_eq!(q.remainder_numerator, "2");
        assert_eq!(
            serde_json::from_str::<ExactSeconds>(&serde_json::to_string(&t).unwrap()).unwrap(),
            t
        );
        assert!(
            serde_json::from_str::<ExactSeconds>(r#"{"numerator":"1","denominator":"0"}"#).is_err()
        );
        let mut s = spec(0);
        assert!(s.validate().is_err());
        s.rate_numerator = 1;
        s.rate_denominator = 0;
        assert!(s.validate().is_err());
        assert!(ExactSeconds::new(i128::MAX, 1)
            .unwrap()
            .checked_ratio(2, 1)
            .is_err());
    }
    #[test]
    fn source_clock_identity_requires_exact_anchor_and_unit_rate() {
        let mut p = spec(3);
        p.project_anchor = ProjectTime::from_nanos(1_000_000_000);
        assert!(p.preserves_source_clock().unwrap());
        p.project_anchor = ProjectTime::from_nanos(1_000_000_001);
        assert!(!p.preserves_source_clock().unwrap());
        p.project_anchor = ProjectTime::from_nanos(1_000_000_000);
        p.rate_numerator = -3;
        assert!(!p.preserves_source_clock().unwrap());
        p.rate_numerator = 2;
        assert!(!p.preserves_source_clock().unwrap());
    }
}
