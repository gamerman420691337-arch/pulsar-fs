//! Verified confined loader transition; not owner or scheduling authority.
#![cfg(target_os = "linux")]
use crate::cuda_mounts::CudaMountWitness;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::{fd::FromRawFd, unix::process::CommandExt},
    process::Command,
    time::{Duration, Instant},
};
const SEALS: i32 = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
/// This supported CUDA runtime needs globally visible CuDNN symbols. Load only
/// its declared captured alias, after namespace verification, and retain it
/// until the native session and evidence emission have completed.
#[cfg(feature = "cuda")]
pub(crate) fn load_cudnn(
    bootstrap: &crate::cuda_mounts::VerifiedCudaBootstrap<'_>,
) -> Result<libloading::os::unix::Library> {
    let execution = bootstrap
        .request()
        .cuda_execution
        .as_ref()
        .context("CUDA declaration missing")?;
    ensure!(
        execution
            .libraries
            .iter()
            .any(|library| library.names.iter().any(|name| name == "libcudnn.so.9")),
        "supported CUDA bootstrap requires declared libcudnn.so.9"
    );
    ensure!(
        Instant::now() < bootstrap.deadline(),
        "CUDA bootstrap deadline expired"
    );
    unsafe {
        libloading::os::unix::Library::open(
            Some(execution.library_directory.join("libcudnn.so.9")),
            libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_GLOBAL,
        )
    }
    .context("load verified captured CuDNN into global symbol scope")
}
#[cfg(feature = "cuda")]
pub(crate) fn load_live_cudnn(
    bootstrap: &crate::cuda_mounts::VerifiedLiveCudaBootstrap<'_>,
) -> Result<libloading::os::unix::Library> {
    let execution = &bootstrap.manifest().execution;
    ensure!(
        execution
            .libraries
            .iter()
            .any(|library| library.names.iter().any(|name| name == "libcudnn.so.9")),
        "supported live CUDA bootstrap requires declared libcudnn.so.9"
    );
    ensure!(
        Instant::now() < bootstrap.deadline(),
        "CUDA live loader deadline expired"
    );
    let library = unsafe {
        libloading::os::unix::Library::open(
            Some(execution.library_directory.join("libcudnn.so.9")),
            libloading::os::unix::RTLD_NOW | libloading::os::unix::RTLD_GLOBAL,
        )
    }
    .context("load verified captured live CuDNN")?;
    ensure!(
        Instant::now() < bootstrap.deadline(),
        "CUDA live loader exceeded deadline"
    );
    Ok(library)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Relay {
    pub role: BootstrapRole,
    pub witness: CudaMountWitness,
    pub deadline_ns: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BootstrapRole {
    Inference,
    MemoryObservation,
}
impl BootstrapRole {
    fn executable_role(self) -> &'static str {
        match self {
            Self::Inference => "cuda-worker-exec",
            Self::MemoryObservation => "cuda-memory-observer-exec",
        }
    }
}
pub(crate) fn now_ns() -> Result<u64> {
    let mut value: libc::timespec = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } == 0
            && value.tv_sec >= 0
            && (0..1_000_000_000).contains(&value.tv_nsec),
        "CUDA bootstrap monotonic clock unavailable"
    );
    (value.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(value.tv_nsec as u64))
        .context("CUDA bootstrap clock overflow")
}
pub(crate) fn deadline_ns(budget_ms: u64) -> Result<u64> {
    ensure!(
        (1..=86_400_000).contains(&budget_ms),
        "CUDA bootstrap deadline must be at most one day"
    );
    now_ns()?
        .checked_add(
            budget_ms
                .checked_mul(1_000_000)
                .context("CUDA bootstrap budget overflow")?,
        )
        .context("CUDA bootstrap deadline overflow")
}
pub(crate) fn deadline_instant(end: u64, budget_ms: u64) -> Result<Instant> {
    let before = Instant::now();
    let remaining = end
        .checked_sub(now_ns()?)
        .context("CUDA bootstrap deadline expired")?;
    ensure!(
        remaining > 0
            && remaining
                <= budget_ms
                    .checked_mul(1_000_000)
                    .context("CUDA bootstrap budget overflow")?,
        "CUDA relay deadline exceeds request"
    );
    before
        .checked_add(Duration::from_nanos(remaining))
        .context("CUDA bootstrap deadline overflow")
}
/// Relay an existing finite-execution cap. The sampling order can shorten its
/// remaining lifetime, but never begins another execution budget.
pub(crate) fn deadline_ns_in_execution(end: Instant, budget_ms: u64) -> Result<u64> {
    ensure!(
        (1..=86_400_000).contains(&budget_ms),
        "CUDA execution deadline must be at most one day"
    );
    let before = now_ns()?;
    let remaining = end
        .checked_duration_since(Instant::now())
        .context("CUDA execution deadline expired")?;
    ensure!(
        !remaining.is_zero() && remaining <= Duration::from_millis(budget_ms),
        "CUDA execution deadline exceeds request budget"
    );
    before
        .checked_add(
            u64::try_from(remaining.as_nanos()).context("CUDA execution deadline overflow")?,
        )
        .context("CUDA execution relay deadline overflow")
}

pub(crate) fn execution_deadline_instant(end: u64, budget_ms: u64) -> Result<Instant> {
    ensure!(
        (1..=86_400_000).contains(&budget_ms),
        "CUDA execution deadline must be at most one day"
    );
    deadline_instant(end, budget_ms)
}

fn relay_deadline_instant(end: u64, budget_ms: u64, inherited: Option<u64>) -> Result<Instant> {
    if let Some(inherited) = inherited {
        ensure!(
            end <= inherited,
            "CUDA sealed relay renewed the inherited execution deadline"
        );
    }
    execution_deadline_instant(end, budget_ms)
}

/// Sample the relay clock before measuring remaining local time, so conversion
/// can shorten the lease by sampling latency but cannot renew it.
pub(crate) fn deadline_ns_in_lease(end: Instant, budget_ms: u64) -> Result<u64> {
    ensure!(
        (1..=30_000).contains(&budget_ms),
        "CUDA live lease budget differs"
    );
    let before = now_ns()?;
    let remaining = end
        .checked_duration_since(Instant::now())
        .context("CUDA live lease expired")?;
    ensure!(
        !remaining.is_zero() && remaining <= Duration::from_millis(budget_ms),
        "CUDA live lease exceeds budget"
    );
    before
        .checked_add(u64::try_from(remaining.as_nanos()).context("CUDA live lease overflow")?)
        .context("CUDA live relay deadline overflow")
}
/// Initial trusted executable verifies bytes before selecting the captured loader.
#[cfg(feature = "cuda")]
pub(crate) fn reexec(witness: CudaMountWitness) -> Result<()> {
    reexec_as(witness, BootstrapRole::Inference)
}
pub(crate) fn reexec_as(witness: CudaMountWitness, role: BootstrapRole) -> Result<()> {
    let end = match witness.execution_deadline_ns {
        Some(end) => end,
        None => deadline_ns(witness.request.budget.wall_time_ms)?,
    };
    let deadline = relay_deadline_instant(
        end,
        witness.request.budget.wall_time_ms,
        witness.execution_deadline_ns,
    )?;
    match role {
        BootstrapRole::Inference => {
            crate::cuda_mounts::verify_worker_bootstrap_at(&witness, deadline)?;
        }
        BootstrapRole::MemoryObservation => {
            crate::cuda_mounts::verify_memory_bootstrap_at(&witness, deadline)?;
        }
    }
    let directory = witness
        .request
        .cuda_execution
        .as_ref()
        .context("CUDA declaration missing")?
        .library_directory
        .clone();
    let relay = Relay {
        role,
        witness,
        deadline_ns: end,
    };
    transition(&directory, role.executable_role(), &relay)
}
fn transition<T: Serialize>(directory: &std::path::Path, role: &str, relay: &T) -> Result<()> {
    let bytes = serde_json::to_vec(relay)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "CUDA relay exceeds control bound"
    );
    let fd = unsafe {
        libc::memfd_create(
            c"pulsar-cuda-bootstrap".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    ensure!(
        fd >= 0,
        "CUDA bootstrap memfd unavailable: {}",
        std::io::Error::last_os_error()
    );
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(&bytes)?;
    file.seek(SeekFrom::Start(0))?;
    ensure!(
        unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, SEALS) } == 0,
        "CUDA bootstrap relay sealing failed"
    );
    // All non-stdio descriptors were erased before request intake. Verification
    // opens only scoped files which have now closed, so the relay occupies fd 3.
    ensure!(fd == 3, "CUDA bootstrap relay descriptor is not exclusive");
    ensure!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == 0,
        "CUDA bootstrap relay inheritance failed"
    );
    let error = Command::new(directory.join("ld-linux-x86-64.so.2"))
        .env_clear()
        .env("PULSAR_WORKER_CONFINED", "1")
        .env("PULSAR_CUDA_RELAY_FD", "3")
        .args(["--inhibit-cache", "--library-path"])
        .arg(directory)
        .args(["/pulsar", role])
        .exec();
    Err(error).context("verified CUDA loader transition failed")
}
#[cfg(feature = "cuda")]
pub(crate) fn read_relay() -> Result<Relay> {
    read_relay_for(BootstrapRole::Inference)
}
pub(crate) fn read_relay_for(expected: BootstrapRole) -> Result<Relay> {
    let bytes = read_relay_bytes()?;
    let relay: Relay = serde_json::from_slice(&bytes)?;
    ensure!(relay.role == expected, "CUDA sealed relay role differs");
    relay_deadline_instant(
        relay.deadline_ns,
        relay.witness.request.budget.wall_time_ms,
        relay.witness.execution_deadline_ns,
    )?;
    Ok(relay)
}

fn read_relay_bytes() -> Result<Vec<u8>> {
    ensure!(
        std::env::var("PULSAR_WORKER_CONFINED").as_deref() == Ok("1")
            && std::env::var("PULSAR_CUDA_RELAY_FD").as_deref() == Ok("3"),
        "CUDA execution requires its private sealed relay"
    );
    ensure!(
        unsafe { libc::fcntl(3, libc::F_GET_SEALS) } == SEALS,
        "CUDA bootstrap relay is not fully sealed"
    );
    let mut file = unsafe { File::from_raw_fd(3) };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() > 0 && metadata.len() <= 1024 * 1024,
        "CUDA relay file admission"
    );
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "CUDA relay read admission");
    Ok(bytes)
}

#[path = "live_cuda_bootstrap.rs"]
mod live;
pub(crate) use live::{read_live_relay, reexec_live_memory};
#[cfg(feature = "cuda")]
pub(crate) use live::{read_live_stream_relay, reexec_live_stream};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_execution_relay_cannot_renew_parent_cap_or_accept_a_rebound_budget() {
        let end = Instant::now() + Duration::from_secs(1);
        let inherited = deadline_ns_in_execution(end, 30_000).unwrap();
        let decoded = execution_deadline_instant(inherited, 30_000).unwrap();
        assert!(decoded <= end && decoded > Instant::now());
        assert!(relay_deadline_instant(inherited, 30_000, Some(inherited)).unwrap() <= end);
        assert!(relay_deadline_instant(inherited + 1, 30_000, Some(inherited)).is_err());
        assert!(deadline_ns_in_execution(end, 0).is_err());
        assert!(deadline_ns_in_execution(end, 86_400_001).is_err());
        assert!(deadline_ns_in_execution(end, 1).is_err());
        assert!(deadline_ns_in_execution(Instant::now() - Duration::from_secs(1), 30_000).is_err());
        assert!(execution_deadline_instant(0, 30_000).is_err());
        assert!(execution_deadline_instant(inherited, 86_400_001).is_err());
    }

    #[test]
    fn live_relay_clock_conversion_never_renews_the_inherited_lease() {
        let end = Instant::now() + Duration::from_millis(500);
        let relay = deadline_ns_in_lease(end, 1000).unwrap();
        let decoded = deadline_instant(relay, 1000).unwrap();
        assert!(decoded <= end && decoded > Instant::now());
        assert!(deadline_ns_in_lease(end, 1).is_err());
        assert!(deadline_ns_in_lease(Instant::now() - Duration::from_secs(1), 1000).is_err());
        assert!(deadline_ns_in_lease(end, 0).is_err());
        assert!(deadline_ns_in_lease(end, 30_001).is_err());
    }
    #[test]
    fn loader_transition_keeps_the_original_deadline() {
        let end = deadline_ns(1000).unwrap();
        let deadline = deadline_instant(end, 1000).unwrap();
        assert!(deadline > Instant::now());
        assert!(deadline.duration_since(Instant::now()) <= Duration::from_secs(1));
        assert!(deadline_instant(end, 1).is_err());
        assert!(deadline_instant(0, 1000).is_err());
        assert!(deadline_instant(u64::MAX, 1000).is_err());
        assert!(deadline_ns(0).is_err());
        assert!(deadline_ns(86_400_001).is_err());
    }
}
