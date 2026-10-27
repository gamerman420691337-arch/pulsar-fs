//! Per-attempt cgroup v2 limits. Never modify a parent or move unrelated tasks.
#[cfg(target_os = "linux")]
use std::{fs, io, path::PathBuf};

#[cfg(target_os = "linux")]
pub fn delegated_root() -> io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let explicit = std::env::var_os("PULSAR_CGROUP_ROOT").map(PathBuf::from);
    let candidates = if let Some(root) = explicit {
        vec![root]
    } else {
        let membership = fs::read_to_string("/proc/self/cgroup")?;
        let relative = membership
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "cgroup v2 membership unavailable",
                )
            })?;
        let current = PathBuf::from("/sys/fs/cgroup").join(relative.trim_start_matches('/'));
        current
            .ancestors()
            .take_while(|p| p.starts_with("/sys/fs/cgroup"))
            .map(|p| p.to_path_buf())
            .collect()
    };
    for root in candidates {
        let meta = match fs::symlink_metadata(&root) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o200 == 0
        {
            continue;
        }
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(root.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cgroup path"))?;
        let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) } != 0
            || unsafe { filesystem.assume_init() }.f_type != 0x63677270
        {
            continue;
        }
        let controllers =
            fs::read_to_string(root.join("cgroup.subtree_control")).unwrap_or_default();
        if ["cpu", "memory", "pids"]
            .iter()
            .all(|required| controllers.split_whitespace().any(|c| c == *required))
        {
            return root.canonicalize();
        }
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "no user-delegated cgroup with cpu,memory,pids controllers; worker refused",
    ))
}

#[cfg(target_os = "linux")]
pub struct AttemptCgroup {
    path: PathBuf,
    directory: fs::File,
    pub procs: fs::File,
    reclaimed: std::sync::atomic::AtomicBool,
    retired_peak: std::sync::atomic::AtomicU64,
}
#[cfg(all(test, target_os = "linux", feature = "cuda"))]
pub(crate) struct FrozenTestGroup(std::sync::Arc<AttemptCgroup>);
#[cfg(all(test, target_os = "linux", feature = "cuda"))]
impl Drop for FrozenTestGroup {
    fn drop(&mut self) {
        use std::io::Write;
        if let Ok(mut file) = self.0.control(c"cgroup.freeze", true) {
            let _ = file.write_all(b"0");
        }
    }
}
#[cfg(target_os = "linux")]
impl AttemptCgroup {
    #[cfg(all(test, feature = "cuda"))]
    pub(crate) fn freeze_for_test(self: &std::sync::Arc<Self>) -> io::Result<FrozenTestGroup> {
        use std::io::Write;
        let guard = FrozenTestGroup(self.clone());
        self.control(c"cgroup.freeze", true)?.write_all(b"1")?;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if self
                .evidence(c"cgroup.events")?
                .lines()
                .any(|line| line == "frozen 1")
            {
                return Ok(guard);
            }
            if std::time::Instant::now() >= end {
                return Err(io::Error::other("owned test cgroup did not freeze"));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    /// The caller already owns and journals this directory; no path creation or
    /// cleanup occurs here if opening process placement fails.
    pub(crate) fn from_held(path: PathBuf, directory: fs::File) -> io::Result<Self> {
        let procs = directory_control(&directory, c"cgroup.procs", true)?;
        Ok(Self {
            path,
            directory,
            procs,
            reclaimed: std::sync::atomic::AtomicBool::new(false),
            retired_peak: std::sync::atomic::AtomicU64::new(0),
        })
    }
    pub fn create(attempt: &str, memory: u64, cpu_threads: u16) -> io::Result<Self> {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let path = delegated_root()?.join(format!("pulsar-worker-{attempt}"));
        fs::create_dir(&path)?;
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        let metadata = directory.metadata()?;
        let configured = (|| {
            configure_limits(&directory, memory, cpu_threads)?;
            let procs = directory_control(&directory, c"cgroup.procs", true)?;
            Ok(Self {
                path: path.clone(),
                directory,
                procs,
                reclaimed: std::sync::atomic::AtomicBool::new(false),
                retired_peak: std::sync::atomic::AtomicU64::new(0),
            })
        })();
        if configured.is_err() {
            // Never remove a replacement or a populated group on setup failure.
            let _ = reclaim_recorded(&path, metadata.dev(), metadata.ino(), false);
        }
        configured
    }
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
    fn control(&self, name: &std::ffi::CStr, write: bool) -> io::Result<fs::File> {
        directory_control(&self.directory, name, write)
    }
    fn evidence(&self, name: &std::ffi::CStr) -> io::Result<String> {
        use std::io::Read;
        let mut text = String::new();
        self.control(name, false)?
            .take(4097)
            .read_to_string(&mut text)?;
        if text.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cgroup evidence exceeds bound",
            ));
        }
        Ok(text)
    }
    pub(crate) fn identity(&self) -> io::Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let meta = self.directory.metadata()?;
        Ok((meta.dev(), meta.ino()))
    }
    /// Called only after direct-child and transport retirement are confirmed.
    pub(crate) fn reclaim(&self) -> io::Result<()> {
        use std::sync::atomic::Ordering;
        if self.reclaimed.load(Ordering::Acquire) {
            return Ok(());
        }
        if !self.is_empty()? {
            return Err(io::Error::other("retired cgroup is populated"));
        }
        let peak = self.peak_memory_bytes()?;
        let (device, inode) = self.identity()?;
        reclaim_recorded(&self.path, device, inode, false)?;
        self.retired_peak.store(peak, Ordering::Relaxed);
        self.reclaimed.store(true, Ordering::Release);
        Ok(())
    }
    fn owns_path(&self) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let held = self.directory.metadata()?;
        let current = fs::symlink_metadata(&self.path)?;
        Ok(current.is_dir() && current.dev() == held.dev() && current.ino() == held.ino())
    }
    pub fn kill(&self) {
        if self.reclaimed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        use std::io::Write;
        if let Ok(mut file) = self.control(c"cgroup.kill", true) {
            let _ = file.write_all(b"1");
        }
    }
    pub(crate) fn is_empty(&self) -> io::Result<bool> {
        if self.reclaimed.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(true);
        }
        let events = self.evidence(c"cgroup.events")?;
        match events
            .lines()
            .find_map(|line| line.strip_prefix("populated "))
        {
            Some("0") => Ok(true),
            Some("1") => Ok(false),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "missing cgroup population evidence",
            )),
        }
    }
    pub fn peak_memory_bytes(&self) -> io::Result<u64> {
        if self.reclaimed.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(self.retired_peak.load(std::sync::atomic::Ordering::Relaxed));
        }
        self.evidence(c"memory.peak")?
            .trim()
            .parse()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn configure_limits(
    directory: &fs::File,
    memory: u64,
    cpu_threads: u16,
) -> io::Result<()> {
    use std::io::Write;
    for (name, value) in [
        (c"memory.max", memory.to_string()),
        (c"memory.swap.max", "0".to_owned()),
        (c"pids.max", "64".to_owned()),
        (
            c"cpu.max",
            format!("{} 100000", u64::from(cpu_threads) * 100000),
        ),
    ] {
        directory_control(directory, name, true)?.write_all(value.as_bytes())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn directory_control(
    directory: &fs::File,
    name: &std::ffi::CStr,
    write: bool,
) -> io::Result<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let mode = if write {
        libc::O_WRONLY
    } else {
        libc::O_RDONLY
    };
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            mode | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}
#[cfg(target_os = "linux")]
pub(crate) fn reclaim_recorded(
    path: &std::path::Path,
    device: u64,
    inode: u64,
    allow_missing: bool,
) -> io::Result<()> {
    recorded_cgroup(path, device, inode, allow_missing, true)
}
#[cfg(target_os = "linux")]
pub(crate) fn check_recorded_empty(
    path: &std::path::Path,
    device: u64,
    inode: u64,
    allow_missing: bool,
) -> io::Result<()> {
    recorded_cgroup(path, device, inode, allow_missing, false)
}
#[cfg(target_os = "linux")]
fn recorded_cgroup(
    path: &std::path::Path,
    device: u64,
    inode: u64,
    allow_missing: bool,
    remove: bool,
) -> io::Result<()> {
    use std::io::Read;
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    };
    if !path.is_absolute() || !path.starts_with("/sys/fs/cgroup") {
        return Err(io::Error::other("invalid recorded cgroup path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("recorded cgroup lacks parent"))?;
    if parent.canonicalize()? != parent {
        return Err(io::Error::other("recorded cgroup parent changed"));
    }
    let parent_file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if parent_file.metadata()?.uid() != unsafe { libc::geteuid() }
        || unsafe { libc::fstatfs(parent_file.as_raw_fd(), stat.as_mut_ptr()) } != 0
        || unsafe { stat.assume_init() }.f_type != 0x63677270
    {
        return Err(io::Error::other(
            "recorded cgroup parent ownership/filesystem differs",
        ));
    }
    let directory = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let held = directory.metadata()?;
    if held.dev() != device || held.ino() != inode || held.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("recorded cgroup identity differs"));
    }
    use std::os::fd::FromRawFd;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c"cgroup.events".as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut events = String::new();
    unsafe { fs::File::from_raw_fd(fd) }
        .take(4097)
        .read_to_string(&mut events)?;
    if events.len() > 4096 || events.lines().find_map(|v| v.strip_prefix("populated ")) != Some("0")
    {
        return Err(io::Error::other(
            "recorded cgroup is populated or lacks bounded evidence",
        ));
    }
    let current = fs::symlink_metadata(path)?;
    if !current.is_dir() || current.dev() != device || current.ino() != inode {
        return Err(io::Error::other("recorded cgroup path was replaced"));
    }
    // The kernel also refuses removal of a populated cgroup. Concurrent hostile
    // same-UID namespace mutation is not established by this identity check.
    if remove {
        fs::remove_dir(path)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for AttemptCgroup {
    fn drop(&mut self) {
        if self.reclaimed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        self.kill();
        for _ in 0..20 {
            // Never remove a replacement directory after this owner was renamed.
            if !self.owns_path().unwrap_or(false) {
                return;
            }
            match fs::remove_dir(&self.path) {
                Ok(()) => return,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
    }
}

#[cfg(all(test, target_os = "linux", feature = "cuda"))]
mod tests {
    use super::*;
    use std::{
        os::{fd::AsRawFd, unix::process::CommandExt},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    fn spawn(group: &AttemptCgroup) -> Child {
        let fd = group.procs.as_raw_fd();
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(move || {
                if libc::write(fd, b"0".as_ptr().cast(), 1) != 1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        cmd.spawn().unwrap()
    }
    fn reap(child: &mut Child) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
    fn native_configuration_refuses_replaced_directory() {
        let name = format!("configuration-test-{}", uuid::Uuid::new_v4());
        let original = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        let path = original.path().to_path_buf();
        fs::remove_dir(&path).unwrap();
        let replacement = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        let result = configure_limits(&original.directory, 64 * 1024 * 1024, 2);
        assert!(directory_control(&original.directory, c"cgroup.procs", true).is_err());
        let (device, inode) = original.identity().unwrap();
        assert!(reclaim_recorded(&path, device, inode, false).is_err());
        let memory = fs::read_to_string(path.join("memory.max")).unwrap();
        let cpu = fs::read_to_string(path.join("cpu.max")).unwrap();
        drop(original);
        assert!(path.exists(), "original cleanup removed replacement");
        drop(replacement);
        assert!(result.is_err(), "configuration followed a replaced path");
        assert_eq!(memory.trim(), (32 * 1024 * 1024).to_string());
        assert_eq!(cpu.trim(), "100000 100000");
    }

    #[test]
    #[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
    fn native_failed_configuration_reclaims_owned_empty_group() {
        let name = format!("failed-configuration-test-{}", uuid::Uuid::new_v4());
        let path = delegated_root()
            .unwrap()
            .join(format!("pulsar-worker-{name}"));
        // The kernel rejects a zero CPU quota after the other controls were set.
        assert!(AttemptCgroup::create(&name, 32 * 1024 * 1024, 0).is_err());
        assert!(!path.exists(), "failed setup left its owned empty group");
    }

    #[test]
    #[ignore = "requires owned writable delegated cgroups; run explicitly serially"]
    fn native_replaced_group_signals_and_observes_only_held_directory() {
        let name = format!("held-directory-test-{}", uuid::Uuid::new_v4());
        let original = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        let old_path = original.path().to_path_buf();
        fs::remove_dir(&old_path).unwrap();
        let replacement = AttemptCgroup::create(&name, 32 * 1024 * 1024, 1).unwrap();
        let mut second = spawn(&replacement);
        original.kill();
        // The removed original is empty or unreadable, never the populated replacement.
        assert!(original.is_empty().unwrap_or(true));
        assert!(!replacement.is_empty().unwrap());
        assert!(second.try_wait().unwrap().is_none());
        drop(original);
        assert!(old_path.exists());
        assert!(second.try_wait().unwrap().is_none());
        replacement.kill();
        reap(&mut second);
        drop(replacement);
        assert!(!old_path.exists());
    }
}
