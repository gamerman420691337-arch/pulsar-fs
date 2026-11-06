//! Shared data-only worker role dispatch for both process composition binaries.
use anyhow::{bail, Result};
use std::ffi::OsString;

/// Unknown roles are never interpreted as client or engine commands.
pub fn run_worker_role(args: &[OsString]) -> Result<bool> {
    let Some(role) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };
    if !is_worker_role(role) {
        return Ok(false);
    }
    dispatch(args)?;
    Ok(true)
}
fn is_worker_role(role: &str) -> bool {
    matches!(
        role,
        "assistant-worker"
            | "wasm-worker"
            | "live-worker"
            | "capture-worker"
            | "native-capture-worker"
            | "cuda-memory-observer"
            | "cuda-memory-observer-exec"
            | "cuda-worker-exec"
            | "cuda-worker"
            | "cuda-live-worker"
            | "cuda-live-worker-exec"
            | "cuda-live-memory-observer"
            | "cuda-live-memory-observer-exec"
            | "cuda-live-mount-check"
            | "cuda-mount-check"
            | "closed-cpu-worker"
            | "closed-cpu-live-worker"
            | "worker"
            | "progress-worker"
            | "__pulsar-worker"
    )
}
fn dispatch(args: &[OsString]) -> Result<()> {
    match args.first().and_then(|arg| arg.to_str()) {
        Some("closed-cpu-live-worker") => {
            if args.len() != 1 {
                bail!("closed CPU live worker accepts only one bounded private mount witness");
            }
            #[cfg(target_os = "linux")]
            return crate::worker::run_closed_cpu_live_worker();
            #[cfg(not(target_os = "linux"))]
            bail!("closed CPU live worker requires Linux confinement");
        }
        Some("closed-cpu-worker") => {
            if args.len() != 1 {
                bail!("closed CPU worker accepts only one bounded private mount witness");
            }
            #[cfg(target_os = "linux")]
            return crate::worker::run_closed_cpu_worker();
            #[cfg(not(target_os = "linux"))]
            bail!("closed CPU worker requires Linux confinement");
        }
        Some("progress-worker") => {
            if args.len() != 1 {
                bail!("progress worker accepts one bounded private generation envelope");
            }
            crate::worker::run_progress_worker()
        }
        Some("assistant-worker") => {
            if args.len() != 1 {
                bail!("assistant worker accepts only a bounded data-only request");
            }
            #[cfg(target_os = "linux")]
            return crate::assistant_worker::run_worker();
            #[cfg(not(target_os = "linux"))]
            bail!("assistant worker requires Linux confinement");
        }
        Some("wasm-worker") => {
            if args.len() != 1 {
                bail!("WASM worker reads one bounded data-only request from standard input");
            }
            #[cfg(feature = "wasm")]
            return crate::wasm_worker::run_worker();
            #[cfg(not(feature = "wasm"))]
            bail!("WASM worker support was not compiled into this build");
        }
        Some("live-worker") => {
            if args.len() != 1 {
                bail!("live worker role accepts only bounded engine stream input");
            }
            crate::worker::run_live_worker()
        }
        Some("capture-worker") => {
            if args.len() != 1 {
                bail!("capture worker role accepts only bounded engine stream input");
            }
            crate::worker::run_capture_worker()
        }
        Some("native-capture-worker") => {
            if args.len() != 1 {
                bail!("native capture worker accepts only a bounded engine request");
            }
            #[cfg(target_os = "linux")]
            return crate::native_capture::run_worker();
            #[cfg(not(target_os = "linux"))]
            bail!("native dependency capture requires Linux confinement");
        }
        Some("cuda-memory-observer") | Some("cuda-memory-observer-exec") => {
            if args.len() != 1 {
                bail!("CUDA memory observation accepts only its private bootstrap");
            }
            #[cfg(target_os = "linux")]
            return if args[0] == "cuda-memory-observer" {
                crate::cuda_memory::run_worker()
            } else {
                crate::cuda_memory::run_worker_exec()
            };
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA memory observation requires Linux confinement");
        }
        Some("cuda-worker-exec") => {
            if args.len() != 1 {
                bail!("CUDA execution accepts only its private sealed relay");
            }
            #[cfg(target_os = "linux")]
            return crate::worker::run_cuda_worker_exec();
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA worker requires Linux confinement");
        }
        Some("cuda-worker") => {
            if args.len() != 1 {
                bail!("CUDA worker accepts only a bounded parent bootstrap");
            }
            #[cfg(target_os = "linux")]
            return crate::worker::run_cuda_worker();
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA worker requires Linux confinement");
        }
        Some("cuda-live-worker") | Some("cuda-live-worker-exec") => {
            if args.len() != 1 {
                bail!("CUDA live inference accepts only its private bootstrap");
            }
            #[cfg(target_os = "linux")]
            return if args[0] == "cuda-live-worker" {
                crate::worker::run_live_cuda_worker()
            } else {
                crate::worker::run_live_cuda_worker_exec()
            };
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA live inference requires Linux confinement");
        }
        Some("cuda-live-memory-observer") | Some("cuda-live-memory-observer-exec") => {
            if args.len() != 1 {
                bail!("CUDA live memory observation accepts only its private bootstrap");
            }
            #[cfg(target_os = "linux")]
            return if args[0] == "cuda-live-memory-observer" {
                crate::cuda_memory::run_live_worker()
            } else {
                crate::cuda_memory::run_live_worker_exec()
            };
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA live memory observation requires Linux confinement");
        }
        Some("cuda-live-mount-check") => {
            if args.len() != 1 {
                bail!("CUDA live mount checker accepts only a bounded parent witness");
            }
            #[cfg(target_os = "linux")]
            return crate::cuda_mounts::run_live_checker();
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA live mount inspection requires Linux confinement");
        }
        Some("cuda-mount-check") => {
            if args.len() != 1 {
                bail!("CUDA mount checker accepts only a bounded parent request");
            }
            #[cfg(target_os = "linux")]
            return crate::cuda_mounts::run_checker();
            #[cfg(not(target_os = "linux"))]
            bail!("CUDA mount inspection requires Linux confinement");
        }
        Some("worker") | Some("__pulsar-worker") => {
            if args.len() != 1 {
                bail!("worker role reads one bounded engine manifest from standard input");
            }
            crate::worker::run_worker()
        }
        _ => bail!("unknown worker role"),
    }
}

/// Resolve the owner-specified image or the installed companion, without PATH search.
/// Existence is checked when the engine opens; help/version remain state-free.
pub fn configured_executable() -> Result<std::path::PathBuf> {
    let current = std::env::current_exe()?;
    select_executable(&current, std::env::var_os("PULSAR_WORKER_EXECUTABLE"))
}
fn select_executable(
    current: &std::path::Path,
    override_path: Option<OsString>,
) -> Result<std::path::PathBuf> {
    if let Some(path) = override_path {
        let path = std::path::PathBuf::from(path);
        anyhow::ensure!(
            path.is_absolute(),
            "PULSAR_WORKER_EXECUTABLE must be an absolute path"
        );
        return Ok(path);
    }
    Ok(current
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Pulsar executable has no parent directory"))?
        .join(if cfg!(windows) {
            "pulsar-worker.exe"
        } else {
            "pulsar-worker"
        }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_cpu_live_role_refuses_arguments_and_an_unimplemented_second_stage() {
        assert!(is_worker_role("closed-cpu-live-worker"));
        assert!(!is_worker_role("closed-cpu-live-worker-exec"));
        assert!(dispatch(&["closed-cpu-live-worker".into(), "--manifest".into()]).is_err());
        assert!(dispatch(&["closed-cpu-live-worker".into(), "worker".into()]).is_err());
    }

    #[test]
    fn closed_cpu_role_is_data_only_and_rejects_arguments_before_reading_input() {
        assert!(is_worker_role("closed-cpu-worker"));
        assert!(!is_worker_role("closed-cpu-worker-exec"));
        assert!(dispatch(&["closed-cpu-worker".into(), "--command".into()]).is_err());
        assert!(dispatch(&["closed-cpu-worker".into(), "worker".into()]).is_err());
    }

    #[test]
    fn selection_does_not_reuse_gui_or_search_path() {
        let current = std::env::current_exe().unwrap();
        let selected = select_executable(&current, None).unwrap();
        assert_eq!(selected.parent(), current.parent());
        assert_ne!(selected, current);
        assert!(select_executable(&current, Some("relative-worker".into())).is_err());
        assert!(select_executable(&current, Some(OsString::new())).is_err());
        let explicit = current.parent().unwrap().join("owner-worker");
        assert_eq!(
            select_executable(&current, Some(explicit.clone().into_os_string())).unwrap(),
            explicit
        );
    }
}
