//! Source-frame selection uses integer PTS and an explicit request clock.
use crate::{CoreError, SourceTimestamp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewTimeBasis {
    #[default]
    SourcePts,
    ElapsedFromFirstFrame,
}
impl PreviewTimeBasis {
    pub fn is_source_pts(&self) -> bool {
        *self == Self::SourcePts
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceFrameSeek {
    pub frame_index: usize,
    pub source_time: SourceTimestamp,
    pub elapsed_time: SourceTimestamp,
    pub source_origin: SourceTimestamp,
}
/// Select the latest actual frame at or before the requested timestamp.
/// A request beyond the last known PTS holds that frame; no duration or new
/// frame is invented. Requests before the first frame are outside this domain.
pub fn seek_source_frames(
    pts: &[i64],
    time_base_num: u32,
    time_base_den: u32,
    requested: SourceTimestamp,
    basis: PreviewTimeBasis,
) -> Result<SourceFrameSeek, CoreError> {
    seek_source_frames_exact(
        pts,
        time_base_num,
        time_base_den,
        crate::ExactSeconds::from_source(requested),
        basis,
    )
}

pub fn seek_source_frames_exact(
    pts: &[i64],
    time_base_num: u32,
    time_base_den: u32,
    requested: crate::ExactSeconds,
    basis: PreviewTimeBasis,
) -> Result<SourceFrameSeek, CoreError> {
    if pts.is_empty()
        || time_base_num == 0
        || time_base_den == 0
        || pts.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(CoreError::InvalidSourceSeek(
            "empty, unordered, or invalid-timebase frame index".into(),
        ));
    }
    let origin = pts[0];
    let frame_time = |p: i64| -> Result<crate::ExactSeconds, CoreError> {
        let tick = if basis == PreviewTimeBasis::ElapsedFromFirstFrame {
            i128::from(p) - i128::from(origin)
        } else {
            i128::from(p)
        };
        crate::ExactSeconds::new(
            tick.checked_mul(i128::from(time_base_num))
                .ok_or(CoreError::Overflow)?,
            u64::from(time_base_den),
        )
    };
    if frame_time(origin)?.checked_cmp(requested)? == std::cmp::Ordering::Greater {
        return Err(CoreError::InvalidSourceSeek(
            "requested time precedes the first source frame".into(),
        ));
    }
    let (mut lo, mut hi) = (0, pts.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if frame_time(pts[mid])?.checked_cmp(requested)? != std::cmp::Ordering::Greater {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let index = lo.saturating_sub(1);
    let exact = |ticks: i128| -> Result<SourceTimestamp, CoreError> {
        SourceTimestamp::new(
            i64::try_from(
                ticks
                    .checked_mul(i128::from(time_base_num))
                    .ok_or(CoreError::Overflow)?,
            )
            .map_err(|_| CoreError::Overflow)?,
            time_base_den,
        )
    };
    Ok(SourceFrameSeek {
        frame_index: index,
        source_time: exact(i128::from(pts[index]))?,
        elapsed_time: exact(i128::from(pts[index]) - i128::from(origin))?,
        source_origin: exact(i128::from(origin))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_pts_signed_origin_and_distinct_clocks() {
        for origin in [-5000, 5000] {
            let pts = [origin, origin + 125, origin + 375, origin + 500];
            let r = SourceTimestamp::new(300, 1000).unwrap();
            let found =
                seek_source_frames(&pts, 1, 1000, r, PreviewTimeBasis::ElapsedFromFirstFrame)
                    .unwrap();
            assert_eq!(found.frame_index, 1);
            assert_eq!(found.elapsed_time, SourceTimestamp::new(125, 1000).unwrap());
            assert_eq!(
                found.source_time,
                SourceTimestamp::new(origin + 125, 1000).unwrap()
            );
            assert_eq!(
                found.source_origin,
                SourceTimestamp::new(origin, 1000).unwrap()
            );
            let absolute = seek_source_frames(
                &pts,
                1,
                1000,
                SourceTimestamp::new(origin + 375, 1000).unwrap(),
                PreviewTimeBasis::SourcePts,
            )
            .unwrap();
            assert_eq!(absolute.frame_index, 2);
        }
    }
    #[test]
    fn invalid_domains_and_overflow_refuse_instead_of_clamping() {
        assert_eq!(
            seek_source_frames(
                &[5, 6],
                1,
                1,
                SourceTimestamp::new(0, 1).unwrap(),
                PreviewTimeBasis::SourcePts
            ),
            Err(CoreError::InvalidSourceSeek(
                "requested time precedes the first source frame".into()
            ))
        );
        for pts in [vec![], vec![1, 1], vec![2, 1]] {
            assert!(seek_source_frames(
                &pts,
                1,
                1,
                SourceTimestamp::new(0, 1).unwrap(),
                PreviewTimeBasis::ElapsedFromFirstFrame
            )
            .is_err());
        }
        assert!(seek_source_frames(
            &[i64::MIN, i64::MAX],
            u32::MAX,
            u32::MAX,
            SourceTimestamp::new(i64::MAX, u32::MAX).unwrap(),
            PreviewTimeBasis::ElapsedFromFirstFrame
        )
        .is_err());
        let hold = seek_source_frames(
            &[5, 6],
            1,
            1,
            SourceTimestamp::new(99, 1).unwrap(),
            PreviewTimeBasis::ElapsedFromFirstFrame,
        )
        .unwrap();
        assert_eq!(hold.frame_index, 1);
        assert_eq!(hold.elapsed_time, SourceTimestamp::new(1, 1).unwrap());
    }
}
