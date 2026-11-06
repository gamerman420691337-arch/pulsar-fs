//! Worker-only CUDA driver effects. Call only after broker capture and confinement.
//!
//! SDK observations are consistency checks, not authorization or a hardware lease.
//! Keep this object alive alongside the session and recheck around native work.
use anyhow::{ensure, Context, Result};
use libloading::Library;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    os::fd::AsRawFd,
    path::{Component, Path},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaSdkExpectation {
    pub device_uuid: String,
    pub pci_bus_address: String,
    /// Logical ordinal in this worker's CUDA driver, not the kernel node minor.
    pub device_ordinal: u16,
}
impl CudaSdkExpectation {
    pub fn validate(&self) -> Result<()> {
        pulsar_protocol::validate_cuda_device_identity(
            u32::from(self.device_ordinal),
            &self.device_uuid,
        )?;
        pci_address(&self.pci_bus_address)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaSdkObservation {
    pub device_uuid: String,
    pub pci_bus_address: String,
    pub device_ordinal: u16,
    pub visible_device_count: u32,
    pub total_bytes: u64,
    pub driver_version: i32,
}

// CUDA's Linux driver ABI uses CUdevice=int, CUresult=int and a 16-byte CUuuid.
#[repr(C)]
struct DriverUuid {
    bytes: [u8; 16],
}
type Init = unsafe extern "C" fn(u32) -> i32;
type Count = unsafe extern "C" fn(*mut i32) -> i32;
type Get = unsafe extern "C" fn(*mut i32, i32) -> i32;
type Uuid = unsafe extern "C" fn(*mut DriverUuid, i32) -> i32;
type Pci = unsafe extern "C" fn(*mut libc::c_char, i32, i32) -> i32;
type Memory = unsafe extern "C" fn(*mut usize, i32) -> i32;
type Version = unsafe extern "C" fn(*mut i32) -> i32;

/// A held, explicitly selected driver and its initial verified SDK observation.
/// This does not create a CUDA context, reserve VRAM or initialize ONNX Runtime.
pub struct CudaSdkDevice {
    // Retain both handles: symbols never outlive the library; loading never
    // reopens the supplier pathname after the regular-file check.
    library: Library,
    _file: File,
    expected: CudaSdkExpectation,
    initial: CudaSdkObservation,
}
impl CudaSdkDevice {
    /// Loads native code from an absolute, nofollow regular file. The caller must
    /// supply an authorized captured driver within an isolated worker; hashes,
    /// namespace mounts and deadlines are broker responsibilities.
    pub fn open(driver: &Path, expected: CudaSdkExpectation) -> Result<Self> {
        expected.validate()?;
        ensure!(
            driver.is_absolute()
                && !driver
                    .components()
                    .any(|c| matches!(c, Component::ParentDir)),
            "CUDA driver must be an explicit absolute path without traversal"
        );
        let file = crate::artifacts::open_regular(driver)?;
        let fd_path = format!("/proc/self/fd/{}", file.as_raw_fd());
        // SAFETY: only trusted captured native code is supported. The held file
        // selects the inode; all requested symbols use the documented Linux ABI.
        let library = unsafe { Library::new(&fd_path) }.context("load selected CUDA driver")?;
        let initial = observe(&library, &expected)?;
        Ok(Self {
            library,
            _file: file,
            expected,
            initial,
        })
    }
    pub fn initial(&self) -> &CudaSdkObservation {
        &self.initial
    }
    /// Re-query this loaded driver and refuse changed identity/inventory facts.
    pub fn recheck(&self) -> Result<CudaSdkObservation> {
        let current = observe(&self.library, &self.expected)?;
        ensure!(
            current == self.initial,
            "CUDA SDK device facts changed during worker lifetime"
        );
        Ok(current)
    }
}

fn status(code: i32, operation: &str) -> Result<()> {
    ensure!(code == 0, "CUDA SDK {operation} failed with status {code}");
    Ok(())
}
fn observe(library: &Library, expected: &CudaSdkExpectation) -> Result<CudaSdkObservation> {
    let (mut count, mut device, mut version) = (0i32, 0i32, 0i32);
    let mut uuid = DriverUuid { bytes: [0; 16] };
    // Nonzero fill catches an absent terminator, including partial writes.
    let mut pci = [0x7fu8; 32];
    let mut total = 0usize;
    // SAFETY: symbol signatures follow cuda.h; all output buffers are live,
    // correctly sized/aligned, and the library is retained throughout the calls.
    unsafe {
        let init = library.get::<Init>(b"cuInit\0")?;
        let get_count = library.get::<Count>(b"cuDeviceGetCount\0")?;
        let get = library.get::<Get>(b"cuDeviceGet\0")?;
        let get_uuid = library.get::<Uuid>(b"cuDeviceGetUuid_v2\0")?;
        let get_pci = library.get::<Pci>(b"cuDeviceGetPCIBusId\0")?;
        let get_memory = library.get::<Memory>(b"cuDeviceTotalMem_v2\0")?;
        let get_version = library.get::<Version>(b"cuDriverGetVersion\0")?;
        status(init(0), "initialization")?;
        status(get_count(&mut count), "device count")?;
        ensure!(
            (1..=1024).contains(&count) && i32::from(expected.device_ordinal) < count,
            "requested CUDA ordinal is outside the visible SDK inventory"
        );
        status(
            get(&mut device, i32::from(expected.device_ordinal)),
            "ordinal lookup",
        )?;
        status(get_uuid(&mut uuid, device), "UUID lookup")?;
        status(
            get_pci(pci.as_mut_ptr().cast(), pci.len() as i32, device),
            "PCI lookup",
        )?;
        status(get_memory(&mut total, device), "memory lookup")?;
        status(get_version(&mut version), "driver version")?;
    }
    let end = pci
        .iter()
        .position(|b| *b == 0)
        .context("CUDA SDK PCI string is not terminated")?;
    let pci = std::str::from_utf8(&pci[..end]).context("CUDA SDK PCI string is not ASCII")?;
    let pci = canonical_pci(pci)?;
    let hex: String = uuid.bytes.iter().map(|b| format!("{b:02x}")).collect();
    let device_uuid = format!(
        "GPU-{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    ensure!(
        total > 0 && (total as u64) <= (1u64 << 40) && version > 0,
        "invalid CUDA SDK memory or driver version"
    );
    let observation = CudaSdkObservation {
        device_uuid,
        pci_bus_address: pci,
        device_ordinal: expected.device_ordinal,
        visible_device_count: count as u32,
        total_bytes: total as u64,
        driver_version: version,
    };
    match_expectation(expected, &observation)?;
    Ok(observation)
}
fn match_expectation(expected: &CudaSdkExpectation, observed: &CudaSdkObservation) -> Result<()> {
    ensure!(
        observed
            .device_uuid
            .eq_ignore_ascii_case(&expected.device_uuid)
            && observed.pci_bus_address == canonical_pci(&expected.pci_bus_address)?
            && observed.device_ordinal == expected.device_ordinal,
        "CUDA SDK UUID, PCI address or ordinal differs from requested device"
    );
    Ok(())
}
fn pci_address(value: &str) -> Result<(u16, u8, u8, u8)> {
    let b = value.as_bytes();
    ensure!(
        (b.len() == 12 || b.len() == 16) && b.is_ascii(),
        "invalid CUDA PCI address length"
    );
    let d = b.len() - 8;
    ensure!(
        b[d] == b':'
            && b[d + 3] == b':'
            && b[d + 6] == b'.'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == d || i == d + 3 || i == d + 6 || c.is_ascii_hexdigit()),
        "invalid CUDA PCI address syntax"
    );
    let domain = u16::from_str_radix(&value[..d], 16)?;
    let bus = u8::from_str_radix(&value[d + 1..d + 3], 16)?;
    let device = u8::from_str_radix(&value[d + 4..d + 6], 16)?;
    let function = u8::from_str_radix(&value[d + 7..], 16)?;
    ensure!(
        device <= 31 && function <= 7,
        "invalid CUDA PCI device/function"
    );
    Ok((domain, bus, device, function))
}
fn canonical_pci(value: &str) -> Result<String> {
    let (d, b, s, f) = pci_address(value)?;
    Ok(format!("{d:04x}:{b:02x}:{s:02x}.{f:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn expected() -> CudaSdkExpectation {
        CudaSdkExpectation {
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            pci_bus_address: "0000:01:00.0".into(),
            device_ordinal: 0,
        }
    }
    #[test]
    fn pci_normalization_and_bounds() {
        assert_eq!(canonical_pci("00000000:AB:1F.7").unwrap(), "0000:ab:1f.7");
        for bad in [
            "../escape",
            "00010000:01:00.0",
            "0000:01:20.0",
            "0000:01:00.8",
            "0000:01:00",
            "００００:01:00.0",
            "0000:01:00.0\0",
        ] {
            assert!(canonical_pci(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn each_identity_coordinate_is_required() {
        let e = expected();
        let o = CudaSdkObservation {
            device_uuid: e.device_uuid.to_uppercase(),
            pci_bus_address: e.pci_bus_address.clone(),
            device_ordinal: 0,
            visible_device_count: 1,
            total_bytes: 1,
            driver_version: 1,
        };
        match_expectation(&e, &o).unwrap();
        let mut bad = o.clone();
        bad.device_ordinal = 1;
        assert!(match_expectation(&e, &bad).is_err());
        bad = o.clone();
        bad.device_uuid.replace_range(4..5, "2");
        assert!(match_expectation(&e, &bad).is_err());
        bad = o;
        bad.pci_bus_address = "0000:02:00.0".into();
        assert!(match_expectation(&e, &bad).is_err());
    }
    #[test]
    fn malformed_expectations_refuse_before_loading() {
        let mut e = expected();
        e.device_ordinal = 64;
        assert!(CudaSdkDevice::open(Path::new("/does-not-exist"), e)
            .err()
            .unwrap()
            .to_string()
            .contains("identity declaration"));
        let e = expected();
        assert!(CudaSdkDevice::open(Path::new("relative.so"), e.clone()).is_err());
        assert!(CudaSdkDevice::open(Path::new("/tmp/../driver.so"), e).is_err());
        assert!(serde_json::from_str::<CudaSdkExpectation>(
            r#"{"device_uuid":"x","pci_bus_address":"x","device_ordinal":0,"extra":1}"#
        )
        .is_err());
        assert!(status(304, "initialization").is_err());
    }
}
