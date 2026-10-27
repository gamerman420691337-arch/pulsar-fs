//! Worker-only NVML query paired with the runtime CUDA ordinal identity.
use super::cuda_identity::{CudaSdkDevice, CudaSdkExpectation};
use crate::{cuda_bootstrap, cuda_memory::MemoryReply, cuda_mounts::VerifiedCudaBootstrap};
use anyhow::{ensure, Context, Result};
use libloading::Library;
use std::{ffi::CString, fs::File, os::fd::AsRawFd};

// nvml.h's v1 memory ABI: total = free + used (including driver reservations).
#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}
type Status = libc::c_uint;
type Init = unsafe extern "C" fn() -> Status;
type Shutdown = unsafe extern "C" fn() -> Status;
type Handle = *mut libc::c_void;
type ByUuid = unsafe extern "C" fn(*const libc::c_char, *mut Handle) -> Status;
type Uuid = unsafe extern "C" fn(Handle, *mut libc::c_char, libc::c_uint) -> Status;
type GetMemory = unsafe extern "C" fn(Handle, *mut Memory) -> Status;
struct Nvml {
    library: Library,
    _file: File,
    shutdown: Shutdown,
}
impl Nvml {
    fn open(path: &std::path::Path) -> Result<Self> {
        let file = crate::artifacts::open_regular(path)?;
        // SAFETY: verified captured native code in an isolated worker; the held
        // file selects the inode and all symbols use installed nvml.h's C ABI.
        let library = unsafe { Library::new(format!("/proc/self/fd/{}", file.as_raw_fd())) }?;
        let shutdown = unsafe { *library.get::<Shutdown>(b"nvmlShutdown\0")? };
        let init = unsafe { *library.get::<Init>(b"nvmlInit_v2\0")? };
        check(unsafe { init() }, "initialization")?;
        Ok(Self {
            library,
            _file: file,
            shutdown,
        })
    }
    fn memory(&self, expected: &str) -> Result<(Memory, u64)> {
        let uuid = CString::new(expected)?;
        let mut handle: Handle = std::ptr::null_mut();
        let mut actual = [0x7fu8; 96];
        let mut memory = Memory::default();
        // SAFETY: signatures, pointer alignment, buffer lengths and live library
        // match the documented NVML ABI; the handle is checked before use.
        let started = unsafe {
            let by_uuid = self.library.get::<ByUuid>(b"nvmlDeviceGetHandleByUUID\0")?;
            let get_uuid = self.library.get::<Uuid>(b"nvmlDeviceGetUUID\0")?;
            let get_memory = self
                .library
                .get::<GetMemory>(b"nvmlDeviceGetMemoryInfo\0")?;
            check(by_uuid(uuid.as_ptr(), &mut handle), "UUID lookup")?;
            ensure!(!handle.is_null(), "NVML returned null device handle");
            check(
                get_uuid(handle, actual.as_mut_ptr().cast(), actual.len() as u32),
                "UUID readback",
            )?;
            let end = actual
                .iter()
                .position(|b| *b == 0)
                .context("NVML UUID is not terminated")?;
            ensure!(
                std::str::from_utf8(&actual[..end])?.eq_ignore_ascii_case(expected),
                "NVML UUID differs from CUDA runtime"
            );
            let started = cuda_bootstrap::now_ns()?;
            check(get_memory(handle, &mut memory), "memory query")?;
            started
        };
        Ok((memory, started))
    }
}
impl Drop for Nvml {
    fn drop(&mut self) {
        let _ = unsafe { (self.shutdown)() };
    }
}
fn check(status: Status, operation: &str) -> Result<()> {
    ensure!(status == 0, "NVML {operation} failed with status {status}");
    Ok(())
}
pub(crate) fn measure(bootstrap: &VerifiedCudaBootstrap<'_>) -> Result<MemoryReply> {
    let request = bootstrap.request();
    let execution = request
        .cuda_execution
        .as_ref()
        .context("CUDA execution missing")?;
    let sample = measure_execution(execution, bootstrap.deadline(), || bootstrap.verify_after())?;
    Ok(MemoryReply {
        schema_version: 1,
        request_sha256: crate::cuda_mounts::request_hash(request)?,
        device: sample.device,
        nvml_total_bytes: sample.nvml_total_bytes,
        nvml_free_bytes: sample.nvml_free_bytes,
        nvml_used_bytes: sample.nvml_used_bytes,
        sample_started_ns: sample.sample_started_ns,
    })
}
pub(crate) fn measure_live(
    bootstrap: &crate::cuda_mounts::VerifiedLiveCudaBootstrap<'_>,
) -> Result<crate::cuda_memory::MemorySample> {
    measure_execution(
        &bootstrap.manifest().execution,
        bootstrap.deadline(),
        || bootstrap.verify_after(),
    )
}
fn measure_execution(
    execution: &pulsar_protocol::WorkerCudaExecution,
    deadline: std::time::Instant,
    verify_after: impl FnOnce() -> Result<()>,
) -> Result<crate::cuda_memory::MemorySample> {
    let directory = &execution.library_directory;
    let driver = CudaSdkDevice::open(
        &directory.join("libcuda.so.1"),
        CudaSdkExpectation {
            device_uuid: execution.device_uuid.clone(),
            pci_bus_address: execution.pci_bus_address.clone(),
            device_ordinal: u16::try_from(execution.device_ordinal)?,
        },
    )?;
    let nvml = Nvml::open(&directory.join("libnvidia-ml.so.1"))?;
    // Complete the initial native query and full post-initialization integrity
    // check before taking the sample used for admission. Re-query actual memory
    // afterward; never renew the timestamp of the earlier measurement.
    let _ = nvml.memory(&driver.initial().device_uuid)?;
    verify_after()?;
    let (memory, sample_started_ns) = nvml.memory(&driver.initial().device_uuid)?;
    let device = driver.recheck()?;
    ensure!(
        std::time::Instant::now() < deadline,
        "CUDA memory query deadline exceeded"
    );
    Ok(crate::cuda_memory::MemorySample {
        device,
        nvml_total_bytes: memory.total,
        nvml_free_bytes: memory.free,
        nvml_used_bytes: memory.used,
        sample_started_ns,
    })
}
