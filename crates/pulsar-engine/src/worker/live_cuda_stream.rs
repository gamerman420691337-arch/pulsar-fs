//! One CUDA model per bounded private stream. Only explicit finish emits evidence.
use super::*;
#[cfg(all(target_os = "linux", feature = "cuda"))]
use anyhow::ensure;
#[cfg(all(target_os = "linux", feature = "cuda"))]
use std::{io::Write, path::Path};

pub(super) fn run() -> Result<()> {
    crate::cuda_mounts::clear_inherited_descriptors()?;
    #[cfg(not(feature = "cuda"))]
    bail!("CUDA live inference is not compiled; no substitution");
    #[cfg(feature = "cuda")]
    {
        let witness: crate::cuda_mounts::LiveCudaMountWitness =
            read_message(&mut std::io::stdin().lock())?;
        let proof = crate::cuda_mounts::verify_live_bootstrap(&witness)?;
        verify_inputs(&proof)?;
        crate::cuda_bootstrap::reexec_live_stream(witness)
    }
}
pub(super) fn run_exec() -> Result<()> {
    #[cfg(not(feature = "cuda"))]
    {
        crate::cuda_mounts::clear_inherited_descriptors()?;
        bail!("CUDA live inference is not compiled; no substitution");
    }
    #[cfg(feature = "cuda")]
    {
        let relay = crate::cuda_bootstrap::read_live_stream_relay()?;
        crate::cuda_mounts::clear_inherited_descriptors()?;
        let proof = crate::cuda_mounts::verify_live_bootstrap(&relay.witness)?;
        verify_inputs(&proof)?;
        let _cudnn = crate::cuda_bootstrap::load_live_cudnn(&proof)?;
        serve(&proof)
    }
}
#[cfg(feature = "cuda")]
fn verify_inputs(proof: &crate::cuda_mounts::VerifiedLiveCudaBootstrap<'_>) -> Result<()> {
    let manifest = proof.manifest();
    ensure!(
        manifest.stream.model.path == Path::new("/input/model"),
        "CUDA live model requires fixed snapshot mount"
    );
    artifacts::verify_artifact(
        Path::new("/pulsar"),
        &manifest.stream.worker,
        proof.deadline(),
    )?;
    artifacts::verify_artifact(
        &manifest.stream.model.path,
        &manifest.stream.model.identity,
        proof.deadline(),
    )?;
    ensure!(
        Instant::now() < proof.deadline(),
        "CUDA live input deadline expired"
    );
    Ok(())
}
#[cfg(feature = "cuda")]
fn serve(proof: &crate::cuda_mounts::VerifiedLiveCudaBootstrap<'_>) -> Result<()> {
    let manifest = proof.manifest();
    let execution = &manifest.execution;
    let limits = manifest
        .stream
        .profile
        .cuda_resources
        .as_ref()
        .context("CUDA live resource limits missing")?;
    let initialized = temporal_position::TemporalPosition::new_cuda(
        &manifest.stream.model.path,
        &manifest.stream.runtime.path,
        &execution.library_directory.join("libcuda.so.1"),
        &manifest.stream.profile.preprocessing.input,
        cuda_session::CudaSessionOptions {
            device: cuda_identity::CudaSdkExpectation {
                device_uuid: execution.device_uuid.clone(),
                pci_bus_address: execution.pci_bus_address.clone(),
                device_ordinal: u16::try_from(execution.device_ordinal)?,
            },
            arena_limit_bytes: limits.arena_limit_bytes,
            threads: manifest.stream.budget.cpu_threads,
            trace_prefix: manifest.output_dir.join("live_cuda_model"),
            maximum_output_values: 1,
        },
    );
    let mut estimator = match initialized {
        Ok(estimator) => estimator,
        Err(error) => {
            if let Some(reply) = provider_failure::live_cuda_observation(manifest, &error) {
                // The original typed negative query is usable only while the
                // same native bytes and inherited lifetime remain valid.
                proof.verify_after()?;
                verify_inputs(proof)?;
                let mut output = std::io::stdout().lock();
                write_bulk_header(&mut output, &reply)?;
                output.flush()?;
            }
            return Err(error);
        }
    };
    proof.verify_after()?;
    verify_inputs(proof)?;
    let manifest_sha256 = manifest.identity_sha256()?;
    let mut transcript = LiveCudaTranscript::new(manifest)?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    write_bulk_header(
        &mut output,
        &LiveCudaStreamReply::Ready {
            manifest_sha256: manifest_sha256.clone(),
        },
    )?;
    output.flush()?;
    loop {
        ensure!(
            Instant::now() < proof.deadline(),
            "CUDA live stream deadline expired"
        );
        // EOF or malformed input fails the stream; neither is successful finish.
        match read_bulk_header::<_, LiveCudaStreamCommand>(&mut input)? {
            LiveCudaStreamCommand::FramePair { input: issued } => {
                transcript.validate_input(&issued)?;
                let mut pixels = [0u8; 2 * LIVE_FRAME_BYTES];
                input.read_exact(&mut pixels)?;
                for (bytes, identity) in [
                    (&pixels[..LIVE_FRAME_BYTES], &issued.previous),
                    (&pixels[LIVE_FRAME_BYTES..], &issued.current),
                ] {
                    ensure!(
                        format!("{:x}", Sha256::digest(bytes)) == identity.sha256,
                        "CUDA live pixels differ from issued pair"
                    );
                }
                ensure!(
                    Instant::now() < proof.deadline(),
                    "CUDA live input deadline expired"
                );
                let started = Instant::now();
                let position =
                    estimator.estimate(&pixels[..LIVE_FRAME_BYTES], &pixels[LIVE_FRAME_BYTES..])?;
                ensure!(
                    Instant::now() < proof.deadline(),
                    "CUDA live inference exceeded deadline"
                );
                let elapsed = u64::try_from(started.elapsed().as_nanos())?;
                transcript.append(&issued, position, elapsed)?;
                write_bulk_header(
                    &mut output,
                    &LiveCudaStreamReply::Estimated {
                        manifest_sha256: manifest_sha256.clone(),
                        input: *issued,
                        position,
                        pair_analysis_elapsed_ns: elapsed,
                    },
                )?;
                output.flush()?;
            }
            LiveCudaStreamCommand::Finish { expected } => {
                let summary = transcript.finish()?;
                ensure!(
                    summary == expected,
                    "CUDA live parent/worker transcripts differ"
                );
                let mut receipt = estimator
                    .finish()?
                    .context("CUDA live native receipt missing")?;
                receipt
                    .as_object_mut()
                    .context("CUDA live receipt object missing")?
                    .insert("live_binding".into(), serde_json::to_value(&summary)?);
                let trace_path = receipt
                    .get("trace_path")
                    .and_then(serde_json::Value::as_str)
                    .context("CUDA live native trace path missing")?;
                let mut trace_file = crate::artifacts::open_regular(Path::new(trace_path))?;
                let mut trace_bytes = Vec::new();
                Read::by_ref(&mut trace_file)
                    .take(crate::inference_trace::MAX_TRACE_BYTES as u64 + 1)
                    .read_to_end(&mut trace_bytes)?;
                let receipt_bytes = serde_json::to_vec(&receipt)?;
                crate::live_cuda_result::verify_bytes(
                    manifest,
                    &summary,
                    &receipt_bytes,
                    &trace_bytes,
                )?;
                let trace = artifacts::pin_artifact(Path::new(trace_path))?;
                ensure!(
                    trace.identity.sha256 == format!("{:x}", Sha256::digest(&trace_bytes))
                        && trace.identity.byte_len == trace_bytes.len() as u64,
                    "CUDA live native trace changed"
                );
                let remaining = manifest
                    .stream
                    .budget
                    .output_bytes
                    .checked_sub(trace.identity.byte_len)
                    .context("CUDA live trace exceeds output admission")?;
                let receipt = artifacts::write_bytes(
                    &manifest.output_dir,
                    "live-cuda-inference.json",
                    &receipt_bytes,
                    remaining.min(65536),
                )?;
                let artifacts = WorkerCudaArtifacts { receipt, trace };
                artifacts.validate()?;
                ensure!(
                    Instant::now() < proof.deadline(),
                    "CUDA live evidence exceeded deadline"
                );
                write_bulk_header(
                    &mut output,
                    &LiveCudaStreamReply::Finished { summary, artifacts },
                )?;
                // Egress the exact already-checked bytes, without reopening either path.
                for bytes in [&receipt_bytes, &trace_bytes] {
                    for chunk in bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
                        ensure!(
                            Instant::now() < proof.deadline(),
                            "CUDA live egress exceeded deadline"
                        );
                        write_artifact_chunk(&mut output, chunk)?;
                    }
                }
                output.flush()?;
                ensure!(
                    Instant::now() < proof.deadline(),
                    "CUDA live final deadline expired"
                );
                return Ok(());
            }
        }
    }
}
