//! Descriptor-held captured aliases. This is mount custody, not native admission.
#![cfg(target_os = "linux")]
use crate::native_dependencies::CapturedNativeDependencies;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedNativeAlias {
    pub name: String,
    pub sha256: String,
    pub byte_len: u64,
    pub filesystem_device: u64,
    pub inode: u64,
}

/// Borrows the capture so its storage reservation and cleanup owner stay alive.
/// The caller must additionally retain this plan until child-tree retirement.
pub struct PinnedNativeMounts<'a> {
    _capture: &'a CapturedNativeDependencies,
    directory: File,
    aliases: Vec<(File, MountedNativeAlias)>,
    guest: PathBuf,
}
fn pin(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}
impl<'a> PinnedNativeMounts<'a> {
    /// Metadata only; the confined worker must authenticate every mounted alias
    /// before loading native code. Same-inode aliases may share a fresh hash in
    /// one verification pass. A held descriptor does not freeze mutable contents.
    pub fn new(capture: &'a CapturedNativeDependencies) -> Result<Self> {
        Self::new_at(capture, &capture.library_directory())
    }
    /// CPU captures use one stable private guest path so a fresh content-equal
    /// capture does not change the immutable request or its progress basis.
    pub(crate) fn new_at(capture: &'a CapturedNativeDependencies, guest: &Path) -> Result<Self> {
        capture.verify_metadata()?;
        let captured = capture.library_directory();
        ensure!(
            guest == captured || guest == Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY),
            "native image guest path is not an admitted capture target"
        );
        ensure!(
            guest.is_absolute()
                && !guest.components().any(|component| matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )),
            "native image guest path must be absolute and normalized"
        );
        let guest = guest.to_path_buf();
        let directory = pin(&captured)?;
        ensure!(
            directory.metadata()?.is_dir(),
            "captured native directory is not a directory"
        );
        let mut aliases = Vec::new();
        for library in &capture.specification().libraries {
            let content = crate::artifacts::open_regular(
                &captured
                    .parent()
                    .context("capture root missing")?
                    .join("content")
                    .join(&library.sha256),
            )?;
            let expected = content.metadata()?;
            for name in &library.names {
                let file = pin(&captured.join(name))?;
                let actual = file.metadata()?;
                ensure!(
                    actual.is_file()
                        && actual.permissions().readonly()
                        && actual.len() == library.byte_len
                        && actual.dev() == expected.dev()
                        && actual.ino() == expected.ino(),
                    "captured native alias changed while pinning"
                );
                aliases.push((
                    file,
                    MountedNativeAlias {
                        name: name.clone(),
                        sha256: library.sha256.clone(),
                        byte_len: library.byte_len,
                        filesystem_device: actual.dev(),
                        inode: actual.ino(),
                    },
                ));
            }
        }
        ensure!(
            !aliases.is_empty() && aliases.len() <= 512,
            "captured native alias capacity"
        );
        Ok(Self {
            _capture: capture,
            directory,
            aliases,
            guest,
        })
    }
    /// Detach descriptor storage only for a caller that has already installed
    /// the capture owner in shared worker custody. This is not image ownership.
    pub(crate) fn into_descriptors(self) -> Vec<File> {
        let mut files = vec![self.directory];
        files.extend(self.aliases.into_iter().map(|(file, _)| file));
        files
    }
    pub fn guest_directory(&self) -> &Path {
        &self.guest
    }
    pub fn witnesses(&self) -> Vec<MountedNativeAlias> {
        self.aliases
            .iter()
            .map(|(_, witness)| witness.clone())
            .collect()
    }
    /// Append before the executable and its arguments in the bubblewrap command.
    /// Overlay every alias from its held descriptor, so replacing the captured
    /// directory entry cannot select replacement bytes during namespace setup.
    pub fn append_bindings(&self, command: &mut Command) -> Result<()> {
        self.append_bindings_inner(command, false)
    }
    /// Default loader roots expose only held aliases, with no host directory or
    /// linker cache fallback. Each anonymous root becomes read-only before exec.
    pub(crate) fn append_closed_runtime_bindings(&self, command: &mut Command) -> Result<()> {
        self.append_bindings_inner(command, true)
    }
    fn append_bindings_inner(&self, command: &mut Command, closed_runtime: bool) -> Result<()> {
        let runtime_roots = [
            Path::new("/usr/lib"),
            Path::new("/lib"),
            Path::new("/lib64"),
        ];
        if closed_runtime {
            for root in runtime_roots {
                command.arg("--tmpfs").arg(root);
            }
        }
        let mut descriptors = vec![self.directory.as_raw_fd()];
        ensure!(
            self.directory.metadata()?.is_dir(),
            "held native directory changed"
        );
        command
            .arg("--ro-bind")
            .arg(format!("/proc/self/fd/{}", self.directory.as_raw_fd()))
            .arg(&self.guest);
        for (file, witness) in &self.aliases {
            let actual = file.metadata()?;
            ensure!(
                actual.is_file()
                    && actual.permissions().readonly()
                    && actual.len() == witness.byte_len
                    && actual.dev() == witness.filesystem_device
                    && actual.ino() == witness.inode,
                "held native alias metadata changed"
            );
            descriptors.push(file.as_raw_fd());
            command
                .arg("--ro-bind")
                .arg(format!("/proc/self/fd/{}", file.as_raw_fd()))
                .arg(self.guest.join(&witness.name));
            if closed_runtime {
                for root in runtime_roots {
                    command
                        .arg("--ro-bind")
                        .arg(format!("/proc/self/fd/{}", file.as_raw_fd()))
                        .arg(root.join(&witness.name));
                }
            }
        }
        if closed_runtime {
            for root in runtime_roots {
                command.arg("--remount-ro").arg(root);
            }
        }
        ensure!(
            descriptors.iter().all(|fd| *fd < 1024),
            "native mount descriptors exceed child limit"
        );
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

/// Verify namespace inode correspondence before native loading. Hash verification
/// remains separate and mandatory; this does not authenticate a caller's table.
pub fn verify_mounted(directory: &Path, expected: &[MountedNativeAlias]) -> Result<()> {
    ensure!(
        directory.is_absolute()
            && !directory
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            && !expected.is_empty()
            && expected.len() <= 512,
        "invalid mounted native table"
    );
    let mut names = std::collections::BTreeSet::new();
    for witness in expected {
        ensure!(
            !witness.name.is_empty()
                && witness.name.len() <= 128
                && witness.name != "."
                && witness.name != ".."
                && witness
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
                && names.insert(witness.name.as_str())
                && witness.byte_len > 0
                && witness.byte_len <= 2 * 1024 * 1024 * 1024
                && witness.sha256.len() == 64
                && witness
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid mounted native alias"
        );
        let file = crate::artifacts::open_regular(&directory.join(&witness.name))?;
        let actual = file.metadata()?;
        ensure!(
            actual.permissions().readonly()
                && actual.len() == witness.byte_len
                && actual.dev() == witness.filesystem_device
                && actual.ino() == witness.inode,
            "mounted native alias differs from held identity"
        );
    }
    let actual = std::fs::symlink_metadata(directory)?;
    ensure!(
        actual.is_dir() && !actual.file_type().is_symlink(),
        "mounted native directory changed"
    );
    let mut count = 0;
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name();
        ensure!(
            name.to_str().is_some_and(|n| names.contains(n)),
            "undeclared mounted native alias"
        );
        count += 1;
        ensure!(count <= expected.len(), "mounted native alias capacity");
    }
    ensure!(count == expected.len(), "mounted native alias missing");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::{NativeDependencySpec, NativeLibrarySpec};
    use sha2::{Digest, Sha256};
    fn fixture(root: &Path) -> CapturedNativeDependencies {
        let input = root.join("supplier");
        std::fs::write(&input, b"authored original").unwrap();
        CapturedNativeDependencies::capture(
            &NativeDependencySpec {
                schema_version: 1,
                libraries: vec![NativeLibrarySpec {
                    path: input,
                    sha256: format!("{:x}", Sha256::digest(b"authored original")),
                    byte_len: 17,
                    names: vec!["libfixture.so".into(), "libfixture.so.1".into()],
                }],
            },
            &root.join("capture"),
        )
        .unwrap()
    }
    #[test]
    fn cpu_guest_path_is_stable_without_changing_cuda_capture_paths() {
        let root = tempfile::tempdir().unwrap();
        let captured = fixture(root.path());
        let ordinary = PinnedNativeMounts::new(&captured).unwrap();
        assert_eq!(ordinary.guest_directory(), captured.library_directory());
        let cpu = PinnedNativeMounts::new_at(
            &captured,
            Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY),
        )
        .unwrap();
        assert_eq!(
            cpu.guest_directory(),
            Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY)
        );
        assert_eq!(
            serde_json::to_vec(&cpu.witnesses()).unwrap(),
            serde_json::to_vec(&ordinary.witnesses()).unwrap()
        );
        for invalid in [
            "/usr/lib",
            "/lib",
            "/lib64",
            "/bin",
            "/",
            "/pulsar",
            "relative",
            "/unadmitted",
        ] {
            assert!(PinnedNativeMounts::new_at(&captured, Path::new(invalid)).is_err());
        }
        let mut command = Command::new("/usr/bin/bwrap");
        cpu.append_closed_runtime_bindings(&mut command).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        for forbidden in ["/usr", "/lib", "/lib64", "/bin", "/etc/ld.so.cache"] {
            assert!(!args
                .windows(3)
                .any(|window| window == ["--ro-bind", forbidden, forbidden]));
        }
        for runtime_root in ["/usr/lib", "/lib", "/lib64"] {
            assert!(args
                .windows(2)
                .any(|window| window == ["--tmpfs", runtime_root]));
            assert!(args
                .windows(2)
                .any(|window| window == ["--remount-ro", runtime_root]));
        }
        assert!(args.contains(&crate::cpu_mounts::CPU_LIBRARY_DIRECTORY));
    }
    #[test]
    fn mounted_identity_refuses_replacement_extra_entries_and_invalid_tables() {
        let root = tempfile::tempdir().unwrap();
        let captured = fixture(root.path());
        let pinned = PinnedNativeMounts::new(&captured).unwrap();
        let witnesses = pinned.witnesses();
        verify_mounted(pinned.guest_directory(), &witnesses).unwrap();
        let mut bad = witnesses.clone();
        bad[0].inode += 1;
        assert!(verify_mounted(pinned.guest_directory(), &bad).is_err());
        bad = witnesses.clone();
        bad[0].name = "../escape".into();
        assert!(verify_mounted(pinned.guest_directory(), &bad).is_err());
        bad = witnesses.clone();
        bad.push(bad[0].clone());
        assert!(verify_mounted(pinned.guest_directory(), &bad).is_err());
        std::fs::write(pinned.guest_directory().join("extra"), b"extra").unwrap();
        assert!(verify_mounted(pinned.guest_directory(), &witnesses).is_err());
        std::fs::remove_file(pinned.guest_directory().join("extra")).unwrap();
        std::fs::rename(
            pinned.guest_directory().join("libfixture.so"),
            root.path().join("old"),
        )
        .unwrap();
        std::fs::write(
            pinned.guest_directory().join("libfixture.so"),
            b"authored original",
        )
        .unwrap();
        assert!(verify_mounted(pinned.guest_directory(), &witnesses).is_err());
        let fd = pinned.aliases[0].0.as_raw_fd();
        assert_eq!(
            std::fs::read(format!("/proc/self/fd/{fd}")).unwrap(),
            b"authored original"
        );
        let mut command = Command::new("/usr/bin/bwrap");
        pinned.append_bindings(&mut command).unwrap();
    }
    #[test]
    fn pinned_aliases_refuse_backing_mode_changes_and_final_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let captured = fixture(root.path());
        let pinned = PinnedNativeMounts::new(&captured).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            pinned.guest_directory().join("libfixture.so"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(pinned
            .append_bindings(&mut Command::new("/usr/bin/bwrap"))
            .is_err());
        assert!(PinnedNativeMounts::new(&captured).is_err());
        std::fs::set_permissions(
            pinned.guest_directory().join("libfixture.so"),
            std::fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        std::fs::remove_file(pinned.guest_directory().join("libfixture.so")).unwrap();
        std::os::unix::fs::symlink(
            "libfixture.so.1",
            pinned.guest_directory().join("libfixture.so"),
        )
        .unwrap();
        assert!(PinnedNativeMounts::new(&captured).is_err());
    }
    #[test]
    fn closed_runtime_roots_use_held_aliases_and_remount_each_root_read_only() {
        let root = tempfile::tempdir().unwrap();
        let captured = fixture(root.path());
        let pinned = PinnedNativeMounts::new(&captured).unwrap();
        let mut command = Command::new("/usr/bin/bwrap");
        pinned.append_closed_runtime_bindings(&mut command).unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for target in ["/usr/lib", "/lib", "/lib64"] {
            let creation = args
                .windows(2)
                .position(|a| a == ["--tmpfs", target])
                .unwrap();
            let sealing = args
                .windows(2)
                .position(|a| a == ["--remount-ro", target])
                .unwrap();
            assert!(creation < sealing);
            for (file, witness) in &pinned.aliases {
                let source = format!("/proc/self/fd/{}", file.as_raw_fd());
                let alias = format!("{target}/{}", witness.name);
                let mount = args
                    .windows(3)
                    .position(|a| a == ["--ro-bind", &source, &alias])
                    .unwrap();
                assert!(creation < mount && mount < sealing);
            }
        }
    }

    #[test]
    #[ignore = "requires explicitly supplied authored static filesystem checker"]
    fn native_closed_runtime_roots_exclude_host_fallback_and_are_read_only() {
        let checker = std::env::var_os("PULSAR_CLOSED_RUNTIME_CHECKER")
            .expect("explicit native checker path required");
        let root = tempfile::tempdir().unwrap();
        let captured = fixture(root.path());
        let pinned = PinnedNativeMounts::new(&captured).unwrap();
        let mut command = Command::new("timeout");
        command.args([
            "10",
            "/usr/bin/bwrap",
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/usr",
            "/usr",
            "--ro-bind",
            "/lib",
            "/lib",
            "--ro-bind",
            "/lib64",
            "/lib64",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
        ]);
        pinned.append_closed_runtime_bindings(&mut command).unwrap();
        command
            .arg("--ro-bind")
            .arg(checker)
            .arg("/checker")
            .arg("/checker")
            .arg(pinned.guest_directory());
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "checker: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            b"closed roots: held bytes, identity, absence, read-only PASS\n"
        );
    }
}
