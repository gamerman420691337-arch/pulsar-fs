use crate::{read_message, write_message, Request, Response, TransportError, PROTOCOL_VERSION};
use interprocess::local_socket::prelude::*;
pub use interprocess::local_socket::{
    prelude as local_socket_prelude, ConnectOptions, GenericFilePath, Listener as LocalListener,
    ListenerOptions, Name as LocalName, Stream as LocalStream,
};
pub use interprocess::ConnectWaitMode;
use std::{
    io::{self, Read, Write},
    path::Path,
    time::{Duration, Instant},
};

pub fn endpoint_name(path: &Path) -> io::Result<LocalName<'_>> {
    path.to_fs_name::<GenericFilePath>()
}

/// A client never creates/removes the server endpoint. Unix owner-only directory
/// permissions and Windows current-user pipe DACLs belong to engine startup.
/// This transport is not a defense against arbitrary code already running with
/// the owner's full OS account privileges.
pub struct LocalClient {
    stream: Option<LocalStream>,
    timeout: Duration,
    deadline: Option<Instant>,
}

/// Local call phase at failure; this describes transport progress, not whether
/// the Engine executed the request or committed its outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCallPhase {
    BeforeWrite,
    RequestWrite,
    ResponseRead,
    ResponseIdentity,
}

impl LocalClient {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, TransportError> {
        if timeout.is_zero() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "zero RPC timeout").into());
        }
        let stream = connect_local(path.as_ref(), timeout)?;
        stream.set_recv_timeout(Some(timeout))?;
        stream.set_send_timeout(Some(timeout))?;
        Ok(Self {
            stream: Some(stream),
            timeout,
            deadline: None,
        })
    }

    /// Connect and bound all later calls by the same absolute deadline.
    pub fn connect_until(path: impl AsRef<Path>, deadline: Instant) -> Result<Self, TransportError> {
        let stream = connect_local_until(path.as_ref(), deadline)?;
        let timeout = connect_remaining(deadline)?;
        stream.set_recv_timeout(Some(timeout))?;
        stream.set_send_timeout(Some(timeout))?;
        connect_remaining(deadline)?;
        Ok(Self {
            stream: Some(stream),
            timeout,
            deadline: Some(deadline),
        })
    }

    /// Bound subsequent RPCs by one caller-owned absolute deadline. This can
    /// only shorten a prior bound; an expired bound poisons the connection.
    /// Callers covering connection setup must pass its remaining budget to
    /// connect(), then install the same absolute deadline before calling.
    pub fn set_deadline(&mut self, deadline: Instant) -> Result<(), TransportError> {
        let deadline = self
            .deadline
            .map_or(deadline, |previous| previous.min(deadline));
        self.deadline = Some(deadline);
        if deadline <= Instant::now() {
            self.stream = None;
            return Err(
                io::Error::new(io::ErrorKind::TimedOut, "RPC absolute deadline expired").into(),
            );
        }
        Ok(())
    }

    /// One in-flight call. A framing, timeout, or identity failure poisons the
    /// connection; callers must reconnect and use the same request ID rather
    /// than silently replaying an ambiguous mutating operation with a new ID.
    pub fn call(&mut self, request: &Request) -> Result<Response, TransportError> {
        self.call_with_phase(request).map_err(|(_, error)| error)
    }

    /// As [`Self::call`], additionally reporting the local phase of failure.
    /// A write failure may follow a partial write; it never proves non-dispatch.
    pub fn call_with_phase(
        &mut self,
        request: &Request,
    ) -> Result<Response, (LocalCallPhase, TransportError)> {
        request
            .validate()
            .map_err(|error| (LocalCallPhase::BeforeWrite, error.into()))?;
        let mut stream = self.stream.take().ok_or((
            LocalCallPhase::BeforeWrite,
            TransportError::ConnectionPoisoned,
        ))?;
        let deadline = Instant::now().checked_add(self.timeout).ok_or_else(|| {
            (
                LocalCallPhase::BeforeWrite,
                io::Error::new(io::ErrorKind::InvalidInput, "RPC timeout overflow").into(),
            )
        })?;
        let deadline = self
            .deadline
            .map_or(deadline, |absolute| absolute.min(deadline));
        let mut io = DeadlineStream::new(&mut stream, deadline);
        write_message(&mut io, request).map_err(|error| (LocalCallPhase::RequestWrite, error))?;
        let response: Response =
            read_message(&mut io).map_err(|error| (LocalCallPhase::ResponseRead, error))?;
        if response.version != PROTOCOL_VERSION || response.request_id != request.request_id {
            return Err((
                LocalCallPhase::ResponseIdentity,
                TransportError::ResponseMismatch,
            ));
        }
        self.stream = Some(stream);
        Ok(response)
    }
}

/// Recomputes the remaining timeout before every partial I/O operation, so a
/// peer sending one byte at a time cannot reset a whole-message deadline.
pub struct DeadlineStream<'a> {
    stream: &'a mut LocalStream,
    deadline: Instant,
}
impl<'a> DeadlineStream<'a> {
    pub fn new(stream: &'a mut LocalStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::TimedOut, "control message deadline exceeded")
            })
    }
}
impl Read for DeadlineStream<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        #[cfg(target_os = "macos")]
        if let Err(error) = self.stream.set_recv_timeout(Some(remaining)) {
            // Darwin rejects socket options after a Unix peer disconnects, even
            // when response bytes remain buffered. Never fall back to a blocking
            // read: the original deadline must remain an upper bound.
            if error.kind() == io::ErrorKind::InvalidInput {
                self.stream.set_nonblocking(true)?;
                let result = self.remaining().and_then(|_| self.stream.read(bytes));
                self.stream.set_nonblocking(false)?;
                return result;
            }
            return Err(error);
        }
        #[cfg(not(target_os = "macos"))]
        self.stream.set_recv_timeout(Some(remaining))?;
        self.stream.read(bytes)
    }
}
impl Write for DeadlineStream<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.remaining()?;
        #[cfg(target_os = "macos")]
        if let Err(error) = self.stream.set_send_timeout(Some(remaining)) {
            // A disconnected Darwin Unix socket rejects timeout changes. A
            // nonblocking attempt exposes the real write result without
            // extending the original deadline or silently replaying bytes.
            if error.kind() == io::ErrorKind::InvalidInput {
                self.stream.set_nonblocking(true)?;
                let result = self.remaining().and_then(|_| self.stream.write(bytes));
                self.stream.set_nonblocking(false)?;
                return result;
            }
            return Err(error);
        }
        #[cfg(not(target_os = "macos"))]
        self.stream.set_send_timeout(Some(remaining))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.remaining()?;
        self.stream.flush()
    }
}

/// Bound Unix connection setup without detached work or raw descriptor access.
/// Each retry creates a fresh socket and shares one absolute deadline. The
/// dependency uses nonblocking connect/poll for ConnectWaitMode::Timeout.
///
/// Linux AF_UNIX backlog EAGAIN is not proof that a connect is progressing.
/// interprocess 2.4.4 may report writable with no SO_ERROR on an unconnected
/// socket; peer_addr prevents publishing that socket as a usable connection.
/// https://man7.org/linux/man-pages/man2/connect.2.html
/// https://docs.rs/interprocess/2.4.4/interprocess/local_socket/struct.ConnectOptions.html
///
/// Windows timeout mode rejects overloaded local pipes promptly, but cannot
/// bound remote named-pipe network activity. Remote pipes are rejected here;
/// only the local pipe namespace is an engine API endpoint.
pub fn connect_local(path: &Path, timeout: Duration) -> io::Result<LocalStream> {
    if timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero connection timeout",
        ));
    }
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "connection timeout overflow")
    })?;
    connect_local_until(path, deadline)
}

fn connect_local_until(path: &Path, deadline: Instant) -> io::Result<LocalStream> {
    use interprocess::{local_socket::ConnectOptions, ConnectWaitMode};
    #[cfg(windows)]
    validate_local_pipe_endpoint(&path.to_string_lossy())?;
    loop {
        let remaining = connect_remaining(deadline)?;
        let result = ConnectOptions::new()
            .name(endpoint_name(path)?)
            .wait_mode(ConnectWaitMode::Timeout(remaining))
            .connect_sync()
            .and_then(verify_connected);
        match result {
            Ok(stream) => {
                connect_remaining(deadline)?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::NotConnected
                ) => {}
            Err(error) => return Err(error),
        }
        std::thread::sleep(connect_remaining(deadline)?.min(Duration::from_millis(2)));
    }
}

fn connect_remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "local connection deadline exceeded",
            )
        })
}

fn verify_connected(stream: LocalStream) -> io::Result<LocalStream> {
    #[cfg(unix)]
    {
        match &stream {
            LocalStream::UdSocket(unix) => {
                if let Some(error) = unix.take_error()? {
                    return Err(error);
                }
                unix.inner().peer_addr()?;
            }
        }
    }
    Ok(stream)
}

#[cfg(any(windows, test))]
fn validate_local_pipe_endpoint(name: &str) -> io::Result<()> {
    let valid = name.strip_prefix(r"\\.\pipe\").is_some_and(|pipe| {
        !pipe.is_empty()
            && pipe != "."
            && pipe != ".."
            && !pipe
                .chars()
                .any(|c| c.is_control() || c == '\\' || c == '/')
    });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "engine endpoint requires the canonical local named-pipe namespace",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn deadline_stream_reads_buffered_reply_after_peer_closes() {
        use std::os::unix::net::UnixStream;
        let (client, mut server) = UnixStream::pair().unwrap();
        server.write_all(b"reply").unwrap();
        drop(server);
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now() + Duration::from_secs(2));
        let mut response = [0; 5];
        bounded.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"reply");
    }

    #[cfg(unix)]
    #[test]
    fn deadline_stream_empty_closed_peer_returns_eof() {
        use std::os::unix::net::UnixStream;
        let (client, server) = UnixStream::pair().unwrap();
        drop(server);
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now() + Duration::from_secs(2));
        assert_eq!(bounded.read(&mut [0; 1]).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn deadline_stream_expired_deadline_refuses_buffered_reply() {
        use std::os::unix::net::UnixStream;
        let (client, mut server) = UnixStream::pair().unwrap();
        server.write_all(b"reply").unwrap();
        drop(server);
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now());
        let error = bounded.read(&mut [0; 5]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[cfg(unix)]
    #[test]
    fn deadline_stream_truncated_closed_reply_is_eof() {
        use std::os::unix::net::UnixStream;
        let (client, mut server) = UnixStream::pair().unwrap();
        server.write_all(b"rep").unwrap();
        drop(server);
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now() + Duration::from_secs(2));
        let mut response = [0; 5];
        assert_eq!(bounded.read_exact(&mut response).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[cfg(unix)]
    #[test]
    fn deadline_stream_closed_peer_write_reports_broken_pipe() {
        use std::os::unix::net::UnixStream;
        let (client, server) = UnixStream::pair().unwrap();
        drop(server);
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now() + Duration::from_secs(2));
        assert_eq!(bounded.write(b"chunk").unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[cfg(unix)]
    #[test]
    fn deadline_stream_expired_deadline_refuses_write() {
        use std::os::unix::net::UnixStream;
        let (client, mut server) = UnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        let mut stream = LocalStream::UdSocket(client.into());
        let mut bounded = DeadlineStream::new(&mut stream, Instant::now());
        assert_eq!(bounded.write(b"chunk").unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(server.read(&mut [0; 1]).unwrap_err().kind(), io::ErrorKind::WouldBlock);
    }

    #[cfg(unix)]
    #[test]
    fn unix_local_connection_is_bounded_verified_and_bidirectional() {
        use std::os::unix::net::UnixListener;
        struct Directory(std::path::PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(self.0.join("control.sock"));
                let _ = std::fs::remove_dir(&self.0);
            }
        }
        let directory = Directory(
            std::env::temp_dir().join(format!(
                "p{}-{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64
            )),
        );
        std::fs::create_dir(&directory.0).unwrap();
        let path = directory.0.join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut input = [0u8; 1];
            stream.read_exact(&mut input).unwrap();
            assert_eq!(input, [42]);
            stream.write_all(&[43]).unwrap();
        });
        let mut stream = connect_local(&path, Duration::from_secs(2))
            .expect("bounded connect and peer verification");
        stream
            .set_send_timeout(Some(Duration::from_secs(2)))
            .expect("send timeout");
        stream
            .set_recv_timeout(Some(Duration::from_secs(2)))
            .expect("receive timeout");
        stream.write_all(&[42]).expect("client write");
        let mut output = [0u8; 1];
        stream.read_exact(&mut output).expect("client read");
        assert_eq!(output, [43]);
        server.join().unwrap();
    }
    #[test]
    fn windows_endpoint_accepts_only_canonical_local_pipe_names() {
        assert!(validate_local_pipe_endpoint(r"\\.\pipe\pulsar-user-control").is_ok());
        for name in [
            r"\\server\pipe\pulsar",
            r"\\?\pipe\pulsar",
            r"\\?\UNC\server\pipe\pulsar",
            "//server/pipe/pulsar",
            "//./pipe/pulsar",
            r"C:\pipe\pulsar",
            "pulsar",
            r"\\.\pipe\",
            r"\\.\pipe\..",
            r"\\.\pipe\sub\name",
            r"\\.\pipe\sub/name",
            "\\\\.\\pipe\\pulsar\0secret",
        ] {
            assert!(
                validate_local_pipe_endpoint(name).is_err(),
                "accepted {name:?}"
            );
        }
    }
}
