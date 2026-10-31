//! Native experiment for socket timeout reconfiguration after peer close.
//! This does not execute Pulsar's transport or establish application causality.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let (mut client, mut server) = UnixStream::pair()?;
    server.write_all(b"reply")?;
    drop(server);
    let timeout_result = client.set_read_timeout(Some(Duration::from_secs(2)));
    println!("timeout_error_os={:?}", timeout_result.as_ref().err().and_then(|e| e.raw_os_error()));
    let mut response = [0; 5];
    client.read_exact(&mut response)?;
    println!("buffered_response_intact={}", response == *b"reply");
    assert_eq!(&response, b"reply");
    #[cfg(target_os = "macos")]
    assert_eq!(timeout_result.unwrap_err().raw_os_error(), Some(22));
    #[cfg(not(target_os = "macos"))]
    timeout_result?;
    Ok(())
}
