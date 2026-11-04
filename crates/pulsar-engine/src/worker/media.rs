//! Native media decoding adapter. Invoked only inside the isolated worker role.

use anyhow::{bail, Context, Result};
use pulsar_protocol::{ExactSeconds, SourceTimestamp};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_PROBE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 4096;
pub const MAX_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct MediaIndex {
    pub width: u32,
    pub height: u32,
    pub time_base_num: u32,
    pub time_base_den: u32,
    /// Decoder presentation order, preserving each source timestamp exactly.
    pub pts: Vec<i64>,
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    #[serde(default)]
    frames: Vec<Frame>,
}

#[derive(Deserialize)]
struct Stream {
    width: u32,
    height: u32,
    time_base: String,
}

#[derive(Deserialize)]
struct Frame {
    best_effort_timestamp: Option<serde_json::Value>,
}

pub fn frame_bytes(width: u32, height: u32) -> Result<usize> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .context("frame dimensions overflow")?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || pixels > MAX_PIXELS
    {
        bail!("frame dimensions exceed the worker's validated buffer contract");
    }
    usize::try_from(pixels.checked_mul(3).context("RGB buffer overflow")?)
        .context("RGB buffer exceeds addressable memory")
}

pub fn fit_dimensions(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> Result<(u32, u32)> {
    frame_bytes(max_width, max_height)?;
    if width == 0 || height == 0 {
        bail!("source contains invalid dimensions");
    }
    let scale = (max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64)
        .min(1.0);
    let result = (
        (width as f64 * scale).floor().max(1.0) as u32,
        (height as f64 * scale).floor().max(1.0) as u32,
    );
    frame_bytes(result.0, result.1)?;
    Ok(result)
}

pub fn probe(tool: &Path, source: &Path, deadline: Instant) -> Result<MediaIndex> {
    let mut command = Command::new(tool);
    command
        .args([
            "-v",
            "error",
            "-threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,time_base:frame=best_effort_timestamp",
            "-show_frames",
            "-of",
            "json",
        ])
        .arg(source);
    let bytes = bounded_output(command, MAX_PROBE_BYTES, deadline)?;
    let parsed: Probe = serde_json::from_slice(&bytes).context("invalid ffprobe metadata")?;
    let stream = parsed
        .streams
        .first()
        .context("source has no video stream")?;
    let (num, den) = stream
        .time_base
        .split_once('/')
        .context("source lacks a rational time base")?;
    let time_base_num: u32 = num.parse().context("invalid source time-base numerator")?;
    let time_base_den: u32 = den
        .parse()
        .context("invalid source time-base denominator")?;
    if time_base_num == 0 || time_base_den == 0 || stream.width == 0 || stream.height == 0 {
        bail!("source time base and dimensions must be positive");
    }
    let mut pts = Vec::with_capacity(parsed.frames.len());
    for frame in parsed.frames {
        let value = frame
            .best_effort_timestamp
            .context("source frame has no presentation timestamp")?;
        let timestamp = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
            .context("source presentation timestamp is not an integer")?;
        if pts.last().is_some_and(|last| *last >= timestamp) {
            bail!("non-increasing source timestamps require an explicit repair policy");
        }
        pts.push(timestamp);
    }
    if pts.is_empty() {
        bail!("source has no decoded video frames");
    }
    Ok(MediaIndex {
        width: stream.width,
        height: stream.height,
        time_base_num,
        time_base_den,
        pts,
    })
}

/// Bounded collection with a deadline. Stderr never reaches framed worker stdout.
pub fn bounded_output(mut command: Command, limit: usize, deadline: Instant) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to launch a declared native tool")?;
    let stdout = child
        .stdout
        .take()
        .context("native tool stdout unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("native tool deadline exceeded");
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => {
                let bytes = match result {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error.into());
                    }
                };
                if bytes.len() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("native tool output exceeded its bound");
                }
                loop {
                    if let Some(status) = child.try_wait()? {
                        if !status.success() {
                            bail!("native tool failed with {status}");
                        }
                        return Ok(bytes);
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        bail!("native tool exit deadline exceeded");
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("native output reader terminated");
            }
        }
    }
}

fn decode_command(tool: &Path, source: &Path, threads: u16) -> Command {
    let mut command = Command::new(tool);
    command
        .args([
            "-v",
            "error",
            "-nostdin",
            "-xerror",
            "-filter_threads",
            "1",
            "-threads",
            &threads.to_string(),
            "-protocol_whitelist",
            "file,pipe",
            "-noautorotate",
            "-i",
        ])
        .arg(source)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-fps_mode",
            "passthrough",
            "-threads",
            &threads.to_string(),
        ]);
    command
}

#[derive(Clone, Copy)]
pub struct FrameTransform<'a> {
    pub width: u32,
    pub height: u32,
    pub filter: &'a str,
}
#[derive(Clone, Copy)]
pub struct SnapshotFrame {
    pub index: usize,
    pub source_time: SourceTimestamp,
}

pub fn extract(
    tool: &Path,
    source: &Path,
    frame: SnapshotFrame,
    width: u32,
    height: u32,
    threads: u16,
    deadline: Instant,
) -> Result<Vec<u8>> {
    extract_filtered(
        tool,
        source,
        frame,
        FrameTransform {
            width,
            height,
            filter: &format!("scale={width}:{height}"),
        },
        threads,
        deadline,
    )
}
pub fn extract_filtered(
    tool: &Path,
    source: &Path,
    frame: SnapshotFrame,
    transform: FrameTransform<'_>,
    threads: u16,
    deadline: Instant,
) -> Result<Vec<u8>> {
    let FrameTransform {
        width,
        height,
        filter,
    } = transform;
    frame_bytes(width, height)?;
    let mut command = decode_command(tool, source, threads);
    command.args([
        "-v", "quiet", "-copyts", "-vf",
        &format!("select=eq(n\\,{}),{filter},format=rgb24", frame.index),
        "-frames:v", "1", "-enc_time_base", "demux", "-c:v", "rawvideo", "-f", "tee",
        "[f=rawvideo:flush_packets=1]pipe:1|[f=framehash:hash=sha256:avoid_negative_ts=disabled:flush_packets=1]pipe:2",
    ]);
    let mut decoder = super::timestamped_decode::TimestampedDecoder::start(
        command,
        Stdio::null(),
        width,
        height,
        deadline,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )?;
    let decoded = decoder
        .next()?
        .context("decoder ended before the indexed snapshot frame")?;
    if decoded.ordinal != 0
        || ExactSeconds::from_source(decoded.source_time)
            .checked_cmp(ExactSeconds::from_source(frame.source_time))?
            != std::cmp::Ordering::Equal
    {
        bail!("decoded snapshot timestamp differs from indexed source frame");
    }
    if decoder.next()?.is_some() {
        bail!("snapshot decoder returned more than one frame");
    }
    Ok(decoded.pixels)
}

pub struct Decoder(super::timestamped_decode::TimestampedDecoder);
impl Decoder {
    pub fn new_filtered(
        tool: &Path,
        source: &Path,
        transform: FrameTransform<'_>,
        stride: u32,
        threads: u16,
        deadline: Instant,
    ) -> Result<Self> {
        let FrameTransform {
            width,
            height,
            filter,
        } = transform;
        if stride == 0 {
            bail!("source sampling stride must be positive");
        }
        frame_bytes(width, height)?;
        let mut command = decode_command(tool, source, threads);
        command.args([
            "-v", "quiet", "-copyts", "-vf",
            &format!("select=not(mod(n\\,{stride})),{filter},format=rgb24"),
            "-enc_time_base", "demux", "-c:v", "rawvideo", "-f", "tee",
            "[f=rawvideo:flush_packets=1]pipe:1|[f=framehash:hash=sha256:avoid_negative_ts=disabled:flush_packets=1]pipe:2",
        ]);
        Ok(Self(super::timestamped_decode::TimestampedDecoder::start(
            command,
            Stdio::null(),
            width,
            height,
            deadline,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )?))
    }
    pub fn next(&mut self) -> Result<Option<super::timestamped_decode::TimestampedRgbFrame>> {
        self.0.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_bounds_reject_overflow_and_empty() {
        assert!(frame_bytes(0, 5).is_err());
        assert!(frame_bytes(u32::MAX, u32::MAX).is_err());
        assert_eq!(frame_bytes(64, 48).unwrap(), 64 * 48 * 3);
    }

    #[test]
    fn preview_preserves_aspect_ratio() {
        assert_eq!(fit_dimensions(1920, 1080, 640, 640).unwrap(), (640, 360));
        assert_eq!(fit_dimensions(100, 50, 640, 640).unwrap(), (100, 50));
    }
}
