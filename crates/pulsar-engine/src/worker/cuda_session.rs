//! Strict isolated-worker CUDA session. Not wired to production admission yet.
use super::{
    cuda_identity::{CudaSdkDevice, CudaSdkExpectation, CudaSdkObservation},
    detector::native,
};
use anyhow::{ensure, Context, Result};
use ndarray::Array4;
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::{Outlet, Tensor, TensorElementType},
};
use pulsar_protocol::InferenceProvider;
use serde::Serialize;
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_VALUES: usize = 4 * 1024 * 1024;
const MAX_RUNS: u64 = 16_384;

pub struct CudaSessionOptions {
    pub device: CudaSdkExpectation,
    /// CUDA EP arena only, not a whole-device allocation quota.
    pub arena_limit_bytes: u64,
    pub threads: u16,
    /// Parent-owned private output path inside the worker namespace.
    pub trace_prefix: PathBuf,
    pub maximum_output_values: usize,
}
impl CudaSessionOptions {
    pub fn validate(&self) -> Result<()> {
        self.device.validate()?;
        ensure!(
            self.arena_limit_bytes > 0
                && self.arena_limit_bytes <= 1u64 << 40
                && usize::try_from(self.arena_limit_bytes).is_ok(),
            "invalid CUDA session arena limit"
        );
        ensure!(
            (1..=256).contains(&self.threads)
                && (1..=MAX_VALUES).contains(&self.maximum_output_values),
            "invalid CUDA session thread/output bounds"
        );
        let name = self
            .trace_prefix
            .file_name()
            .and_then(|s| s.to_str())
            .context("missing CUDA trace prefix")?;
        ensure!(
            self.trace_prefix.is_absolute()
                && !self
                    .trace_prefix
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
                && !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "invalid CUDA trace path"
        );
        Ok(())
    }
}

/// CPU-owned copies only; native output values cannot escape the SDK lifetime.
pub struct FloatTensorOutput {
    pub shape: Vec<i64>,
    pub values: Vec<f32>,
}
#[derive(Serialize)]
pub struct CudaSessionReceipt {
    pub device: CudaSdkObservation,
    pub successful_runs: u64,
    pub trace_path: PathBuf,
    pub trace: crate::inference_trace::StrictNodeTrace,
}
pub struct CheckedCudaSession {
    // Declaration order drops the native session before the held driver.
    session: Session,
    device: CudaSdkDevice,
    options: CudaSessionOptions,
    runs: u64,
    poisoned: bool,
}
impl CheckedCudaSession {
    /// Caller owns captured model/runtime/driver identities, confinement,
    /// reservation and process deadlines. No engine authority is granted here.
    pub fn new(
        model: &Path,
        runtime: &Path,
        driver: &Path,
        options: CudaSessionOptions,
    ) -> Result<Self> {
        options.validate()?;
        let device = CudaSdkDevice::open(driver, options.device.clone())?;
        ensure!(
            ort::init_from(runtime)
                .map_err(|e| anyhow::anyhow!("load selected CUDA ONNX runtime: {e}"))?
                .with_name("pulsar-isolated-cuda-worker")
                .commit(),
            "ONNX runtime was already initialized; a new attempt process is required"
        );
        let cuda = ort::ep::CUDA::default()
            .with_device_id(i32::from(options.device.device_ordinal))
            .with_memory_limit(usize::try_from(options.arena_limit_bytes)?)
            .with_arena_extend_strategy(ort::ep::ArenaExtendStrategy::SameAsRequested)
            .with_tf32(false);
        // Driver identity and pinned runtime initialization remain prerequisites.
        // Only an explicit absence from this runtime's provider list is typed.
        super::provider_failure::require_cuda_available(&cuda)?;
        let ep = cuda.build().error_on_failure();
        let builder = native(Session::builder())?;
        let builder = native(builder.with_disable_cpu_fallback())?;
        let builder = native(builder.with_execution_providers([ep]))?;
        let builder = native(builder.with_intra_threads(usize::from(options.threads)))?;
        let builder = native(builder.with_inter_threads(1))?;
        let builder = native(builder.with_optimization_level(GraphOptimizationLevel::Level3))?;
        let mut builder = native(builder.with_profiling(&options.trace_prefix))?;
        let session = native(builder.commit_from_file(model))?;
        ensure!(
            session.inputs().len() == 1
                && session.outputs().len() == 1
                && session.inputs()[0].dtype().tensor_type() == Some(TensorElementType::Float32)
                && session.outputs()[0].dtype().tensor_type() == Some(TensorElementType::Float32),
            "CUDA adapter requires one float32 input and output"
        );
        device.recheck()?;
        Ok(Self {
            session,
            device,
            options,
            runs: 0,
            poisoned: false,
        })
    }
    pub fn input(&self) -> &Outlet {
        &self.session.inputs()[0]
    }
    pub fn output(&self) -> &Outlet {
        &self.session.outputs()[0]
    }
    pub fn device(&self) -> &CudaSdkObservation {
        self.device.initial()
    }

    /// Synchronous float32 NCHW run. Any error permanently invalidates this
    /// attempt's evidence; no native value or session handle is returned.
    pub fn run_f32(&mut self, input: Array4<f32>) -> Result<FloatTensorOutput> {
        ensure!(!self.poisoned, "CUDA session is failed");
        self.poisoned = true;
        ensure!(self.runs < MAX_RUNS, "CUDA session run bound exceeded");
        validate_input(input.shape(), input.iter().copied())?;
        self.device.recheck()?;
        let tensor = native(Tensor::from_array(input))?;
        let copied = {
            let outputs = native(self.session.run(ort::inputs![tensor]))?;
            ensure!(outputs.len() == 1, "CUDA output count changed");
            let (shape, values) = native(outputs[0].try_extract_tensor::<f32>())?;
            ensure!(
                !values.is_empty()
                    && values.len() <= self.options.maximum_output_values
                    && values.iter().all(|v| v.is_finite()),
                "CUDA output copy bound or finiteness failure"
            );
            FloatTensorOutput {
                shape: shape.to_vec(),
                values: values.to_vec(),
            }
        };
        self.device.recheck()?;
        self.runs += 1;
        self.poisoned = false;
        Ok(copied)
    }
    /// Consume the session and independently check the caller's expected count
    /// against successful calls and the bounded native provider trace.
    pub fn finish(self, expected_runs: u64) -> Result<CudaSessionReceipt> {
        ensure!(
            !self.poisoned && expected_runs > 0 && expected_runs == self.runs,
            "CUDA session failed or expected run count differs"
        );
        self.finish_trace(expected_runs, false)
    }
    pub(super) fn finish_without_inference(self) -> Result<CudaSessionReceipt> {
        ensure!(
            !self.poisoned && self.runs == 0,
            "no-inference session executed or failed"
        );
        self.finish_trace(0, true)
    }
    fn finish_trace(
        mut self,
        expected_runs: u64,
        initialization_only: bool,
    ) -> Result<CudaSessionReceipt> {
        self.device.recheck()?;
        let trace_path = PathBuf::from(native(self.session.end_profiling())?);
        let prefix = self
            .options
            .trace_prefix
            .file_name()
            .and_then(|s| s.to_str())
            .context("trace prefix")?;
        let name = trace_path
            .file_name()
            .and_then(|s| s.to_str())
            .context("trace name")?;
        ensure!(
            trace_path.parent() == self.options.trace_prefix.parent()
                && name.starts_with(&format!("{prefix}_"))
                && name.ends_with(".json"),
            "CUDA trace escaped output prefix"
        );
        let mut file = crate::artifacts::open_regular(&trace_path)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(crate::inference_trace::MAX_TRACE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let trace = if initialization_only {
            crate::inference_trace::validate_initialization_trace(&bytes)?
        } else {
            crate::inference_trace::validate_node_trace(
                &bytes,
                InferenceProvider::Cuda,
                expected_runs,
            )?
        };
        let device = self.device.recheck()?;
        Ok(CudaSessionReceipt {
            device,
            successful_runs: self.runs,
            trace_path,
            trace,
        })
    }
}
fn validate_input(shape: &[usize], mut values: impl Iterator<Item = f32>) -> Result<()> {
    let count = shape
        .iter()
        .try_fold(1usize, |n, d| n.checked_mul(*d))
        .context("CUDA input size overflow")?;
    ensure!(
        shape.len() == 4 && shape[0] == 1 && shape.iter().all(|d| *d > 0) && count <= MAX_VALUES,
        "CUDA input requires bounded single-batch NCHW"
    );
    ensure!(values.all(|v| v.is_finite()), "CUDA input must be finite");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> CudaSessionOptions {
        CudaSessionOptions {
            device: CudaSdkExpectation {
                device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
                pci_bus_address: "0000:01:00.0".into(),
                device_ordinal: 0,
            },
            arena_limit_bytes: 64 << 20,
            threads: 2,
            trace_prefix: "/output/trace".into(),
            maximum_output_values: 1,
        }
    }
    #[test]
    fn options_refuse_before_native_loading() {
        options().validate().unwrap();
        let mut o = options();
        o.arena_limit_bytes = 0;
        assert!(o.validate().is_err());
        let mut o = options();
        o.threads = 0;
        assert!(o.validate().is_err());
        let mut o = options();
        o.maximum_output_values = MAX_VALUES + 1;
        assert!(o.validate().is_err());
        let mut o = options();
        o.trace_prefix = "/output/../trace".into();
        assert!(o.validate().is_err());
        let mut o = options();
        o.trace_prefix = "/output/trace.json".into();
        assert!(o.validate().is_err());
    }
    #[test]
    fn input_copy_contract_is_bounded_and_finite() {
        validate_input(&[1, 6, 64, 64], [0.0, 1.0].into_iter()).unwrap();
        for shape in [
            &[2, 6, 64, 64][..],
            &[1, 0, 64, 64],
            &[1, 6, usize::MAX, 64],
            &[1, 6, 1024, 1024],
            &[1, 6, 64],
        ] {
            assert!(validate_input(shape, [0.0].into_iter()).is_err());
        }
        assert!(validate_input(&[1, 6, 64, 64], [f32::NAN].into_iter()).is_err());
    }
}
