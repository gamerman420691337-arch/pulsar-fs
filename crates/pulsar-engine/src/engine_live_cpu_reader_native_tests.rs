// Included inside captured_cpu_live_native_tests. Requires the parent's real
// authenticated Engine/worker helpers; this creates no synthetic storage rows.
const PRECEDING_APP_SHA: &str = "fe31502b670c9784723939ad4633fc1f646c5d8e9c7f0e43088072878982d3c6";
const PRECEDING_WORKER_SHA: &str =
    "e398f883ba2003e65e81b314eb3be575c49e99e045229d0f779448b346748de0";
const PRECEDING_DIAGNOSTIC: &str = "pulsar: unknown inference fallback reader version";
const READER_FILE_LIMIT: u64 = 64 * 1024 * 1024;
const READER_OUTPUT_LIMIT: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct ReaderFile {
    path: PathBuf,
    file_type: &'static str,
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    links: u64,
    byte_len: u64,
    sha256: String,
}
fn reader_file(
    path: &Path,
    limit: u64,
    inspection_files: &mut Vec<fs::File>,
) -> Result<(ReaderFile, Vec<u8>)> {
    use std::os::unix::fs::MetadataExt;
    let prior = fs::symlink_metadata(path)?;
    ensure!(
        prior.is_file() && prior.nlink() == 1 && prior.uid() == unsafe { libc::geteuid() },
        "reader fixture path is not an owned single-link regular file"
    );
    ensure!(
        prior.len() > 0 && prior.len() <= limit,
        "reader fixture file exceeds admitted bytes"
    );
    ensure!(
        inspection_files.len() < 64,
        "reader inspection descriptor budget exhausted"
    );
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    // Closing any raw descriptor for a SQLite file can drop this process's
    // POSIX locks. Retain inspection descriptors until every SQLite pin closes.
    inspection_files.push(file);
    let file = inspection_files
        .last_mut()
        .context("inspection descriptor absent")?;
    let held = file.metadata()?;
    ensure!(
        held.dev() == prior.dev() && held.ino() == prior.ino() && held.len() == prior.len(),
        "reader fixture path changed before read"
    );
    let mut bytes = Vec::new();
    Read::by_ref(file).take(limit + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    ensure!(
        bytes.len() as u64 == held.len()
            && after.len() == held.len()
            && after.mtime() == held.mtime()
            && after.mtime_nsec() == held.mtime_nsec()
            && named.is_file()
            && named.dev() == held.dev()
            && named.ino() == held.ino()
            && named.mode() == held.mode()
            && named.uid() == held.uid()
            && named.gid() == held.gid()
            && named.nlink() == 1,
        "reader fixture changed during snapshot"
    );
    Ok((
        ReaderFile {
            path: path.to_path_buf(),
            file_type: "regular",
            device: held.dev(),
            inode: held.ino(),
            mode: held.mode(),
            uid: held.uid(),
            gid: held.gid(),
            links: held.nlink(),
            byte_len: held.len(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        },
        bytes,
    ))
}
fn reader_images(
    state: &Path,
    inspection_files: &mut Vec<fs::File>,
) -> Result<std::collections::BTreeMap<String, ReaderFile>> {
    let mut images = std::collections::BTreeMap::new();
    for name in [
        "projects.sqlite3",
        "projects.sqlite3-wal",
        "projects.sqlite3-shm",
        "worker-fence.json",
    ] {
        images.insert(
            name.into(),
            reader_file(&state.join(name), READER_FILE_LIMIT, inspection_files)?.0,
        );
    }
    Ok(images)
}
// Diagnostic metadata only: retain no SHM byte values or database payloads.
fn reader_shm_changes(before: &[u8], after: &[u8]) -> Result<Value> {
    ensure!(
        before.len() == 32768 && after.len() == before.len(),
        "SHM diagnostic requires the observed single WAL-index page"
    );
    let mut changed = 0_usize;
    let mut ranges = Vec::new();
    let mut start = None;
    let mut reader_marks_only = true;
    for index in 0..=before.len() {
        let differs = index < before.len() && before[index] != after[index];
        if differs {
            changed += 1;
            reader_marks_only &= (100..120).contains(&index);
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            if ranges.len() < 64 {
                ranges.push((begin, index));
            }
        }
    }
    let regions = [
        ("header_a", 0, 48),
        ("header_b", 48, 96),
        ("backfill", 96, 100),
        ("reader_marks", 100, 120),
        ("lock_bytes", 120, 128),
        ("backfill_attempted", 128, 132),
        ("padding", 132, 136),
        ("remaining_index", 136, 32768),
    ]
    .into_iter()
    .map(|(name, begin, end)| {
        json!({"name":name,"begin":begin,"end_exclusive":end,
            "before_sha256":format!("{:x}",Sha256::digest(&before[begin..end])),
            "after_sha256":format!("{:x}",Sha256::digest(&after[begin..end])),
            "changed_bytes":before[begin..end].iter().zip(&after[begin..end]).filter(|(a,b)|a!=b).count()})
    })
    .collect::<Vec<_>>();
    Ok(json!({"byte_len":before.len(),"changed_byte_count":changed,
        "changed_ranges_end_exclusive":ranges,"range_limit":64,
        "ranges_may_be_truncated":ranges.len()==64,
        "changes_within_reader_marks":changed>0 && reader_marks_only,
        "regions":regions,"raw_byte_values_exported":false}))
}
fn reader_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(offset..offset + 4)
            .context("WAL word missing")?
            .try_into()?,
    ))
}
fn reader_wal(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.len() > 32,
        "preceding reader proof requires a nonempty WAL"
    );
    let magic = reader_u32(bytes, 0)?;
    let version = reader_u32(bytes, 4)?;
    let page = reader_u32(bytes, 8)? as usize;
    ensure!(
        matches!(magic, 0x377f0682 | 0x377f0683) && version == 3_007_000,
        "unsupported actual WAL header"
    );
    ensure!(
        matches!(
            page,
            512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768 | 65536
        ),
        "WAL page size differs"
    );
    let stride = 24 + page;
    ensure!(
        (bytes.len() - 32).is_multiple_of(stride),
        "WAL frame boundary differs"
    );
    let salts = &bytes[16..24];
    let mut commits = 0;
    let mut last_commit = 0;
    let mut valid_frames = 0;
    let mut versions = Vec::new();
    for (index, frame) in bytes[32..].chunks_exact(stride).enumerate() {
        if &frame[8..16] != salts {
            continue;
        }
        valid_frames += 1;
        let number = reader_u32(frame, 0)?;
        ensure!(number > 0, "WAL frame page is invalid");
        if number == 1 {
            ensure!(
                &frame[24..40] == b"SQLite format 3\0",
                "WAL page-one header differs"
            );
            versions.push((index + 1, reader_u32(frame, 24 + 60)?));
        }
        if reader_u32(frame, 4)? != 0 {
            commits += 1;
            last_commit = index + 1;
        }
    }
    ensure!(
        commits > 0
            && versions
                .iter()
                .any(|(frame, version)| *frame <= last_commit && *version == 38),
        "WAL lacks a committed captured-live reader38 page-one image"
    );
    Ok(
        json!({"page_size":page,"frame_count":(bytes.len()-32)/stride,"matching_salt_frames":valid_frames,
        "commit_frame_count":commits,"last_commit_frame":last_commit,"page_one_versions":versions,
        "reader38_in_committed_wal":true,"checksum_validation":"SQLite independently opens and validates the committed view"}),
    )
}
fn reader_pin(state: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(
        state.join("projects.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.busy_timeout(Duration::from_secs(2))?;
    db.execute_batch("PRAGMA query_only=ON;BEGIN;")?;
    let mode: String = db.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    ensure!(
        mode == "wal",
        "native reader proof requires actual WAL mode"
    );
    Ok(db)
}
fn reader_current_snapshot(
    db: &Connection,
    initial: &LiveCaptureReport,
    start: &Request,
) -> Result<Value> {
    let version: i64 = db.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    ensure!(
        version == 38,
        "public captured admission did not establish reader38"
    );
    crate::live_cpu_history::validate(db)?;
    let manifest = initial
        .inference
        .as_ref()
        .and_then(|i| i.cpu_manifest.as_ref())
        .context("original mode absent")?;
    let count: i64 = db.query_row("SELECT COUNT(*) FROM live_cpu_admissions", [], |r| r.get(0))?;
    let row:Option<(String,String,String,i64,String,Vec<u8>)>=db.query_row(
        "SELECT owner,request,project,revision,attempt,origin FROM live_cpu_admissions WHERE epoch=?1 AND typeof(origin)='blob' AND length(origin)<=2097152",
        [initial.identity.epoch.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let (owner, request, project, revision, attempt, origin) =
        row.context("original captured admission missing")?;
    ensure!(
        count == 1
            && Some(owner.as_str()) == start.session.as_ref().map(SessionId::as_str)
            && request == start.request_id.as_str()
            && project == initial.identity.project.as_str()
            && revision == i64::try_from(initial.identity.revision.value())?
            && attempt == manifest.stream.attempt_id.as_str(),
        "actual reader38 origin membership differs"
    );
    let value: Value = serde_json::from_slice(&origin)?;
    ensure!(
        value["request"]["auth_token"].is_null(),
        "actual reader38 origin retained a token"
    );
    // Value widens f32 to f64, unlike canonical typed JSON bytes. Compare the
    // full validated manifest in its declared types, as live_cpu_history does.
    let stored: LiveCpuInferenceManifest = serde_json::from_value(
        value
            .get("manifest")
            .context("actual reader38 origin manifest absent")?
            .clone(),
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "actual reader38 origin manifest cannot be decoded as captured CPU metadata"
        )
    })?;
    stored.validate()?;
    ensure!(
        serde_json::to_vec(&stored)? == serde_json::to_vec(manifest.as_ref())?,
        "actual reader38 origin changed captured mode"
    );
    let jobs: i64 = db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))?;
    let candidates: i64 = db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))?;
    ensure!(
        jobs == 0 && candidates == 0,
        "reader fixture created unrelated finite jobs/candidates"
    );
    Ok(
        json!({"reader_version":version,"admission_rows":count,"epoch":initial.identity.epoch,
        "attempt":manifest.stream.attempt_id,"original_request_id":start.request_id,
        "manifest_sha256":manifest.identity_sha256()?,"image":manifest.image.identity()?,
        "jobs":jobs,"candidates":candidates,"tokenless_origin_verified":true,"typed_manifest_bytes_equal":true,"sqlite_committed_view_validated":true}),
    )
}

struct ReaderProcess {
    child: std::process::Child,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    stdout_bytes: Vec<u8>,
    stderr_bytes: Vec<u8>,
    stdout_eof: bool,
    stderr_eof: bool,
    output_exceeded: bool,
    exit: Option<std::process::ExitStatus>,
    timed_out: bool,
    forced_kill: bool,
}
impl ReaderProcess {
    fn from_child(mut child: std::process::Child) -> Self {
        Self {
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            stdout_bytes: vec![],
            stderr_bytes: vec![],
            stdout_eof: false,
            stderr_eof: false,
            output_exceeded: false,
            exit: None,
            timed_out: false,
            forced_kill: false,
        }
    }
    fn nonblocking(&self) -> Result<()> {
        use std::os::fd::AsRawFd;
        for fd in self
            .stdout
            .iter()
            .map(AsRawFd::as_raw_fd)
            .chain(self.stderr.iter().map(AsRawFd::as_raw_fd))
        {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            ensure!(
                flags >= 0
                    && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
                "previous reader output nonblocking setup failed"
            );
        }
        Ok(())
    }
    fn drain<P: Read>(
        pipe: &mut Option<P>,
        bytes: &mut Vec<u8>,
        eof: &mut bool,
        exceeded: &mut bool,
    ) -> Result<()> {
        let Some(reader) = pipe.as_mut() else {
            return Ok(());
        };
        // A bounded amount per poll prevents noisy output from starving the
        // original deadline. Excess bytes are drained but never retained.
        for _ in 0..16 {
            let mut buffer = [0u8; 4096];
            match reader.read(&mut buffer) {
                Ok(0) => {
                    *eof = true;
                    *pipe = None;
                    break;
                }
                Ok(n) => {
                    if bytes.len() + n <= READER_OUTPUT_LIMIT {
                        bytes.extend_from_slice(&buffer[..n]);
                    } else {
                        *exceeded = true;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<()> {
        Self::drain(
            &mut self.stdout,
            &mut self.stdout_bytes,
            &mut self.stdout_eof,
            &mut self.output_exceeded,
        )?;
        Self::drain(
            &mut self.stderr,
            &mut self.stderr_bytes,
            &mut self.stderr_eof,
            &mut self.output_exceeded,
        )?;
        if self.exit.is_none() {
            self.exit = self.child.try_wait()?;
        }
        Ok(())
    }
    fn wait_refusal(&mut self) -> Result<()> {
        ensure!(
            self.stdout.is_some() && self.stderr.is_some(),
            "preceding reader output pipes absent"
        );
        self.nonblocking()?;
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            self.poll()?;
            if self.exit.is_some() && self.stdout_eof && self.stderr_eof {
                break;
            }
            if Instant::now() >= end {
                self.timed_out = true;
                bail!("preceding reader exceeded bounded exit/output observation");
            }
            ensure!(
                !self.output_exceeded,
                "preceding reader diagnostic exceeded bound"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
        ensure!(
            !self.output_exceeded && self.exit.as_ref().and_then(|s| s.code()) == Some(1),
            "preceding reader did not exit with expected refusal status"
        );
        ensure!(
            self.stdout_bytes.is_empty()
                && std::str::from_utf8(&self.stderr_bytes)?.trim() == PRECEDING_DIAGNOSTIC,
            "preceding reader did not emit the pinned read-only reader37 gate diagnostic"
        );
        Ok(())
    }
    fn cleanup(&mut self) -> Result<()> {
        if self.exit.is_none() {
            self.exit = self.child.try_wait()?;
        }
        if self.exit.is_none() {
            self.forced_kill = true;
            if let Err(error) = self.child.kill() {
                self.exit = self.child.try_wait()?;
                if self.exit.is_none() {
                    return Err(error.into());
                }
            }
        }
        let end = Instant::now() + Duration::from_secs(5);
        while self.exit.is_none() {
            self.exit = self.child.try_wait()?;
            ensure!(
                Instant::now() < end,
                "preceding reader child remained unjoined after owned kill"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
        // Do not block on inherited pipe writers. A pipe without EOF leaves the
        // fixture retained for the outer supervisor's exact descendant cleanup.
        self.nonblocking()?;
        self.poll()?;
        ensure!(
            self.stdout_eof && self.stderr_eof,
            "preceding reader inherited output writers remain unresolved"
        );
        Ok(())
    }
    fn receipt(&self) -> Value {
        let exact = std::str::from_utf8(&self.stderr_bytes)
            .is_ok_and(|s| s.trim() == PRECEDING_DIAGNOSTIC)
            && self.stdout_bytes.is_empty();
        json!({"pid":self.child.id(),"joined":self.exit.is_some(),"exit_code":self.exit.as_ref().and_then(|s|s.code()),
            "timed_out":self.timed_out,"forced_kill":self.forced_kill,"stdout_eof":self.stdout_eof,"stderr_eof":self.stderr_eof,
            "output_exceeded":self.output_exceeded,"stdout_bytes":self.stdout_bytes.len(),"stderr_bytes":self.stderr_bytes.len(),
            "stdout_sha256":format!("{:x}",Sha256::digest(&self.stdout_bytes)),"stderr_sha256":format!("{:x}",Sha256::digest(&self.stderr_bytes)),
            "specific_read_only_gate_refusal":exact,"diagnostic":if exact{Some(PRECEDING_DIAGNOSTIC)}else{None},
            "unexpected_output_not_exported":!exact,"outer_descendant_join_required":true})
    }
}

struct ReaderRig {
    directory: Option<tempfile::TempDir>,
    engine: Option<Arc<Engine>>,
    pins: Vec<Connection>,
    inspection_files: Vec<fs::File>,
    child: Option<ReaderProcess>,
    engines_joined: u32,
    engine_release_confirmed: bool,
}
impl ReaderRig {
    fn new() -> Result<Self> {
        Ok(Self {
            directory: Some(tempfile::tempdir()?),
            engine: None,
            pins: vec![],
            inspection_files: vec![],
            child: None,
            engines_joined: 0,
            engine_release_confirmed: true,
        })
    }
    fn directory(&self) -> &Path {
        self.directory.as_ref().expect("owned directory").path()
    }
    fn close_engine(&mut self) -> Result<()> {
        if let Some(engine) = &self.engine {
            engine.finish_shutdown()?;
            await_custody_clear(engine)?;
            let weak = Arc::downgrade(engine);
            drop(self.engine.take());
            self.engine_release_confirmed = weak.upgrade().is_none();
            ensure!(
                self.engine_release_confirmed,
                "current Engine Arc remained after joined shutdown"
            );
            self.engines_joined += 1;
        }
        Ok(())
    }
    fn close_pins(&mut self) -> Result<()> {
        while let Some(pin) = self.pins.pop() {
            pin.execute_batch("ROLLBACK;")?;
            pin.close().map_err(|(_, error)| error)?;
        }
        self.inspection_files.clear();
        Ok(())
    }
    fn finish(&mut self) -> Value {
        let child = self.child.as_mut().map(ReaderProcess::cleanup).transpose();
        let engine = self.close_engine();
        let pins = self.close_pins();
        let reader_started = self.child.is_some();
        let clean_refusal = self.child.as_ref().is_some_and(|child| {
            child.exit.as_ref().and_then(|status| status.code()) == Some(1)
                && !child.timed_out
                && !child.forced_kill
                && !child.output_exceeded
                && child.stdout_eof
                && child.stderr_eof
                && child.stdout_bytes.is_empty()
                && std::str::from_utf8(&child.stderr_bytes)
                    .is_ok_and(|text| text.trim() == PRECEDING_DIAGNOSTIC)
        });
        let safe = child.is_ok()
            && (!reader_started || clean_refusal)
            && engine.is_ok()
            && pins.is_ok()
            && self.engine_release_confirmed;
        let mut receipt = json!({"preceding_reader_started":reader_started,"preceding_reader_cleanup_joined":child.is_ok(),"preceding_reader_clean_refusal":clean_refusal,"engines_joined":self.engines_joined,
            "current_engine_joined":engine.is_ok() && self.engine_release_confirmed,"sqlite_pins_closed":pins.is_ok(),
            "inspection_descriptors_closed_after_pins":pins.is_ok() && self.inspection_files.is_empty(),
            "cleanup_errors":{"child":child.err().map(|e|format!("{e:#}")),"engine":engine.err().map(|e|format!("{e:#}")),"pins":pins.err().map(|e|format!("{e:#}"))},
            "private_state_removed":false,"retained_scratch":null,"directory_remove_error":null});
        if let Some(directory) = self.directory.take() {
            if safe {
                match directory.close() {
                    Ok(()) => receipt["private_state_removed"] = json!(true),
                    Err(error) => receipt["directory_remove_error"] = json!(error.to_string()),
                }
            } else {
                receipt["retained_scratch"] = json!(directory.keep());
            }
        }
        receipt
    }
}

#[test]
#[ignore = "requires current captured CPU worker/policy plus exact pinned E4 reader37 app/worker and owned bounded native supervisor; run exactly and serially"]
fn native_captured_cpu_live_reader37_refuses_committed_reader38_wal() {
    let output =
        environment_path("PULSAR_CAPTURED_CPU_LIVE_READER_OUTPUT").expect("owned reader output");
    let mut receipt = json!({"schema_version":1,"passed":false,"qualified":false,"physical_device":false,
        "public_equivalence_acceptance_called":false,"accepted_fixture_created":false,"authored_neutral_inputs":true,
        "scope":"Actual current public captured CPU Start/Ready/Stop creates reader38; exact pinned preceding reader37 app must refuse its committed WAL before writable Engine startup",
        "error":null,"teardown":null,"preceding_reader":null});
    let mut rig = ReaderRig::new().expect("owned reader fixture");
    receipt["fixture_directory"] = json!(rig.directory());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        ensure!(
            output.is_dir(),
            "reader output directory must already exist"
        );
        let inputs = Inputs::load()?;
        receipt["subjects"] = inputs.subjects();
        let old_app = environment_path("PULSAR_CAPTURED_CPU_LIVE_PRECEDING_APP")?;
        let old_worker = environment_path("PULSAR_CAPTURED_CPU_LIVE_PRECEDING_WORKER")?;
        let app_identity = file_identity(&old_app)?;
        let worker_identity = file_identity(&old_worker)?;
        ensure!(
            app_identity.sha256 == PRECEDING_APP_SHA
                && worker_identity.sha256 == PRECEDING_WORKER_SHA,
            "preceding reader subjects differ from fixed E4 pins"
        );
        receipt["preceding_subjects"] = json!({"app":{"path":old_app,"identity":app_identity},"worker":{"path":old_worker,"identity":worker_identity}});
        let state = rig.directory().join("state");
        let mut config = EngineConfig::new(state.clone(), inputs.worker.clone());
        config.cpu_owner_policy = Some(inputs.policy.clone());
        config.worker_timeout = Duration::from_secs(60);
        rig.engine = Some(Engine::open(config.clone())?);
        rig.engine_release_confirmed = false;
        let (actor, project, original, start_request, initial) = {
            let engine = rig.engine.as_ref().unwrap();
            let (actor, project, original) = create_project(engine)?;
            let prior = reader_pin(&state)?;
            let before: i64 = prior.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
            let count: i64 = prior.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))?;
            let existing: bool = prior.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='live_cpu_admissions')",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                before < 38 && count == 1 && !existing,
                "pre-capture snapshot was not original lower-reader history"
            );
            rig.pins.push(prior);
            receipt["pre_capture_snapshot"] = json!({"reader_version":before,"project_rows":count,"captured_admission_table":false,"read_transaction_held":true});
            let start_request = request(
                &actor,
                Some(&project),
                Some(0),
                Command::StartLiveCapture {
                    engine_epoch: AttemptId::new(engine.engine_epoch.clone())?,
                    options: inputs.options(LEASE_MS)?,
                },
            );
            let end = Instant::now() + Duration::from_millis(u64::from(LEASE_MS));
            let initial = report(engine.handle(start_request.clone()).result?)?;
            let ready = wait_ready(engine, &actor, &project, &initial, end)?;
            ensure!(
                ready.inference.as_ref().is_some_and(|i| i.state
                    == LiveCaptureInferenceState::Running
                    && i.completed == 0),
                "actual native captured Ready was not observed before producer input"
            );
            let terminal = report(call(
                engine,
                &actor,
                Some(&project),
                Some(0),
                Command::StopLiveCapture {
                    epoch: initial.identity.epoch.clone(),
                },
            )?)?;
            ensure!(
                !terminal.active && terminal.retirement == Some(LiveCaptureRetirement::Stopped),
                "public Stop did not retire original captured epoch"
            );
            await_custody_clear(engine)?;
            let after = snapshot(engine, &actor, &project)?;
            ensure!(
                after.revision == original.revision
                    && encode(&after.motion)? == encode(&original.motion)?,
                "reader capture changed committed motion"
            );
            receipt["capture"] = json!({"initial":initial,"ready":ready,"terminal":terminal,"public_start":true,"actual_captured_ready":true,
                "public_stop":true,"actual_estimates_claimed":0,"master_unchanged":true,"custody_clear_before_shutdown":true});
            let current = reader_pin(&state)?;
            receipt["current_snapshot"] =
                reader_current_snapshot(&current, &initial, &start_request)?;
            rig.pins.push(current);
            (actor, project, original, start_request, initial)
        };
        rig.close_engine()?;
        receipt["current_engine_released_before_probe"] = json!(true);
        let control_before = reader_images(&state, &mut rig.inspection_files)?;
        let (control_shm, control_bytes) = reader_file(
            &state.join("projects.sqlite3-shm"),
            READER_FILE_LIMIT,
            &mut rig.inspection_files,
        )?;
        ensure!(
            control_before["projects.sqlite3-shm"] == control_shm,
            "SHM changed before child-absent control"
        );
        let control_started = Instant::now();
        std::thread::park_timeout(Duration::from_millis(50));
        let before = reader_images(&state, &mut rig.inspection_files)?;
        let (before_shm, before_shm_bytes) = reader_file(
            &state.join("projects.sqlite3-shm"),
            READER_FILE_LIMIT,
            &mut rig.inspection_files,
        )?;
        ensure!(
            before["projects.sqlite3-shm"] == before_shm,
            "SHM changed while capturing pre-child diagnostic"
        );
        receipt["shm_diagnostics"] = json!({"parent_sqlite_calls_between_snapshots":false,
            "inspection_descriptors_retained_until_sqlite_pins_closed":true,
            "child_absent_control":{"elapsed_ns":u64::try_from(control_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                "all_files_unchanged":control_before==before,
                "shm":reader_shm_changes(&control_bytes,&before_shm_bytes)?}});
        let (main_image, main_bytes) = reader_file(
            &state.join("projects.sqlite3"),
            READER_FILE_LIMIT,
            &mut rig.inspection_files,
        )?;
        let (wal_image, wal_bytes) = reader_file(
            &state.join("projects.sqlite3-wal"),
            READER_FILE_LIMIT,
            &mut rig.inspection_files,
        )?;
        ensure!(
            before["projects.sqlite3"] == main_image && before["projects.sqlite3-wal"] == wal_image,
            "retained SQLite images changed between observations"
        );
        ensure!(
            main_bytes.len() >= 100 && &main_bytes[..16] == b"SQLite format 3\0",
            "main SQLite header differs"
        );
        let main_version = reader_u32(&main_bytes, 60)?;
        ensure!(
            main_version < 38,
            "reader38 declaration was checkpointed; this is not a WAL-dependent proof"
        );
        receipt["main_file_user_version"] = json!(main_version);
        receipt["committed_wal"] = reader_wal(&wal_bytes)?;
        receipt["before"] = serde_json::to_value(&before)?;
        ensure!(
            !state.join("engine.sock").exists() && !state.join("bulk.sock").exists(),
            "unexpected pre-probe endpoint"
        );
        let home = rig.directory().join("reader-home");
        artifacts::private_directory(&home)?;
        let child = std::process::Command::new(&old_app)
            .arg("engine")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("TMPDIR", rig.directory())
            .env("PULSAR_STATE_DIR", &state)
            .env("PULSAR_WORKER_EXECUTABLE", &old_worker)
            .current_dir(rig.directory())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        rig.child = Some(ReaderProcess::from_child(child));
        let refusal = rig.child.as_mut().unwrap().wait_refusal();
        receipt["preceding_reader"] = rig.child.as_ref().unwrap().receipt();
        let after = reader_images(&state, &mut rig.inspection_files)?;
        let (after_shm, after_shm_bytes) = reader_file(
            &state.join("projects.sqlite3-shm"),
            READER_FILE_LIMIT,
            &mut rig.inspection_files,
        )?;
        ensure!(
            after["projects.sqlite3-shm"] == after_shm,
            "SHM changed while capturing post-child diagnostic"
        );
        receipt["shm_diagnostics"]["preceding_reader_interval"] =
            reader_shm_changes(&before_shm_bytes, &after_shm_bytes)?;
        receipt["after"] = serde_json::to_value(&after)?;
        receipt["database_wal_shm_and_fence_unchanged"] = json!(before == after);
        receipt["endpoint_created"] =
            json!(state.join("engine.sock").exists() || state.join("bulk.sock").exists());
        refusal?;
        ensure!(
            before == after,
            "preceding reader changed exact DB/WAL/SHM/fence paths, types, identities or bytes"
        );
        ensure!(
            !state.join("engine.sock").exists() && !state.join("bulk.sock").exists(),
            "preceding reader created an Engine endpoint"
        );
        ensure!(
            file_identity(&old_app)? == app_identity
                && file_identity(&old_worker)? == worker_identity,
            "preceding pinned subjects changed"
        );
        receipt["post_probe_current_snapshot"] = reader_current_snapshot(
            rig.pins.last().context("current pinned view missing")?,
            &initial,
            &start_request,
        )?;
        ensure!(
            reader_images(&state, &mut rig.inspection_files)? == before,
            "read-only post-probe validation changed frozen SQLite images"
        );
        // File-preservation comparison ends here. Closing the original snapshots
        // and reopening the current Engine may legitimately checkpoint the WAL.
        rig.close_pins()?;
        rig.engine = Some(Engine::open(config)?);
        rig.engine_release_confirmed = false;
        {
            let engine = rig.engine.as_ref().unwrap();
            let replayed = report(engine.handle(start_request).result?)?;
            ensure!(
                !replayed.active
                    && replayed.retirement == Some(LiveCaptureRetirement::EngineRestarted)
                    && replayed
                        .inference
                        .as_ref()
                        .is_some_and(|i| i.state == LiveCaptureInferenceState::Retired),
                "current restart replay reconstructed a live capture"
            );
            let replay_manifest = replayed
                .inference
                .as_ref()
                .and_then(|i| i.cpu_manifest.as_ref())
                .context("restart lost captured mode")?;
            let old_manifest = initial
                .inference
                .as_ref()
                .and_then(|i| i.cpu_manifest.as_ref())
                .context("original mode absent")?;
            ensure!(
                replay_manifest.identity_sha256()? == old_manifest.identity_sha256()?,
                "restart rebound captured manifest"
            );
            ensure!(
                engine
                    .live_bulk_ready(&handshake(&actor, &initial), None)
                    .is_err(),
                "retired boot regained ingress"
            );
            let absent = engine
                .handle(request(
                    &actor,
                    Some(&project),
                    Some(0),
                    Command::LiveCaptureStatus {
                        epoch: initial.identity.epoch.clone(),
                    },
                ))
                .result;
            ensure!(
                matches!(absent,Err(ref error) if error.code==ErrorCode::NotFound),
                "restart replay reconstructed a capture registry entry"
            );
            await_custody_clear(engine)?;
            let after = snapshot(engine, &actor, &project)?;
            ensure!(
                after.revision == original.revision
                    && encode(&after.motion)? == encode(&original.motion)?,
                "current reader restart changed master"
            );
            receipt["current_restart"] = json!({"replay":replayed,"original_manifest_preserved":true,"retired_ingress_refused":true,
                "capture_registry_entry_absent":true,"no_worker_or_resource_claim_recreated":true,"master_unchanged":true});
        }
        inputs.verify_unchanged()?;
        receipt["borrowed_subjects_unchanged"] = json!(true);
        Ok(())
    }));
    let outcome = match result {
        Ok(value) => value,
        Err(_) => Err(anyhow::anyhow!(
            "reader native proof panicked; inspect test output"
        )),
    };
    let teardown = rig.finish();
    if let Some(child) = &rig.child {
        receipt["preceding_reader"] = child.receipt();
    }
    let cleanup = teardown["private_state_removed"] == true
        && teardown["current_engine_joined"] == true
        && teardown["preceding_reader_cleanup_joined"] == true
        && teardown["sqlite_pins_closed"] == true;
    receipt["teardown"] = teardown;
    receipt["passed"] = json!(outcome.is_ok() && cleanup);
    receipt["error"] = json!(outcome.as_ref().err().map(|e| format!("{e:#}")));
    save(&output.join("reader-campaign.json"), &receipt)
        .expect("retain exact preceding-reader result");
    outcome.unwrap();
    assert!(
        cleanup,
        "native preceding-reader cleanup incomplete; inspect receipt"
    );
}

#[test]
fn reader_typed_f32_roundtrip_preserves_bits_without_value_equality() {
    let original = vec![1.0_f32 / 255.0; 6];
    let bytes = serde_json::to_vec(&original).unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    let promoted = serde_json::to_value(&original).unwrap();
    assert_ne!(
        parsed, promoted,
        "the observed Value/f32 representation mismatch disappeared"
    );
    let restored: Vec<f32> = serde_json::from_value(parsed).unwrap();
    assert!(original
        .iter()
        .zip(&restored)
        .all(|(a, b)| a.to_bits() == b.to_bits()));
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    let mut changed = restored;
    changed[0] = f32::from_bits(changed[0].to_bits() + 1);
    assert_ne!(serde_json::to_vec(&changed).unwrap(), bytes);
}
