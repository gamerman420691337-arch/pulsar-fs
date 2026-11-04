//! Admitted creation custody; successful byte users require explicit retirement.
use super::*;
use crate::staging_cleanup::Cleanup;
use std::sync::Arc;

/// Success means durable publication. Clear must match this exact obligation;
/// an error, including lost acknowledgement, leaves custody outstanding.
pub(crate) trait Journal: Send + Sync {
    fn store(&self, record: &Record) -> Result<()>;
    fn clear(&self, record: &Record) -> Result<()>;
}
struct Custody {
    record: Option<Record>,
    parent: Option<File>,
    root: Option<File>,
    journal: Arc<dyn Journal>,
    _claim: Box<dyn Send>,
    retirement_acked: bool,
}
impl Custody {
    fn retire(&mut self) -> Result<()> {
        let Some(record) = self.record.as_mut() else {
            return Ok(());
        };
        if let (Some(held_parent), Some(root)) = (&self.parent, &self.root) {
            record.phase = Phase::Retired(Identity::of(root)?);
            if !self.retirement_acked {
                self.journal.store(record)?;
                self.retirement_acked = true;
            }
            let owned = Created {
                record: record.clone(),
                parent: held_parent.try_clone()?,
                root: root.try_clone()?,
            };
            owned.remove()?;
            self.journal.clear(record)?;
        } else {
            // No identity was captured. Never infer ownership from the name.
            let expected = record.clone();
            reconcile_after_use_retired(record, &mut |next| match next {
                Some(next) => self.journal.store(next),
                None => self.journal.clear(&expected),
            })?;
        }
        self.record = None;
        Ok(())
    }
    fn cleanup(self) -> Cleanup {
        self.cleanup_with(())
    }
    fn cleanup_with(mut self, retention: impl Send + 'static) -> Cleanup {
        Cleanup::start_task(move || {
            let _keep_alive = &retention;
            self.retire().map_err(std::io::Error::other)
        })
    }
}
pub(crate) struct Admitted {
    custody: Option<Custody>,
    exposed: bool,
}
impl Admitted {
    pub(crate) fn create(
        parent_path: &Path,
        claim: impl Send + 'static,
        journal: Arc<dyn Journal>,
    ) -> Result<Self> {
        // Install ownership before even the first fallible host operation.
        let mut admitted = Self {
            custody: Some(Custody {
                record: None,
                parent: None,
                root: None,
                journal,
                _claim: Box::new(claim),
                retirement_acked: false,
            }),
            exposed: false,
        };
        let custody = admitted.custody.as_mut().unwrap();
        let path = parent_path.canonicalize()?;
        let held = directory(&path)?;
        let record = Record {
            parent: path,
            parent_identity: Identity::of(&held)?,
            name: format!("pulsar-stage-{}", uuid::Uuid::new_v4()),
            phase: Phase::Intent,
        };
        parent(&record)?;
        custody.parent = Some(held);
        custody.record = Some(record);
        let record = custody.record.as_mut().unwrap();
        custody.journal.store(record)?;
        parent(record)?;
        let held = custody.parent.as_ref().unwrap();
        let name = std::ffi::CString::new(record.name.as_str())?;
        ensure!(
            unsafe { libc::mkdirat(held.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
            "staging creation failed: {}",
            std::io::Error::last_os_error()
        );
        // Capture identity before another fallible sync/publication handoff.
        custody.root = Some(child(held, &record.name)?);
        let root = custody.root.as_ref().unwrap();
        record.phase = Phase::Bound(Identity::of(root)?);
        held.sync_all()?;
        check_locator(record, root)?;
        custody.journal.store(record)?;
        check_locator(record, root)?;
        admitted.exposed = true;
        Ok(admitted)
    }
    pub(crate) fn path(&self) -> PathBuf {
        let record = self.custody.as_ref().unwrap().record.as_ref().unwrap();
        record.parent.join(&record.name)
    }
    /// Requires independent confirmation that every byte user has retired.
    #[cfg(test)]
    pub(crate) fn close_after_use_retired(self) -> Cleanup {
        self.close_after_use_retired_with(())
    }
    pub(crate) fn close_after_use_retired_with(
        mut self,
        retention: impl Send + 'static,
    ) -> Cleanup {
        self.custody.take().unwrap().cleanup_with(retention)
    }
}
impl Drop for Admitted {
    fn drop(&mut self) {
        if let Some(custody) = self.custody.take() {
            if self.exposed {
                // A successful handle may have handed bytes to an OS worker.
                // Its Drop alone cannot prove retirement. Preserve custody.
                std::mem::forget(custody);
            } else {
                // No handle escaped; failed creation cannot have admitted users.
                drop(custody.cleanup());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourcePool;
    use std::os::unix::fs::PermissionsExt;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        },
        time::{Duration, Instant},
    };
    struct TestJournal {
        record: Mutex<Option<Record>>,
        fail_bound: bool,
        fail_intent: bool,
        publish_bound: bool,
        acknowledge_retirement: AtomicBool,
        retirement_seen: AtomicBool,
        acknowledge_clear: AtomicBool,
        clear_seen: AtomicBool,
        lose_clear_ack: AtomicBool,
    }
    impl Journal for TestJournal {
        fn store(&self, record: &Record) -> Result<()> {
            if matches!(record.phase, Phase::Retired(_)) {
                self.retirement_seen.store(true, Ordering::Release);
                ensure!(
                    self.acknowledge_retirement.load(Ordering::Acquire),
                    "retirement not acknowledged"
                );
            }
            if !matches!(record.phase, Phase::Bound(_)) || self.publish_bound {
                *self.record.lock().unwrap() = Some(record.clone());
            }
            if self.fail_intent && matches!(record.phase, Phase::Intent) {
                anyhow::bail!("lost intent acknowledgement");
            }
            if self.fail_bound && matches!(record.phase, Phase::Bound(_)) {
                fs::write(
                    record.parent.join(&record.name).join("authored"),
                    b"retained",
                )?;
                anyhow::bail!("lost identity acknowledgement");
            }
            Ok(())
        }
        fn clear(&self, record: &Record) -> Result<()> {
            ensure!(
                self.record
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_none_or(|current| current == record),
                "wrong obligation"
            );
            self.clear_seen.store(true, Ordering::Release);
            ensure!(
                self.acknowledge_clear.load(Ordering::Acquire),
                "clear not acknowledged"
            );
            *self.record.lock().unwrap() = None;
            ensure!(
                !self.lose_clear_ack.swap(false, Ordering::AcqRel),
                "lost clear acknowledgement"
            );
            Ok(())
        }
    }
    fn journal(fail_bound: bool) -> Arc<TestJournal> {
        Arc::new(TestJournal {
            record: Mutex::new(None),
            fail_bound,
            fail_intent: false,
            publish_bound: true,
            acknowledge_retirement: AtomicBool::new(!fail_bound),
            retirement_seen: AtomicBool::new(false),
            acknowledge_clear: AtomicBool::new(false),
            clear_seen: AtomicBool::new(false),
            lose_clear_ack: AtomicBool::new(false),
        })
    }
    fn wait(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "cleanup did not reach expected state"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn creation_ack_failure_must_retain_storage_until_cleanup_ack() {
        failed_bound(true);
        failed_bound(false);
    }
    fn failed_bound(published: bool) {
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let mut journal = journal(true);
        Arc::get_mut(&mut journal).unwrap().publish_bound = published;
        let claim = pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        assert!(Admitted::create(parent.path(), claim, journal.clone()).is_err());
        wait(|| journal.retirement_seen.load(Ordering::Acquire));
        let pending = journal.record.lock().unwrap().clone().unwrap();
        assert!(pending.parent.join(&pending.name).join("authored").exists());
        assert_eq!(pool.reserved_storage(), 4096);
        journal
            .acknowledge_retirement
            .store(true, Ordering::Release);
        wait(|| journal.clear_seen.load(Ordering::Acquire));
        assert_eq!(pool.reserved_storage(), 4096);
        assert!(pool.reserve(1024).is_err());
        let record = journal.record.lock().unwrap().clone().unwrap();
        assert!(matches!(record.phase, Phase::Retired(_)));
        assert!(!record.parent.join(&record.name).exists());
        journal.acknowledge_clear.store(true, Ordering::Release);
        wait(|| pool.reserved_storage() == 0);
        assert!(journal.record.lock().unwrap().is_none());
        assert!(pool.reserve(1024).is_ok());
    }
    #[test]
    fn lost_intent_ack_holds_claim_until_absence_and_clear_ack() {
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let mut journal = journal(false);
        Arc::get_mut(&mut journal).unwrap().fail_intent = true;
        let claim = pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        assert!(Admitted::create(parent.path(), claim, journal.clone()).is_err());
        wait(|| journal.clear_seen.load(Ordering::Acquire));
        let record = journal.record.lock().unwrap().clone().unwrap();
        assert!(matches!(record.phase, Phase::Intent));
        assert!(!record.parent.join(&record.name).exists());
        assert_eq!(pool.reserved_storage(), 4096);
        journal.acknowledge_clear.store(true, Ordering::Release);
        wait(|| pool.reserved_storage() == 0);
        assert!(journal.record.lock().unwrap().is_none());
    }

    #[test]
    fn changed_root_or_parent_preserves_canary_and_claim_until_restored() {
        for replace_parent in [false, true] {
            let outer = tempfile::tempdir().unwrap();
            fs::set_permissions(outer.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let parent = outer.path().join("parent");
            fs::create_dir(&parent).unwrap();
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
            let pool = ResourcePool::new(1024, 1);
            let journal = journal(false);
            journal.acknowledge_clear.store(true, Ordering::Release);
            let claim = pool
                .reserve(1024)
                .unwrap()
                .with_storage(4096, 32 * 1024 * 1024)
                .unwrap();
            let admitted = Admitted::create(&parent, claim, journal.clone()).unwrap();
            let path = admitted.path();
            fs::write(path.join("original"), b"original bytes").unwrap();
            let location = if replace_parent {
                parent.clone()
            } else {
                path.clone()
            };
            let moved = outer.path().join("original");
            let canary = outer.path().join("replacement");
            fs::rename(&location, &moved).unwrap();
            fs::create_dir(&location).unwrap();
            fs::set_permissions(&location, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(location.join("canary"), b"unrelated bytes").unwrap();
            let cleanup = admitted.close_after_use_retired();
            wait(|| journal.retirement_seen.load(Ordering::Acquire));
            assert!(cleanup
                .wait(Instant::now() + Duration::from_millis(120))
                .is_err());
            assert_eq!(pool.reserved_storage(), 4096);
            assert_eq!(
                fs::read(location.join("canary")).unwrap(),
                b"unrelated bytes"
            );
            fs::rename(&location, &canary).unwrap();
            fs::rename(&moved, &location).unwrap();
            cleanup
                .wait(Instant::now() + Duration::from_secs(3))
                .unwrap();
            assert!(!path.exists());
            assert_eq!(fs::read(canary.join("canary")).unwrap(), b"unrelated bytes");
            assert_eq!(pool.reserved_storage(), 0);
        }
    }

    #[test]
    fn failed_deletion_and_lost_clear_ack_retain_claim_until_retry() {
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let journal = journal(false);
        journal.acknowledge_clear.store(true, Ordering::Release);
        journal.lose_clear_ack.store(true, Ordering::Release);
        let claim = pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        let admitted = Admitted::create(parent.path(), claim, journal.clone()).unwrap();
        let path = admitted.path();
        fs::write(path.join("authored"), b"retained bytes").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
        let cleanup = admitted.close_after_use_retired();
        wait(|| journal.retirement_seen.load(Ordering::Acquire));
        assert!(cleanup
            .wait(Instant::now() + Duration::from_millis(120))
            .is_err());
        assert_eq!(pool.reserved_storage(), 4096);
        assert_eq!(fs::read(path.join("authored")).unwrap(), b"retained bytes");
        assert!(!journal.clear_seen.load(Ordering::Acquire));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        cleanup
            .wait(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert!(!path.exists());
        assert!(!journal.lose_clear_ack.load(Ordering::Acquire));
        assert!(journal.record.lock().unwrap().is_none());
        assert_eq!(pool.reserved_storage(), 0);
    }

    #[test]
    fn explicit_retirement_retains_claim_until_clear_acknowledgement() {
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let journal = journal(false);
        let claim = pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        let admitted = Admitted::create(parent.path(), claim, journal.clone()).unwrap();
        let path = admitted.path();
        fs::write(path.join("authored"), b"test bytes").unwrap();
        assert_eq!(pool.reserved_storage(), 4096);
        let cleanup = admitted.close_after_use_retired();
        wait(|| journal.clear_seen.load(Ordering::Acquire));
        assert!(!path.exists());
        assert!(!cleanup.is_complete());
        assert_eq!(pool.reserved_storage(), 4096);
        journal.acknowledge_clear.store(true, Ordering::Release);
        cleanup
            .wait(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert_eq!(pool.reserved_storage(), 0);
    }
}
