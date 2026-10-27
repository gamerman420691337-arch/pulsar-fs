//! Nonblocking worker pipes share one parent deadline and cancellation token.
//! Host filesystem operations and process retirement have separate obligations.
#![cfg(target_os = "linux")]
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(test)]
thread_local! {
    static BLOCKED_READ_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
    static BLOCKED_WRITE_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}
#[cfg(all(test, feature = "cuda"))]
pub(crate) fn blocked_read_hook_for_test(hook: Box<dyn FnOnce()>) {
    BLOCKED_READ_HOOK.with(|slot| *slot.borrow_mut() = Some(hook));
}
#[cfg(all(test, feature = "wasm"))]
pub(crate) fn blocked_write_hook_for_test(hook: Box<dyn FnOnce()>) {
    BLOCKED_WRITE_HOOK.with(|slot| *slot.borrow_mut() = Some(hook));
}

pub(crate) fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) struct Monitor<'a, R> {
    pub(crate) deadline: Instant,
    cancel: Cancel<'a>,
    stderr: Option<R>,
    diagnostics: Vec<u8>,
}
enum Cancel<'a> {
    Borrowed(&'a AtomicBool),
    Owned(std::sync::Arc<AtomicBool>),
}
impl Cancel<'_> {
    fn is_cancelled(&self) -> bool {
        match self {
            Self::Borrowed(v) => v.load(Ordering::Acquire),
            Self::Owned(v) => v.load(Ordering::Acquire),
        }
    }
}
impl<'a, R: Read> Monitor<'a, R> {
    pub(crate) fn new(deadline: Instant, cancel: &'a AtomicBool, stderr: R) -> Self {
        Self {
            deadline,
            cancel: Cancel::Borrowed(cancel),
            stderr: Some(stderr),
            diagnostics: Vec::new(),
        }
    }
    pub(crate) fn owned(
        deadline: Instant,
        cancel: std::sync::Arc<AtomicBool>,
        stderr: R,
    ) -> Monitor<'static, R> {
        Monitor {
            deadline,
            cancel: Cancel::Owned(cancel),
            stderr: Some(stderr),
            diagnostics: Vec::new(),
        }
    }
    pub(crate) fn check(&self) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("worker attempt cancelled"));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "worker wall deadline exceeded",
            ));
        }
        Ok(())
    }
    pub(crate) fn tick(&mut self) -> io::Result<()> {
        self.check()?;
        // One bounded read per tick prevents diagnostic floods starving checks.
        let mut chunk = [0u8; 8192];
        if let Some(stderr) = &mut self.stderr {
            match stderr.read(&mut chunk) {
                Ok(0) => {
                    self.stderr = None;
                }
                Ok(n) => {
                    let retain = n.min(16384 - self.diagnostics.len());
                    self.diagnostics.extend_from_slice(&chunk[..retain]);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub(crate) fn diagnostics(&self) -> &[u8] {
        &self.diagnostics
    }
    pub(crate) fn read<'b, T: Read>(&'b mut self, pipe: &'b mut T) -> Input<'b, 'a, T, R> {
        Input {
            pipe,
            monitor: self,
        }
    }
    pub(crate) fn write<'b, T: Write>(&'b mut self, pipe: &'b mut T) -> Output<'b, 'a, T, R> {
        Output {
            pipe,
            monitor: self,
        }
    }
}
pub(crate) struct Input<'b, 'a, T, R> {
    pipe: &'b mut T,
    monitor: &'b mut Monitor<'a, R>,
}
impl<T: Read, R: Read> Read for Input<'_, '_, T, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.monitor.tick()?;
            match self.pipe.read(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    #[cfg(test)]
                    if e.kind() == io::ErrorKind::WouldBlock {
                        BLOCKED_READ_HOOK.with(|slot| {
                            let hook = slot.borrow_mut().take();
                            if let Some(hook) = hook {
                                hook();
                            }
                        });
                    }
                    std::thread::sleep(Duration::from_millis(2))
                }
                result => {
                    self.monitor.check()?;
                    return result;
                }
            }
        }
    }
}
pub(crate) struct Output<'b, 'a, T, R> {
    pipe: &'b mut T,
    monitor: &'b mut Monitor<'a, R>,
}
impl<T: Write, R: Read> Write for Output<'_, '_, T, R> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.monitor.tick()?;
            match self.pipe.write(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    #[cfg(test)]
                    if e.kind() == io::ErrorKind::WouldBlock {
                        BLOCKED_WRITE_HOOK.with(|slot| {
                            let hook = slot.borrow_mut().take();
                            if let Some(hook) = hook {
                                hook();
                            }
                        });
                    }
                    std::thread::sleep(Duration::from_millis(2))
                }
                result => {
                    self.monitor.check()?;
                    return result;
                }
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.monitor.tick()?;
        self.pipe.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    #[test]
    fn owned_monitor_observes_cancellation_after_construction() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let monitor = Monitor::owned(
            Instant::now() + Duration::from_secs(1),
            cancel.clone(),
            io::empty(),
        );
        monitor.check().unwrap();
        cancel.store(true, Ordering::Release);
        assert!(monitor.check().is_err());
    }
    #[test]
    fn inherited_open_pipe_cannot_extend_deadline() {
        let (mut pipe, _held) = UnixStream::pair().unwrap();
        nonblocking(&pipe).unwrap();
        let cancel = AtomicBool::new(false);
        let mut monitor = Monitor::new(
            Instant::now() + Duration::from_millis(25),
            &cancel,
            io::empty(),
        );
        let start = Instant::now();
        assert_eq!(
            monitor.read(&mut pipe).read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn blocked_request_writer_obeys_deadline() {
        let (mut pipe, _held) = UnixStream::pair().unwrap();
        nonblocking(&pipe).unwrap();
        let cancel = AtomicBool::new(false);
        let mut monitor = Monitor::new(
            Instant::now() + Duration::from_millis(25),
            &cancel,
            io::empty(),
        );
        assert_eq!(
            monitor
                .write(&mut pipe)
                .write_all(&vec![0; 4 * 1024 * 1024])
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
    #[test]
    fn cancellation_is_not_retried_as_a_signal_interrupt() {
        let (mut pipe, _held) = UnixStream::pair().unwrap();
        nonblocking(&pipe).unwrap();
        let cancel = AtomicBool::new(true);
        let mut monitor = Monitor::new(
            Instant::now() + Duration::from_secs(1),
            &cancel,
            io::empty(),
        );
        assert_eq!(
            monitor
                .read(&mut pipe)
                .read_exact(&mut [0])
                .unwrap_err()
                .kind(),
            io::ErrorKind::Other
        );
    }
    #[test]
    fn stderr_flood_does_not_block_response() {
        let (mut pipe, mut sender) = UnixStream::pair().unwrap();
        nonblocking(&pipe).unwrap();
        let (stderr, mut logger) = UnixStream::pair().unwrap();
        nonblocking(&stderr).unwrap();
        let producer = std::thread::spawn(move || {
            logger.write_all(&vec![7; 1024 * 1024]).unwrap();
            sender.write_all(b"ok").unwrap();
        });
        let cancel = AtomicBool::new(false);
        let mut monitor = Monitor::new(Instant::now() + Duration::from_secs(5), &cancel, stderr);
        let mut bytes = [0; 2];
        monitor.read(&mut pipe).read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ok");
        assert_eq!(monitor.diagnostics().len(), 16384);
        producer.join().unwrap();
    }
    #[test]
    fn diagnostics_are_drained_after_retention_limit() {
        let cancel = AtomicBool::new(false);
        let mut monitor = Monitor::new(
            Instant::now() + Duration::from_secs(1),
            &cancel,
            io::Cursor::new(vec![7; 65536]),
        );
        for _ in 0..9 {
            monitor.tick().unwrap();
        }
        assert_eq!(monitor.diagnostics(), &vec![7; 16384]);
        assert!(monitor.stderr.is_none());
    }
}
