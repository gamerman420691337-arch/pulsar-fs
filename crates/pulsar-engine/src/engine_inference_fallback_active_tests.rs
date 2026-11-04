// Included only in the ignored native SOFTWARE SIMULATION test module.
// This borrows the production-owned cgroup; it never constructs an owning cgroup
// object, renews a lease, changes a worker budget, or signals a process by PID.
use std::os::{
    fd::{AsRawFd, FromRawFd, OwnedFd},
    unix::fs::MetadataExt,
};

fn active_mode(mode: &str) -> bool {
    matches!(
        mode,
        "active_cancel"
            | "active_withdraw"
            | "active_expiry"
            | "active_dossier_expiry"
            | "pack_withdraw_active"
    )
}
fn active_wall_ms() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}
fn active_read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "native observation is not regular"
    );
    let mut raw = Vec::new();
    file.take(maximum as u64 + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() <= maximum, "native observation exceeds bound");
    Ok(raw)
}
fn active_control(directory: &fs::File, name: &str, write: bool) -> Result<fs::File> {
    let name = std::ffi::CString::new(name)?;
    let flags = if write {
        libc::O_WRONLY
    } else {
        libc::O_RDONLY
    };
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    ensure!(
        fd >= 0,
        "held cgroup control unavailable: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}
fn active_control_text(directory: &fs::File, name: &str) -> Result<String> {
    let mut raw = Vec::new();
    active_control(directory, name, false)?
        .take(4097)
        .read_to_end(&mut raw)?;
    ensure!(raw.len() <= 4096, "cgroup control exceeds bound");
    Ok(String::from_utf8(raw)?)
}
fn active_members(directory: &fs::File) -> Result<Vec<u32>> {
    let mut pids = active_control_text(directory, "cgroup.procs")?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<Vec<u32>, _>>()?;
    pids.sort_unstable();
    pids.dedup();
    ensure!(
        pids.len() <= 64 && !pids.contains(&std::process::id()),
        "native member scope differs"
    );
    Ok(pids)
}
fn active_process(pid: u32) -> Result<(u32, u64)> {
    let raw = active_read(&PathBuf::from(format!("/proc/{pid}/stat")), 8192)?;
    let text = std::str::from_utf8(&raw)?;
    let fields: Vec<_> = text
        .rsplit_once(')')
        .context("process stat framing")?
        .1
        .split_whitespace()
        .collect();
    ensure!(fields.len() >= 20, "process stat fields absent");
    Ok((fields[1].parse()?, fields[19].parse()?))
}
fn active_descendant(pid: u32) -> Result<()> {
    let mut current = pid;
    for _ in 0..64 {
        ensure!(current > 1, "cgroup member is outside native test ancestry");
        let (parent, _) = active_process(current)?;
        if parent == std::process::id() {
            return Ok(());
        }
        ensure!(parent != current, "process ancestry cycle");
        current = parent;
    }
    anyhow::bail!("native ancestry exceeds bound")
}
struct ActiveProcess {
    pid: u32,
    start: u64,
    handle: OwnedFd,
}
impl ActiveProcess {
    fn pin(pid: u32) -> Result<Self> {
        let (_, start) = active_process(pid)?;
        active_descendant(pid)?;
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        ensure!(
            fd >= 0,
            "pin native process: {}",
            std::io::Error::last_os_error()
        );
        let result = Self {
            pid,
            start,
            handle: unsafe { OwnedFd::from_raw_fd(fd as i32) },
        };
        result.recheck()?;
        Ok(result)
    }
    fn exited(&self) -> Result<bool> {
        let mut poll = libc::pollfd {
            fd: self.handle.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let count = unsafe { libc::poll(&mut poll, 1, 0) };
        ensure!(
            count >= 0 && poll.revents & libc::POLLNVAL == 0,
            "native pidfd poll failed"
        );
        Ok(count > 0 && poll.revents & (libc::POLLIN | libc::POLLHUP) != 0)
    }
    fn recheck(&self) -> Result<()> {
        ensure!(
            !self.exited()? && active_process(self.pid)?.1 == self.start,
            "native process retired or changed"
        );
        active_descendant(self.pid)
    }
    fn receipt(&self) -> Value {
        json!({"pid":self.pid,"start_time_ticks":self.start,"pidfd_held":true})
    }
}
fn active_ledger(engine: &Engine) -> Result<Value> {
    {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        engine.require_worker_fence(&db)?;
    }
    let path = engine.config.state_dir.join("worker-fence.json");
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "worker fence ledger ownership differs"
    );
    let ledger: Value = serde_json::from_slice(&active_read(&path, 1024 * 1024)?)?;
    let mut lease: libc::stat = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstat(engine.worker_fence.lease_fd()?, &mut lease) } == 0,
        "worker fence lease unavailable"
    );
    ensure!(
        ledger["scope"].as_str() == Some(engine.worker_fence.scope()?.as_str())
            && ledger["version"].as_u64() == Some(u64::from(engine.worker_fence.version()?))
            && ledger["lease_device"].as_u64() == Some(lease.st_dev)
            && ledger["lease_inode"].as_u64() == Some(lease.st_ino),
        "worker fence lease identity differs"
    );
    ensure!(
        ledger["groups"].as_object().context("worker groups")?.len() <= 128,
        "worker group bound"
    );
    Ok(ledger)
}
struct ActiveFrozenGroup {
    directory: fs::File,
    path: PathBuf,
    device: u64,
    inode: u64,
    members: Vec<ActiveProcess>,
    // Held objects prove namespace cwd identity without trusting display paths.
    cwd_objects: Vec<fs::File>,
    armed: bool,
}
impl ActiveFrozenGroup {
    fn open(value: &Value) -> Result<Self> {
        let path = PathBuf::from(value["path"].as_str().context("group path")?);
        ensure!(
            path.starts_with("/sys/fs/cgroup")
                && path != Path::new("/sys/fs/cgroup")
                && fs::canonicalize(&path)? == path,
            "group path is not canonical delegated cgroup"
        );
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        let meta = directory.metadata()?;
        let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
        ensure!(
            unsafe { libc::fstatfs(directory.as_raw_fd(), &mut filesystem) } == 0
                && filesystem.f_type == 0x6367_7270
                && meta.uid() == unsafe { libc::geteuid() }
                && value["device"].as_u64() == Some(meta.dev())
                && value["inode"].as_u64() == Some(meta.ino()),
            "held group filesystem or ownership differs"
        );
        Ok(Self {
            directory,
            path,
            device: meta.dev(),
            inode: meta.ino(),
            members: Vec::new(),
            cwd_objects: Vec::new(),
            armed: false,
        })
    }
    fn recheck_ledger(&self, engine: &Engine) -> Result<()> {
        let ledger = active_ledger(engine)?;
        let key = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .context("group name")?;
        let value = &ledger["groups"][key];
        ensure!(
            value["path"].as_str() == self.path.to_str()
                && value["device"].as_u64() == Some(self.device)
                && value["inode"].as_u64() == Some(self.inode),
            "held group left current Engine ledger"
        );
        let meta = fs::symlink_metadata(&self.path)?;
        ensure!(
            meta.is_dir() && meta.dev() == self.device && meta.ino() == self.inode,
            "group pathname changed"
        );
        Ok(())
    }
    fn pin_members(&mut self, request: &WorkerRequest) -> Result<Value> {
        let pids = active_members(&self.directory)?;
        ensure!(!pids.is_empty(), "successor group is empty");
        let mut members = Vec::new();
        let mut worker = None;
        let mut supervisor = None;
        let mut cwd_objects = Vec::new();
        for pid in pids {
            let member = ActiveProcess::pin(pid)?;
            let membership = active_read(&PathBuf::from(format!("/proc/{pid}/cgroup")), 8192)?;
            let membership = std::str::from_utf8(&membership)?;
            let relative = membership
                .lines()
                .find_map(|line| line.strip_prefix("0::"))
                .context("unified process cgroup")?;
            ensure!(
                relative.starts_with('/') && !relative.split('/').any(|part| part == ".."),
                "process cgroup path differs"
            );
            let observed =
                fs::metadata(Path::new("/sys/fs/cgroup").join(relative.trim_start_matches('/')))?;
            ensure!(
                observed.dev() == self.device && observed.ino() == self.inode,
                "process is outside held group"
            );
            let raw = active_read(&PathBuf::from(format!("/proc/{pid}/cmdline")), 65536)?;
            let argv: Vec<&[u8]> = raw.split(|b| *b == 0).filter(|v| !v.is_empty()).collect();
            let cpu_worker = argv.contains(&b"closed-cpu-worker".as_slice());
            let bwrap = argv.contains(&b"--unshare-all".as_slice());
            if cpu_worker && bwrap {
                ensure!(
                    argv.windows(2).any(|w| w[0] == b"--chdir"
                        && w[1] == request.output_dir.as_os_str().as_encoded_bytes()),
                    "supervisor output binding differs"
                );
                supervisor = Some(member.receipt());
            } else if cpu_worker {
                ensure!(
                    argv.contains(&b"/pulsar".as_slice()),
                    "captured worker executable argument absent"
                );
                let cwd = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC)
                    .open(format!("/proc/{pid}/cwd"))?;
                let guest_output = PathBuf::from(format!("/proc/{pid}/root"))
                    .join(request.output_dir.strip_prefix("/")?);
                let output = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC)
                    .open(guest_output)?;
                let a = cwd.metadata()?;
                let b = output.metadata()?;
                ensure!(
                    a.dev() == b.dev() && a.ino() == b.ino(),
                    "worker cwd is not exact guest output object"
                );
                ensure!(
                    worker.is_none(),
                    "multiple closed CPU workers in held group"
                );
                worker = Some(
                    json!({"process":member.receipt(),"cwd_device":a.dev(),"cwd_inode":a.ino(),"guest_output_device":b.dev(),"guest_output_inode":b.ino(),"cwd_object_match":true}),
                );
                cwd_objects.extend([cwd, output]);
            }
            member.recheck()?;
            members.push(member);
        }
        ensure!(
            worker.is_some() && supervisor.is_some(),
            "actual closed CPU worker and bwrap supervisor were not both observed"
        );
        self.members = members;
        self.cwd_objects = cwd_objects;
        Ok(
            json!({"path":self.path,"device":self.device,"inode":self.inode,"ledger_bound":true,
            "worker":worker,"supervisor":supervisor,"members":self.members.iter().map(ActiveProcess::receipt).collect::<Vec<_>>() }),
        )
    }
    fn freeze(&mut self) -> Result<()> {
        for member in &self.members {
            member.recheck()?;
        }
        self.armed = true; // Every subsequent error owns a thaw obligation.
        active_control(&self.directory, "cgroup.freeze", true)?.write_all(b"1")?;
        let end = Instant::now() + Duration::from_secs(1);
        loop {
            if active_control_text(&self.directory, "cgroup.events")?
                .lines()
                .any(|s| s == "frozen 1")
            {
                return Ok(());
            }
            ensure!(
                Instant::now() < end,
                "owned cgroup did not freeze within existing one-second bound"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn release(&mut self) -> Result<Value> {
        if !self.armed {
            return Ok(json!({"released":true,"already_released":true}));
        }
        let thaw = active_control(&self.directory, "cgroup.freeze", true)
            .and_then(|mut f| Ok(f.write_all(b"0")?));
        let proof = match thaw {
            Ok(()) => {
                json!({"released":true,"method":"descriptor_relative_thaw","original_group_only":true})
            }
            Err(error) => {
                let absent_or_replaced = match fs::symlink_metadata(&self.path) {
                    Ok(meta) => meta.dev() != self.device || meta.ino() != self.inode,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                    Err(e) => return Err(e.into()),
                };
                let exited = self
                    .members
                    .iter()
                    .map(ActiveProcess::exited)
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    absent_or_replaced && !exited.is_empty() && exited.iter().all(|v| *v),
                    "cannot prove original group retirement after thaw failed: {error:#}"
                );
                json!({"released":true,"method":"original_group_and_pidfds_retired","original_path_absent_or_replaced":true,"all_original_pidfds_exited":true,"replacement_untouched":true})
            }
        };
        self.armed = false;
        self.cwd_objects.clear();
        Ok(proof)
    }
}
impl Drop for ActiveFrozenGroup {
    fn drop(&mut self) {
        if self.armed {
            // Never reopen the pathname: even an error path can only thaw the
            // exact old group. The outer supervisor retains failed-run scratch.
            if let Err(error) = self.release() {
                eprintln!("native held-group thaw failed; supervisor cleanup required: {error:#}");
            }
        }
    }
}

struct ActiveConsumerBarrier {
    manifest: WorkerRequest,
    checkpoint: crate::generation_progress::Binding,
    group: ActiveFrozenGroup,
    cancel: Arc<AtomicBool>,
    proof: Value,
    held_at: Instant,
}
impl ActiveConsumerBarrier {
    fn manifest(&self) -> &WorkerRequest {
        &self.manifest
    }
    fn checkpoint(&self) -> &crate::generation_progress::Binding {
        &self.checkpoint
    }
    fn observe_invalidation_and_release(
        mut self,
        trigger: &str,
        terminal_deadline: Instant,
    ) -> Result<Value> {
        ensure!(
            !trigger.is_empty() && trigger.len() <= 64,
            "active trigger label bound"
        );
        while !self.cancel.load(Ordering::Acquire) {
            ensure!(
                Instant::now() < terminal_deadline,
                "active consumer cancellation was not observed within original observation bound"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let cancelled_at = active_wall_ms()?;
        let release = self.group.release()?;
        self.proof["trigger"] = json!(trigger);
        self.proof["actual_successor_cancel_observed"] = json!(true);
        self.proof["cancel_observed_at_unix_ms"] = json!(cancelled_at);
        self.proof["held_elapsed_ms"] = json!(self.held_at.elapsed().as_millis());
        self.proof["release"] = release;
        let admitted = self.proof["original_timing"]["admitted_at_unix_ms"]
            .as_u64()
            .context("admission time")?;
        let expires = self.proof["original_timing"]["accepted_expires_at_unix_ms"]
            .as_u64()
            .context("record expiry")?;
        let budget_end = admitted
            .checked_add(self.manifest.budget.wall_time_ms)
            .context("budget bound")?;
        if trigger == "attempt_budget_expiry" {
            ensure!(
                expires > budget_end && cancelled_at >= budget_end && cancelled_at < expires,
                "attempt-budget expiry did not occur inside the still-current record window"
            );
        } else if trigger == "accepted_record_expiry" {
            ensure!(
                expires < budget_end && cancelled_at >= expires && cancelled_at < budget_end,
                "record expiry did not invalidate before original attempt budget could elapse"
            );
        }
        Ok(self.proof.clone())
    }
}

fn active_job(
    engine: &Arc<Engine>,
    actor: &SessionId,
    previous: &WorkerRequest,
) -> Result<JobSnapshot> {
    let ResponseBody::Job(value) = call(
        engine,
        actor,
        Some(&previous.project_id),
        None,
        Command::JobStatus {
            job_id: previous.job_id.clone(),
        },
    )?
    else {
        anyhow::bail!("active job response absent");
    };
    Ok(value)
}
fn hold_active_successor(
    engine: &Arc<Engine>,
    actor: &SessionId,
    previous: &WorkerRequest,
    expected_profile: &DeclaredInferenceProfile,
    terminal_deadline: Instant,
) -> Result<ActiveConsumerBarrier> {
    let (target, checkpoint) = loop {
        ensure!(
            Instant::now() < terminal_deadline,
            "successor progress not observed within original bound"
        );
        let job = active_job(engine, actor, previous)?;
        ensure!(
            !["completed", "failed", "cancelled"].contains(&job.state.as_str()),
            "successor settled before verified-progress barrier"
        );
        if job.state == "running" && job.attempt_id != previous.attempt_id {
            let target = retained_manifest(engine, &job)?;
            if let Some(checkpoint) = crate::generation_progress::latest(&target)? {
                if checkpoint.record.analysed_samples >= 16 {
                    break (target, checkpoint);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let WorkerOperation::Generate { settings, .. } = &target.operation else {
        anyhow::bail!("successor is not generation");
    };
    ensure!(
        encode(
            settings
                .inference_profile
                .as_ref()
                .context("successor profile")?
        )? == encode(expected_profile)?
            && target.cuda_execution.is_none()
            && target.budget.wall_time_ms == 60_000,
        "successor profile or original budget differs"
    );
    let cancel = engine
        .workers
        .lock()
        .map_err(|_| anyhow::anyhow!("worker lock"))?
        .get(target.job_id.as_str())
        .cloned()
        .context("active successor cancellation handle absent")?;
    ensure!(
        !cancel.load(Ordering::Acquire),
        "successor was already invalidated before barrier"
    );
    let ledger = active_ledger(engine)?;
    let mut matches = Vec::new();
    for (key, value) in ledger["groups"].as_object().context("worker groups")? {
        let mut group = ActiveFrozenGroup::open(value)?;
        ensure!(
            group.path.file_name().and_then(|s| s.to_str()) == Some(key.as_str()),
            "group ledger key differs"
        );
        // Do not suppress identity failures for an unrelated ledger group. This
        // campaign admits exactly one active successor in this private Engine.
        let observed = group.pin_members(&target)?;
        matches.push((group, observed));
    }
    ensure!(
        matches.len() == 1,
        "expected exactly one owned successor group"
    );
    let (mut group, _) = matches.pop().unwrap();
    group.recheck_ledger(engine)?;
    ensure!(
        !cancel.load(Ordering::Acquire),
        "successor invalidated before freeze"
    );
    group.freeze()?;
    group.recheck_ledger(engine)?;
    let ownership = group.pin_members(&target)?;
    let job = active_job(engine, actor, previous)?;
    ensure!(
        job.state == "running"
            && job.attempt_id == target.attempt_id
            && !cancel.load(Ordering::Acquire),
        "successor state changed during freeze"
    );
    let frozen_checkpoint =
        crate::generation_progress::latest(&target)?.context("verified checkpoint disappeared")?;
    ensure!(
        frozen_checkpoint.record.analysed_samples >= checkpoint.record.analysed_samples
            && frozen_checkpoint.record.analysed_samples >= 16,
        "verified successor progress regressed"
    );
    let admission = {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        inference_fallback_store::retained(&db, &target)?
            .context("successor admission absent")?
            .admission
    };
    let wall_upper = admission
        .admitted_at_unix_ms
        .checked_add(target.budget.wall_time_ms)
        .context("original wall bound overflow")?
        .min(admission.expires_at_unix_ms);
    let frozen_at = active_wall_ms()?;
    ensure!(
        frozen_at < wall_upper,
        "successor lifetime expired before active barrier"
    );
    let proof = json!({"stalled_owned_cgroup_simulation":true,"simulated_eligibility":true,
        "primary_provider_absence_simulated":true,"real_measured_equivalence":false,"qualified":false,
        "physical_stopping_claim":false,"latency_qualification":false,
        "attempt_id":target.attempt_id,"manifest_sha256":fixture_hash(&fixture_bytes(&target)),
        "verified_progress":frozen_checkpoint,"analysed_samples":frozen_checkpoint.record.analysed_samples,
        "actual_model_run_count":null,"ownership":ownership,"frozen_before_trigger":true,"frozen_at_unix_ms":frozen_at,
        "original_timing":{"attempt_budget_ms":target.budget.wall_time_ms,"admitted_at_unix_ms":admission.admitted_at_unix_ms,
            "accepted_expires_at_unix_ms":admission.expires_at_unix_ms,"admitted_wall_upper_bound_unix_ms":wall_upper,
            "production_monotonic_deadline_observed":false,"deadline_renewed":false,"observation_bound_seconds":120}});
    Ok(ActiveConsumerBarrier {
        manifest: target,
        checkpoint: frozen_checkpoint,
        group,
        cancel,
        proof,
        held_at: Instant::now(),
    })
}

fn assert_active_terminal(
    mode: &str,
    terminal: &JobSnapshot,
    history: &JobAttemptPage,
    cleared_custody: &NativeCustodyClear,
) -> Result<Value> {
    let (state, code, message) = match mode {
        "active_cancel" | "pack_withdraw_active" => (
            "cancelled",
            ErrorCode::Cancelled,
            "fallback attempt was cancelled",
        ),
        "active_withdraw" => (
            "failed",
            ErrorCode::Forbidden,
            "fallback equivalence decision was invalidated",
        ),
        "active_expiry" | "active_dossier_expiry" => (
            "failed",
            ErrorCode::Cancelled,
            "fallback attempt lifetime expired",
        ),
        _ => anyhow::bail!("unknown active terminal case"),
    };
    ensure!(
        terminal.state == state && terminal.candidate_id.is_none(),
        "active terminal state differs for {mode}"
    );
    let target = history
        .attempts
        .iter()
        .find(|a| a.attempt_id == terminal.attempt_id)
        .context("active target history absent")?;
    let outcome = target
        .outcome
        .as_ref()
        .context("active target outcome absent")?;
    let diagnostic = outcome
        .validation_error
        .as_ref()
        .context("active target diagnostic absent")?;
    ensure!(
        diagnostic.code == code && diagnostic.message == message && !diagnostic.message_truncated,
        "active target diagnostic differs: {:?}: {}",
        diagnostic.code,
        diagnostic.message
    );
    ensure!(
        cleared_custody.workers_checked
            && cleared_custody.engine_epoch == outcome.engine_epoch.as_str(),
        "active target custody observation differs from its Engine epoch"
    );
    // Cancellation can retain an unknown historical observation. The independent
    // fence, reservations and worker-map checks above establish current cleanup.
    ensure!(
        outcome
            .worker_retirement
            .as_ref()
            .is_none_or(|retirement| retirement.provider_unavailable.is_none()),
        "active target reported another provider absence"
    );
    Ok(json!({"expected_state":state,"expected_code":code,"expected_message":message,
        "diagnostic":diagnostic,"worker_retirement_observed":true,
        "history_retirement_observed":outcome.worker_retirement.is_some(),
        "custody_observation":{"engine_epoch":cleared_custody.engine_epoch,
            "pending_worker_custody":false,"reserved_storage_bytes":0,
            "reserved_cpu_threads":0,"worker_count":0}}))
}
