//! Explicit eye/layout/projection declarations. No metadata or eye is guessed.
use crate::CoreError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoLayout {
    Mono,
    SideBySideLeftRight,
    SideBySideRightLeft,
    TopBottomLeftRight,
    TopBottomRightLeft,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoEye {
    Left,
    Right,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerspectiveView {
    pub yaw_degrees: f64,
    pub pitch_degrees: f64,
    pub roll_degrees: f64,
    pub horizontal_fov_degrees: f64,
    pub vertical_fov_degrees: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "viewport",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum VideoProjection {
    Flat,
    Equirectangular360(PerspectiveView),
    HalfEquirectangular180(PerspectiveView),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoView {
    pub layout: StereoLayout,
    pub eye: StereoEye,
    pub projection: VideoProjection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EyeCrop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
fn invalid() -> CoreError {
    CoreError::InvalidGeometry("invalid or ambiguous declared video view".into())
}
impl VideoView {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.layout == StereoLayout::Mono && self.eye != StereoEye::Left {
            return Err(invalid());
        }
        if let VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v) =
            &self.projection
        {
            if ![
                v.yaw_degrees,
                v.pitch_degrees,
                v.roll_degrees,
                v.horizontal_fov_degrees,
                v.vertical_fov_degrees,
            ]
            .iter()
            .all(|n| n.is_finite())
                || v.yaw_degrees.abs() > 180.0
                || v.pitch_degrees.abs() > 90.0
                || v.roll_degrees.abs() > 180.0
                || !(1.0..=170.0).contains(&v.horizontal_fov_degrees)
                || !(1.0..=170.0).contains(&v.vertical_fov_degrees)
            {
                return Err(invalid());
            }
            if matches!(self.projection, VideoProjection::HalfEquirectangular180(_)) {
                let diagonal = ((v.horizontal_fov_degrees.to_radians() * 0.5).tan().powi(2)
                    + (v.vertical_fov_degrees.to_radians() * 0.5).tan().powi(2))
                .sqrt()
                .atan()
                .to_degrees();
                if v.yaw_degrees.abs() + v.pitch_degrees.abs() + diagonal >= 90.0 {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
    pub fn eye_crop(&self, width: u32, height: u32) -> Result<EyeCrop, CoreError> {
        self.validate()?;
        if width == 0 || height == 0 || width > 65536 || height > 65536 {
            return Err(invalid());
        }
        let right = self.eye == StereoEye::Right;
        let crop = match self.layout {
            StereoLayout::Mono => EyeCrop {
                x: 0,
                y: 0,
                width,
                height,
            },
            StereoLayout::SideBySideLeftRight | StereoLayout::SideBySideRightLeft => {
                if !width.is_multiple_of(2) {
                    return Err(invalid());
                }
                let second = right == (self.layout == StereoLayout::SideBySideLeftRight);
                EyeCrop {
                    x: if second { width / 2 } else { 0 },
                    y: 0,
                    width: width / 2,
                    height,
                }
            }
            StereoLayout::TopBottomLeftRight | StereoLayout::TopBottomRightLeft => {
                if !height.is_multiple_of(2) {
                    return Err(invalid());
                }
                let second = right == (self.layout == StereoLayout::TopBottomLeftRight);
                EyeCrop {
                    x: 0,
                    y: if second { height / 2 } else { 0 },
                    width,
                    height: height / 2,
                }
            }
        };
        // These adapters implement the declared full/half equirectangular
        // families only; fisheye/cubemap and ambiguous aspect ratios are refused.
        match self.projection {
            VideoProjection::Equirectangular360(_) if crop.width != crop.height * 2 => {
                return Err(invalid())
            }
            VideoProjection::HalfEquirectangular180(_) if crop.width != crop.height => {
                return Err(invalid())
            }
            _ => {}
        }
        Ok(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_eye_order_and_even_packing_are_checked() {
        for (layout, left, right) in [
            (StereoLayout::SideBySideLeftRight, 0, 50),
            (StereoLayout::SideBySideRightLeft, 50, 0),
        ] {
            let mut view = VideoView {
                layout,
                eye: StereoEye::Left,
                projection: VideoProjection::Flat,
            };
            assert_eq!(view.eye_crop(100, 40).unwrap().x, left);
            view.eye = StereoEye::Right;
            assert_eq!(view.eye_crop(100, 40).unwrap().x, right);
            assert!(view.eye_crop(99, 40).is_err());
        }
        for (layout, left, right) in [
            (StereoLayout::TopBottomLeftRight, 0, 20),
            (StereoLayout::TopBottomRightLeft, 20, 0),
        ] {
            let mut view = VideoView {
                layout,
                eye: StereoEye::Left,
                projection: VideoProjection::Flat,
            };
            assert_eq!(view.eye_crop(100, 40).unwrap().y, left);
            view.eye = StereoEye::Right;
            assert_eq!(view.eye_crop(100, 40).unwrap().y, right);
            assert!(view.eye_crop(100, 39).is_err());
        }
    }
    #[test]
    fn spherical_family_and_numeric_domain_are_explicit() {
        let viewport = PerspectiveView {
            yaw_degrees: 0.0,
            pitch_degrees: 0.0,
            roll_degrees: 0.0,
            horizontal_fov_degrees: 90.0,
            vertical_fov_degrees: 90.0,
        };
        let mut view = VideoView {
            layout: StereoLayout::Mono,
            eye: StereoEye::Left,
            projection: VideoProjection::Equirectangular360(viewport.clone()),
        };
        assert!(view.eye_crop(100, 50).is_ok());
        assert!(view.eye_crop(100, 100).is_err());
        view.projection = VideoProjection::HalfEquirectangular180(viewport.clone());
        assert!(view.eye_crop(100, 100).is_ok());
        let mut outside = viewport.clone();
        outside.yaw_degrees = 80.0;
        view.projection = VideoProjection::HalfEquirectangular180(outside);
        assert!(view.validate().is_err());
        let mut bad = viewport;
        bad.horizontal_fov_degrees = f64::NAN;
        view.projection = VideoProjection::Equirectangular360(bad);
        assert!(view.validate().is_err());
        view.eye = StereoEye::Right;
        assert!(view.validate().is_err());
    }
}
