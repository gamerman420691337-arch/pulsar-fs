//! Durable cgroup observations plus a CLOEXEC lease for the fork-to-exec gap.
//! Engine admission binds this lease scope before tracked worker dispatch.
#![cfg(target_os = "linux")]
#[path = "worker_fence_creation.rs"]
mod creation;
use crate::{
    artifacts,
    cgroup::AttemptCgroup,
    staging_obligation::{self, Record},
};
use anyhow::{ensure, Context, Result};
pub(crate) use creation::admitted::Admitted as AdmittedGroup;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::Child,
    sync::Mutex,
};
const LIMIT: u64 = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Group {
    path: PathBuf,
    device: u64,
    inode: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    scope: String,
    lease_device: u64,
    lease_inode: u64,
    groups: BTreeMap<String, Group>,
    retired: BTreeMap<String, Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    staging: Option<BTreeMap<String, Record>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    creation: Option<creation::Roster>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyLedger {
    version: u32,
    scope: String,
    lease_device: u64,
    lease_inode: u64,
    groups: BTreeMap<String, Group>,
}
#[derive(Debug)]
pub(crate) struct WorkerFence {
    directory: PathBuf,
    lease: File,
    ledger: Mutex<Ledger>,
}

fn group_directory(path: &Path) -> Result<(File, Group)> {
    ensure!(
        path.is_absolute() && path.starts_with("/sys/fs/cgroup") && path.as_os_str().len() <= 4096,
        "invalid worker fence cgroup path"
    );
    ensure!(
        path.canonicalize()? == path,
        "worker fence cgroup path is not canonical"
    );
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let meta = dir.metadata()?;
    ensure!(
        meta.is_dir() && meta.uid() == unsafe { libc::geteuid() },
        "worker fence cgroup ownership differs"
    );
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    ensure!(
        unsafe { libc::fstatfs(dir.as_raw_fd(), stat.as_mut_ptr()) } == 0
            && unsafe { stat.assume_init() }.f_type == 0x63677270,
        "worker fence requires cgroup v2"
    );
    Ok((
        dir,
        Group {
            path: path.into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
    ))
}
fn group_identity(path: &Path) -> Result<Group> {
    Ok(group_directory(path)?.1)
}
fn empty(group: &Group) -> Result<bool> {
    let (directory, identity) = group_directory(&group.path)?;
    ensure!(identity == *group, "worker fence cgroup identity differs");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c"cgroup.events".as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    ensure!(fd >= 0, "cannot open held cgroup population evidence");
    use std::os::fd::FromRawFd;
    let mut file = unsafe { File::from_raw_fd(fd) };
    ensure!(
        file.metadata()?.is_file(),
        "cgroup population evidence is not regular"
    );
    ensure!(
        file.metadata()?.len() <= 4096,
        "oversized cgroup population evidence"
    );
    let mut text = String::new();
    Read::by_ref(&mut file)
        .take(4097)
        .read_to_string(&mut text)?;
    ensure!(text.len() <= 4096, "oversized cgroup population evidence");
    match text
        .lines()
        .find_map(|line| line.strip_prefix("populated "))
    {
        Some("0") => Ok(true),
        Some("1") => Ok(false),
        _ => anyhow::bail!("missing worker fence population evidence"),
    }
}
fn validate(ledger: &Ledger) -> Result<()> {
    ensure!(
        matches!(ledger.version, 2..=4)
            && uuid::Uuid::parse_str(&ledger.scope)?.to_string() == ledger.scope,
        "unknown worker fence ledger identity"
    );
    ensure!(
        (ledger.version >= 3) == ledger.staging.is_some(),
        "staging roster missing or incompatible with ledger version"
    );
    ensure!(
        (ledger.version == 4) == ledger.creation.is_some(),
        "creation roster missing or incompatible with ledger version"
    );
    if let Some(roster) = &ledger.creation {
        roster.validate(ledger)?;
    }
    for (name, record) in ledger.staging.iter().flat_map(|records| records.iter()) {
        record.validate_shape()?;
        ensure!(name == record.name(), "staging record name differs");
    }
    ensure!(
        ledger
            .groups
            .len()
            .checked_add(ledger.retired.len())
            .and_then(|count| count.checked_add(ledger.staging.as_ref().map_or(0, BTreeMap::len)))
            .and_then(|count| count
                .checked_add(ledger.creation.as_ref().map_or(0, creation::Roster::len)))
            .is_some_and(|v| v <= 4096),
        "worker fence record capacity exceeded"
    );
    ensure!(
        ledger
            .groups
            .keys()
            .all(|name| !ledger.retired.contains_key(name)),
        "worker fence active/retired overlap"
    );
    for (name, group) in ledger.groups.iter().chain(&ledger.retired) {
        ensure!(
            !name.is_empty()
                && name.len() <= 160
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "invalid worker fence group name"
        );
        ensure!(
            group.path.file_name().and_then(|v| v.to_str()) == Some(name)
                && name.starts_with("pulsar-worker-"),
            "worker fence name/path mismatch"
        );
        ensure!(
            group.inode != 0
                && group.path.is_absolute()
                && group.path.starts_with("/sys/fs/cgroup")
                && group.path.as_os_str().len() <= 4096,
            "invalid worker fence group identity"
        );
    }
    Ok(())
}
fn validate_namespace(directory: &Path, ledger: &Ledger, scan: bool) -> Result<()> {
    if scan {
        if let Some(roster) = &ledger.creation {
            roster.namespace(ledger)?;
        }
    }
    if let Some(records) = &ledger.staging {
        ensure!(
            records
                .values()
                .all(|record| record.parent_path() == directory),
            "staging record escapes ownership directory"
        );
        if scan {
            // Inspect only the engine-owned namespace, never shared temporary roots.
            for (index, entry) in fs::read_dir(directory)?.enumerate() {
                ensure!(index < 8192, "ownership directory scan bound exceeded");
                let entry = entry?;
                if entry
                    .file_name()
                    .as_encoded_bytes()
                    .starts_with(b"pulsar-stage-")
                {
                    let name = entry.file_name();
                    ensure!(
                        name.to_str().is_some_and(|name| records.contains_key(name)),
                        "unrecorded staging resource; admission refused"
                    );
                }
            }
        }
    }
    Ok(())
}
fn save(directory: &Path, ledger: &Ledger) -> Result<()> {
    validate(ledger)?;
    validate_namespace(directory, ledger, false)?;
    let bytes = serde_json::to_vec(ledger)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "worker fence ledger exceeds bound"
    );
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(directory.join("worker-fence.json"))?;
    artifacts::sync_dir(directory)?;
    Ok(())
}
fn load(directory: &Path) -> Result<Option<Ledger>> {
    let path = directory.join("worker-fence.json");
    let mut file = match artifacts::open_regular(&path) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None)
        }
        Err(error) => return Err(error),
    };
    let meta = file.metadata()?;
    ensure!(
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0 && meta.len() <= LIMIT,
        "worker fence ledger is not a bounded private file"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= LIMIT,
        "worker fence ledger grew beyond bound"
    );
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let version: Version = serde_json::from_slice(&bytes)?;
    let (ledger, canonical) = match version.version {
        1 => {
            let prior: LegacyLedger = serde_json::from_slice(&bytes)?;
            let canonical = serde_json::to_vec(&prior)?;
            (
                Ledger {
                    version: 2,
                    scope: prior.scope,
                    lease_device: prior.lease_device,
                    lease_inode: prior.lease_inode,
                    groups: prior.groups,
                    retired: BTreeMap::new(),
                    staging: None,
                    creation: None,
                },
                canonical,
            )
        }
        2..=4 => {
            let ledger: Ledger = serde_json::from_slice(&bytes)?;
            let canonical = serde_json::to_vec(&ledger)?;
            (ledger, canonical)
        }
        _ => anyhow::bail!("unknown worker fence ledger version"),
    };
    validate(&ledger)?;
    ensure!(canonical == bytes, "worker fence ledger is not canonical");
    Ok(Some(ledger))
}
impl WorkerFence {
    #[cfg(test)]
    pub(crate) fn open(directory: &Path) -> Result<Self> {
        Self::open_bound(directory, None)
    }
    #[cfg(test)]
    pub(crate) fn open_bound(directory: &Path, expected_scope: Option<&str>) -> Result<Self> {
        Self::open_bound_required(directory, expected_scope, 2)
    }
    pub(crate) fn open_bound_required(
        directory: &Path,
        expected_scope: Option<&str>,
        minimum_version: u32,
    ) -> Result<Self> {
        ensure!(
            matches!(minimum_version, 2..=4),
            "unknown required worker fence ledger version"
        );
        ensure!(
            minimum_version == 2 || expected_scope.is_some(),
            "staging requirement needs a bound worker scope"
        );
        artifacts::private_directory(directory)?;
        let directory = directory.canonicalize()?;
        let path = directory.join("worker-fence.lock");
        if expected_scope.is_some() {
            ensure!(
                path.try_exists()? && directory.join("worker-fence.json").try_exists()?,
                "bound worker fence files are missing; no fresh ownership scope permitted"
            );
        }
        // Open read-only after creation: the inherited lease conveys no host write FD.
        let created = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => {
                file.sync_all()?;
                artifacts::sync_dir(&directory)?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        let lease = artifacts::open_regular(&path)?;
        let meta = lease.metadata()?;
        ensure!(
            meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0 && meta.len() == 0,
            "invalid worker fence lease file"
        );
        lease
            .try_lock_exclusive()
            .context("worker fence lease is still held")?;
        let mut ledger = match load(&directory)? {
            Some(ledger) => ledger,
            None => {
                ensure!(
                    created,
                    "worker fence ledger is missing for an existing lease"
                );
                Ledger {
                    version: 2,
                    scope: uuid::Uuid::new_v4().to_string(),
                    lease_device: meta.dev(),
                    lease_inode: meta.ino(),
                    groups: BTreeMap::new(),
                    retired: BTreeMap::new(),
                    staging: None,
                    creation: None,
                }
            }
        };
        ensure!(
            ledger.lease_device == meta.dev() && ledger.lease_inode == meta.ino(),
            "worker fence lease identity changed"
        );
        if let Some(expected) = expected_scope {
            ensure!(
                ledger.scope == expected,
                "worker fence scope differs from database binding"
            );
        }
        ensure!(ledger.version >= minimum_version,
            "worker fence ledger is below its database requirement; staging ownership missing or downgraded");
        // Validate the entire roster before any cleanup. Never signal a group here.
        for group in ledger.groups.values() {
            ensure!(
                empty(group)?,
                "prior worker tree is still populated; admission refused"
            );
        }
        for group in ledger.retired.values() {
            crate::cgroup::check_recorded_empty(&group.path, group.device, group.inode, true)?;
        }
        validate_namespace(&directory, &ledger, true)?;
        for record in ledger.staging.iter().flat_map(|records| records.values()) {
            staging_obligation::validate_recovery(record)?;
        }
        if let Some(roster) = &ledger.creation {
            roster.preflight()?;
        }
        // Commit retirement identities before deletion. Missing active records
        // still refuse; only these durable tombstones may recover missing paths.
        ledger.retired.append(&mut ledger.groups);
        save(&directory, &ledger)?;
        for group in ledger.retired.values() {
            crate::cgroup::reclaim_recorded(&group.path, group.device, group.inode, true)?;
        }
        ledger.retired.clear();
        save(&directory, &ledger)?;
        creation::recover(&directory, &mut ledger)?;
        // Every cgroup user is independently empty before staged bytes are touched.
        let records: Vec<_> = ledger
            .staging
            .iter()
            .flat_map(|r| r.values())
            .cloned()
            .collect();
        for record in records {
            let expected = record.clone();
            staging_obligation::reconcile_after_use_retired(&record, &mut |next| {
                let mut updated = ledger.clone();
                let roster = updated.staging.as_mut().context("staging roster missing")?;
                match next {
                    Some(next) => {
                        roster.insert(next.name().to_owned(), next.clone());
                    }
                    None => {
                        ensure!(
                            roster.get(expected.name()).is_some(),
                            "staging obligation missing during recovery"
                        );
                        roster.remove(expected.name());
                    }
                }
                save(&directory, &updated)?;
                ledger = updated;
                Ok(())
            })?;
        }
        Ok(Self {
            directory,
            lease,
            ledger: Mutex::new(ledger),
        })
    }
    /// Publish format 3 before the database minimum requirement and before creation.
    /// The caller must commit schema 19 before exposing the creator to an engine job.
    pub(crate) fn enable_staging(&self) -> Result<()> {
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        if ledger.version >= 3 {
            return Ok(());
        }
        let mut next = ledger.clone();
        next.version = 3;
        next.staging = Some(BTreeMap::new());
        validate_namespace(&self.directory, &next, true)?;
        save(&self.directory, &next)?;
        *ledger = next;
        Ok(())
    }
    pub(crate) fn staging_parent(&self) -> &Path {
        &self.directory
    }
    pub(crate) fn create_staging(
        self: &std::sync::Arc<Self>,
        claim: impl Send + 'static,
    ) -> Result<staging_obligation::creation::Admitted> {
        ensure!(self.version()? >= 3, "durable staging requires format 3");
        staging_obligation::creation::Admitted::create(&self.directory, claim, self.clone())
    }
    pub(crate) fn scope(&self) -> Result<String> {
        Ok(self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?
            .scope
            .clone())
    }
    pub(crate) fn version(&self) -> Result<u32> {
        Ok(self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?
            .version)
    }
    /// Keep this object alive through fork. CLOEXEC must remain set: the child
    /// holds the lease until exec, after its pre_exec cgroup placement syscall.
    pub(crate) fn lease_fd(&self) -> Result<std::os::fd::RawFd> {
        let fd = self.lease.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        ensure!(
            flags >= 0 && flags & libc::FD_CLOEXEC != 0,
            "worker fence lease must be CLOEXEC"
        );
        Ok(fd)
    }
    /// Conservative cleanup fence: a detached reaper or creation obligation
    /// still owns bytes even when the initiating operation's thread has ended.
    pub(crate) fn has_pending_worker_custody(&self) -> Result<bool> {
        let ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        Ok(!ledger.groups.is_empty()
            || !ledger.retired.is_empty()
            || ledger
                .creation
                .as_ref()
                .is_some_and(|roster| roster.has_pending_creation()))
    }
    pub(crate) fn has_pending_storage_custody(&self) -> Result<bool> {
        let ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        Ok(!ledger.groups.is_empty()
            || !ledger.retired.is_empty()
            || ledger
                .staging
                .as_ref()
                .is_some_and(|records| !records.is_empty())
            || ledger
                .creation
                .as_ref()
                .is_some_and(|roster| roster.has_pending_creation()))
    }
    pub(crate) fn register(&self, group: &AttemptCgroup) -> Result<()> {
        let identity = group_identity(group.path())?;
        let held_identity = group.identity()?;
        ensure!(
            (identity.device, identity.inode) == held_identity,
            "worker fence registration differs from held cgroup identity"
        );
        let name = identity
            .path
            .file_name()
            .context("missing cgroup name")?
            .to_str()
            .context("non-UTF8 cgroup name")?
            .to_string();
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        if ledger.version == 4 {
            ensure!(
                name.starts_with(&format!("pulsar-worker-{}-", ledger.scope))
                    && ledger.groups.get(&name) == Some(&identity),
                "format-4 registration requires completed creation transfer"
            );
            return save(&self.directory, &ledger);
        }
        ensure!(
            !ledger.groups.contains_key(&name) && !ledger.retired.contains_key(&name),
            "worker fence group is already registered"
        );
        ledger.groups.insert(name, identity);
        // Leave the in-memory record on IO failure: callers must retain custody
        // until an explicit retired-child clear succeeds, rather than spawn.
        save(&self.directory, &ledger)
    }
    pub(crate) fn retire(&self, child: &mut Child, group: &AttemptCgroup) -> Result<()> {
        ensure!(
            child.try_wait()?.is_some(),
            "direct worker child has not exited"
        );
        self.clear(group, false)
    }
    /// Only pre-spawn custody may use this path. A failed registration can have
    /// inserted an in-memory record even when durable replacement failed.
    pub(crate) fn retire_unspawned(&self, group: &AttemptCgroup) -> Result<()> {
        self.clear(group, true)
    }
    fn mark_retired(&self, ledger: &mut Ledger, name: &str, identity: Group) -> Result<()> {
        if !ledger.retired.contains_key(name) {
            let mut next = ledger.clone();
            next.groups.remove(name);
            next.retired.insert(name.to_owned(), identity);
            save(&self.directory, &next)?;
            *ledger = next;
        }
        Ok(())
    }
    fn clear(&self, group: &AttemptCgroup, allow_absent: bool) -> Result<()> {
        let (device, inode) = group.identity()?;
        let identity = Group {
            path: group.path().into(),
            device,
            inode,
        };
        let name = identity
            .path
            .file_name()
            .context("missing cgroup name")?
            .to_str()
            .context("non-UTF8 cgroup name")?
            .to_owned();
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        match ledger
            .groups
            .get(&name)
            .or_else(|| ledger.retired.get(&name))
        {
            Some(record) => ensure!(
                *record == identity,
                "worker fence retirement identity differs"
            ),
            None => ensure!(allow_absent, "worker fence retirement record is missing"),
        }
        ensure!(
            group.is_empty()?,
            "worker fence retirement tree is still populated"
        );
        self.mark_retired(&mut ledger, &name, identity)?;
        group.reclaim()?;
        let mut next = ledger.clone();
        next.retired.remove(&name);
        save(&self.directory, &next)?;
        *ledger = next;
        Ok(())
    }
}
impl staging_obligation::creation::Journal for WorkerFence {
    fn store(&self, record: &Record) -> Result<()> {
        record.validate_shape()?;
        ensure!(
            record.parent_path() == self.directory,
            "staging record escapes ownership directory"
        );
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        let records = ledger
            .staging
            .as_mut()
            .context("durable staging requires format 3")?;
        if let Some(current) = records.get(record.name()) {
            ensure!(
                current.permits_successor(record),
                "staging obligation identity or phase differs"
            );
        } else {
            ensure!(record.is_intent(), "staging creation intent is missing");
        }
        // Keep attempted publication in memory on IO/lost-ack failure, as for groups.
        records.insert(record.name().to_owned(), record.clone());
        save(&self.directory, &ledger)
    }
    fn clear(&self, record: &Record) -> Result<()> {
        ensure!(
            record.parent_path() == self.directory,
            "staging record escapes ownership directory"
        );
        record.validate_clear()?;
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| anyhow::anyhow!("worker fence ledger unavailable"))?;
        let records = ledger
            .staging
            .as_ref()
            .context("durable staging requires format 3")?;
        ensure!(
            records
                .get(record.name())
                .is_none_or(|current| current == record),
            "staging clear obligation differs"
        );
        let mut next = ledger.clone();
        next.staging.as_mut().unwrap().remove(record.name());
        save(&self.directory, &next)?;
        *ledger = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    fn fixture() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        root
    }
    #[test]
    fn cleanup_observation_keeps_active_and_deferred_retirement_custody() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        assert!(!fence.has_pending_worker_custody().unwrap());
        let fact = Group {
            path: root.path().join("test-pending-group"),
            device: 0,
            inode: 0,
        };
        fence
            .ledger
            .lock()
            .unwrap()
            .groups
            .insert("pending".into(), fact.clone());
        assert!(fence.has_pending_worker_custody().unwrap());
        {
            let mut ledger = fence.ledger.lock().unwrap();
            ledger.groups.clear();
            ledger.retired.insert("pending".into(), fact);
        }
        assert!(fence.has_pending_worker_custody().unwrap());
        fence.ledger.lock().unwrap().retired.clear();
        assert!(!fence.has_pending_worker_custody().unwrap());
    }
    #[test]
    fn required_staging_version_refuses_downgrade_before_recovery() {
        let root = fixture();
        let owner = WorkerFence::open(root.path()).unwrap();
        let scope = owner.scope().unwrap();
        drop(owner);
        let before = fs::read(root.path().join("worker-fence.json")).unwrap();
        let error = after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("below its database requirement"),
            "{error:#}"
        );
        assert_eq!(
            fs::read(root.path().join("worker-fence.json")).unwrap(),
            before
        );
        assert!(WorkerFence::open_bound_required(root.path(), Some(&scope), 4).is_err());
        assert_eq!(
            fs::read(root.path().join("worker-fence.json")).unwrap(),
            before
        );
        let untouched = fixture();
        assert!(WorkerFence::open_bound_required(untouched.path(), None, 3).is_err());
        assert_eq!(fs::read_dir(untouched.path()).unwrap().count(), 0);
        after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 2)
        })
        .unwrap();
    }
    fn after_transient_fork_leases(path: &Path) -> Result<WorkerFence> {
        after_transient_fork_lease_with(|| WorkerFence::open(path))
    }
    // Test synchronization only. Production must refuse a held lease. A fork
    // in another test thread can briefly retain a just-dropped owner's FD.
    fn after_transient_fork_lease_with(
        mut open: impl FnMut() -> Result<WorkerFence>,
    ) -> Result<WorkerFence> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match open() {
                Err(error)
                    if error.to_string() == "worker fence lease is still held"
                        && error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                result => return result,
            }
        }
    }
    #[test]
    fn held_lease_refuses_a_second_owner_and_reopens_after_drop() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        fence.lease_fd().unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        drop(fence);
        after_transient_fork_leases(root.path()).unwrap();
    }
    #[test]
    fn format3_creator_retires_exact_obligation_and_releases_claim() {
        let root = fixture();
        let fence = std::sync::Arc::new(WorkerFence::open(root.path()).unwrap());
        fence.enable_staging().unwrap();
        let pool = crate::resources::ResourcePool::new(1024, 1);
        let claim = pool
            .reserve(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        let created = fence.create_staging(claim).unwrap();
        let path = created.path();
        fs::write(path.join("authored"), b"owned bytes").unwrap();
        let record = fence
            .ledger
            .lock()
            .unwrap()
            .staging
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .clone();
        use staging_obligation::creation::Journal;
        assert!(Journal::clear(&*fence, &record).is_err());
        assert!(path.exists());
        created
            .close_after_use_retired()
            .wait(std::time::Instant::now() + std::time::Duration::from_secs(3))
            .unwrap();
        assert!(!path.exists());
        assert_eq!(pool.reserved_storage(), 0);
        assert!(fence
            .ledger
            .lock()
            .unwrap()
            .staging
            .as_ref()
            .unwrap()
            .is_empty());
        let scope = fence.scope().unwrap();
        drop(fence);
        assert_eq!(
            after_transient_fork_lease_with(|| {
                WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
            })
            .unwrap()
            .version()
            .unwrap(),
            3
        );
    }
    #[test]
    fn format3_missing_roster_and_unrecorded_roots_refuse_without_rewrite() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        fence.enable_staging().unwrap();
        let scope = fence.scope().unwrap();
        let path = root.path().join("worker-fence.json");
        let original = fs::read(&path).unwrap();
        drop(fence);
        let mut invalid: serde_json::Value = serde_json::from_slice(&original).unwrap();
        invalid.as_object_mut().unwrap().remove("staging");
        let invalid = serde_json::to_vec(&invalid).unwrap();
        fs::write(&path, &invalid).unwrap();
        let error = after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("staging roster missing or incompatible"));
        assert_eq!(fs::read(&path).unwrap(), invalid);
        fs::write(&path, &original).unwrap();
        let unknown = root
            .path()
            .join(format!("pulsar-stage-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&unknown).unwrap();
        fs::write(unknown.join("canary"), b"unrecorded bytes").unwrap();
        let error = after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("unrecorded staging resource"));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            fs::read(unknown.join("canary")).unwrap(),
            b"unrecorded bytes"
        );
        fs::remove_dir_all(unknown).unwrap();
        after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
        })
        .unwrap();
    }
    #[test]
    fn format3_recovery_preflights_all_roots_before_any_deletion() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        fence.enable_staging().unwrap();
        let mut records = BTreeMap::new();
        let mut created = Vec::new();
        for _ in 0..2 {
            created.push(
                staging_obligation::create(root.path(), &mut |record| {
                    let record = record.unwrap();
                    records.insert(record.name().to_owned(), record.clone());
                    Ok(())
                })
                .unwrap(),
            );
        }
        let mut ledger = fence.ledger.lock().unwrap().clone();
        ledger.staging = Some(records);
        save(root.path(), &ledger).unwrap();
        let first = ledger.staging.as_ref().unwrap().values().next().unwrap();
        let last = ledger.staging.as_ref().unwrap().values().last().unwrap();
        let first_path = root.path().join(first.name());
        let last_path = root.path().join(last.name());
        let moved = root.path().join("authored-original");
        fs::rename(&last_path, &moved).unwrap();
        fs::create_dir(&last_path).unwrap();
        fs::set_permissions(&last_path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(last_path.join("canary"), b"replacement bytes").unwrap();
        let prior = fs::read(root.path().join("worker-fence.json")).unwrap();
        let scope = fence.scope().unwrap();
        drop(fence);
        assert!(WorkerFence::open_bound_required(root.path(), Some(&scope), 3).is_err());
        assert!(first_path.exists());
        assert_eq!(
            fs::read(last_path.join("canary")).unwrap(),
            b"replacement bytes"
        );
        assert_eq!(
            fs::read(root.path().join("worker-fence.json")).unwrap(),
            prior
        );
        fs::remove_dir_all(&last_path).unwrap();
        fs::rename(&moved, &last_path).unwrap();
        after_transient_fork_lease_with(|| {
            WorkerFence::open_bound_required(root.path(), Some(&scope), 3)
        })
        .unwrap();
        assert!(!first_path.exists() && !last_path.exists());
    }
    #[test]
    fn canonical_legacy_ledger_migrates_without_changing_scope_or_lease() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let ledger = fence.ledger.lock().unwrap().clone();
        let prior = LegacyLedger {
            version: 1,
            scope: ledger.scope.clone(),
            lease_device: ledger.lease_device,
            lease_inode: ledger.lease_inode,
            groups: ledger.groups.clone(),
        };
        let path = root.path().join("worker-fence.json");
        fs::write(&path, serde_json::to_vec(&prior).unwrap()).unwrap();
        drop(fence);
        let current = after_transient_fork_lease_with(|| {
            WorkerFence::open_bound(root.path(), Some(&prior.scope))
        })
        .unwrap();
        let recovered = current.ledger.lock().unwrap();
        assert_eq!(recovered.version, 2);
        assert_eq!(recovered.scope, prior.scope);
        assert_eq!(recovered.lease_device, prior.lease_device);
        assert_eq!(recovered.lease_inode, prior.lease_inode);
        assert!(recovered.groups.is_empty() && recovered.retired.is_empty());
    }
    #[test]
    fn unknown_version_missing_retired_and_overlapping_records_refuse_without_rewrite() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let mut ledger = fence.ledger.lock().unwrap().clone();
        drop(fence);
        let path = root.path().join("worker-fence.json");
        let mut unknown = serde_json::to_value(&ledger).unwrap();
        unknown["version"] = 5.into();
        let mut missing = serde_json::to_value(&ledger).unwrap();
        missing.as_object_mut().unwrap().remove("retired");
        let name = "pulsar-worker-authored".to_string();
        let group = Group {
            path: format!("/sys/fs/cgroup/{name}").into(),
            device: 1,
            inode: 1,
        };
        ledger.groups.insert(name.clone(), group.clone());
        ledger.retired.insert(name, group);
        let overlap = serde_json::to_value(&ledger).unwrap();
        for (invalid, reason) in [
            (unknown, "unknown worker fence ledger version"),
            (missing, "missing field `retired`"),
            (overlap, "active/retired overlap"),
        ] {
            let bytes = serde_json::to_vec(&invalid).unwrap();
            fs::write(&path, &bytes).unwrap();
            let observed = format!(
                "{:#}",
                after_transient_fork_lease_with(|| WorkerFence::open(root.path())).unwrap_err()
            );
            assert!(
                observed.contains(reason),
                "expected refusal {reason:?}, observed {observed:?}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    fn mark_fixture_retired(fence: &WorkerFence, group: &AttemptCgroup) {
        let identity = group_identity(group.path()).unwrap();
        let name = group.path().file_name().unwrap().to_str().unwrap();
        fence
            .mark_retired(&mut fence.ledger.lock().unwrap(), name, identity)
            .unwrap();
    }
    #[test]
    #[ignore = "requires owned delegated cgroups; explicit native journal boundary fixture"]
    fn native_retirement_marker_recovers_before_and_after_directory_removal() {
        for removed in [false, true] {
            let root = fixture();
            let fence = WorkerFence::open(root.path()).unwrap();
            let scope = fence.scope().unwrap();
            let group = AttemptCgroup::create(
                &format!("retired-fixture-{}", uuid::Uuid::new_v4()),
                32 * 1024 * 1024,
                1,
            )
            .unwrap();
            fence.register(&group).unwrap();
            // This explicitly seeds a valid durable cut in the production marker
            // helper. No actual process crash or power-loss execution is claimed.
            mark_fixture_retired(&fence, &group);
            assert!(fence.ledger.lock().unwrap().groups.is_empty());
            assert_eq!(fence.ledger.lock().unwrap().retired.len(), 1);
            if removed {
                group.reclaim().unwrap();
            }
            drop(fence);
            let recovered = WorkerFence::open_bound(root.path(), Some(&scope)).unwrap();
            assert!(!group.path().exists());
            let ledger = recovered.ledger.lock().unwrap();
            assert!(ledger.groups.is_empty() && ledger.retired.is_empty());
        }
    }
    #[test]
    #[ignore = "requires owned delegated cgroups; run explicitly"]
    fn native_missing_active_record_refuses_without_rewrite() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let group = AttemptCgroup::create(
            &format!("missing-fixture-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        fence.register(&group).unwrap();
        group.reclaim().unwrap();
        drop(fence);
        let path = root.path().join("worker-fence.json");
        let bytes = fs::read(&path).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    #[test]
    #[ignore = "requires owned delegated cgroups; run explicitly"]
    fn native_registration_refuses_replaced_held_directory() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let name = format!("registration-fixture-{}", uuid::Uuid::new_v4());
        let group = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        let held_identity = group.identity().unwrap();
        group.reclaim().unwrap();
        let replacement = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        assert_ne!(held_identity, replacement.identity().unwrap());
        let ledger_path = root.path().join("worker-fence.json");
        let before = fs::read(&ledger_path).unwrap();
        assert!(
            fence.register(&group).is_err(),
            "registration adopted a replacement directory"
        );
        assert_eq!(fs::read(&ledger_path).unwrap(), before);
        assert!(fence.ledger.lock().unwrap().groups.is_empty());
        assert!(replacement.path().exists());
        assert!(replacement.is_empty().unwrap());
    }
    #[test]
    #[ignore = "requires owned delegated cgroups; run explicitly"]
    fn native_replaced_retired_group_refuses_and_preserves_replacement() {
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let name = format!("replacement-fixture-{}", uuid::Uuid::new_v4());
        let group = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        fence.register(&group).unwrap();
        mark_fixture_retired(&fence, &group);
        group.reclaim().unwrap();
        let replacement = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        drop(fence);
        let path = root.path().join("worker-fence.json");
        let bytes = fs::read(&path).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(replacement.path().exists());
        assert!(replacement.is_empty().unwrap());
    }
    #[test]
    #[ignore = "requires owned delegated cgroups; run explicitly"]
    fn native_populated_retired_record_refuses_before_cleaning_any_other_group() {
        use std::os::{fd::AsRawFd, unix::process::CommandExt};
        use std::process::{Command, Stdio};
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let empty_group = AttemptCgroup::create(
            &format!("first-fixture-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        fence.register(&empty_group).unwrap();
        let group = AttemptCgroup::create(
            &format!("populated-fixture-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        fence.register(&group).unwrap();
        mark_fixture_retired(&fence, &group);
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        drop(fence);
        let path = root.path().join("worker-fence.json");
        let bytes = fs::read(&path).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(empty_group.path().exists());
        assert!(child.try_wait().unwrap().is_none());
        group.kill();
        let _ = child.kill();
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let recovered = WorkerFence::open(root.path()).unwrap();
        assert!(!group.path().exists() && !empty_group.path().exists());
        assert!(recovered.ledger.lock().unwrap().retired.is_empty());
    }

    #[test]
    fn malformed_ledger_is_refused_without_rewriting_bytes() {
        let root = fixture();
        drop(WorkerFence::open(root.path()).unwrap());
        let path = root.path().join("worker-fence.json");
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(after_transient_fork_leases(root.path()).is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn unsafe_lease_symlink_cannot_change_target() {
        let root = fixture();
        let target = root.path().join("target");
        fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, root.path().join("worker-fence.lock")).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
    #[test]
    fn oversized_ledger_refuses_before_deserialization() {
        let root = fixture();
        drop(WorkerFence::open(root.path()).unwrap());
        let path = root.path().join("worker-fence.json");
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(LIMIT + 1).unwrap();
        assert!(after_transient_fork_leases(root.path()).is_err());
        assert_eq!(file.metadata().unwrap().len(), LIMIT + 1);
    }
    #[test]
    fn missing_ledger_and_replaced_lease_are_refused() {
        let root = fixture();
        drop(WorkerFence::open(root.path()).unwrap());
        let ledger_path = root.path().join("worker-fence.json");
        let bytes = fs::read(&ledger_path).unwrap();
        fs::remove_file(&ledger_path).unwrap();
        assert!(after_transient_fork_leases(root.path()).is_err());
        fs::write(&ledger_path, &bytes).unwrap();
        fs::rename(
            root.path().join("worker-fence.lock"),
            root.path().join("old-lease"),
        )
        .unwrap();
        assert!(after_transient_fork_leases(root.path()).is_err());
        assert_eq!(fs::read(ledger_path).unwrap(), bytes);
    }
    #[test]
    #[ignore = "requires a private writable delegated cgroup; run explicitly"]
    fn native_populated_and_altered_groups_refuse_then_empty_roster_recovers() {
        use std::{
            os::{fd::AsRawFd, unix::process::CommandExt},
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        let group = AttemptCgroup::create(
            &format!("fence-test-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        fence.register(&group).unwrap();
        let fd = group.procs.as_raw_fd();
        fence.lease_fd().unwrap();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        assert!(!group.is_empty().unwrap());
        drop(fence);
        let path = root.path().join("worker-fence.json");
        let original = fs::read(&path).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(child.try_wait().unwrap().is_none());
        let mut ledger: Ledger = serde_json::from_slice(&original).unwrap();
        ledger.groups.values_mut().next().unwrap().inode ^= 1;
        save(root.path(), &ledger).unwrap();
        let altered = fs::read(&path).unwrap();
        assert!(WorkerFence::open(root.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), altered);
        fs::write(&path, &original).unwrap();
        group.kill();
        let _ = child.kill();
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() || !group.is_empty().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let recovered = WorkerFence::open(root.path()).unwrap();
        assert!(recovered.ledger.lock().unwrap().groups.is_empty());
        assert!(!group.path().exists());
        assert!(recovered.ledger.lock().unwrap().retired.is_empty());
        drop(recovered);
        // A reclaimed attempt cannot be reused; explicit retirement uses a fresh group.
        let group = AttemptCgroup::create(
            &format!("fence-test-{}", uuid::Uuid::new_v4()),
            32 * 1024 * 1024,
            1,
        )
        .unwrap();
        let fd = group.procs.as_raw_fd();
        let mut command = Command::new("/bin/true");
        unsafe {
            command.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let fence = WorkerFence::open(root.path()).unwrap();
        fence.register(&group).unwrap();
        fence.retire(&mut child, &group).unwrap();
        assert!(fence.ledger.lock().unwrap().groups.is_empty());
        assert!(fence.ledger.lock().unwrap().retired.is_empty());
        assert!(!group.path().exists());
        assert!(group.is_empty().unwrap());
        group.peak_memory_bytes().unwrap();
    }
    #[test]
    #[ignore = "fork inherits other test threads' leases; run this native gate serially"]
    fn forked_pre_exec_child_retains_lease_after_parent_owner_drops() {
        use std::os::fd::FromRawFd;
        use std::time::{Duration, Instant};
        fn pipe() -> (File, File) {
            let mut fds = [0; 2];
            assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
            unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
        }
        struct ForkChild(libc::pid_t);
        impl Drop for ForkChild {
            fn drop(&mut self) {
                if self.0 > 0 {
                    unsafe {
                        libc::kill(self.0, libc::SIGKILL);
                        libc::waitpid(self.0, std::ptr::null_mut(), 0);
                    }
                }
            }
        }
        let root = fixture();
        let fence = WorkerFence::open(root.path()).unwrap();
        fence.enable_staging().unwrap();
        let scope = fence.scope().unwrap();
        fence.lease_fd().unwrap();
        let (mut ready, ready_writer) = pipe();
        let (release_reader, mut release) = pipe();
        let ready_fd = ready_writer.as_raw_fd();
        let release_fd = release_reader.as_raw_fd();
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            // Only async-signal-safe syscalls in the post-fork child. No Rust
            // destruction or allocator use before exec/_exit.
            unsafe {
                if libc::write(ready_fd, b"1".as_ptr().cast(), 1) != 1 {
                    libc::_exit(71);
                }
                let mut byte = 0u8;
                if libc::read(release_fd, (&mut byte as *mut u8).cast(), 1) != 1 {
                    libc::_exit(72);
                }
                libc::execl(
                    c"/bin/sleep".as_ptr(),
                    c"sleep".as_ptr(),
                    c"30".as_ptr(),
                    std::ptr::null::<libc::c_char>(),
                );
                libc::_exit(73);
            }
        }
        assert!(pid > 0);
        let mut child = ForkChild(pid);
        crate::launch_pipes::nonblocking(&ready).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut byte = [0u8];
        loop {
            match ready.read(&mut byte) {
                Ok(1) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                other => panic!("fork readiness failed: {other:?}"),
            };
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(fence);
        let error = WorkerFence::open_bound_required(root.path(), Some(&scope), 3).unwrap_err();
        assert_eq!(error.to_string(), "worker fence lease is still held");
        assert!(error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock));
        release.write_all(b"1").unwrap();
        let mut status = 0;
        let reopened = loop {
            assert_eq!(
                unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
                0,
                "exec child exited before lease-release observation"
            );
            match WorkerFence::open_bound_required(root.path(), Some(&scope), 3) {
                Ok(fence) => break fence,
                Err(error) => assert_eq!(error.to_string(), "worker fence lease is still held"),
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) }, 0);
        drop(reopened);
        // The lease became available while the exec'd child was still alive.
        // Stop only this unreaped child; its PID cannot have been reused.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        loop {
            let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if result == pid {
                child.0 = 0;
                break;
            }
            assert_eq!(result, 0);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(libc::WIFSIGNALED(status));
        assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
    }
}
