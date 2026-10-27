//! Pure numeric training view planning through the native generation adapter.
//! This role neither opens media nor authorizes model use or physical effects.
use super::*;
use std::ffi::OsString;

type Request = AnnotatedTemporalView;

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    schema_version: u16,
    recipe: String,
    declaration: Request,
    view_dimensions: [u32; 2],
    output_dimensions: [u32; 2],
    pub(super) spatial_filter: String,
    orientation: String,
    limitation: String,
}

pub(super) fn plan(request: Request) -> Result<Plan> {
    request.validate()?;
    let [width, height] = request.analysis_dimensions;
    media::frame_bytes(width, height)?;
    let (width, height, filter, _) = video_views::plan_dimensions(
        Some(&request.video_view),
        request.source_dimensions[0],
        request.source_dimensions[1],
        width,
        height,
    )?;
    let [x, y, crop_width, crop_height] = request.target_region;
    if crop_width == 0
        || crop_height == 0
        || x.checked_add(crop_width).is_none_or(|right| right > width)
        || y.checked_add(crop_height)
            .is_none_or(|bottom| bottom > height)
    {
        bail!("target region is outside the resolved analysis view");
    }
    Ok(Plan {
        schema_version: 1,
        recipe: "annotated-temporal-view-v1".into(),
        declaration: request,
        view_dimensions: [width, height],
        output_dimensions: [64, 64],
        spatial_filter: format!("{filter},crop={crop_width}:{crop_height}:{x}:{y}:exact=1,scale=64:64:flags=neighbor"),
        orientation: "encoded source; display rotation disabled".into(),
        limitation: "Declared training region only; target identity/alignment and model/runtime qualification unverified".into(),
    })
}

/// One bounded JSON argument; no Engine startup, media I/O or native execution.
pub fn run_training_view_plan(args: &[OsString]) -> Result<()> {
    if args.len() != 1 {
        bail!("training-input-plan requires one bounded JSON declaration");
    }
    let input = args[0]
        .to_str()
        .context("training declaration must be UTF-8")?;
    if input.len() > 4096 {
        bail!("training declaration exceeds byte admission");
    }
    let request: Request = serde_json::from_str(input)?;
    let output = plan(request)?;
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        serde_json::from_value(serde_json::json!({
            "schema_version":1,"source_dimensions":[192,64],"analysis_dimensions":[96,64],
            "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}},
            "target_region":[8,4,32,24],"target_id":"authored-target"
        })).unwrap()
    }
    #[test]
    fn eye_and_region_use_the_shared_native_view_plan() {
        let value = plan(request()).unwrap();
        assert_eq!(value.view_dimensions, [96, 64]);
        assert_eq!(
            value.spatial_filter,
            "crop=96:64:96:0:exact=1,scale=96:64,crop=32:24:8:4:exact=1,scale=64:64:flags=neighbor"
        );
    }
    #[test]
    fn malformed_geometry_and_unknown_declarations_refuse() {
        for region in [
            [0, 0, 0, 1],
            [95, 0, 2, 1],
            [0, 63, 1, 2],
            [u32::MAX, 0, 2, 1],
        ] {
            let mut value = request();
            value.target_region = region;
            assert!(plan(value).is_err());
        }
        let mut value = request();
        value.source_dimensions[0] = 191;
        assert!(plan(value).is_err());
        let mut value = request();
        value.target_id = "guessed target".into();
        assert!(plan(value).is_err());
        let mut value = serde_json::to_value(request()).unwrap();
        value["extra"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
    #[test]
    fn spherical_projection_uses_the_existing_view_validation_and_filter() {
        let mut value = request();
        value.source_dimensions = [256, 64];
        value.video_view.projection =
            VideoProjection::Equirectangular360(pulsar_core::PerspectiveView {
                yaw_degrees: 20.0,
                pitch_degrees: 10.0,
                roll_degrees: 0.0,
                horizontal_fov_degrees: 80.0,
                vertical_fov_degrees: 60.0,
            });
        assert!(plan(value.clone())
            .unwrap()
            .spatial_filter
            .contains("v360=input=equirect:output=flat"));
        value.source_dimensions = [192, 64];
        assert!(plan(value).is_err());
    }
}
