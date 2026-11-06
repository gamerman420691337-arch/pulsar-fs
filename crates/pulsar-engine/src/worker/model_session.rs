//! Shared model effect lifetime. Decoded CUDA failures poison the whole attempt.
use super::detector::{cpu_session, native};
use anyhow::{ensure, Context, Result};
use ndarray::Array4;
use ort::{
    session::Session,
    value::{Outlet, Tensor},
};
use std::path::Path;
use std::time::Instant;

enum Backend {
    Cpu(Session),
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    Cuda(Box<super::cuda_session::CheckedCudaSession>),
}
pub(super) struct ModelSession {
    backend: Backend,
    failed: bool,
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    runs: u64,
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    binding: Option<serde_json::Value>,
    deadline: Option<Instant>,
    no_inference_preview: bool,
}
pub(super) fn contract(
    request: &pulsar_protocol::WorkerRequest,
) -> Result<&pulsar_protocol::ModelInputContract> {
    match &request.operation {
        pulsar_protocol::WorkerOperation::Generate { settings, .. } => {
            settings.model_input.as_ref()
        }
        pulsar_protocol::WorkerOperation::Preview { model_input, .. } => model_input.as_ref(),
        _ => None,
    }
    .context("model adapter requires declared input contract")
}
impl ModelSession {
    pub fn cpu(model: &Path, runtime: &Path, threads: u16) -> Result<Self> {
        Ok(Self {
            backend: Backend::Cpu(cpu_session(model, runtime, threads)?),
            failed: false,
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            runs: 0,
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            binding: None,
            deadline: None,
            no_inference_preview: false,
        })
    }
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    pub fn cuda(
        model: &Path,
        runtime: &Path,
        driver: &Path,
        options: super::cuda_session::CudaSessionOptions,
    ) -> Result<Self> {
        // CUDA must never reuse the thread's preceding CPU telemetry.
        super::telemetry::clear();
        Ok(Self {
            backend: Backend::Cuda(Box::new(super::cuda_session::CheckedCudaSession::new(
                model, runtime, driver, options,
            )?)),
            failed: false,
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            runs: 0,
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            binding: None,
            deadline: None,
            no_inference_preview: false,
        })
    }
    pub fn from_request_at(
        request: &pulsar_protocol::WorkerRequest,
        deadline: Instant,
    ) -> Result<Self> {
        use pulsar_protocol::*;
        request.validate()?;
        ensure!(
            Instant::now() < deadline,
            "model adapter attempt deadline expired"
        );
        let (model, profile) = match &request.operation {
            WorkerOperation::Generate {
                model, settings, ..
            } => (model.as_ref(), settings.inference_profile.as_ref()),
            WorkerOperation::Preview {
                model,
                inference_profile,
                ..
            } => (model.as_ref(), inference_profile.as_deref()),
            _ => (None, None),
        };
        let model = model.context("model adapter requires a model")?;
        let runtime = request
            .tools
            .onnx_runtime
            .as_ref()
            .context("model adapter requires pinned runtime")?;
        let mut session = match profile
            .map(|p| p.provider)
            .unwrap_or(InferenceProvider::Cpu)
        {
            InferenceProvider::Cpu => {
                Self::cpu(&model.path, &runtime.path, request.budget.cpu_threads)
            }
            InferenceProvider::Cuda => {
                #[cfg(not(all(target_os = "linux", feature = "cuda")))]
                anyhow::bail!("CUDA model adapter is not compiled; no automatic substitution");
                #[cfg(all(target_os = "linux", feature = "cuda"))]
                {
                    use sha2::{Digest, Sha256};
                    let profile = profile.context("CUDA model profile missing")?;
                    crate::inference_profiles::admit_tensor_adapter(profile, &request.budget)?;
                    let execution = request
                        .cuda_execution
                        .as_ref()
                        .context("CUDA execution declaration missing")?;
                    super::artifacts::verify_artifact(&model.path, &model.identity, deadline)?;
                    for library in &execution.libraries {
                        for name in &library.names {
                            super::artifacts::verify_artifact(
                                &execution.library_directory.join(name),
                                &library.identity,
                                deadline,
                            )?;
                        }
                    }
                    let limits = profile
                        .cuda_resources
                        .as_ref()
                        .context("CUDA resource limits missing")?;
                    let mut session = Self::cuda(
                        &model.path,
                        &execution.library_directory.join("libonnxruntime.so.1"),
                        &execution.library_directory.join("libcuda.so.1"),
                        super::cuda_session::CudaSessionOptions {
                            device: super::cuda_identity::CudaSdkExpectation {
                                device_uuid: execution.device_uuid.clone(),
                                pci_bus_address: execution.pci_bus_address.clone(),
                                device_ordinal: u16::try_from(execution.device_ordinal)?,
                            },
                            arena_limit_bytes: limits.arena_limit_bytes,
                            threads: request.budget.cpu_threads,
                            trace_prefix: request.output_dir.join("cuda_model"),
                            maximum_output_values: 4 * 1024 * 1024,
                        },
                    )?;
                    session.binding = Some(
                        serde_json::json!({"attempt_id":request.attempt_id,"job_id":request.job_id,
                        "request_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(request)?)),
                        "profile_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(profile)?)),
                        "model":profile.model,"runtime":profile.runtime,"owner_policy":execution.owner_policy}),
                    );
                    Ok(session)
                }
            }
            _ => anyhow::bail!(
                "unsupported pinned model adapter provider; no automatic substitution"
            ),
        }?;
        ensure!(
            Instant::now() < deadline,
            "model adapter attempt deadline expired during initialization"
        );
        session.deadline = Some(deadline);
        session.no_inference_preview = matches!(
            &request.operation,
            WorkerOperation::Preview {
                annotated: Some(_),
                ..
            }
        );
        Ok(session)
    }
    pub fn input(&self) -> &[Outlet] {
        match &self.backend {
            Backend::Cpu(s) => s.inputs(),
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            Backend::Cuda(s) => std::slice::from_ref(s.input()),
        }
    }
    pub fn output(&self) -> &[Outlet] {
        match &self.backend {
            Backend::Cpu(s) => s.outputs(),
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            Backend::Cuda(s) => std::slice::from_ref(s.output()),
        }
    }
    /// Preprocessing and decoding belong to the same failure scope as native
    /// execution. CPU remains the existing borrowed-output adapter.
    pub fn run<T>(
        &mut self,
        input: impl FnOnce() -> Result<Array4<f32>>,
        decode: impl FnOnce(&[i64], &[f32]) -> Result<T>,
    ) -> Result<T> {
        ensure!(!self.failed, "model adapter session is failed");
        if self.deadline.is_some_and(|end| Instant::now() >= end) {
            self.failed = true;
            anyhow::bail!("model adapter attempt deadline expired");
        }
        let result = (|| match &mut self.backend {
            Backend::Cpu(session) => {
                let tensor = native(Tensor::from_array(input()?))?;
                let started = std::time::Instant::now();
                let outputs = native(session.run(ort::inputs![tensor]))?;
                super::telemetry::completed(started.elapsed());
                ensure!(
                    outputs.len() == 1,
                    "model contract requires exactly one output"
                );
                let (_, output) = outputs.into_iter().next().context("missing model output")?;
                let (shape, values) = native(output.try_extract_tensor::<f32>())?;
                decode(shape.as_ref(), values)
            }
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            Backend::Cuda(session) => {
                self.failed = true;
                let output = session.run_f32(input()?)?;
                let decoded = decode(&output.shape, &output.values)?;
                self.runs = self
                    .runs
                    .checked_add(1)
                    .context("model adapter run overflow")?;
                self.failed = false;
                Ok(decoded)
            }
        })();
        if self.deadline.is_some_and(|end| Instant::now() >= end) {
            self.failed = true;
            anyhow::bail!("model adapter attempt deadline expired during execution");
        }
        result
    }
    pub fn finish(self) -> Result<Option<serde_json::Value>> {
        ensure!(
            !self.deadline.is_some_and(|end| Instant::now() >= end),
            "model adapter attempt deadline expired; receipt refused"
        );
        ensure!(
            !self.failed,
            "model adapter session failed; receipt refused"
        );
        match self.backend {
            Backend::Cpu(_) => Ok(None),
            #[cfg(all(target_os = "linux", feature = "cuda"))]
            Backend::Cuda(session) => {
                let session = *session;
                let initialization_only = self.no_inference_preview && self.runs == 0;
                let mut receipt = serde_json::to_value(if initialization_only {
                    session.finish_without_inference()?
                } else {
                    session.finish(self.runs)?
                })?;
                ensure!(
                    !self.deadline.is_some_and(|end| Instant::now() >= end),
                    "model adapter attempt deadline expired during trace finalization"
                );
                receipt["provider"] = "cuda".into();
                receipt["provider_evidence"] = if initialization_only {
                    "cuda-session-initialized-without-inference"
                } else {
                    "strict-cuda-node-trace"
                }
                .into();
                if let Some(binding) = self.binding {
                    receipt["worker_binding"] = binding;
                    let trace_path = receipt["trace_path"]
                        .as_str()
                        .context("CUDA trace path missing")?;
                    let trace = super::artifacts::pin_artifact(Path::new(trace_path))?;
                    ensure!(
                        trace.path == Path::new(trace_path),
                        "CUDA trace is not a canonical attempt file"
                    );
                    ensure!(
                        trace.identity.sha256
                            == receipt["trace"]["trace_sha256"]
                                .as_str()
                                .context("CUDA trace digest missing")?,
                        "CUDA trace changed after finalization"
                    );
                    let directory = trace
                        .path
                        .parent()
                        .context("CUDA trace directory missing")?;
                    let evidence = pulsar_protocol::WorkerCudaArtifacts {
                        receipt: super::artifacts::write_json(
                            directory,
                            "cuda-inference.json",
                            &receipt,
                            65536,
                        )?,
                        trace,
                    };
                    evidence.validate()?;
                    ensure!(
                        !self.deadline.is_some_and(|end| Instant::now() >= end),
                        "model adapter attempt deadline expired during evidence publication"
                    );
                    super::telemetry::set_cuda(evidence);
                }
                Ok(Some(receipt))
            }
        }
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn temporal_factory_refuses_expired_attempt_before_native_loading() {
        let request: pulsar_protocol::WorkerRequest = serde_json::from_str(include_str!(
            "../../tests/fixtures/cuda-intent-request.json"
        ))
        .unwrap();
        request.validate().unwrap();
        super::super::telemetry::clear();
        let expired = Instant::now()
            .checked_sub(std::time::Duration::from_secs(1))
            .unwrap();
        let error =
            super::super::temporal_position::TemporalPosition::from_request_at(&request, expired)
                .err()
                .expect("expired temporal factory must refuse");
        assert_eq!(error.to_string(), "model adapter attempt deadline expired");
        assert!(super::super::telemetry::receipt().is_none());
        assert!(super::super::telemetry::take_cuda().is_none());
    }
}
