//! Fresh byte proof shared only by aliases of the same unchanged inode in one pass.
#![cfg(target_os = "linux")]
use crate::native_mounts::MountedNativeAlias;
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, fs::Metadata, io::Read, os::unix::fs::MetadataExt, path::Path,
    time::Instant,
};

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Stamp {
    fn new(m: &Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            bytes: m.len(),
            mode: m.mode(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        }
    }
}
struct Proof {
    stamp: Stamp,
    digest: String,
}

/// Authenticate every mounted alias, hashing identical inodes once in this call.
/// No proof survives the call or substitutes for a later bootstrap verification.
pub(crate) fn verify(
    directory: &Path,
    aliases: &[MountedNativeAlias],
    deadline: Instant,
) -> Result<u64> {
    verify_inner(directory, aliases, deadline, || {})
}
fn verify_inner(
    directory: &Path,
    aliases: &[MountedNativeAlias],
    deadline: Instant,
    mut after_hash: impl FnMut(),
) -> Result<u64> {
    crate::native_mounts::verify_mounted(directory, aliases)?;
    let mut proofs: BTreeMap<(u64, u64), Proof> = BTreeMap::new();
    let mut buffer = [0u8; 65536];
    let mut hashed_bytes = 0u64;
    for alias in aliases {
        ensure!(
            Instant::now() < deadline,
            "mount checker hash deadline exceeded"
        );
        let mut file = crate::artifacts::open_regular(&directory.join(&alias.name))?;
        let stamp = Stamp::new(&file.metadata()?);
        ensure!(
            stamp.device == alias.filesystem_device
                && stamp.inode == alias.inode
                && stamp.bytes == alias.byte_len
                && stamp.mode & 0o222 == 0,
            "mounted alias differs before byte verification"
        );
        let key = (stamp.device, stamp.inode);
        if let Some(proof) = proofs.get(&key) {
            ensure!(
                proof.stamp == stamp && proof.digest == alias.sha256,
                "mounted alias shared byte proof differs"
            );
            continue;
        }
        let mut hash = Sha256::new();
        let mut count = 0u64;
        loop {
            ensure!(
                Instant::now() < deadline,
                "mount checker hash deadline exceeded"
            );
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            count = count
                .checked_add(read as u64)
                .context("mounted alias size overflow")?;
            ensure!(count <= alias.byte_len, "mounted alias grew");
            hash.update(&buffer[..read]);
        }
        let digest = format!("{:x}", hash.finalize());
        ensure!(
            count == alias.byte_len && digest == alias.sha256,
            "mounted native alias hash differs"
        );
        ensure!(
            Stamp::new(&file.metadata()?) == stamp,
            "mounted alias changed during byte verification"
        );
        hashed_bytes = hashed_bytes
            .checked_add(count)
            .context("mounted proof byte count overflow")?;
        proofs.insert(key, Proof { stamp, digest });
        after_hash();
    }
    // Re-open every name: replacement or a same-inode mutation after its hash
    // invalidates the whole pass, including proofs already used by other aliases.
    for alias in aliases {
        ensure!(
            Instant::now() < deadline,
            "mount checker hash deadline exceeded"
        );
        let file = crate::artifacts::open_regular(&directory.join(&alias.name))?;
        let stamp = Stamp::new(&file.metadata()?);
        let proof = proofs
            .get(&(alias.filesystem_device, alias.inode))
            .context("mounted proof absent")?;
        ensure!(
            stamp == proof.stamp && alias.sha256 == proof.digest,
            "mounted alias changed after byte verification"
        );
    }
    crate::native_mounts::verify_mounted(directory, aliases)?;
    ensure!(
        Instant::now() < deadline,
        "mount checker hash deadline exceeded"
    );
    Ok(hashed_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
    fn fixture(hard_link: bool) -> (tempfile::TempDir, Vec<MountedNativeAlias>) {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("lib-first.so");
        let b = root.path().join("lib-second.so");
        fs::write(&a, b"authored mounted bytes").unwrap();
        if hard_link {
            fs::hard_link(&a, &b).unwrap();
        } else {
            fs::copy(&a, &b).unwrap();
        }
        let mut aliases = Vec::new();
        for p in [&a, &b] {
            fs::set_permissions(p, fs::Permissions::from_mode(0o444)).unwrap();
            let m = fs::metadata(p).unwrap();
            aliases.push(MountedNativeAlias {
                name: p.file_name().unwrap().to_str().unwrap().into(),
                sha256: format!("{:x}", Sha256::digest(fs::read(p).unwrap())),
                byte_len: m.len(),
                filesystem_device: m.dev(),
                inode: m.ino(),
            });
        }
        (root, aliases)
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(5)
    }
    #[test]
    fn preparing_a_shared_loader_preserves_an_active_byte_proof() {
        use crate::native_dependencies::{
            CapturedNativeDependencies, NativeDependencySpec, NativeLibrarySpec,
        };
        let root = tempfile::tempdir().unwrap();
        let supplier = root.path().join("supplier");
        fs::write(&supplier, b"authored loader bytes").unwrap();
        let spec = NativeDependencySpec {
            schema_version: 1,
            libraries: vec![NativeLibrarySpec {
                path: supplier,
                sha256: format!("{:x}", Sha256::digest(b"authored loader bytes")),
                byte_len: 21,
                names: vec!["ld-linux-x86-64.so.2".into()],
            }],
        };
        let capture =
            CapturedNativeDependencies::capture(&spec, &root.path().join("capture")).unwrap();
        capture.verify_loader_ready().unwrap();
        let m = fs::metadata(capture.library_directory().join("ld-linux-x86-64.so.2")).unwrap();
        let aliases = vec![MountedNativeAlias {
            name: "ld-linux-x86-64.so.2".into(),
            sha256: spec.libraries[0].sha256.clone(),
            byte_len: 21,
            filesystem_device: m.dev(),
            inode: m.ino(),
        }];
        assert_eq!(
            verify_inner(&capture.library_directory(), &aliases, deadline(), || {
                // Give coarse timestamp filesystems a distinct ctime for the
                // sibling preparation cut. No mutation is allowed after capture.
                std::thread::sleep(Duration::from_millis(20));
                capture.verify_loader_ready().unwrap();
            })
            .unwrap(),
            21
        );
    }

    #[test]
    fn identical_inode_aliases_share_one_fresh_hash_per_call() {
        let (root, aliases) = fixture(true);
        assert_eq!(
            verify(root.path(), &aliases, deadline()).unwrap(),
            aliases[0].byte_len
        );
        assert_eq!(
            verify(root.path(), &aliases, deadline()).unwrap(),
            aliases[0].byte_len
        );
        let (root, aliases) = fixture(false);
        assert_eq!(
            verify(root.path(), &aliases, deadline()).unwrap(),
            2 * aliases[0].byte_len
        );
    }
    #[test]
    fn conflicting_digest_or_identity_cannot_reuse_proof() {
        let (root, mut aliases) = fixture(true);
        aliases[1].sha256 = "0".repeat(64);
        assert!(verify(root.path(), &aliases, deadline()).is_err());
        aliases[1].sha256 = aliases[0].sha256.clone();
        aliases[1].inode += 1;
        assert!(verify(root.path(), &aliases, deadline()).is_err());
    }
    #[test]
    fn same_length_mutation_after_hash_invalidates_alias_proof() {
        let (root, aliases) = fixture(true);
        assert!(verify_inner(root.path(), &aliases, deadline(), || {
            let p = root.path().join(&aliases[0].name);
            fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
            fs::write(&p, vec![b'x'; aliases[0].byte_len as usize]).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        })
        .is_err());
        assert!(verify(root.path(), &aliases, deadline()).is_err());
    }
    #[test]
    fn replacement_symlink_and_expired_pass_are_refused() {
        let (root, aliases) = fixture(true);
        assert!(verify(root.path(), &aliases, Instant::now()).is_err());
        assert!(verify_inner(root.path(), &aliases, deadline(), || {
            let p = root.path().join(&aliases[1].name);
            fs::remove_file(&p).unwrap();
            std::os::unix::fs::symlink(&aliases[0].name, &p).unwrap();
        })
        .is_err());
    }
    #[test]
    fn mutation_of_an_earlier_inode_is_rejected_by_final_recheck() {
        let (root, aliases) = fixture(false);
        let mut hashes = 0;
        assert!(verify_inner(root.path(), &aliases, deadline(), || {
            hashes += 1;
            if hashes == 2 {
                let p = root.path().join(&aliases[0].name);
                fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
                fs::write(&p, vec![b'x'; aliases[0].byte_len as usize]).unwrap();
                fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
            }
        })
        .is_err());
    }
    #[test]
    fn sharing_a_hash_does_not_extend_the_deadline() {
        let (root, aliases) = fixture(true);
        assert!(verify_inner(
            root.path(),
            &aliases,
            Instant::now() + Duration::from_millis(50),
            || {
                std::thread::sleep(Duration::from_millis(60));
            }
        )
        .is_err());
    }
}
