//! Local MCP transport for a fixed project and an owner-selected consent document.

use super::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: u64 = 64 * 1024;
const MAX_WORK_CALLS: u32 = 128;
const MAX_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadCandidate {
    candidate_id: CandidateId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposePattern {
    revision: u64,
    pattern: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArguments {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    ReadCandidate,
    ProposePattern,
    StopPlayback,
    PlayPreparedScript,
    ProjectSummary,
    DeviceStatus,
    PausePresentation,
    ProposeMedia,
}

#[derive(PartialEq, Eq)]
enum ToolClass {
    Read,
    Author,
    Effect,
    Stop,
}

impl Tool {
    const ALL: [Self; 8] = [
        Self::ReadCandidate,
        Self::ProposePattern,
        Self::StopPlayback,
        Self::PlayPreparedScript,
        Self::ProjectSummary,
        Self::DeviceStatus,
        Self::PausePresentation,
        Self::ProposeMedia,
    ];

    fn class(self) -> ToolClass {
        match self {
            Self::ReadCandidate | Self::ProjectSummary | Self::DeviceStatus => ToolClass::Read,
            Self::ProposePattern => ToolClass::Author,
            Self::PlayPreparedScript | Self::ProposeMedia => ToolClass::Effect,
            Self::StopPlayback | Self::PausePresentation => ToolClass::Stop,
        }
    }

    fn available(self, media_configured: bool) -> bool {
        self != Self::ProposeMedia || media_configured
    }

    fn name(self) -> &'static str {
        match self {
            Self::ReadCandidate => "read_candidate",
            Self::ProposePattern => "propose_pattern",
            Self::StopPlayback => "stop_playback",
            Self::PlayPreparedScript => "play_prepared_script",
            Self::ProjectSummary => "project_summary",
            Self::DeviceStatus => "device_status",
            Self::PausePresentation => "pause_presentation",
            Self::ProposeMedia => "propose_media",
        }
    }

    fn descriptor(self) -> Value {
        let description = match self {
            Self::ReadCandidate => "Read a retained candidate in the configured project.",
            Self::ProposePattern => "Stage an explicit pattern for review at the named revision.",
            Self::StopPlayback => "Stop the local Engine-owned Desktop player.",
            Self::PlayPreparedScript => {
                "Request playback of the loaded script under the active user-enabled Handy grant."
            }
            Self::ProjectSummary => {
                "Read the current revision and source counts for the configured project."
            }
            Self::DeviceStatus => "Read the local player and Handy session state.",
            Self::PausePresentation => {
                "Pause local video-only presentation and cancel its pending review."
            }
            Self::ProposeMedia => {
                "Queue the host-selected local clip for review in Pulsar Desktop."
            }
        };
        let mut schema = json!({"type":"object","additionalProperties":false});
        match self {
            Self::ReadCandidate => {
                schema["required"] = json!(["candidate_id"]);
                schema["properties"] = json!({"candidate_id":{"type":"string"}});
            }
            Self::ProposePattern => {
                schema["required"] = json!(["revision", "pattern"]);
                schema["properties"] = json!({
                    "revision":{"type":"integer","minimum":0},
                    "pattern":{"type":"string","maxLength":MAX_TOOL_ARGUMENT_BYTES}
                });
            }
            _ => {}
        }
        json!({"name":self.name(),"description":description,"inputSchema":schema})
    }
}

fn tools(media_configured: bool) -> Value {
    let entries: Vec<_> = Tool::ALL
        .into_iter()
        .filter(|tool| tool.available(media_configured))
        .map(Tool::descriptor)
        .collect();
    json!({"tools": entries})
}

struct Server<'a> {
    endpoint: &'a Path,
    bootstrap: &'a Path,
    project: ProjectId,
    consent: &'a Path,
    media: Option<&'a Path>,
    work_calls: u32,
}

impl Server<'_> {
    fn owner_proof(&self) -> Result<String> {
        let directory = self
            .bootstrap
            .parent()
            .context("Engine state directory is missing")?;
        project_package::read_local_owner_proof(directory, "handy-owner.token")
    }

    fn player_status(client: &mut impl EngineApi, proof: String) -> Result<DesktopPlaybackStatus> {
        let response = client.execute(
            Command::DesktopPlayback {
                action: DesktopPlaybackAction::Status,
                owner_proof: proof,
            },
            None,
            None,
        )?;
        let ResponseBody::DesktopPlayback(status) = response else {
            anyhow::bail!("Engine returned an unexpected player response");
        };
        Ok(*status)
    }

    fn execute(&mut self, tool: Tool, arguments: Value) -> Result<Value> {
        if tool.class() == ToolClass::Stop {
            let _: NoArguments = serde_json::from_value(arguments)?;
            let proof = self.owner_proof()?;
            let mut client = SessionClient::connect(self.endpoint, self.bootstrap)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            client.set_operation_deadline(Some(deadline));
            if tool == Tool::PausePresentation {
                wait_desktop_playback_control(
                    &mut client,
                    &proof,
                    deadline,
                    DesktopPlaybackAction::PausePresentation,
                )?;
                return Ok(json!({"presentation_paused":true}));
            }
            stop_desktop_playback(&mut client, &proof, deadline)?;
            return Ok(json!({"playback_stopped":true,"physical_stop_verified":false}));
        }

        anyhow::ensure!(
            self.work_calls < MAX_WORK_CALLS,
            "MCP work budget exhausted"
        );
        self.work_calls += 1;
        require_roleplay_attestation(self.consent, consent_now_ns())?;
        let mut client = SessionClient::connect(self.endpoint, self.bootstrap)?;
        client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(5)));
        match tool {
            Tool::ProjectSummary => {
                let _: NoArguments = serde_json::from_value(arguments)?;
                let project = get_snapshot(&mut client, &self.project)?;
                Ok(json!({
                    "project_id": project.project_id,
                    "revision": project.revision,
                    "source_count": project.sources.len(),
                    "placement_count": project.placements.len()
                }))
            }
            Tool::DeviceStatus => {
                let _: NoArguments = serde_json::from_value(arguments)?;
                let status = Self::player_status(&mut client, self.owner_proof()?)?;
                Ok(json!({
                    "connected": status.connected,
                    "armed": status.armed,
                    "playing": status.playing,
                    "busy": status.busy,
                    "assistant_control": status.assistant_grant.is_some(),
                    "expires_in_ms": status.expires_in_ms,
                    "limits_applied": status.limits_applied,
                    "physical_playback_verified": false
                }))
            }
            Tool::PlayPreparedScript => {
                let _: NoArguments = serde_json::from_value(arguments)?;
                let proof = self.owner_proof()?;
                let status = Self::player_status(&mut client, proof.clone())?;
                let grant = status
                    .assistant_grant
                    .context("No active assistant Handy grant")?;
                let response = client.execute(
                    Command::DesktopPlayback {
                        action: DesktopPlaybackAction::AssistantPlay {
                            grant,
                            project: self.project.clone(),
                            consent: self.consent.to_path_buf(),
                        },
                        owner_proof: proof,
                    },
                    None,
                    None,
                )?;
                let ResponseBody::DesktopPlayback(_) = response else {
                    anyhow::bail!("Engine returned an unexpected player response");
                };
                Ok(json!({"playback_queued":true,"physical_playback_verified":false}))
            }
            Tool::ProposeMedia => {
                let _: NoArguments = serde_json::from_value(arguments)?;
                let video = self.media.context("No media is configured")?;
                let response = client.execute(
                    Command::DesktopPlayback {
                        action: DesktopPlaybackAction::PrepareMediaReview {
                            video: video.to_path_buf(),
                            consent: self.consent.to_path_buf(),
                        },
                        owner_proof: self.owner_proof()?,
                    },
                    None,
                    None,
                )?;
                let ResponseBody::DesktopPlayback(_) = response else {
                    anyhow::bail!("Engine returned an unexpected media review response");
                };
                Ok(json!({"proposal_queued":true,"playback_started":false}))
            }
            Tool::ReadCandidate => {
                let input: ReadCandidate = serde_json::from_value(arguments)?;
                let response = client.execute(
                    Command::GetCandidate {
                        candidate_id: input.candidate_id,
                    },
                    Some(self.project.clone()),
                    None,
                )?;
                let ResponseBody::Candidate(_) = response else {
                    anyhow::bail!("Engine returned an unexpected candidate response");
                };
                Ok(serde_json::to_value(response)?)
            }
            Tool::ProposePattern => {
                let input: ProposePattern = serde_json::from_value(arguments)?;
                let pattern = patterns::build_pattern_input(&input.pattern)?;
                let project = get_snapshot(&mut client, &self.project)?;
                let candidate = patterns::generate_candidate(
                    &mut client,
                    project,
                    RevisionId::new(input.revision),
                    pattern,
                )?;
                Ok(serde_json::to_value(ResponseBody::Candidate(Box::new(
                    candidate,
                )))?)
            }
            Tool::StopPlayback | Tool::PausePresentation => {
                unreachable!("handled by the stop path")
            }
        }
    }
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    New,
    Negotiated,
    Ready,
}

fn handle(server: &mut Server<'_>, state: &mut Lifecycle, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);
    if request["jsonrpc"] != "2.0" || method.is_none() {
        return Some(error(Value::Null, -32600, "Invalid request"));
    }
    let method = method?;
    let Some(id) = id else {
        if method == "notifications/initialized" && *state == Lifecycle::Negotiated {
            *state = Lifecycle::Ready;
        }
        return None;
    };
    if !id.is_string() && !id.is_i64() && !id.is_u64() {
        return Some(error(Value::Null, -32600, "Invalid request ID"));
    }
    let params = &request["params"];
    let result = match method {
        "ping" => json!({}),
        "initialize" if *state == Lifecycle::New => {
            if !params["protocolVersion"].is_string()
                || !params["capabilities"].is_object()
                || !params["clientInfo"]["name"].is_string()
                || !params["clientInfo"]["version"].is_string()
            {
                return Some(error(id, -32602, "Invalid initialization parameters"));
            }
            *state = Lifecycle::Negotiated;
            json!({"protocolVersion":PROTOCOL,"capabilities":{"tools":{}},
                "serverInfo":{"name":"pulsar-mcp","version":env!("CARGO_PKG_VERSION")}})
        }
        _ if *state != Lifecycle::Ready => {
            return Some(error(id, -32000, "MCP initialization required"));
        }
        "tools/list" => tools(server.media.is_some()),
        "tools/call" => {
            let Some(tool) = params["name"].as_str().and_then(|name| {
                Tool::ALL
                    .into_iter()
                    .find(|tool| tool.name() == name && tool.available(server.media.is_some()))
            }) else {
                return Some(error(id, -32602, "Unknown tool"));
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !args.is_object() || serde_json::to_vec(&args).ok()?.len() > MAX_TOOL_ARGUMENT_BYTES
            {
                return Some(error(
                    id,
                    -32602,
                    "Tool arguments exceed the object or size bound",
                ));
            }
            let (text, failed) = match server.execute(tool, args) {
                Ok(value) => (value.to_string(), false),
                Err(_) => (
                    "Pulsar refused the tool call or could not confirm its result".into(),
                    true,
                ),
            };
            json!({"content":[{"type":"text","text":text}],"isError":failed})
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub(crate) fn run(
    endpoint: &Path,
    bootstrap: &Path,
    project: ProjectId,
    consent: &Path,
    media: Option<&Path>,
) -> Result<()> {
    let mut server = Server {
        endpoint,
        bootstrap,
        project,
        consent,
        media,
        work_calls: 0,
    };
    let mut state = Lifecycle::New;
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = (&mut input)
            .take(MAX_MESSAGE_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            count as u64 <= MAX_MESSAGE_BYTES,
            "MCP input exceeds its bound"
        );
        let response = match serde_json::from_slice(&line) {
            Ok(request) => handle(&mut server, &mut state, request),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_proposal_is_optional_and_accepts_no_model_path() {
        let listed = tools(true);
        assert_eq!(listed["tools"][7]["name"], "propose_media");
        assert_eq!(
            listed["tools"][7]["inputSchema"]["additionalProperties"],
            false
        );
        assert!(serde_json::from_value::<NoArguments>(json!({"video":"/untrusted.mp4"})).is_err());
        assert!(serde_json::from_value::<NoArguments>(json!({})).is_ok());
    }

    #[test]
    fn mcp_requires_initialization_and_refuses_unlisted_effects() {
        let mut server = Server {
            endpoint: Path::new("unused"),
            bootstrap: Path::new("unused"),
            project: ProjectId::new("fixture").unwrap(),
            consent: Path::new("unused"),
            media: None,
            work_calls: 0,
        };
        let mut state = Lifecycle::New;
        let mut request = |value| handle(&mut server, &mut state, value);
        assert_eq!(
            request(json!({"jsonrpc":"2.0","id":0,"method":"tools/list"})).unwrap()["error"]
                ["code"],
            -32000
        );
        let init = request(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":PROTOCOL,"capabilities":{},
                "clientInfo":{"name":"fixture","version":"1"}}}))
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL);
        assert!(request(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let listed = request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<_> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "read_candidate",
                "propose_pattern",
                "stop_playback",
                "play_prepared_script",
                "project_summary",
                "device_status",
                "pause_presentation"
            ]
        );
        let denied = request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"play_playback","arguments":{}}}))
        .unwrap();
        assert_eq!(denied["error"]["code"], -32602);
        let oversized = request(json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"propose_pattern","arguments":{"revision":0,"pattern":"x".repeat(MAX_TOOL_ARGUMENT_BYTES)}}}))
        .unwrap();
        assert_eq!(oversized["error"]["code"], -32602);
        assert_eq!(server.work_calls, 0);
    }
}
