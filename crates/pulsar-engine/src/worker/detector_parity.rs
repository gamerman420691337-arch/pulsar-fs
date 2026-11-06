//! Opt-in, pinned publisher-model numerical comparison. Not quality evidence.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::PathBuf};

fn contract() -> ModelInputContract {
    ModelInputContract {
        width: 640,
        height: 640,
        channels: 3,
        layout: TensorLayout::Nchw,
        decoder: DetectorDecoder::YoloChannelMajor,
        class_count: 80,
    }
}

#[test]
fn tensor_preserves_rgb_planes_and_non_square_nearest_coordinates() {
    let rgb = [0, 127, 255, 30, 60, 90, 255, 10, 20, 80, 160, 240];
    let mut c = contract();
    c.width = 3;
    c.height = 4;
    let input = detector_tensor(&rgb, 2, 2, &c).unwrap();
    assert_eq!(input.shape(), &[1, 3, 4, 3]);
    assert_eq!(input[[0, 1, 1, 1]], 127.0 / 255.0);
    assert_eq!(input[[0, 2, 0, 2]], 90.0 / 255.0);
    assert_eq!(input[[0, 0, 3, 0]], 1.0);
    assert_eq!(input[[0, 1, 2, 2]], 160.0 / 255.0);
    for (pixels, w, h) in [(&rgb[..11], 2, 2), (&rgb[..], 0, 2), (&rgb[..], 4097, 1)] {
        assert!(detector_tensor(pixels, w, h, &c).is_err());
    }
    c.width = 1025;
    assert!(detector_tensor(&rgb, 2, 2, &c).is_err());
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    model: PathBuf,
    runtime: PathBuf,
    width: u32,
    height: u32,
    rgb: PathBuf,
    rgb_sha256: String,
    input: PathBuf,
    input_sha256: String,
    output: PathBuf,
    output_sha256: String,
    resize: Option<InferenceResize>,
    padding_byte: Option<u8>,
}

pub(super) fn checked(path: &Path, digest: &str, limit: u64) -> Vec<u8> {
    assert!(path.is_absolute());
    let mut data = Vec::new();
    fs::File::open(path)
        .unwrap()
        .take(limit + 1)
        .read_to_end(&mut data)
        .unwrap();
    assert!(data.len() as u64 <= limit);
    assert_eq!(format!("{:x}", Sha256::digest(&data)), digest);
    data
}

pub(super) fn floats(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(bytes.len() % 4, 0);
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}

#[test]
#[ignore = "requires explicitly supplied pinned model/runtime and publisher tensor fixture"]
fn pinned_publisher_tensor_and_cpu_output_parity() {
    let path = PathBuf::from(
        std::env::var_os("PULSAR_DETECTOR_PARITY_FIXTURE").expect("fixture required"),
    );
    assert!(path.is_absolute());
    let manifest = fs::File::open(&path).unwrap().take(64 * 1024 + 1);
    let f: Fixture = serde_json::from_reader(manifest).unwrap();
    checked(
        &f.model,
        "36849cda776713a29472569fc5955b6015994f1b2f0063ac53427c9b8340fb74",
        16 * 1024 * 1024,
    );
    checked(
        &f.runtime,
        "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7",
        32 * 1024 * 1024,
    );
    let rgb = checked(&f.rgb, &f.rgb_sha256, 16 * 1024 * 1024);
    let expected_input = floats(&checked(&f.input, &f.input_sha256, 8 * 1024 * 1024));
    let (input, geometry) = match f.resize.unwrap_or(InferenceResize::Stretch) {
        InferenceResize::Stretch => (
            detector_tensor(&rgb, f.width, f.height, &contract()).unwrap(),
            pulsar_core::perception::DetectorTransform::resize(f.width, f.height, 640, 640)
                .unwrap(),
        ),
        InferenceResize::Letterbox => letterbox::tensor(
            &rgb,
            f.width,
            f.height,
            &contract(),
            f.padding_byte.expect("declared letterbox padding required"),
        )
        .unwrap(),
        _ => panic!("unsupported comparison recipe"),
    };
    assert_eq!(input.as_slice().unwrap(), expected_input);
    let expected = floats(&checked(&f.output, &f.output_sha256, 8 * 1024 * 1024));
    assert_eq!(expected.len(), 84 * 8400);
    let mut session =
        super::super::model_session::ModelSession::cpu(&f.model, &f.runtime, 2).unwrap();
    let actual = session
        .run(
            || Ok(input),
            |shape, values| {
                assert_eq!(shape, &[1, 84, 8400]);
                Ok(values.to_vec())
            },
        )
        .unwrap();
    assert_eq!(actual.len(), expected.len());
    let mut maxima = [0.0f32; 2];
    for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
        assert!(a.is_finite() && b.is_finite());
        let category = usize::from(i >= 4 * 8400);
        maxima[category] = maxima[category].max((a - b).abs());
    }
    assert!(
        maxima[0] <= 0.03 && maxima[1] <= 0.0001,
        "raw output errors {maxima:?}"
    );
    let a = decode_transformed(&actual, &[1, 84, 8400], 80, &geometry, 0.25).unwrap();
    let b = decode_transformed(&expected, &[1, 84, 8400], 80, &geometry, 0.25).unwrap();
    assert!(!a.is_empty(), "positive-detection fixture required");
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!(a.class_id, b.class_id);
        assert!((a.score - b.score).abs() <= 0.0001);
        assert!(a
            .bbox
            .iter()
            .zip(b.bbox)
            .all(|(a, b)| (a - b).abs() <= 0.0001));
    }
    println!("publisher comparison: bit-identical input, raw maxima {maxima:?}, {} matching decoded detections; unqualified", a.len());
    println!(
        "comparison_json={}",
        serde_json::json!({
            "qualified": false, "input_bit_identical": true,
            "raw_coordinate_max_absolute_error": maxima[0],
            "raw_score_max_absolute_error": maxima[1],
            "detections": a.iter().map(|d| serde_json::json!({
                "class_id": d.class_id, "score": d.score, "bbox": d.bbox
            })).collect::<Vec<_>>()
        })
    );
}
