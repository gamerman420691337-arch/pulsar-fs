//! Authored native IPC harness. Never emits bearer credentials or device effects.
use anyhow::{ensure, Result};
use pulsar_clients::{EngineApi, ResponseBody, SessionClient};
use pulsar_protocol::*;
use serde::Deserialize;
use std::{
    io::Read,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    state: PathBuf,
    project: ProjectId,
    revision: RevisionId,
    epoch: AttemptId,
    source: PathBuf,
    mode: String,
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 1, "usage: encoded-capture-fixture INPUT.json");
    let mut config = Vec::new();
    std::fs::File::open(&args[0])?
        .take(65537)
        .read_to_end(&mut config)?;
    ensure!(config.len() <= 65536, "bounded fixture config required");
    let input: Input = serde_json::from_slice(&config)?;
    let mut client = SessionClient::connect(
        &input.state.join("engine.sock"),
        &input.state.join("pairing.token"),
    )?;
    let status = |client: &mut SessionClient| -> Result<LiveCaptureReport> {
        let ResponseBody::LiveCapture(report) = client.execute(
            Command::LiveCaptureStatus {
                epoch: input.epoch.clone(),
            },
            Some(input.project.clone()),
            Some(input.revision),
        )?
        else {
            anyhow::bail!("capture status expected")
        };
        Ok(*report)
    };
    let mut bytes = Vec::new();
    std::fs::File::open(&input.source)?
        .take(65537)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= 65536,
        "bounded authored stream required"
    );
    let initial = status(&mut client)?;
    #[cfg(unix)]
    if input.mode.starts_with("raw-") {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        // The fixture reads its own temporary first-party credential in memory.
        // It never serializes/logs the handshake or credential value.
        let credential: serde_json::Value = serde_json::from_slice(&std::fs::read(
            input.state.join("first-party.session.json"),
        )?)?;
        let mut scope = LiveBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: LiveBulkChannel::LiveEncodedSource,
            session: client.session_id().clone(),
            auth_token: credential["auth_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("temporary fixture credential missing"))?
                .to_owned(),
            engine_epoch: initial.engine_epoch.clone(),
            identity: initial.identity.clone(),
        };
        let refused_before_attach = match input.mode.as_str() {
            "raw-auth" => {
                scope.auth_token = "invalid-authored-token".into();
                true
            }
            "raw-boot" => {
                scope.engine_epoch = AttemptId::new("retired-boot")?;
                true
            }
            "raw-source" => {
                scope.identity.source = SourceVersionId::new("foreign-source")?;
                true
            }
            "raw-revision" => {
                scope.identity.revision = RevisionId::new(1);
                true
            }
            _ => false,
        };
        let open = || -> Result<UnixStream> {
            let stream = UnixStream::connect(input.state.join("bulk.sock"))?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            Ok(stream)
        };
        let mut stream = open()?;
        write_bulk_header(&mut stream, &scope)?;
        let ready: LiveEncodedReply = read_bulk_header(&mut stream)?;
        let mut refused_before_body = true;
        let reply = if refused_before_attach {
            ready
        } else {
            ensure!(
                matches!(ready, LiveEncodedReply::Ready(_)),
                "native encoded ready expected"
            );
            if input.mode == "raw-duplicate" {
                let mut other = open()?;
                write_bulk_header(&mut other, &scope)?;
                read_bulk_header::<_, LiveEncodedReply>(&mut other)?
            } else {
                let request = LiveEncodedRequest::Push {
                    sequence: if input.mode == "raw-sequence" { 1 } else { 0 },
                    byte_len: if input.mode == "raw-oversize" {
                        65537
                    } else {
                        3
                    },
                    sha256: "a".repeat(64),
                };
                write_bulk_header(&mut stream, &request)?;
                let reply: LiveEncodedReply = read_bulk_header(&mut stream)?;
                if matches!(reply, LiveEncodedReply::SendBytes { .. }) {
                    refused_before_body = false;
                    stream.write_all(b"bad")?;
                    read_bulk_header(&mut stream)?
                } else {
                    reply
                }
            }
        };
        ensure!(
            matches!(reply, LiveEncodedReply::Error(_)),
            "malformed/foreign encoded request must fail"
        );
        drop(stream);
        let deadline = Instant::now() + Duration::from_secs(5);
        let final_report = loop {
            let report = status(&mut client)?;
            if refused_before_attach || !report.active {
                break report;
            }
            ensure!(Instant::now() < deadline, "raw source retirement deadline");
            std::thread::sleep(Duration::from_millis(10));
        };
        if refused_before_attach {
            ensure!(
                final_report
                    .encoded_source
                    .as_ref()
                    .expect("admitted")
                    .state
                    == LiveEncodedSourceState::Pending,
                "refused caller cannot claim source"
            );
            client.execute(
                Command::StopLiveCapture {
                    epoch: input.epoch.clone(),
                },
                Some(input.project.clone()),
                Some(input.revision),
            )?;
        }
        println!(
            "{}",
            serde_json::json!({"reply":reply,"final":final_report,"refused_before_attach":refused_before_attach,"refused_before_body":refused_before_body,"physical_dispatch":false})
        );
        return Ok(());
    }
    let mut encoded = client.open_live_encoded(&initial)?;
    ensure!(
        encoded.push(&[]).is_err(),
        "empty local chunk must refuse without poisoning"
    );
    encoded.push(&bytes)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let before_eof = loop {
        let report = status(&mut client)?;
        ensure!(report.active, "capture retired before authored frame");
        if report.received_frames >= 3 {
            break report;
        }
        ensure!(
            Instant::now() < deadline,
            "native publication before EOF deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut refused = None;
    match input.mode.as_str() {
        "normal" => {
            let report = encoded.finish()?;
            println!(
                "{}",
                serde_json::json!({"before_eof":before_eof,"final":report,"physical_dispatch":false})
            );
            return Ok(());
        }
        "continue" => {
            let mut options = before_eof.options.clone();
            options.source_anchor = SourceTimestamp::new(1, 1)?;
            options.project_anchor = ProjectTime::from_nanos(1_000_000_000);
            options.presentation_anchor = LiveClockTick(0);
            let ResponseBody::LiveCapture(successor) = client.execute(
                Command::ContinueLiveCapture {
                    engine_epoch: before_eof.engine_epoch.clone(),
                    predecessor: input.epoch.clone(),
                    options,
                },
                Some(input.project.clone()),
                Some(input.revision),
            )?
            else {
                anyhow::bail!("continuation report expected")
            };
            ensure!(
                successor.active && successor.received_frames == 0,
                "fresh successor admission"
            );
            ensure!(encoded.push(&[0]).is_err(), "retired source input refused");
            drop(encoded);
            let prior = status(&mut client)?;
            ensure!(
                prior.retirement == Some(LiveCaptureRetirement::Superseded),
                "predecessor superseded"
            );
            let ResponseBody::LiveCapture(current) = client.execute(
                Command::LiveCaptureStatus {
                    epoch: successor.identity.epoch.clone(),
                },
                Some(input.project.clone()),
                Some(input.revision),
            )?
            else {
                anyhow::bail!("successor status expected")
            };
            ensure!(current.active, "old source failure cannot retire successor");
            client.execute(
                Command::StopLiveCapture {
                    epoch: successor.identity.epoch.clone(),
                },
                Some(input.project.clone()),
                Some(input.revision),
            )?;
            println!(
                "{}",
                serde_json::json!({"before_eof": before_eof, "final": prior, "successor": current, "physical_dispatch":false})
            );
            return Ok(());
        }
        "drop" => drop(encoded),
        "stop" => {
            client.execute(
                Command::StopLiveCapture {
                    epoch: input.epoch.clone(),
                },
                Some(input.project.clone()),
                Some(input.revision),
            )?;
            refused = Some(encoded.push(&[0]).unwrap_err().to_string());
            drop(encoded);
        }
        "expire" => {
            let elapsed_ms = before_eof.server_elapsed.expect("active").0 / 1_000_000;
            let remaining = u64::from(before_eof.options.lease_ms).saturating_sub(elapsed_ms);
            std::thread::sleep(Duration::from_millis(remaining + 150));
            refused = Some(encoded.push(&[0]).unwrap_err().to_string());
            drop(encoded);
        }
        _ => anyhow::bail!("unsupported authored fixture mode"),
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let retired = loop {
        let report = status(&mut client)?;
        if !report.active {
            break report;
        }
        ensure!(Instant::now() < deadline, "source retirement deadline");
        std::thread::sleep(Duration::from_millis(10));
    };
    println!(
        "{}",
        serde_json::json!({"before_eof":before_eof,"final":retired,"refused":refused,"physical_dispatch":false})
    );
    Ok(())
}
