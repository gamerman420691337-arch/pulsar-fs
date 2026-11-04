//! Authored Unix peers exercise framing and client failure behavior, not capture hardware.
#![cfg(unix)]
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("bulk.sock"));
        let _ = fs::remove_dir(&self.0);
    }
}
fn peer(server: impl FnOnce(UnixStream) + Send + 'static, client: impl FnOnce(&Path)) {
    let dir = Directory(std::env::temp_dir().join(format!(
        "plt-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&dir.0).unwrap();
    let path = dir.0.join("bulk.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        server(stream);
    });
    let client_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client(&path)));
    let server_result = handle.join();
    if let Err(error) = client_result {
        std::panic::resume_unwind(error);
    }
    server_result.unwrap();
}
fn report() -> LiveCaptureReport {
    let identity = LiveIdentity {
        epoch: AttemptId::new("capture").unwrap(),
        project: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(2),
        source: SourceVersionId::new("source").unwrap(),
    };
    let options = LiveCaptureOptions {
        browser_analysis: None,
        browser_media: None,
        model_pack: None,
        source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
        continuation_mode: LiveContinuationMode::Immediate,
        encoded_source: None,
        inference: None,
        policy: LivePolicy {
            maximum_analysis_age_ns: 200_000_000,
            added_buffer_ns: 0,
        },
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        exact_project_anchor: None,
        presentation_anchor: LiveClockTick(0),
        lease_ms: 30_000,
    };
    LiveCaptureReport {
        input_close: None,
        continuation: None,
        age_basis: LiveCaptureAgeBasis::ProducerDeclared,
        encoded_source: None,
        engine_epoch: AttemptId::new("boot").unwrap(),
        clock: options.clock(identity.epoch.clone()),
        identity,
        options,
        active: true,
        retirement: None,
        server_elapsed: Some(LiveClockTick(10_000_000)),
        admitted_pixel_bytes: 61440,
        received_frames: 0,
        last_frame: None,
        inference: None,
        queue: None,
    }
}
fn scope() -> LiveBulkHandshake {
    let report = report();
    LiveBulkHandshake {
        version: PROTOCOL_VERSION,
        channel: LiveBulkChannel::LiveCapture,
        session: SessionId::new("actor").unwrap(),
        auth_token: "authored-private-token".into(),
        engine_epoch: report.engine_epoch,
        identity: report.identity,
    }
}
fn header() -> LiveFrameHeader {
    LiveFrameHeader {
        frame: FrameId::new(1),
        source_time: SourceTimestamp::new(1, 1000).unwrap(),
        capture_tick: LiveClockTick(1),
        sha256: format!("{:x}", Sha256::digest(vec![7; LIVE_FRAME_BYTES])),
    }
}
fn authenticate(stream: &mut UnixStream) {
    let actual: LiveBulkHandshake = read_bulk_header(stream).unwrap();
    assert_eq!(actual.identity, scope().identity);
    write_bulk_header(stream, &LiveBulkReply::Ready(report())).unwrap();
}
#[test]
fn live_pixels_follow_permission_and_exact_receipt_and_local_errors_do_not_poison() {
    peer(
        |mut stream| {
            authenticate(&mut stream);
            for replayed in [false, true] {
                let actual: LiveFrameHeader = read_bulk_header(&mut stream).unwrap();
                assert_eq!(actual, header());
                write_bulk_header(
                    &mut stream,
                    &LiveBulkReply::SendPixels {
                        frame: actual.frame,
                    },
                )
                .unwrap();
                let mut bytes = [0u8; LIVE_FRAME_BYTES];
                stream.read_exact(&mut bytes).unwrap();
                assert_eq!(bytes, [7; LIVE_FRAME_BYTES]);
                let mut report = report();
                report.received_frames = 1;
                report.last_frame = Some(actual.clone());
                write_bulk_header(
                    &mut stream,
                    &LiveBulkReply::Accepted {
                        frame: actual.frame,
                        replayed,
                        report,
                    },
                )
                .unwrap();
            }
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.handshake(&scope()).unwrap();
            assert!(client.upload(&header(), &[]).is_err());
            assert!(client
                .upload(&header(), &vec![8; LIVE_FRAME_BYTES])
                .is_err());
            assert_eq!(
                client
                    .upload(&header(), &vec![7; LIVE_FRAME_BYTES])
                    .unwrap()
                    .received_frames,
                1
            );
            assert_eq!(
                client
                    .upload(&header(), &vec![7; LIVE_FRAME_BYTES])
                    .unwrap()
                    .received_frames,
                1
            );
        },
    );
}
#[test]
fn changed_frame_permission_poisoned_before_any_pixel_body() {
    peer(
        |mut stream| {
            authenticate(&mut stream);
            let _: LiveFrameHeader = read_bulk_header(&mut stream).unwrap();
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::SendPixels {
                    frame: FrameId::new(2),
                },
            )
            .unwrap();
            let mut byte = [0u8; 1];
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.handshake(&scope()).unwrap();
            assert!(matches!(
                client.upload(&header(), &vec![7; LIVE_FRAME_BYTES]),
                Err(TransportError::ResponseMismatch)
            ));
            assert!(matches!(
                client.upload(&header(), &vec![7; LIVE_FRAME_BYTES]),
                Err(TransportError::ConnectionPoisoned)
            ));
        },
    );
}
#[test]
fn changed_post_body_identity_poisoned_with_no_automatic_replay() {
    peer(
        |mut stream| {
            authenticate(&mut stream);
            let actual: LiveFrameHeader = read_bulk_header(&mut stream).unwrap();
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::SendPixels {
                    frame: actual.frame,
                },
            )
            .unwrap();
            let mut bytes = [0u8; LIVE_FRAME_BYTES];
            stream.read_exact(&mut bytes).unwrap();
            let mut report = report();
            report.identity.source = SourceVersionId::new("foreign-source").unwrap();
            report.received_frames = 1;
            report.last_frame = Some(actual.clone());
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::Accepted {
                    frame: actual.frame,
                    replayed: false,
                    report,
                },
            )
            .unwrap();
            let mut byte = [0u8; 1];
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.handshake(&scope()).unwrap();
            assert!(matches!(
                client.upload(&header(), &vec![7; LIVE_FRAME_BYTES]),
                Err(TransportError::ResponseMismatch)
            ));
            assert!(matches!(
                client.upload(&header(), &vec![7; LIVE_FRAME_BYTES]),
                Err(TransportError::ConnectionPoisoned)
            ));
        },
    );
}

#[test]
fn measured_clock_bracket_contains_actual_same_host_capture_and_upload_preserves_source() {
    let origin = Instant::now();
    peer(
        move |mut stream| {
            let actual: LiveBulkHandshake = read_bulk_header(&mut stream).unwrap();
            assert_eq!(actual.identity, scope().identity);
            thread::sleep(Duration::from_millis(25));
            let mut ready = report();
            ready.server_elapsed = Some(LiveClockTick(origin.elapsed().as_nanos() as u64));
            write_bulk_header(&mut stream, &LiveBulkReply::Ready(ready)).unwrap();
            let frame: LiveFrameHeader = read_bulk_header(&mut stream).unwrap();
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::SendPixels { frame: frame.frame },
            )
            .unwrap();
            let mut pixels = [0u8; LIVE_FRAME_BYTES];
            stream.read_exact(&mut pixels).unwrap();
            assert_eq!(pixels, [7; LIVE_FRAME_BYTES]);
            let mut accepted = report();
            accepted.server_elapsed = Some(LiveClockTick(origin.elapsed().as_nanos() as u64));
            assert!(frame.capture_tick <= accepted.server_elapsed.unwrap());
            accepted.received_frames = 1;
            accepted.last_frame = Some(frame.clone());
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::Accepted {
                    frame: frame.frame,
                    replayed: false,
                    report: accepted,
                },
            )
            .unwrap();
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            assert!(client.clock_observation().is_err());
            client.handshake(&scope()).unwrap();
            let captured = Instant::now();
            let actual_tick = captured.duration_since(origin).as_nanos() as u64;
            let bounds = client
                .clock_observation()
                .unwrap()
                .bounds_at(captured)
                .unwrap();
            assert!(bounds.earliest.0 <= actual_tick && actual_tick <= bounds.latest.0);
            assert!(bounds.uncertainty_ns >= 25_000_000);
            let source = SourceTimestamp::new(-5, 7).unwrap();
            assert!(client
                .upload_captured(FrameId::new(1), source, origin, &[7; LIVE_FRAME_BYTES])
                .is_err());
            let accepted = client
                .upload_captured(FrameId::new(1), source, captured, &[7; LIVE_FRAME_BYTES])
                .unwrap();
            assert_eq!(accepted.stamp.bounds, bounds);
            assert_eq!(accepted.stamp.header.capture_tick, bounds.earliest);
            assert_eq!(accepted.stamp.header.source_time, source);
            assert_eq!(
                accepted.report.last_frame.as_ref(),
                Some(&accepted.stamp.header)
            );
            assert_eq!(accepted.stamp.clock.identity, scope().identity);
            assert_eq!(accepted.stamp.clock.engine_epoch, scope().engine_epoch);
        },
    );
}

#[test]
fn clock_observation_dies_with_poisoned_connection() {
    peer(
        |mut stream| {
            authenticate(&mut stream);
            let actual: LiveFrameHeader = read_bulk_header(&mut stream).unwrap();
            write_bulk_header(
                &mut stream,
                &LiveBulkReply::SendPixels {
                    frame: FrameId::new(actual.frame.value() + 1),
                },
            )
            .unwrap();
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.handshake(&scope()).unwrap();
            assert!(client.clock_observation().is_ok());
            assert!(client
                .upload_captured(
                    FrameId::new(1),
                    SourceTimestamp::new(1, 1000).unwrap(),
                    Instant::now(),
                    &[7; LIVE_FRAME_BYTES]
                )
                .is_err());
            assert!(matches!(
                client.clock_observation(),
                Err(TransportError::ConnectionPoisoned)
            ));
        },
    );
}

fn timeout(error: BulkError) {
    assert!(
        matches!(&error, TransportError::Io(error) if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock)),
        "expected timeout, received {error:?}"
    );
}

fn receive_frame(stream: &mut UnixStream) -> LiveFrameHeader {
    let actual: LiveFrameHeader = read_bulk_header(stream).unwrap();
    write_bulk_header(
        stream,
        &LiveBulkReply::SendPixels {
            frame: actual.frame,
        },
    )
    .unwrap();
    let mut pixels = [0; LIVE_FRAME_BYTES];
    stream.read_exact(&mut pixels).unwrap();
    assert_eq!(pixels, [7; LIVE_FRAME_BYTES]);
    actual
}

fn accept_frame(stream: &mut UnixStream) {
    let actual = receive_frame(stream);
    let mut accepted = report();
    accepted.server_elapsed = Some(LiveClockTick(actual.capture_tick.0.max(10_000_000)));
    accepted.received_frames = 1;
    accepted.last_frame = Some(actual.clone());
    write_bulk_header(
        stream,
        &LiveBulkReply::Accepted {
            frame: actual.frame,
            replayed: false,
            report: accepted,
        },
    )
    .unwrap();
}

fn no_bytes(stream: &mut UnixStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut byte = [0];
    let error = stream.read(&mut byte).unwrap_err();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
}

#[test]
fn live_expired_operation_deadline_refuses_before_handshake_and_can_be_replaced() {
    let (checked, observed) = mpsc::channel();
    peer(
        move |mut stream| {
            no_bytes(&mut stream);
            checked.send(()).unwrap();
            authenticate(&mut stream);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.set_operation_deadline(Some(Instant::now()));
            timeout(client.handshake(&scope()).unwrap_err());
            observed.recv_timeout(Duration::from_secs(3)).unwrap();
            client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
            client.handshake(&scope()).unwrap();
            assert!(client.clock_observation().is_ok());
        },
    );
}

#[test]
fn live_absolute_deadline_survives_calls_and_expired_upload_writes_nothing() {
    let (check_next, check_requested) = mpsc::channel();
    let (checked, observed) = mpsc::channel();
    peer(
        move |mut stream| {
            authenticate(&mut stream);
            accept_frame(&mut stream);
            check_requested
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            no_bytes(&mut stream);
            checked.send(()).unwrap();
            accept_frame(&mut stream);
            accept_frame(&mut stream);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            client.set_operation_deadline(Some(deadline));
            client.handshake(&scope()).unwrap();
            client.upload(&header(), &[7; LIVE_FRAME_BYTES]).unwrap();
            let original =
                serde_json::to_value(client.clock_observation().unwrap().facts()).unwrap();
            thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
            );
            timeout(
                client
                    .upload(&header(), &[7; LIVE_FRAME_BYTES])
                    .unwrap_err(),
            );
            timeout(
                client
                    .upload_captured(
                        FrameId::new(2),
                        SourceTimestamp::new(2, 1000).unwrap(),
                        Instant::now(),
                        &[7; LIVE_FRAME_BYTES],
                    )
                    .unwrap_err(),
            );
            check_next.send(()).unwrap();
            observed.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(
                serde_json::to_value(client.clock_observation().unwrap().facts()).unwrap(),
                original
            );
            client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
            let captured = client
                .upload_captured(
                    FrameId::new(2),
                    SourceTimestamp::new(2, 1000).unwrap(),
                    Instant::now(),
                    &[7; LIVE_FRAME_BYTES],
                )
                .unwrap();
            assert_eq!(
                serde_json::to_value(captured.stamp.clock).unwrap(),
                original
            );
            client.set_operation_deadline(Some(Instant::now()));
            client.set_operation_deadline(None);
            client.upload(&header(), &[7; LIVE_FRAME_BYTES]).unwrap();
            assert_eq!(
                serde_json::to_value(client.clock_observation().unwrap().facts()).unwrap(),
                original
            );
        },
    );
}

#[test]
fn live_absolute_deadline_bounds_an_exchange_and_replacement_cannot_revive_poison() {
    let (finished, completion) = mpsc::channel();
    peer(
        move |mut stream| {
            authenticate(&mut stream);
            receive_frame(&mut stream);
            // No accepted receipt: the absolute bound must finish the caller
            // before its ordinary three-second timeout can do so.
            completion.recv_timeout(Duration::from_secs(2)).unwrap();
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_secs(3)).unwrap();
            client.handshake(&scope()).unwrap();
            client.set_operation_deadline(Some(Instant::now() + Duration::from_millis(300)));
            timeout(
                client
                    .upload(&header(), &[7; LIVE_FRAME_BYTES])
                    .unwrap_err(),
            );
            finished.send(()).unwrap();
            client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
            assert!(matches!(
                client.upload(&header(), &[7; LIVE_FRAME_BYTES]),
                Err(TransportError::ConnectionPoisoned)
            ));
            client.set_operation_deadline(None);
            assert!(matches!(
                client.clock_observation(),
                Err(TransportError::ConnectionPoisoned)
            ));
        },
    );
}

#[test]
fn live_operation_deadline_never_extends_the_ordinary_timeout() {
    let (finished, completion) = mpsc::channel();
    peer(
        move |mut stream| {
            authenticate(&mut stream);
            receive_frame(&mut stream);
            completion.recv_timeout(Duration::from_secs(2)).unwrap();
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_millis(300)).unwrap();
            client.handshake(&scope()).unwrap();
            client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
            timeout(
                client
                    .upload(&header(), &[7; LIVE_FRAME_BYTES])
                    .unwrap_err(),
            );
            finished.send(()).unwrap();
        },
    );
}

#[test]
fn live_default_timeout_still_starts_fresh_for_each_retained_operation() {
    peer(
        |mut stream| {
            authenticate(&mut stream);
            accept_frame(&mut stream);
        },
        |path| {
            let mut client = LiveBulkClient::connect(path, Duration::from_millis(300)).unwrap();
            client.handshake(&scope()).unwrap();
            // With no absolute bound, idle time between completed operations
            // does not consume the ordinary timeout of the next call.
            thread::sleep(Duration::from_millis(350));
            client.upload(&header(), &[7; LIVE_FRAME_BYTES]).unwrap();
        },
    );
}
