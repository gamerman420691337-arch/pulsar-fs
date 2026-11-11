//! Offline diagnostic of the production CPU temporal adapter at zero offset.
use anyhow::{ensure, Context, Result};
use pulsar_engine::{artifacts::digest_file, worker::temporal_position::TemporalPosition};
use pulsar_protocol::{
    ArtifactId, ArtifactIdentity, ArtifactReference, DetectorDecoder, ModelInputContract,
    SourceArtifact, SourceVersionId, TensorLayout,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const FRAME_BYTES: usize = 64 * 64 * 3;
const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    origin: String,
    fps: u32,
    frames: usize,
    playback_offset_ms: i32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    case: String,
    clip: String,
    clip_sha256: String,
    reference: String,
    reference_sha256: String,
}

#[derive(Deserialize)]
struct Reference {
    actions: Vec<Action>,
}

#[derive(Deserialize)]
struct Action {
    at: u32,
    pos: u8,
}

fn identity(path: &Path, limit: u64) -> Result<ArtifactIdentity> {
    let (sha256, byte_len) = digest_file(path, limit)?;
    Ok(ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone())?,
        sha256,
        byte_len,
    })
}

fn case_file(root: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    ensure!(
        path.file_name().and_then(|value| value.to_str()) == Some(name),
        "holdout manifest must use local filenames"
    );
    Ok(root.join(path))
}

fn evaluate(
    estimator: &mut TemporalPosition,
    root: &Path,
    manifest: &Manifest,
    case: &Case,
) -> Result<Value> {
    let clip = case_file(root, &case.clip)?;
    let reference = case_file(root, &case.reference)?;
    ensure!(
        identity(&clip, MAX_ASSET_BYTES)?.sha256 == case.clip_sha256
            && identity(&reference, 64 * 1024)?.sha256 == case.reference_sha256,
        "holdout source or reference differs from manifest"
    );
    let reference: Reference = serde_json::from_slice(&fs::read(reference)?)?;
    ensure!(
        reference.actions.len() == manifest.frames,
        "reference frame count differs"
    );
    ensure!(
        reference.actions.iter().enumerate().all(|(index, action)| {
            let at = (index as u64 * 1000 + u64::from(manifest.fps) / 2) / u64::from(manifest.fps);
            u64::from(action.at) == at && action.pos <= 100
        }),
        "reference does not match the zero-offset source frame grid"
    );
    let decoded = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(clip)
        .args([
            "-map",
            "0:v:0",
            "-vf",
            "scale=64:64:flags=neighbor",
            "-fps_mode",
            "passthrough",
            "-frames:v",
        ])
        .arg((manifest.frames + 1).to_string())
        .args([
            "-threads", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1",
        ])
        .stdin(Stdio::null())
        .output()
        .context("decode generated holdout")?;
    ensure!(decoded.status.success(), "holdout decoder failed");
    ensure!(
        decoded.stdout.len() == manifest.frames * FRAME_BYTES,
        "decoded frame count differs"
    );
    let frames: Vec<_> = decoded.stdout.chunks_exact(FRAME_BYTES).collect();
    let estimates: Vec<f64> = frames
        .windows(2)
        .map(|pair| {
            estimator
                .estimate(pair[0], pair[1])
                .map(|value| 100.0 * value.value())
        })
        .collect::<Result<_>>()?;
    let labels: Vec<f64> = reference
        .actions
        .iter()
        .skip(1)
        .map(|action| f64::from(action.pos))
        .collect();
    let count = estimates.len() as f64;
    let mae = estimates
        .iter()
        .zip(&labels)
        .map(|(a, b)| (a - b).abs())
        .sum::<f64>()
        / count;
    let mean = |values: &[f64]| values.iter().sum::<f64>() / count;
    let (x_mean, y_mean) = (mean(&estimates), mean(&labels));
    let variance = |values: &[f64], center| {
        values
            .iter()
            .map(|value| (value - center).powi(2))
            .sum::<f64>()
    };
    let scale = (variance(&estimates, x_mean) * variance(&labels, y_mean)).sqrt();
    let correlation = (scale > 0.0).then(|| {
        estimates
            .iter()
            .zip(&labels)
            .map(|(x, y)| (x - x_mean) * (y - y_mean))
            .sum::<f64>()
            / scale
    });
    Ok(json!({
        "case": case.case,
        "clip_sha256": case.clip_sha256,
        "reference_sha256": case.reference_sha256,
        "scored_frames": estimates.len(),
        "mean_absolute_error": mae,
        "pearson": correlation
    }))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(
        args.len() == 3,
        "usage: benchmark-outline-holdout MODEL ORT_RUNTIME HOLDOUT_DIR"
    );
    let root = args[2].canonicalize()?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(root.join("manifest.json"))?)?;
    ensure!(
        manifest.origin == "independently generated geometry"
            && manifest.playback_offset_ms == 0
            && (1..=60).contains(&manifest.fps)
            && (2..=1000).contains(&manifest.frames)
            && (1..=8).contains(&manifest.cases.len()),
        "unsupported generated holdout manifest"
    );
    let model_path = args[0].canonicalize()?;
    let runtime_path = args[1].canonicalize()?;
    let model = identity(&model_path, MAX_ASSET_BYTES)?;
    let runtime = identity(&runtime_path, MAX_ASSET_BYTES)?;
    let mut estimator = TemporalPosition::new(
        &SourceArtifact {
            source_version: SourceVersionId::new(model.sha256.clone())?,
            path: model_path,
            identity: model.clone(),
        },
        &ArtifactReference {
            identity: runtime.clone(),
            path: runtime_path,
        },
        &ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        },
        2,
    )?;
    let cases: Vec<_> = manifest
        .cases
        .iter()
        .map(|case| evaluate(&mut estimator, &root, &manifest, case))
        .collect::<Result<_>>()?;
    estimator.finish()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope": "synthetic direct CPU temporal adapter",
            "model_sha256": model.sha256,
            "runtime_sha256": runtime.sha256,
            "playback_offset_ms": 0,
            "first_frame_unscored": true,
            "cases": cases
        }))?
    );
    Ok(())
}
