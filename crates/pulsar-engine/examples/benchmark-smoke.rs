//! Authored pixels through the production process boundary, never real-media qualification.
use anyhow::{ensure, Result};
use pulsar_engine::{benchmark, EngineConfig};
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, sync::atomic::AtomicBool};

fn pin(path: &Path) -> Result<ArtifactReference> {
    let path = path.canonicalize()?;
    let mut file = std::fs::File::open(&path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut count = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        ensure!(
            count <= 1024 * 1024 * 1024,
            "smoke dependency exceeds 1 GiB"
        );
        hash.update(&buffer[..n]);
    }
    let sha256 = format!("{:x}", hash.finalize());
    Ok(ArtifactReference {
        path,
        identity: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len: count,
        },
    })
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "usage: benchmark-smoke PULSAR-WORKER NEW-EVIDENCE-DIRECTORY"
    );
    let worker = pin(Path::new(&args[0]))?;
    let root = Path::new(&args[1]);
    std::fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    let root = root.canonicalize()?;
    let mut pixels = b"YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n".to_vec();
    for frame in 0..8usize {
        pixels.extend_from_slice(b"FRAME\n");
        for y in 0..64usize {
            for x in 0..64usize {
                let sy = (y + 64 - frame) % 64;
                pixels.push(((x * 31 + sy * 17 + (x / 7) * (sy / 5) * 13) % 256) as u8);
            }
        }
    }
    let source_path = root.join("authored-moving.y4m");
    std::fs::write(&source_path, pixels)?;
    let source = pin(&source_path)?;
    let output_dir = root.join("output");
    std::fs::create_dir(&output_dir)?;
    let manifest = WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new(format!("benchmark-smoke-{}", std::process::id()))?,
        job_id: JobId::new("benchmark-smoke-job")?,
        project_id: ProjectId::new("benchmark-smoke-project")?,
        base_revision: RevisionId::new(0),
        source: SourceArtifact {
            source_version: SourceVersionId::new("authored-v1")?,
            path: source.path,
            identity: source.identity,
        },
        output_dir,
        operation: WorkerOperation::Generate {
            preset: "fast".into(),
            model: None,
            settings: (GenerationSettings {
                inference_profile: None,
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
        },
        budget: ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            wall_time_ms: 30_000,
            cpu_threads: 1,
        },
        dependencies: Vec::new(),
        tools: WorkerTools {
            ffmpeg: pin(Path::new("/usr/bin/ffmpeg"))?,
            ffprobe: pin(Path::new("/usr/bin/ffprobe"))?,
            onnx_runtime: None,
        },
    };
    let config = EngineConfig::new(root.join("unused-state"), worker.path);
    let receipt = benchmark::execute_campaign_attempt(
        &config,
        &manifest,
        &worker.identity,
        &AtomicBool::new(false),
    )?;
    let receipt_path = root.join("execution.json");
    benchmark::write_execution_receipt(&receipt_path, &receipt)?;
    println!("{}", receipt_path.display());
    match receipt.result {
        Ok(WorkerOutput::Generated { .. }) => Ok(()),
        other => anyhow::bail!("production generation did not succeed: {other:?}"),
    }
}
