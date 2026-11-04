//! Shared Linux worker confinement. Callers own byte snapshots, identity,
//! aggregate admission, supervision and effect authority.
#![cfg(target_os = "linux")]
use crate::{cgroup::AttemptCgroup, EngineConfig};
use pulsar_protocol::ResourceBudget;
use std::{
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
/// The admitted worker budget and its independently selected virtual-address cap.
#[derive(Clone, Copy)]
pub(crate) struct ConfinementLimits<'a> {
    pub budget: &'a ResourceBudget,
    pub address_space_bytes: u64,
}

/// Each entry point supplies exactly one mount/executable policy.
enum LaunchAssets<'a, 'image> {
    Ordinary,
    Assistant {
        worker: (i32, u64),
        runtime: (i32, u64),
        model: (i32, u64),
        vision: Option<((i32, u64), (i32, u64))>,
    },
    Capture(&'a Path),
    Cuda(&'a crate::cuda_mounts::CudaMounts<'image>),
    Cpu(&'a crate::native_mounts::PinnedNativeMounts<'image>),
    LiveCpu {
        image: &'a crate::native_mounts::PinnedNativeMounts<'image>,
        worker: (i32, u64),
        model: (i32, u64),
    },
    #[cfg(feature = "wasm")]
    Wasm {
        executable_fd: i32,
        executable_bytes: u64,
    },
}
pub(crate) fn command(
    config: &EngineConfig,
    budget: &ResourceBudget,
    address_space_bytes: u64,
    group: &AttemptCgroup,
    bindings: &[(PathBuf, PathBuf)],
    output_dir: &Path,
    role: &'static str,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        ConfinementLimits {
            budget,
            address_space_bytes,
        },
        group,
        bindings,
        output_dir,
        role,
        LaunchAssets::Ordinary,
    )
}

pub(crate) fn assistant_command(
    config: &EngineConfig,
    limits: ConfinementLimits<'_>,
    group: &AttemptCgroup,
    worker: (i32, u64),
    runtime: (i32, u64),
    model: (i32, u64),
    vision: Option<((i32, u64), (i32, u64))>,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        limits,
        group,
        &[],
        Path::new("/out"),
        "assistant-worker",
        LaunchAssets::Assistant {
            worker,
            runtime,
            model,
            vision,
        },
    )
}

#[cfg(feature = "wasm")]
pub(crate) fn wasm_command(
    config: &EngineConfig,
    budget: &ResourceBudget,
    address_space_bytes: u64,
    group: &AttemptCgroup,
    executable_fd: i32,
    executable_bytes: u64,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        ConfinementLimits {
            budget,
            address_space_bytes,
        },
        group,
        &[],
        Path::new("/out"),
        "wasm-worker",
        LaunchAssets::Wasm {
            executable_fd,
            executable_bytes,
        },
    )
}

pub(crate) fn cuda_command(
    config: &EngineConfig,
    limits: ConfinementLimits<'_>,
    group: &AttemptCgroup,
    bindings: &[(PathBuf, PathBuf)],
    output_dir: &Path,
    role: &'static str,
    mounts: &crate::cuda_mounts::CudaMounts<'_>,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        limits,
        group,
        bindings,
        output_dir,
        role,
        LaunchAssets::Cuda(mounts),
    )
}

/// A captured CPU image supplies both initial interpreter and descendant loader roots.
pub(crate) fn cpu_image_command(
    config: &EngineConfig,
    limits: ConfinementLimits<'_>,
    group: &AttemptCgroup,
    bindings: &[(PathBuf, PathBuf)],
    output_dir: &Path,
    image: &crate::native_mounts::PinnedNativeMounts<'_>,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        limits,
        group,
        bindings,
        output_dir,
        "closed-cpu-worker",
        LaunchAssets::Cpu(image),
    )
}

/// Captured live inputs have fixed guest coordinates and descriptor-held bytes.
pub(crate) fn live_cpu_image_command(
    config: &EngineConfig,
    limits: ConfinementLimits<'_>,
    group: &AttemptCgroup,
    image: &crate::native_mounts::PinnedNativeMounts<'_>,
    worker: (i32, u64),
    model: (i32, u64),
) -> anyhow::Result<Command> {
    command_inner(
        config,
        limits,
        group,
        &[],
        Path::new("/out"),
        "closed-cpu-live-worker",
        LaunchAssets::LiveCpu {
            image,
            worker,
            model,
        },
    )
}

/// The only writable host mount is a new engine-owned capture staging directory.
pub(crate) fn native_capture_command(
    config: &EngineConfig,
    budget: &ResourceBudget,
    address_space_bytes: u64,
    group: &AttemptCgroup,
    bindings: &[(PathBuf, PathBuf)],
    staging: &Path,
) -> anyhow::Result<Command> {
    command_inner(
        config,
        ConfinementLimits {
            budget,
            address_space_bytes,
        },
        group,
        bindings,
        Path::new("/capture"),
        "native-capture-worker",
        LaunchAssets::Capture(staging),
    )
}
fn command_inner(
    config: &EngineConfig,
    limits: ConfinementLimits<'_>,
    group: &AttemptCgroup,
    bindings: &[(PathBuf, PathBuf)],
    output_dir: &Path,
    role: &'static str,
    assets: LaunchAssets<'_, '_>,
) -> anyhow::Result<Command> {
    let ConfinementLimits {
        budget,
        address_space_bytes,
    } = limits;
    let capture_staging = if let LaunchAssets::Capture(staging) = &assets {
        Some(*staging)
    } else {
        None
    };
    let cuda_mounts = if let LaunchAssets::Cuda(mounts) = &assets {
        Some(*mounts)
    } else {
        None
    };
    let cpu_image = match &assets {
        LaunchAssets::Cpu(image) | LaunchAssets::LiveCpu { image, .. } => Some(*image),
        _ => None,
    };
    let live_cpu_assets = match &assets {
        LaunchAssets::LiveCpu { worker, model, .. } => Some((*worker, *model)),
        _ => None,
    };
    let executable = match &assets {
        LaunchAssets::Assistant { worker, .. } => Some(*worker),
        #[cfg(feature = "wasm")]
        LaunchAssets::Wasm {
            executable_fd,
            executable_bytes,
        } => Some((*executable_fd, *executable_bytes)),
        _ => None,
    };
    let native_assets = match &assets {
        LaunchAssets::Assistant { runtime, model, .. } => Some((*runtime, *model)),
        _ => None,
    };
    let vision_assets = match &assets {
        LaunchAssets::Assistant { vision, .. } => *vision,
        _ => None,
    };
    let procs_fd = group.procs.as_raw_fd();
    let mut command = Command::new("/usr/bin/bwrap");
    command.env_clear();
    command.args([
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
    ]);
    let closed_runtime = cpu_image.is_some()
        || (cuda_mounts.is_some() && !matches!(role, "cuda-mount-check" | "cuda-live-mount-check"));
    append_host_runtime_bindings(&mut command, closed_runtime);
    command.args([
        "--proc", "/proc", "--dev", "/dev", "--size", "16777216", "--tmpfs", "/tmp",
    ]);
    for (host, guest) in bindings {
        command.arg("--ro-bind").arg(host).arg(guest);
    }
    if let Some(mounts) = cuda_mounts {
        mounts.append_module_bindings(&mut command)?;
        if closed_runtime {
            mounts.image.append_closed_runtime_bindings(&mut command)?;
        } else {
            mounts.image.append_bindings(&mut command)?;
        }
        mounts.devices.append_bindings(&mut command)?;
    }
    if let Some(image) = cpu_image {
        image.append_closed_runtime_bindings(&mut command)?;
    }
    if let Some((worker, model)) = live_cpu_assets {
        append_live_cpu_bindings(&mut command, worker, model)?;
    }
    if let Some(staging) = capture_staging {
        command.arg("--bind").arg(staging).arg(output_dir);
    } else {
        command
            .arg("--size")
            .arg(budget.output_bytes.to_string())
            .arg("--tmpfs")
            .arg(output_dir);
    }
    if let Some((runtime, model)) = native_assets {
        for (asset, path, mode) in [
            (runtime, "/assistant/runtime", "0500"),
            (model, "/assistant/model.gguf", "0400"),
        ] {
            command
                .args(["--perms", mode, "--ro-bind-data"])
                .arg(asset.0.to_string())
                .arg(path);
        }
    }
    if let Some((projector, rgb)) = vision_assets {
        for (asset, path) in [
            (projector, "/assistant/mmproj.gguf"),
            (rgb, "/assistant/image.rgb"),
        ] {
            command
                .args(["--perms", "0400", "--ro-bind-data"])
                .arg(asset.0.to_string())
                .arg(path);
        }
    }
    if let Some((fd, _)) = executable {
        // An anonymous sealed file has no pathname for bind-fd's realpath step.
        // Copy its immutable bytes into bwrap's private read-only file mount.
        command
            .args(["--perms", "0500", "--ro-bind-data"])
            .arg(fd.to_string())
            .arg("/pulsar");
    } else if live_cpu_assets.is_none() {
        command
            .arg("--ro-bind")
            .arg(&config.worker_executable)
            .arg("/pulsar");
    }
    command
        .arg("--chdir")
        .arg(output_dir)
        .args([
            "--remount-ro",
            "/proc",
            "--remount-ro",
            "/dev",
            "--remount-ro",
            "/",
        ])
        .args([
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "HOME",
            "/tmp",
            "--setenv",
            "OMP_NUM_THREADS",
            "2",
            "--setenv",
            "OPENBLAS_NUM_THREADS",
            "2",
            "--setenv",
            "PULSAR_WORKER_CONFINED",
            "1",
        ]);
    if let Some(image) = cpu_image {
        if live_cpu_assets.is_some() {
            append_cpu_entry_role(&mut command, image.guest_directory(), CpuEntryRole::Live);
        } else {
            append_cpu_entry(&mut command, image.guest_directory());
        }
    } else {
        command.args(["/pulsar", role]);
    }
    let virtual_memory = address_space_bytes;
    // bwrap's private executable copy precedes worker intake. The real WASM
    // entry point lowers this cap to output_bytes before executing guest code.
    let output = budget
        .output_bytes
        .max(executable.map_or(0, |(_, bytes)| bytes))
        .max(native_assets.map_or(0, |(runtime, model)| runtime.1.max(model.1)))
        .max(vision_assets.map_or(0, |(projector, rgb)| projector.1.max(rgb.1)));
    // RLIMIT_CPU counts all threads in a process. The cgroup bounds concurrent
    // CPU use and the supervisor bounds wall time; permit that declared envelope.
    let cpu_seconds = process_cpu_seconds(budget.wall_time_ms, budget.cpu_threads);
    let file_limit = if cuda_mounts.is_some() || cpu_image.is_some() {
        1024
    } else if capture_staging.is_some() {
        512
    } else {
        128
    };
    unsafe {
        command.pre_exec(move || {
            // Only async-signal-safe syscalls after fork. Writing zero moves
            // this child (and its future descendants), never the parent.
            if libc::write(procs_fd, b"0".as_ptr().cast(), 1) != 1 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in [
                (libc::RLIMIT_AS, virtual_memory),
                (libc::RLIMIT_FSIZE, output),
                (libc::RLIMIT_CPU, cpu_seconds),
                (libc::RLIMIT_NOFILE, file_limit),
            ] {
                let limit = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    Ok(command)
}

fn append_host_runtime_bindings(command: &mut Command, closed_runtime: bool) {
    if !closed_runtime {
        for path in ["/usr", "/lib", "/lib64", "/bin", "/etc/ld.so.cache"] {
            if Path::new(path).exists() {
                command.args(["--ro-bind", path, path]);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CpuEntryRole {
    Finite,
    Live,
}
impl CpuEntryRole {
    fn argument(self) -> &'static str {
        match self {
            Self::Finite => "closed-cpu-worker",
            Self::Live => "closed-cpu-live-worker",
        }
    }
}
fn append_cpu_entry(command: &mut Command, directory: &Path) {
    append_cpu_entry_role(command, directory, CpuEntryRole::Finite);
}
fn append_cpu_entry_role(command: &mut Command, directory: &Path, role: CpuEntryRole) {
    command
        .args(["--setenv", "LD_LIBRARY_PATH"])
        .arg(directory)
        .arg(directory.join("ld-linux-x86-64.so.2"))
        .args(["--inhibit-cache", "--library-path"])
        .arg(directory)
        .args(["/pulsar", role.argument()]);
}
fn append_live_cpu_bindings(
    command: &mut Command,
    worker: (i32, u64),
    model: (i32, u64),
) -> anyhow::Result<()> {
    anyhow::ensure!(
        worker.0 != model.0
            && [worker, model]
                .iter()
                .all(|(fd, bytes)| (3..1024).contains(fd) && *bytes > 0),
        "captured live CPU descriptor binding exceeds admission"
    );
    for (file, guest) in [
        (worker, "/pulsar"),
        (model, pulsar_protocol::LIVE_CPU_MODEL_PATH),
    ] {
        command
            .arg("--ro-bind")
            .arg(format!("/proc/self/fd/{}", file.0))
            .arg(guest);
    }
    unsafe {
        command.pre_exec(move || {
            for fd in [worker.0, model.0] {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(())
}

fn process_cpu_seconds(wall_time_ms: u64, cpu_threads: u16) -> u64 {
    wall_time_ms
        .div_ceil(1000)
        .saturating_mul(u64::from(cpu_threads))
        .saturating_add(2)
}

#[cfg(test)]
mod cpu_limit_tests {
    use super::*;
    #[test]
    fn closed_cpu_entry_starts_captured_loader_and_omits_host_roots() {
        let mut command = Command::new("/usr/bin/bwrap");
        append_host_runtime_bindings(&mut command, true);
        assert_eq!(command.get_args().count(), 0);
        append_cpu_entry(&mut command, Path::new("/captured/libraries"));
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "--setenv",
                "LD_LIBRARY_PATH",
                "/captured/libraries",
                "/captured/libraries/ld-linux-x86-64.so.2",
                "--inhibit-cache",
                "--library-path",
                "/captured/libraries",
                "/pulsar",
                "closed-cpu-worker"
            ]
        );
        let mut legacy = Command::new("/usr/bin/bwrap");
        append_host_runtime_bindings(&mut legacy, false);
        for path in ["/usr", "/lib", "/lib64", "/bin", "/etc/ld.so.cache"] {
            if Path::new(path).exists() {
                let args: Vec<_> = legacy.get_args().map(|arg| arg.to_str().unwrap()).collect();
                assert!(args
                    .windows(3)
                    .any(|window| window == ["--ro-bind", path, path]));
            }
        }
    }
    #[test]
    fn live_cpu_entry_is_explicit_and_has_only_fd_bound_inputs() {
        let mut command = Command::new("/usr/bin/bwrap");
        append_host_runtime_bindings(&mut command, true);
        append_live_cpu_bindings(&mut command, (17, 100), (18, 200)).unwrap();
        append_cpu_entry_role(
            &mut command,
            Path::new("/pulsar-cpu-libraries"),
            CpuEntryRole::Live,
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        assert_eq!(
            &args[..6],
            &[
                "--ro-bind",
                "/proc/self/fd/17",
                "/pulsar",
                "--ro-bind",
                "/proc/self/fd/18",
                "/input/model"
            ]
        );
        assert_eq!(args.last(), Some(&"closed-cpu-live-worker"));
        assert!(!args.contains(&"closed-cpu-worker"));
        for path in ["/usr", "/lib", "/lib64", "/bin", "/etc/ld.so.cache"] {
            assert!(!args
                .windows(3)
                .any(|window| window == ["--ro-bind", path, path]));
        }
    }
    #[test]
    fn live_cpu_bindings_reject_stdio_reuse_duplicate_or_out_of_limit_descriptors() {
        for (worker, model) in [
            ((2, 100), (5, 200)),
            ((5, 100), (5, 200)),
            ((1024, 100), (5, 200)),
            ((5, 0), (6, 200)),
        ] {
            let mut command = Command::new("/usr/bin/bwrap");
            assert!(append_live_cpu_bindings(&mut command, worker, model).is_err());
            assert_eq!(command.get_args().count(), 0);
        }
    }
    #[test]
    fn process_cpu_limit_covers_the_declared_thread_and_wall_envelope() {
        for (wall, threads, expected) in [
            (500, 1, 3),
            (500, 4, 6),
            (120000, 2, 242),
            (45000, 4, 182),
            (120001, 2, 244),
        ] {
            assert_eq!(process_cpu_seconds(wall, threads), expected);
        }
        assert!(process_cpu_seconds(u64::MAX, u16::MAX) > u64::MAX / 1000);
    }
}
