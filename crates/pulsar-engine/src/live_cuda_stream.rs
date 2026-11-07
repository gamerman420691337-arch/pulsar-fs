//! Private persistent GPU transport. Final evidence precedes custody release;
//! estimates remain provisional and confer no device or project authority.
use crate::{launch_pipes::Monitor, worker_retirement::WorkerCustody, EngineConfig};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    process::{ChildStderr, ChildStdin, ChildStdout},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crate::live_cuda_evidence::{EvidenceAdmission, LiveCudaEvidence};

/// Original initialization failure after clean stream EOF, the expected failed
/// child exit and confirmed whole-tree/resource retirement. A deserialized
/// reply cannot construct this capability. It still grants no source, device,
/// equivalence or target-execution authority.
#[derive(Debug)]
pub(crate) struct SettledLiveCudaProviderFailure {
    manifest: LiveCudaInferenceManifest,
    observation: InferenceProviderUnavailable,
    deadline: Instant,
}
impl SettledLiveCudaProviderFailure {
    pub(crate) fn original_deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn capture_metadata(
        &self,
        capture: &LiveCaptureReport,
        worker: &ArtifactIdentity,
    ) -> Result<BrowserInferenceFallbackFailure> {
        self.matches_manifest(&self.manifest)?;
        capture.validate()?;
        let info = capture
            .inference
            .as_ref()
            .context("failed capture inference absent")?;
        let selected = capture
            .options
            .inference
            .as_ref()
            .context("failed capture profile absent")?;
        ensure!(
            capture.active
                && info.state == LiveCaptureInferenceState::Starting
                && info.attempt_id == self.manifest.stream.attempt_id
                && capture.identity == self.manifest.stream.identity
                && capture.clock == self.manifest.stream.clock
                && serde_json::to_vec(&selected.profile)?
                    == serde_json::to_vec(&self.manifest.stream.profile)?
                && info.profile_sha256 == self.observation.profile_sha256
                && self.manifest.stream.worker == *worker
                && info.completed == 0
                && info.accepted == 0
                && info.latest.is_none()
                && info.cuda_manifest_sha256.is_none()
                && info.cuda_evidence.is_none(),
            "settled provider absence differs from original pre-Ready capture"
        );
        Ok(BrowserInferenceFallbackFailure {
            manifest: Box::new(self.manifest.clone()),
            observation: self.observation.clone(),
        })
    }
    pub(crate) fn matches_manifest(&self, manifest: &LiveCudaInferenceManifest) -> Result<()> {
        ensure!(
            self.manifest.identity_sha256()? == manifest.identity_sha256()?,
            "settled live provider failure differs from original manifest"
        );
        ensure!(
            Instant::now() < self.deadline,
            "settled live provider failure exceeded original lifetime"
        );
        self.observation.validate_for(
            &manifest.stream.profile,
            &crate::inference_profiles::profile_identity(&manifest.stream.profile)?.sha256,
        )?;
        Ok(())
    }
}
impl std::fmt::Display for SettledLiveCudaProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "original live worker reports CUDA provider absent after confirmed retirement",
        )
    }
}
impl std::error::Error for SettledLiveCudaProviderFailure {}

#[cfg(test)]
#[path = "live_cuda_failure_tests.rs"]
mod failure_tests;

pub(crate) struct LiveCudaWorker {
    manifest: LiveCudaInferenceManifest,
    scope: crate::live_frames::LiveRgbScope,
    sequence: u64,
    transcript: Option<LiveCudaTranscript>,
    stdin: ChildStdin,
    stdout: ChildStdout,
    monitor: Monitor<'static, ChildStderr>,
    stop: Arc<AtomicBool>,
    custody: Option<WorkerCustody>,
}
impl LiveCudaWorker {
    #[cfg(all(test, feature = "cuda"))]
    pub(crate) fn freeze_for_test(&mut self) -> Result<crate::cgroup::FrozenTestGroup> {
        Ok(self
            .custody
            .as_mut()
            .context("CUDA live custody missing")?
            .group_handle()
            .freeze_for_test()?)
    }
    pub(crate) fn ready_manifest(
        &self,
        scope: &crate::live_frames::LiveRgbScope,
    ) -> Result<&LiveCudaInferenceManifest> {
        self.scope.validate_scope(scope)?;
        Ok(&self.manifest)
    }
    pub(crate) fn provisional(
        &self,
        work: &crate::live_frames::LiveRgbWork,
        reply: LiveCudaStreamReply,
    ) -> Result<LiveInferenceReply> {
        self.scope.validate_work(work)?;
        let LiveCudaStreamReply::Estimated {
            manifest_sha256,
            input,
            position,
            pair_analysis_elapsed_ns,
        } = reply
        else {
            anyhow::bail!("CUDA stream supplied no provisional estimate");
        };
        ensure!(
            manifest_sha256 == self.manifest.identity_sha256()? && input.sequence == self.sequence,
            "CUDA provisional estimate differs from current stream exchange"
        );
        input.validate(&self.manifest.stream)?;
        Ok(LiveInferenceReply::CudaProvisional {
            attempt_id: self.manifest.stream.attempt_id.clone(),
            identity: self.manifest.stream.identity.clone(),
            profile_sha256: crate::inference_profiles::profile_identity(
                &self.manifest.stream.profile,
            )?
            .sha256,
            manifest_sha256,
            input,
            position,
            pair_analysis_elapsed_ns,
        })
    }
    pub(crate) fn start(
        config: &EngineConfig,
        manifest: LiveCudaInferenceManifest,
        admitted: crate::cuda_memory::AdmittedLiveCudaImage,
        prepared: crate::live_staging::GpuPrepared<2>,
        scope: crate::live_frames::LiveRgbScope,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        manifest.validate()?;
        scope.validate_manifest(&manifest.stream)?;
        ensure!(
            manifest.stream.model.path == std::path::Path::new("/input/model")
                && prepared.identities()[0] == manifest.stream.worker
                && prepared.identities()[1] == manifest.stream.model.identity,
            "CUDA live staged worker/model differ from admitted manifest"
        );
        ensure!(
            manifest.stream.budget.memory_bytes <= config.max_ram_bytes
                && Duration::from_millis(manifest.stream.budget.wall_time_ms)
                    <= config.worker_timeout,
            "CUDA live stream exceeds configured limits"
        );
        let mut config = config.clone();
        config.worker_executable = prepared.paths()[0].clone();
        let model = prepared.paths()[1].clone();
        let (image, devices, reservation, end) = admitted.into_launch_parts(&manifest)?;
        let deadline =
            crate::cuda_bootstrap::deadline_instant(end, manifest.stream.budget.wall_time_ms)?;
        let custody = prepared.into_reserved_worker(
            reservation,
            manifest.stream.budget.memory_bytes,
            manifest.stream.budget.cpu_threads,
        )?;
        let plan = crate::cuda_mounts::OwnedLiveCudaMountCommand::stream(
            &config, &manifest, end, image, devices, custody, &model,
        )?;
        let (mut command, mut custody, witness) = plan.into_parts();
        ensure!(
            !cancel.load(Ordering::Acquire) && Instant::now() < deadline,
            "CUDA live launch cancelled or expired"
        );
        custody.spawn(&mut command)?;
        let (stdin, stdout, stderr) = {
            let mut child = custody.child();
            (
                child.stdin.take().context("CUDA live stdin missing")?,
                child.stdout.take().context("CUDA live stdout missing")?,
                child.stderr.take().context("CUDA live stderr missing")?,
            )
        };
        crate::launch_pipes::nonblocking(&stdin)?;
        crate::launch_pipes::nonblocking(&stdout)?;
        crate::launch_pipes::nonblocking(&stderr)?;
        let stop = Arc::new(AtomicBool::new(false));
        let held_stop = stop.clone();
        let held_cancel = cancel.clone();
        let child = custody.child_handle();
        let group = custody.group_handle();
        let watchdog = std::thread::Builder::new()
            .name("pulsar-live-cuda-watchdog".into())
            .spawn(move || {
                loop {
                    if held_stop.load(Ordering::Acquire)
                        || held_cancel.load(Ordering::Acquire)
                        || Instant::now() >= deadline
                    {
                        break;
                    }
                    if let Ok(mut child) = child.try_lock() {
                        match child.try_wait() {
                            Ok(None) => {}
                            _ => return,
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                crate::worker_retirement::signal_and_poll_child(
                    &group,
                    &child,
                    Instant::now() + Duration::from_millis(250),
                );
            })?;
        custody.retain_thread(watchdog);
        let transcript = LiveCudaTranscript::new(&manifest)?;
        let mut worker = Self {
            manifest,
            scope,
            sequence: 0,
            transcript: Some(transcript),
            stdin,
            stdout,
            monitor: Monitor::owned(deadline, cancel, stderr),
            stop,
            custody: Some(custody),
        };
        write_message(&mut worker.monitor.write(&mut worker.stdin), &witness)?;
        let ready: LiveCudaStreamReply =
            read_bulk_header(&mut worker.monitor.read(&mut worker.stdout))?;
        if matches!(ready, LiveCudaStreamReply::ProviderUnavailable { .. }) {
            return Err(worker.settle_provider_unavailable(ready)?.into());
        }
        ensure!(
            matches!(ready, LiveCudaStreamReply::Ready { manifest_sha256 }
            if manifest_sha256 == worker.manifest.identity_sha256()?),
            "CUDA live readiness differs"
        );
        Ok(worker)
    }

    fn settle_provider_unavailable(
        &mut self,
        reply: LiveCudaStreamReply,
    ) -> Result<SettledLiveCudaProviderFailure> {
        self.monitor.check()?;
        ensure!(
            self.sequence == 0,
            "provider absence must precede live Ready"
        );
        let observation = reply.validate_provider_unavailable(&self.manifest)?.clone();
        ensure!(
            self.monitor.read(&mut self.stdout).read(&mut [0u8; 1])? == 0,
            "live provider failure has undeclared trailing output"
        );
        loop {
            self.monitor.tick()?;
            let status = self
                .custody
                .as_mut()
                .context("live provider failure custody absent")?
                .child()
                .try_wait()?;
            if let Some(status) = status {
                // The original worker entry exits 1 on a returned error. A
                // signal, successful exit or unrelated status cannot qualify.
                ensure!(
                    status.code() == Some(1),
                    "live provider failure exit differs"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.stop.store(true, Ordering::Release);
        self.custody
            .as_mut()
            .context("live provider failure custody absent")?
            .retire(self.monitor.deadline)?;
        // Drop the confirmed owner before returning proof: it holds CPU/RAM,
        // GPU admission, native images, staging, watchdog and fence obligations.
        drop(self.custody.take());
        self.monitor.check()?;
        self.transcript = None;
        let settled = SettledLiveCudaProviderFailure {
            manifest: self.manifest.clone(),
            observation,
            deadline: self.monitor.deadline,
        };
        settled.matches_manifest(&self.manifest)?;
        Ok(settled)
    }
    /// Borrow only runtime-issued pixels; the caller retains ownership for
    /// independent freshness/completion. Failed scope or exchange retires us.
    pub(crate) fn estimate(
        &mut self,
        work: &crate::live_frames::LiveRgbWork,
    ) -> Result<LiveCudaStreamReply> {
        let result = (|| {
            self.scope.validate_work(work)?;
            let identity = |bytes: &[u8]| {
                let sha256 = format!("{:x}", Sha256::digest(bytes));
                ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone()).expect("SHA256"),
                    sha256,
                    byte_len: bytes.len() as u64,
                }
            };
            let input = LiveInferenceInput {
                sequence: self
                    .sequence
                    .checked_add(1)
                    .context("CUDA live sequence overflow")?,
                pair: work.declaration(),
                previous: identity(work.previous()),
                current: identity(work.current()),
            };
            self.exchange(&input, work.previous(), work.current())
        })();
        if result.is_err() {
            self.close();
        }
        result
    }
    fn exchange(
        &mut self,
        input: &LiveInferenceInput,
        previous: &[u8],
        current: &[u8],
    ) -> Result<LiveCudaStreamReply> {
        let result = self.exchange_inner(input, previous, current);
        if result.is_err() {
            self.close();
        }
        result
    }
    fn exchange_inner(
        &mut self,
        input: &LiveInferenceInput,
        previous: &[u8],
        current: &[u8],
    ) -> Result<LiveCudaStreamReply> {
        self.monitor.check()?;
        let transcript = self
            .transcript
            .as_mut()
            .context("CUDA live stream retired")?;
        transcript.validate_input(input)?;
        ensure!(
            previous.len() == LIVE_FRAME_BYTES && current.len() == LIVE_FRAME_BYTES,
            "CUDA live pair byte admission"
        );
        for (bytes, identity) in [(previous, &input.previous), (current, &input.current)] {
            ensure!(
                format!("{:x}", Sha256::digest(bytes)) == identity.sha256,
                "CUDA live issued pixel hash differs"
            );
        }
        let mut output = self.monitor.write(&mut self.stdin);
        write_bulk_header(
            &mut output,
            &LiveCudaStreamCommand::FramePair {
                input: (input.clone()).into(),
            },
        )?;
        output.write_all(previous)?;
        output.write_all(current)?;
        output.flush()?;
        let reply: LiveCudaStreamReply =
            read_bulk_header(&mut self.monitor.read(&mut self.stdout))?;
        match &reply {
            LiveCudaStreamReply::Estimated {
                manifest_sha256,
                input: returned,
                position,
                pair_analysis_elapsed_ns,
            } => {
                ensure!(
                    *manifest_sha256 == self.manifest.identity_sha256()? && *returned == *input,
                    "CUDA live reply manifest or input differs"
                );
                transcript.append(input, *position, *pair_analysis_elapsed_ns)?;
                self.sequence = input.sequence;
            }
            _ => anyhow::bail!("CUDA live estimate reply differs"),
        }
        Ok(reply)
    }
    pub(crate) fn finish(
        mut self,
        pool: &crate::resources::ResourcePool,
    ) -> Result<LiveCudaEvidence> {
        let admission = EvidenceAdmission::reserve(pool)?;
        let summary = self
            .transcript
            .take()
            .context("CUDA live stream retired")?
            .finish()?;
        write_bulk_header(
            &mut self.monitor.write(&mut self.stdin),
            &LiveCudaStreamCommand::Finish {
                expected: summary.clone(),
            },
        )?;
        let reply: LiveCudaStreamReply = read_bulk_header(&mut self.monitor.read(&mut self.stdout))
            .with_context(|| {
                format!(
                    "CUDA live final reply failed; worker diagnostics: {}",
                    String::from_utf8_lossy(self.monitor.diagnostics())
                )
            })?;
        let artifacts = match reply {
            LiveCudaStreamReply::Finished {
                summary: returned,
                artifacts,
            } if returned == summary => artifacts,
            _ => anyhow::bail!("CUDA live final transcript differs"),
        };
        artifacts.validate()?;
        ensure!(
            artifacts
                .receipt
                .identity
                .byte_len
                .checked_add(artifacts.trace.identity.byte_len)
                .is_some_and(|total| total <= self.manifest.stream.budget.output_bytes),
            "CUDA live egress exceeds admission"
        );
        let receipt = receive(
            &mut self.monitor.read(&mut self.stdout),
            &self.manifest,
            &artifacts.receipt,
            65536,
        )?;
        let trace = receive(
            &mut self.monitor.read(&mut self.stdout),
            &self.manifest,
            &artifacts.trace,
            crate::inference_trace::MAX_TRACE_BYTES as u64,
        )?;
        let evidence = admission.finished(&self.manifest, &summary, receipt, trace)?;
        ensure!(
            self.monitor.read(&mut self.stdout).read(&mut [0u8; 1])? == 0,
            "CUDA live worker emitted trailing bytes"
        );
        loop {
            self.monitor.tick()?;
            let status = self
                .custody
                .as_mut()
                .context("CUDA live custody missing")?
                .child()
                .try_wait()?;
            if let Some(status) = status {
                ensure!(
                    status.success(),
                    "CUDA live worker failed after final evidence"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.stop.store(true, Ordering::Release);
        self.custody
            .take()
            .context("CUDA live custody missing")?
            .retire(self.monitor.deadline)?;
        self.monitor.check()?;
        Ok(evidence)
    }
    #[cfg(test)]
    pub(crate) fn await_idle_exit_for_test(&mut self, end: Instant) -> Result<()> {
        loop {
            let custody = self.custody.as_mut().context("CUDA live custody missing")?;
            let exited = custody.child().try_wait()?.is_some();
            if exited && custody.group_handle().is_empty()? {
                return Ok(());
            }
            ensure!(
                Instant::now() < end,
                "CUDA live watchdog did not retire idle child"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(test)]
    pub(crate) fn reject_command_for_test(
        &mut self,
        command: &LiveCudaStreamCommand,
        pixels: &[u8],
    ) -> Result<String> {
        write_bulk_header(&mut self.monitor.write(&mut self.stdin), command)?;
        // Invalid control input may close stdin before the deliberately supplied
        // bytes finish writing. Still require EOF and a failed direct child.
        let _ = self.monitor.write(&mut self.stdin).write_all(pixels);
        let reply: std::result::Result<LiveCudaStreamReply, _> =
            read_bulk_header(&mut self.monitor.read(&mut self.stdout));
        ensure!(
            matches!(&reply, Err(TransportError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof),
            "invalid native stream did not close without a successful reply"
        );
        let status = loop {
            self.monitor.tick()?;
            if let Some(status) = self
                .custody
                .as_mut()
                .context("CUDA live custody missing")?
                .child()
                .try_wait()?
            {
                break status;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        ensure!(
            !status.success(),
            "invalid native stream returned successful exit"
        );
        self.monitor.tick()?;
        let diagnostic = String::from_utf8_lossy(self.monitor.diagnostics()).into_owned();
        self.close();
        Ok(diagnostic)
    }
    fn close(&mut self) {
        self.transcript = None;
        self.stop.store(true, Ordering::Release);
        if let Some(mut custody) = self.custody.take() {
            let _ = custody.retire(Instant::now() + Duration::from_millis(250));
        }
    }
}
impl Drop for LiveCudaWorker {
    fn drop(&mut self) {
        self.close();
    }
}
fn receive(
    reader: &mut impl Read,
    manifest: &LiveCudaInferenceManifest,
    artifact: &ArtifactReference,
    limit: u64,
) -> Result<Vec<u8>> {
    artifact.identity.validate()?;
    ensure!(
        artifact.path.parent() == Some(manifest.output_dir.as_path())
            && artifact.identity.byte_len > 0
            && artifact.identity.byte_len <= limit,
        "CUDA live artifact path/length admission"
    );
    let mut bytes = Vec::with_capacity(usize::try_from(artifact.identity.byte_len)?);
    while bytes.len() as u64 != artifact.identity.byte_len {
        let chunk = read_artifact_chunk(reader)?;
        ensure!(
            !chunk.is_empty()
                && chunk.len() as u64 <= artifact.identity.byte_len - bytes.len() as u64,
            "CUDA live chunk exceeds remaining artifact bytes"
        );
        bytes.extend_from_slice(&chunk);
    }
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == artifact.identity.sha256,
        "CUDA live egress hash differs"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_egress_reassembles_exact_bytes_and_refuses_length_hash_path_truncation() {
        let manifest = crate::cuda_mounts::live_test_witness().manifest;
        let bytes = b"exact-evidence";
        let sha = format!("{:x}", Sha256::digest(bytes));
        let artifact = ArtifactReference {
            path: manifest.output_dir.join("receipt.json"),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new(sha.clone()).unwrap(),
                sha256: sha,
                byte_len: bytes.len() as u64,
            },
        };
        let mut framed = Vec::new();
        write_artifact_chunk(&mut framed, &bytes[..3]).unwrap();
        write_artifact_chunk(&mut framed, &bytes[3..]).unwrap();
        assert_eq!(
            receive(&mut &framed[..], &manifest, &artifact, 65536).unwrap(),
            bytes
        );
        let mut bad = artifact.clone();
        bad.identity.byte_len -= 1;
        assert!(receive(&mut &framed[..], &manifest, &bad, 65536).is_err());
        let mut bad = artifact.clone();
        bad.identity.byte_len += 1;
        assert!(receive(&mut &framed[..], &manifest, &bad, 65536).is_err());
        let mut bad = artifact.clone();
        bad.identity.sha256 = "0".repeat(64);
        assert!(receive(&mut &framed[..], &manifest, &bad, 65536).is_err());
        let mut bad = artifact.clone();
        bad.path = "/outside/receipt.json".into();
        assert!(receive(&mut &framed[..], &manifest, &bad, 65536).is_err());
        assert!(receive(
            &mut &framed[..framed.len() - 1],
            &manifest,
            &artifact,
            65536
        )
        .is_err());
        assert!(receive(&mut &framed[..], &manifest, &artifact, 1).is_err());
    }
}
