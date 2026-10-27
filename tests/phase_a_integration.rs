//! Linux subprocess qualification of the enabled architecture path. These tests
//! exercise the shipped executable and the same SessionClient used by Desktop;
//! they do not qualify detector accuracy, other operating systems, or hardware.
#![cfg(target_os = "linux")]

use anyhow::{bail, Context, Result};
use pulsar_clients::session::{EngineApi, SessionClient};
use pulsar_clients::{
    CandidateSnapshot as ClientCandidateSnapshot, ProjectSnapshot as ClientProjectSnapshot,
    ResponseBody as ClientResponseBody,
};
use pulsar_protocol::*;
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const RPC_TIMEOUT: Duration = Duration::from_secs(120);
const PROCESS_TIMEOUT: Duration = Duration::from_secs(150);

fn unique_id(prefix: &str) -> RequestId {
    RequestId::new(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
    .unwrap()
}

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    child: Option<Child>,
}

impl Fixture {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root =
            std::env::temp_dir().join(format!("pulsar-process-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)?;
        let mut fixture = Self {
            state: root.join("state"),
            root,
            child: None,
        };
        fixture.start()?;
        Ok(fixture)
    }

    fn endpoint(&self) -> PathBuf {
        self.state.join("engine.sock")
    }

    fn start(&mut self) -> Result<()> {
        let stderr = File::create(self.root.join("engine.stderr"))?;
        self.child = Some(
            ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
                .arg("engine")
                .env("PULSAR_STATE_DIR", &self.state)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(stderr)
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = self.child.as_mut().unwrap().try_wait()? {
                bail!(
                    "engine exited during startup ({status}): {}",
                    fs::read_to_string(self.root.join("engine.stderr"))?
                );
            }
            if std::os::unix::net::UnixStream::connect(self.endpoint()).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "engine startup deadline exceeded; diagnostics at {}",
                    self.root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        Ok(())
    }

    fn restart(&mut self) -> Result<()> {
        self.stop()?;
        self.start()
    }

    fn client(&self) -> Result<SessionClient> {
        SessionClient::connect(&self.endpoint(), &self.state.join("pairing.token"))
    }

    fn call(&self, request: &Request) -> Result<Response> {
        let mut connection = LocalClient::connect(self.endpoint(), RPC_TIMEOUT)?;
        let response = connection.call(request)?;
        assert_eq!(response.version, PROTOCOL_VERSION);
        assert_eq!(response.request_id, request.request_id);
        Ok(response)
    }

    fn pair(&self, name: &str) -> Result<Actor> {
        let response = self
            .call(&Request::new(
                unique_id("pair"),
                Command::Pair {
                    client_name: name.to_owned(),
                    pairing_token: fs::read_to_string(self.state.join("pairing.token"))?
                        .trim()
                        .to_owned(),
                },
            ))?
            .result?;
        let ResponseBody::Paired {
            session,
            auth_token,
        } = response
        else {
            bail!("pairing returned {response:?}");
        };
        assert_ne!(
            session.as_str(),
            auth_token,
            "public session identity must not be an authentication secret"
        );
        Ok(Actor {
            session,
            auth_token,
        })
    }

    fn run(
        &self,
        command: &mut ProcessCommand,
        label: &str,
    ) -> Result<(ExitStatus, String, String)> {
        let stdout_path = self.root.join(format!("{label}.stdout"));
        let stderr_path = self.root.join(format!("{label}.stderr"));
        command
            .stdin(Stdio::null())
            .stdout(File::create(&stdout_path)?)
            .stderr(File::create(&stderr_path)?);
        let mut child = OwnedChild(command.spawn()?);
        let deadline = Instant::now() + PROCESS_TIMEOUT;
        let status = loop {
            if let Some(status) = child.0.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                bail!(
                    "{label} exceeded subprocess deadline; diagnostics at {}",
                    self.root.display()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        Ok((
            status,
            fs::read_to_string(stdout_path)?,
            fs::read_to_string(stderr_path)?,
        ))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop();
        if std::thread::panicking() {
            eprintln!(
                "Preserved failing subprocess evidence: {}",
                self.root.display()
            );
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

struct Actor {
    session: SessionId,
    auth_token: String,
}
impl Actor {
    fn request(
        &self,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Request {
        let mut request = Request::new(unique_id("request"), command)
            .with_session(self.session.clone())
            .with_auth_token(self.auth_token.clone());
        request.project = project;
        request.expected_revision = revision;
        request
    }

    fn execute(
        &self,
        fixture: &Fixture,
        command: Command,
        project: Option<ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        Ok(fixture
            .call(&self.request(command, project, revision))?
            .result?)
    }
}

fn wire_project(response: ResponseBody) -> Result<ProjectSnapshot> {
    if let ResponseBody::Project(project) = response {
        Ok(*project)
    } else {
        bail!("expected project, got {response:?}")
    }
}

fn project(response: ClientResponseBody) -> Result<ClientProjectSnapshot> {
    if let ClientResponseBody::Project(project) = response {
        Ok(*project)
    } else {
        bail!("expected hydrated project, got {response:?}")
    }
}

impl Actor {
    // Exercise values-only upload and normal commit authority. The fixture does
    // not insert trusted candidates directly or restore the removed inline API.
    fn edit_request(
        &self,
        fixture: &Fixture,
        project_id: &ProjectId,
        revision: RevisionId,
        program: &MotionProgram,
        label: &str,
    ) -> Result<Request> {
        let mut api = SessionClient::from_credentials(
            &fixture.endpoint(),
            self.session.clone(),
            self.auth_token.clone(),
        )?;
        let candidate = api.upload_edit(project_id.clone(), revision, program, label)?;
        Ok(self.request(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(project_id.clone()),
            Some(revision),
        ))
    }
}

fn expect_code(response: Response, code: ErrorCode) {
    assert_eq!(
        response
            .result
            .expect_err("operation must be rejected")
            .code,
        code
    );
}

fn motion(offset: f64) -> MotionProgram {
    let actions = [(0, 0.35), (500_000_000, 0.55), (1_000_000_000, 0.4)]
        .into_iter()
        .map(|(time, position)| {
            MotionAction::new(
                ProjectTime::from_nanos(time),
                NormalizedPosition::new(position + offset).unwrap(),
                EvidenceKind::Synthesized,
            )
            .unwrap()
        })
        .collect();
    MotionProgram::new(vec![MotionTrack::new(Axis::Stroke, actions).unwrap()]).unwrap()
}

/// The media is generated here, not downloaded or borrowed from a developer's
/// cache. A translating textured patch creates actual changing decoded pixels.
fn media_fixture(fixture: &Fixture) -> Result<PathBuf> {
    let frames = fixture.root.join("frames");
    fs::create_dir(&frames)?;
    let (width, height) = (96usize, 96usize);
    for index in 0..24 {
        let mut file = File::create(frames.join(format!("frame-{index:03}.ppm")))?;
        write!(file, "P6\n{width} {height}\n255\n")?;
        let mut pixels = vec![24u8; width * height * 3];
        for y in 0..32 {
            for x in 0..32 {
                let intensity = 75 + ((x * 17 + y * 11 + x * y) % 155) as u8;
                let start = ((y + 18 + index) * width + x + 28) * 3;
                pixels[start..start + 3].fill(intensity);
            }
        }
        file.write_all(&pixels)?;
    }
    let path = fixture.root.join("moving.mkv");
    let (status, _, stderr) = fixture.run(
        ProcessCommand::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-threads",
                "1",
                "-filter_threads",
                "1",
                "-framerate",
                "12",
                "-i",
            ])
            .arg(frames.join("frame-%03d.ppm"))
            .args([
                "-c:v",
                "ffv1",
                "-threads",
                "1",
                "-fps_mode",
                "passthrough",
                "-y",
            ])
            .arg(&path),
        "fixture-encode",
    )?;
    assert!(status.success(), "media fixture encoding failed: {stderr}");
    Ok(path)
}

fn neutral_export(path: &Path) -> Result<Value> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let actions = value["actions"]
        .as_array()
        .context("neutral export has no actions")?;
    assert!(
        actions.len() > 1,
        "actual decoded motion must reach the exported neutral program"
    );
    let mut previous = None;
    for action in actions {
        let object = action.as_object().context("action must be an object")?;
        assert_eq!(
            object.len(),
            2,
            "neutral action must strip internal lineage/evidence"
        );
        let time = action["at"]
            .as_u64()
            .context("invalid exported timestamp")?;
        let position = action["pos"]
            .as_u64()
            .context("invalid exported position")?;
        assert!(position <= 100);
        assert!(previous.is_none_or(|previous| time > previous));
        previous = Some(time);
    }
    let serialized = serde_json::to_string(&value)?;
    for private_field in [
        "source_version",
        "attempt_id",
        "provenance",
        "auth_token",
        "evidence",
    ] {
        assert!(
            !serialized.contains(private_field),
            "neutral export leaked {private_field}"
        );
    }
    Ok(value)
}

fn completed_candidate(
    api: &mut impl EngineApi,
    project_id: &ProjectId,
    mut job: JobSnapshot,
) -> Result<ClientCandidateSnapshot> {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    loop {
        if let Some(error) = job.error {
            bail!("real worker failed in state {}: {error}", job.state);
        }
        if let Some(candidate_id) = job.candidate_id {
            return match api.execute(
                Command::GetCandidate { candidate_id },
                Some(project_id.clone()),
                None,
            )? {
                ClientResponseBody::Candidate(candidate) => Ok(*candidate),
                response => bail!("expected candidate, got {response:?}"),
            };
        }
        if matches!(job.state.as_str(), "cancelled" | "failed" | "interrupted") {
            bail!("job ended without a candidate: {}", job.state);
        }
        if Instant::now() >= deadline {
            bail!("real worker did not finish within process-test deadline");
        }
        std::thread::sleep(Duration::from_millis(50));
        job = match api.execute(
            Command::JobStatus { job_id: job.job_id },
            Some(project_id.clone()),
            None,
        )? {
            ClientResponseBody::Job(job) => job,
            response => bail!("expected job, got {response:?}"),
        };
    }
}

#[test]
fn actual_clients_worker_and_durable_authority_cross_process() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let result = exercise_architecture(&mut fixture);
    if result.is_err() {
        // Preserve the process logs on any propagated error, not only a panic.
        eprintln!("Subprocess diagnostics: {}", fixture.root.display());
        let preserved = fixture.root.with_extension("failed");
        fixture.stop()?;
        fs::rename(&fixture.root, &preserved)?;
        eprintln!("Failure evidence retained at {}", preserved.display());
    }
    result
}

fn exercise_architecture(fixture: &mut Fixture) -> Result<()> {
    eprintln!("A6 process gate: real client/import/worker/candidate/revision/export");
    let media = media_fixture(fixture)?;
    let mut gui_api = fixture.client()?;
    let initial = project(gui_api.execute(
        Command::CreateProject {
            name: "GUI shared API fixture".into(),
        },
        None,
        None,
    )?)?;
    let project_id = initial.project_id.clone();
    let source_version = match gui_api.execute(
        Command::ImportSource {
            path: media.clone(),
        },
        Some(project_id.clone()),
        Some(initial.revision),
    )? {
        ClientResponseBody::Source { source_version } => source_version,
        response => bail!("expected source, got {response:?}"),
    };
    let imported =
        project(gui_api.execute(Command::GetSnapshot, Some(project_id.clone()), None)?)?;
    let job = match gui_api.execute(
        Command::Generate {
            source_version: source_version.clone(),
            preset: "fast".into(),
            settings: Some(GenerationSettings::default()),
            model_path: None,
        },
        Some(project_id.clone()),
        Some(imported.revision),
    )? {
        ClientResponseBody::Job(job) => job,
        response => bail!("expected job, got {response:?}"),
    };
    let candidate = completed_candidate(&mut gui_api, &project_id, job)?;
    assert_eq!(candidate.base_revision, imported.revision);
    assert!(!candidate.program.tracks().is_empty());
    assert!(
        candidate
            .program
            .tracks()
            .iter()
            .flat_map(|track| track.actions())
            .all(|action| action.evidence() != EvidenceKind::Observed),
        "global pixel motion is inferred evidence, not detector observation"
    );

    let concurrent = gui_api.upload_edit(
        project_id.clone(),
        imported.revision,
        &motion(0.0),
        "Concurrent gesture",
    )?;
    let edited = project(gui_api.execute(
        Command::CommitCandidate {
            candidate_id: concurrent.candidate_id,
        },
        Some(project_id.clone()),
        Some(imported.revision),
    )?)?;
    let stale = gui_api
        .execute(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(project_id.clone()),
            Some(imported.revision),
        )
        .expect_err("stale candidate must require an explicit rebase");
    assert_eq!(
        stale
            .downcast_ref::<ProtocolError>()
            .context("expected typed revision error")?
            .code,
        ErrorCode::RevisionConflict
    );
    // Replacing the concurrent author's protected motion requires an explicit owner unlock.
    let edited = project(gui_api.execute(
        Command::SetProtectedRegions { regions: vec![] },
        Some(project_id.clone()),
        Some(edited.revision),
    )?)?;
    let rebased = match gui_api.execute(
        Command::RebaseCandidate {
            candidate_id: candidate.candidate_id,
        },
        Some(project_id.clone()),
        Some(edited.revision),
    )? {
        ClientResponseBody::Candidate(candidate) => candidate,
        response => bail!("expected rebased candidate, got {response:?}"),
    };
    let committed = project(gui_api.execute(
        Command::CommitCandidate {
            candidate_id: rebased.candidate_id,
        },
        Some(project_id.clone()),
        Some(edited.revision),
    )?)?;
    let exported = fixture.root.join("gui-api.funscript");
    let response = gui_api.execute(
        Command::Export {
            path: exported.clone(),
        },
        Some(project_id.clone()),
        Some(committed.revision),
    )?;
    assert!(
        matches!(response, ClientResponseBody::Exported { revision, .. } if revision == committed.revision)
    );
    let gui_export = neutral_export(&exported)?;

    eprintln!("A6 process gate: resolved preview identity and late-result rejection");
    let context = FrameContext {
        source_version,
        source_placement: SourcePlacementId::new("integration-placement")?,
        frame: FrameId::new(0),
        transform: TransformId::new("source-identity")?,
        seek_generation: 1,
        request_generation: 2,
    };
    let requested_time = SourceTimestamp::new(1, 2)?;
    let preview = match gui_api.execute(
        Command::PreviewFrame {
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: None,
            context: context.clone(),
            source_time: requested_time,
            model_path: None,
            model_input: None,
            inference_profile: None,
        },
        Some(project_id.clone()),
        None,
    )? {
        ClientResponseBody::Preview(preview) => preview,
        response => bail!("expected decoded preview, got {response:?}"),
    };
    assert!(preview.matches_seek_request(&context, &requested_time));
    assert!(preview.matches_request(&preview.context));
    assert_eq!(preview.context.frame.value(), preview.source_frame_index);
    let mut newer_request = context.clone();
    newer_request.request_generation += 1;
    assert!(!preview.matches_seek_request(&newer_request, &requested_time));
    assert!(
        !preview.matches_request(&context),
        "resolved seek frame is not the caller's old displayed frame"
    );
    assert!(
        preview.observations.is_empty(),
        "absence of a detector must not become a decorative tracking box"
    );
    assert!(
        preview.frame.is_some(),
        "real decoded media must be delivered to the viewer"
    );

    eprintln!("A6 process gate: actual CLI uses the same durable worker path");
    let cli_export_path = fixture.root.join("cli.funscript");
    let (status, stdout, stderr) = fixture.run(
        ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .env("PULSAR_STATE_DIR", &fixture.state)
            .arg("generate")
            .arg(&media)
            .arg("--profile")
            .arg("fast")
            .arg("--output")
            .arg(&cli_export_path),
        "cli-generate",
    )?;
    assert!(status.success(), "actual CLI generation failed: {stderr}");
    assert!(
        stdout.contains("exported"),
        "CLI did not report an engine export receipt: {stdout}"
    );
    let cli_export = neutral_export(&cli_export_path)?;
    assert_eq!(
        gui_export["actions"], cli_export["actions"],
        "identical CPU inputs must produce identical CLI/Desktop actions"
    );

    eprintln!("A6 process gate: durable request dedupe and hard crash/restart");
    let owner = fixture.pair("integration owner")?;
    let authority_project = wire_project(owner.execute(
        fixture,
        Command::CreateProject {
            name: "Authority fixture".into(),
        },
        None,
        None,
    )?)?;
    let authority_id = authority_project.project_id.clone();
    let edit_request = owner.edit_request(
        fixture,
        &authority_id,
        authority_project.revision,
        &motion(0.0),
        "Durable gesture",
    )?;
    let once = wire_project(fixture.call(&edit_request)?.result?)?;
    let repeated = wire_project(fixture.call(&edit_request)?.result?)?;
    assert_eq!(once.revision, repeated.revision);
    let mut conflicting = edit_request.clone();
    conflicting.command = owner
        .edit_request(
            fixture,
            &authority_id,
            once.revision,
            &motion(0.1),
            "Different bytes",
        )?
        .command;
    expect_code(fixture.call(&conflicting)?, ErrorCode::RequestConflict);
    fixture.restart()?;
    let mut reconnected_gui = fixture.client()?;
    let restored =
        project(reconnected_gui.execute(Command::GetSnapshot, Some(project_id.clone()), None)?)?;
    assert_eq!(restored.revision, committed.revision);
    assert_eq!(restored.program, committed.program);
    let replayed = wire_project(fixture.call(&edit_request)?.result?)?;
    assert_eq!(
        replayed.revision, once.revision,
        "crash/restart must not turn replay into a second edit"
    );

    // This scoped-authority fixture starts from an explicitly unlocked owner baseline.
    let once = wire_project(owner.execute(
        fixture,
        Command::SetProtectedRegions { regions: vec![] },
        Some(authority_id.clone()),
        Some(once.revision),
    )?)?;

    eprintln!("A6 process gate: scoped protection history, revocation and credential isolation");
    let editor = fixture.pair("scoped integration editor")?;
    let observer = fixture.pair("scoped integration observer")?;
    owner.execute(
        fixture,
        Command::Grant {
            session: editor.session.clone(),
            project_id: authority_id.clone(),
            scopes: vec![Scope::Read, Scope::Edit],
        },
        None,
        None,
    )?;
    owner.execute(
        fixture,
        Command::Grant {
            session: observer.session.clone(),
            project_id: authority_id.clone(),
            scopes: vec![Scope::Read],
        },
        None,
        None,
    )?;
    let region = ProtectedRegion {
        axis: Some(Axis::Stroke),
        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_100_000_000))?,
    };
    let protected = wire_project(owner.execute(
        fixture,
        Command::SetProtectedRegions {
            regions: vec![region],
        },
        Some(authority_id.clone()),
        Some(once.revision),
    )?)?;
    expect_code(
        fixture.call(&editor.request(
            Command::SetProtectedRegions { regions: vec![] },
            Some(authority_id.clone()),
            Some(protected.revision),
        ))?,
        ErrorCode::Forbidden,
    );
    expect_code(
        fixture.call(&editor.request(
            Command::ImportSource { path: media },
            Some(authority_id.clone()),
            None,
        ))?,
        ErrorCode::Forbidden,
    );
    expect_code(
        fixture.call(&editor.edit_request(
            fixture,
            &authority_id,
            protected.revision,
            &motion(0.1),
            "Protected edit",
        )?)?,
        ErrorCode::Forbidden,
    );
    let undone = wire_project(owner.execute(
        fixture,
        Command::Undo,
        Some(authority_id.clone()),
        Some(protected.revision),
    )?)?;
    let redone = wire_project(owner.execute(
        fixture,
        Command::Redo,
        Some(authority_id.clone()),
        Some(undone.revision),
    )?)?;
    expect_code(
        fixture.call(&editor.edit_request(
            fixture,
            &authority_id,
            redone.revision,
            &motion(0.1),
            "Protection survived redo",
        )?)?,
        ErrorCode::Forbidden,
    );
    let unprotected = wire_project(owner.execute(
        fixture,
        Command::Undo,
        Some(authority_id.clone()),
        Some(redone.revision),
    )?)?;
    let allowed_request = editor.edit_request(
        fixture,
        &authority_id,
        unprotected.revision,
        &motion(0.1),
        "Undo removed protection",
    )?;
    let allowed = wire_project(fixture.call(&allowed_request)?.result?)?;
    assert!(allowed.revision > unprotected.revision);

    let separate_project = wire_project(editor.execute(
        fixture,
        Command::CreateProject {
            name: "Unrelated editor project".into(),
        },
        None,
        None,
    )?)?;
    owner.execute(
        fixture,
        Command::Revoke {
            session: editor.session.clone(),
            project_id: authority_id.clone(),
        },
        None,
        None,
    )?;
    expect_code(
        fixture.call(&editor.request(Command::GetSnapshot, Some(authority_id.clone()), None))?,
        ErrorCode::Forbidden,
    );
    let events = observer.execute(
        fixture,
        Command::Events {
            after_cursor: None,
            limit: 100,
        },
        Some(authority_id.clone()),
        None,
    )?;
    let serialized_events = serde_json::to_string(&events)?;
    assert!(
        !serialized_events.contains(&editor.auth_token),
        "revocation event leaked another client's credential"
    );
    let mut stolen_public_identity = observer.request(
        Command::GetSnapshot,
        Some(separate_project.project_id.clone()),
        None,
    );
    stolen_public_identity.session = Some(editor.session.clone());
    expect_code(
        fixture.call(&stolen_public_identity)?,
        ErrorCode::Unauthorized,
    );
    stolen_public_identity.auth_token = None;
    expect_code(
        fixture.call(&stolen_public_identity)?,
        ErrorCode::Unauthorized,
    );
    editor.execute(
        fixture,
        Command::GetSnapshot,
        Some(separate_project.project_id),
        None,
    )?;
    let no_effects = owner.execute(fixture, Command::Capabilities, None, None)?;
    assert!(matches!(
        no_effects,
        ResponseBody::Capabilities(Capabilities {
            physical_playback: false,
            ..
        })
    ));
    eprintln!("A6 process gate passed: real CPU media path, IPC authority, persistence and scoped effects; no neural accuracy or physical qualification claim");
    Ok(())
}

#[test]
fn desktop_shared_api_elapsed_seek_preserves_nonzero_source_origin_after_restart() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let original = media_fixture(&fixture)?;
    let shifted = fixture.root.join("shifted.mkv");
    let (status, _, stderr) = fixture.run(
        ProcessCommand::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-itsoffset", "5", "-i"])
            .arg(&original)
            .args(["-c", "copy"])
            .arg(&shifted),
        "offset-remux",
    )?;
    assert!(status.success(), "offset remux failed: {stderr}");
    let mut client = fixture.client()?;
    let p = project(client.execute(
        Command::CreateProject {
            name: "elapsed-seek".into(),
        },
        None,
        None,
    )?)?;
    let source = match client.execute(
        Command::ImportSource { path: shifted },
        Some(p.project_id.clone()),
        Some(p.revision),
    )? {
        ClientResponseBody::Source { source_version } => source_version,
        other => bail!("unexpected import: {other:?}"),
    };
    let context = FrameContext {
        source_version: source,
        source_placement: SourcePlacementId::new("offset-primary")?,
        frame: FrameId::new(0),
        transform: TransformId::new("source-identity")?,
        seek_generation: 3,
        request_generation: 4,
    };
    let fetch = |client: &mut SessionClient,
                 basis: PreviewTimeBasis,
                 time: SourceTimestamp|
     -> Result<PreviewResult> {
        match client.execute(
            Command::PreviewFrame {
                time_basis: basis,
                video_view: None,
                context: context.clone(),
                source_time: time,
                model_path: None,
                model_input: None,
                inference_profile: None,
            },
            Some(p.project_id.clone()),
            None,
        )? {
            ClientResponseBody::Preview(result) => {
                assert!(result.matches_timed_seek_request(&context, &time, basis));
                Ok(*result)
            }
            other => bail!("unexpected preview: {other:?}"),
        }
    };
    let elapsed = fetch(
        &mut client,
        PreviewTimeBasis::ElapsedFromFirstFrame,
        SourceTimestamp::new(1, 2)?,
    )?;
    let absolute = fetch(
        &mut client,
        PreviewTimeBasis::SourcePts,
        SourceTimestamp::new(11, 2)?,
    )?;
    assert_eq!(elapsed.source_frame_index, 6);
    assert_eq!(elapsed.source_frame_index, absolute.source_frame_index);
    assert_eq!(
        elapsed.frame.as_ref().unwrap().artifact.identity.sha256,
        absolute.frame.as_ref().unwrap().artifact.identity.sha256
    );
    assert_eq!(
        elapsed.source_time.numerator() * 2,
        i64::from(elapsed.source_time.denominator()) * 11
    );
    let origin = elapsed.source_origin.unwrap();
    assert_eq!(origin.numerator(), i64::from(origin.denominator()) * 5);
    let elapsed_time = elapsed.elapsed_source_time.unwrap();
    assert_eq!(
        elapsed_time.numerator() * 2,
        i64::from(elapsed_time.denominator())
    );
    fixture.restart()?;
    let mut reconnected = fixture.client()?;
    reconnected.execute(
        Command::OpenProject {
            project_id: p.project_id.clone(),
        },
        None,
        None,
    )?;
    let recovered = fetch(
        &mut reconnected,
        PreviewTimeBasis::ElapsedFromFirstFrame,
        SourceTimestamp::new(1, 2)?,
    )?;
    assert_eq!(recovered.source_frame_index, elapsed.source_frame_index);
    assert_eq!(recovered.source_origin, elapsed.source_origin);
    assert_eq!(recovered.elapsed_source_time, elapsed.elapsed_source_time);
    Ok(())
}

#[test]
fn source_placement_shared_api_cli_preview_history_and_restart_preserve_motion() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let media = media_fixture(&fixture)?;
    let mut client = fixture.client()?;
    let initial = project(client.execute(
        Command::CreateProject {
            name: "placed media".into(),
        },
        None,
        None,
    )?)?;
    let source = match client.execute(
        Command::ImportSource { path: media },
        Some(initial.project_id.clone()),
        Some(initial.revision),
    )? {
        ClientResponseBody::Source { source_version } => source_version,
        other => bail!("unexpected import {other:?}"),
    };
    let proposed = client.upload_edit(
        initial.project_id.clone(),
        initial.revision,
        &motion(0.0),
        "Authored motion",
    )?;
    let committed = project(client.execute(
        Command::CommitCandidate {
            candidate_id: proposed.candidate_id,
        },
        Some(initial.project_id.clone()),
        Some(initial.revision),
    )?)?;
    let base = project(client.execute(
        Command::SetProtectedRegions {
            regions: vec![ProtectedRegion {
                axis: Some(Axis::Stroke),
                range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_100_000_000))?,
            }],
        },
        Some(initial.project_id.clone()),
        Some(committed.revision),
    )?)?;
    let spec = SourcePlacementSpec {
        source_version: source,
        source_start: SourceTimestamp::new(0, 1)?,
        source_end: SourceTimestamp::new(2, 1)?,
        source_anchor: SourceTimestamp::new(1, 2)?,
        project_anchor: ProjectTime::from_nanos(2_000_000_000),
        rate_numerator: 1,
        rate_denominator: 1,
    };
    let spec_path = fixture.root.join("placement.json");
    fs::write(&spec_path, serde_json::to_vec(&spec)?)?;
    let (status, stdout, stderr) = fixture.run(
        ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .env("PULSAR_STATE_DIR", &fixture.state)
            .args(["project", "place", initial.project_id.as_str(), "--spec"])
            .arg(&spec_path),
        "cli-place",
    )?;
    assert!(status.success(), "CLI placement failed: {stderr}");
    assert!(stdout.contains("placement_id"));
    let placed =
        project(client.execute(Command::GetSnapshot, Some(initial.project_id.clone()), None)?)?;
    assert_eq!(placed.motion.program, base.motion.program);
    assert_eq!(placed.placements.len(), 1);
    let old = placed.placements[0].clone();
    let make_context = |p: &SourcePlacement| FrameContext {
        source_version: p.spec.source_version.clone(),
        source_placement: p.placement_id.clone(),
        frame: FrameId::new(0),
        transform: TransformId::new("source-identity").unwrap(),
        seek_generation: 4,
        request_generation: 5,
    };
    let context = make_context(&old);
    let seek = PlacedPreviewRequest {
        placement: old.clone(),
        project_time: spec.project_anchor,
    };
    let result = match client.execute(
        Command::PreviewPlacedFrame {
            placement_id: old.placement_id.clone(),
            project_time: spec.project_anchor,
            context: context.clone(),
            video_view: None,
            model_path: None,
            model_input: None,
            inference_profile: None,
        },
        Some(initial.project_id.clone()),
        None,
    )? {
        ClientResponseBody::Preview(result) => result,
        other => bail!("unexpected preview {other:?}"),
    };
    assert!(result.matches_placed_seek_request(&context, &seek));
    assert_eq!(result.source_frame_index, 6);
    assert_eq!(
        result.placed.as_ref().unwrap().actual_project_time,
        ExactSeconds::new(2, 1)?
    );
    let (status, stdout, stderr) = fixture.run(
        ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .env("PULSAR_STATE_DIR", &fixture.state)
            .args([
                "project",
                "preview-placement",
                initial.project_id.as_str(),
                old.placement_id.as_str(),
                "--time-ns",
                "2000000000",
            ]),
        "cli-preview-placement",
    )?;
    assert!(status.success(), "CLI placed preview failed: {stderr}");
    assert!(stdout.contains("actual_project_time"));
    let mut changed = spec.clone();
    changed.project_anchor = ProjectTime::from_nanos(3_000_000_000);
    changed.source_anchor = SourceTimestamp::new(3, 2)?;
    changed.rate_numerator = -1;
    let replaced = project(client.execute(
        Command::SetSourcePlacement {
            spec: changed.clone(),
            replace: Some(old.placement_id.clone()),
        },
        Some(initial.project_id.clone()),
        Some(placed.revision),
    )?)?;
    assert_ne!(replaced.placements[0].placement_id, old.placement_id);
    assert_eq!(replaced.motion.program, base.motion.program);
    let stale = client
        .execute(
            Command::PreviewPlacedFrame {
                placement_id: old.placement_id.clone(),
                project_time: spec.project_anchor,
                context,
                video_view: None,
                model_path: None,
                model_input: None,
                inference_profile: None,
            },
            Some(initial.project_id.clone()),
            None,
        )
        .unwrap_err();
    assert_eq!(
        stale.downcast_ref::<ProtocolError>().unwrap().code,
        ErrorCode::NotFound
    );
    let restored = project(client.execute(
        Command::Undo,
        Some(initial.project_id.clone()),
        Some(replaced.revision),
    )?)?;
    assert_eq!(restored.placements, placed.placements);
    assert_eq!(restored.motion.program, base.motion.program);
    let redone = project(client.execute(
        Command::Redo,
        Some(initial.project_id.clone()),
        Some(restored.revision),
    )?)?;
    assert_eq!(redone.placements, replaced.placements);
    fixture.restart()?;
    let mut connected = fixture.client()?;
    let recovered = project(connected.execute(
        Command::GetSnapshot,
        Some(initial.project_id.clone()),
        None,
    )?)?;
    assert_eq!(recovered.placements, replaced.placements);
    assert_eq!(recovered.motion.program, base.motion.program);
    let new = &recovered.placements[0];
    let context = make_context(new);
    let expected = PlacedPreviewRequest {
        placement: new.clone(),
        project_time: changed.project_anchor,
    };
    let result = match connected.execute(
        Command::PreviewPlacedFrame {
            placement_id: new.placement_id.clone(),
            project_time: changed.project_anchor,
            context: context.clone(),
            video_view: None,
            model_path: None,
            model_input: None,
            inference_profile: None,
        },
        Some(initial.project_id.clone()),
        None,
    )? {
        ClientResponseBody::Preview(result) => result,
        other => bail!("unexpected preview {other:?}"),
    };
    assert!(result.matches_placed_seek_request(&context, &expected));
    assert_eq!(result.source_frame_index, 18);
    let mut forged = new.clone();
    forged.spec.project_anchor = ProjectTime::from_nanos(9_000_000_000);
    let error = connected
        .execute(
            Command::Generate {
                source_version: new.spec.source_version.clone(),
                preset: "fast".into(),
                settings: Some(GenerationSettings {
                    source_placement: Some(forged),
                    ..Default::default()
                }),
                model_path: None,
            },
            Some(initial.project_id.clone()),
            Some(recovered.revision),
        )
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<ProtocolError>().unwrap().code,
        ErrorCode::DependencyMismatch
    );
    let (status, stdout, stderr) = fixture.run(
        ProcessCommand::new(env!("CARGO_BIN_EXE_pulsar"))
            .env("PULSAR_STATE_DIR", &fixture.state)
            .args([
                "project",
                "generate-placement",
                initial.project_id.as_str(),
                new.placement_id.as_str(),
            ]),
        "cli-generate-placement",
    )?;
    assert!(status.success(), "CLI placed generation failed: {stderr}");
    let response: serde_json::Value = serde_json::from_str(&stdout)?;
    assert_eq!(response["kind"], "job");
    let job: JobSnapshot = serde_json::from_value(response["value"].clone())?;
    let candidate = completed_candidate(&mut connected, &initial.project_id, job)?;
    assert_eq!(candidate.base_revision, recovered.revision);
    let track = candidate
        .program
        .track(Axis::Stroke)
        .context("placed candidate stroke")?;
    assert!(!track.actions().is_empty());
    assert!(track
        .actions()
        .windows(2)
        .all(|p| p[0].time() < p[1].time()));
    assert!(track
        .actions()
        .iter()
        .all(|a| a.time() >= ProjectTime::from_nanos(2_500_000_000)
            && a.time() <= ProjectTime::from_nanos(4_500_000_000)));
    let unchanged = project(connected.execute(
        Command::GetSnapshot,
        Some(initial.project_id.clone()),
        None,
    )?)?;
    assert_eq!(unchanged.revision, recovered.revision);
    assert_eq!(unchanged.motion.program, base.motion.program);
    Ok(())
}

#[test]
#[ignore = "requires an explicitly pinned historical schema-2 executable"]
fn historical_schema2_reader_refuses_placement_database_without_losing_membership() -> Result<()> {
    historical_reader_database_refusal("PULSAR_SCHEMA2_TEST_BINARY")
}
#[test]
#[ignore = "requires an explicitly pinned historical schema-3 executable"]
fn historical_schema3_reader_refuses_timing_proof_database_without_losing_membership() -> Result<()>
{
    historical_reader_database_refusal("PULSAR_SCHEMA3_TEST_BINARY")
}
fn historical_reader_database_refusal(variable: &str) -> Result<()> {
    let reader = PathBuf::from(
        std::env::var_os(variable).context("pin exact historical reader executable")?,
    );
    let mut fixture = Fixture::new()?;
    let media = media_fixture(&fixture)?;
    let mut client = fixture.client()?;
    let p = project(client.execute(
        Command::CreateProject {
            name: "reader boundary".into(),
        },
        None,
        None,
    )?)?;
    let source = match client.execute(
        Command::ImportSource { path: media },
        Some(p.project_id.clone()),
        None,
    )? {
        ClientResponseBody::Source { source_version } => source_version,
        other => bail!("unexpected import {other:?}"),
    };
    let spec = SourcePlacementSpec {
        source_version: source,
        source_start: SourceTimestamp::new(0, 1)?,
        source_end: SourceTimestamp::new(2, 1)?,
        source_anchor: SourceTimestamp::new(1, 2)?,
        project_anchor: ProjectTime::from_nanos(2_000_000_000),
        rate_numerator: 1,
        rate_denominator: 1,
    };
    let placed = project(client.execute(
        Command::SetSourcePlacement {
            spec,
            replace: None,
        },
        Some(p.project_id.clone()),
        Some(p.revision),
    )?)?;
    fixture.stop()?;
    let (status, _, stderr) = fixture.run(
        ProcessCommand::new(reader)
            .arg("engine")
            .env("PULSAR_STATE_DIR", &fixture.state),
        "old-schema-reader",
    )?;
    assert!(!status.success());
    assert!(
        stderr.contains("schema requires an explicit recoverable migration"),
        "unexpected reader refusal: {stderr}"
    );
    fixture.start()?;
    let mut connected = fixture.client()?;
    let recovered = project(connected.execute(Command::GetSnapshot, Some(p.project_id), None)?)?;
    assert_eq!(recovered.placements, placed.placements);
    assert_eq!(recovered.revision, placed.revision);
    assert_eq!(recovered.motion.program, placed.motion.program);
    Ok(())
}
