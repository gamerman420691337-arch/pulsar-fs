//! Linux kernel identity and lifetime-held device nodes. No authority, loading or VRAM quota.
#![cfg(target_os = "linux")]
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::os::unix::process::CommandExt;
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CudaDeviceExpectation {
    pub device_uuid: String,
    pub pci_bus_address: String,
    /// Kernel device minor, not a CUDA logical ordinal.
    pub device_minor: u32,
}
impl CudaDeviceExpectation {
    pub fn validate(&self) -> Result<()> {
        pulsar_protocol::validate_cuda_device_identity(0, &self.device_uuid)?;
        let b = self.pci_bus_address.as_bytes();
        ensure!(
            b.len() == 12
                && b.iter().enumerate().all(|(i, b)| match i {
                    4 | 7 => *b == b':',
                    10 => *b == b'.',
                    _ => b.is_ascii_hexdigit(),
                })
                && u8::from_str_radix(&self.pci_bus_address[8..10], 16)? <= 31
                && u8::from_str_radix(&self.pci_bus_address[11..], 16)? <= 7
                && self.device_minor <= 63,
            "invalid CUDA kernel device expectation"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceNodeIdentity {
    pub guest_path: String,
    pub major: u32,
    pub minor: u32,
    pub filesystem_device: u64,
    pub inode: u64,
}
struct HeldNode {
    file: File,
    identity: DeviceNodeIdentity,
}
pub struct PinnedCudaDevices {
    expectation: CudaDeviceExpectation,
    nodes: Vec<HeldNode>,
}

fn kernel_text(path: &Path, limit: usize) -> Result<String> {
    let mut file = crate::artifacts::open_regular(path)?;
    ensure!(
        file.metadata()?.uid() == 0,
        "kernel GPU information must be root-owned"
    );
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } == 0,
        "cannot establish kernel information filesystem"
    );
    ensure!(
        filesystem.f_type == libc::PROC_SUPER_MAGIC,
        "GPU information must come from procfs"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "kernel GPU information exceeds limit");
    Ok(String::from_utf8(bytes)?)
}
fn match_information(text: &str, expected: &CudaDeviceExpectation) -> Result<()> {
    expected.validate()?;
    ensure!(text.len() <= 8192, "kernel GPU information exceeds limit");
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if matches!(key, "GPU UUID" | "Bus Location" | "Device Minor") {
                ensure!(
                    fields.insert(key, value.trim()).is_none(),
                    "duplicate kernel GPU identity field"
                );
            }
        }
    }
    ensure!(
        fields.get("GPU UUID").copied() == Some(expected.device_uuid.as_str())
            && fields.get("Bus Location").copied() == Some(expected.pci_bus_address.as_str())
            && fields
                .get("Device Minor")
                .context("missing kernel device minor")?
                .parse::<u32>()?
                == expected.device_minor,
        "kernel GPU identity differs from expectation"
    );
    Ok(())
}
fn registered_major(text: &str, name: &str) -> Result<u32> {
    ensure!(text.len() <= 65536, "kernel device registry exceeds limit");
    let mut character = false;
    let mut result = None;
    for line in text.lines() {
        if line == "Character devices:" {
            character = true;
            continue;
        }
        if line == "Block devices:" {
            character = false;
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if character && fields.len() == 2 && fields[1] == name {
            ensure!(
                result.is_none(),
                "duplicate kernel character-device registration"
            );
            let major = fields[0].parse::<u32>()?;
            ensure!(major > 0 && major <= 4095, "invalid kernel device major");
            result = Some(major);
        }
    }
    result.context("required kernel character device is not registered")
}
fn open_path(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}
fn pin_node(path: &Path, major: u32, minor: u32) -> Result<HeldNode> {
    let file = open_path(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.file_type().is_char_device() && metadata.uid() == 0,
        "CUDA device must be a root-owned character node"
    );
    ensure!(
        libc::major(metadata.rdev()) == major && libc::minor(metadata.rdev()) == minor,
        "CUDA device number differs from kernel registration"
    );
    Ok(HeldNode {
        file,
        identity: DeviceNodeIdentity {
            guest_path: path.to_str().context("invalid CUDA node path")?.into(),
            major,
            minor,
            filesystem_device: metadata.dev(),
            inode: metadata.ino(),
        },
    })
}
impl PinnedCudaDevices {
    pub fn open(expected: &CudaDeviceExpectation) -> Result<Self> {
        expected.validate()?;
        let information = kernel_text(
            &PathBuf::from("/proc/driver/nvidia/gpus")
                .join(&expected.pci_bus_address)
                .join("information"),
            8192,
        )?;
        match_information(&information, expected)?;
        let registry = kernel_text(Path::new("/proc/devices"), 65536)?;
        let gpu = registered_major(&registry, "nvidia")?;
        let control = registered_major(&registry, "nvidiactl")?;
        ensure!(
            gpu == control,
            "NVIDIA control and GPU registrations differ"
        );
        let uvm = registered_major(&registry, "nvidia-uvm")?;
        let nodes = vec![
            pin_node(Path::new("/dev/nvidiactl"), control, 255)?,
            pin_node(
                &PathBuf::from(format!("/dev/nvidia{}", expected.device_minor)),
                gpu,
                expected.device_minor,
            )?,
            pin_node(Path::new("/dev/nvidia-uvm"), uvm, 0)?,
        ];
        Ok(Self {
            expectation: expected.clone(),
            nodes,
        })
    }
    pub fn expectation(&self) -> &CudaDeviceExpectation {
        &self.expectation
    }
    pub fn identities(&self) -> Vec<&DeviceNodeIdentity> {
        self.nodes.iter().map(|node| &node.identity).collect()
    }
    /// Caller must authorize access and retain self for the complete child lifetime.
    /// Child inherits O_PATH descriptors. Verify guest identities before loading
    /// GPU dependencies: namespace bind implementations can resolve magic links.
    pub fn append_bindings(&self, command: &mut Command) -> Result<()> {
        for node in &self.nodes {
            let metadata = node.file.metadata()?;
            ensure!(
                metadata.file_type().is_char_device()
                    && metadata.dev() == node.identity.filesystem_device
                    && metadata.ino() == node.identity.inode
                    && libc::major(metadata.rdev()) == node.identity.major
                    && libc::minor(metadata.rdev()) == node.identity.minor,
                "held CUDA device identity changed"
            );
            command
                .arg("--dev-bind")
                .arg(format!("/proc/self/fd/{}", node.file.as_raw_fd()))
                .arg(&node.identity.guest_path);
        }
        let descriptors: Vec<_> = self
            .nodes
            .iter()
            .map(|node| node.file.as_raw_fd())
            .collect();
        unsafe {
            command.pre_exec(move || {
                for fd in &descriptors {
                    let flags = libc::fcntl(*fd, libc::F_GETFD);
                    if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        Ok(())
    }
}
/// Called inside confinement before loading GPU libraries. Mount implementations
/// may resolve a descriptor magic link; matching the mounted inode fails closed.
pub fn verify_mounted_nodes(nodes: &[DeviceNodeIdentity]) -> Result<()> {
    ensure!(
        nodes.len() == 3
            && nodes[0].guest_path == "/dev/nvidiactl"
            && nodes[0].minor == 255
            && nodes[1].guest_path == format!("/dev/nvidia{}", nodes[1].minor)
            && nodes[1].minor <= 63
            && nodes[2].guest_path == "/dev/nvidia-uvm"
            && nodes[2].minor == 0
            && nodes[0].major == nodes[1].major,
        "invalid mounted CUDA node table"
    );
    for node in nodes {
        let file = open_path(Path::new(&node.guest_path))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.file_type().is_char_device()
                && metadata.dev() == node.filesystem_device
                && metadata.ino() == node.inode
                && libc::major(metadata.rdev()) == node.major
                && libc::minor(metadata.rdev()) == node.minor,
            "mounted CUDA node differs from held identity"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn expected() -> CudaDeviceExpectation {
        CudaDeviceExpectation {
            device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
            pci_bus_address: "0000:01:00.0".into(),
            device_minor: 0,
        }
    }
    #[test]
    fn kernel_fields_must_match_unique_bounded_expectation() {
        let e = expected();
        let text = format!(
            "GPU UUID: {}\nBus Location: {}\nDevice Minor: 0\n",
            e.device_uuid, e.pci_bus_address
        );
        match_information(&text, &e).unwrap();
        assert!(match_information(&(text.clone() + "GPU UUID: wrong\n"), &e).is_err());
        assert!(match_information(&text.replace("Minor: 0", "Minor: 1"), &e).is_err());
        assert!(match_information(&text.replace(&e.device_uuid, "wrong"), &e).is_err());
        let mut bad = e.clone();
        bad.pci_bus_address = "../escape".into();
        assert!(bad.validate().is_err());
        bad = e.clone();
        bad.pci_bus_address = "0000:01:ff.0".into();
        assert!(bad.validate().is_err());
        assert!(match_information(&"x".repeat(8193), &e).is_err());
    }
    #[test]
    fn mounted_gate_refuses_empty_tables_and_unknown_identity_fields() {
        assert!(verify_mounted_nodes(&[]).is_err());
        assert!(serde_json::from_str::<DeviceNodeIdentity>(r#"{"guest_path":"/dev/nvidiactl","major":195,"minor":255,"filesystem_device":1,"inode":1,"authorized":true}"#).is_err());
    }
    #[test]
    fn registry_requires_unique_character_registration() {
        assert_eq!(
            registered_major(
                "Character devices:\n235 nvidia-uvm\nBlock devices:\n7 nvidia-uvm\n",
                "nvidia-uvm"
            )
            .unwrap(),
            235
        );
        assert!(registered_major(
            "Character devices:\n235 nvidia-uvm\n236 nvidia-uvm\n",
            "nvidia-uvm"
        )
        .is_err());
        assert!(registered_major("Block devices:\n235 nvidia-uvm\n", "nvidia-uvm").is_err());
    }
    #[test]
    fn held_descriptor_survives_original_path_replacement_and_regular_nodes_refuse() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        std::fs::write(&path, b"old").unwrap();
        let file = open_path(&path).unwrap();
        let inode = file.metadata().unwrap().ino();
        std::fs::rename(&path, root.path().join("old")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        let source = format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd());
        assert_eq!(std::fs::metadata(&source).unwrap().ino(), inode);
        assert_eq!(std::fs::read(&source).unwrap(), b"old");
        assert!(pin_node(&path, 1, 3).is_err());
        let link = root.path().join("link");
        std::os::unix::fs::symlink("/dev/null", &link).unwrap();
        assert!(pin_node(&link, 1, 3).is_err());
    }
}
