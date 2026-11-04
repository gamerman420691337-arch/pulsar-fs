//! In-process cleanup custody. Restart journaling is a separate obligation.
#![cfg(target_os = "linux")]
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

struct Root {
    path: PathBuf,
    directory: Option<File>,
}
impl Root {
    fn remove(&mut self) -> io::Result<()> {
        if self.directory.is_none() {
            self.directory = Some(
                OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(&self.path)?,
            );
        }
        let held = self.directory.as_ref().unwrap().metadata()?;
        if held.nlink() == 0 {
            return Ok(());
        }
        let current = fs::symlink_metadata(&self.path)?;
        if !current.is_dir() || current.dev() != held.dev() || current.ino() != held.ino() {
            return Err(io::Error::other(
                "staging cleanup directory identity differs",
            ));
        }
        fs::remove_dir_all(&self.path)?;
        if self.directory.as_ref().unwrap().metadata()?.nlink() != 0 {
            return Err(io::Error::other("staging cleanup deletion unconfirmed"));
        }
        Ok(())
    }
}
struct State {
    roots: Vec<Root>,
    retention: Option<Box<dyn Send>>,
    task: Option<Box<dyn FnMut() -> io::Result<()> + Send>>,
}
pub(crate) struct Cleanup {
    state: Arc<Mutex<State>>,
    done: Arc<AtomicBool>,
    started: bool,
}
impl Cleanup {
    /// Paths must be exclusively transferred owned directories, never scan results.
    /// No filesystem calls run on the starting or polling caller.
    pub(crate) fn start(paths: Vec<PathBuf>, retention: impl Send + 'static) -> Self {
        Self::start_owned(paths, Box::new(retention), None)
    }
    pub(crate) fn start_task(task: impl FnMut() -> io::Result<()> + Send + 'static) -> Self {
        Self::start_owned(Vec::new(), Box::new(()), Some(Box::new(task)))
    }
    fn start_owned(
        paths: Vec<PathBuf>,
        retention: Box<dyn Send>,
        cleanup_task: Option<Box<dyn FnMut() -> io::Result<()> + Send>>,
    ) -> Self {
        let state = Arc::new(Mutex::new(State {
            roots: paths
                .into_iter()
                .map(|path| Root {
                    path,
                    directory: None,
                })
                .collect(),
            retention: Some(retention),
            task: cleanup_task,
        }));
        let done = Arc::new(AtomicBool::new(false));
        let task = state.clone();
        let completed = done.clone();
        let started = std::thread::Builder::new()
            .name("pulsar-staging-cleanup".into())
            .spawn(move || {
                // Panic cannot discard the task's last resource/directory owner.
                let _custody = Self {
                    state: task.clone(),
                    done: completed.clone(),
                    started: false,
                };
                loop {
                    let mut held = task.lock().unwrap_or_else(|e| e.into_inner());
                    // A failure retains every remaining identity and resource. Retry
                    // only these owned roots; never widen recovery to a parent sweep.
                    held.roots.retain_mut(|root| root.remove().is_err());
                    let task_done = held.roots.is_empty()
                        && held.task.as_mut().is_none_or(|task| task().is_ok());
                    if task_done {
                        // Drop the task and its captured claims before publishing completion.
                        held.task.take();
                        held.retention.take();
                        completed.store(true, Ordering::Release);
                        return;
                    }
                    drop(held);
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .is_ok();
        Self {
            state,
            done,
            started,
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    pub(crate) fn wait(&self, deadline: Instant) -> io::Result<()> {
        while !self.is_complete() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "staging cleanup unconfirmed",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.started && !self.is_complete() {
            // A failed spawn/panic must not discard the last owner. An active
            // task can still finish and release retention through this state.
            std::mem::forget(self.state.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::ResourcePool;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn held_directory_replacement_is_not_deleted_and_retry_keeps_admission() {
        let parent = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir_in(parent.path()).unwrap().keep();
        let moved = parent.path().join("held-original");
        struct Restore {
            root: PathBuf,
            moved: PathBuf,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                if self.moved.exists() {
                    let _ = fs::remove_dir_all(&self.root);
                    let _ = fs::rename(&self.moved, &self.root);
                }
                let _ = fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700));
            }
        }
        let _restore = Restore {
            root: root.clone(),
            moved: moved.clone(),
        };
        fs::write(root.join("original"), b"owned bytes").unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
        let pool = ResourcePool::new(1024, 1);
        let cleanup = Cleanup::start(vec![root.clone()], pool.reserve(1024).unwrap());
        // Confirm the task holds the original inode before replacing its locator.
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            if cleanup
                .state
                .try_lock()
                .is_ok_and(|held| held.roots[0].directory.is_some())
            {
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("canary"), b"replacement bytes").unwrap();
        assert!(cleanup
            .wait(Instant::now() + Duration::from_millis(150))
            .is_err());
        assert_eq!(fs::read(root.join("canary")).unwrap(), b"replacement bytes");
        assert_eq!(fs::read(moved.join("original")).unwrap(), b"owned bytes");
        assert!(pool.reserve(1024).is_err());
        fs::remove_dir_all(&root).unwrap();
        fs::rename(&moved, &root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        cleanup
            .wait(Instant::now() + Duration::from_secs(3))
            .unwrap();
        assert!(!root.exists());
        assert!(pool.reserve(1024).is_ok());
    }
}
