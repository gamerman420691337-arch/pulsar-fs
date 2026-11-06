//! Authored native experiment; does not admit production GPU jobs.
#[cfg(all(target_os = "linux", feature = "cuda"))]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use pulsar_engine::worker::{
        cuda_identity::CudaSdkExpectation,
        cuda_session::{CheckedCudaSession, CudaSessionOptions},
    };
    use std::{collections::BTreeSet, io::Read, path::PathBuf};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3 && args[0] == "cuda",
        "usage: rust-cuda-probe cuda AUTHORED-MODEL PROFILE-PREFIX"
    );
    // Research-only expectation supplied by the namespace harness. Production
    // attempts must carry broker-bound inventory and durable identity records.
    let expected: CudaSdkExpectation =
        serde_json::from_str(&std::env::var("PULSAR_CUDA_SDK_EXPECTATION")?)?;
    let negative_case = std::env::var("PULSAR_RESEARCH_CUDA_CASE").ok();
    if let Some(case) = &negative_case {
        ensure!(
            ["invalid-input", "wrong-run-count", "preinitialized-runtime"].contains(&case.as_str()),
            "invalid research negative case"
        );
    }
    if negative_case.as_deref() == Some("preinitialized-runtime") {
        ensure!(
            ort::init_from("/runtime/libonnxruntime.so.1")
                .map_err(|e| anyhow::anyhow!("research runtime load: {e}"))?
                .commit(),
            "initial research runtime commit failed"
        );
    }
    let mut session = CheckedCudaSession::new(
        &PathBuf::from(&args[1]),
        std::path::Path::new("/runtime/libonnxruntime.so.1"),
        std::path::Path::new("/runtime/libcuda.so.1"),
        CudaSessionOptions {
            device: expected,
            arena_limit_bytes: 64 << 20,
            threads: 2,
            trace_prefix: PathBuf::from(&args[2]),
            maximum_output_values: 1,
        },
    )?;
    ensure!(
        session.input().name() == "frame_pair"
            && session.output().name() == "script_position"
            && session.input().dtype().tensor_shape().map(|s| s.as_ref())
                == Some(&[1, 6, 64, 64][..])
            && session.output().dtype().tensor_shape().map(|s| s.as_ref()) == Some(&[1, 1][..]),
        "authored tensor contract differs"
    );
    if negative_case.as_deref() == Some("invalid-input") {
        let error = session
            .run_f32(ndarray::Array4::zeros((1, 3, 64, 64)))
            .err()
            .ok_or_else(|| anyhow::anyhow!("invalid native input unexpectedly succeeded"))?;
        let reuse = session
            .run_f32(ndarray::Array4::zeros((1, 6, 64, 64)))
            .err()
            .ok_or_else(|| anyhow::anyhow!("failed session was reused"))?;
        ensure!(
            reuse.to_string().contains("session is failed"),
            "unexpected reuse refusal"
        );
        let finish = session
            .finish(1)
            .err()
            .ok_or_else(|| anyhow::anyhow!("failed session produced a receipt"))?;
        ensure!(
            finish.to_string().contains("session failed"),
            "unexpected finish refusal"
        );
        eprintln!("CUDA_POISON_REUSE_AND_FINISH_REFUSED; initial native error: {error}");
        anyhow::bail!("intentional invalid-input research refusal");
    }
    let mut values = Vec::new();
    for level in [0.0f32, 0.25, 0.5, 1.0] {
        let output = session.run_f32(ndarray::Array4::from_elem((1, 6, 64, 64), level))?;
        ensure!(
            output.shape == [1, 1]
                && output.values.len() == 1
                && (output.values[0] - (0.25 + 0.5 * level)).abs() <= 1e-5,
            "authored numerical oracle differs"
        );
        values.push(output.values[0]);
    }
    let receipt = session.finish(if negative_case.as_deref() == Some("wrong-run-count") {
        3
    } else {
        4
    })?;
    eprintln!("PROFILE_PATH={}", receipt.trace_path.display());
    let mut file = std::fs::File::open("/proc/self/maps")?;
    let mut maps = String::new();
    file.by_ref()
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut maps)?;
    ensure!(
        maps.len() <= 2 * 1024 * 1024,
        "native mappings exceed bound"
    );
    let mut paths = BTreeSet::new();
    for line in maps.lines() {
        ensure!(line.len() <= 8192, "native mapping line exceeds bound");
        if let Some(index) = line.find('/') {
            let path = &line[index..];
            if path.contains(".so") {
                paths.insert(path);
            }
        }
    }
    for path in paths {
        eprintln!("LIBRARY_PATH={path}");
    }
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"runs":4,"outputs":values,"rust_session":receipt,"production_integrated":false})
        )?
    );
    Ok(())
}
#[cfg(not(all(target_os = "linux", feature = "cuda")))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("Rust CUDA probe requires Linux and the cuda feature")
}
