//! Checked point maps. Coordinates are normalized image-edge coordinates:
//! pixel (i,j) has center ((i+.5)/width,(j+.5)/height). No box mapping is implied.
use crate::{CoreError, VideoProjection, VideoView};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewPoint {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackedSourcePoint {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewCoordinateMap {
    view: VideoView,
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
}
fn bad() -> CoreError {
    CoreError::InvalidGeometry("invalid view coordinate map or point".into())
}
fn point(x: f64, y: f64) -> Result<(), CoreError> {
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        Err(bad())
    } else {
        Ok(())
    }
}
impl ViewCoordinateMap {
    pub fn new(
        view: VideoView,
        source_width: u32,
        source_height: u32,
        output_width: u32,
        output_height: u32,
    ) -> Result<Self, CoreError> {
        let map = Self {
            view,
            source_width,
            source_height,
            output_width,
            output_height,
        };
        map.validate()?;
        Ok(map)
    }
    pub fn validate(&self) -> Result<(), CoreError> {
        let c = self.view.eye_crop(self.source_width, self.source_height)?;
        if self.output_width == 0
            || self.output_height == 0
            || self.output_width > 65536
            || self.output_height > 65536
            || (!matches!(self.view.projection, VideoProjection::Flat)
                && (c.width < 2 || c.height < 2))
        {
            return Err(bad());
        }
        Ok(())
    }
    pub fn view(&self) -> &VideoView {
        &self.view
    }
    /// Dimensions of the packed source before eye selection and projection.
    pub fn source_dimensions(&self) -> (u32, u32) {
        (self.source_width, self.source_height)
    }
    pub fn output_dimensions(&self) -> (u32, u32) {
        (self.output_width, self.output_height)
    }
    /// Nominal geometric sampling center, before interpolation/border handling.
    pub fn to_source(&self, p: ViewPoint) -> Result<PackedSourcePoint, CoreError> {
        self.validate()?;
        point(p.x, p.y)?;
        let c = self.view.eye_crop(self.source_width, self.source_height)?;
        let (x, y) = match &self.view.projection {
            VideoProjection::Flat => (p.x * f64::from(c.width), p.y * f64::from(c.height)),
            VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v) => {
                let ray = [
                    (2.0 * p.x - 1.0) * (v.horizontal_fov_degrees.to_radians() / 2.0).tan(),
                    (2.0 * p.y - 1.0) * (v.vertical_fov_degrees.to_radians() / 2.0).tan(),
                    1.0,
                ];
                let r = rotate(ray, v, false);
                let norm = (r.iter().map(|a| a * a).sum::<f64>()).sqrt();
                let phi = r[0].atan2(r[2]);
                let theta = (r[1] / norm).clamp(-1.0, 1.0).asin();
                let range =
                    if matches!(self.view.projection, VideoProjection::Equirectangular360(_)) {
                        std::f64::consts::PI
                    } else {
                        std::f64::consts::FRAC_PI_2
                    };
                if phi.abs() > range + 1e-12 {
                    return Err(bad());
                }
                (
                    (phi / range * 0.5 + 0.5) * f64::from(c.width - 1) + 0.5,
                    (theta / std::f64::consts::PI + 0.5) * f64::from(c.height - 1) + 0.5,
                )
            }
        };
        Ok(PackedSourcePoint {
            x: (f64::from(c.x) + x) / f64::from(self.source_width),
            y: (f64::from(c.y) + y) / f64::from(self.source_height),
        })
    }
    /// None means another eye, outside the sample domain/FOV, behind the camera,
    /// or a spherical pole whose longitude is not uniquely invertible.
    pub fn to_view(&self, p: PackedSourcePoint) -> Result<Option<ViewPoint>, CoreError> {
        self.validate()?;
        point(p.x, p.y)?;
        let c = self.view.eye_crop(self.source_width, self.source_height)?;
        let x = p.x * f64::from(self.source_width) - f64::from(c.x);
        let y = p.y * f64::from(self.source_height) - f64::from(c.y);
        if x < 0.0 || y < 0.0 || x > f64::from(c.width) || y > f64::from(c.height) {
            return Ok(None);
        }
        let (u, v) = match &self.view.projection {
            VideoProjection::Flat => (x / f64::from(c.width), y / f64::from(c.height)),
            VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v) => {
                let sx = (x - 0.5) / f64::from(c.width - 1);
                let sy = (y - 0.5) / f64::from(c.height - 1);
                if !(0.0..=1.0).contains(&sx)
                    || !(0.0..=1.0).contains(&sy)
                    || sy <= 1e-12
                    || sy >= 1.0 - 1e-12
                {
                    return Ok(None);
                }
                let range =
                    if matches!(self.view.projection, VideoProjection::Equirectangular360(_)) {
                        std::f64::consts::PI
                    } else {
                        std::f64::consts::FRAC_PI_2
                    };
                let phi = (2.0 * sx - 1.0) * range;
                let theta = (sy - 0.5) * std::f64::consts::PI;
                let r = rotate(
                    [
                        theta.cos() * phi.sin(),
                        theta.sin(),
                        theta.cos() * phi.cos(),
                    ],
                    v,
                    true,
                );
                if r[2] <= 1e-12 {
                    return Ok(None);
                }
                (
                    0.5 + 0.5 * r[0] / r[2] / (v.horizontal_fov_degrees.to_radians() / 2.0).tan(),
                    0.5 + 0.5 * r[1] / r[2] / (v.vertical_fov_degrees.to_radians() / 2.0).tan(),
                )
            }
        };
        if u < -1e-10 || v < -1e-10 || u > 1.0 + 1e-10 || v > 1.0 + 1e-10 {
            return Ok(None);
        }
        Ok(Some(ViewPoint {
            x: u.clamp(0.0, 1.0),
            y: v.clamp(0.0, 1.0),
        }))
    }
}
fn rotate(mut r: [f64; 3], v: &crate::PerspectiveView, inverse: bool) -> [f64; 3] {
    let angles = [
        v.roll_degrees.to_radians(),
        v.pitch_degrees.to_radians(),
        v.yaw_degrees.to_radians(),
    ];
    for axis in if inverse { [2, 1, 0] } else { [0, 1, 2] } {
        let (s, c) = (angles[axis] * if inverse { -1.0 } else { 1.0 }).sin_cos();
        r = match axis {
            0 => [c * r[0] - s * r[1], s * r[0] + c * r[1], r[2]],
            1 => [r[0], c * r[1] - s * r[2], s * r[1] + c * r[2]],
            _ => [c * r[0] + s * r[2], r[1], -s * r[0] + c * r[2]],
        };
    }
    r
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PerspectiveView, StereoEye, StereoLayout};
    #[test]
    fn rotated_seam_and_half_views_round_trip() {
        for half in [false, true] {
            for yaw in if half {
                vec![0.0, 20.0]
            } else {
                vec![-180.0, 90.0, 180.0]
            } {
                let v = PerspectiveView {
                    yaw_degrees: yaw,
                    pitch_degrees: 10.0,
                    roll_degrees: 25.0,
                    horizontal_fov_degrees: 60.0,
                    vertical_fov_degrees: 40.0,
                };
                let map = ViewCoordinateMap::new(
                    VideoView {
                        layout: StereoLayout::SideBySideLeftRight,
                        eye: StereoEye::Right,
                        projection: if half {
                            VideoProjection::HalfEquirectangular180(v)
                        } else {
                            VideoProjection::Equirectangular360(v)
                        },
                    },
                    if half { 256 } else { 512 },
                    128,
                    64,
                    64,
                )
                .unwrap();
                for x in [0.0, 0.2, 0.5, 0.8, 1.0] {
                    for y in [0.0, 0.3, 0.7, 1.0] {
                        let p = ViewPoint { x, y };
                        let s = map.to_source(p).unwrap();
                        assert!(s.x >= 0.5);
                        let back = map.to_view(s).unwrap().unwrap();
                        assert!((back.x - x).abs() < 1e-10 && (back.y - y).abs() < 1e-10);
                    }
                }
                assert_eq!(
                    map.to_view(PackedSourcePoint { x: 0.25, y: 0.5 }).unwrap(),
                    None
                );
                assert!(map
                    .to_source(ViewPoint {
                        x: f64::NAN,
                        y: 0.5
                    })
                    .is_err());
            }
        }
    }
    #[test]
    fn flat_crop_and_invisible_source_are_checked() {
        let map = ViewCoordinateMap::new(
            VideoView {
                layout: StereoLayout::TopBottomRightLeft,
                eye: StereoEye::Left,
                projection: VideoProjection::Flat,
            },
            100,
            80,
            50,
            20,
        )
        .unwrap();
        assert_eq!(
            map.to_source(ViewPoint { x: 0.5, y: 0.5 }).unwrap(),
            PackedSourcePoint { x: 0.5, y: 0.75 }
        );
        assert_eq!(
            map.to_view(PackedSourcePoint { x: 0.5, y: 0.25 }).unwrap(),
            None
        );
        let mut raw = serde_json::to_value(&map).unwrap();
        raw["source_width"] = 0.into();
        let invalid: ViewCoordinateMap = serde_json::from_value(raw).unwrap();
        assert!(invalid.to_source(ViewPoint { x: 0.5, y: 0.5 }).is_err());
    }
    #[test]
    fn poles_back_faces_and_outside_fov_have_no_inverse() {
        let map = ViewCoordinateMap::new(
            VideoView {
                layout: StereoLayout::Mono,
                eye: StereoEye::Left,
                projection: VideoProjection::Equirectangular360(PerspectiveView {
                    yaw_degrees: 0.0,
                    pitch_degrees: 0.0,
                    roll_degrees: 0.0,
                    horizontal_fov_degrees: 60.0,
                    vertical_fov_degrees: 40.0,
                }),
            },
            256,
            128,
            64,
            64,
        )
        .unwrap();
        for p in [
            PackedSourcePoint {
                x: 0.5,
                y: 0.5 / 128.0,
            },
            PackedSourcePoint {
                x: 0.5,
                y: 127.5 / 128.0,
            },
            PackedSourcePoint {
                x: 0.5 / 256.0,
                y: 0.5,
            },
            PackedSourcePoint {
                x: (0.75 * 255.0 + 0.5) / 256.0,
                y: 0.5,
            },
        ] {
            assert_eq!(map.to_view(p).unwrap(), None);
        }
    }
}
