//! Native single-pass RGB/packet correspondence. The caller owns tool/source
//! pinning, confinement and admission. Delivery time is not exposure time.
use anyhow::{bail, Context, Result};
use pulsar_protocol::{ExactSeconds, SourceTimestamp};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
const MAX_LINE: usize = 1024;
const MAX_HEADER_LINES: usize = 32;
#[derive(Debug)]
pub(super) struct TimestampedRgbFrame {
    pub ordinal: u64,
    pub source_time: SourceTimestamp,
    pub pixels: Vec<u8>,
    /// Completion of the native stdout read, before queueing/caller delay.
    pub delivered: Instant,
}
#[derive(Debug)]
struct Packet {
    time: SourceTimestamp,
    hash: String,
}
struct HashParser {
    time_base: Option<(u32, u32)>,
    format: bool,
    version: bool,
    sha: bool,
    dimensions: bool,
    codec: bool,
    video: bool,
    headers: usize,
    frames: u64,
    last: Option<SourceTimestamp>,
    width: u32,
    height: u32,
    expected: usize,
}
impl HashParser {
    fn new(width: u32, height: u32, expected: usize) -> Self {
        Self {
            time_base: None,
            format: false,
            version: false,
            sha: false,
            dimensions: false,
            codec: false,
            video: false,
            headers: 0,
            frames: 0,
            last: None,
            width,
            height,
            expected,
        }
    }
    fn line(&mut self, line: &str) -> Result<Option<Packet>> {
        if line.len() > MAX_LINE || !line.is_ascii() {
            bail!("decoder packet line exceeds grammar");
        }
        let line = line.trim();
        if line.starts_with('#') {
            if self.frames != 0 {
                bail!("decoder packet header changed after frames");
            }
            self.headers += 1;
            if self.headers > MAX_HEADER_LINES {
                bail!("decoder packet header exceeds bound");
            }
            match line {
                "#format: frame checksums" if !self.format => self.format = true,
                "#version: 2" if !self.version => self.version = true,
                "#hash: SHA256" if !self.sha => self.sha = true,
                "#codec_id 0: rawvideo" if !self.codec => self.codec = true,
                "#media_type 0: video" if !self.video => self.video = true,
                _ if line.starts_with("#tb 0: ") && self.time_base.is_none() => {
                    let (n, d) = line[7..]
                        .split_once('/')
                        .context("decoder packet time base absent")?;
                    let (n, d): (u32, u32) = (n.parse()?, d.parse()?);
                    if n == 0 || d == 0 {
                        bail!("decoder packet time base is not positive");
                    }
                    self.time_base = Some((n, d));
                }
                _ if line == format!("#dimensions 0: {}x{}", self.width, self.height)
                    && !self.dimensions =>
                {
                    self.dimensions = true
                }
                _ if line.starts_with("#software: ")
                    || line.starts_with("#sar 0: ")
                    || line == "#stream#, dts,        pts, duration,     size, hash" => {}
                _ => bail!("decoder packet header differs from fixed RGB contract"),
            }
            return Ok(None);
        }
        if !(self.format && self.version && self.sha && self.dimensions && self.codec && self.video)
        {
            bail!("decoder packet header incomplete");
        }
        let (n, d) = self.time_base.context("decoder packet time base missing")?;
        let values: Vec<_> = line.split(',').map(str::trim).collect();
        if values.len() != 6 || values[0] != "0" {
            bail!("decoder packet stream/field count differs");
        }
        let dts: i64 = values[1].parse()?;
        let pts: i64 = values[2].parse()?;
        let duration: i64 = values[3].parse()?;
        let size: usize = values[4].parse()?;
        if dts != pts
            || pts == i64::MIN
            || duration < 0
            || size != self.expected
            || values[5].len() != 64
            || !values[5]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("decoder packet clock/size/hash differs");
        }
        let time = SourceTimestamp::new(i64::try_from(i128::from(pts) * i128::from(n))?, d)?;
        if let Some(last) = self.last {
            if ExactSeconds::from_source(time).checked_cmp(ExactSeconds::from_source(last))?
                != std::cmp::Ordering::Greater
            {
                bail!("decoder packet timestamp did not increase");
            }
        }
        self.frames = self
            .frames
            .checked_add(1)
            .context("decoder ordinal overflow")?;
        self.last = Some(time);
        Ok(Some(Packet {
            time,
            hash: values[5].into(),
        }))
    }
    fn finish(&self) -> Result<()> {
        if self.frames == 0 {
            bail!("decoder emitted no timestamped RGB packets");
        }
        Ok(())
    }
}
type DecodedFrame = (Vec<u8>, Instant);

pub(super) struct TimestampedDecoder {
    child: Arc<Mutex<Child>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    pixels: Option<Receiver<Result<Option<DecodedFrame>>>>,
    packets: Option<Receiver<Result<Option<Packet>>>>,
    readers: Vec<JoinHandle<()>>,
    watchdog: Option<JoinHandle<()>>,
    ordinal: u64,
    terminal: bool,
}
impl TimestampedDecoder {
    pub(super) fn take_input(&mut self) -> Result<std::process::ChildStdin> {
        self.child
            .lock()
            .map_err(|_| anyhow::anyhow!("decoder lock poisoned"))?
            .stdin
            .take()
            .context("decoder has no owned pipe input")
    }
    pub(super) fn start(
        mut command: Command,
        input: Stdio,
        width: u32,
        height: u32,
        deadline: Instant,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let expected = super::media::frame_bytes(width, height)?;
        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
            bail!("decoder cancelled or expired before launch");
        }
        // Input options/source and fixed RGB filter are provided by the worker
        // command builder. No native diagnostics share the packet-hash stream.
        let mut child = command
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("launch timestamped decoder")?;
        let pipes = (child.stdout.take(), child.stderr.take());
        let (Some(mut stdout), Some(stderr)) = pipes else {
            let _ = child.kill();
            let _ = child.wait();
            bail!("decoder pipes missing");
        };
        let child = Arc::new(Mutex::new(child));
        let stop = Arc::new(AtomicBool::new(false));
        let mut decoder = Self {
            child: child.clone(),
            stop: stop.clone(),
            cancel: cancel.clone(),
            deadline,
            pixels: None,
            packets: None,
            readers: vec![],
            watchdog: None,
            ordinal: 0,
            terminal: false,
        };
        decoder.watchdog = Some(
            std::thread::Builder::new()
                .name("pulsar-decode-watch".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                            if let Ok(mut c) = child.lock() {
                                let _ = c.kill();
                                let _ = c.wait();
                            }
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })?,
        );
        let (tx, rx) = mpsc::sync_channel(2);
        decoder.pixels = Some(rx);
        decoder.readers.push(
            std::thread::Builder::new()
                .name("pulsar-decode-rgb".into())
                .spawn(move || loop {
                    let mut bytes = vec![0; expected];
                    let result = (|| -> Result<Option<(Vec<u8>, Instant)>> {
                        if stdout.read(&mut bytes[..1])? == 0 {
                            return Ok(None);
                        }
                        stdout.read_exact(&mut bytes[1..])?;
                        Ok(Some((bytes, Instant::now())))
                    })();
                    let terminal = !matches!(result, Ok(Some(_)));
                    if tx.send(result).is_err() || terminal {
                        break;
                    }
                })?,
        );
        let (tx, rx) = mpsc::sync_channel(2);
        decoder.packets = Some(rx);
        let packet_child = decoder.child.clone();
        decoder.readers.push(
            std::thread::Builder::new()
                .name("pulsar-decode-clock".into())
                .spawn(move || {
                    let mut input = BufReader::new(stderr);
                    let mut parser = HashParser::new(width, height, expected);
                    loop {
                        let mut bytes = Vec::with_capacity(MAX_LINE + 1);
                        let result = (|| -> Result<Option<Packet>> {
                            let read = (&mut input)
                                .take((MAX_LINE + 1) as u64)
                                .read_until(b'\n', &mut bytes)?;
                            if read == 0 {
                                parser.finish()?;
                                return Ok(None);
                            }
                            if bytes.len() > MAX_LINE || !bytes.ends_with(b"\n") {
                                bail!("decoder packet line truncated or exceeds bound");
                            }
                            parser.line(std::str::from_utf8(&bytes)?)
                        })();
                        match result {
                            Ok(None) if !bytes.is_empty() => continue,
                            other => {
                                if other.is_err() {
                                    if let Ok(mut child) = packet_child.lock() {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                    }
                                }
                                let terminal = !matches!(other, Ok(Some(_)));
                                if tx.send(other).is_err() || terminal {
                                    break;
                                }
                            }
                        }
                    }
                })?,
        );
        Ok(decoder)
    }
    fn receive<T>(&self, rx: &Receiver<Result<Option<T>>>) -> Result<Option<T>> {
        loop {
            if self.cancel.load(Ordering::Acquire) || Instant::now() >= self.deadline {
                bail!("decoder cancelled or absolute deadline expired");
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(v) => return v,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub(super) fn next(&mut self) -> Result<Option<TimestampedRgbFrame>> {
        if self.terminal {
            bail!("timestamped decoder retired");
        }
        let result = (|| -> Result<Option<TimestampedRgbFrame>> {
            let pixels = self.receive(
                self.pixels
                    .as_ref()
                    .context("decoder pixel channel closed")?,
            )?;
            let packet = self.receive(
                self.packets
                    .as_ref()
                    .context("decoder packet channel closed")?,
            )?;
            match (pixels, packet) {
                (Some((pixels, delivered)), Some(packet)) => {
                    if format!("{:x}", Sha256::digest(&pixels)) != packet.hash {
                        bail!("decoder pixels differ from timestamped packet hash");
                    }
                    let ordinal = self.ordinal;
                    self.ordinal = self
                        .ordinal
                        .checked_add(1)
                        .context("decoder ordinal overflow")?;
                    Ok(Some(TimestampedRgbFrame {
                        ordinal,
                        source_time: packet.time,
                        pixels,
                        delivered,
                    }))
                }
                (None, None) => {
                    loop {
                        if let Some(status) = self
                            .child
                            .lock()
                            .map_err(|_| anyhow::anyhow!("decoder lock poisoned"))?
                            .try_wait()?
                        {
                            if !status.success() {
                                bail!("timestamped decoder exited {status}");
                            }
                            break;
                        }
                        if self.cancel.load(Ordering::Acquire) || Instant::now() >= self.deadline {
                            bail!("decoder exit deadline expired");
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(None)
                }
                _ => bail!("decoder pixels/packet counts differ"),
            }
        })();
        if result.is_err() {
            self.retire();
        }
        result
    }
    fn retire(&mut self) {
        self.terminal = true;
        self.stop.store(true, Ordering::Release);
        self.pixels.take();
        self.packets.take();
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
impl Drop for TimestampedDecoder {
    fn drop(&mut self) {
        self.retire();
        if let Some(h) = self.watchdog.take() {
            let _ = h.join();
        }
        for h in self.readers.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parser() -> HashParser {
        let mut p = HashParser::new(64, 64, 12288);
        for line in [
            "#format: frame checksums",
            "#version: 2",
            "#hash: SHA256",
            "#tb 0: 1/1000",
            "#media_type 0: video",
            "#codec_id 0: rawvideo",
            "#dimensions 0: 64x64",
        ] {
            assert!(p.line(line).unwrap().is_none());
        }
        p
    }
    fn line(pts: i64, size: usize, hash: &str) -> String {
        format!("0, {pts}, {pts}, 1, {size}, {hash}")
    }
    #[test]
    fn packet_grammar_preserves_signed_irregular_pts_and_exact_sha() {
        let mut p = parser();
        for pts in [-15, 0, 12, 900] {
            let hash = "a".repeat(64);
            let packet = p.line(&line(pts, 12288, &hash)).unwrap().unwrap();
            assert_eq!(packet.time, SourceTimestamp::new(pts, 1000).unwrap());
            assert_eq!(packet.hash, hash);
        }
        assert_eq!(p.frames, 4);
    }
    #[test]
    fn packet_grammar_refuses_changed_headers_missing_contract_and_ordering() {
        for bad in [
            "#hash: MD5",
            "#dimensions 0: 65x64",
            "#tb 0: 1/1000",
            "0, 1, 2, 1, 12288, wrong",
        ] {
            assert!(parser().line(bad).is_err());
        }
        let mut empty = HashParser::new(64, 64, 12288);
        assert!(empty.line(&line(1, 12288, &"a".repeat(64))).is_err());
        let mut p = parser();
        p.line(&line(5, 12288, &"a".repeat(64))).unwrap();
        assert!(p.line(&line(5, 12288, &"b".repeat(64))).is_err());
        assert!(p.line("#software: changed").is_err());
    }
    #[test]
    fn packet_grammar_refuses_excess_line_header_hash_size_and_overflow() {
        assert!(parser()
            .line(&line(i64::MIN, 12288, &"a".repeat(64)))
            .is_err());
        assert!(parser().line(&"x".repeat(MAX_LINE + 1)).is_err());
        assert!(parser().line(&line(1, 12287, &"a".repeat(64))).is_err());
        assert!(parser().line(&line(1, 12288, &"A".repeat(64))).is_err());
        let mut p = parser();
        p.time_base = Some((u32::MAX, 1));
        assert!(p.line(&line(i64::MAX, 12288, &"a".repeat(64))).is_err());
        let mut p = parser();
        for _ in p.headers..MAX_HEADER_LINES {
            p.line("#software: bounded").unwrap();
        }
        assert!(p.line("#software: excess").is_err());
    }
    #[cfg(unix)]
    fn ffmpeg(source: &str) -> Command {
        let mut c = Command::new("/usr/bin/ffmpeg");
        if source == "pipe:0" {
            c.args([
                "-f",
                "yuv4mpegpipe",
                "-probesize",
                "32",
                "-analyzeduration",
                "0",
            ]);
        }
        c.args([
            "-v",
            "quiet",
            "-nostdin",
            "-copyts",
            "-filter_threads",
            "1",
            "-threads",
            "1",
            "-protocol_whitelist",
            "file,pipe",
            "-noautorotate",
            "-i",
            source,
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-vf",
            "scale=64:64,format=rgb24",
            "-fps_mode",
            "passthrough",
            "-enc_time_base",
            "demux",
            "-c:v",
            "rawvideo",
            "-threads",
            "1",
            "-f",
            "tee",
            "[f=rawvideo:flush_packets=1]pipe:1|[f=framehash:hash=sha256:avoid_negative_ts=disabled:flush_packets=1]pipe:2",
        ]);
        c
    }
    #[cfg(unix)]
    #[test]
    fn actual_unknown_length_native_pipe_pairs_timestamps_and_pixels_without_probe() {
        use std::{
            io::Write,
            os::{fd::OwnedFd, unix::net::UnixStream},
        };
        let (input, mut producer) = UnixStream::pair().unwrap();
        let input: OwnedFd = input.into();
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let producer = std::thread::spawn(move || {
            producer
                .write_all(b"YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n")
                .unwrap();
            for value in [0, 128, 255] {
                producer.write_all(b"FRAME\n").unwrap();
                producer.write_all(&[value; 4096]).unwrap();
                std::thread::sleep(Duration::from_millis(5));
            }
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let mut d = TimestampedDecoder::start(
            ffmpeg("pipe:0"),
            Stdio::from(std::fs::File::from(input)),
            64,
            64,
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut frames = vec![];
        for _ in 0..3 {
            frames.push(d.next().unwrap().unwrap());
        }
        assert!(
            !producer.is_finished(),
            "frames must arrive before source EOF"
        );
        release_tx.send(()).unwrap();
        assert!(d.next().unwrap().is_none());
        assert_eq!(frames.len(), 3);
        for (i, f) in frames.iter().enumerate() {
            assert_eq!(f.ordinal, i as u64);
            assert_eq!(f.source_time, SourceTimestamp::new(i as i64, 4).unwrap());
            assert_eq!(f.pixels.len(), 12288);
            assert!(f.delivered <= Instant::now());
        }
        assert!(frames[0].pixels.iter().all(|b| *b == 0));
        assert!(frames[2].pixels.iter().all(|b| *b == 255));
        producer.join().unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn independent_watchdog_reaps_idle_decoder_on_cancel_and_expiry_without_next() {
        for cancelled in [false, true] {
            let mut c = Command::new("/bin/sleep");
            c.arg("5");
            let cancel = Arc::new(AtomicBool::new(false));
            let deadline = Instant::now()
                + if cancelled {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(40)
                };
            let mut d =
                TimestampedDecoder::start(c, Stdio::null(), 64, 64, deadline, cancel.clone())
                    .unwrap();
            if cancelled {
                cancel.store(true, Ordering::Release);
            }
            let observation_end = Instant::now() + Duration::from_secs(2);
            loop {
                if d.child.lock().unwrap().try_wait().unwrap().is_some() {
                    break;
                }
                assert!(Instant::now() < observation_end);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(d.next().is_err());
            assert!(d.next().is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn malformed_native_source_fails_without_frame_or_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-media");
        std::fs::write(&path, b"authored invalid media").unwrap();
        let mut d = TimestampedDecoder::start(
            ffmpeg(path.to_str().unwrap()),
            Stdio::null(),
            64,
            64,
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(d.next().is_err());
        assert!(d.next().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn native_irregular_nonzero_source_pts_survive_without_framerate_quantization() {
        let dir = tempfile::tempdir().unwrap();
        let y4m = dir.path().join("source.y4m");
        let nut = dir.path().join("source.nut");
        let mut data = b"YUV4MPEG2 W64 H64 F8:1 Ip A1:1 Cmono\n".to_vec();
        for value in [40, 100, 180] {
            data.extend_from_slice(b"FRAME\n");
            data.extend_from_slice(&[value; 4096]);
        }
        std::fs::write(&y4m, data).unwrap();
        let status = Command::new("/usr/bin/ffmpeg")
            .args(["-v", "error", "-nostdin", "-filter_threads", "1", "-i"])
            .arg(&y4m)
            .args([
                "-vf",
                "settb=expr=1/1000,setpts=1234+N*125+N*N*17",
                "-fps_mode",
                "passthrough",
                "-enc_time_base",
                "filter",
                "-c:v",
                "ffv1",
                "-threads",
                "1",
            ])
            .arg(&nut)
            .status()
            .unwrap();
        assert!(status.success());
        let mut d = TimestampedDecoder::start(
            ffmpeg(nut.to_str().unwrap()),
            Stdio::null(),
            64,
            64,
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        for n in 0..3 {
            let f = d.next().unwrap().unwrap();
            let expected = SourceTimestamp::new(1234 + n * 125 + n * n * 17, 1000).unwrap();
            assert_eq!(
                ExactSeconds::from_source(f.source_time)
                    .checked_cmp(ExactSeconds::from_source(expected))
                    .unwrap(),
                std::cmp::Ordering::Equal
            );
        }
        assert!(d.next().unwrap().is_none());
    }
    #[cfg(unix)]
    #[test]
    fn wrong_pixel_hash_and_unterminated_packet_stream_poison_and_reap() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = dir.path().join("pixels");
        let packets = dir.path().join("packets");
        std::fs::write(&pixels, [0; 12288]).unwrap();
        for truncated in [false, true] {
            let mut wire=String::from("#format: frame checksums\n#version: 2\n#hash: SHA256\n#tb 0: 1/1000\n#media_type 0: video\n#codec_id 0: rawvideo\n#dimensions 0: 64x64\n");
            wire += &line(1, 12288, &"a".repeat(64));
            if !truncated {
                wire.push('\n');
            }
            std::fs::write(&packets, wire).unwrap();
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "cat \"$1\"; cat \"$2\" >&2", "authored-peer"])
                .arg(&pixels)
                .arg(&packets);
            let mut d = TimestampedDecoder::start(
                command,
                Stdio::null(),
                64,
                64,
                Instant::now() + Duration::from_secs(5),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert!(d.next().is_err());
            assert!(d.next().is_err());
            assert!(d.child.lock().unwrap().try_wait().unwrap().is_some());
        }
    }
}
