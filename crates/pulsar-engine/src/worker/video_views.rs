//! Fixed filter adapter for explicit selected-eye analysis views.
use super::*;
pub(super) fn plan(
    view: Option<&VideoView>,
    index: &media::MediaIndex,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, String, TransformId)> {
    plan_dimensions(view, index.width, index.height, max_width, max_height)
}

pub(super) fn plan_dimensions(
    view: Option<&VideoView>,
    source_width: u32,
    source_height: u32,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32, String, TransformId)> {
    let Some(view) = view else {
        let (width, height) =
            media::fit_dimensions(source_width, source_height, max_width, max_height)?;
        return Ok((
            width,
            height,
            format!("scale={width}:{height}"),
            TransformId::new("source-identity")?,
        ));
    };
    let crop = view.eye_crop(source_width, source_height)?;
    let prefix = format!(
        "crop={}:{}:{}:{}:exact=1",
        crop.width, crop.height, crop.x, crop.y
    );
    let (width, height, filter) = match &view.projection {
        VideoProjection::Flat => {
            let (width, height) =
                media::fit_dimensions(crop.width, crop.height, max_width, max_height)?;
            (width, height, format!("{prefix},scale={width}:{height}"))
        }
        VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v) => {
            media::frame_bytes(max_width, max_height)?;
            let input = if matches!(view.projection, VideoProjection::Equirectangular360(_)) {
                "equirect"
            } else {
                "hequirect"
            };
            (max_width,max_height,format!("{prefix},v360=input={input}:output=flat:interp=line:w={max_width}:h={max_height}:yaw={}:pitch={}:roll={}:rorder=ypr:h_fov={}:v_fov={}",v.yaw_degrees,v.pitch_degrees,v.roll_degrees,v.horizontal_fov_degrees,v.vertical_fov_degrees))
        }
    };
    Ok((width, height, filter, video_view_transform(Some(view))?))
}
