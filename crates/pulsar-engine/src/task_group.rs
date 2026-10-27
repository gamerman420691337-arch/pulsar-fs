//! Bounded custody for workers whose callers do not need individual join handles.
//!
//! Shutdown must explicitly call `join`. There is deliberately no joining `Drop`:
//! a tracked worker may own the last `Arc` of its containing engine or task group.

use std::io;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle, ThreadId};

const MAX_WORKERS: usize = 256;

pub(crate) struct TaskGroup {
    state: Mutex<State>,
    changed: Condvar,
    capacity: usize,
}

struct State {
    closing: bool,
    handles: Vec<JoinHandle<()>>,
    // A drain owns its handles outside the mutex. Retain their IDs so one of
    // those workers cannot wait on the caller that is currently joining it.
    draining: bool,
    draining_threads: Vec<ThreadId>,
    failures: usize,
    first_failure: Option<String>,
}

impl State {
    fn failure(&mut self, diagnostic: String) {
        self.failures = self.failures.saturating_add(1);
        if self.first_failure.is_none() {
            self.first_failure = Some(diagnostic);
        }
    }

    fn result(&self) -> io::Result<()> {
        match &self.first_failure {
            Some(message) => Err(io::Error::other(format!(
                "task group recorded {} failure(s): {message}",
                self.failures
            ))),
            None => Ok(()),
        }
    }

    fn contains(&self, id: ThreadId) -> bool {
        self.draining_threads.contains(&id)
            || self.handles.iter().any(|handle| handle.thread().id() == id)
    }
}

impl Default for TaskGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskGroup {
    pub(crate) fn new() -> Self {
        Self::with_capacity(MAX_WORKERS)
    }

    fn with_capacity(capacity: usize) -> Self {
        assert!((1..=MAX_WORKERS).contains(&capacity));
        Self {
            state: Mutex::new(State {
                closing: false,
                handles: Vec::with_capacity(capacity),
                draining: false,
                draining_threads: Vec::with_capacity(capacity),
                failures: 0,
                first_failure: None,
            }),
            changed: Condvar::new(),
            capacity,
        }
    }

    fn recover_lock<'a>(
        &self,
        result: std::sync::LockResult<MutexGuard<'a, State>>,
    ) -> MutexGuard<'a, State> {
        match result {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.closing = true;
                state.failure("task group mutex was poisoned".into());
                // Existing handles remain owned and can still be joined.
                self.state.clear_poison();
                state
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.recover_lock(self.state.lock())
    }

    /// Accept a worker before close, or return its spawn/rejection error.
    /// Rejected work is dropped without execution. Completed workers are reaped
    /// before applying the 256-worker limit; their panic diagnostics survive.
    pub(crate) fn spawn(
        &self,
        name: &str,
        stack_size: Option<usize>,
        work: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        // Builder::spawn panics for NUL names. Reject before acquiring custody.
        if name.as_bytes().contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "worker name contains a NUL byte",
            ));
        }
        let mut state = self.lock();
        loop {
            if state.closing {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "task group is closing",
                ));
            }
            if state.draining {
                state = self.recover_lock(self.changed.wait(state));
                continue;
            }
            let mut finished = Vec::new();
            for index in (0..state.handles.len()).rev() {
                if state.handles[index].is_finished() {
                    finished.push(state.handles.swap_remove(index));
                }
            }
            if !finished.is_empty() {
                Self::begin_drain(&mut state, &finished);
                drop(state);
                self.finish_drain(finished);
                state = self.lock();
                continue;
            }
            if state.handles.len() >= self.capacity {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "task group worker capacity reached",
                ));
            }
            let mut builder = thread::Builder::new().name(name.to_owned());
            if let Some(size) = stack_size {
                builder = builder.stack_size(size);
            }
            // Holding the mutex through spawn and handle insertion makes close
            // atomic with admission. A newly running worker waits for insertion
            // before it can inspect or join its own task group.
            state.handles.push(builder.spawn(work)?);
            return Ok(());
        }
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.lock().closing
    }

    /// Atomically stop admission; accepted workers retain custody until join.
    pub(crate) fn close(&self) {
        self.lock().closing = true;
        self.changed.notify_all();
    }

    fn begin_drain(state: &mut State, handles: &[JoinHandle<()>]) {
        state.draining = true;
        state.draining_threads.clear();
        state
            .draining_threads
            .extend(handles.iter().map(|handle| handle.thread().id()));
    }

    fn finish_drain(&self, handles: Vec<JoinHandle<()>>) {
        let mut failures = Vec::new();
        for handle in handles {
            let name = handle.thread().name().unwrap_or("<unnamed>").to_owned();
            // A custom panic payload can itself panic in Drop. Catch that too,
            // so one worker cannot strand the rest of this retained batch.
            let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                handle.join().map_err(|_| ())
            }));
            match joined {
                Ok(Ok(())) => {}
                Ok(Err(())) => failures.push(format!("worker {name:?} panicked")),
                Err(payload) => {
                    // Do not run a second arbitrary panic-payload destructor.
                    // This exceptional payload is leaked to preserve shutdown.
                    std::mem::forget(payload);
                    failures.push(format!("joining worker {name:?} panicked"));
                }
            }
        }
        let mut state = self.lock();
        for failure in failures {
            state.failure(failure);
        }
        state.draining_threads.clear();
        state.draining = false;
        self.changed.notify_all();
    }

    /// Close and join every accepted worker outside the mutex. Concurrent
    /// callers wait for the same drain and receive the retained final result.
    /// A tracked worker receives WouldBlock, preserving every handle for an
    /// external caller; it must not retry synchronously from that worker.
    pub(crate) fn join(&self) -> io::Result<()> {
        let current = thread::current().id();
        let mut state = self.lock();
        state.closing = true;
        self.changed.notify_all();
        loop {
            if state.contains(current) {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "tracked worker cannot join its own task group",
                ));
            }
            if state.draining {
                state = self.recover_lock(self.changed.wait(state));
                continue;
            }
            if state.handles.is_empty() {
                return state.result();
            }
            let handles = std::mem::take(&mut state.handles);
            Self::begin_drain(&mut state, &handles);
            drop(state);
            self.finish_drain(handles);
            return self.lock().result();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant};

    struct DropProbe(Arc<AtomicUsize>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(Instant::now() < deadline, "test condition timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn join_waits_for_work_and_concurrent_joiners_share_completion() {
        let group = Arc::new(TaskGroup::new());
        let (release, wait) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        group
            .spawn("blocked", Some(128 * 1024), move || {
                started.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
        let (done, returned) = mpsc::channel();
        let mut callers = Vec::new();
        for _ in 0..2 {
            let group = group.clone();
            let done = done.clone();
            callers.push(thread::spawn(move || done.send(group.join()).unwrap()));
        }
        wait_until(|| group.is_closing());
        assert!(matches!(
            returned.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        for _ in 0..2 {
            returned
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
        }
        for caller in callers {
            caller.join().unwrap();
        }
        group.join().unwrap();
    }

    #[test]
    fn close_racing_spawn_never_loses_accepted_or_rejected_work() {
        for _ in 0..40 {
            let group = Arc::new(TaskGroup::default());
            let gate = Arc::new(Barrier::new(3));
            let runs = Arc::new(AtomicUsize::new(0));
            let drops = Arc::new(AtomicUsize::new(0));
            let (spawn_group, spawn_gate) = (group.clone(), gate.clone());
            let work_runs = runs.clone();
            let probe = DropProbe(drops.clone());
            let spawner = thread::spawn(move || {
                spawn_gate.wait();
                spawn_group.spawn("racing", None, move || {
                    let _probe = probe;
                    work_runs.fetch_add(1, Ordering::SeqCst);
                })
            });
            let (close_group, close_gate) = (group.clone(), gate.clone());
            let closer = thread::spawn(move || {
                close_gate.wait();
                close_group.close();
            });
            gate.wait();
            let admitted = spawner.join().unwrap();
            closer.join().unwrap();
            group.join().unwrap();
            let expected = match admitted {
                Ok(()) => 1,
                Err(error) => {
                    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                    0
                }
            };
            assert_eq!(runs.load(Ordering::SeqCst), expected);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert!(group.is_closing());
        }
    }

    #[test]
    fn rejected_closure_is_dropped_once_without_execution() {
        let group = TaskGroup::new();
        group.close();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe(drops.clone());
        let error = group
            .spawn("rejected", None, move || {
                let _probe = probe;
                panic!("rejected closure was executed");
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        group.join().unwrap();
    }

    #[test]
    fn capacity_is_bounded_and_finished_handles_are_reaped_on_spawn() {
        let group = TaskGroup::with_capacity(2);
        let mut releases = Vec::new();
        for _ in 0..2 {
            let (release, wait) = mpsc::channel();
            releases.push(release);
            group
                .spawn("capacity", None, move || {
                    wait.recv_timeout(Duration::from_secs(3)).unwrap();
                })
                .unwrap();
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe(drops.clone());
        assert_eq!(
            group
                .spawn("over-capacity", None, move || drop(probe))
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        for release in releases {
            release.send(()).unwrap();
        }
        wait_until(|| group.lock().handles.iter().all(JoinHandle::is_finished));
        let runs = Arc::new(AtomicUsize::new(0));
        let work_runs = runs.clone();
        group
            .spawn("after-reap", None, move || {
                work_runs.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert_eq!(group.lock().handles.len(), 1);
        group.join().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reaped_panic_remains_reported_and_join_still_waits_for_other_workers() {
        let group = Arc::new(TaskGroup::new());
        group
            .spawn("early-panic", None, || panic!("fixture"))
            .unwrap();
        wait_until(|| group.lock().handles[0].is_finished());
        let (release, wait) = mpsc::channel();
        group
            .spawn("still-working", None, move || {
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
            })
            .unwrap();
        assert_eq!(group.lock().handles.len(), 1);
        let (done, returned) = mpsc::channel();
        let joining = group.clone();
        let caller = thread::spawn(move || done.send(joining.join()).unwrap());
        wait_until(|| group.is_closing());
        assert!(matches!(
            returned.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release.send(()).unwrap();
        let error = returned
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("early-panic"));
        caller.join().unwrap();
        assert!(group.lock().handles.is_empty());
        assert_eq!(group.join().unwrap_err().to_string(), error.to_string());
    }

    #[test]
    fn self_join_refuses_without_losing_other_handles() {
        let group = Arc::new(TaskGroup::new());
        let worker_group = group.clone();
        let (done, returned) = mpsc::channel();
        let other = Arc::new(AtomicUsize::new(0));
        let (release, wait) = mpsc::channel();
        group
            .spawn("self-join", None, move || {
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
                done.send(worker_group.join().unwrap_err().kind()).unwrap();
            })
            .unwrap();
        let other_work = other.clone();
        group
            .spawn("other", None, move || {
                other_work.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        release.send(()).unwrap();
        assert_eq!(
            returned.recv_timeout(Duration::from_secs(3)).unwrap(),
            io::ErrorKind::WouldBlock
        );
        group.join().unwrap();
        assert_eq!(other.load(Ordering::SeqCst), 1);
        assert!(group.lock().handles.is_empty());
    }

    #[test]
    fn self_join_during_external_drain_does_not_wait_on_its_joiner() {
        let group = Arc::new(TaskGroup::new());
        let worker_group = group.clone();
        let (release, wait) = mpsc::channel();
        let (done, returned) = mpsc::channel();
        group
            .spawn("self-during-drain", None, move || {
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
                done.send(worker_group.join().unwrap_err().kind()).unwrap();
            })
            .unwrap();
        let joining = group.clone();
        let caller = thread::spawn(move || joining.join());
        wait_until(|| group.lock().draining);
        release.send(()).unwrap();
        assert_eq!(
            returned.recv_timeout(Duration::from_secs(3)).unwrap(),
            io::ErrorKind::WouldBlock
        );
        caller.join().unwrap().unwrap();
        assert!(group.lock().handles.is_empty());
    }

    #[test]
    fn worker_panic_does_not_prevent_joining_later_handles() {
        let group = TaskGroup::new();
        let gate = Arc::new(Barrier::new(3));
        let panic_gate = gate.clone();
        group
            .spawn("panic-first", None, move || {
                panic_gate.wait();
                panic!("fixture panic");
            })
            .unwrap();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe(drops.clone());
        let finish_gate = gate.clone();
        group
            .spawn("finish-second", None, move || {
                let _probe = probe;
                finish_gate.wait();
            })
            .unwrap();
        gate.wait();
        assert!(group
            .join()
            .unwrap_err()
            .to_string()
            .contains("panic-first"));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(group.lock().handles.is_empty());
    }

    #[test]
    fn invalid_name_returns_error_and_preserves_rejected_closure_custody() {
        let group = TaskGroup::new();
        let drops = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe(drops.clone());
        assert_eq!(
            group
                .spawn("bad\0name", None, move || drop(probe))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        group.join().unwrap();
    }
}
