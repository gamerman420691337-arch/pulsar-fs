// Explicit serial native campaign support. Controls only the single worker
// group journaled by this campaign's private Engine; no production test hooks.
use std::os::{
    fd::{AsRawFd, FromRawFd, OwnedFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
};

#[derive(serde::Serialize, serde::Deserialize)]
struct CpuCrashSpec {
    state_dir: PathBuf,
    worker: PathBuf,
    policy: PathBuf,
    project: ProjectId,
    session: SessionId,
    source: SourceVersionId,
    settings: GenerationSettings,
    model: PathBuf,
    ready: PathBuf,
}

struct HeldCpuGroup {
    directory: fs::File,
    observation: serde_json::Value,
    processes: Vec<OwnedFd>,
    frozen: bool,
}
impl HeldCpuGroup {
    fn control(&self, name: &std::ffi::CStr, write: bool) -> fs::File {
        let access = if write {
            libc::O_WRONLY
        } else {
            libc::O_RDONLY
        };
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                access | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        assert!(
            fd >= 0,
            "owned group control: {}",
            std::io::Error::last_os_error()
        );
        unsafe { fs::File::from_raw_fd(fd) }
    }
    fn open(observation: serde_json::Value) -> Self {
        let path = PathBuf::from(observation["path"].as_str().unwrap());
        assert!(path.starts_with("/sys/fs/cgroup") && path != Path::new("/sys/fs/cgroup"));
        assert_eq!(fs::canonicalize(&path).unwrap(), path);
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .unwrap();
        let metadata = directory.metadata().unwrap();
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(observation["device"].as_u64(), Some(metadata.dev()));
        assert_eq!(observation["inode"].as_u64(), Some(metadata.ino()));
        let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
        assert_eq!(
            unsafe { libc::fstatfs(directory.as_raw_fd(), filesystem.as_mut_ptr()) },
            0
        );
        assert_eq!(unsafe { filesystem.assume_init() }.f_type, 0x6367_7270);
        Self {
            directory,
            observation,
            processes: Vec::new(),
            frozen: false,
        }
    }
    fn freeze(&mut self) {
        use std::io::Write;
        self.frozen = true;
        self.control(c"cgroup.freeze", true)
            .write_all(b"1")
            .unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let mut text = String::new();
            self.control(c"cgroup.events", false)
                .take(4097)
                .read_to_string(&mut text)
                .unwrap();
            assert!(text.len() <= 4096);
            if text.lines().any(|line| line == "frozen 1") {
                break;
            }
            assert!(
                std::time::Instant::now() < end,
                "owned CPU group did not freeze"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn pin_processes(&mut self) {
        let mut text = String::new();
        self.control(c"cgroup.procs", false)
            .take(4097)
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.len() <= 4096);
        let pids: Vec<u32> = text
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        assert!(!pids.is_empty() && pids.len() <= 64);
        for pid in &pids {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, *pid, 0) };
            assert!(fd >= 0, "cannot pin owned CPU process");
            self.processes
                .push(unsafe { OwnedFd::from_raw_fd(fd as i32) });
        }
        self.observation["pids"] = serde_json::json!(pids);
    }
    fn thaw(&mut self) {
        if self.frozen {
            self.control(c"cgroup.freeze", true)
                .write_all(b"0")
                .unwrap();
            self.frozen = false;
        }
    }
    fn assert_exited(&self) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let all = self.processes.iter().all(|fd| {
                let mut p = libc::pollfd {
                    fd: fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                assert!(unsafe { libc::poll(&mut p, 1, 0) } >= 0);
                p.revents & libc::POLLIN != 0
            });
            if all {
                return;
            }
            assert!(
                std::time::Instant::now() < end,
                "owned CPU processes did not exit"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
impl Drop for HeldCpuGroup {
    fn drop(&mut self) {
        if self.frozen {
            // Use the held descriptor even on assertion failure; never follow a
            // replacement pathname or signal an unpinned process.
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    c"cgroup.freeze".as_ptr(),
                    libc::O_WRONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd >= 0 {
                let mut file = unsafe { fs::File::from_raw_fd(fd) };
                let _ = file.write_all(b"0");
            }
        }
    }
}

fn hold_running_cpu(engine: &Arc<Engine>, job: &JobSnapshot) -> HeldCpuGroup {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let request: WorkerRequest = {
            let db = engine.db.lock().unwrap();
            engine.require_worker_fence(&db).unwrap();
            let snap = job_snapshot(&db, &job.job_id, &job.project_id).unwrap();
            assert!(
                matches!(snap.state.as_str(), "queued" | "running"),
                "worker finished before running capture: {:?}",
                snap
            );
            let raw: String = db
                .query_row(
                    "SELECT manifest FROM attempts WHERE id=?1",
                    [job.attempt_id.as_str()],
                    |r| r.get(0),
                )
                .unwrap();
            decode(&raw).unwrap()
        };
        if let Some(checkpoint) = crate::generation_progress::latest(&request).unwrap() {
            if checkpoint.record.analysed_samples >= 16 {
                let ledger: serde_json::Value = serde_json::from_slice(
                    &fs::read(engine.config.state_dir.join("worker-fence.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    ledger["scope"].as_str(),
                    Some(engine.worker_fence.scope().unwrap().as_str())
                );
                let groups = ledger["groups"].as_object().unwrap();
                assert_eq!(
                    groups.len(),
                    1,
                    "private Engine must own exactly one active group"
                );
                let (name, observation) = groups.iter().next().unwrap();
                let prefix = format!("pulsar-worker-{}-", engine.worker_fence.scope().unwrap());
                assert!(name.starts_with(&prefix));
                assert_eq!(
                    Path::new(observation["path"].as_str().unwrap())
                        .file_name()
                        .unwrap(),
                    name.as_str()
                );
                let mut group = HeldCpuGroup::open(observation.clone());
                group.freeze();
                group.pin_processes();
                let db = engine.db.lock().unwrap();
                assert_eq!(
                    job_snapshot(&db, &job.job_id, &job.project_id)
                        .unwrap()
                        .state,
                    "running"
                );
                assert!(engine
                    .workers
                    .lock()
                    .unwrap()
                    .contains_key(job.job_id.as_str()));
                group.observation["analysed_samples"] =
                    serde_json::json!(checkpoint.record.analysed_samples);
                group.observation["attempt_id"] = serde_json::json!(job.attempt_id);
                return group;
            }
        }
        assert!(
            std::time::Instant::now() < end,
            "no native CPU progress before running capture"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn start_cpu_history_job(engine: &Arc<Engine>, spec: &CpuCrashSpec, token: &str) -> JobSnapshot {
    let ResponseBody::Job(job) = engine
        .handle(
            Request::new(
                id!(RequestId),
                Command::Generate {
                    source_version: spec.source.clone(),
                    preset: "default".into(),
                    settings: Some(spec.settings.clone()),
                    model_path: Some(spec.model.clone()),
                },
            )
            .with_session(spec.session.clone())
            .with_auth_token(token.to_owned())
            .in_project(spec.project.clone(), Some(RevisionId::new(1))),
        )
        .result
        .unwrap()
    else {
        panic!("CPU job response")
    };
    job
}

#[test]
#[ignore = "subprocess only: explicit native CPU interruption campaign"]
fn native_cpu_interruption_child() {
    let Some(path) = std::env::var_os("PULSAR_CPU_CRASH_CHILD_SPEC") else {
        return;
    };
    let spec: CpuCrashSpec = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let mut cfg = EngineConfig::new(spec.state_dir.clone(), spec.worker.clone());
    cfg.cpu_owner_policy = Some(spec.policy.clone());
    let engine = Engine::open(cfg).unwrap();
    let token = std::env::var("PULSAR_CPU_CRASH_AUTH_TOKEN").unwrap();
    std::env::remove_var("PULSAR_CPU_CRASH_AUTH_TOKEN");
    let job = start_cpu_history_job(&engine, &spec, &token);
    let group = hold_running_cpu(&engine, &job);
    let job = job_snapshot(&engine.db.lock().unwrap(), &job.job_id, &spec.project).unwrap();
    assert_eq!(job.state, "running");
    let ready = serde_json::json!({"job": job, "group": group.observation});
    let pending = spec.ready.with_extension("pending");
    fs::write(&pending, serde_json::to_vec(&ready).unwrap()).unwrap();
    fs::rename(pending, &spec.ready).unwrap();
    // Parent must kill this actual Engine process, not manufacture a DB state.
    // A missed rendezvous fails and thaws through RAII within this bound.
    std::thread::sleep(std::time::Duration::from_secs(30));
    drop(group);
    panic!("parent did not interrupt the native Engine child");
}

struct CpuCrashChild(std::process::Child);
impl Drop for CpuCrashChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
