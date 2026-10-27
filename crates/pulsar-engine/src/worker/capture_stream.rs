//! Private encoded source role. The engine must own snapshots, confinement,
//! transport admission, authority and publication; these replies grant none.
use super::*;
#[cfg(not(target_os = "linux"))]
pub(super) fn run() -> Result<()> {
    bail!("native encoded source confinement unsupported on this platform")
}
#[cfg(target_os = "linux")]
pub(super) fn run() -> Result<()> {
    use std::os::fd::FromRawFd;
    if std::env::var("PULSAR_WORKER_CONFINED").as_deref() != Ok("1") {
        bail!("encoded source role requires parent-owned confinement");
    }
    // File reads do not prefetch encoded bytes while reading the framed
    // manifest. The owned duplicate is the sole reader and closes on drop.
    let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut input = unsafe { std::fs::File::from_raw_fd(fd) };
    let manifest: LiveDecodeManifest = read_bulk_header(&mut input)?;
    let mut output = std::io::stdout().lock();
    let result = execute(&manifest, input, &mut output);
    if let Err(error) = &result {
        let diagnostic = error.to_string().chars().take(512).collect::<String>();
        let _ = write_bulk_header(
            &mut output,
            &LiveDecodeReply::Error(ProtocolError::new(ErrorCode::Unavailable, diagnostic)),
        );
        let _ = std::io::Write::flush(&mut output);
    }
    result
}
#[cfg(target_os = "linux")]
fn execute(
    manifest: &LiveDecodeManifest,
    mut source: std::fs::File,
    output: &mut impl std::io::Write,
) -> Result<()> {
    use std::{
        io::{Read, Write},
        os::fd::AsRawFd,
        process::{Command, Stdio},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
    };
    manifest.validate()?;
    let origin = Instant::now();
    let deadline = origin + Duration::from_millis(manifest.budget.wall_time_ms);
    artifacts::verify_artifact(&manifest.ffmpeg.path, &manifest.ffmpeg.identity, deadline)?;
    let mut command = Command::new(&manifest.ffmpeg.path);
    command.args(["-v","quiet","-nostdin","-copyts","-xerror","-filter_threads","1","-threads",&manifest.budget.cpu_threads.to_string(),"-protocol_whitelist","pipe","-f",manifest.format.demuxer(),"-probesize","32","-analyzeduration","0","-err_detect","explode","-noautorotate","-i","pipe:0","-map","0:v:0","-an","-sn","-dn","-vf","scale=64:64,format=rgb24","-fps_mode","passthrough","-enc_time_base","demux","-c:v","rawvideo","-threads","1","-f","tee","[f=rawvideo:flush_packets=1]pipe:1|[f=framehash:hash=sha256:avoid_negative_ts=disabled:flush_packets=1]pipe:2"]);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut decoder = timestamped_decode::TimestampedDecoder::start(
        command,
        Stdio::piped(),
        64,
        64,
        deadline,
        cancel.clone(),
    )?;
    let mut sink = decoder.take_input()?;
    let elapsed = || u64::try_from(origin.elapsed().as_nanos());
    let ready = LiveDecodeReply::Ready {
        attempt_id: manifest.attempt_id.clone(),
        identity: manifest.identity.clone(),
        format: manifest.format,
        worker_elapsed_ns: elapsed()?,
    };
    ready.validate(manifest)?;
    write_bulk_header(output, &ready)?;
    output.flush()?;
    let maximum = manifest.maximum_input_bytes;
    let intake_cancel = cancel.clone();
    let intake = thread::Builder::new()
        .name("pulsar-encoded-intake".into())
        .spawn(move || -> Result<(u64, String)> {
            let result = (|| {
                let mut bytes = 0u64;
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 65536];
                loop {
                    if intake_cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
                        bail!("encoded source intake cancelled or expired");
                    }
                    let mut poll = libc::pollfd {
                        fd: source.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let ready = unsafe { libc::poll(&mut poll, 1, 10) };
                    if ready < 0 {
                        let error = std::io::Error::last_os_error();
                        if error.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(error.into());
                    }
                    if ready == 0 {
                        continue;
                    }
                    let length = usize::try_from((maximum - bytes + 1).min(buffer.len() as u64))?;
                    let n = source.read(&mut buffer[..length])?;
                    if n == 0 {
                        break;
                    }
                    bytes = bytes
                        .checked_add(n as u64)
                        .context("encoded input byte sum overflow")?;
                    if bytes > maximum {
                        bail!("encoded source exceeds admitted byte limit");
                    }
                    sink.write_all(&buffer[..n])?;
                    hash.update(&buffer[..n]);
                }
                sink.flush()?;
                Ok((bytes, format!("{:x}", hash.finalize())))
            })();
            if result.is_err() {
                intake_cancel.store(true, Ordering::Release);
            }
            result
        })?;
    let decoded = (|| -> Result<u32> {
        let mut count = 0u32;
        while let Some(frame) = decoder.next()? {
            if count >= manifest.maximum_frames {
                bail!("encoded source exceeds admitted frame limit");
            }
            let hash = format!("{:x}", Sha256::digest(&frame.pixels));
            let reply = LiveDecodeReply::Frame {
                attempt_id: manifest.attempt_id.clone(),
                identity: manifest.identity.clone(),
                frame: FrameId::new(frame.ordinal),
                source_time: frame.source_time,
                delivered_elapsed_ns: u64::try_from(
                    frame.delivered.duration_since(origin).as_nanos(),
                )?,
                pixels: ArtifactIdentity {
                    artifact_id: ArtifactId::new(hash.clone())?,
                    sha256: hash,
                    byte_len: LIVE_FRAME_BYTES as u64,
                },
            };
            reply.validate(manifest)?;
            write_bulk_header(output, &reply)?;
            output.write_all(&frame.pixels)?;
            output.flush()?;
            count = count
                .checked_add(1)
                .context("native source frame count overflow")?;
        }
        Ok(count)
    })();
    // Close/reap the native consumer before joining a potentially blocked
    // writer; cancellation also interrupts idle source polling.
    if decoded.is_err() {
        cancel.store(true, Ordering::Release);
    }
    drop(decoder);
    let received = intake
        .join()
        .map_err(|_| anyhow::anyhow!("encoded intake thread panicked"))?;
    let count = decoded?;
    let (input_bytes, input_sha256) = received?;
    let end = LiveDecodeReply::End {
        attempt_id: manifest.attempt_id.clone(),
        identity: manifest.identity.clone(),
        frames: count,
        input_sha256,
        input_bytes,
    };
    end.validate(manifest)?;
    write_bulk_header(output, &end)?;
    output.flush()?;
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    fn manifest() -> LiveDecodeManifest {
        let tool = artifacts::pin_artifact(std::path::Path::new("/usr/bin/ffmpeg")).unwrap();
        LiveDecodeManifest {
            version: PROTOCOL_VERSION,
            attempt_id: AttemptId::new("source-attempt").unwrap(),
            identity: LiveIdentity {
                epoch: AttemptId::new("capture").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(0),
                source: SourceVersionId::new("source").unwrap(),
            },
            worker: tool.identity.clone(),
            ffmpeg: tool,
            format: LiveEncodedFormat::Yuv4mpegPipe,
            maximum_input_bytes: 1 << 20,
            maximum_frames: 10,
            budget: ResourceBudget {
                memory_bytes: 1 << 30,
                output_bytes: 1 << 20,
                wall_time_ms: 3000,
                cpu_threads: 1,
            },
        }
    }
    fn fixture() -> Vec<u8> {
        let mut bytes = b"YUV4MPEG2 W64 H64 F4:1 Ip A1:1 Cmono\n".to_vec();
        for p in [0, 128, 255] {
            bytes.extend_from_slice(b"FRAME\n");
            bytes.extend_from_slice(&[p; 4096]);
        }
        bytes
    }
    #[test]
    fn native_source_role_reports_exact_intake_hash_clock_and_fixed_rgb_frames() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.y4m");
        let bytes = fixture();
        std::fs::write(&source, &bytes).unwrap();
        let m = manifest();
        let mut output = vec![];
        execute(&m, std::fs::File::open(source).unwrap(), &mut output).unwrap();
        let mut input = std::io::Cursor::new(output);
        let ready: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
        ready.validate(&m).unwrap();
        let LiveDecodeReply::Ready {
            worker_elapsed_ns, ..
        } = ready
        else {
            panic!("readiness missing")
        };
        for n in 0..3 {
            let frame: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
            frame.validate(&m).unwrap();
            let LiveDecodeReply::Frame {
                frame,
                source_time,
                delivered_elapsed_ns,
                pixels,
                ..
            } = frame
            else {
                panic!("frame missing")
            };
            assert_eq!(frame, FrameId::new(n));
            assert_eq!(source_time, SourceTimestamp::new(n as i64, 4).unwrap());
            assert!(delivered_elapsed_ns >= worker_elapsed_ns);
            let mut body = [0u8; LIVE_FRAME_BYTES];
            std::io::Read::read_exact(&mut input, &mut body).unwrap();
            assert_eq!(format!("{:x}", Sha256::digest(body)), pixels.sha256);
        }
        let end: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
        end.validate(&m).unwrap();
        let LiveDecodeReply::End {
            frames,
            input_sha256,
            input_bytes,
            ..
        } = end
        else {
            panic!("end missing")
        };
        assert_eq!(frames, 3);
        assert_eq!(input_bytes, bytes.len() as u64);
        assert_eq!(input_sha256, format!("{:x}", Sha256::digest(bytes)));
        assert_eq!(input.position(), input.get_ref().len() as u64);
    }
    #[test]
    fn native_source_intake_and_frame_limits_refuse_without_end_or_replay() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.y4m");
        std::fs::write(&source, fixture()).unwrap();
        for cap_bytes in [false, true] {
            let mut m = manifest();
            if cap_bytes {
                m.maximum_input_bytes = 16;
            } else {
                m.maximum_frames = 1;
            }
            let mut out = vec![];
            assert!(execute(&m, std::fs::File::open(&source).unwrap(), &mut out).is_err());
            let mut input = std::io::Cursor::new(out);
            let _: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
            if !cap_bytes {
                let reply: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
                assert!(matches!(reply, LiveDecodeReply::Frame { .. }));
            }
        }
    }
    #[test]
    fn native_source_wrong_tool_bytes_and_wrong_declared_format_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.y4m");
        std::fs::write(&source, fixture()).unwrap();
        let mut m = manifest();
        m.ffmpeg.identity.sha256 = "a".repeat(64);
        let mut out = vec![];
        assert!(execute(&m, std::fs::File::open(&source).unwrap(), &mut out).is_err());
        assert!(out.is_empty());
        m = manifest();
        m.format = LiveEncodedFormat::Nut;
        assert!(execute(&m, std::fs::File::open(&source).unwrap(), &mut vec![]).is_err());
    }
    #[test]
    fn native_source_declared_nut_and_matroska_decode_with_exact_intake_correspondence() {
        let dir = tempfile::tempdir().unwrap();
        let y4m = dir.path().join("source.y4m");
        std::fs::write(&y4m, fixture()).unwrap();
        for format in [LiveEncodedFormat::Nut, LiveEncodedFormat::Matroska] {
            let source = dir.path().join(format.demuxer());
            let status = std::process::Command::new("/usr/bin/ffmpeg")
                .args(["-v", "error", "-nostdin", "-filter_threads", "1", "-i"])
                .arg(&y4m)
                .args(["-c:v", "ffv1", "-threads", "1", "-f", format.demuxer()])
                .arg(&source)
                .status()
                .unwrap();
            assert!(status.success());
            let mut m = manifest();
            m.format = format;
            let mut out = vec![];
            execute(&m, std::fs::File::open(&source).unwrap(), &mut out).unwrap();
            let mut input = std::io::Cursor::new(out);
            let ready: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
            ready.validate(&m).unwrap();
            for n in 0..3 {
                let frame: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
                frame.validate(&m).unwrap();
                let LiveDecodeReply::Frame {
                    source_time,
                    pixels,
                    ..
                } = frame
                else {
                    panic!("frame missing")
                };
                assert_eq!(
                    ExactSeconds::from_source(source_time)
                        .checked_cmp(ExactSeconds::new(n, 4).unwrap())
                        .unwrap(),
                    std::cmp::Ordering::Equal
                );
                let mut body = [0; LIVE_FRAME_BYTES];
                std::io::Read::read_exact(&mut input, &mut body).unwrap();
                assert_eq!(format!("{:x}", Sha256::digest(body)), pixels.sha256);
            }
            let end: LiveDecodeReply = read_bulk_header(&mut input).unwrap();
            end.validate(&m).unwrap();
            let LiveDecodeReply::End {
                input_bytes,
                input_sha256,
                ..
            } = end
            else {
                panic!("end missing")
            };
            let bytes = std::fs::read(source).unwrap();
            assert_eq!(input_bytes, bytes.len() as u64);
            assert_eq!(input_sha256, format!("{:x}", Sha256::digest(bytes)));
        }
    }
    #[test]
    fn native_encoded_stream_emits_framed_pixels_while_producer_remains_open() {
        use std::{
            io::{Read, Write},
            os::{fd::OwnedFd, unix::net::UnixStream},
            sync::{
                atomic::{AtomicBool, Ordering},
                mpsc, Arc,
            },
        };
        let (source, mut producer) = UnixStream::pair().unwrap();
        let source: OwnedFd = source.into();
        let (mut out, mut observation) = UnixStream::pair().unwrap();
        let (release, wait) = mpsc::sync_channel(1);
        let closed = Arc::new(AtomicBool::new(false));
        let marker = closed.clone();
        let bytes = fixture();
        let expected = format!("{:x}", Sha256::digest(&bytes));
        let sender = std::thread::spawn(move || {
            producer.write_all(&bytes).unwrap();
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
            marker.store(true, Ordering::Release);
        });
        let m = manifest();
        let scope = m.clone();
        let observer = std::thread::spawn(move || {
            observation
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let ready: LiveDecodeReply = read_bulk_header(&mut observation).unwrap();
            ready.validate(&scope).unwrap();
            for n in 0..3 {
                let header: LiveDecodeReply = read_bulk_header(&mut observation).unwrap();
                header.validate(&scope).unwrap();
                let LiveDecodeReply::Frame { frame, pixels, .. } = header else {
                    panic!("frame missing")
                };
                assert_eq!(frame, FrameId::new(n));
                let mut body = [0; LIVE_FRAME_BYTES];
                observation.read_exact(&mut body).unwrap();
                assert_eq!(format!("{:x}", Sha256::digest(body)), pixels.sha256);
            }
            assert!(
                !closed.load(Ordering::Acquire),
                "EOF cannot substitute for streaming delivery"
            );
            release.send(()).unwrap();
            let end: LiveDecodeReply = read_bulk_header(&mut observation).unwrap();
            end.validate(&scope).unwrap();
            let LiveDecodeReply::End {
                input_sha256,
                frames,
                ..
            } = end
            else {
                panic!("completion missing")
            };
            assert_eq!(input_sha256, expected);
            assert_eq!(frames, 3);
        });
        execute(&m, std::fs::File::from(source), &mut out).unwrap();
        sender.join().unwrap();
        observer.join().unwrap();
    }
}
