//! Unqualified, attended bench playback of a hosted Handy HSSP script.
//!
//! This intentionally bypasses the Engine, the qualification dossier, the device
//! journal, the controller lease and the clock synchronization. It exists so an
//! owner can prove motion on real hardware while the qualified chain is being
//! repaired. Nothing here is evidence: no admission, no stopping bound, no
//! qualification. The power switch is the stop.
//!
//! The Handy only plays scripts it can download, so the script must already be
//! hosted; `handy-prepare-stroke` + `handy-host-stroke` produce that reference.
use anyhow::{bail, Context, Result};
use std::{
    env, io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
const HOSTING_ORIGIN: &str = "https://www.handyfeeling.com";

pub struct PlayRequest {
    pub script_url: Option<String>,
    pub funscript: Option<PathBuf>,
    pub video: Option<PathBuf>,
    pub lead_ms: u64,
    pub seconds: u64,
    pub api_key_env: String,
    pub connection_key_env: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Handy {
    agent: ureq::Agent,
    token: String,
    connection: String,
}

impl Handy {
    fn connect(api_key: &str, connection: String) -> Result<Self> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let url = format!("{ORIGIN}/auth/token/issue?to={connection}&ttl=900");
        let response = agent
            .get(&url)
            .header("X-Api-Key", api_key)
            .header("Accept", "application/json")
            .call()
            .context("Handy token request failed")?;
        if response.status().as_u16() != 200 {
            bail!("Handy token request returned HTTP {}", response.status());
        }
        let body = read_json(response)?;
        let token = body
            .get("result")
            .and_then(|r| r.get("token"))
            .and_then(|t| t.as_str())
            .context("Handy token response has no token")?
            .to_string();
        Ok(Self {
            agent,
            token,
            connection,
        })
    }

    fn call(&self, method: &str, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value> {
        let url = format!("{ORIGIN}{path}");
        let authorization = format!("Bearer {}", self.token);
        let response = match method {
            "GET" => self
                .agent
                .get(&url)
                .force_send_body()
                .header("Authorization", &authorization)
                .header("X-Connection-Key", &self.connection)
                .header("Accept", "application/json")
                .send_empty(),
            "PUT" => self
                .agent
                .put(&url)
                .header("Authorization", &authorization)
                .header("X-Connection-Key", &self.connection)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .send(serde_json::to_vec(body.unwrap_or(&serde_json::json!({})))?),
            other => bail!("unsupported bench method {other}"),
        }
        .with_context(|| format!("Handy {method} {path} failed"))?;
        if response.status().as_u16() != 200 {
            bail!("Handy {method} {path} returned HTTP {}", response.status());
        }
        read_json(response)
    }

    fn state(&self) -> Result<serde_json::Value> {
        Ok(self.call("GET", "/hssp/state", None)?["result"].clone())
    }

    fn stop(&self) -> Result<()> {
        self.call(
            "PUT",
            "/hssp/stop",
            Some(&serde_json::json!({
                "start_time": 0, "server_time": 0, "playback_rate": 1.0, "loop": false
            })),
        )?;
        Ok(())
    }
}

fn read_json(mut response: ureq::http::Response<ureq::Body>) -> Result<serde_json::Value> {
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(256 * 1024)
        .read_to_end(&mut bytes)
        .context("Handy response read failed")?;
    serde_json::from_slice(&bytes).context("Handy response was not JSON")
}


/// Upload a numeric CSV to the vendor's public temporary hosting and verify the
/// exact bytes download again. The service takes no credentials; the URL is public
/// until the vendor expires it.
fn host_csv(csv: &[u8]) -> Result<String> {
    if csv.is_empty() {
        bail!("refusing to host an empty script");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let boundary = format!("pulsar{}", uuid::Uuid::new_v4().simple());
    let mut payload = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"script.csv\"\r\nContent-Type: text/csv\r\n\r\n"
    )
    .into_bytes();
    payload.extend_from_slice(csv);
    payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = agent
        .post(&format!("{HOSTING_ORIGIN}/api/hosting/v2/upload"))
        .header("Accept", "application/json")
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .send(payload.as_slice())
        .context("Handy hosting upload failed")?;
    if response.status().as_u16() != 200 {
        bail!("Handy hosting upload returned HTTP {}", response.status());
    }
    let reply = read_json(response)?;
    let url = reply
        .get("url")
        .and_then(|u| u.as_str())
        .context("Handy hosting reply has no url")?
        .to_string();
    if !url.starts_with(&format!("{HOSTING_ORIGIN}/api/hosting/v2/download/")) {
        bail!("Handy hosting returned an untrusted location");
    }
    let mut check = agent
        .get(&url)
        .header("Accept", "text/csv")
        .call()
        .context("Handy hosted script verification failed")?;
    if check.status().as_u16() != 200 {
        bail!("Handy hosted script returned HTTP {}", check.status());
    }
    let mut downloaded = Vec::new();
    check
        .body_mut()
        .as_reader()
        .take(16 * 1024 * 1024)
        .read_to_end(&mut downloaded)
        .context("Handy hosted script read failed")?;
    if downloaded != csv {
        bail!("Handy hosted script bytes differ from what was uploaded");
    }
    Ok(url)
}

/// Minimal funscript -> Handy numeric CSV (`time_ms,pos`). Positions are clamped
/// to 0..=100; actions are ordered by time and duplicate timestamps collapsed.
fn funscript_csv(bytes: &[u8]) -> Result<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct Action { at: i64, pos: f64 }
    #[derive(serde::Deserialize)]
    struct Script { actions: Vec<Action> }
    let script: Script = serde_json::from_slice(bytes).context("funscript is not valid JSON")?;
    if script.actions.is_empty() {
        bail!("funscript has no actions");
    }
    let mut actions = script.actions;
    actions.sort_by_key(|a| a.at);
    // The hosting service strips comment lines, so the upload stays row-only.
    let mut csv = String::new();
    let mut last = -1i64;
    for action in actions {
        if action.at < 0 || action.at == last {
            continue;
        }
        last = action.at;
        let pos = action.pos.clamp(0.0, 100.0).round() as i64;
        csv.push_str(&format!("{},{pos}\n", action.at));
    }
    Ok(csv.into_bytes())
}

fn credential(name: &str) -> Result<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => bail!(
            "{name} is not set. Launch this command under `secrets run --` so the Engine-free bench path can read it by name."
        ),
    }
}

pub fn play(request: PlayRequest) -> Result<()> {
    let script_url = match (&request.script_url, &request.funscript) {
        (Some(url), None) => {
            if url.is_empty() {
                bail!("--script-url must not be empty");
            }
            url.clone()
        }
        (None, Some(path)) => {
            let bytes = std::fs::read(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let csv = funscript_csv(&bytes)?;
            println!("Hosting {} ({} CSV bytes) on the public vendor service", path.display(), csv.len());
            host_csv(&csv)?
        }
        (Some(_), Some(_)) => bail!("pass either --script-url or --funscript, not both"),
        (None, None) => bail!("pass --script-url or --funscript"),
    };
    if request.lead_ms > 5_000 {
        bail!("--lead-ms must be at most 5000");
    }
    if request.seconds == 0 || request.seconds > 600 {
        bail!("--seconds must be between 1 and 600");
    }
    let api_key = credential(&request.api_key_env)?;
    let connection = credential(&request.connection_key_env)?;

    println!("UNQUALIFIED BENCH PLAY");
    println!("No dossier, no device journal, no stopping bound. Attended only; the power switch is the stop.");
    println!("Script: {script_url}");

    let handy = Handy::connect(&api_key, connection)?;
    let connected = handy.call("GET", "/connected", None)?;
    if connected["result"]["connected"].as_bool() != Some(true) {
        bail!("Handy reports not connected");
    }
    let before = handy.state()?;
    if before["play_state"].as_u64() == Some(1) {
        bail!("Handy is already playing (play_state=1); stop it before starting a bench play");
    }

    handy.call("PUT", "/mode", Some(&serde_json::json!({"mode": 1})))?;
    handy.call(
        "PUT",
        "/hssp/setup",
        Some(&serde_json::json!({
            "url": script_url, "loop": false, "notify": false
        })),
    )?;
    let loaded = handy.state()?;
    let points = loaded["points"].as_u64().unwrap_or(0);
    println!("Loaded: points={points} last_point_time={}", loaded["last_point_time"]);
    if points == 0 {
        bail!("Handy reported no loaded points; the hosted script did not load");
    }

    let mut video = match &request.video {
        Some(path) => Some(spawn_video(path)?),
        None => None,
    };
    let server_time = now_ms() + request.lead_ms;
    println!("Starting in {} ms", request.lead_ms);
    handy.call(
        "PUT",
        "/hssp/play",
        Some(&serde_json::json!({
            "start_time": 0,
            "server_time": server_time,
            "playback_rate": 1.0,
            "loop": false
        })),
    )?;

    let deadline = now_ms() + request.seconds * 1_000;
    let mut observation = handy.state();
    while now_ms() < deadline {
        sleep(Duration::from_millis(1_000));
        observation = handy.state();
        match &observation {
            Ok(state) => println!(
                "  play_state={} current_time={} point={}/{}",
                state["play_state"], state["current_time"], state["current_point"], state["points"]
            ),
            Err(error) => println!("  state unavailable: {error}"),
        }
    }

    println!("Stopping");
    let stop = handy.stop();
    if let Ok(state) = handy.state() {
        println!("Final: play_state={} points={}", state["play_state"], state["points"]);
    }
    if let Some(child) = video.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    stop?;
    println!("Stop requested. A 200 response and a frozen current_point are not proof of physical stopping.");
    Ok(())
}

pub fn stop(api_key_env: &str, connection_key_env: &str) -> Result<()> {
    let api_key = credential(api_key_env)?;
    let connection = credential(connection_key_env)?;
    let handy = Handy::connect(&api_key, connection)?;
    handy.stop()?;
    if let Ok(state) = handy.state() {
        println!("Stop requested. play_state={} points={}", state["play_state"], state["points"]);
    }
    println!("Verify by observation; the power switch is the stop.");
    Ok(())
}

fn spawn_video(path: &PathBuf) -> Result<Child> {
    if !path.is_file() {
        bail!("--video {} is not a file", path.display());
    }
    Command::new("ffplay")
        .args(["-autoexit", "-loglevel", "quiet", "-i"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start ffplay for {}", path.display()))
}
