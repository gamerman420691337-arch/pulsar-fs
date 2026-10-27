//! Single-attachment encoded transport. Ambiguous remote/I/O results poison
//! this connection; status reconciliation never authorizes automatic rearm.
use crate::*;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};
pub struct LiveEncodedClient {
    manifest_sha256: Option<String>,
    stream: Option<LocalStream>,
    timeout: Duration,
    absolute_deadline: Option<Instant>,
    scope: Option<LiveBulkHandshake>,
    sequence: u64,
    bytes: u64,
    maximum: u64,
}
impl LiveEncodedClient {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, BulkError> {
        if timeout.is_zero() {
            return Err(ProtocolError::invalid("zero encoded timeout").into());
        }
        Ok(Self {
            manifest_sha256: None,
            stream: Some(crate::transport::connect_local(path.as_ref(), timeout)?),
            timeout,
            absolute_deadline: None,
            scope: None,
            sequence: 0,
            bytes: 0,
            maximum: 0,
        })
    }
    fn deadline(&self) -> Result<Instant, BulkError> {
        let end = Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(|| ProtocolError::invalid("encoded timeout overflow"))?;
        let end = self
            .absolute_deadline
            .map_or(end, |absolute| end.min(absolute));
        if end <= Instant::now() {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "encoded absolute deadline expired",
            )
            .into());
        }
        Ok(end)
    }
    /// Tighten all subsequent I/O exchanges to one caller-owned absolute end.
    /// This cannot extend an earlier end or revive a poisoned connection.
    pub fn limit_deadline(&mut self, end: Instant) -> Result<(), BulkError> {
        if self.stream.is_none() {
            return Err(TransportError::ConnectionPoisoned);
        }
        if end <= Instant::now() {
            return Err(ProtocolError::invalid("encoded deadline already expired").into());
        }
        self.absolute_deadline = Some(self.absolute_deadline.map_or(end, |prior| prior.min(end)));
        Ok(())
    }
    fn check(scope: &LiveBulkHandshake, report: &LiveCaptureReport) -> Result<(), BulkError> {
        report.validate()?;
        if !report.active
            || report.identity != scope.identity
            || report.engine_epoch != scope.engine_epoch
            || report.age_basis != LiveCaptureAgeBasis::DecodedStdoutLocalMonotonicEqualRate
            || report.encoded_source.is_none()
        {
            return Err(TransportError::ResponseMismatch);
        }
        Ok(())
    }
    fn manifest_hash(report: &LiveCaptureReport) -> Result<String, BulkError> {
        let manifest = report
            .encoded_source
            .as_ref()
            .and_then(|s| s.manifest.as_ref())
            .ok_or(TransportError::ResponseMismatch)?;
        Ok(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(manifest)
                    .map_err(|_| ProtocolError::invalid("encoded manifest encoding"))?
            )
        ))
    }
    fn check_manifest(&self, report: &LiveCaptureReport) -> Result<(), BulkError> {
        if self.manifest_sha256.as_ref() != Some(&Self::manifest_hash(report)?) {
            return Err(TransportError::ResponseMismatch);
        }
        Ok(())
    }
    pub fn handshake(&mut self, scope: &LiveBulkHandshake) -> Result<LiveCaptureReport, BulkError> {
        scope.validate()?;
        if scope.channel != LiveBulkChannel::LiveEncodedSource || self.scope.is_some() {
            return Err(ProtocolError::invalid("encoded handshake marker/state differs").into());
        }
        let end = self.deadline()?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        write_bulk_header(&mut io, scope)?;
        let report = match read_bulk_header::<_, LiveEncodedReply>(&mut io)? {
            LiveEncodedReply::Ready(report) => report,
            LiveEncodedReply::Error(e) => return Err(e.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        Self::check(scope, &report)?;
        if report.encoded_source.as_ref().expect("checked").state != LiveEncodedSourceState::Running
        {
            return Err(TransportError::ResponseMismatch);
        }
        self.maximum = report
            .options
            .encoded_source
            .as_ref()
            .expect("checked")
            .maximum_input_bytes;
        self.manifest_sha256 = Some(Self::manifest_hash(&report)?);
        self.scope = Some(scope.clone());
        self.stream = Some(stream);
        Ok(report)
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<LiveCaptureReport, BulkError> {
        if bytes.is_empty() || bytes.len() > 65536 || bytes.len() as u64 > self.maximum - self.bytes
        {
            return Err(ProtocolError::invalid("encoded chunk exceeds local admission").into());
        }
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("encoded handshake required"))?;
        let end = self.deadline()?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        let request = LiveEncodedRequest::Push {
            sequence: self.sequence,
            byte_len: bytes.len() as u32,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        write_bulk_header(&mut io, &request)?;
        match read_bulk_header::<_, LiveEncodedReply>(&mut io)? {
            LiveEncodedReply::SendBytes { sequence } if sequence == self.sequence => {}
            LiveEncodedReply::Error(e) => return Err(e.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        io.write_all(bytes)?;
        let expected = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ProtocolError::invalid("encoded intake overflow"))?;
        let report = match read_bulk_header::<_, LiveEncodedReply>(&mut io)? {
            LiveEncodedReply::Accepted {
                sequence,
                input_bytes,
                report,
            } if sequence == self.sequence && input_bytes == expected => report,
            LiveEncodedReply::Error(e) => return Err(e.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        Self::check(scope, &report)?;
        self.check_manifest(&report)?;
        if report.encoded_source.as_ref().expect("checked").input_bytes != expected {
            return Err(TransportError::ResponseMismatch);
        }
        self.bytes = expected;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| ProtocolError::invalid("encoded sequence overflow"))?;
        self.stream = Some(stream);
        Ok(report)
    }
    pub fn finish(mut self) -> Result<LiveCaptureReport, BulkError> {
        let scope = self
            .scope
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("encoded handshake required"))?;
        let end = self.deadline()?;
        let mut stream = self
            .stream
            .take()
            .ok_or(TransportError::ConnectionPoisoned)?;
        let mut io = DeadlineStream::new(&mut stream, end);
        write_bulk_header(
            &mut io,
            &LiveEncodedRequest::Finish {
                sequence: self.sequence,
            },
        )?;
        let report = match read_bulk_header::<_, LiveEncodedReply>(&mut io)? {
            LiveEncodedReply::Finished(report) => report,
            LiveEncodedReply::Error(e) => return Err(e.into()),
            _ => return Err(TransportError::ResponseMismatch),
        };
        Self::check(scope, &report)?;
        self.check_manifest(&report)?;
        let info = report.encoded_source.as_ref().expect("checked");
        if info.state != LiveEncodedSourceState::Finished || info.input_bytes != self.bytes {
            return Err(TransportError::ResponseMismatch);
        }
        Ok(report)
    }
}
