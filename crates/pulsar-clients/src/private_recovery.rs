//! Bounded private recovery storage shared by non-authoritative client journals.
use anyhow::Result;
#[cfg(unix)]
use anyhow::{ensure, Context};
use pulsar_protocol::SessionId;
use serde::{de::DeserializeOwned, Serialize};
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::path::Path;
#[cfg(unix)]
const LIMIT: u64 = 1024 * 1024;
#[cfg(not(unix))]
pub(crate) fn transaction<J: Serialize + DeserializeOwned, T>(
    _: &Path,
    _: &str,
    _: &SessionId,
    _: J,
    _: impl FnOnce(&mut J) -> Result<(T, bool)>,
) -> Result<T> {
    anyhow::bail!("Private recovery storage is not implemented on this platform")
}
#[cfg(unix)]
pub(crate) fn transaction<J: Serialize + DeserializeOwned, T>(
    directory: &Path,
    folder: &str,
    session: &SessionId,
    initial: J,
    operation: impl FnOnce(&mut J) -> Result<(T, bool)>,
) -> Result<T> {
    transaction_with_lock(directory, folder, session, initial, |journal, _| {
        operation(journal)
    })
}

/// Persist pre-effect checkpoints while keeping the workflow's private lock held.
#[cfg(not(unix))]
pub(crate) fn checkpoint_transaction<J: Serialize + DeserializeOwned, T>(
    _: &Path,
    _: &str,
    _: &SessionId,
    _: J,
    _: impl FnOnce(&mut J, &mut dyn FnMut(&J) -> Result<()>) -> Result<T>,
) -> Result<T> {
    anyhow::bail!("Private recovery storage is not implemented on this platform")
}
#[cfg(unix)]
pub(crate) fn checkpoint_transaction<J: Serialize + DeserializeOwned, T>(
    directory: &Path,
    folder: &str,
    session: &SessionId,
    initial: J,
    operation: impl FnOnce(&mut J, &mut dyn FnMut(&J) -> Result<()>) -> Result<T>,
) -> Result<T> {
    transaction_checkpoints_and_lock(directory, folder, session, initial, |j, _, save| {
        Ok((operation(j, save)?, false))
    })
}

/// Release the transaction's open-file-description lock even when a forked
/// child retains a duplicate descriptor. Ordinary completion reports failures;
/// Drop also attempts release on early refusal and unwinding.
#[cfg(unix)]
struct TransactionLock<'a> {
    file: &'a std::fs::File,
    held: bool,
}
#[cfg(unix)]
impl TransactionLock<'_> {
    fn release(&mut self) -> Result<()> {
        rustix::fs::flock(self.file, rustix::fs::FlockOperation::Unlock)
            .context("Recovery journal unlock failed; retain the original intent")?;
        self.held = false;
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for TransactionLock<'_> {
    fn drop(&mut self) {
        if self.held {
            let _ = rustix::fs::flock(self.file, rustix::fs::FlockOperation::Unlock);
        }
    }
}

#[cfg(unix)]
fn transaction_with_lock<J: Serialize + DeserializeOwned, T>(
    directory: &Path,
    folder: &str,
    session: &SessionId,
    initial: J,
    operation: impl FnOnce(&mut J, &std::fs::File) -> Result<(T, bool)>,
) -> Result<T> {
    transaction_checkpoints_and_lock(directory, folder, session, initial, |j, lock, _| {
        operation(j, lock)
    })
}
#[cfg(unix)]
fn transaction_checkpoints_and_lock<J: Serialize + DeserializeOwned, T>(
    directory: &Path,
    folder: &str,
    session: &SessionId,
    initial: J,
    operation: impl FnOnce(
        &mut J,
        &std::fs::File,
        &mut dyn FnMut(&J) -> Result<()>,
    ) -> Result<(T, bool)>,
) -> Result<T> {
    ensure!(
        !folder.is_empty()
            && folder
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid recovery storage namespace"
    );
    use rustix::fs::{
        flock, mkdirat, open, openat, renameat, statat, unlinkat, AtFlags, FlockOperation, Mode,
        OFlags,
    };
    use std::{
        fs::File,
        io::{Read, Write},
        os::unix::fs::MetadataExt,
    };
    let root = File::from(open(
        directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let private = |file: &File, is_directory: bool| -> Result<()> {
        let meta = file.metadata()?;
        ensure!(meta.uid()==rustix::process::geteuid().as_raw()&&meta.mode()&0o077==0&&if is_directory{meta.is_dir()}else{meta.is_file()&&meta.mode()&0o777==0o600&&meta.nlink()==1},"Recovery storage must be private and owned; files must be single-link 0600 regular files");
        Ok(())
    };
    private(&root, true)?;
    match mkdirat(&root, folder, Mode::from_raw_mode(0o700)) {
        Ok(()) => root.sync_all()?,
        Err(rustix::io::Errno::EXIST) => {}
        Err(error) => return Err(error.into()),
    }
    let dir = File::from(openat(
        &root,
        folder,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    private(&dir, true)?;
    let namespace = format!("{:x}", Sha256::digest(session.as_str().as_bytes()));
    let name = format!("{namespace}.json");
    let lock_name = format!("{namespace}.lock");
    let lock = File::from(openat(
        &dir,
        &lock_name,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?);
    private(&lock, false)?;
    flock(&lock, FlockOperation::NonBlockingLockExclusive)
        .context("Recovery journal is busy; no mutation was dispatched")?;
    let mut transaction_lock = TransactionLock {
        file: &lock,
        held: true,
    };
    let lock_matches = || -> Result<()> {
        let held = rustix::fs::fstat(&lock)?;
        let named = statat(&dir, &lock_name, AtFlags::SYMLINK_NOFOLLOW)?;
        ensure!(
            held.st_ino == named.st_ino && held.st_dev == named.st_dev,
            "Recovery lock identity changed"
        );
        Ok(())
    };
    lock_matches()?;
    let mut journal = match openat(
        &dir,
        &name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let file = File::from(fd);
            private(&file, false)?;
            ensure!(
                file.metadata()?.len() <= LIMIT,
                "Recovery journal exceeds byte limit"
            );
            let mut bytes = vec![];
            file.take(LIMIT + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= LIMIT,
                "Recovery journal exceeds byte limit"
            );
            serde_json::from_slice::<J>(&bytes)
                .context("Invalid recovery journal; original bytes preserved")?
        }
        Err(rustix::io::Errno::NOENT) => initial,
        Err(error) => return Err(error.into()),
    };
    let mut checkpoint = |journal: &J| -> Result<()> {
        let bytes = serde_json::to_vec(journal)?;
        ensure!(
            bytes.len() as u64 <= LIMIT,
            "Recovery journal exceeds byte limit"
        );
        lock_matches()?;
        let temporary = format!(".pending-{}", uuid::Uuid::new_v4());
        let mut file = File::from(openat(
            &dir,
            &temporary,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        let written = (|| -> Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            lock_matches()?;
            renameat(&dir, &temporary, &dir, &name)?;
            dir.sync_all()?;
            Ok(())
        })();
        if written.is_err() {
            let _ = unlinkat(&dir, &temporary, AtFlags::empty());
        }
        written?;
        Ok(())
    };
    let (value, changed) = operation(&mut journal, &lock, &mut checkpoint)?;
    if changed {
        checkpoint(&journal)?;
    }
    transaction_lock.release()?;
    Ok(value)
}

#[cfg(all(test, target_os = "linux"))]
mod inherited_lock_tests {
    use super::*;
    use std::{
        os::{
            fd::OwnedFd,
            unix::{
                fs::{MetadataExt, PermissionsExt},
                net::UnixStream,
            },
        },
        panic::{catch_unwind, AssertUnwindSafe},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn checkpoint_survives_refusal_and_keeps_workflow_exclusive() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new("checkpoint-session").unwrap();
        let mut nested_effect = false;
        let failed: Result<()> = checkpoint_transaction(
            directory.path(),
            "checkpoint-fixture",
            &session,
            0u32,
            |value, save| {
                *value = 1;
                save(value)?;
                assert!(transaction(
                    directory.path(),
                    "checkpoint-fixture",
                    &session,
                    0u32,
                    |_| {
                        nested_effect = true;
                        Ok(((), false))
                    }
                )
                .is_err());
                *value = 2;
                anyhow::bail!("operation failed after durable checkpoint")
            },
        );
        assert!(failed.is_err());
        assert!(!nested_effect);
        let retained = transaction(
            directory.path(),
            "checkpoint-fixture",
            &session,
            0u32,
            |value| Ok((*value, false)),
        )
        .unwrap();
        assert_eq!(retained, 1);
    }

    #[test]
    fn transaction_releases_inherited_lock_on_success_refusal_and_unwind() {
        for outcome in 0..3 {
            inherited_lock_case(outcome, Duration::ZERO);
        }
    }

    #[test]
    fn inherited_descriptor_survives_slow_transaction_completion() {
        inherited_lock_case(0, Duration::from_secs(6));
    }

    fn inherited_lock_case(outcome: u32, hold: Duration) {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new(format!("inherited-lock-{outcome}")).unwrap();
        let mut child = None;
        let mut control = None;
        let mut identity = None;
        let original = catch_unwind(AssertUnwindSafe(|| {
            transaction_with_lock(
                directory.path(),
                "inherited-lock-fixture",
                &session,
                0u32,
                |journal, lock| {
                    identity = Some(lock.metadata()?);
                    // Retain the real held open-file description in child stdin.
                    // A separate duplex control descriptor keeps the child alive
                    // across slow transaction syncs; it never reads journal bytes.
                    let (parent, held) = UnixStream::pair()?;
                    control = Some(parent);
                    child = Some(
                        Command::new("/bin/bash")
                            .env_clear()
                            .args(["--noprofile", "--norc", "-c", "read -r -u 1 release"])
                            .stdin(Stdio::from(lock.try_clone()?))
                            .stdout(Stdio::from(OwnedFd::from(held)))
                            .stderr(Stdio::null())
                            .spawn()?,
                    );
                    let contender = transaction(
                        directory.path(),
                        "inherited-lock-fixture",
                        &session,
                        0u32,
                        |journal| Ok((*journal, false)),
                    );
                    assert!(
                        contender.unwrap_err().to_string().contains("busy"),
                        "transaction must retain exclusion until completion"
                    );
                    // Exercise completion beyond the old child lifetime without
                    // changing the transaction's sync or lock implementation.
                    std::thread::sleep(hold);
                    *journal = 42;
                    match outcome {
                        0 => Ok(((), true)),
                        1 => anyhow::bail!("injected operation refusal"),
                        _ => panic!("injected operation unwind"),
                    }
                },
            )
        }));
        let mut child = child.expect("owned child launched");
        let child_identity = std::fs::metadata(format!("/proc/{}/fd/0", child.id()));
        let alive = child.try_wait().unwrap().is_none();
        let reread = transaction(
            directory.path(),
            "inherited-lock-fixture",
            &session,
            0u32,
            |journal| Ok((*journal, false)),
        );
        // Retire the owned process before asserting the red/green verdict.
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let exit = loop {
            if let Some(exit) = child.try_wait().unwrap() {
                break exit;
            }
            assert!(Instant::now() < deadline, "owned child did not retire");
            std::thread::sleep(Duration::from_millis(1));
        };
        drop(control);
        assert!(
            !exit.success(),
            "controlled child must be retired by the owner"
        );
        assert!(alive, "child must hold the descriptor during reread");
        let held = identity.unwrap();
        let inherited = child_identity.unwrap();
        assert_eq!((held.dev(), held.ino()), (inherited.dev(), inherited.ino()));
        match outcome {
            0 => original.unwrap().unwrap(),
            1 => assert_eq!(
                original.unwrap().unwrap_err().to_string(),
                "injected operation refusal"
            ),
            _ => assert!(original.is_err()),
        }
        assert_eq!(
            reread.unwrap_or_else(|error| panic!(
                "returned transaction left inherited lock busy: {error:#}"
            )),
            if outcome == 0 { 42 } else { 0 }
        );
    }
}
