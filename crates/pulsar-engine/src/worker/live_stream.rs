//! One loaded CPU model serves an ordered bounded input stream in a confined role.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

pub(super) fn run() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let manifest: LiveInferenceManifest = read_bulk_header(&mut input)?;
    manifest.validate()?;
    let end = Instant::now() + Duration::from_millis(manifest.budget.wall_time_ms);
    for artifact in [&manifest.model, &manifest.runtime] {
        artifacts::verify_artifact(&artifact.path, &artifact.identity, end)?;
    }
    let mut estimator = estimator(&manifest)?;
    let profile_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest.profile)?)
    );
    serve(
        &mut input,
        &mut output,
        &manifest,
        end,
        Replies::Host(profile_sha256),
        |previous, current| estimator.estimate(previous, current),
        || Ok(()),
    )
}

/// The captured loader already started this role inside the closed namespace.
/// Only the private bootstrap can supply its manifest and original deadline.
#[cfg(target_os = "linux")]
pub(super) fn run_captured() -> Result<()> {
    crate::live_cpu_mounts::clear_inherited_descriptors()?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let witness: crate::live_cpu_mounts::LiveCpuMountWitness = read_message(&mut input)?;
    let proof = crate::live_cpu_mounts::verify_bootstrap(&witness)?;
    serve_captured(&proof, &mut input, &mut output)
}

#[cfg(target_os = "linux")]
fn serve_captured<R: Read, W: Write>(
    proof: &crate::live_cpu_mounts::VerifiedLiveCpuBootstrap<'_>,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let manifest = proof.manifest();
    // Bootstrap verified all mounted image/worker/model bytes before this load.
    anyhow::ensure!(
        Instant::now() < proof.deadline(),
        "captured CPU live initialization expired"
    );
    let binding = manifest.reply_binding()?;
    anyhow::ensure!(
        Instant::now() < proof.deadline(),
        "captured CPU live model-load deadline expired"
    );
    let mut estimator = estimator(&manifest.stream)?;
    // Native initialization may load additional image members. Ready is emitted
    // only after checking the same owned bytes again, within the inherited cap.
    proof.verify_after_execution()?;
    serve(
        input,
        output,
        &manifest.stream,
        proof.deadline(),
        Replies::Captured(&binding),
        |previous, current| estimator.estimate(previous, current),
        || proof.verify_after_execution(),
    )
}

fn estimator(manifest: &LiveInferenceManifest) -> Result<temporal_position::TemporalPosition> {
    let model = SourceArtifact {
        source_version: manifest.identity.source.clone(),
        path: manifest.model.path.clone(),
        identity: manifest.model.identity.clone(),
    };
    temporal_position::TemporalPosition::new(
        &model,
        &manifest.runtime,
        &manifest.profile.preprocessing.input,
        manifest.budget.cpu_threads,
    )
}

enum Replies<'a> {
    Host(String),
    // The production constructor is Linux-only; tests use inert metadata and a
    // pure estimator to exercise framing, never to construct a bootstrap proof.
    #[allow(dead_code)]
    Captured(&'a LiveCpuReplyBinding),
}
impl Replies<'_> {
    fn captured(&self) -> bool {
        matches!(self, Self::Captured(_))
    }
    fn ready(&self, manifest: &LiveInferenceManifest) -> Result<LiveInferenceReply> {
        Ok(match self {
            Self::Host(profile_sha256) => LiveInferenceReply::Ready {
                attempt_id: manifest.attempt_id.clone(),
                identity: manifest.identity.clone(),
                profile_sha256: profile_sha256.clone(),
            },
            Self::Captured(binding) => {
                let reply = LiveInferenceReply::CapturedCpuReady {
                    attempt_id: manifest.attempt_id.clone(),
                    identity: manifest.identity.clone(),
                    profile_sha256: binding.profile_sha256().into(),
                    manifest_sha256: binding.manifest_sha256().into(),
                };
                binding.validate_ready(&reply)?;
                reply
            }
        })
    }
    fn estimated(
        &self,
        manifest: &LiveInferenceManifest,
        request: &LiveInferenceInput,
        position: NormalizedPosition,
        elapsed: u64,
    ) -> Result<LiveInferenceReply> {
        Ok(match self {
            Self::Host(profile_sha256) => LiveInferenceReply::Estimated {
                attempt_id: manifest.attempt_id.clone(),
                identity: manifest.identity.clone(),
                profile_sha256: profile_sha256.clone(),
                input: request.clone(),
                position,
                pair_analysis_elapsed_ns: elapsed,
            },
            Self::Captured(binding) => {
                let reply = LiveInferenceReply::CapturedCpuEstimated {
                    attempt_id: manifest.attempt_id.clone(),
                    identity: manifest.identity.clone(),
                    profile_sha256: binding.profile_sha256().into(),
                    manifest_sha256: binding.manifest_sha256().into(),
                    input: request.clone(),
                    position,
                    pair_analysis_elapsed_ns: elapsed,
                };
                binding.validate_estimated(request, &reply)?;
                reply
            }
        })
    }
}

/// Captured initialization uses a bounded control frame. Subsequent messages
/// retain the existing bounded live headers and fixed-size ordered pixel pairs.
fn next_input<R: Read>(input: &mut R, strict_eof: bool) -> Result<Option<LiveInferenceInput>> {
    if strict_eof {
        let mut first = [0u8; 1];
        match input.read_exact(&mut first) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let mut prefixed = std::io::Cursor::new(first).chain(input);
        return Ok(Some(read_bulk_header(&mut prefixed)?));
    }
    match read_bulk_header(input) {
        Ok(value) => Ok(Some(value)),
        Err(TransportError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn serve<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    manifest: &LiveInferenceManifest,
    end: Instant,
    replies: Replies<'_>,
    mut estimate: impl FnMut(&[u8], &[u8]) -> Result<NormalizedPosition>,
    mut completed: impl FnMut() -> Result<()>,
) -> Result<()> {
    if replies.captured() {
        anyhow::ensure!(
            Instant::now() < end,
            "captured CPU live Ready deadline expired"
        );
    }
    write_bulk_header(output, &replies.ready(manifest)?)?;
    let mut sequence = 0u64;
    let mut last_frame = None;
    loop {
        let Some(request) = next_input(input, replies.captured())? else {
            return completed();
        };
        let result = (|| -> Result<LiveInferenceReply> {
            request.validate(manifest)?;
            if sequence.checked_add(1) != Some(request.sequence)
                || last_frame.is_some_and(|last| request.pair.current_frame <= last)
            {
                bail!("live worker stream identity or order differs");
            }
            if Instant::now() >= end {
                bail!("live worker lifetime exceeded");
            }
            let mut pixels = [0u8; 2 * LIVE_FRAME_BYTES];
            input.read_exact(&mut pixels)?;
            for (bytes, identity) in [
                (&pixels[..LIVE_FRAME_BYTES], &request.previous),
                (&pixels[LIVE_FRAME_BYTES..], &request.current),
            ] {
                if format!("{:x}", Sha256::digest(bytes)) != identity.sha256 {
                    bail!("live worker pixels differ from issued pair");
                }
            }
            let pair = crate::live_frames::PackedRgbPair::new(&pixels)?;
            if replies.captured() {
                anyhow::ensure!(
                    Instant::now() < end,
                    "captured CPU live input deadline expired"
                );
            }
            let started = Instant::now();
            let position = estimate(pair.previous(), pair.current())?;
            if Instant::now() >= end {
                bail!("live worker lifetime exceeded during inference");
            }
            let pair_analysis_elapsed_ns = u64::try_from(started.elapsed().as_nanos())?;
            replies.estimated(manifest, &request, position, pair_analysis_elapsed_ns)
        })();
        match result {
            Ok(reply) => {
                sequence = request.sequence;
                last_frame = Some(request.pair.current_frame);
                write_bulk_header(output, &reply)?;
                output.flush()?;
            }
            Err(error) => {
                write_bulk_header(
                    output,
                    &LiveInferenceReply::Error(ProtocolError::new(
                        ErrorCode::Unavailable,
                        format!("live inference refused: {error}"),
                    )),
                )?;
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn identity(index: u8) -> ArtifactIdentity {
        let sha256 = format!("{index:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: u64::from(index) + 1,
        }
    }
    fn fixture() -> LiveCpuInferenceManifest {
        let request = super::super::tests::cpu_request();
        let WorkerOperation::Generate {
            settings, model, ..
        } = request.operation
        else {
            unreachable!()
        };
        let profile = settings.inference_profile.unwrap();
        let mut libraries = vec![
            CapturedNativeLibrary {
                identity: identity(1),
                names: vec!["ld-linux-x86-64.so.2".into()],
            },
            CapturedNativeLibrary {
                identity: identity(2),
                names: vec!["libc.so.6".into()],
            },
            CapturedNativeLibrary {
                identity: profile.runtime.clone(),
                names: vec!["libonnxruntime.so.1".into()],
            },
        ];
        libraries.sort_by(|left, right| left.identity.sha256.cmp(&right.identity.sha256));
        let image = CpuRuntimeImage {
            schema_version: 1,
            owner_policy: identity(9),
            libraries,
        };
        let identity = LiveIdentity {
            epoch: AttemptId::new("authored-live-epoch").unwrap(),
            project: request.project_id,
            revision: request.base_revision,
            source: request.source.source_version,
        };
        let stream = LiveInferenceManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("authored-live-attempt").unwrap(),
            clock: LivePresentationClock {
                epoch: identity.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            identity,
            model: ArtifactReference {
                path: LIVE_CPU_MODEL_PATH.into(),
                identity: model.unwrap().identity,
            },
            runtime: ArtifactReference {
                path: LIVE_CPU_RUNTIME_PATH.into(),
                identity: profile.runtime.clone(),
            },
            worker: self::identity(7),
            profile,
            budget: request.budget,
        };
        LiveCpuInferenceManifest::new(stream, image).unwrap()
    }
    fn issued(
        manifest: &LiveInferenceManifest,
        sequence: u64,
        pixels: &[u8],
    ) -> LiveInferenceInput {
        let sha256 = format!("{:x}", Sha256::digest(pixels));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: pixels.len() as u64,
        };
        LiveInferenceInput {
            sequence,
            pair: RawFramePair {
                source_epoch: manifest.identity.epoch.clone(),
                previous_frame: FrameId::new(sequence - 1),
                current_frame: FrameId::new(sequence),
                previous_time: SourceTimestamp::new((sequence - 1) as i64, 10).unwrap(),
                current_time: SourceTimestamp::new(sequence as i64, 10).unwrap(),
                presentation_clock: Some(manifest.clock.clone()),
            },
            previous: identity.clone(),
            current: identity,
        }
    }
    fn frame(bytes: &mut Vec<u8>, input: &LiveInferenceInput, pixels: &[u8]) {
        write_bulk_header(bytes, input).unwrap();
        bytes.extend_from_slice(pixels);
        bytes.extend_from_slice(pixels);
    }
    fn replies(bytes: &[u8]) -> Vec<LiveInferenceReply> {
        let mut cursor = std::io::Cursor::new(bytes);
        let mut values = Vec::new();
        while cursor.position() < bytes.len() as u64 {
            values.push(read_bulk_header(&mut cursor).unwrap());
        }
        values
    }

    #[test]
    fn captured_stream_emits_bound_ready_and_ordered_estimates_then_rechecks_completion() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        let pixels = vec![7; LIVE_FRAME_BYTES];
        let mut data = Vec::new();
        let inputs = [
            issued(&manifest.stream, 1, &pixels),
            issued(&manifest.stream, 2, &pixels),
        ];
        for input in &inputs {
            frame(&mut data, input, &pixels);
        }
        let executions = Cell::new(0);
        let completions = Cell::new(0);
        let mut output = Vec::new();
        serve(
            &mut std::io::Cursor::new(data),
            &mut output,
            &manifest.stream,
            Instant::now() + Duration::from_secs(1),
            Replies::Captured(&binding),
            |previous, current| {
                assert_eq!(previous, pixels);
                assert_eq!(current, pixels);
                executions.set(executions.get() + 1);
                Ok(NormalizedPosition::new(0.5)?)
            },
            || {
                completions.set(completions.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(executions.get(), 2);
        assert_eq!(completions.get(), 1);
        let output = replies(&output);
        assert_eq!(output.len(), 3);
        binding.validate_ready(&output[0]).unwrap();
        for (input, reply) in inputs.iter().zip(output.iter().skip(1)) {
            binding.validate_estimated(input, reply).unwrap();
        }
    }

    #[test]
    fn ordinary_stream_retains_original_reply_variants() {
        let manifest = fixture();
        let pixels = vec![1; LIVE_FRAME_BYTES];
        let input = issued(&manifest.stream, 1, &pixels);
        let mut data = Vec::new();
        frame(&mut data, &input, &pixels);
        let mut output = Vec::new();
        serve(
            &mut std::io::Cursor::new(data),
            &mut output,
            &manifest.stream,
            Instant::now() + Duration::from_secs(1),
            Replies::Host("a".repeat(64)),
            |_, _| Ok(NormalizedPosition::new(0.5)?),
            || Ok(()),
        )
        .unwrap();
        let output = replies(&output);
        assert!(matches!(&output[0], LiveInferenceReply::Ready { .. }));
        assert!(
            matches!(&output[1],LiveInferenceReply::Estimated {input: observed,..} if observed==&input)
        );
        assert!(manifest.validate_ready(&output[0]).is_err());
        assert!(manifest.validate_estimated(&input, &output[1]).is_err());
    }

    #[test]
    fn expired_captured_initialization_never_emits_ready_or_runs_an_estimator() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        let mut output = Vec::new();
        assert!(serve(
            &mut std::io::empty(),
            &mut output,
            &manifest.stream,
            Instant::now() - Duration::from_secs(1),
            Replies::Captured(&binding),
            |_, _| panic!("expired execution"),
            || panic!("not completed")
        )
        .is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn captured_truncated_header_is_not_a_verified_clean_end() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        for bytes in [vec![0], vec![0, 0, 0, 8, b'{']] {
            let completed = Cell::new(false);
            let mut output = Vec::new();
            assert!(serve(
                &mut std::io::Cursor::new(bytes),
                &mut output,
                &manifest.stream,
                Instant::now() + Duration::from_secs(1),
                Replies::Captured(&binding),
                |_, _| panic!("truncated header"),
                || {
                    completed.set(true);
                    Ok(())
                }
            )
            .is_err());
            assert!(!completed.get());
            assert_eq!(replies(&output).len(), 1);
        }
    }

    #[test]
    fn malformed_pixels_or_sequence_retire_the_stream_before_native_execution() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        let pixels = vec![2; LIVE_FRAME_BYTES];
        for bad_pixels in [false, true] {
            let mut input = issued(&manifest.stream, 1, &pixels);
            if !bad_pixels {
                input.sequence = 2;
            }
            let mut data = Vec::new();
            frame(&mut data, &input, &pixels);
            if bad_pixels {
                *data.last_mut().unwrap() ^= 1;
            }
            let mut output = Vec::new();
            let completed = Cell::new(false);
            assert!(serve(
                &mut std::io::Cursor::new(data),
                &mut output,
                &manifest.stream,
                Instant::now() + Duration::from_secs(1),
                Replies::Captured(&binding),
                |_, _| panic!("invalid input executed"),
                || {
                    completed.set(true);
                    Ok(())
                }
            )
            .is_err());
            let output = replies(&output);
            assert_eq!(output.len(), 2);
            assert!(matches!(output[1], LiveInferenceReply::Error(_)));
            assert!(!completed.get());
        }
    }

    #[test]
    fn repeated_frame_after_valid_result_never_emits_another_estimate() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        let pixels = vec![4; LIVE_FRAME_BYTES];
        let first = issued(&manifest.stream, 1, &pixels);
        let mut second = first.clone();
        second.sequence = 2;
        let mut data = Vec::new();
        frame(&mut data, &first, &pixels);
        frame(&mut data, &second, &pixels);
        let mut output = Vec::new();
        let count = Cell::new(0);
        assert!(serve(
            &mut std::io::Cursor::new(data),
            &mut output,
            &manifest.stream,
            Instant::now() + Duration::from_secs(1),
            Replies::Captured(&binding),
            |_, _| {
                count.set(count.get() + 1);
                Ok(NormalizedPosition::new(0.5)?)
            },
            || Ok(())
        )
        .is_err());
        assert_eq!(count.get(), 1);
        let output = replies(&output);
        assert_eq!(output.len(), 3);
        binding.validate_estimated(&first, &output[1]).unwrap();
        assert!(matches!(output[2], LiveInferenceReply::Error(_)));
    }

    #[test]
    fn clean_eof_propagates_native_completion_verification_failure() {
        let manifest = fixture();
        let binding = manifest.reply_binding().unwrap();
        let mut output = Vec::new();
        let completed = Cell::new(false);
        let error = serve(
            &mut std::io::empty(),
            &mut output,
            &manifest.stream,
            Instant::now() + Duration::from_secs(1),
            Replies::Captured(&binding),
            |_, _| panic!("no input"),
            || {
                completed.set(true);
                bail!("authored mounted-byte change at completion")
            },
        )
        .unwrap_err();
        assert!(completed.get());
        assert!(error.to_string().contains("mounted-byte change"));
        assert_eq!(replies(&output).len(), 1);
    }
}
