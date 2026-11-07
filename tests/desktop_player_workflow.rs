//! Actual Engine + headless mpv Desktop workflow. No Handy is armed or contacted.
#![cfg(target_os = "linux")]

use anyhow::{bail, Context, Result};
use pulsar_clients::{EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::{Command, DesktopPlaybackAction as Action, DesktopPlaybackStatus as Status};
use std::{
    fs::{self, File},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

struct Fixture {
    root: tempfile::TempDir,
    state: PathBuf,
    engine: Option<Child>,
}

impl Fixture {
    fn start() -> Result<Self> {
        let root = tempfile::tempdir().context("create Desktop workflow fixture")?;
        let state = root.path().join("state");
        let binary = std::env::var_os("PULSAR_PLAYER_TEST_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_pulsar")));
        let engine = Process::new(binary)
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state)
            .env("PULSAR_PLAYER_HEADLESS", "1")
            .env_remove("HANDY_API_KEY")
            .env_remove("HANDY_CONNECTION_KEY")
            .env_remove("PULSAR_ASSISTANT_OWNER_POLICY")
            .env_remove("PULSAR_CPU_OWNER_POLICY")
            .env_remove("PULSAR_CUDA_OWNER_POLICY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(root.path().join("engine.stderr"))?)
            // Own a separate process group so failed-test cleanup also closes
            // this Engine's mpv child; never target another user's player.
            .process_group(0)
            .spawn()
            .context("start actual Pulsar Engine")?;
        let mut fixture = Self {
            root,
            state,
            engine: Some(engine),
        };
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = fixture.engine.as_mut().unwrap().try_wait()? {
                bail!(
                    "Engine startup failed ({status}): {}",
                    fs::read_to_string(fixture.root.path().join("engine.stderr"))?
                );
            }
            if fixture.state.join("pairing.token").exists()
                && fixture.state.join("handy-owner.token").exists()
                && std::os::unix::net::UnixStream::connect(fixture.endpoint()).is_ok()
            {
                return Ok(fixture);
            }
            if Instant::now() >= deadline {
                bail!("Engine did not become ready within 45 seconds");
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn endpoint(&self) -> PathBuf {
        self.state.join("engine.sock")
    }

    fn client(&self) -> Result<SessionClient> {
        SessionClient::connect(&self.endpoint(), &self.state.join("pairing.token"))
    }

    fn proof(&self) -> Result<String> {
        // Generated test-only host capability. Never print it in diagnostics.
        Ok(fs::read_to_string(self.state.join("handy-owner.token"))?
            .trim()
            .to_owned())
    }

    fn media(&self) -> Result<(PathBuf, PathBuf)> {
        let video = self.root.path().join("local-player-fixture.mp4");
        let script = video.with_extension("funscript");
        let mut ffmpeg = Process::new("ffmpeg");
        ffmpeg
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=10",
                "-t",
                "20",
                "-c:v",
                "mpeg4",
                "-pix_fmt",
                "yuv420p",
                "-y",
            ])
            .arg(&video)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        run_bounded(&mut ffmpeg, Duration::from_secs(10))
            .context("generate small synthetic local video")?;
        let actions: Vec<_> = (0..=40)
            .map(|index| {
                serde_json::json!({
                    "at": index * 500,
                    "pos": if index % 2 == 0 { 45 } else { 55 }
                })
            })
            .collect();
        fs::write(
            &script,
            serde_json::to_vec(&serde_json::json!({"version":"1.0", "actions":actions}))?,
        )?;
        Ok((video, script))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut engine) = self.engine.take() {
            let group = format!("-{}", engine.id());
            let mut terminate = Process::new("/usr/bin/kill");
            terminate
                .args(["-TERM", "--", &group])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = run_bounded(&mut terminate, Duration::from_secs(1));
            let deadline = Instant::now() + Duration::from_secs(2);
            while engine.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
            // Escalate only within the dedicated fixture process group.
            let mut kill = Process::new("/usr/bin/kill");
            kill.args(["-KILL", "--", &group])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let _ = run_bounded(&mut kill, Duration::from_secs(1));
            let _ = engine.kill();
            let _ = engine.wait();
        }
    }
}

fn run_bounded(command: &mut Process, limit: Duration) -> Result<()> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            bail!("fixture subprocess failed with {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("fixture subprocess exceeded its time limit");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn call(client: &mut SessionClient, owner_proof: &str, action: Action) -> Result<Status> {
    match client.execute(
        Command::DesktopPlayback {
            owner_proof: owner_proof.to_owned(),
            action,
        },
        None,
        None,
    )? {
        ResponseBody::DesktopPlayback(status) => Ok(*status),
        _ => bail!("unexpected Desktop player reply"),
    }
}

fn wait_for(
    client: &mut SessionClient,
    proof: &str,
    description: &str,
    condition: impl Fn(&Status) -> bool,
) -> Result<Status> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        let status = call(client, proof, Action::Status)?;
        if !status.busy && condition(&status) {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for {description}: loaded={}, playing={}, busy={}, position={}ms; {}",
                status.loaded,
                status.playing,
                status.busy,
                status.position_ms,
                status.message
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires actual Pulsar binary, mpv and ffmpeg; no hardware or Handy credentials"]
fn desktop_player_local_media_workflow_and_controller_loss() -> Result<()> {
    let fixture = Fixture::start()?;
    let (video, script) = fixture.media()?;
    let proof = fixture.proof()?;
    let mut client = fixture.client()?;

    assert!(call(&mut client, "wrong-owner-proof", Action::Status).is_err());
    call(&mut client, &proof, Action::Open { video, script })?;
    let opened = wait_for(&mut client, &proof, "local media open", |s| s.loaded)?;
    assert!(!opened.armed && !opened.playing);
    assert!(opened.duration_ms >= 19_000);

    call(&mut client, &proof, Action::Play { sync_offset_ms: 0 })?;
    wait_for(&mut client, &proof, "video advancing", |s| {
        s.playing && s.position_ms >= 300
    })?;
    call(&mut client, &proof, Action::Pause)?;
    let paused = wait_for(&mut client, &proof, "paused video", |s| !s.playing)?;
    thread::sleep(Duration::from_millis(300));
    let still_paused = call(&mut client, &proof, Action::Status)?;
    assert!(!still_paused.playing);
    assert!(still_paused.position_ms.abs_diff(paused.position_ms) <= 100);

    call(&mut client, &proof, Action::Seek { position_ms: 5_000 })?;
    wait_for(&mut client, &proof, "paused seek", |s| {
        !s.playing && s.position_ms.abs_diff(5_000) <= 200
    })?;
    call(&mut client, &proof, Action::Play { sync_offset_ms: 0 })?;
    wait_for(&mut client, &proof, "resume from seek", |s| {
        s.playing && s.position_ms >= 5_200
    })?;
    call(&mut client, &proof, Action::Stop)?;
    let stopped = wait_for(&mut client, &proof, "stop at start", |s| {
        !s.playing && s.position_ms <= 100
    })?;
    assert!(stopped.loaded && !stopped.armed);

    call(&mut client, &proof, Action::Play { sync_offset_ms: 0 })?;
    wait_for(&mut client, &proof, "playing before controller loss", |s| {
        s.playing
    })?;
    // No Status heartbeat: the Engine must pause independently of the GUI.
    thread::sleep(Duration::from_millis(3_800));
    let abandoned = call(&mut client, &proof, Action::Status)?;
    assert!(!abandoned.playing, "controller loss must stop playback");
    assert!(
        !abandoned.armed,
        "controller loss must leave Handy disarmed"
    );

    // Play at EOF must restart the source and script together, not resume an
    // exhausted HSP stream against a restarted video clock.
    call(
        &mut client,
        &proof,
        Action::Seek {
            position_ms: 19_000,
        },
    )?;
    wait_for(&mut client, &proof, "seek near end", |s| {
        s.position_ms >= 18_900
    })?;
    call(&mut client, &proof, Action::Play { sync_offset_ms: 0 })?;
    wait_for(&mut client, &proof, "video EOF", |s| {
        !s.playing && s.position_ms >= 19_800
    })?;
    call(&mut client, &proof, Action::Play { sync_offset_ms: 0 })?;
    wait_for(&mut client, &proof, "replay after EOF", |s| {
        s.playing && (300..2000).contains(&s.position_ms)
    })?;
    call(&mut client, &proof, Action::Stop)?;
    wait_for(&mut client, &proof, "stop replay", |s| {
        !s.playing && s.position_ms <= 100
    })?;

    call(&mut client, &proof, Action::Close)?;
    wait_for(&mut client, &proof, "player close", |s| !s.loaded)?;
    Ok(())
}
