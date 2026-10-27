//! Bounded arbitrary source ranges. Native lease lifetime is enforced by Engine.
use crate::*;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Read},
    path::Path,
    time::{Duration, Instant},
};

pub const SOURCE_MEDIA_CHUNK_BYTES: u32 = 65_536;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMediaBulkChannel {
    SourceMedia,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMediaBulkHandshake {
    pub version: u16,
    pub channel: SourceMediaBulkChannel,
    pub session: SessionId,
    pub auth_token: String,
    pub lease_id: ArtifactId,
    pub engine_epoch: AttemptId,
    pub preparation_request_id: RequestId,
    pub source_sha256: String,
}
impl fmt::Debug for SourceMediaBulkHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceMediaBulkHandshake")
            .field("version", &self.version)
            .field("channel", &self.channel)
            .field("session", &self.session)
            .field("auth_token", &"[REDACTED]")
            .field("lease_id", &self.lease_id)
            .field("engine_epoch", &self.engine_epoch)
            .field("preparation_request_id", &self.preparation_request_id)
            .field("source_sha256", &self.source_sha256)
            .finish()
    }
}
impl SourceMediaBulkHandshake {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::unsupported("source media bulk version"));
        }
        bounded_text(&self.auth_token, 4096, "source media authentication token")?;
        validate_sha256(&self.source_sha256)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMediaBulkRequest {
    pub offset: u64,
    pub byte_len: u32,
}
impl SourceMediaBulkRequest {
    pub fn validate(&self, total_bytes: u64) -> Result<(), ProtocolError> {
        if !(1..=SOURCE_MEDIA_CHUNK_BYTES).contains(&self.byte_len)
            || self
                .offset
                .checked_add(u64::from(self.byte_len))
                .is_none_or(|end| end > total_bytes)
        {
            return Err(ProtocolError::invalid(
                "source media range is outside admitted bytes",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SourceMediaBulkReply {
    Ready {
        binding: Box<BrowserMediaBinding>,
    },
    Download {
        offset: u64,
        byte_len: u32,
        sha256: String,
    },
    Error(ProtocolError),
}
impl SourceMediaBulkReply {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Ready { binding } => binding.validate(),
            Self::Download {
                offset,
                byte_len,
                sha256,
            } => {
                SourceMediaBulkRequest {
                    offset: *offset,
                    byte_len: *byte_len,
                }
                .validate(u64::MAX)?;
                validate_sha256(sha256)
            }
            Self::Error(_) => Ok(()),
        }
    }
}

/// Invalid local ranges do not write. Ambiguous or mismatched remote exchanges
/// poison the stream; changing an operation deadline cannot restore it.
pub struct SourceMediaBulkClient {
    stream: Option<LocalStream>,
    timeout: Duration,
    operation_deadline: Option<Instant>,
    binding: Option<BrowserMediaBinding>,
}
impl SourceMediaBulkClient {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, BulkError> {
        if timeout.is_zero() {
            return Err(ProtocolError::invalid("zero source media timeout").into());
        }
        Ok(Self {
            stream: Some(crate::transport::connect_local(path.as_ref(), timeout)?),
            timeout,
            operation_deadline: None,
            binding: None,
        })
    }
    /// Replace only between operations; a call snapshots its effective bound once.
    pub fn set_operation_deadline(&mut self, deadline: Option<Instant>) {
        self.operation_deadline = deadline;
    }
    fn deadline(&self) -> Result<Instant, BulkError> {
        let now = Instant::now();
        let ordinary = now
            .checked_add(self.timeout)
            .ok_or_else(|| ProtocolError::invalid("source media timeout overflow"))?;
        let end = self
            .operation_deadline
            .map_or(ordinary, |absolute| absolute.min(ordinary));
        if end <= now {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "source media operation deadline expired",
            )
            .into());
        }
        Ok(end)
    }
    pub fn handshake(
        &mut self,
        request: &SourceMediaBulkHandshake,
    ) -> Result<BrowserMediaBinding, BulkError> {
        let end = self.deadline()?;
        request.validate()?;
        if self.binding.is_some() {
            return Err(ProtocolError::invalid("source media handshake already complete").into());
        }
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        write_bulk_header(&mut io, request)?;
        let binding = match read_bulk_header::<_, SourceMediaBulkReply>(&mut io)? {
            SourceMediaBulkReply::Ready { binding } => binding,
            SourceMediaBulkReply::Error(error) => return Err(error.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        binding.validate()?;
        if binding.lease_id != request.lease_id
            || binding.engine_epoch != request.engine_epoch
            || binding.preparation_request_id != request.preparation_request_id
            || binding.selection.source.sha256 != request.source_sha256
        {
            return Err(TransportError::ResponseMismatch);
        }
        // lease_ms describes the original lease; receipt must not restart it.
        self.binding = Some((*binding).clone());
        self.stream = Some(stream);
        Ok(*binding)
    }
    pub fn download(&mut self, offset: u64, byte_len: u32) -> Result<Vec<u8>, BulkError> {
        let end = self.deadline()?;
        let binding = self
            .binding
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("source media handshake required"))?;
        let request = SourceMediaBulkRequest { offset, byte_len };
        request.validate(binding.selection.source.byte_len)?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        write_bulk_header(&mut io, &request)?;
        let hash = match read_bulk_header::<_, SourceMediaBulkReply>(&mut io)? {
            SourceMediaBulkReply::Download {
                offset: actual,
                byte_len: length,
                sha256,
            } if actual == offset && length == byte_len => sha256,
            SourceMediaBulkReply::Error(error) => return Err(error.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        validate_sha256(&hash)?;
        // Existing artifact chunk framing, with the stricter range limit checked
        // before body allocation rather than the generic artifact maximum.
        let mut prefix = [0; 4];
        io.read_exact(&mut prefix)?;
        let actual = u32::from_be_bytes(prefix);
        if actual != byte_len {
            return Err(TransportError::InvalidLength(actual));
        }
        let mut bytes = vec![0; byte_len as usize];
        io.read_exact(&mut bytes)?;
        if format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err(TransportError::ResponseMismatch);
        }
        self.stream = Some(stream);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn handshake() -> SourceMediaBulkHandshake {
        let b = crate::browser_media::test_binding();
        SourceMediaBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: SourceMediaBulkChannel::SourceMedia,
            session: SessionId::new("actor").unwrap(),
            auth_token: "synthetic-private-token".into(),
            lease_id: b.lease_id,
            engine_epoch: b.engine_epoch,
            preparation_request_id: b.preparation_request_id,
            source_sha256: b.selection.source.sha256,
        }
    }
    #[test]
    fn source_media_bulk_marker_secrets_and_exact_range_bounds() {
        let h = handshake();
        h.validate().unwrap();
        assert!(!format!("{h:?}").contains("synthetic-private-token"));
        let mut value = serde_json::to_value(&h).unwrap();
        assert!(matches!(
            serde_json::from_value::<BulkHandshakeEnvelope>(value.clone()).unwrap(),
            BulkHandshakeEnvelope::SourceMedia(_)
        ));
        value.as_object_mut().unwrap().remove("channel");
        assert!(serde_json::from_value::<BulkHandshakeEnvelope>(value).is_err());
        for (offset, byte_len, total, valid) in [
            (0, 65536, 65536, true),
            (2, 3, 5, true),
            (5, 1, 5, false),
            (0, 0, 1, false),
            (0, 65537, 65537, false),
            (u64::MAX, 1, u64::MAX, false),
        ] {
            assert_eq!(
                SourceMediaBulkRequest { offset, byte_len }
                    .validate(total)
                    .is_ok(),
                valid
            );
        }
    }

    #[cfg(unix)]
    mod native {
        use super::*;
        use std::{
            fs,
            io::Write,
            os::unix::net::{UnixListener, UnixStream},
            path::PathBuf,
            sync::{
                atomic::{AtomicU64, Ordering},
                mpsc,
            },
            thread,
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
                "psmb-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
            fs::create_dir(&dir.0).unwrap();
            let path = dir.0.join("bulk.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let worker = thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                server(stream);
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client(&path)));
            let joined = worker.join();
            if let Err(error) = result {
                std::panic::resume_unwind(error);
            }
            joined.unwrap();
        }
        fn ready(stream: &mut UnixStream) {
            let h: SourceMediaBulkHandshake = read_bulk_header(stream).unwrap();
            h.validate().unwrap();
            write_bulk_header(
                stream,
                &SourceMediaBulkReply::Ready {
                    binding: Box::new(crate::browser_media::test_binding()),
                },
            )
            .unwrap();
        }
        fn send(stream: &mut UnixStream, r: SourceMediaBulkRequest) {
            let bytes: Vec<u8> = (r.offset..r.offset + u64::from(r.byte_len))
                .map(|n| n as u8)
                .collect();
            write_bulk_header(
                stream,
                &SourceMediaBulkReply::Download {
                    offset: r.offset,
                    byte_len: r.byte_len,
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                },
            )
            .unwrap();
            write_artifact_chunk(stream, &bytes).unwrap();
        }
        #[test]
        fn source_media_bulk_arbitrary_seek_retry_and_local_invalid_range_are_exact() {
            peer(
                |mut stream| {
                    ready(&mut stream);
                    for pair in [(7, 3), (0, 5), (7, 3)] {
                        let r: SourceMediaBulkRequest = read_bulk_header(&mut stream).unwrap();
                        assert_eq!((r.offset, r.byte_len), pair);
                        send(&mut stream, r);
                    }
                },
                |path| {
                    let mut c =
                        SourceMediaBulkClient::connect(path, Duration::from_secs(3)).unwrap();
                    let b = c.handshake(&handshake()).unwrap();
                    assert_eq!(b, crate::browser_media::test_binding());
                    assert!(c.download(100, 1).is_err());
                    for (offset, len) in [(7, 3), (0, 5), (7, 3)] {
                        assert_eq!(
                            c.download(offset, len).unwrap(),
                            (offset..offset + u64::from(len))
                                .map(|n| n as u8)
                                .collect::<Vec<_>>()
                        );
                    }
                },
            );
        }
        #[test]
        fn source_media_bulk_mismatched_identity_length_hash_or_truncation_poisons() {
            for case in 0..5 {
                peer(
                    move |mut stream| {
                        if case == 0 {
                            let _: SourceMediaBulkHandshake =
                                read_bulk_header(&mut stream).unwrap();
                            let mut b = crate::browser_media::test_binding();
                            b.engine_epoch = AttemptId::new("foreign").unwrap();
                            write_bulk_header(
                                &mut stream,
                                &SourceMediaBulkReply::Ready {
                                    binding: Box::new(b),
                                },
                            )
                            .unwrap();
                            return;
                        }
                        ready(&mut stream);
                        let r: SourceMediaBulkRequest = read_bulk_header(&mut stream).unwrap();
                        write_bulk_header(
                            &mut stream,
                            &SourceMediaBulkReply::Download {
                                offset: if case == 1 { r.offset + 1 } else { r.offset },
                                byte_len: r.byte_len,
                                sha256: if case == 2 {
                                    "0".repeat(64)
                                } else {
                                    format!("{:x}", Sha256::digest([1, 2, 3]))
                                },
                            },
                        )
                        .unwrap();
                        if case == 1 {
                            return;
                        }
                        if case == 3 {
                            stream.write_all(&65537u32.to_be_bytes()).unwrap();
                            return;
                        }
                        if case == 4 {
                            stream.write_all(&3u32.to_be_bytes()).unwrap();
                            stream.write_all(&[1, 2]).unwrap();
                            return;
                        }
                        write_artifact_chunk(&mut stream, &[1, 2, 3]).unwrap();
                    },
                    |path| {
                        let mut c =
                            SourceMediaBulkClient::connect(path, Duration::from_secs(3)).unwrap();
                        if case == 0 {
                            assert!(c.handshake(&handshake()).is_err());
                            assert!(matches!(
                                c.handshake(&handshake()),
                                Err(TransportError::ConnectionPoisoned)
                            ));
                        } else {
                            c.handshake(&handshake()).unwrap();
                            assert!(c.download(0, 3).is_err());
                            c.set_operation_deadline(None);
                            assert!(matches!(
                                c.download(0, 3),
                                Err(TransportError::ConnectionPoisoned)
                            ));
                        }
                    },
                );
            }
        }
        #[test]
        fn source_media_bulk_expired_bound_sends_nothing_and_explicit_replacement_is_local() {
            let (checked, observed) = mpsc::channel();
            peer(
                move |mut stream| {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(50)))
                        .unwrap();
                    let error = stream.read(&mut [0]).unwrap_err();
                    assert!(matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ));
                    checked.send(()).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    ready(&mut stream);
                    let r: SourceMediaBulkRequest = read_bulk_header(&mut stream).unwrap();
                    send(&mut stream, r);
                },
                |path| {
                    let mut c =
                        SourceMediaBulkClient::connect(path, Duration::from_secs(3)).unwrap();
                    c.set_operation_deadline(Some(Instant::now()));
                    assert!(
                        matches!(c.handshake(&handshake()),Err(TransportError::Io(e)) if e.kind()==io::ErrorKind::TimedOut)
                    );
                    observed.recv_timeout(Duration::from_secs(3)).unwrap();
                    c.set_operation_deadline(Some(Instant::now() + Duration::from_secs(3)));
                    c.handshake(&handshake()).unwrap();
                    c.set_operation_deadline(Some(Instant::now()));
                    assert!(c.download(0, 3).is_err());
                    c.set_operation_deadline(None);
                    assert_eq!(c.download(0, 3).unwrap(), [0, 1, 2]);
                },
            );
        }
    }
}
