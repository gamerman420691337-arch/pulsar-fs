//! ONNX effect adapter. No caller outside the worker may create a session.
//!
//! This adapter accepts one declared tensor contract rather than guessing a
//! decoder from tensor lengths. Output geometry is validated before it crosses
//! into the checked observation domain.

use anyhow::{bail, Context, Result};
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use pulsar_protocol::{DetectorDecoder, InferenceResize, ModelInputContract, TensorLayout};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct RawDetection {
    pub class_id: u32,
    pub score: f32,
    pub bbox: [f32; 4],
}

pub struct Detector {
    session: super::model_session::ModelSession,
    contract: ModelInputContract,
    resize: InferenceResize,
    padding_byte: u8,
    score_threshold: f32,
}

pub(super) fn cpu_session(model: &Path, runtime: &Path, threads: u16) -> Result<Session> {
    super::telemetry::reset();
    let committed = ort::init_from(runtime)
        .map_err(|e| anyhow::anyhow!("failed to load pinned ONNX runtime: {e}"))?
        .with_name("pulsar-isolated-worker")
        .commit();
    if !committed {
        bail!("ONNX runtime was already initialized; a new attempt process is required");
    }
    let session = native(Session::builder())?;
    let session = native(
        session.with_execution_providers([ort::ep::CPU::default().build().error_on_failure()]),
    )?;
    let session = native(session.with_intra_threads(usize::from(threads)))?;
    let session = native(session.with_inter_threads(1))?;
    let mut session = native(session.with_optimization_level(GraphOptimizationLevel::Level3))?;
    native(session.commit_from_file(model))
}
pub(super) fn native<T, E: std::fmt::Display>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|e| anyhow::anyhow!("ONNX adapter failed: {e}"))
}

impl Detector {
    pub fn new(
        model: &Path,
        runtime: &Path,
        contract: &ModelInputContract,
        threads: u16,
    ) -> Result<Self> {
        super::telemetry::reset();
        if contract.channels != 3
            || !matches!(contract.layout, TensorLayout::Nchw)
            || !matches!(contract.decoder, DetectorDecoder::YoloChannelMajor)
            || contract.width == 0
            || contract.height == 0
            || contract.width > 1024
            || contract.height > 1024
            || contract.class_count == 0
            || contract.class_count > 256
        {
            bail!("unsupported model contract: require bounded RGB NCHW YOLO channel-major input/output");
        }
        let session = super::model_session::ModelSession::cpu(model, runtime, threads)?;
        Ok(Self {
            session,
            contract: contract.clone(),
            resize: InferenceResize::Stretch,
            padding_byte: 0,
            score_threshold: 0.25,
        })
    }

    /// Caller owns image/device binding, reservations and process confinement.
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    pub fn new_cuda(
        model: &Path,
        runtime: &Path,
        driver: &Path,
        contract: &ModelInputContract,
        options: super::cuda_session::CudaSessionOptions,
    ) -> Result<Self> {
        contract.validate()?;
        if contract.channels != 3
            || !matches!(contract.layout, TensorLayout::Nchw)
            || contract.decoder != DetectorDecoder::YoloChannelMajor
            || contract.width > 1024
            || contract.height > 1024
            || contract.class_count > 256
        {
            bail!("unsupported CUDA detector tensor contract");
        }
        let session = super::model_session::ModelSession::cuda(model, runtime, driver, options)?;
        Ok(Self {
            session,
            contract: contract.clone(),
            resize: InferenceResize::Stretch,
            padding_byte: 0,
            score_threshold: 0.25,
        })
    }
    /// CUDA evidence requires consumed trace finalization. CPU telemetry stays
    /// unchanged and is read separately by the worker.
    pub fn finish(self) -> Result<Option<serde_json::Value>> {
        self.session.finish()
    }
    pub fn from_request(request: &pulsar_protocol::WorkerRequest) -> Result<Self> {
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(
                request.budget.wall_time_ms,
            ))
            .context("detector deadline overflow")?;
        Self::from_request_at(request, deadline)
    }
    pub fn from_request_at(
        request: &pulsar_protocol::WorkerRequest,
        deadline: std::time::Instant,
    ) -> Result<Self> {
        let contract = super::model_session::contract(request)?;
        contract.validate()?;
        if contract.channels != 3
            || !matches!(contract.layout, TensorLayout::Nchw)
            || contract.decoder != DetectorDecoder::YoloChannelMajor
            || contract.width > 1024
            || contract.height > 1024
            || contract.class_count > 256
        {
            bail!("unsupported detector tensor contract");
        }
        let profile = crate::inference_profiles::resolve_bound_profile(request)?
            .context("detector requires a resolved preprocessing profile")?;
        let score_threshold = match &request.operation {
            pulsar_protocol::WorkerOperation::Generate { settings, .. } => {
                settings.effective_detector_score_threshold()
            }
            pulsar_protocol::WorkerOperation::Preview { .. } => 0.25,
            _ => bail!("detector requires generation or preview"),
        };
        Ok(Self {
            session: super::model_session::ModelSession::from_request_at(request, deadline)?,
            contract: contract.clone(),
            resize: profile.preprocessing.resize,
            padding_byte: profile.preprocessing.padding_byte,
            score_threshold,
        })
    }

    pub fn detect(&mut self, rgb: &[u8], width: u32, height: u32) -> Result<Vec<RawDetection>> {
        use pulsar_core::perception::DetectorTransform;
        let transform = std::cell::RefCell::new(None);
        self.session.run(
            || {
                let (input, geometry) = match self.resize {
                    InferenceResize::Stretch => (
                        detector_tensor(rgb, width, height, &self.contract)?,
                        DetectorTransform::resize(
                            width,
                            height,
                            self.contract.width,
                            self.contract.height,
                        )?,
                    ),
                    InferenceResize::Letterbox => {
                        letterbox::tensor(rgb, width, height, &self.contract, self.padding_byte)?
                    }
                    _ => bail!("unsupported detector resize recipe"),
                };
                transform.replace(Some(geometry));
                Ok(input)
            },
            |shape, values| {
                decode_transformed(
                    values,
                    shape,
                    self.contract.class_count as usize,
                    transform
                        .borrow()
                        .as_ref()
                        .context("detector tensor transform absent")?,
                    self.score_threshold,
                )
            },
        )
    }
}

// Keep the exact production tensor recipe available to the opt-in numerical
// campaign. This does not load a model or grant native execution authority.
fn detector_tensor(
    rgb: &[u8],
    width: u32,
    height: u32,
    contract: &ModelInputContract,
) -> Result<Array4<f32>> {
    validate_tensor_source(rgb, width, height, contract)?;
    let w = contract.width as usize;
    let h = contract.height as usize;
    let mut input = Array4::<f32>::zeros((1, 3, h, w));
    for y in 0..h {
        let source_y = y * height as usize / h;
        for x in 0..w {
            let source_x = x * width as usize / w;
            let base = (source_y * width as usize + source_x) * 3;
            for c in 0..3 {
                input[[0, c, y, x]] = rgb[base + c] as f32 / 255.0;
            }
        }
    }
    Ok(input)
}

fn validate_tensor_source(
    rgb: &[u8],
    width: u32,
    height: u32,
    contract: &ModelInputContract,
) -> Result<()> {
    contract.validate()?;
    if contract.channels != 3
        || contract.layout != TensorLayout::Nchw
        || contract.decoder != DetectorDecoder::YoloChannelMajor
        || contract.width > 1024
        || contract.height > 1024
        || contract.class_count > 256
    {
        bail!("unsupported detector tensor contract");
    }
    let expected = super::media::frame_bytes(width, height)?;
    if rgb.len() != expected {
        bail!("RGB tensor source violates its declared layout");
    }
    Ok(())
}

#[path = "detector_letterbox.rs"]
mod letterbox;

#[cfg(test)]
#[path = "detector_parity.rs"]
mod parity;

#[cfg(test)]
fn decode_channel_major(
    values: &[f32],
    shape: &[i64],
    classes: usize,
    input_width: u32,
    input_height: u32,
    threshold: f32,
) -> Result<Vec<RawDetection>> {
    let transform = pulsar_core::perception::DetectorTransform::resize(
        input_width,
        input_height,
        input_width,
        input_height,
    )?;
    decode_transformed(values, shape, classes, &transform, threshold)
}

fn decode_transformed(
    values: &[f32],
    shape: &[i64],
    classes: usize,
    transform: &pulsar_core::perception::DetectorTransform,
    threshold: f32,
) -> Result<Vec<RawDetection>> {
    use pulsar_core::perception::{decode_detections, DetectorLayout};
    let shape = shape
        .iter()
        .map(|dimension| usize::try_from(*dimension))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("negative or overflowing detector dimension")?;
    let detections = decode_detections(
        values,
        &shape,
        DetectorLayout::YoloChannelMajor,
        classes,
        transform,
        f64::from(threshold),
        0.45,
    )?;
    let (input_width, input_height) = transform.source_dimensions();
    Ok(detections
        .into_iter()
        .map(|detection| {
            let bounds = detection.bounds();
            RawDetection {
                class_id: detection.class_id(),
                score: detection.score() as f32,
                bbox: [
                    (bounds.x_min() / f64::from(input_width)) as f32,
                    (bounds.y_min() / f64::from(input_height)) as f32,
                    (bounds.x_max() / f64::from(input_width)) as f32,
                    (bounds.y_max() / f64::from(input_height)) as f32,
                ],
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_score_changes_filtering_and_includes_exact_boundary() {
        let output = [320.0, 320.0, 100.0, 200.0, 0.35];
        assert_eq!(
            decode_channel_major(&output, &[1, 5, 1], 1, 640, 640, 0.25)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            decode_channel_major(&output, &[1, 5, 1], 1, 640, 640, 0.35)
                .unwrap()
                .len(),
            1
        );
        assert!(decode_channel_major(&output, &[1, 5, 1], 1, 640, 640, 0.5)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn decoder_rejects_shape_guessing_nonfinite_and_reversed_boxes() {
        let good = [320.0, 320.0, 100.0, 200.0, 0.9];
        assert_eq!(
            decode_channel_major(&good, &[1, 5, 1], 1, 640, 640, 0.25)
                .unwrap()
                .len(),
            1
        );
        assert!(decode_channel_major(&good, &[1, 1, 5], 1, 640, 640, 0.25).is_err());
        let mut invalid = good;
        invalid[4] = f32::INFINITY;
        assert!(decode_channel_major(&invalid, &[1, 5, 1], 1, 640, 640, 0.25).is_err());
        invalid = good;
        invalid[2] = -5.0;
        assert!(decode_channel_major(&invalid, &[1, 5, 1], 1, 640, 640, 0.25).is_err());
    }

    #[test]
    fn clipping_remains_normalized_and_absence_is_empty() {
        let border = [0.0, 320.0, 100.0, 200.0, 0.9];
        let result = decode_channel_major(&border, &[1, 5, 1], 1, 640, 640, 0.25).unwrap();
        assert_eq!(result[0].bbox[0], 0.0);
        let absent = [320.0, 320.0, 100.0, 200.0, 0.01];
        assert!(decode_channel_major(&absent, &[1, 5, 1], 1, 640, 640, 0.25)
            .unwrap()
            .is_empty());
    }
}
