// Test-owned WAL sync interception. Never registered as a process-global VFS.
use rusqlite::ffi;
type SyncFn = unsafe extern "C" fn(*mut ffi::sqlite3_file, i32) -> i32;
struct SyncDelayState {
    file: *mut ffi::sqlite3_file,
    original: SyncFn,
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
    calls: u64,
    commit_entered: bool,
    commit_returned: bool,
    original_result: Option<i32>,
    delayed_ns: u128,
}
thread_local! {
    static SYNC_DELAY: std::cell::RefCell<Option<SyncDelayState>> =
        const { std::cell::RefCell::new(None) };
}
unsafe extern "C" fn delayed_sqlite_sync(file: *mut ffi::sqlite3_file, flags: i32) -> i32 {
    // Rust panics must never unwind into SQLite's C stack.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SYNC_DELAY.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(state) = slot.as_mut() else { return ffi::SQLITE_IOERR_FSYNC; };
            if state.file != file || !state.commit_entered || state.commit_returned { return ffi::SQLITE_IOERR_FSYNC; }
            state.calls += 1;
            if state.calls == 1 {
                let began = Instant::now();
                if state.entered.send(()).is_err()
                    || state.release.recv_timeout(Duration::from_secs(35)).is_err() {
                    return ffi::SQLITE_IOERR_FSYNC;
                }
                state.delayed_ns = began.elapsed().as_nanos();
            }
            let result = unsafe { (state.original)(file, flags) };
            state.original_result = Some(result);
            result
        })
    })).unwrap_or(ffi::SQLITE_IOERR_FSYNC)
}
struct SqliteSyncDelay<'a> {
    db: &'a Mutex<Connection>,
    file: *mut ffi::sqlite3_file,
    original: *const ffi::sqlite3_io_methods,
    methods: Box<ffi::sqlite3_io_methods>,
}
impl<'a> SqliteSyncDelay<'a> {
    fn install(db: &'a Mutex<Connection>, entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>) -> Self {
        let connection = db.lock().unwrap();
        assert_eq!(connection.query_row("PRAGMA journal_mode", [], |r| r.get::<_,String>(0)).unwrap(), "wal");
        assert_eq!(connection.query_row("PRAGMA synchronous", [], |r| r.get::<_,i64>(0)).unwrap(), 2);
        let mut file: *mut ffi::sqlite3_file = std::ptr::null_mut();
        // SAFETY: the connection mutex excludes SQLite access while installing
        // this per-file table. The owned connection outlives the guard, and no
        // close/checkpoint/reopen occurs until the guard restores the table.
        unsafe {
            assert_eq!(ffi::sqlite3_file_control(connection.handle(), c"main".as_ptr(),
                ffi::SQLITE_FCNTL_JOURNAL_POINTER, (&mut file as *mut *mut ffi::sqlite3_file).cast()), ffi::SQLITE_OK);
            assert!(!file.is_null());
            let original = (*file).pMethods;
            assert!(!original.is_null());assert_eq!((*original).iVersion, 3);
            let original_sync = (*original).xSync.expect("actual WAL xSync");
            let mut methods = Box::new(*original);
            methods.xSync = Some(delayed_sqlite_sync);
            SYNC_DELAY.with(|slot| {
                assert!(slot.borrow().is_none());
                *slot.borrow_mut() = Some(SyncDelayState {file, original: original_sync,
                    entered, release, calls: 0, commit_entered: false, commit_returned: false, original_result: None, delayed_ns: 0});
            });
            (*file).pMethods = &*methods;
            Self {db, file, original, methods}
        }
    }
    fn checkpoint(phase: &str) {
        SYNC_DELAY.with(|slot| {
            let mut slot = slot.borrow_mut();let state = slot.as_mut().unwrap();
            match phase {
                "before_commit" => state.commit_entered = true,
                "after_commit" => state.commit_returned = true,
                _ => {}
            }
        });
    }
    fn summary(&self) -> serde_json::Value {
        SYNC_DELAY.with(|slot| {
            let slot = slot.borrow();let state = slot.as_ref().unwrap();
            assert!(state.calls > 0);assert!(state.commit_entered && state.commit_returned);assert_eq!(state.original_result, Some(ffi::SQLITE_OK));
            serde_json::json!({"wal_sync_calls":state.calls,"original_sync_returned":state.original_result,
                "delay_ns":state.delayed_ns,"commit_entry_and_return_observed":true,"all_sync_calls_inside_commit":true,"synchronous":"FULL","global_vfs_changed":false})
        })
    }
}
impl Drop for SqliteSyncDelay<'_> {
    fn drop(&mut self) {
        let _connection = self.db.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: restoration uses the same exclusion and still-live WAL file.
        unsafe { (*self.file).pMethods = self.original; }
        SYNC_DELAY.with(|slot| { slot.borrow_mut().take(); });
        let _ = &self.methods; // Keep the replacement table alive through restoration.
    }
}
