#![cfg(target_os = "linux")]
// Production-controller process checks. All media are tiny generated fixtures;
// these checks do not qualify a detector, GUI rendering, or physical devices.
use pulsar_clients::{
    portable::{
        PortableAction, PortableController, PortableEvent, PortableFailure, PortableReply,
        PortableTicket,
    },
    project_package::PackagePrivacyConsent,
    EngineApi, ResponseBody as ClientResponse, SessionClient,
};
use pulsar_core::{ProjectId, RevisionId};
use pulsar_protocol::{
    read_artifact_chunk, read_bulk_header, write_artifact_chunk, write_bulk_header, Command,
    LocalClient, PackageExportOperationState, PackageImportOperationId,
    PackageImportOperationState, PackageImportProgress, PackageImportStatus,
    PackageUploadDeclaration, PackageUploadHandshake, PackageUploadReply, PackageUploadRequest,
    RequestId, MAX_PACKAGE_UPLOAD_CHUNK_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use socket2::{Domain, SockAddr, Socket, Type};
use std::{
    fs::{self, File},
    io,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WAIT: Duration = Duration::from_secs(45);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ppd-{}-{nanos}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !thread::panicking() {
            let _ = fs::remove_dir_all(&self.root);
        } else {
            eprintln!(
                "portable controller fixture retained: {}",
                self.root.display()
            );
        }
    }
}

struct Engine {
    state: PathBuf,
    stderr: PathBuf,
    child: Option<Child>,
}

impl Engine {
    fn start(root: &Path, label: &str) -> Self {
        let state = root.join(format!("{label}-state"));
        let stderr = root.join(format!("{label}.stderr"));
        let child = ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .expect("spawn isolated production engine");
        let mut engine = Self {
            state,
            stderr,
            child: Some(child),
        };
        let deadline = Instant::now() + WAIT;
        loop {
            if LocalClient::connect(engine.endpoint(), Duration::from_millis(200)).is_ok() {
                return engine;
            }
            if let Some(status) = engine.child.as_mut().unwrap().try_wait().unwrap() {
                panic!(
                    "engine exited {status}: {}",
                    fs::read_to_string(&engine.stderr).unwrap_or_default()
                );
            }
            assert!(
                Instant::now() < deadline,
                "engine readiness timed out; see {}",
                engine.stderr.display()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn endpoint(&self) -> PathBuf {
        self.state.join("engine.sock")
    }
    fn bootstrap(&self) -> PathBuf {
        self.state.join("pairing.token")
    }

    fn client(&self) -> SessionClient {
        SessionClient::connect(&self.endpoint(), &self.bootstrap()).unwrap()
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Driver {
    controller: PortableController,
    pending: Vec<PortableEvent>,
    serial: u64,
}

impl Driver {
    fn new(engine: &Engine) -> Self {
        Self {
            controller: PortableController::new(engine.endpoint(), engine.bootstrap()).unwrap(),
            pending: Vec::new(),
            serial: 0,
        }
    }

    fn id(&mut self, label: &str) -> RequestId {
        self.serial += 1;
        RequestId::new(format!("desktop-{label}-{}", self.serial)).unwrap()
    }

    fn submit(&mut self, action: PortableAction) -> PortableTicket {
        self.controller
            .submit(action)
            .expect("submit production controller action")
    }

    fn wait(&mut self, ticket: PortableTicket, timeout: Duration) -> PortableEvent {
        let deadline = Instant::now() + timeout;
        loop {
            self.pending.extend(self.controller.poll());
            if let Some(index) = self.pending.iter().position(|event| event.ticket == ticket) {
                return self.pending.remove(index);
            }
            assert!(
                Instant::now() < deadline,
                "controller ticket {ticket:?} timed out"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn run(&mut self, action: PortableAction) -> PortableReply {
        let ticket = self.submit(action);
        self.wait(ticket, WAIT)
            .result
            .unwrap_or_else(|error| panic!("controller action failed: {}", error.message))
    }

    fn export(&mut self, project: &ProjectState, destination: &Path) {
        match self.run(PortableAction::AllowPackaging {
            project_id: project.project_id.clone(),
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        }) {
            PortableReply::PackagingAllowed { project_id } => {
                assert_eq!(project_id, project.project_id)
            }
            _ => panic!("expected explicit packaging acknowledgement"),
        }
        let request_id = self.id("start-export");
        let mut status = match self.run(PortableAction::StartExport {
            project_id: project.project_id.clone(),
            revision: project.revision,
            request_id,
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        }) {
            PortableReply::ExportStatus(status) => status,
            _ => panic!("expected export operation"),
        };
        let deadline = Instant::now() + WAIT;
        while status.state != PackageExportOperationState::Ready {
            assert!(
                !matches!(
                    status.state,
                    PackageExportOperationState::Failed
                        | PackageExportOperationState::Cancelled
                        | PackageExportOperationState::Interrupted
                        | PackageExportOperationState::Released
                ),
                "export became terminal before Ready: {status:?}"
            );
            assert!(Instant::now() < deadline, "export publication timed out");
            thread::sleep(Duration::from_millis(10));
            status = match self.run(PortableAction::ExportStatus {
                project_id: project.project_id.clone(),
                operation_id: status.operation_id.clone(),
            }) {
                PortableReply::ExportStatus(status) => status,
                _ => panic!("expected export status"),
            };
        }
        let request_id = self.id("download");
        match self.run(PortableAction::DownloadExport {
            project_id: project.project_id.clone(),
            operation_id: status.operation_id,
            destination: destination.to_path_buf(),
            request_id,
            consent: PackagePrivacyConsent::acknowledge_unencrypted_private_data(),
        }) {
            PortableReply::Downloaded(_) => {}
            _ => panic!("expected durably downloaded package"),
        }
        assert!(
            destination.is_file(),
            "successful controller result must publish the selected path"
        );
        // Release is intentionally not implicit. The desktop requires a separate
        // explicit disposal action rather than treating download as that consent.
    }

    fn start_import(&mut self, source: &Path) -> PackageImportOperationId {
        match self.run(PortableAction::PrepareImport {
            source: source.to_path_buf(),
        }) {
            PortableReply::ImportPrepared { declaration } => {
                assert!(declaration.byte_len > 52);
            }
            _ => panic!("expected retained-input preparation"),
        }
        let request_id = self.id("start-import");
        match self.run(PortableAction::StartImport { request_id }) {
            PortableReply::ImportStatus(status) => {
                assert!(
                    status.receipt.is_none(),
                    "starting upload must not publish a clone"
                );
                status.operation_id
            }
            _ => panic!("expected provisional import operation"),
        }
    }

    fn continue_import(&mut self, operation_id: PackageImportOperationId) -> PortableTicket {
        let begin_request_id = self.id("begin-upload");
        let seal_request_id = self.id("seal-upload");
        self.submit(PortableAction::ContinueImport {
            operation_id,
            begin_request_id,
            seal_request_id,
        })
    }

    fn import(&mut self, source: &Path) -> ProjectId {
        let operation_id = self.start_import(source);
        let ticket = self.continue_import(operation_id);
        match self
            .wait(ticket, WAIT)
            .result
            .unwrap_or_else(|error| panic!("import failed: {}", error.message))
        {
            PortableReply::ImportStatus(status) => {
                assert_eq!(status.state, PackageImportOperationState::Completed);
                status
                    .receipt
                    .expect("only completed import publishes a fresh project")
                    .project_id
            }
            _ => panic!("expected completed import status"),
        }
    }
}

#[derive(Clone)]
struct ProjectState {
    project_id: ProjectId,
    revision: RevisionId,
    program: Value,
}

fn project(response: ClientResponse) -> ProjectState {
    match response {
        ClientResponse::Project(snapshot) => ProjectState {
            project_id: snapshot.project_id,
            revision: snapshot.revision,
            program: serde_json::to_value(snapshot.program.as_ref()).unwrap(),
        },
        _ => panic!("expected project snapshot"),
    }
}

fn snapshot(client: &mut SessionClient, project_id: &ProjectId) -> ProjectState {
    project(
        client
            .execute(Command::GetSnapshot, Some(project_id.clone()), None)
            .unwrap(),
    )
}

fn script_edit(
    client: &mut SessionClient,
    base: &ProjectState,
    path: &Path,
    positions: [u8; 4],
) -> ProjectState {
    let actions: Vec<_> = positions
        .iter()
        .enumerate()
        .map(|(index, position)| serde_json::json!({"at": index * 100, "pos": position}))
        .collect();
    fs::write(
        path,
        serde_json::to_vec(&serde_json::json!({"version":"1.0", "actions":actions})).unwrap(),
    )
    .unwrap();
    let candidate_id = match client
        .execute(
            Command::ImportFunscript {
                path: path.to_path_buf(),
            },
            Some(base.project_id.clone()),
            Some(base.revision),
        )
        .unwrap()
    {
        ClientResponse::Candidate(candidate) => candidate.candidate_id,
        _ => panic!("expected a proposed script candidate"),
    };
    let edited = project(
        client
            .execute(
                Command::CommitCandidate { candidate_id },
                Some(base.project_id.clone()),
                Some(base.revision),
            )
            .unwrap(),
    );
    assert_eq!(edited.revision.value(), base.revision.value() + 1);
    assert_motion(&edited.program, positions);
    edited
}

fn assert_motion(program: &Value, positions: [u8; 4]) {
    let tracks = program["tracks"]
        .as_array()
        .expect("canonical motion tracks");
    let stroke = tracks
        .iter()
        .find(|track| track["axis"] == "stroke")
        .expect("stroke track");
    let actions = stroke["actions"].as_array().unwrap();
    assert_eq!(actions.len(), positions.len());
    for (index, (action, position)) in actions.iter().zip(positions).enumerate() {
        assert_eq!(action["time"].as_i64(), Some(index as i64 * 100_000_000));
        assert!((action["position"].as_f64().unwrap() - f64::from(position) / 100.0).abs() < 1e-12);
    }
}

struct SourceFixture {
    project: ProjectState,
    media: PathBuf,
    source_version: String,
    bytes: Vec<u8>,
}

fn source_project(client: &mut SessionClient, root: &Path) -> SourceFixture {
    let base = project(
        client
            .execute(
                Command::CreateProject {
                    name: "portable controller fixture".into(),
                },
                None,
                None,
            )
            .unwrap(),
    );
    let media = root.join("source.ppm");
    let bytes = b"P6
2 2
255
\xff\0\0\0\xff\0\0\0\xff\xff\xff\xff"
        .to_vec();
    fs::write(&media, &bytes).unwrap();
    let source_version = match client
        .execute(
            Command::ImportSource {
                path: media.clone(),
            },
            Some(base.project_id.clone()),
            None,
        )
        .unwrap()
    {
        ClientResponse::Source { source_version, .. } => source_version.as_str().to_owned(),
        _ => panic!("expected immutable source snapshot"),
    };
    let project = script_edit(
        client,
        &base,
        &root.join("initial.funscript"),
        [35, 45, 40, 50],
    );
    SourceFixture {
        project,
        media,
        source_version,
        bytes,
    }
}

// Independent, bounded inspection of the documented container layout. The
// controller already verifies the complete downloaded package; this checks the
// fixture's known source bytes and identity rather than trusting roundtrip alone.
fn assert_source_archive(path: &Path, source: &SourceFixture, expected_version: u16) {
    let bytes = fs::read(path).unwrap();
    assert!(
        bytes.len() >= 52 && bytes.len() < 2 * 1024 * 1024,
        "fixture package unexpectedly large"
    );
    assert_eq!(&bytes[..8], b"PULSPKG\0");
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        expected_version
    );
    assert_eq!(&bytes[10..12], &[0, 0]);
    let manifest_len =
        usize::try_from(u64::from_le_bytes(bytes[12..20].try_into().unwrap())).unwrap();
    let manifest_end = 52usize.checked_add(manifest_len).unwrap();
    assert!(manifest_end <= bytes.len());
    assert_eq!(
        &Sha256::digest(&bytes[52..manifest_end])[..],
        &bytes[20..52]
    );
    let manifest: Value = serde_json::from_slice(&bytes[52..manifest_end]).unwrap();
    assert_eq!(
        manifest["format_version"].as_u64(),
        Some(u64::from(expected_version))
    );
    let hash = format!("{:x}", Sha256::digest(&source.bytes));
    let row = manifest["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source_version"] == source.source_version)
        .expect("original source catalog entry");
    assert_eq!(row["kind"], "media");
    assert_eq!(row["identity"]["sha256"], hash);
    assert_eq!(
        row["identity"]["byte_len"].as_u64(),
        Some(source.bytes.len() as u64)
    );
    let mut cursor = manifest_end;
    let mut found = false;
    for object in manifest["objects"].as_array().unwrap() {
        let length = usize::try_from(object["byte_len"].as_u64().unwrap()).unwrap();
        let end = cursor.checked_add(length).unwrap();
        assert!(end <= bytes.len());
        if object["sha256"] == hash {
            assert_eq!(&bytes[cursor..end], source.bytes.as_slice());
            found = true;
        }
        cursor = end;
    }
    assert!(found, "source metadata must resolve exact embedded bytes");
    assert_eq!(cursor, bytes.len(), "container must have exact EOF");
    if expected_version == 2 {
        assert!(!manifest["imported_origins"].as_array().unwrap().is_empty());
    }
}

#[test]
fn portable_controller_relocates_source_and_reexports_real_cloned_edits() {
    let fixture = Fixture::new();
    let mut source_engine = Engine::start(&fixture.root, "source");
    let mut source_client = source_engine.client();
    let source = source_project(&mut source_client, &fixture.root);
    let mut source_driver = Driver::new(&source_engine);
    let export = fixture.root.join("original.pulsarpkg");
    source_driver.export(&source.project, &export);
    assert_source_archive(&export, &source, 1);

    let relocated_dir = fixture.root.join("relocated");
    fs::create_dir(&relocated_dir).unwrap();
    let relocated = relocated_dir.join("renamed.pulsarpkg");
    fs::rename(&export, &relocated).unwrap();
    source_engine.stop();
    fs::remove_file(&source.media).unwrap();
    fs::rename(
        &source_engine.state,
        fixture.root.join("offline-source-state"),
    )
    .unwrap();
    assert!(!export.exists() && !source.media.exists() && !source_engine.state.exists());

    let clone_a_engine = Engine::start(&fixture.root, "clone-a");
    let mut clone_a_client = clone_a_engine.client();
    let mut clone_a_driver = Driver::new(&clone_a_engine);
    let clone_a_id = clone_a_driver.import(&relocated);
    assert_ne!(clone_a_id, source.project.project_id);
    let cloned = snapshot(&mut clone_a_client, &clone_a_id);
    assert_eq!(cloned.revision, source.project.revision);
    assert_eq!(cloned.program, source.project.program);
    let edited = script_edit(
        &mut clone_a_client,
        &cloned,
        &fixture.root.join("local-edit.funscript"),
        [20, 55, 40, 60],
    );
    assert_ne!(edited.program, cloned.program);

    let reexport = relocated_dir.join("edited.pulsarpkg");
    clone_a_driver.export(&edited, &reexport);
    assert_source_archive(&reexport, &source, 2);

    let clone_b_engine = Engine::start(&fixture.root, "clone-b");
    let mut clone_b_client = clone_b_engine.client();
    let mut clone_b_driver = Driver::new(&clone_b_engine);
    let clone_b_id = clone_b_driver.import(&reexport);
    assert_ne!(clone_b_id, clone_a_id);
    assert_ne!(clone_b_id, source.project.project_id);
    let second = snapshot(&mut clone_b_client, &clone_b_id);
    assert_eq!(second.revision, edited.revision);
    assert_eq!(second.program, edited.program);
    assert_motion(&second.program, [20, 55, 40, 60]);
}

struct UploadAckBarrier {
    socket: PathBuf,
    upstream: PathBuf,
    held: Receiver<u64>,
    release: Option<Sender<()>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl UploadAckBarrier {
    fn install(engine: &Engine) -> Self {
        let socket = engine.state.join("bulk.sock");
        let upstream = engine.state.join("b.sock");
        fs::rename(&socket, &upstream).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (held_tx, held) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let target = upstream.clone();
        let worker = thread::spawn(move || {
            proxy_one_upload(listener, &target, held_tx, release_rx, &worker_stop)
        });
        Self {
            socket,
            upstream,
            held,
            release: Some(release),
            stop,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .expect("proxy thread must not panic")
                .expect("real bulk proxy must complete");
        }
    }
}

impl Drop for UploadAckBarrier {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
        let _ = fs::rename(&self.upstream, &self.socket);
    }
}

fn proxy_one_upload(
    listener: UnixListener,
    upstream: &Path,
    held: Sender<u64>,
    release: Receiver<()>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut client = loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match listener.accept() {
            Ok((client, _)) => break client,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("proxy accept deadline".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    // The private engine listener is already running. Nonblocking connect fails
    // honestly rather than hanging if this fixture's upstream is unavailable.
    let socket =
        Socket::new(Domain::UNIX, Type::STREAM, None).map_err(|error| error.to_string())?;
    socket
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    socket
        .connect(&SockAddr::unix(upstream).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    socket
        .set_nonblocking(false)
        .map_err(|error| error.to_string())?;
    let owned: std::os::fd::OwnedFd = socket.into();
    let mut engine = UnixStream::from(owned);
    for stream in [&client, &engine] {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
    }

    let handshake: PackageUploadHandshake =
        read_bulk_header(&mut client).map_err(|error| error.to_string())?;
    write_bulk_header(&mut engine, &handshake).map_err(|error| error.to_string())?;
    let ready: PackageUploadReply =
        read_bulk_header(&mut engine).map_err(|error| error.to_string())?;
    let declared_length = match &ready {
        PackageUploadReply::Ready { lease } => lease.package.byte_len,
        _ => return Err("real engine did not admit upload handshake".into()),
    };
    if declared_length > MAX_PACKAGE_UPLOAD_CHUNK_BYTES as u64 {
        return Err("barrier fixture must fit in one actual upload chunk".into());
    }
    write_bulk_header(&mut client, &ready).map_err(|error| error.to_string())?;
    let request: PackageUploadRequest =
        read_bulk_header(&mut client).map_err(|error| error.to_string())?;
    let bytes = read_artifact_chunk(&mut client).map_err(|error| error.to_string())?;
    write_bulk_header(&mut engine, &request).map_err(|error| error.to_string())?;
    write_artifact_chunk(&mut engine, &bytes).map_err(|error| error.to_string())?;
    let acknowledgement: PackageUploadReply =
        read_bulk_header(&mut engine).map_err(|error| error.to_string())?;
    let accepted = match &acknowledgement {
        PackageUploadReply::Uploaded { next_offset, .. } if *next_offset == declared_length => {
            *next_offset
        }
        _ => return Err("real engine did not acknowledge the complete fixture chunk".into()),
    };
    held.send(accepted).map_err(|error| error.to_string())?;
    release
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| error.to_string())?;
    write_bulk_header(&mut client, &acknowledgement).map_err(|error| error.to_string())?;
    Ok(())
}

fn assert_cancelled_transfer_result(result: Result<PortableReply, PortableFailure>) {
    match result {
        Ok(PortableReply::ImportStatus(status)) => {
            assert_ne!(status.state, PackageImportOperationState::Completed);
            assert!(status.receipt.is_none());
        }
        Err(_) => {}
        Ok(_) => panic!("cancelled transfer returned an unrelated successful reply"),
    }
}

#[test]
#[should_panic(expected = "cancelled transfer returned an unrelated successful reply")]
fn cancelled_transfer_assertion_rejects_unrelated_success() {
    assert_cancelled_transfer_result(Ok(PortableReply::PackagingAllowed {
        project_id: ProjectId::new("cancelled-transfer-unrelated-success").unwrap(),
    }));
}

#[test]
fn cancelled_transfer_assertion_accepts_unpublished_import_status() {
    let status = PackageImportStatus {
        operation_id: PackageImportOperationId::new("cancelled-transfer-status").unwrap(),
        request_id: RequestId::new("cancelled-transfer-start").unwrap(),
        package: PackageUploadDeclaration {
            format_version: 1,
            sha256: "a".repeat(64),
            byte_len: 1024,
        },
        state: PackageImportOperationState::AwaitingUpload,
        progress: PackageImportProgress {
            received_bytes: 0,
            verified_bytes: 0,
            total_bytes: 1024,
            completed_objects: 0,
            total_objects: None,
        },
        capture: None,
        receipt: None,
        error: None,
    };
    status.validate().expect("valid unpublished import status");
    assert_cancelled_transfer_result(Ok(PortableReply::ImportStatus(status)));
}

#[test]
fn portable_controller_control_lane_cancels_while_real_upload_ack_is_held() {
    let fixture = Fixture::new();
    let source_engine = Engine::start(&fixture.root, "source");
    let mut source_client = source_engine.client();
    let source = source_project(&mut source_client, &fixture.root);
    let mut source_driver = Driver::new(&source_engine);
    let archive = fixture.root.join("controller.pulsarpkg");
    source_driver.export(&source.project, &archive);

    let destination = Engine::start(&fixture.root, "destination");
    let _same_principal = destination.client();
    let mut driver = Driver::new(&destination);
    let operation_id = driver.start_import(&archive);
    let mut barrier = UploadAckBarrier::install(&destination);
    let transfer = driver.continue_import(operation_id.clone());
    let accepted = match barrier.held.recv_timeout(Duration::from_secs(15)) {
        Ok(accepted) => accepted,
        Err(error) => {
            barrier.stop.store(true, Ordering::Relaxed);
            if let Some(release) = barrier.release.take() {
                let _ = release.send(());
            }
            let proxy = barrier.worker.take().map(|worker| worker.join());
            panic!("real upload ACK barrier failed: {error}; proxy result: {proxy:?}");
        }
    };
    assert!(accepted > 0);
    assert!(driver.controller.transfer_busy());

    let query = driver.submit(PortableAction::ImportStatus {
        operation_id: operation_id.clone(),
    });
    match driver
        .wait(query, Duration::from_secs(5))
        .result
        .unwrap_or_else(|error| panic!("{}", error.message))
    {
        PortableReply::ImportStatus(status) => {
            assert!(status.receipt.is_none());
            assert_eq!(status.progress.received_bytes, accepted);
        }
        _ => panic!("control lane did not return the real import status"),
    }
    assert!(
        driver.controller.transfer_busy(),
        "control query must complete before the held transfer finishes"
    );

    let cancel = driver.submit(PortableAction::CancelImport {
        operation_id: operation_id.clone(),
    });
    match driver
        .wait(cancel, Duration::from_secs(5))
        .result
        .unwrap_or_else(|error| panic!("{}", error.message))
    {
        PortableReply::ImportStatus(status) => {
            assert_eq!(status.state, PackageImportOperationState::Cancelled);
            assert!(status.receipt.is_none());
            assert_cancelled_transfer_result(Ok(PortableReply::ImportStatus(status)));
        }
        _ => panic!("control lane did not cancel the actual import"),
    }
    assert!(
        driver.controller.transfer_busy(),
        "cancellation must be received before releasing the bulk ACK"
    );
    barrier.finish();
    assert_cancelled_transfer_result(driver.wait(transfer, WAIT).result);
    match driver.run(PortableAction::ImportStatus { operation_id }) {
        PortableReply::ImportStatus(status) => {
            assert_eq!(status.state, PackageImportOperationState::Cancelled);
            assert!(status.receipt.is_none());
        }
        _ => panic!("cancelled import must retain its terminal receipt"),
    }
}
