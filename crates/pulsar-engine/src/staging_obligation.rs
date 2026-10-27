//! Identity-bound staging lifecycle. Publications synchronously commit ownership.
#[cfg(test)]
#[path = "staging_obligation_crash_tests.rs"]
mod crash_tests;
#[path = "staging_creation.rs"]
pub(crate) mod creation;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn of(file: &File) -> Result<Self> {
        let meta = file.metadata()?;
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "staging directory owner or privacy differs"
        );
        Ok(Self {
            device: meta.dev(),
            inode: meta.ino(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Phase {
    Intent,
    Bound(Identity),
    Retired(Identity),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    parent: PathBuf,
    parent_identity: Identity,
    name: String,
    phase: Phase,
}
/// Legacy untracked helper ownership or an engine-journaled creation.
/// Tracked production live inference uses Durable, with intent before creation.
pub(crate) enum Staging {
    Legacy(tempfile::TempDir),
    Retained {
        root: Box<Staging>,
        retention: Box<dyn Send>,
    },
    Durable(creation::Admitted),
}
impl From<tempfile::TempDir> for Staging {
    fn from(root: tempfile::TempDir) -> Self {
        Self::Legacy(root)
    }
}
impl Staging {
    pub(crate) fn retaining(self, retention: impl Send + 'static) -> Self {
        Self::Retained {
            root: Box::new(self),
            retention: Box::new(retention),
        }
    }
    pub(crate) fn path(&self) -> PathBuf {
        match self {
            Self::Legacy(root) => root.path().to_owned(),
            Self::Durable(root) => root.path(),
            Self::Retained { root, .. } => root.path(),
        }
    }
    pub(crate) fn cleanup_after_use_retired(
        self,
        retention: impl Send + 'static,
    ) -> crate::staging_cleanup::Cleanup {
        self.cleanup_boxed(Box::new(retention))
    }
    fn cleanup_boxed(self, retention: Box<dyn Send>) -> crate::staging_cleanup::Cleanup {
        match self {
            Self::Legacy(root) => {
                crate::staging_cleanup::Cleanup::start(vec![root.keep()], retention)
            }
            Self::Durable(root) => root.close_after_use_retired_with(retention),
            Self::Retained {
                root,
                retention: held,
            } => root.cleanup_boxed(Box::new((held, retention))),
        }
    }
}
impl Record {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn parent_path(&self) -> &Path {
        &self.parent
    }
    pub(crate) fn permits_successor(&self, next: &Self) -> bool {
        self.parent == next.parent
            && self.parent_identity == next.parent_identity
            && self.name == next.name
            && match (&self.phase, &next.phase) {
                (Phase::Intent, _) => true,
                (Phase::Bound(before), Phase::Bound(after) | Phase::Retired(after)) => {
                    before == after
                }
                (Phase::Retired(before), Phase::Retired(after)) => before == after,
                _ => false,
            }
    }
    pub(crate) fn is_intent(&self) -> bool {
        matches!(self.phase, Phase::Intent)
    }
    pub(crate) fn validate_clear(&self) -> Result<()> {
        ensure!(
            !matches!(self.phase, Phase::Bound(_)),
            "active staging cannot be cleared"
        );
        let held = parent(self)?;
        match child(&held, &self.name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                held.sync_all()?;
                Ok(())
            }
            Err(error) => Err(error.into()),
            Ok(_) => anyhow::bail!("staging deletion is not confirmed"),
        }
    }
    pub(crate) fn validate_shape(&self) -> Result<()> {
        ensure!(
            self.parent_identity.inode != 0,
            "invalid staging parent identity"
        );
        ensure!(
            self.parent.is_absolute()
                && self.parent.as_os_str().len() <= 4096
                && self.parent.components().all(|c| matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )),
            "invalid staging parent path"
        );
        let id = self
            .name
            .strip_prefix("pulsar-stage-")
            .ok_or_else(|| anyhow::anyhow!("invalid staging name"))?;
        ensure!(
            uuid::Uuid::parse_str(id)?.to_string() == id,
            "staging name is not canonical"
        );
        if let Phase::Bound(id) | Phase::Retired(id) = &self.phase {
            ensure!(id.inode != 0, "invalid staging root identity");
        }
        Ok(())
    }
}
/// Preflight without mutation. Call for the full roster before any recovery effect.
pub(crate) fn validate_recovery(record: &Record) -> Result<()> {
    record.validate_shape()?;
    let held = parent(record)?;
    match child(&held, &record.name) {
        Ok(root) => {
            let expected = match &record.phase {
                Phase::Intent => anyhow::bail!("created staging root has no acknowledged identity"),
                Phase::Bound(id) | Phase::Retired(id) => id,
            };
            ensure!(
                &Identity::of(&root)? == expected,
                "staging root identity differs"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !matches!(record.phase, Phase::Bound(_)),
                "bound staging root is missing"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn directory(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}
fn parent(record: &Record) -> Result<File> {
    ensure!(
        record.parent_identity.inode != 0,
        "invalid staging parent identity"
    );
    if let Phase::Bound(id) | Phase::Retired(id) = &record.phase {
        ensure!(id.inode != 0, "invalid staging root identity");
    }
    ensure!(
        record.parent.is_absolute()
            && record.parent.as_os_str().len() <= 4096
            && record.parent.canonicalize()? == record.parent,
        "staging parent is not canonical"
    );
    let id = record
        .name
        .strip_prefix("pulsar-stage-")
        .ok_or_else(|| anyhow::anyhow!("invalid staging name"))?;
    ensure!(
        uuid::Uuid::parse_str(id)?.to_string() == id,
        "staging name is not canonical"
    );
    let file = directory(&record.parent)?;
    ensure!(
        file.metadata()?.mode() & 0o077 == 0,
        "staging parent is not private"
    );
    ensure!(
        Identity::of(&file)? == record.parent_identity,
        "staging parent identity differs"
    );
    Ok(file)
}
fn child(parent: &File, name: &str) -> std::io::Result<File> {
    let name = std::ffi::CString::new(name).map_err(std::io::Error::other)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn check_locator(record: &Record, held_root: &File) -> Result<()> {
    let held_parent = parent(record)?;
    let located = child(&held_parent, &record.name)?;
    ensure!(
        Identity::of(&located)? == Identity::of(held_root)?,
        "staging locator identity differs"
    );
    Ok(())
}

/// A successful publication must be durable, not merely queued. An error may
/// mean a lost acknowledgement: never undo it or proceed to the next effect.
#[cfg(test)]
pub(crate) fn create(
    parent_path: &Path,
    publish: &mut impl FnMut(Option<&Record>) -> Result<()>,
) -> Result<Created> {
    let parent_path = parent_path.canonicalize()?;
    let held_parent = directory(&parent_path)?;
    let mut record = Record {
        parent: parent_path,
        parent_identity: Identity::of(&held_parent)?,
        name: format!("pulsar-stage-{}", uuid::Uuid::new_v4()),
        phase: Phase::Intent,
    };
    parent(&record)?;
    publish(Some(&record))?;
    // Publication may block or invoke a host adapter. Recheck the locator at
    // each acknowledged handoff instead of assuming its parent stayed bound.
    parent(&record)?;
    let name = std::ffi::CString::new(record.name.as_str())?;
    ensure!(
        unsafe { libc::mkdirat(held_parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
        "staging creation failed: {}",
        std::io::Error::last_os_error()
    );
    held_parent.sync_all()?;
    let root = child(&held_parent, &record.name)?;
    record.phase = Phase::Bound(Identity::of(&root)?);
    check_locator(&record, &root)?;
    publish(Some(&record))?;
    check_locator(&record, &root)?;
    // No path or directory handle escapes before the identity acknowledgement.
    Ok(Created {
        record,
        parent: held_parent,
        root,
    })
}
pub(crate) struct Created {
    record: Record,
    parent: File,
    root: File,
}
impl Created {
    pub(crate) fn path(&self) -> PathBuf {
        self.record.parent.join(&self.record.name)
    }

    /// Caller must independently establish that all users of these bytes have
    /// retired. A lease release or cancellation acknowledgement is insufficient.
    pub(crate) fn finish_after_use_retired(
        &mut self,
        publish: &mut impl FnMut(Option<&Record>) -> Result<()>,
    ) -> Result<()> {
        self.record.phase = Phase::Retired(Identity::of(&self.root)?);
        publish(Some(&self.record))?;
        self.remove()?;
        publish(None)
    }
    fn remove(&self) -> Result<()> {
        parent(&self.record)?;
        if self.root.metadata()?.nlink() != 0 {
            let current = child(&self.parent, &self.record.name)?;
            ensure!(
                Identity::of(&current)? == Identity::of(&self.root)?,
                "staging root identity differs"
            );
            // This prototype does not claim safety against every hostile same-UID
            // pathname race between identity observation and recursive deletion.
            fs::remove_dir_all(self.path())?;
            ensure!(
                self.root.metadata()?.nlink() == 0,
                "staging deletion unconfirmed"
            );
        }
        self.parent.sync_all()?;
        Ok(())
    }
}

/// Recover only after all prior byte users are independently confirmed retired.
/// An existing resource with intent alone is ambiguous and must be preserved.
pub(crate) fn reconcile_after_use_retired(
    record: &Record,
    publish: &mut impl FnMut(Option<&Record>) -> Result<()>,
) -> Result<()> {
    let held_parent = parent(record)?;
    let root = match child(&held_parent, &record.name) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !matches!(record.phase, Phase::Bound(_)),
                "bound staging root is missing"
            );
            held_parent.sync_all()?;
            return publish(None);
        }
        Err(error) => return Err(error.into()),
    };
    let expected = match &record.phase {
        Phase::Intent => anyhow::bail!("created staging root has no acknowledged identity"),
        Phase::Bound(id) | Phase::Retired(id) => id,
    };
    ensure!(
        &Identity::of(&root)? == expected,
        "staging root identity differs"
    );
    let mut owned = Created {
        record: record.clone(),
        parent: held_parent,
        root,
    };
    owned.finish_after_use_retired(publish)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    fn save(slot: &mut Option<Record>, record: Option<&Record>) -> Result<()> {
        *slot = record.cloned();
        Ok(())
    }
    #[test]
    fn changed_parent_during_intent_acknowledgement_cannot_authorize_creation_or_use() {
        let outer = fixture();
        let location = outer.path().join("parent");
        let moved = outer.path().join("original-parent");
        fs::create_dir(&location).unwrap();
        fs::set_permissions(&location, fs::Permissions::from_mode(0o700)).unwrap();
        let mut slot = None;
        let result = create(&location, &mut |record| {
            save(&mut slot, record)?;
            if matches!(record.unwrap().phase, Phase::Intent) {
                fs::rename(&location, &moved)?;
                fs::create_dir(&location)?;
                fs::set_permissions(&location, fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        });
        assert!(
            result.is_err(),
            "creation escaped with a locator in a replacement parent"
        );
        assert!(matches!(slot.unwrap().phase, Phase::Intent));
        assert_eq!(fs::read_dir(&location).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
    }
    #[test]
    fn creation_identity_and_retirement_publications_precede_effects() {
        let root = fixture();
        let mut slot = None;
        let mut observed = Vec::new();
        let mut created = create(root.path(), &mut |record| {
            let r = record.unwrap();
            observed.push((r.phase.clone(), r.parent.join(&r.name).exists()));
            save(&mut slot, record)
        })
        .unwrap();
        assert!(matches!(
            observed.as_slice(),
            [(Phase::Intent, false), (Phase::Bound(_), true)]
        ));
        let path = created.path();
        fs::write(path.join("bytes"), b"authored").unwrap();
        created
            .finish_after_use_retired(&mut |record| {
                match record {
                    Some(r) => {
                        assert!(matches!(r.phase, Phase::Retired(_)));
                        assert!(path.exists());
                    }
                    None => assert!(!path.exists()),
                }
                save(&mut slot, record)
            })
            .unwrap();
        assert!(slot.is_none());
    }
    #[test]
    fn lost_creation_acknowledgements_never_authorize_use_or_erase_intent() {
        for cut in 0..4 {
            let root = fixture();
            let mut slot = None;
            let mut calls = 0;
            let result = create(root.path(), &mut |record| {
                calls += 1;
                if cut == 0 {
                    anyhow::bail!("intent publication failed");
                }
                if cut == 2 && calls == 2 {
                    anyhow::bail!("identity not published");
                }
                save(&mut slot, record)?;
                if calls == 1 && cut == 1 {
                    anyhow::bail!("intent acknowledgement lost");
                }
                if calls == 2 && cut == 3 {
                    anyhow::bail!("identity acknowledgement lost");
                }
                Ok(())
            });
            assert!(result.is_err());
            if let Some(record) = slot.clone() {
                if cut == 1 || cut == 3 {
                    reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).unwrap();
                    assert!(slot.is_none());
                    assert!(!record.parent.join(&record.name).exists());
                } else {
                    assert!(
                        reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).is_err()
                    );
                    assert!(record.parent.join(&record.name).exists());
                    assert_eq!(slot.as_ref(), Some(&record));
                }
            } else {
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
            }
        }
    }
    #[test]
    fn root_replaced_during_identity_acknowledgement_never_escapes_as_created() {
        let root = fixture();
        let mut slot = None;
        let result = create(root.path(), &mut |record| {
            let r = record.unwrap();
            save(&mut slot, record)?;
            if matches!(r.phase, Phase::Bound(_)) {
                let path = r.parent.join(&r.name);
                fs::rename(&path, root.path().join("original"))?;
                fs::create_dir(&path)?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                fs::write(path.join("canary"), b"replacement")?;
            }
            Ok(())
        });
        assert!(result.is_err());
        let record = slot.clone().unwrap();
        assert!(reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).is_err());
        assert_eq!(
            fs::read(record.parent.join(&record.name).join("canary")).unwrap(),
            b"replacement"
        );
        assert!(root.path().join("original").is_dir());
    }
    #[test]
    fn failed_retirement_ack_or_deletion_retains_bytes_until_reconciliation() {
        for lost_ack in [true, false] {
            let root = fixture();
            let mut slot = None;
            let mut owned = create(root.path(), &mut |r| save(&mut slot, r)).unwrap();
            let path = owned.path();
            struct Restore(PathBuf);
            impl Drop for Restore {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
                }
            }
            let _restore = Restore(path.clone());
            fs::write(path.join("bytes"), b"retained bytes").unwrap();
            if !lost_ack {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
            }
            let result = owned.finish_after_use_retired(&mut |r| {
                save(&mut slot, r)?;
                if lost_ack {
                    anyhow::bail!("retirement acknowledgement lost");
                }
                Ok(())
            });
            assert!(result.is_err());
            assert_eq!(fs::read(path.join("bytes")).unwrap(), b"retained bytes");
            let record = slot.clone().unwrap();
            assert!(matches!(record.phase, Phase::Retired(_)));
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).unwrap();
            assert!(slot.is_none());
            assert!(!path.exists());
        }
    }
    #[test]
    fn bound_missing_refuses_but_deleted_retirement_tombstone_can_clear() {
        let root = fixture();
        let mut slot = None;
        let mut owned = create(root.path(), &mut |r| save(&mut slot, r)).unwrap();
        let bound = slot.clone().unwrap();
        let result = owned.finish_after_use_retired(&mut |r| {
            if r.is_none() {
                anyhow::bail!("clear acknowledgement lost");
            }
            save(&mut slot, r)
        });
        assert!(result.is_err());
        assert!(!owned.path().exists());
        assert!(reconcile_after_use_retired(&bound, &mut |r| save(&mut slot, r)).is_err());
        let retired = slot.clone().unwrap();
        reconcile_after_use_retired(&retired, &mut |r| save(&mut slot, r)).unwrap();
        assert!(slot.is_none());
    }
    #[test]
    fn replaced_root_and_parent_are_preserved_without_journal_clear() {
        let root = fixture();
        let mut slot = None;
        let owned = create(root.path(), &mut |r| save(&mut slot, r)).unwrap();
        let record = slot.clone().unwrap();
        let path = owned.path();
        let moved = root.path().join("original");
        fs::rename(&path, &moved).unwrap();
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(path.join("canary"), b"replacement").unwrap();
        assert!(reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).is_err());
        assert_eq!(fs::read(path.join("canary")).unwrap(), b"replacement");
        assert_eq!(slot.as_ref(), Some(&record));
        let outer = fixture();
        let parent_path = outer.path().join("parent");
        fs::rename(root.path(), &parent_path).unwrap();
        fs::create_dir(root.path()).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(reconcile_after_use_retired(&record, &mut |r| save(&mut slot, r)).is_err());
        assert_eq!(
            fs::read(parent_path.join(&record.name).join("canary")).unwrap(),
            b"replacement"
        );
    }
}
