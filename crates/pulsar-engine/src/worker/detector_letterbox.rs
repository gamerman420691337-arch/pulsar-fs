//! Fixed-size centered RGB letterbox, with bounded byte-space linear sampling.
//! Recipe and comparison sources are recorded in PULSAR_DETECTOR_LETTERBOX.md.
use super::*;
use pulsar_core::perception::DetectorTransform;

pub(super) fn tensor(
    rgb: &[u8],
    width: u32,
    height: u32,
    contract: &ModelInputContract,
    padding: u8,
) -> Result<(Array4<f32>, DetectorTransform)> {
    validate_tensor_source(rgb, width, height, contract)?;
    let ratio = (f64::from(contract.width) / f64::from(width))
        .min(f64::from(contract.height) / f64::from(height));
    let resized_width = (f64::from(width) * ratio).round_ties_even() as u32;
    let resized_height = (f64::from(height) * ratio).round_ties_even() as u32;
    if resized_width == 0
        || resized_height == 0
        || resized_width > contract.width
        || resized_height > contract.height
    {
        bail!("letterbox dimensions exceed declared tensor");
    }
    let left = (contract.width - resized_width) / 2;
    let top = (contract.height - resized_height) / 2;
    let transform = DetectorTransform::new(
        width,
        height,
        contract.width,
        contract.height,
        [
            f64::from(resized_width) / f64::from(width),
            f64::from(resized_height) / f64::from(height),
        ],
        [f64::from(left), f64::from(top)],
    )?;
    let mut input = Array4::from_elem(
        (1, 3, contract.height as usize, contract.width as usize),
        f32::from(padding) / 255.0,
    );
    let horizontal = weights(width, resized_width, true);
    let vertical = weights(height, resized_height, false);
    for (y, &(y0, y1, b0, b1)) in vertical.iter().enumerate() {
        for (x, &(x0, x1, a0, a1)) in horizontal.iter().enumerate() {
            for c in 0..3 {
                let pixel = if width == resized_width && height == resized_height {
                    rgb[(y * width as usize + x) * 3 + c]
                } else {
                    let row = |y: usize| {
                        i32::from(rgb[(y * width as usize + x0) * 3 + c]) * a0
                            + i32::from(rgb[(y * width as usize + x1) * 3 + c]) * a1
                    };
                    let value =
                        (((b0 * (row(y0) >> 4)) >> 16) + ((b1 * (row(y1) >> 4)) >> 16) + 2) >> 2;
                    u8::try_from(value).context("letterbox interpolation exceeded byte range")?
                };
                input[[0, c, y + top as usize, x + left as usize]] = f32::from(pixel) / 255.0;
            }
        }
    }
    Ok((input, transform))
}

fn weights(source: u32, destination: u32, horizontal: bool) -> Vec<(usize, usize, i32, i32)> {
    let scale = f64::from(source) / f64::from(destination);
    (0..destination)
        .map(|d| {
            let coordinate = ((f64::from(d) + 0.5) * scale - 0.5) as f32;
            let mut start = coordinate.floor() as i32;
            let mut fraction = coordinate - start as f32;
            if horizontal && (start < 0 || start >= source as i32 - 1) {
                start = start.clamp(0, source as i32 - 1);
                fraction = 0.0;
            }
            let first = start.clamp(0, source as i32 - 1) as usize;
            let second = (start + 1).clamp(0, source as i32 - 1) as usize;
            (
                first,
                second,
                ((1.0 - fraction) * 2048.0).round_ties_even() as i32,
                (fraction * 2048.0).round_ties_even() as i32,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contract(width: u32, height: u32) -> ModelInputContract {
        ModelInputContract {
            width,
            height,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        }
    }
    #[test]
    fn linear_rgb_sampling_padding_and_inverse_boxes_share_geometry() {
        let (t, geometry) = tensor(&[0, 0, 255, 255, 0, 0], 2, 1, &contract(4, 4), 114).unwrap();
        assert_eq!(t[[0, 0, 0, 0]], 114.0 / 255.0);
        assert_eq!(t[[0, 0, 1, 1]], 64.0 / 255.0);
        assert_eq!(t[[0, 2, 1, 1]], 191.0 / 255.0);
        assert_eq!(t[[0, 0, 3, 3]], 114.0 / 255.0);
        let all =
            decode_transformed(&[2.0, 2.0, 4.0, 2.0, 0.9], &[1, 5, 1], 1, &geometry, 0.25).unwrap();
        assert_eq!(all[0].bbox, [0.0, 0.0, 1.0, 1.0]);
        let padding_only =
            decode_transformed(&[2.0, 0.5, 2.0, 0.5, 0.9], &[1, 5, 1], 1, &geometry, 0.25).unwrap();
        assert!(padding_only.is_empty());
    }
    #[test]
    fn odd_padding_single_pixel_extreme_aspect_and_buffer_refusals() {
        let (t, geometry) = tensor(&[255; 9], 3, 1, &contract(3, 4), 7).unwrap();
        assert_eq!(t[[0, 1, 0, 0]], 7.0 / 255.0);
        assert_eq!(t[[0, 1, 1, 0]], 1.0);
        assert_eq!(t[[0, 1, 2, 0]], 7.0 / 255.0);
        assert_eq!(geometry.source_dimensions(), (3, 1));
        let (t, _) = tensor(&[13, 27, 59], 1, 1, &contract(3, 5), 0).unwrap();
        assert_eq!(t[[0, 2, 2, 1]], 59.0 / 255.0);
        assert!(tensor(&[0; 6], 4096, 1, &contract(1, 1), 0).is_err());
        assert!(tensor(&[0; 3], 1, 0, &contract(1, 1), 0).is_err());
        assert!(tensor(&[0; 3], 1, 1, &contract(1025, 1), 0).is_err());
        let pixels = vec![42; 4096 * 3];
        assert!(tensor(&pixels, 4096, 1, &contract(1, 1), 0).is_err());
    }

    #[test]
    #[ignore = "requires an explicit independently generated pinned interpolation fixture"]
    fn pinned_opencv_byte_space_correspondence() {
        use super::super::parity::{checked, floats};
        use std::{fs::File, io::Read, path::PathBuf};
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Case {
            width: u32,
            height: u32,
            target_width: u32,
            target_height: u32,
            rgb: PathBuf,
            rgb_sha256: String,
            input: PathBuf,
            input_sha256: String,
        }
        let path = PathBuf::from(
            std::env::var_os("PULSAR_LETTERBOX_INTERPOLATION_FIXTURE").expect("fixture required"),
        );
        assert!(path.is_absolute());
        let cases: Vec<Case> =
            serde_json::from_reader(File::open(path).unwrap().take(64 * 1024 + 1)).unwrap();
        assert_eq!(cases.len(), 13);
        for (index, c) in cases.iter().enumerate() {
            let rgb = checked(&c.rgb, &c.rgb_sha256, 16 * 1024 * 1024);
            let expected = floats(&checked(&c.input, &c.input_sha256, 8 * 1024 * 1024));
            let (actual, _) = tensor(
                &rgb,
                c.width,
                c.height,
                &contract(c.target_width, c.target_height),
                114,
            )
            .unwrap();
            let actual = actual.as_slice().unwrap();
            assert_eq!(actual.len(), expected.len());
            let mismatches = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
            assert_eq!(
                mismatches, 0,
                "case {index}: {}x{} -> {}x{}",
                c.width, c.height, c.target_width, c.target_height
            );
        }
        println!("13 independent OpenCV letterbox inputs bit-identical; unqualified");
    }
}
