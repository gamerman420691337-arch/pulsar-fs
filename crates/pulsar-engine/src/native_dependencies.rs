//! Bounded engine-owned byte capture. No loading, ABI, closure or device authority.
use crate::artifacts;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub use pulsar_protocol::{NativeDependencySpec, NativeLibrarySpec};

/// Lifetime-owned private capture. Drop attempts cleanup, including partial failures.
/// Owner processes can still alter their files; recheck before a read-only mount.
/// The execution caller must own admission, lifetime and confinement.
pub struct CapturedNativeDependencies {
    root: CaptureRoot,
    spec: NativeDependencySpec,
}
enum CaptureRoot {
    Temporary(tempfile::TempDir),
    Supervised(OwnedCaptureDirectory),
}
struct OwnedCaptureDirectory {
    path: Option<PathBuf>,
}
impl Drop for OwnedCaptureDirectory {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_dir_all(path);
        }
    }
}
impl CaptureRoot {
    fn path(&self) -> &Path {
        match self {
            Self::Temporary(root) => root.path(),
            Self::Supervised(root) => root.path.as_deref().expect("capture directory custody"),
        }
    }
    fn keep(self) -> PathBuf {
        match self {
            Self::Temporary(root) => root.keep(),
            Self::Supervised(mut root) => root.path.take().expect("capture directory custody"),
        }
    }
    fn close(self) -> std::io::Result<()> {
        match self {
            Self::Temporary(root) => root.close(),
            Self::Supervised(mut root) => {
                fs::remove_dir_all(root.path.take().expect("capture directory custody"))
            }
        }
    }
}

impl CapturedNativeDependencies {
    /// The confined capture child alone transfers this directory to its parent.
    /// Parent adoption uses its own specification, never child-supplied paths.
    pub(crate) fn keep_for_parent(self) -> PathBuf {
        self.root.keep()
    }
    pub(crate) fn adopt_supervised(root: PathBuf, spec: NativeDependencySpec) -> Self {
        Self {
            root: CaptureRoot::Supervised(OwnedCaptureDirectory { path: Some(root) }),
            spec,
        }
    }
    pub fn capture(spec: &NativeDependencySpec, parent: &Path) -> Result<Self> {
        let mut remaining = spec.validate()?;
        artifacts::private_directory(parent)?;
        let mut builder = tempfile::Builder::new();
        builder.prefix("native-dependencies-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let root = builder.tempdir_in(parent)?;
        let content = root.path().join("content");
        let libraries = root.path().join("libraries");
        artifacts::private_directory(&content)?;
        artifacts::private_directory(&libraries)?;
        for library in &spec.libraries {
            // Do not canonicalize/reopen a final symlink. Capture this descriptor.
            let mut input = artifacts::open_regular(&library.path)?;
            ensure!(
                input.metadata()?.len() == library.byte_len,
                "native dependency length mismatch"
            );
            remaining -= library.byte_len;
            let (snapshot, sha256, size) = artifacts::snapshot_opened_reserved(
                &mut input,
                &content,
                library.byte_len,
                remaining,
            )?;
            ensure!(
                sha256 == library.sha256 && size == library.byte_len,
                "native dependency content mismatch"
            );
            for name in &library.names {
                fs::hard_link(&snapshot, libraries.join(name))?;
            }
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            // Finalize the loader before publishing this image. A launch-time
            // chmod, even to the same mode, changes ctime and invalidates a
            // sibling worker's fresh mounted-byte proof.
            for library in &spec.libraries {
                if library
                    .names
                    .iter()
                    .any(|name| name == "ld-linux-x86-64.so.2")
                {
                    let loader = artifacts::open_regular(&content.join(&library.sha256))?;
                    loader.set_permissions(fs::Permissions::from_mode(0o500))?;
                    loader.sync_all()?;
                }
            }
        }
        artifacts::sync_dir(&libraries)?;
        artifacts::sync_dir(root.path())?;
        let captured = Self {
            root: CaptureRoot::Temporary(root),
            spec: spec.clone(),
        };
        captured.verify()?;
        Ok(captured)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn verify_loader_ready(&self) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        self.verify_metadata()?;
        let loader =
            artifacts::open_regular(&self.library_directory().join("ld-linux-x86-64.so.2"))?;
        ensure!(
            loader.metadata()?.permissions().mode() & 0o7777 == 0o500,
            "captured CUDA loader permissions differ"
        );
        Ok(())
    }

    pub fn library_directory(&self) -> PathBuf {
        self.root.path().join("libraries")
    }
    pub fn specification(&self) -> &NativeDependencySpec {
        &self.spec
    }

    /// Explicit cleanup reports errors; Drop alone cannot attest successful removal.
    pub fn close(self) -> Result<()> {
        self.root
            .close()
            .context("native dependency capture cleanup failed")
    }

    /// Rehash captured bytes and check every alias against the captured inode.
    /// Original host paths are never consulted after capture.
    pub fn verify(&self) -> Result<()> {
        self.verify_metadata()?;
        for library in &self.spec.libraries {
            let content = self.root.path().join("content").join(&library.sha256);
            let (sha256, size) = artifacts::digest_file(&content, library.byte_len)?;
            ensure!(
                sha256 == library.sha256 && size == library.byte_len,
                "captured native dependency changed"
            );
        }
        Ok(())
    }
    /// No content reads. Full hashing belongs in the supervised capture/worker.
    pub(crate) fn verify_metadata(&self) -> Result<()> {
        self.spec.validate()?;
        let directory = self.library_directory();
        let metadata = fs::symlink_metadata(&directory)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "captured library directory changed"
        );
        let expected: BTreeSet<&str> = self
            .spec
            .libraries
            .iter()
            .flat_map(|library| library.names.iter().map(String::as_str))
            .collect();
        let mut count = 0usize;
        for entry in fs::read_dir(&directory)? {
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("unexpected native library name"))?;
            ensure!(
                expected.contains(name.as_str()),
                "undeclared captured native dependency"
            );
            count += 1;
            ensure!(
                count <= expected.len(),
                "captured native dependency count changed"
            );
        }
        ensure!(
            count == expected.len(),
            "captured native dependency alias missing"
        );
        for library in &self.spec.libraries {
            let content = self.root.path().join("content").join(&library.sha256);
            let content_file = artifacts::open_regular(&content)?;
            let content_metadata = content_file.metadata()?;
            ensure!(
                content_metadata.len() == library.byte_len
                    && content_metadata.permissions().readonly(),
                "captured native dependency metadata changed"
            );
            for name in &library.names {
                let alias = artifacts::open_regular(&self.library_directory().join(name))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let expected = &content_metadata;
                    let actual = alias.metadata()?;
                    ensure!(
                        actual.dev() == expected.dev() && actual.ino() == expected.ino(),
                        "captured native dependency alias changed"
                    );
                }
                #[cfg(not(unix))]
                {
                    let _ = alias;
                    let (hash, len) = artifacts::digest_file(
                        &self.library_directory().join(name),
                        library.byte_len,
                    )?;
                    ensure!(
                        hash == library.sha256 && len == library.byte_len,
                        "captured native dependency alias changed"
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn fixture(root: &Path) -> NativeDependencySpec {
        let path = root.join("source");
        fs::write(&path, b"original native bytes").unwrap();
        NativeDependencySpec {
            schema_version: 1,
            libraries: vec![NativeLibrarySpec {
                path,
                sha256: format!("{:x}", Sha256::digest(b"original native bytes")),
                byte_len: 21,
                names: vec![
                    "libfixture.so".into(),
                    "libfixture.so.1".into(),
                    "libstdc++.so.6".into(),
                ],
            }],
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn loader_permissions_are_final_before_publication_and_never_repaired_on_use() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let mut spec = fixture(root.path());
        spec.libraries[0].names = vec!["ld-linux-x86-64.so.2".into()];
        let capture =
            CapturedNativeDependencies::capture(&spec, &root.path().join("capture")).unwrap();
        let path = capture.library_directory().join("ld-linux-x86-64.so.2");
        let before = fs::metadata(&path).unwrap();
        assert_eq!(before.permissions().mode() & 0o7777, 0o500);
        capture.verify_loader_ready().unwrap();
        capture.verify_loader_ready().unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(
            (before.ctime(), before.ctime_nsec()),
            (after.ctime(), after.ctime_nsec())
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(capture.verify_loader_ready().is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
            0o400
        );
    }
    #[test]
    fn captures_exact_bytes_independent_of_original_and_removes_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture(root.path());
        let parent = root.path().join("private");
        let c = CapturedNativeDependencies::capture(&spec, &parent).unwrap();
        fs::write(&spec.libraries[0].path, b"replaced native bytes").unwrap();
        c.verify().unwrap();
        let dir = c.library_directory();
        assert_eq!(
            fs::read(dir.join("libfixture.so.1")).unwrap(),
            b"original native bytes"
        );
        c.close().unwrap();
        assert!(!dir.exists());
        assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
    }
    #[test]
    fn refuses_digest_length_alias_and_bound_changes_without_partial_capture() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture(root.path());
        let parent = root.path().join("private");
        let mut bad = spec.clone();
        bad.libraries[0].sha256 = "0".repeat(64);
        assert!(CapturedNativeDependencies::capture(&bad, &parent).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        bad = spec.clone();
        bad.libraries[0].byte_len += 1;
        assert!(CapturedNativeDependencies::capture(&bad, &parent).is_err());
        bad = spec.clone();
        bad.libraries[0].names.push("../escape".into());
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.libraries[0].names.push("libfixture.so".into());
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.libraries[0].byte_len = pulsar_protocol::MAX_NATIVE_LIBRARY_BYTES + 1;
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.libraries = vec![spec.libraries[0].clone(); pulsar_protocol::MAX_NATIVE_LIBRARIES + 1];
        assert!(bad.validate().is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    }
    #[test]
    fn refuses_added_library_and_changed_captured_content() {
        let root = tempfile::tempdir().unwrap();
        let spec = fixture(root.path());
        let parent = root.path().join("private");
        let c = CapturedNativeDependencies::capture(&spec, &parent).unwrap();
        let extra = c.library_directory().join("undeclared.so");
        fs::write(&extra, b"extra").unwrap();
        assert!(c.verify().is_err());
        fs::remove_file(extra).unwrap();
        c.verify().unwrap();
        let file = c.library_directory().join("libfixture.so");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let mut permissions = fs::metadata(&file).unwrap().permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&file, permissions).unwrap();
        }
        fs::write(file, b"altered native bytes!").unwrap();
        assert!(c.verify().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_fifo_and_alias_replacement() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        let root = tempfile::tempdir().unwrap();
        let spec = fixture(root.path());
        let parent = root.path().join("private");
        let link = root.path().join("link");
        symlink(&spec.libraries[0].path, &link).unwrap();
        let mut bad = spec.clone();
        bad.libraries[0].path = link;
        assert!(CapturedNativeDependencies::capture(&bad, &parent).is_err());
        let fifo = root.path().join("fifo");
        let cpath = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        bad.libraries[0].path = fifo;
        assert!(CapturedNativeDependencies::capture(&bad, &parent).is_err());
        let c = CapturedNativeDependencies::capture(&spec, &parent).unwrap();
        let alias = c.library_directory().join("libfixture.so");
        fs::remove_file(&alias).unwrap();
        fs::write(alias, b"original native bytes").unwrap();
        assert!(c.verify().is_err());
    }
}
