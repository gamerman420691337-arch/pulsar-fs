//! Engine-owned local mpv process and bounded JSON IPC. No device authority lives here.
#[cfg(unix)]
use anyhow::Context;
use anyhow::{bail, Result};
use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct MpvState {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub paused: bool,
    pub eof: bool,
}

#[cfg(unix)]
mod native {
    use super::*;
    use serde_json::{json, Value};
    use std::ffi::OsString;
    use std::io::{BufRead, BufReader, Write};
    use std::net::Shutdown;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const REPLY_LIMIT: usize = 64 * 1024;
    const RPC_TIMEOUT: Duration = Duration::from_secs(1);
    const START_TIMEOUT: Duration = Duration::from_secs(5);

    pub(crate) struct MpvPlayer {
        child: Option<Child>,
        stream: Option<BufReader<UnixStream>>,
        directory: Option<tempfile::TempDir>,
        next_request: u64,
        operation_deadline: Option<Instant>,
    }

    fn remaining(deadline: Instant) -> Result<Duration> {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .context("mpv IPC deadline exceeded")
    }

    fn read_line(reader: &mut BufReader<UnixStream>, deadline: Instant) -> Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            reader
                .get_mut()
                .set_read_timeout(Some(remaining(deadline)?))?;
            let bytes = reader.fill_buf().context("mpv IPC read failed")?;
            if bytes.is_empty() {
                bail!("mpv IPC closed");
            }
            let take = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |n| n + 1);
            anyhow::ensure!(
                line.len() + take <= REPLY_LIMIT,
                "mpv IPC reply exceeds 64 KiB"
            );
            let complete = bytes[take - 1] == b'\n';
            line.extend_from_slice(&bytes[..take]);
            reader.consume(take);
            if complete {
                return Ok(line);
            }
        }
    }

    fn receive_reply(
        reader: &mut BufReader<UnixStream>,
        id: u64,
        deadline: Instant,
    ) -> Result<Value> {
        loop {
            let reply: Value = serde_json::from_slice(&read_line(reader, deadline)?)
                .context("invalid mpv IPC JSON")?;
            if reply.get("event").is_some() && reply.get("request_id").is_none() {
                continue;
            }
            anyhow::ensure!(
                reply.get("request_id").and_then(Value::as_u64) == Some(id),
                "mpv IPC reply identity differs"
            );
            return Ok(reply);
        }
    }

    fn milliseconds(value: Option<Value>) -> Result<u64> {
        let Some(value) = value else { return Ok(0) };
        let seconds = value
            .as_f64()
            .context("mpv returned a nonnumeric media time")?;
        anyhow::ensure!(
            seconds.is_finite() && seconds >= 0.0 && seconds * 1000.0 < u64::MAX as f64,
            "mpv returned an invalid media time"
        );
        Ok((seconds * 1000.0).round() as u64)
    }

    impl MpvPlayer {
        pub(crate) fn open(video: &Path, runtime_dir: &Path) -> Result<Self> {
            Self::open_with_headless(
                video,
                runtime_dir,
                std::env::var_os("PULSAR_PLAYER_HEADLESS").is_some_and(|v| v == "1"),
            )
        }

        fn open_with_headless(video: &Path, runtime_dir: &Path, headless: bool) -> Result<Self> {
            let software = std::env::var_os("PULSAR_PLAYER_SOFTWARE").is_some_and(|v| v == "1");
            let first = Self::spawn(video, runtime_dir, headless, software);
            if first.is_err() && !headless && !software && std::env::var_os("DISPLAY").is_some() {
                return Self::spawn(video, runtime_dir, false, true)
                    .context("video renderer unavailable (GPU and X11 fallback)");
            }
            first
        }

        fn spawn(video: &Path, runtime_dir: &Path, headless: bool, software: bool) -> Result<Self> {
            anyhow::ensure!(
                video.is_absolute() && runtime_dir.is_absolute(),
                "player paths must be absolute"
            );
            let video = video.canonicalize().context("local video is unavailable")?;
            anyhow::ensure!(
                video.metadata()?.is_file(),
                "local video must be a regular file"
            );
            std::fs::create_dir_all(runtime_dir).context("player runtime directory unavailable")?;
            let directory = tempfile::Builder::new()
                .prefix("mpv-")
                .tempdir_in(runtime_dir)?;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
            let socket = directory.path().join("ipc");
            anyhow::ensure!(
                socket.as_os_str().as_bytes().len() < 104,
                "player runtime path is too long for a Unix socket"
            );
            let mut ipc = OsString::from("--input-ipc-server=");
            ipc.push(&socket);
            let mut command = Command::new("mpv");
            command
                .args([
                    "--pause",
                    "--keep-open=yes",
                    "--idle=no",
                    "--no-config",
                    "--no-terminal",
                    "--load-scripts=no",
                    "--ytdl=no",
                    "--access-references=no",
                    "--input-default-bindings=no",
                    "--input-vo-keyboard=no",
                    "--osc=no",
                    "--title=Pulsar video",
                ])
                .arg(ipc)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // mpv needs desktop/audio access, never the Engine's provider/device keys.
            command.env_clear();
            for name in [
                "PATH",
                "HOME",
                "DISPLAY",
                "WAYLAND_DISPLAY",
                "XAUTHORITY",
                "XDG_RUNTIME_DIR",
                "PULSE_SERVER",
                "PULSE_COOKIE",
                "DBUS_SESSION_BUS_ADDRESS",
                "LANG",
                "LC_ALL",
                "TMPDIR",
                "ALSA_CONFIG_PATH",
                "LIBGL_ALWAYS_SOFTWARE",
                "MESA_LOADER_DRIVER_OVERRIDE",
                "DRI_PRIME",
                "VDPAU_DRIVER",
                "LIBVA_DRIVER_NAME",
            ] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            if headless {
                command.args(["--vo=null", "--ao=null"]);
            } else if software {
                command.arg("--vo=x11");
            }
            command.arg("--").arg(video);
            let child = command
                .spawn()
                .context("mpv could not start; install mpv for local playback")?;
            let mut player = Self {
                child: Some(child),
                stream: None,
                directory: Some(directory),
                next_request: 1,
                operation_deadline: Some(Instant::now() + START_TIMEOUT),
            };
            let deadline = Instant::now() + START_TIMEOUT;
            let connected = (|| -> Result<()> {
                loop {
                    anyhow::ensure!(
                        player.child.as_mut().unwrap().try_wait()?.is_none(),
                        "mpv exited before the local player was ready"
                    );
                    match UnixStream::connect(&socket) {
                        Ok(stream) => {
                            stream.set_read_timeout(Some(RPC_TIMEOUT))?;
                            stream.set_write_timeout(Some(RPC_TIMEOUT))?;
                            player.stream = Some(BufReader::new(stream));
                            return Ok(());
                        }
                        Err(_) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(20))
                        }
                        Err(_) => {
                            bail!("mpv did not expose its local IPC socket within five seconds")
                        }
                    }
                }
            })();
            if let Err(error) = connected {
                player.close();
                return Err(error);
            }
            // A socket can precede file loading. Wait for actual local media metadata.
            let ready = (|| -> Result<()> {
                loop {
                    if player.state()?.duration_ms > 0
                        && (headless
                            || player
                                .command(json!(["get_property", "video-out-params"]), true)?
                                .is_some())
                    {
                        return Ok(());
                    }
                    remaining(deadline)?;
                    std::thread::sleep(Duration::from_millis(20));
                }
            })();
            if let Err(error) = ready {
                player.close();
                return Err(error.context("mpv could not load local video"));
            }
            player.operation_deadline = None;
            Ok(player)
        }

        fn request_raw(&mut self, command: Value) -> Result<Value> {
            let id = self.next_request;
            self.next_request = id
                .checked_add(1)
                .context("mpv request identity exhausted")?;
            let ordinary_deadline = Instant::now() + RPC_TIMEOUT;
            let deadline = self.operation_deadline.unwrap_or(ordinary_deadline);
            let reader = self.stream.as_mut().context("mpv player is closed")?;
            reader
                .get_mut()
                .set_write_timeout(Some(remaining(deadline)?))?;
            let mut message = serde_json::to_vec(&json!({"command":command,"request_id":id}))?;
            message.push(b'\n');
            reader
                .get_mut()
                .write_all(&message)
                .context("mpv IPC write failed")?;
            receive_reply(reader, id, deadline)
        }

        fn command(&mut self, command: Value, optional: bool) -> Result<Option<Value>> {
            let result = (|| -> Result<Option<Value>> {
                let reply = self.request_raw(command)?;
                match reply.get("error").and_then(Value::as_str) {
                    Some("success") => Ok(reply.get("data").filter(|v| !v.is_null()).cloned()),
                    Some("property unavailable") if optional => Ok(None),
                    _ => bail!("mpv refused the player command"),
                }
            })();
            if result.is_err() {
                self.close();
            }
            result
        }

        pub(crate) fn state(&mut self) -> Result<MpvState> {
            let result = (|| -> Result<MpvState> {
                if self
                    .child
                    .as_mut()
                    .context("mpv player is closed")?
                    .try_wait()?
                    .is_some()
                {
                    self.close();
                    bail!("mpv player exited");
                }
                let position_ms =
                    milliseconds(self.command(json!(["get_property", "time-pos"]), true)?)?;
                let duration_ms =
                    milliseconds(self.command(json!(["get_property", "duration"]), true)?)?;
                let paused = self
                    .command(json!(["get_property", "pause"]), false)?
                    .and_then(|v| v.as_bool())
                    .context("mpv returned no pause state")?;
                let eof = self
                    .command(json!(["get_property", "eof-reached"]), true)?
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Ok(MpvState {
                    position_ms,
                    duration_ms,
                    paused,
                    eof,
                })
            })();
            if result.is_err() {
                self.close();
            }
            result
        }

        pub(crate) fn pause(&mut self, paused: bool) -> Result<()> {
            self.command(json!(["set_property", "pause", paused]), false)?;
            Ok(())
        }

        pub(crate) fn seek(&mut self, position_ms: u64) -> Result<()> {
            let deadline = Instant::now() + RPC_TIMEOUT;
            self.operation_deadline = Some(deadline);
            let result = (|| -> Result<()> {
                let duration_ms = self.state()?.duration_ms;
                let target = if duration_ms > 0 {
                    position_ms.min(duration_ms)
                } else {
                    position_ms
                };
                self.command(
                    json!(["seek", target as f64 / 1000.0, "absolute+exact"]),
                    false,
                )?;
                loop {
                    let state = self.state()?;
                    let seeking = self
                        .command(json!(["get_property", "seeking"]), true)?
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if !seeking && state.position_ms.abs_diff(target) <= 200 {
                        return Ok(());
                    }
                    remaining(deadline)?;
                    std::thread::sleep(Duration::from_millis(10));
                }
            })();
            self.operation_deadline = None;
            if result.is_err() {
                self.close();
            }
            result
        }

        pub(crate) fn close(&mut self) {
            self.operation_deadline = None;
            if let Some(mut reader) = self.stream.take() {
                let stream = reader.get_mut();
                let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                let _ = stream.write_all(b"{\"command\":[\"quit\"]}\n");
                let _ = stream.shutdown(Shutdown::Both);
            }
            if let Some(mut child) = self.child.take() {
                let until = Instant::now() + Duration::from_millis(250);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) if Instant::now() < until => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        _ => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                    }
                }
            }
            self.directory.take();
        }
    }
    impl Drop for MpvPlayer {
        fn drop(&mut self) {
            self.close();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn events_are_drained_and_reply_identity_is_checked() {
            let (client, mut server) = UnixStream::pair().unwrap();
            server.write_all(b"{\"event\":\"file-loaded\"}\n{\"request_id\":7,\"error\":\"success\",\"data\":true}\n").unwrap();
            let reply = receive_reply(&mut BufReader::new(client), 7, Instant::now() + RPC_TIMEOUT)
                .unwrap();
            assert_eq!(reply["data"], true);
            let (client, mut server) = UnixStream::pair().unwrap();
            server
                .write_all(b"{\"request_id\":8,\"error\":\"success\"}\n")
                .unwrap();
            assert!(
                receive_reply(&mut BufReader::new(client), 7, Instant::now() + RPC_TIMEOUT)
                    .is_err()
            );
        }
        #[test]
        fn oversized_and_truncated_replies_refuse() {
            let (client, mut server) = UnixStream::pair().unwrap();
            let writer = std::thread::spawn(move || {
                let _ = server.write_all(&vec![b'x'; REPLY_LIMIT + 1]);
            });
            assert!(read_line(&mut BufReader::new(client), Instant::now() + RPC_TIMEOUT).is_err());
            writer.join().unwrap();
            let (client, mut server) = UnixStream::pair().unwrap();
            server.write_all(b"{\"event\":").unwrap();
            drop(server);
            assert!(read_line(&mut BufReader::new(client), Instant::now() + RPC_TIMEOUT).is_err());
        }
        #[test]
        fn numeric_media_time_is_bounded() {
            assert_eq!(milliseconds(Some(json!(1.25))).unwrap(), 1250);
            assert!(milliseconds(Some(json!(-1))).is_err());
            assert!(milliseconds(Some(json!("1"))).is_err());
        }
        #[test]
        #[ignore = "requires installed mpv and ffmpeg; exercises a real headless player"]
        fn real_headless_player_opens_pauses_seeks_and_cleans_up() {
            let directory = tempfile::tempdir().unwrap();
            let video = directory.path().join("fixture.mp4");
            let mut child = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=black:s=64x64:r=10",
                    "-t",
                    "2",
                    "-c:v",
                    "mpeg4",
                    "-y",
                ])
                .arg(&video)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let until = Instant::now() + START_TIMEOUT;
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success());
                    break;
                }
                if Instant::now() >= until {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("ffmpeg fixture timeout");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut player = MpvPlayer::open_with_headless(&video, directory.path(), true).unwrap();
            let socket_dir = player.directory.as_ref().unwrap().path().to_path_buf();
            assert!(player.state().unwrap().paused);
            assert!(player.state().unwrap().duration_ms >= 1900);
            player.pause(false).unwrap();
            let until = Instant::now() + Duration::from_secs(2);
            while player.state().unwrap().position_ms == 0 {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(20));
            }
            player.pause(true).unwrap();
            player.seek(500).unwrap();
            let until = Instant::now() + Duration::from_secs(2);
            while player.state().unwrap().position_ms.abs_diff(500) > 100 {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(20));
            }
            player.close();
            assert!(!socket_dir.exists());
            assert!(player.state().is_err());
        }
    }
}

#[cfg(unix)]
pub(crate) use native::MpvPlayer;

#[cfg(not(unix))]
pub(crate) struct MpvPlayer;
#[cfg(not(unix))]
impl MpvPlayer {
    pub(crate) fn open(_video: &Path, _runtime_dir: &Path) -> Result<Self> {
        bail!("local mpv playback is unavailable on this platform")
    }
    pub(crate) fn state(&mut self) -> Result<MpvState> {
        bail!("local mpv playback is unavailable on this platform")
    }
    pub(crate) fn pause(&mut self, _paused: bool) -> Result<()> {
        bail!("local mpv playback is unavailable on this platform")
    }
    pub(crate) fn seek(&mut self, _position_ms: u64) -> Result<()> {
        bail!("local mpv playback is unavailable on this platform")
    }
    pub(crate) fn close(&mut self) {}
}
