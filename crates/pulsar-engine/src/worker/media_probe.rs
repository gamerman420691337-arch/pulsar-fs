//! Metadata query only; decoding/accuracy acceptance remains separate.
use super::*;
use anyhow::ensure;
use serde::Deserialize;
use std::process::Command;
#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<Stream>,
    format: Option<Format>,
}
#[derive(Deserialize)]
struct Format {
    format_name: Option<String>,
    duration: Option<String>,
}
#[derive(Deserialize)]
struct Stream {
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    avg_frame_rate: Option<String>,
}
pub(super) fn run(request: &WorkerRequest, deadline: Instant) -> Result<MediaProbeReport> {
    let mut command = Command::new(&request.tools.ffprobe.path);
    command.args(["-v","error","-threads","1","-protocol_whitelist","file,pipe","-show_entries",
        "format=format_name,duration:stream=index,codec_type,codec_name,width,height,sample_rate,channels,avg_frame_rate","-of","json"])
        .arg(&request.source.path);
    let bytes = media::bounded_output(command, 65536, deadline)?;
    let parsed: Probe = serde_json::from_slice(&bytes).context("invalid media probe metadata")?;
    let mut streams = Vec::new();
    for stream in parsed.streams {
        let frame_rate = match stream.avg_frame_rate.as_deref() {
            None | Some("0/0") | Some("0/1") => None,
            Some(text) => {
                let (num, den) = text
                    .split_once('/')
                    .context("invalid metadata frame rate")?;
                let num = num.parse::<u64>()?;
                let den = den.parse::<u64>()?;
                ensure!(den != 0, "metadata frame rate denominator is zero");
                Some(num as f64 / den as f64)
            }
        };
        streams.push(MediaStreamInfo {
            index: stream.index,
            kind: stream.codec_type,
            codec: stream.codec_name,
            width: stream.width,
            height: stream.height,
            sample_rate: stream.sample_rate.map(|v| v.parse()).transpose()?,
            channels: stream.channels,
            frame_rate,
        });
    }
    let (format, declared_duration_seconds) = match parsed.format {
        Some(value) => (
            value.format_name,
            value.duration.map(|s| s.parse()).transpose()?,
        ),
        None => (None, None),
    };
    let report = MediaProbeReport {
        schema_version: 1,
        project_id: request.project_id.clone(),
        revision: request.base_revision,
        source_version: request.source.source_version.clone(),
        source: request.source.identity.clone(),
        format,
        declared_duration_seconds,
        streams,
        decoded: false,
        qualified: false,
    };
    report.validate()?;
    Ok(report)
}
