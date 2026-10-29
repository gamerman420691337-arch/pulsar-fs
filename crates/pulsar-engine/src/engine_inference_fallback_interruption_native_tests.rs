// Private native campaign support. Authentication crosses an anonymous socket
// in memory only; the parent receipt never serializes the rendezvous structure.
#[derive(serde::Serialize, serde::Deserialize)]
struct NativeFallbackCrashReady {
    actor: SessionId,
    auth_token: String,
    project: ProjectId,
    target: WorkerRequest,
    barrier: Value,
    running_archive: Value,
}

fn native_fallback_interruption_rendezvous(
    engine: &Arc<Engine>,
    input: &Inputs,
    barrier: &ActiveConsumerBarrier,
) -> Result<Value> {
    let running_archive = archive_native_fallback(engine, &input.actor, &input.project)?;
    ensure!(
        running_archive["original_states"]
            .as_array()
            .is_some_and(|states| states.iter().any(|state| state == "running")),
        "native running archive lost actual running state"
    );
    ensure!(
        !barrier.cancel.load(Ordering::Acquire)
            && active_wall_ms()?
                < barrier.proof["original_timing"]["admitted_wall_upper_bound_unix_ms"]
                    .as_u64()
                    .context("original lifetime")?,
        "running archival outlived original successor admission"
    );
    let auth_token = TOKENS
        .with(|tokens| tokens.borrow().get(input.actor.as_str()).cloned())
        .context("child actor token absent")?;
    let ready = NativeFallbackCrashReady {
        actor: input.actor.clone(),
        auth_token,
        project: input.project.clone(),
        target: barrier.manifest().clone(),
        barrier: barrier.proof.clone(),
        running_archive,
    };
    let fd: i32 = std::env::var("PULSAR_FALLBACK_CRASH_CONTROL_FD")?.parse()?;
    ensure!(fd > 2, "crash control descriptor invalid");
    let mut socket = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    let bytes = serde_json::to_vec(&ready)?;
    ensure!(bytes.len() <= 1024 * 1024, "crash rendezvous exceeds bound");
    socket.write_all(&(bytes.len() as u32).to_le_bytes())?;
    socket.write_all(&bytes)?;
    socket.flush()?;
    drop(socket);
    // A missed parent rendezvous fails and thaws through the barrier's Drop.
    // The parent must terminate this real Engine, not manufacture a DB state.
    std::thread::sleep(Duration::from_secs(30));
    anyhow::bail!("parent did not interrupt actual fallback Engine process")
}

#[test]
#[ignore = "subprocess only: explicit native CPU fallback interruption campaign"]
fn native_cpu_fallback_interruption_child() {
    let state = environment_path("PULSAR_FALLBACK_CRASH_STATE").expect("explicit crash state");
    let fd: i32 = std::env::var("PULSAR_FALLBACK_CRASH_CONTROL_FD")
        .expect("anonymous control descriptor")
        .parse()
        .expect("control descriptor integer");
    assert!(fd > 2);
    // The descriptor crosses only the test exec. No later worker or reader may
    // inherit the authentication rendezvous channel.
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
        0
    );
    let mut config = EngineConfig::new(
        state,
        environment_path("PULSAR_FALLBACK_NATIVE_WORKER").unwrap(),
    );
    config.cpu_owner_policy = Some(environment_path("PULSAR_FALLBACK_NATIVE_POLICY").unwrap());
    config.worker_timeout = Duration::from_secs(60);
    let engine = Engine::open(config).unwrap();
    let result = run_case(&engine, "archive_interruption", true);
    engine
        .finish_shutdown()
        .expect("missed-rendezvous shutdown");
    result.expect("native fallback interruption parent rendezvous failed");
    panic!("interruption child unexpectedly returned");
}

struct NativeFallbackCrashChild(std::process::Child);
impl Drop for NativeFallbackCrashChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct NativeFallbackCrashDirectory(Option<tempfile::TempDir>);
impl NativeFallbackCrashDirectory {
    fn path(&self) -> &Path {
        self.0.as_ref().expect("live native crash directory").path()
    }
}
impl Drop for NativeFallbackCrashDirectory {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            // On failure the outer host supervisor must join descendants before
            // removing storage. Do not let TempDir race that process cleanup.
            let _ = directory.keep();
        }
    }
}

fn native_fallback_interruption_campaign() -> Result<Value> {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let mut directory = NativeFallbackCrashDirectory(Some(tempfile::tempdir()?));
    let state = directory.path().join("state");
    let (mut parent_socket, child_socket) = std::os::unix::net::UnixStream::pair()?;
    parent_socket.set_read_timeout(Some(Duration::from_secs(120)))?;
    let fd = child_socket.as_raw_fd();
    let child_log = fs::File::create(directory.path().join("child.log"))?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.args([
        "authority::tests::inference_fallback_native_tests::native_cpu_fallback_interruption_child",
        "--exact", "--ignored", "--nocapture", "--test-threads=1",
    ]).env("PULSAR_FALLBACK_CRASH_STATE", &state)
        .env("PULSAR_FALLBACK_CRASH_CONTROL_FD", fd.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(child_log.try_clone()?).stderr(child_log);
    unsafe {
        command.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = NativeFallbackCrashChild(command.spawn()?);
    drop(child_socket);
    let engine_process = ActiveProcess::pin(child.0.id())?;
    let mut prefix = [0; 4];
    if let Err(error) = parent_socket.read_exact(&mut prefix) {
        let diagnostic = active_read(&directory.path().join("child.log"), 64 * 1024)
            .ok()
            .and_then(|raw| String::from_utf8(raw).ok())
            .unwrap_or_default();
        anyhow::bail!(
            "child did not reach running fallback archive rendezvous: {error}; {diagnostic}"
        );
    }
    let length = u32::from_le_bytes(prefix) as usize;
    ensure!(
        length > 0 && length <= 1024 * 1024,
        "crash rendezvous length invalid"
    );
    let mut raw = vec![0; length];
    parent_socket.read_exact(&mut raw)?;
    let ready: NativeFallbackCrashReady = serde_json::from_slice(&raw)?;
    drop(raw);
    drop(parent_socket);
    ensure!(
        ready.target.project_id == ready.project
            && ready.barrier["analysed_samples"]
                .as_u64()
                .is_some_and(|n| n >= 16),
        "crash rendezvous does not bind native progress"
    );
    let mut group = ActiveFrozenGroup::open(&ready.barrier["ownership"])?;
    group.pin_members(&ready.target)?;
    ensure!(
        active_control_text(&group.directory, "cgroup.events")?
            .lines()
            .any(|line| line == "frozen 1"),
        "original child group is not frozen"
    );
    group.armed = true;
    engine_process.recheck()?;
    child.0.kill()?;
    ensure!(
        child.0.wait()?.signal() == Some(libc::SIGKILL),
        "Engine was not interrupted by SIGKILL"
    );
    ensure!(
        engine_process.exited()?,
        "interrupted Engine pidfd remains live"
    );
    let thaw = group.release()?;
    TOKENS.with(|tokens| {
        tokens
            .borrow_mut()
            .insert(ready.actor.to_string(), ready.auth_token)
    });
    let mut config = EngineConfig::new(state, environment_path("PULSAR_FALLBACK_NATIVE_WORKER")?);
    config.cpu_owner_policy = Some(environment_path("PULSAR_FALLBACK_NATIVE_POLICY")?);
    config.worker_timeout = Duration::from_secs(60);
    let engine = Engine::open(config)?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if group
                .members
                .iter()
                .map(ActiveProcess::exited)
                .collect::<Result<Vec<_>>>()?
                .iter()
                .all(|exited| *exited)
            {
                break;
            }
            ensure!(
                Instant::now() < end,
                "interrupted CPU pidfds did not retire"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        await_custody_clear(&engine, true)?;
        let ResponseBody::Job(job) = call(
            &engine,
            &ready.actor,
            Some(&ready.project),
            None,
            Command::JobStatus {
                job_id: ready.target.job_id.clone(),
            },
        )?
        else {
            anyhow::bail!("interrupted job response absent");
        };
        ensure!(
            job.state == "interrupted" && job.attempt_id == ready.target.attempt_id,
            "restart did not preserve original interrupted attempt"
        );
        let (attempts, candidates, pending): (i64, i64, i64) = {
            let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
            (
                db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get(0))?,
                db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))?,
                db.query_row("SELECT COUNT(*) FROM inference_fallback_pending", [], |r| {
                    r.get(0)
                })?,
            )
        };
        ensure!(
            attempts == 2 && candidates == 0 && pending == 0,
            "restart relaunched fallback or changed original history"
        );
        let interrupted_archive = archive_native_fallback(&engine, &ready.actor, &ready.project)?;
        ensure!(
            interrupted_archive["original_states"]
                .as_array()
                .is_some_and(|states| states.iter().any(|state| state == "interrupted")),
            "interrupted archive lost original interrupted state"
        );
        Ok(
            json!({"passed":true,"simulated_eligibility":true,"qualified":false,
            "engine_signal":"SIGKILL","engine_pidfd_exited":true,
            "original_worker_pidfds_exited":true,"restart_did_not_relaunch":true,
            "attempts":attempts,"candidates":candidates,"pending":pending,
            "original_progress":ready.barrier,"thaw":thaw,
            "running_archive":ready.running_archive,"interrupted_archive":interrupted_archive,
            "public_acceptance_called":false,"physical_device":false}),
        )
    }));
    let shutdown = engine.finish_shutdown();
    let custody = await_custody_clear(&engine, true);
    drop(engine);
    shutdown?;
    custody?;
    let mut proof = match result {
        Ok(result) => result?,
        Err(_) => anyhow::bail!("native fallback interruption archive panicked"),
    };
    directory.0.take().expect("live crash directory").close()?;
    proof["private_fixture_database_removed"] = json!(true);
    proof["shutdown_joined"] = json!(true);
    Ok(proof)
}
