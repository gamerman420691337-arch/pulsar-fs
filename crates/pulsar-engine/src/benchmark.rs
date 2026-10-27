//! Offline qualification harness using the production confined launcher.
//! Not a client command or a substitute for shared-engine authorization.
use crate::{launch, EngineConfig};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct CampaignExecution {
    pub schema_version: u16,
    pub manifest_sha256: String,
    pub manifest: WorkerRequest,
    pub worker: ArtifactIdentity,
    pub declared_profile: Option<AdmittedCampaignProfile>,
    pub elapsed_ns: String,
    pub worker_peak_memory_bytes: Option<u64>,
    pub result: Result<WorkerOutput, ProtocolError>,
    pub release_qualified: bool,
}

/// Caller supplies an admitted, immutable-input manifest and a fresh output
/// directory. This function never opens a project or commits candidate motion.
/// Corpus permissions and independent scoring remain campaign-layer gates.
pub fn execute_campaign_attempt(
    config: &EngineConfig,
    manifest: &WorkerRequest,
    expected_worker: &ArtifactIdentity,
    cancel: &AtomicBool,
) -> Result<CampaignExecution> {
    manifest.validate()?;
    expected_worker.validate()?;
    ensure!(
        expected_worker.byte_len <= 1024 * 1024 * 1024,
        "benchmark worker exceeds 1 GiB bound"
    );
    ensure!(
        manifest.budget.memory_bytes <= config.max_ram_bytes,
        "benchmark exceeds memory admission"
    );
    ensure!(
        manifest.budget.wall_time_ms <= config.worker_timeout.as_millis() as u64,
        "benchmark exceeds configured deadline"
    );
    ensure!(
        manifest.output_dir.canonicalize()? == manifest.output_dir,
        "canonical output directory required"
    );
    ensure!(
        std::fs::read_dir(&manifest.output_dir)?.next().is_none(),
        "benchmark output directory must be empty"
    );

    // Execute copied, checked bytes rather than a mutable caller executable.
    let executable = tempfile::tempdir().context("private benchmark executable staging")?;
    let worker_path = executable.path().join("pulsar-worker");
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let input = options.open(&config.worker_executable)?;
    ensure!(input.metadata()?.is_file(), "worker must be a regular file");
    ensure!(
        input.metadata()?.len() == expected_worker.byte_len,
        "worker length mismatch"
    );
    let mut reader = input.take(expected_worker.byte_len + 1);
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&worker_path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut count = 0u64;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        ensure!(
            count <= expected_worker.byte_len,
            "worker grew during staging"
        );
        output.write_all(&buffer[..n])?;
        digest.update(&buffer[..n]);
    }
    ensure!(
        count == expected_worker.byte_len
            && format!("{:x}", digest.finalize()) == expected_worker.sha256,
        "worker bytes do not match campaign identity"
    );
    output.sync_all()?;
    drop(output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&worker_path, std::fs::Permissions::from_mode(0o500))?;
    }
    let mut execution_config = config.clone();
    execution_config.worker_executable = worker_path;
    let manifest_bytes = serde_json::to_vec(manifest)?;
    let started = Instant::now();
    let (result, worker_peak_memory_bytes) =
        match launch::execute_with_metrics(&execution_config, manifest, cancel) {
            Ok(execution) => (Ok(execution.output), execution.worker_peak_memory_bytes),
            Err(error) => (Err(error), None),
        };
    Ok(CampaignExecution {
        schema_version: 1,
        manifest_sha256: format!("{:x}", Sha256::digest(&manifest_bytes)),
        manifest: manifest.clone(),
        worker: expected_worker.clone(),
        declared_profile: None,
        elapsed_ns: started.elapsed().as_nanos().to_string(),
        worker_peak_memory_bytes,
        result,
        release_qualified: false,
    })
}

#[derive(Debug, Serialize)]
pub struct AdmittedCampaignProfile {
    pub sha256: String,
    pub profile: DeclaredInferenceProfile,
}

/// Binds declarations to the CPU adapter implemented by this source revision.
/// A caller-supplied worker digest is not proof that those bytes implement it.
pub fn admit_generation_profile(
    profile: &DeclaredInferenceProfile,
    manifest: &WorkerRequest,
) -> Result<AdmittedCampaignProfile> {
    let WorkerOperation::Generate { model: Some(_), .. } = &manifest.operation else {
        anyhow::bail!("inference profile requires model-backed video generation");
    };
    crate::inference_profiles::admit_profile(profile, manifest)?;
    let resolved = crate::inference_profiles::resolve_profile(manifest)?
        .context("campaign manifest resolves no model profile")?;
    ensure!(
        serde_json::to_value(profile)? == serde_json::to_value(&resolved)?,
        "campaign declaration differs from the profile resolved by its worker manifest"
    );
    Ok(AdmittedCampaignProfile {
        sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(profile)?)),
        profile: profile.clone(),
    })
}

/// Append-only receipt publication; existing campaign evidence is never replaced.
pub fn write_execution_receipt(path: &std::path::Path, receipt: &CampaignExecution) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file: File = options.open(path)?;
    serde_json::to_writer(&mut file, receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// Independent monotone-run segmentation of checked neutral motion.
/// No reference annotations enter this function. Missing edges are never bridged.
pub fn motion_candidate(
    bytes: &[u8],
    expected_sha256: &str,
    subject: &str,
    target: &str,
    window: TimeRange,
) -> Result<serde_json::Value> {
    motion_candidate_for_window(
        bytes,
        expected_sha256,
        subject,
        target,
        window,
        EvaluationWindowPolicy::Strict,
    )
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationWindowPolicy {
    #[default]
    Strict,
    /// Explicitly evaluate recorded actions inside the declared window. No
    /// interpolation, retiming or fabricated boundary anchor is performed.
    RecordedActions,
}
pub fn motion_candidate_for_window(
    bytes: &[u8],
    expected_sha256: &str,
    subject: &str,
    target: &str,
    window: TimeRange,
    policy: EvaluationWindowPolicy,
) -> Result<serde_json::Value> {
    ensure!(
        bytes.len() <= MAX_MOTION_BYTES as usize,
        "motion exceeds campaign bound"
    );
    validate_sha256(expected_sha256)?;
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == expected_sha256,
        "motion digest mismatch"
    );
    ensure!(
        !subject.is_empty() && subject.len() <= 256 && !target.is_empty() && target.len() <= 256,
        "bounded subject and target required"
    );
    ensure!(window.start().as_nanos() >= 0, "negative evaluation window");
    let program: MotionProgram = serde_json::from_slice(bytes)?;
    ensure!(
        program
            .tracks()
            .iter()
            .map(|t| t.actions().len())
            .sum::<usize>()
            <= MAX_MOTION_ACTIONS as usize,
        "too many campaign actions"
    );
    let mut strokes = Vec::new();
    let mut skipped_gap_edges = 0usize;
    let mut excluded_actions = 0usize;
    for track in program.tracks() {
        let mut points: Vec<_> = track
            .actions()
            .iter()
            .map(|a| {
                (
                    a.time().as_nanos(),
                    (a.position().value() * 1_000_000.0).round() as i64,
                )
            })
            .collect();
        let inside =
            |p: &(i64, i64)| p.0 >= window.start().as_nanos() && p.0 <= window.end().as_nanos();
        if policy == EvaluationWindowPolicy::Strict {
            ensure!(
                points.iter().all(inside),
                "motion outside evaluation window; no implicit clipping"
            );
        } else {
            let count = points.len();
            points.retain(inside);
            excluded_actions += count - points.len();
        }
        let mut run: Option<(usize, usize, i64)> = None;
        let flush = |run: &mut Option<(usize, usize, i64)>,
                     strokes: &mut Vec<serde_json::Value>|
         -> Result<()> {
            if let Some((a, b, _)) = run.take() {
                ensure!(strokes.len() < 5000, "too many candidate strokes");
                strokes.push(serde_json::json!({"id":format!("stroke-{}",strokes.len()),
                    "target_id":target,"axis":track.axis().as_str(),
                    "start_ns":points[a].0.to_string(),"end_ns":points[b].0.to_string(),
                    "start_position_ppm":points[a].1,"end_position_ppm":points[b].1}));
            }
            Ok(())
        };
        let mut gap_index = 0usize;
        for i in 1..points.len() {
            while gap_index < track.gaps().len()
                && track.gaps()[gap_index].end().as_nanos() <= points[i - 1].0
            {
                gap_index += 1;
            }
            let crosses_gap = gap_index < track.gaps().len()
                && track.gaps()[gap_index].start().as_nanos() < points[i].0;
            let direction = (points[i].1 - points[i - 1].1).signum();
            if crosses_gap || direction == 0 {
                flush(&mut run, &mut strokes)?;
                if crosses_gap {
                    skipped_gap_edges += 1;
                }
            } else if let Some((_, last, prior)) = &mut run {
                if *prior == direction {
                    *last = i;
                } else {
                    flush(&mut run, &mut strokes)?;
                    run = Some((i - 1, i, direction));
                }
            } else {
                run = Some((i - 1, i, direction));
            }
        }
        flush(&mut run, &mut strokes)?;
    }
    Ok(serde_json::json!({
        "candidate":{"schema_version":1,"subject_id":subject,
            "window":{"start_ns":window.start().as_nanos().to_string(),"end_ns":window.end().as_nanos().to_string()},
            "strokes":strokes,"flags":[]},
        "receipt":{"schema_version":1,"adapter":"neutral-monotone-plateau-v1",
            "raw_sha256":expected_sha256,"raw_byte_len":bytes.len(),"skipped_gap_edges":skipped_gap_edges,
            "window_policy":policy,"excluded_actions":excluded_actions,"boundary_interpolation":false,
            "quantization":{"time_unit_ns":1,"position_unit_ppm":1,"position_rounding":"nearest"},
            "review_evidence":"not-imported-by-motion-adapter","release_qualified":false}
    }))
}

/// Project native warnings onto one evaluated axis without inventing labels.
pub fn benchmark_review_flags(
    review: &[ReviewFlag],
    axis: Axis,
    window: TimeRange,
) -> Result<serde_json::Value> {
    benchmark_review_flags_for_window(review, axis, window, EvaluationWindowPolicy::Strict)
}
pub fn benchmark_review_flags_for_window(
    review: &[ReviewFlag],
    axis: Axis,
    window: TimeRange,
    policy: EvaluationWindowPolicy,
) -> Result<serde_json::Value> {
    validate_reviews(review)?;
    let mut flags = Vec::new();
    for (index, flag) in review.iter().enumerate() {
        if flag.axis.is_some_and(|scope| scope != axis) {
            continue;
        }
        let (start, end) = if policy == EvaluationWindowPolicy::Strict {
            ensure!(
                flag.range.start() >= window.start() && flag.range.end() <= window.end(),
                "review interval outside evaluation window; no implicit clipping"
            );
            (flag.range.start(), flag.range.end())
        } else {
            (
                flag.range.start().max(window.start()),
                flag.range.end().min(window.end()),
            )
        };
        if start >= end {
            continue;
        }
        flags.push(serde_json::json!({"id":format!("worker-review-{index}"),
            "start_ns":start.as_nanos().to_string(),
            "end_ns":end.as_nanos().to_string(),
            "reasons":[flag.reason.as_str()]}));
    }
    Ok(serde_json::Value::Array(flags))
}
