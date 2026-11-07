//! Declared script-position regression. It is not object detection or calibration.
use super::*;
use ndarray::Array4;

pub struct TemporalPosition {
    session: super::model_session::ModelSession,
    annotated_binding: Option<crate::pack_onnx::AnnotatedModelBinding>,
}
impl TemporalPosition {
    pub fn new(
        model: &SourceArtifact,
        runtime: &ArtifactReference,
        contract: &ModelInputContract,
        threads: u16,
    ) -> Result<Self> {
        contract.validate()?;
        if contract.decoder != DetectorDecoder::TemporalScriptPosition {
            bail!("temporal position decoder differs from declared contract");
        }
        let session = super::model_session::ModelSession::cpu(&model.path, &runtime.path, threads)?;
        Self::from_session(session)
    }
    fn from_session(session: super::model_session::ModelSession) -> Result<Self> {
        Self::from_session_named(session, "frame_pair")
    }
    fn from_session_named(
        session: super::model_session::ModelSession,
        input_name: &str,
    ) -> Result<Self> {
        for (outlets, name, shape) in [
            (session.input(), input_name, &[1, 6, 64, 64][..]),
            (session.output(), "script_position", &[1, 1][..]),
        ] {
            if outlets.len() != 1
                || outlets[0].name() != name
                || outlets[0].dtype().tensor_type() != Some(ort::value::TensorElementType::Float32)
                || outlets[0].dtype().tensor_shape().map(|s| s.as_ref()) != Some(shape)
            {
                bail!("temporal position model does not match fixed named float32 tensor contract");
            }
        }
        Ok(Self {
            session,
            annotated_binding: None,
        })
    }
    /// Explicit CPU adapter for already transformed annotated-view RGB pairs.
    /// Caller owns view application, attempt admission, confinement and lifetime.
    /// File workers use from_request_at to retain the owned attempt lifetime.
    pub fn new_annotated(
        model: &SourceArtifact,
        runtime: &ArtifactReference,
        contract: &ModelInputContract,
        threads: u16,
    ) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(30);
        super::artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
        let file = std::fs::File::open(&model.path)?;
        let binding =
            crate::pack_onnx::validate_annotated(file, model.identity.byte_len, contract)?;
        let session = super::model_session::ModelSession::cpu(&model.path, &runtime.path, threads)?;
        let mut estimator = Self::from_session_named(session, ANNOTATED_TEMPORAL_INPUT_NAME)?;
        super::artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
        estimator.annotated_binding = Some(binding);
        Ok(estimator)
    }
    pub fn annotated_contract(&self) -> Option<&AnnotatedTemporalModelContract> {
        self.annotated_binding
            .as_ref()
            .map(|binding| &binding.contract)
    }
    pub fn training_intake_sha256(&self) -> Option<&str> {
        self.annotated_binding
            .as_ref()
            .map(|binding| binding.training_intake_sha256.as_str())
    }
    /// Caller owns image/device binding, reservations and process confinement.
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    pub fn new_cuda(
        model: &std::path::Path,
        runtime: &std::path::Path,
        driver: &std::path::Path,
        contract: &ModelInputContract,
        options: super::cuda_session::CudaSessionOptions,
    ) -> Result<Self> {
        contract.validate()?;
        if contract.decoder != DetectorDecoder::TemporalScriptPosition {
            bail!("CUDA temporal decoder differs from contract");
        }
        Self::from_session(super::model_session::ModelSession::cuda(
            model, runtime, driver, options,
        )?)
    }
    pub fn finish(self) -> Result<Option<serde_json::Value>> {
        self.session.finish()
    }
    pub fn from_request(request: &WorkerRequest) -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(request.budget.wall_time_ms))
            .context("temporal adapter deadline overflow")?;
        Self::from_request_at(request, deadline)
    }
    pub fn from_request_at(request: &WorkerRequest, deadline: Instant) -> Result<Self> {
        let contract = super::model_session::contract(request)?;
        contract.validate()?;
        if contract.decoder != DetectorDecoder::TemporalScriptPosition {
            bail!("temporal decoder differs from declared contract");
        }
        request.validate()?;
        let annotated = match &request.operation {
            WorkerOperation::Generate {
                model: Some(model),
                settings,
                ..
            } if settings.annotated_view.is_some() => Some(model),
            WorkerOperation::Preview {
                model: Some(model),
                annotated: Some(_),
                ..
            } => Some(model),
            _ => None,
        };
        if let Some(model) = annotated {
            if Instant::now() >= deadline {
                bail!("model adapter attempt deadline expired");
            }
            crate::inference_profiles::resolve_bound_profile(request)?
                .context("annotated profile missing")?;
            super::artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
            let binding = crate::pack_onnx::validate_annotated(
                std::fs::File::open(&model.path)?,
                model.identity.byte_len,
                contract,
            )?;
            if Instant::now() >= deadline {
                bail!("model adapter attempt deadline expired during metadata admission");
            }
            let session = super::model_session::ModelSession::from_request_at(request, deadline)?;
            let mut estimator = Self::from_session_named(session, ANNOTATED_TEMPORAL_INPUT_NAME)?;
            super::artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
            estimator.annotated_binding = Some(binding);
            return Ok(estimator);
        }
        Self::from_session(super::model_session::ModelSession::from_request_at(
            request, deadline,
        )?)
    }
    pub fn estimate(&mut self, previous: &[u8], current: &[u8]) -> Result<NormalizedPosition> {
        self.session.run(
            || {
                if previous.len() != 64 * 64 * 3 || current.len() != 64 * 64 * 3 {
                    bail!("temporal position requires two exact 64x64 RGB frames");
                }
                let mut input = Array4::<f32>::zeros((1, 6, 64, 64));
                for (frame, pixels) in [previous, current].iter().enumerate() {
                    for y in 0..64 {
                        for x in 0..64 {
                            for c in 0..3 {
                                input[[0, frame * 3 + c, y, x]] =
                                    pixels[(y * 64 + x) * 3 + c] as f32 / 255.0;
                            }
                        }
                    }
                }
                Ok(input)
            },
            decode_position,
        )
    }
}
fn decode_position(shape: &[i64], values: &[f32]) -> Result<NormalizedPosition> {
    if shape != [1, 1] || values.len() != 1 {
        bail!("temporal position output must be float32 [1,1]");
    }
    Ok(NormalizedPosition::new(f64::from(values[0]))?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_output_refuses_detector_shapes_nonfinite_and_out_of_range() {
        assert_eq!(decode_position(&[1, 1], &[0.25]).unwrap().value(), 0.25);
        for (shape, values) in [
            (vec![1], vec![0.25]),
            (vec![1, 1], vec![f32::NAN]),
            (vec![1, 1], vec![f32::INFINITY]),
            (vec![1, 1], vec![-0.1]),
            (vec![1, 1], vec![1.1]),
            (vec![1, 1], vec![0.2, 0.3]),
        ] {
            assert!(decode_position(&shape, &values).is_err());
        }
    }
    #[test]
    #[ignore = "requires explicitly retained authored model/RGB fixture and pinned CPU runtime"]
    fn authored_annotated_model_matches_python_cpu_positions() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PULSAR_ANNOTATED_MODEL_FIXTURE_DIR").expect("fixture directory"),
        );
        let expected: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();
        let pinned = pin_artifact(&root.join("model.onnx")).unwrap();
        assert_eq!(
            pinned.identity.sha256,
            expected["model_sha256"].as_str().unwrap()
        );
        let model = SourceArtifact {
            source_version: SourceVersionId::new("authored-annotated-model").unwrap(),
            path: pinned.path,
            identity: pinned.identity,
        };
        let runtime = discover_tools()
            .unwrap()
            .onnx_runtime
            .expect("pinned CPU runtime");
        let contract = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        assert!(crate::pack_onnx::validate(
            std::fs::File::open(&model.path).unwrap(),
            model.identity.byte_len,
            &contract
        )
        .is_err());
        let mut estimator =
            TemporalPosition::new_annotated(&model, &runtime, &contract, 1).unwrap();
        assert_eq!(
            estimator.annotated_contract().unwrap(),
            &AnnotatedTemporalModelContract::expected()
        );
        assert_eq!(
            estimator.training_intake_sha256().unwrap(),
            expected["training_intake_sha256"].as_str().unwrap()
        );
        let rgb = std::fs::read(root.join("pairs.rgb")).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&rgb)),
            expected["rgb_sha256"].as_str().unwrap()
        );
        let positions = expected["positions"].as_array().unwrap();
        assert_eq!(rgb.len(), positions.len() * 2 * 64 * 64 * 3);
        for (pair, position) in rgb
            .as_chunks::<{ 2 * 64 * 64 * 3 }>()
            .0
            .iter()
            .zip(positions)
        {
            let actual = estimator
                .estimate(&pair[..64 * 64 * 3], &pair[64 * 64 * 3..])
                .unwrap();
            assert!((actual.value() - position.as_f64().unwrap()).abs() <= 1e-6);
        }
        estimator.finish().unwrap();
    }
    #[test]
    #[ignore = "requires retained authored model and a fresh pinned CPU runtime process"]
    fn authored_annotated_model_legacy_adapter_refuses() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PULSAR_ANNOTATED_MODEL_FIXTURE_DIR").expect("fixture directory"),
        );
        let pinned = pin_artifact(&root.join("model.onnx")).unwrap();
        let model = SourceArtifact {
            source_version: SourceVersionId::new("authored-annotated-model").unwrap(),
            path: pinned.path,
            identity: pinned.identity,
        };
        let runtime = discover_tools()
            .unwrap()
            .onnx_runtime
            .expect("pinned CPU runtime");
        let contract = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        let error = TemporalPosition::new(&model, &runtime, &contract, 1)
            .err()
            .expect("legacy adapter must refuse annotated input name");
        assert!(
            error
                .to_string()
                .contains("fixed named float32 tensor contract"),
            "{error:#}"
        );
    }
}
