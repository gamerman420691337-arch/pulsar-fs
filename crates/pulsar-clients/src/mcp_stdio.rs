//! Local MCP transport for a fixed project and an owner-selected consent document.

use super::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: u64 = 64 * 1024;
const MAX_WORK_CALLS: u32 = 128;

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
struct StopPlayback {}

fn tools() -> Value {
    json!({"tools": [
        {
            "name": "read_candidate",
            "description": "Read a retained candidate in the configured project.",
            "inputSchema": {"type":"object","additionalProperties":false,"required":["candidate_id"],
                "properties":{"candidate_id":{"type":"string"}}}
        },
        {
            "name": "propose_pattern",
            "description": "Stage an explicit pattern for review at the named revision.",
            "inputSchema": {"type":"object","additionalProperties":false,"required":["revision","pattern"],
                "properties":{"revision":{"type":"integer","minimum":0},"pattern":{"type":"string","maxLength":24576}}}
        },
        {
            "name": "stop_playback",
            "description": "Stop the local Engine-owned Desktop player.",
            "inputSchema": {"type":"object","additionalProperties":false}
        }
    ]})
}

struct Server<'a> {
    endpoint: &'a Path,
    bootstrap: &'a Path,
    project: ProjectId,
    consent: &'a Path,
    work_calls: u32,
}

impl Server<'_> {
    fn execute(&mut self, name: &str, arguments: Value) -> Result<Value> {
        if name == "stop_playback" {
            let _: StopPlayback = serde_json::from_value(arguments)?;
            let directory = self
                .bootstrap
                .parent()
                .context("Engine state directory is missing")?;
            let proof = project_package::read_local_owner_proof(directory, "handy-owner.token")?;
            let mut client = SessionClient::connect(self.endpoint, self.bootstrap)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            client.set_operation_deadline(Some(deadline));
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
        match name {
            "read_candidate" => {
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
            "propose_pattern" => {
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
            _ => anyhow::bail!("Unknown MCP tool"),
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
        "tools/list" => tools(),
        "tools/call" => {
            let Some(name @ ("read_candidate" | "propose_pattern" | "stop_playback")) =
                params["name"].as_str()
            else {
                return Some(error(id, -32602, "Unknown tool"));
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (text, failed) = match server.execute(name, args) {
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
) -> Result<()> {
    let mut server = Server {
        endpoint,
        bootstrap,
        project,
        consent,
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
    fn mcp_requires_initialization_and_refuses_unlisted_effects() {
        let mut server = Server {
            endpoint: Path::new("unused"),
            bootstrap: Path::new("unused"),
            project: ProjectId::new("fixture").unwrap(),
            consent: Path::new("unused"),
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
            ["read_candidate", "propose_pattern", "stop_playback"]
        );
        let denied = request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"play_playback","arguments":{}}}))
        .unwrap();
        assert_eq!(denied["error"]["code"], -32602);
        assert_eq!(server.work_calls, 0);
    }
}
